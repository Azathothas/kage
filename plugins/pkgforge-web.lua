-- pkgforge-web.lua - the pkgforge web services as kage tools.
--
-- https://web.pkgforge.dev runs free, keyless HTTP services for agents.
-- This plugin registers one tool per job, each read from the TOOLS
-- table below:
--
--   web_search        search.web.pkgforge.dev   web search, in place of
--                                               the built-in web_search
--   web_read          read.web.pkgforge.dev     a page or a document as
--                                               Markdown
--   paper_search      research.web.pkgforge.dev scholarly works, by query
--                                               or by DOI or arXiv id
--   web_crawl         crawl.web.pkgforge.dev    a site's pages as Markdown
--   web_archive       archive.web.pkgforge.dev  archived copies of a URL
--   web_archive_save  archive.web.pkgforge.dev  a public capture, now
--
-- Copy this file to ~/.config/kage/plugins/ and grant it the network:
--
--   [plugins.capabilities]
--   pkgforge-web = ["net"]
--
-- `disable` in [plugins.config.pkgforge-web] lists tools to leave out,
-- for example ["web_search"] to keep kage's own web_search, or
-- ["web_archive_save"] so the model cannot make public captures.
--
-- The services choose their upstream providers themselves and answer
-- in one shape, so nothing here names a provider: a provider added or
-- dropped upstream needs no change. Each answer says which provider
-- served it and whether it came from a cache. An answer in a shape this
-- file does not know is passed on as it came, cut short, rather than
-- failing the call, and a service that is down fails only its own tools.
--
-- /pkgforge-web-check compares every route and parameter here with
-- each service's openapi.json and lists what drifted.

local caps = kage.request_capabilities({ "net" })
if not caps.net then
  kage.notify("pkgforge-web: disabled (grant net in [plugins.capabilities])")
  return
end

local USER_AGENT = "kage-pkgforge-web (+https://github.com/QaidVoid/kage)"

-- The most bytes one answer hands the model: under the 100 kB a tool
-- result is cut at, with room for the notes around it.
local PAGE_BYTES = 60000

-- What a crawled page keeps of its Markdown. Ten pages to an answer
-- stay under PAGE_BYTES.
local CRAWL_PAGE_BYTES = 5500

-- The body a route with a large answer may read, past kage.http's 2 MB
-- default: a page's Markdown, an archived capture, ten crawled pages.
local LARGE_BYTES = 16000000

-- How often a crawl or a save is polled. A running crawl's state is
-- kept for 5 s by the service, so asking sooner learns nothing new.
local POLL_MS = 5000

local SEARCH = "search.web.pkgforge.dev"
local READ = "read.web.pkgforge.dev"
local RESEARCH = "research.web.pkgforge.dev"
local CRAWL = "crawl.web.pkgforge.dev"
local ARCHIVE = "archive.web.pkgforge.dev"

-- One entry per tool. `input` is the tool's schema, in order, and
-- `routes` say where each input goes: `query` and `body` map a service
-- parameter to an input, and `{name}` in `path` takes the input of that
-- name. The first route whose `when` holds is called: a string names an
-- input that must be set, a table inputs and their values. `show` says
-- how an answer reads: `list` is where its items are, and each item
-- shows its `title`, `url`, `facts` and `text`; `provenance` names the
-- fields that say how it was served.
local TOOLS = {
  {
    name = "web_search",
    replaces = true,
    description = "Search the web through search.web.pkgforge.dev. Returns up to "
      .. "`count` results, each with its title, URL and a snippet, sorted by "
      .. "relevance. Use web_read to read a result's page.",
    input = {
      {
        name = "query",
        type = "string",
        required = true,
        doc = "What to search for, at most 400 characters.",
      },
      {
        name = "count",
        type = "integer",
        min = 1,
        max = 20,
        doc = "How many results to return. Defaults to 10, at most 20.",
      },
      {
        name = "freshness",
        type = "string",
        enum = { "day", "week", "month", "year" },
        doc = "Only results from the last day, week, month or year.",
      },
      {
        name = "include_domains",
        type = "string",
        doc = "Only results from these hosts, comma-separated.",
      },
      {
        name = "exclude_domains",
        type = "string",
        doc = "No results from these hosts, comma-separated.",
      },
    },
    routes = {
      {
        host = SEARCH,
        method = "GET",
        path = "/search",
        query = {
          { "q", "query" },
          { "count", "count" },
          { "freshness", "freshness" },
          { "include_domains", "include_domains" },
          { "exclude_domains", "exclude_domains" },
        },
        show = {
          list = "results",
          title = "title",
          url = "url",
          text = "snippet",
          text_max = 500,
          facts = { "published" },
          empty = "No results.",
          provenance = { "provider", "cached", "rank" },
        },
      },
    },
  },
  {
    name = "web_read",
    description = "Read a web page, a PDF or an office document as Markdown through "
      .. "read.web.pkgforge.dev. Pages that need a browser or sit behind a bot "
      .. "challenge are read by a provider. The answer starts with the page's "
      .. "facts (title, final URL, the origin's status). A long page comes in "
      .. "pieces: the answer says which `offset` reads the rest.",
    input = {
      { name = "url", type = "string", required = true, doc = "The http(s) URL to read." },
      {
        name = "offset",
        type = "integer",
        min = 0,
        doc = "Where to start in the Markdown, in bytes, to read the rest of a long page.",
      },
      {
        name = "provider",
        type = "string",
        doc = "Have this provider read the page, as an earlier answer names one, "
          .. "when the origin refused or the first read came back empty.",
      },
    },
    routes = {
      {
        host = READ,
        method = "GET",
        path = "/",
        accept = "text/markdown",
        timeout = 40,
        max_bytes = LARGE_BYTES,
        query = { { "url", "url" }, { "provider", "provider" } },
        show = {
          body = true,
          provenance_headers = { "x-reader-source", "x-reader-final-url", "x-web-cache" },
        },
      },
    },
  },
  {
    name = "paper_search",
    description = "Find scholarly works through research.web.pkgforge.dev. Search by "
      .. "`query`, or look up one work by `id`: a DOI, arXiv id, PMID, PMCID or "
      .. "OpenAlex id (`doi:10.7717/peerj.4375`, `arxiv:1706.03762`, or a doi.org "
      .. "or arxiv.org URL). Each work has its authors, year, venue, DOI, citation "
      .. "count, an open-access link and its abstract. Give one of query or id.",
    input = {
      {
        name = "query",
        type = "string",
        doc = "What to search for; quoted phrases and AND, OR, NOT work.",
      },
      { name = "id", type = "string", doc = "One work's id, instead of a query." },
      {
        name = "count",
        type = "integer",
        min = 1,
        max = 25,
        doc = "How many works a search returns. Defaults to 10, at most 25.",
      },
      {
        name = "from",
        type = "string",
        doc = "Published on or after this year or date, YYYY or YYYY-MM-DD.",
      },
      {
        name = "to",
        type = "string",
        doc = "Published on or before this year or date, YYYY or YYYY-MM-DD.",
      },
      {
        name = "type",
        type = "string",
        doc = "Only this kind of work, such as article, preprint, review or dataset.",
      },
      {
        name = "open_access",
        type = "boolean",
        doc = "true for open-access works only, false for closed ones only.",
      },
    },
    one_of = { "query", "id" },
    routes = {
      {
        when = "id",
        host = RESEARCH,
        method = "GET",
        path = "/works/{id}",
        show = {
          list = "works",
          title = "title",
          text = "abstract",
          text_max = 3000,
          facts = { "year", "authors", "venue", "type", "doi", "ids", "cited_by", "oa_url" },
          empty = "No such work.",
          provenance = { "provider", "cached", "arxiv_check.status" },
        },
      },
      {
        host = RESEARCH,
        method = "GET",
        path = "/works",
        query = {
          { "q", "query" },
          { "count", "count" },
          { "from", "from" },
          { "to", "to" },
          { "type", "type" },
          { "oa", "open_access" },
        },
        show = {
          list = "works",
          title = "title",
          text = "abstract",
          text_max = 600,
          facts = { "year", "authors", "venue", "type", "doi", "cited_by", "oa_url" },
          empty = "No works matched.",
          provenance = { "provider", "cached", "rank", "merged" },
        },
      },
    },
  },
  {
    name = "web_crawl",
    job = "crawl",
    description = "Crawl a site from a start `url` through crawl.web.pkgforge.dev and "
      .. "get its pages as Markdown. A crawl is a job: this call waits up to "
      .. "`wait_secs` for it to end, and returns its `job_id` when it has not, "
      .. "for a later call to wait again. An ended job answers 10 pages at a "
      .. "time; `cursor` with the `job_id` reads the next ones. For one page, "
      .. "use web_read.",
    input = {
      { name = "url", type = "string", doc = "The start URL, to submit a new crawl." },
      {
        name = "job_id",
        type = "string",
        doc = "A crawl an earlier call submitted, to wait for it or read its pages.",
      },
      {
        name = "cursor",
        type = "string",
        doc = "With job_id: the cursor an earlier answer named, for the next pages.",
      },
      {
        name = "limit",
        type = "integer",
        min = 1,
        max = 100,
        doc = "The most pages to fetch. Defaults to 10, at most 100.",
      },
      {
        name = "depth",
        type = "integer",
        min = 1,
        max = 10,
        doc = "How many links away from the start URL to go, 1 to 10.",
      },
      {
        name = "include_paths",
        type = "array",
        doc = "Path globs a page must match one of, such as /docs/**; "
          .. "`*` stays within one path segment.",
      },
      { name = "exclude_paths", type = "array", doc = "Path globs no page may match." },
      {
        name = "main_content",
        type = "boolean",
        doc = "true to keep each page's main content without its navigation.",
      },
      { name = "render", type = "boolean", doc = "true to render pages in a browser first." },
      {
        name = "wait_secs",
        type = "integer",
        min = 0,
        max = 120,
        doc = "How long to wait for the job to end. Defaults to 30, at most 120.",
      },
    },
    one_of = { "url", "job_id" },
    routes = {
      {
        name = "submit",
        host = CRAWL,
        method = "POST",
        path = "/crawl",
        body = {
          { "url", "url" },
          { "limit", "limit" },
          { "depth", "depth" },
          { "include_paths", "include_paths" },
          { "exclude_paths", "exclude_paths" },
          { "main_content", "main_content" },
          { "render", "render" },
        },
      },
      {
        name = "poll",
        host = CRAWL,
        method = "GET",
        path = "/crawl/{job_id}",
        timeout = 40,
        max_bytes = LARGE_BYTES,
        query = { { "cursor", "cursor" } },
      },
    },
  },
  {
    name = "web_archive",
    description = "Look up archived copies of a URL in public web archives through "
      .. "archive.web.pkgforge.dev. `captures` lists them, oldest first; "
      .. "`closest` finds the one nearest the instant `at`; `capture` returns "
      .. "one capture's archived bytes, `at` being its 14-digit timestamp. For "
      .. "an archived page as Markdown, give a capture's `replay` URL to web_read.",
    input = {
      {
        name = "action",
        type = "string",
        enum = { "captures", "closest", "capture" },
        doc = "What to look up. Defaults to captures.",
      },
      {
        name = "url",
        type = "string",
        required = true,
        doc = "The URL whose archived copies to look up.",
      },
      {
        name = "at",
        type = "string",
        doc = "YYYYMMDDhhmmss, or a prefix such as 2019: the instant for closest, "
          .. "and for capture the capture's full 14-digit timestamp.",
      },
      {
        name = "from",
        type = "string",
        doc = "For captures: the earliest, 1 to 14 digits of YYYYMMDDhhmmss.",
      },
      {
        name = "to",
        type = "string",
        doc = "For captures: the latest, 1 to 14 digits of YYYYMMDDhhmmss.",
      },
      {
        name = "limit",
        type = "integer",
        min = 1,
        max = 100,
        doc = "For captures: how many to list. Defaults to 50, at most 100.",
      },
      {
        name = "cursor",
        type = "string",
        doc = "For captures: the cursor an earlier answer named, for the next page.",
      },
      {
        name = "provider",
        type = "string",
        doc = "The archive to ask, as an earlier answer names it; a cursor needs the archive that "
          .. "wrote it.",
      },
      {
        name = "offset",
        type = "integer",
        min = 0,
        doc = "For capture: where to start in the archived body, in bytes.",
      },
    },
    defaults = { action = "captures", limit = 50 },
    routes = {
      {
        when = { action = "captures" },
        host = ARCHIVE,
        method = "GET",
        path = "/captures",
        query = {
          { "url", "url" },
          { "from", "from" },
          { "to", "to" },
          { "limit", "limit" },
          { "cursor", "cursor" },
          { "provider", "provider" },
        },
        show = {
          list = "captures",
          title = "timestamp",
          url = "original_url",
          facts = { "status", "mimetype", "replay" },
          empty = "No captures.",
          provenance = { "provider", "cached" },
          more = {
            when = "more",
            args = { cursor = "cursor", from = "next_from", provider = "provider" },
          },
        },
      },
      {
        when = { action = "closest" },
        host = ARCHIVE,
        method = "GET",
        path = "/closest",
        query = { { "url", "url" }, { "at", "at" }, { "provider", "provider" } },
        show = {
          list = "closest",
          title = "timestamp",
          url = "original_url",
          facts = { "status", "mimetype", "replay" },
          provenance = { "provider", "cached" },
        },
      },
      {
        when = { action = "capture" },
        host = ARCHIVE,
        method = "GET",
        path = "/capture/{rest}",
        accept = "*/*",
        timeout = 40,
        max_bytes = LARGE_BYTES,
        rest = { "at", "url" },
        query = { { "provider", "provider" } },
        show = {
          body = true,
          provenance_headers = {
            "x-web-capture-timestamp",
            "x-web-original-url",
            "x-web-origin-status",
            "x-web-provider",
            "content-type",
          },
        },
      },
    },
  },
  {
    name = "web_archive_save",
    job = "save",
    risk = "write",
    description = "Have a page archived now, through archive.web.pkgforge.dev. The "
      .. "capture is public, and each URL has a small daily allowance of saves. "
      .. "This call waits up to `wait_secs` for the capture and returns its "
      .. "`job` when it is not done, for a later call to wait again.",
    input = {
      { name = "url", type = "string", doc = "The http(s) URL to archive." },
      {
        name = "job",
        type = "string",
        doc = "A save an earlier call started, to wait for it again.",
      },
      {
        name = "wait_secs",
        type = "integer",
        min = 0,
        max = 120,
        doc = "How long to wait for the capture. Defaults to 30, at most 120.",
      },
    },
    one_of = { "url", "job" },
    routes = {
      {
        name = "submit",
        host = ARCHIVE,
        method = "POST",
        path = "/save",
        query = { { "url",
        "url" } },
      },
      { name = "poll", host = ARCHIVE, method = "GET", path = "/save/{job}" },
    },
  },
}

-- Helpers ---------------------------------------------------------------

local function trim(text)
  return (text:gsub("^%s+", ""):gsub("%s+$", ""))
end

-- `text` cut to at most `max` bytes without splitting a UTF-8 sequence.
local function cut(text, max)
  if #text <= max then
    return text
  end
  local stop = max
  while stop > 0 and (text:byte(stop + 1) & 0xC0) == 0x80 do
    stop = stop - 1
  end
  return text:sub(1, stop)
end

-- One line of at most `max` bytes, "..." marking a cut.
local function line(text, max)
  local flat = trim(text:gsub("%s+", " "))
  if #flat <= max then
    return flat
  end
  return cut(flat, max) .. "..."
end

local function encode(value)
  return (value:gsub("[^%w%-._~]", function(c)
    return string.format("%%%02X", c:byte())
  end))
end

-- A query value: a list goes comma-separated.
local function as_param(value)
  if type(value) ~= "table" then
    return tostring(value)
  end
  local items = {}
  for _, item in ipairs(value) do
    items[#items + 1] = tostring(item)
  end
  return table.concat(items, ",")
end

local function decode(body)
  local ok, value = pcall(kage.json.decode, body)
  if ok and type(value) == "table" then
    return value
  end
  return nil
end

-- The value at a dotted `path` in `doc`.
local function field(doc, path)
  local value = doc
  for key in path:gmatch("[^.]+") do
    if type(value) ~= "table" then
      return nil
    end
    value = value[key]
  end
  return value
end

-- A value from an answer as short text: a list shows its first few
-- items, a record its fields in name order.
local function show(value)
  if type(value) == "table" then
    local parts = {}
    if value[1] ~= nil then
      for i, item in ipairs(value) do
        if i > 6 then
          parts[#parts + 1] = string.format("and %d more", #value - 6)
          break
        end
        parts[#parts + 1] = show(item)
      end
    else
      local keys = {}
      for key, item in pairs(value) do
        if type(item) ~= "table" then
          keys[#keys + 1] = key
        end
      end
      table.sort(keys)
      for _, key in ipairs(keys) do
        parts[#parts + 1] = key .. " " .. show(value[key])
      end
    end
    return table.concat(parts, ", ")
  end
  if math.type(value) == "float" and math.tointeger(value) then
    return tostring(math.tointeger(value))
  end
  return tostring(value)
end

-- "host: name value, name value" from the fields of `doc` that are set.
local function provenance(host, doc, names)
  local parts = {}
  for _, name in ipairs(names or {}) do
    local value = field(doc, name)
    if value ~= nil and type(value) ~= "table" then
      parts[#parts + 1] = name .. " " .. show(value)
    end
  end
  if #parts == 0 then
    return nil
  end
  return host .. ": " .. table.concat(parts, ", ")
end

-- Inputs ----------------------------------------------------------------

-- `value` as the type `spec` declares, or nil and why not. Numbers are
-- clamped to the declared range, and an empty string is no value.
local function coerce(spec, value)
  if value == nil then
    return nil
  end
  if spec.type == "integer" then
    local n = math.tointeger(tonumber(value))
    if n == nil then
      return nil, spec.name .. " must be a whole number"
    end
    if spec.min and n < spec.min then
      n = spec.min
    end
    if spec.max and n > spec.max then
      n = spec.max
    end
    return n
  end
  if spec.type == "boolean" then
    if value == true or value == "true" then
      return true
    end
    if value == false or value == "false" then
      return false
    end
    return nil, spec.name .. " must be true or false"
  end
  if spec.type == "array" then
    if type(value) == "string" then
      local items = {}
      for item in value:gmatch("[^,]+") do
        items[#items + 1] = trim(item)
      end
      value = items
    end
    if type(value) ~= "table" then
      return nil, spec.name .. " must be a list of strings"
    end
    return #value > 0 and value or nil
  end
  if type(value) == "table" then
    value = as_param(value)
  end
  value = trim(show(value))
  if value == "" then
    return nil
  end
  if spec.enum then
    for _, allowed in ipairs(spec.enum) do
      if value == allowed then
        return value
      end
    end
    return nil, spec.name .. " must be one of " .. table.concat(spec.enum, ", ")
  end
  return value
end

-- The tool's input, typed and checked, or nil and what is wrong.
local function read_input(tool, raw)
  local args = {}
  for _, spec in ipairs(tool.input) do
    local value, err = coerce(spec, raw[spec.name])
    if err then
      return nil, err
    end
    if value == nil and tool.defaults then
      value = tool.defaults[spec.name]
    end
    if value == nil and spec.required then
      return nil, spec.name .. " is required"
    end
    args[spec.name] = value
  end
  if tool.one_of then
    local set = {}
    for _, name in ipairs(tool.one_of) do
      if args[name] ~= nil then
        set[#set + 1] = name
      end
    end
    if #set ~= 1 then
      return nil, "give exactly one of " .. table.concat(tool.one_of, " or ")
    end
  end
  return args
end

local function holds(route, args)
  local when = route.when
  if when == nil then
    return true
  end
  if type(when) == "string" then
    return args[when] ~= nil
  end
  for name, value in pairs(when) do
    if args[name] ~= value then
      return false
    end
  end
  return true
end

local function route_named(tool, name)
  for _, route in ipairs(tool.routes) do
    if route.name == name then
      return route
    end
  end
end

-- Requests --------------------------------------------------------------

-- The URL and JSON body that call `route` with `args`.
local function request_for(route, args)
  local missing
  local path = route.path:gsub("{([%w_]+)}", function(name)
    local value = args[name]
    if name == "rest" and route.rest then
      local at, url = args[route.rest[1]], args[route.rest[2]]
      value = at and url and (at .. "/" .. url)
    end
    if value == nil then
      missing = missing or name
      return ""
    end
    return encode(as_param(value))
  end)
  if missing then
    local name = missing == "rest" and route.rest[1] or missing
    return nil, nil, string.format("%s: %s is required here", route.tool, name)
  end
  local query = {}
  for _, pair in ipairs(route.query or {}) do
    local value = args[pair[2]]
    if value ~= nil then
      query[#query + 1] = pair[1] .. "=" .. encode(as_param(value))
    end
  end
  local url = "https://" .. route.host .. path
  if #query > 0 then
    url = url .. "?" .. table.concat(query, "&")
  end
  local body
  if route.body then
    body = {}
    for _, pair in ipairs(route.body) do
      body[pair[1]] = args[pair[2]]
    end
  end
  return url, body
end

local function provider_rows(rows)
  if type(rows) ~= "table" or rows[1] == nil then
    return nil
  end
  local parts = {}
  for i, row in ipairs(rows) do
    if i > 8 then
      break
    end
    local why = row.reason or row.kind
    if why == nil and type(row.attempts) == "table" then
      local kinds = {}
      for _, attempt in ipairs(row.attempts) do
        kinds[#kinds + 1] = tostring(attempt.kind)
      end
      why = table.concat(kinds, ", ")
    end
    parts[#parts + 1] = tostring(row.provider) .. " " .. tostring(why)
  end
  return table.concat(parts, "; ")
end

-- The text of an error result for an answer that is not a 2xx.
local function refusal(route, res)
  local host = route.host
  local problem = (res.content_type or ""):find("json") and decode(res.body)
  if problem and problem.reason then
    local lines = {
      string.format(
        "%s answered %d %s: %s",
        host,
        res.status,
        tostring(problem.reason),
        tostring(problem.title)
      ),
    }
    if problem.detail then
      lines[#lines + 1] = tostring(problem.detail)
    end
    local retry = res.headers and res.headers["retry-after"]
    if retry then
      lines[#lines + 1] = "Retry after " .. retry .. " s."
    end
    local rows = provider_rows(problem.errors) or provider_rows(problem.misses)
    if rows then
      lines[#lines + 1] = "Providers asked: " .. rows .. "."
    end
    if problem.see then
      lines[#lines + 1] = "See " .. tostring(problem.see)
    end
    return table.concat(lines, "\n")
  end
  local text = string.format("%s answered %d", host, res.status)
  if res.status == 404 or res.status == 410 then
    text = text
      .. " without a problem body: the service may have moved or been removed. "
      .. "If this keeps happening, leave its tools out with `disable` in "
      .. "[plugins.config.pkgforge-web]."
  end
  local body = line(res.body or "", 500)
  if body ~= "" then
    text = text .. "\n" .. body
  end
  return text
end

-- Call `route` with `args`. Returns the answer, or nil and the text of
-- an error result.
local function call(route, args)
  local url, body, err = request_for(route, args)
  if url == nil then
    return nil, err
  end
  local opts = {
    headers = { ["user-agent"] = USER_AGENT, accept = route.accept or "application/json" },
    timeout_secs = route.timeout or 30,
    max_bytes = route.max_bytes,
  }
  local send = kage.http.get
  if route.method == "POST" then
    send = kage.http.post
    opts.json = body
  end
  local ok, res = pcall(send, url, opts)
  if not ok then
    return nil,
      string.format(
        "%s could not be reached: %s\nIf this keeps happening the service may be "
          .. "gone; leave its tools out with `disable` in [plugins.config.pkgforge-web].",
        route.host,
        tostring(res)
      )
  end
  if res.status < 200 or res.status > 299 then
    return nil, refusal(route, res), res
  end
  return res
end

-- Answers ---------------------------------------------------------------

-- A body too long for one answer, from byte `offset` on, with a note
-- naming the offset that reads on.
local function page(body, offset, tool, note)
  offset = offset or 0
  if offset > 0 then
    if offset >= #body then
      return string.format("offset %d is past the end: the body is %d bytes.", offset, #body)
    end
    while offset < #body and (body:byte(offset + 1) & 0xC0) == 0x80 do
      offset = offset + 1
    end
  end
  local piece = cut(body:sub(offset + 1), PAGE_BYTES)
  local stop = offset + #piece
  local text = piece
  if stop < #body then
    text = text
      .. string.format(
        "\n\n[%s: bytes %d to %d of %d; call %s again with offset = %d for the rest]",
        note,
        offset,
        stop,
        #body,
        tool,
        stop
      )
  end
  return text
end

-- An answer whose items are listed, as `show` describes.
local function list_text(tool, route, res)
  local spec = route.show
  local doc = decode(res.body)
  local items = doc and field(doc, spec.list)
  if type(items) ~= "table" then
    return {
      text = string.format(
        "%s answered without `%s`, so here is its answer as it came, up to 4000 bytes:\n%s",
        route.host,
        spec.list,
        cut(res.body, 4000)
      ),
      structured = doc,
    }
  end
  if next(items) ~= nil and items[1] == nil then
    items = { items }
  end
  local out = {}
  if #items == 0 then
    out[#out + 1] = spec.empty or "Nothing found."
  end
  for i, item in ipairs(items) do
    if type(item) == "table" then
      local title = line(show(field(item, spec.title) or "(untitled)"), 300)
      out[#out + 1] = string.format("%d. %s", i, title)
      local url = spec.url and field(item, spec.url)
      if url ~= nil then
        out[#out + 1] = "   " .. show(url)
      end
      local facts = {}
      for _, name in ipairs(spec.facts or {}) do
        local value = field(item, name)
        if value ~= nil and show(value) ~= "" then
          facts[#facts + 1] = name .. ": " .. show(value)
        end
      end
      if #facts > 0 then
        out[#out + 1] = "   " .. line(table.concat(facts, "; "), 1000)
      end
      local text = spec.text and field(item, spec.text)
      if type(text) == "string" and text ~= "" then
        out[#out + 1] = "   " .. line(text, spec.text_max or 500)
      end
    end
  end
  local misses = provider_rows(doc.misses)
  if misses then
    out[#out + 1] = "Not held by: " .. misses .. "."
  end
  local more = spec.more
  if more and field(doc, more.when) then
    local named = {}
    for input, name in pairs(more.args) do
      local value = field(doc, name)
      if value ~= nil then
        named[#named + 1] = string.format("%s = %q", input, show(value))
      end
    end
    table.sort(named)
    out[#out + 1] = string.format(
      "More: call %s again with the same arguments and %s.",
      tool.name,
      table.concat(named, ", ")
    )
  end
  out[#out + 1] = provenance(route.host, doc, spec.provenance)
  return { text = table.concat(out, "\n"), structured = doc }
end

-- An answer whose body is the content itself: a read page, an archived
-- capture.
local function body_text(tool, route, res, args)
  local text = page(res.body, args.offset, tool.name, route.host)
  if res.truncated then
    text = text .. string.format("\n\n[%s: the body was cut at %d bytes]", route.host, #res.body)
  end
  local notes = {}
  for _, name in ipairs(route.show.provenance_headers or {}) do
    local value = res.headers and res.headers[name]
    if value then
      notes[#notes + 1] = name .. " " .. value
    end
  end
  -- A read with front matter states its own facts first.
  if #notes > 0 and not res.body:find("^%-%-%-\n") then
    text = route.host .. ": " .. table.concat(notes, ", ") .. "\n\n" .. text
  end
  return { text = text }
end

-- Jobs ------------------------------------------------------------------

-- Wait `ms` in the slices kage.sleep_ms allows. A cancelled call raises
-- out of the sleep, which ends the poll.
local function nap(ms)
  while ms > 0 do
    local slice = math.min(ms, 500)
    kage.sleep_ms(slice)
    ms = ms - slice
  end
end

-- Poll `route` with `args` until `done(answer)` or `wait_secs` pass.
-- Returns the last answer, or nil and the text of an error result.
local function poll(route, args, wait_secs, done)
  local deadline = kage.now_ms() + wait_secs * 1000
  while true do
    local res, err = call(route, args)
    if res == nil then
      return nil, err
    end
    local doc = decode(res.body)
    if doc == nil or done(doc) or kage.now_ms() + POLL_MS > deadline then
      return res, nil, doc
    end
    nap(POLL_MS)
  end
end

local function crawl_text(route, res, doc, job_id)
  if doc == nil then
    local text = route.host .. " answered a poll in a shape this tool does not read:\n"
    return { text = text .. cut(res.body, 4000) }
  end
  local out = {}
  local headers = res.headers or {}
  local source = {}
  for _, name in ipairs({ "provider", "status", "reason" }) do
    if doc[name] ~= nil then
      source[#source + 1] = name .. " " .. show(doc[name])
    end
  end
  if headers["x-web-crawl-content"] then
    source[#source + 1] = "content " .. headers["x-web-crawl-content"]
  end
  if headers["x-web-cache"] then
    source[#source + 1] = "cache " .. headers["x-web-cache"]
  end
  if doc.status == "running" then
    out[#out + 1] = string.format("Crawl %s is still running.", job_id)
    if type(doc.progress) == "table" and next(doc.progress) then
      out[#out + 1] = "Progress: " .. show(doc.progress) .. "."
    end
    out[#out + 1] = string.format("Call web_crawl with job_id = %q to wait for it again.", job_id)
    out[#out + 1] = route.host .. ": " .. table.concat(source, ", ")
    return { text = table.concat(out, "\n"), structured = doc }
  end
  local pages = type(doc.pages) == "table" and doc.pages or {}
  out[#out + 1] = string.format(
    "Crawl %s from %s: %d %s in this answer.",
    job_id,
    show(doc.url),
    #pages,
    #pages == 1 and "page" or "pages"
  )
  for _, p in ipairs(pages) do
    out[#out + 1] = ""
    out[#out + 1] = "## " .. line(show(p.title or p.url), 200)
    out[#out + 1] = string.format("%s (status %s)", show(p.url), show(p.status))
    if type(p.markdown) == "string" then
      local markdown = cut(p.markdown, CRAWL_PAGE_BYTES)
      out[#out + 1] = ""
      out[#out + 1] = markdown
      if #markdown < #p.markdown then
        out[#out + 1] = string.format(
          "[cut at %d of %d bytes; web_read the URL for all of it]",
          #markdown,
          #p.markdown
        )
      end
    else
      out[#out + 1] = "(its content is not read yet; poll again for it)"
    end
  end
  out[#out + 1] = ""
  if type(doc.dropped) == "table" and next(doc.dropped) then
    out[#out + 1] = "Dropped: " .. show(doc.dropped) .. "."
  end
  if doc.next_cursor then
    out[#out + 1] = string.format(
      "More pages: call web_crawl with job_id = %q and cursor = %q.",
      job_id,
      show(doc.next_cursor)
    )
  end
  out[#out + 1] = route.host .. ": " .. table.concat(source, ", ")
  return { text = table.concat(out, "\n"), structured = doc }
end

local function run_crawl(tool, args)
  local poll_route = route_named(tool, "poll")
  local job_id = args.job_id
  if job_id == nil then
    local res, err = call(route_named(tool, "submit"), args)
    if res == nil then
      return { is_error = true, text = err }
    end
    local doc = decode(res.body)
    job_id = doc and doc.job_id
    if job_id == nil then
      local text = CRAWL .. " accepted the crawl without a job_id:\n" .. cut(res.body, 2000)
      return { is_error = true, text = text }
    end
  end
  local wait = args.wait_secs or 30
  local target = { job_id = job_id, cursor = args.cursor }
  local res, err, doc = poll(poll_route, target, wait, function(state)
    return state.status ~= "running"
  end)
  if res == nil then
    return { is_error = true, text = err .. string.format("\n(crawl job_id %q)", job_id) }
  end
  return crawl_text(poll_route, res, doc, job_id)
end

local function save_text(route, doc, job)
  local out = {}
  if doc.status == "success" then
    out[#out + 1] = string.format("Archived %s at %s.", show(doc.url), show(doc.timestamp))
    if doc.capture then
      out[#out + 1] = "Capture: " .. show(doc.capture)
    end
  elseif doc.status == "error" then
    local why = show(doc.reason or doc.error)
    out[#out + 1] = string.format("The save of %s failed: %s.", show(doc.url), why)
  else
    out[#out + 1] = string.format("The save of %s is still %s.", show(doc.url), show(doc.status))
    out[#out + 1] = string.format("Call web_archive_save with job = %q to wait for it again.", job)
  end
  out[#out + 1] = provenance(route.host, doc, { "provider", "submitted_at" })
  return { is_error = doc.status == "error", text = table.concat(out, "\n"), structured = doc }
end

local function run_save(tool, args)
  local poll_route = route_named(tool, "poll")
  local job = args.job
  if job == nil then
    local res, err = call(route_named(tool, "submit"), args)
    if res == nil then
      return { is_error = true, text = err }
    end
    local doc = decode(res.body)
    job = doc and doc.job
    if job == nil then
      local text = ARCHIVE .. " accepted the save without a job:\n" .. cut(res.body, 2000)
      return { is_error = true, text = text }
    end
  end
  local res, err, doc = poll(poll_route, { job = job }, args.wait_secs or 30, function(state)
    return state.status ~= "pending"
  end)
  if res == nil then
    return { is_error = true, text = err .. string.format("\n(save job %q)", job) }
  end
  if doc == nil then
    local text = poll_route.host .. " answered in a shape this tool does not read:\n"
    return { text = text .. cut(res.body, 4000) }
  end
  return save_text(poll_route, doc, job)
end

local JOBS = { crawl = run_crawl, save = run_save }

-- Registration ----------------------------------------------------------

local function run(tool, raw)
  local args, err = read_input(tool, raw or {})
  if args == nil then
    return { is_error = true, text = tool.name .. ": " .. err }
  end
  if tool.job then
    return JOBS[tool.job](tool, args)
  end
  for _, route in ipairs(tool.routes) do
    if holds(route, args) then
      local res, failure, refused = call(route, args)
      -- A read of an origin's own 404 page is that page, not a problem.
      if refused and route.show.body and (refused.content_type or ""):find("^text/") then
        local out = body_text(tool, route, refused, args)
        out.is_error = true
        local head = string.format("%s answered %d with this page:", route.host, refused.status)
        out.text = head .. "\n\n" .. out.text
        return out
      end
      if res == nil then
        return { is_error = true, text = failure }
      end
      if route.show.body then
        return body_text(tool, route, res, args)
      end
      return list_text(tool, route, res)
    end
  end
  return { is_error = true, text = tool.name .. ": no route takes these arguments" }
end

local function schema(tool)
  local properties, required = {}, {}
  for _, spec in ipairs(tool.input) do
    local property = { type = spec.type, description = spec.doc }
    if spec.enum then
      property.enum = spec.enum
    end
    if spec.type == "array" then
      property.items = { type = "string" }
    end
    if spec.min then
      property.minimum = spec.min
    end
    if spec.max then
      property.maximum = spec.max
    end
    properties[spec.name] = property
    if spec.required then
      required[#required + 1] = spec.name
    end
  end
  local out = { type = "object", properties = properties }
  if #required > 0 then
    out.required = required
  end
  return out
end

local disabled = {}
local config = kage.plugin_config()
for _, name in ipairs(type(config.disable) == "table" and config.disable or {}) do
  disabled[name] = true
end

for _, tool in ipairs(TOOLS) do
  for _, route in ipairs(tool.routes) do
    route.tool = tool.name
  end
  if not disabled[tool.name] then
    local register = tool.replaces and kage.override_tool or kage.register_tool
    register({
      name = tool.name,
      description = tool.description,
      schema = schema(tool),
      risk = tool.risk or "network",
      execute = function(input)
        return run(tool, input)
      end,
    })
  end
end

-- Drift -----------------------------------------------------------------

-- The schema of the parameter or body property `name` that `operation`
-- (or its path item) declares, $refs followed through `spec`.
local function declared(spec, item, operation, place, name)
  local function resolve(node)
    while type(node) == "table" and node["$ref"] do
      local target = spec
      for key in node["$ref"]:gsub("^#/", ""):gmatch("[^/]+") do
        target = type(target) == "table" and target[key:gsub("~1", "/"):gsub("~0", "~")] or nil
      end
      node = target
    end
    return node
  end
  if place == "body" then
    local body = resolve(operation.requestBody)
    local schema_node = body and resolve(field(body, "content.application/json.schema"))
    local properties = schema_node and schema_node.properties
    return properties and resolve(properties[name])
  end
  for _, list in ipairs({ operation.parameters or {}, item.parameters or {} }) do
    for _, parameter in ipairs(list) do
      parameter = resolve(parameter)
      if type(parameter) == "table" and parameter.name == name and parameter["in"] == place then
        return resolve(parameter.schema) or {}
      end
    end
  end
end

-- Every way the table disagrees with the services' openapi.json, one
-- line each, fetching each service's document once.
local function drift()
  local specs, problems, checked = {}, {}, 0
  local function spec_of(host)
    if specs[host] == nil then
      local ok, res = pcall(kage.http.get, "https://" .. host .. "/openapi.json", {
        headers = { ["user-agent"] = USER_AGENT, accept = "application/json" },
        timeout_secs = 30,
      })
      specs[host] = ok and res.status == 200 and decode(res.body) or false
      if not specs[host] then
        local why = ok and tostring(res.status) or tostring(res)
        problems[#problems + 1] = host .. ": no openapi.json (" .. why .. ")"
      end
    end
    return specs[host]
  end
  for _, tool in ipairs(TOOLS) do
    local inputs = {}
    for _, spec in ipairs(tool.input) do
      inputs[spec.name] = spec
    end
    for _, route in ipairs(tool.routes) do
      local spec = spec_of(route.host)
      if spec then
        local where = string.format("%s: %s %s %s", tool.name, route.host, route.method, route.path)
        local item = (spec.paths or {})[route.path] or {}
        local operation = item[route.method:lower()]
        if operation == nil then
          problems[#problems + 1] = where .. ": no such route"
        else
          checked = checked + 1
          local wanted = {}
          for name in route.path:gmatch("{([%w_]+)}") do
            wanted[#wanted + 1] = { "path", name }
          end
          for _, pair in ipairs(route.query or {}) do
            wanted[#wanted + 1] = { "query", pair[1], pair[2] }
          end
          for _, pair in ipairs(route.body or {}) do
            wanted[#wanted + 1] = { "body", pair[1], pair[2] }
          end
          for _, want in ipairs(wanted) do
            local found = declared(spec, item, operation, want[1], want[2])
            if found == nil then
              problems[#problems + 1] =
                string.format("%s: no %s parameter `%s`", where, want[1], want[2])
            else
              checked = checked + 1
              local input = want[3] and inputs[want[3]]
              if input and input.enum and type(found.enum) == "table" then
                for _, value in ipairs(input.enum) do
                  local known = false
                  for _, allowed in ipairs(found.enum) do
                    known = known or allowed == value
                  end
                  if not known then
                    problems[#problems + 1] =
                      string.format("%s: `%s` no longer takes %q", where, want[2], value)
                  end
                end
              end
            end
          end
        end
      end
    end
  end
  return problems, checked
end

kage.register_command({
  name = "pkgforge-web-check",
  description = "compare the pkgforge-web tools with each service's openapi.json",
  handler = function()
    local problems, checked = drift()
    if #problems == 0 then
      return string.format(
        "pkgforge-web: all %d routes and parameters match the services' openapi.json",
        checked
      )
    end
    local noun = #problems == 1 and "mismatch" or "mismatches"
    return string.format("pkgforge-web: %d %s\n", #problems, noun) .. table.concat(problems, "\n")
  end,
})
