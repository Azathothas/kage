//! `plugins/pkgforge-web.lua` against recorded answers of the pkgforge
//! web services.
//!
//! Each test loads the plugin with `net` granted, then swaps its
//! `kage.http.get` and `kage.http.post` for a stub that answers from
//! the fixtures under `tests/fixtures/pkgforge-web/`, so nothing here
//! touches the network. The `#[ignore]`d tests at the end call the live
//! services:
//!
//! ```sh
//! cargo test -p kage-plugin --test pkgforge_web -- --ignored
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use kage_core::{CancelFlag, Risk, ToolOutput};
use kage_plugin::{BridgePrep, BridgeStep, CommandOutput, HostLog, LogLevel, PluginRuntime};
use kage_tools::{Tool, ToolContext, ToolError};
use serde_json::{Value, json};

const PLUGIN: &str = "pkgforge-web";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .to_path_buf()
}

fn source() -> String {
    std::fs::read_to_string(repo_root().join("plugins").join("pkgforge-web.lua"))
        .expect("read plugins/pkgforge-web.lua")
}

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/pkgforge-web")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn fixture_json(name: &str) -> Value {
    serde_json::from_str(&fixture(name)).unwrap()
}

/// `text` as a Lua long string, at a bracket level nothing in `text`
/// closes. A long string drops the newline right after its opening, so
/// one is put there.
fn lua_str(text: &str) -> String {
    let mut level = 0;
    while format!("{text}]").contains(&format!("]{}]", "=".repeat(level))) {
        level += 1;
    }
    let eq = "=".repeat(level);
    format!("[{eq}[\n{text}]{eq}]")
}

/// Replaces the plugin's `kage.http.get` and `kage.http.post` with a
/// stub answering from `answers`. The first entry whose `match` is part
/// of `"<METHOD> <url>"` answers; one with `bodies` answers them in turn
/// and then keeps answering the last; one with `raise` fails the call
/// the way an unreachable host does, and one with `truncated` reports
/// its body cut at the cap. Each request is kept in `requests`.
const STUB: &str = r#"
requests, answers = {}, {}
local function stub(method)
  return function(url, opts)
    local key = method .. " " .. url
    requests[#requests + 1] = {
      key = key,
      json = opts.json and kage.json.encode(opts.json) or "",
      max_bytes = opts.max_bytes,
    }
    for _, a in ipairs(answers) do
      if key:find(a.match, 1, true) then
        if a.raise then
          error(a.raise)
        end
        local body = a.body
        if a.bodies then
          body = a.bodies[1]
          if #a.bodies > 1 then
            table.remove(a.bodies, 1)
          end
        end
        return {
          status = a.status or 200,
          body = body,
          content_type = a.content_type or "application/json",
          truncated = a.truncated or false,
          headers = a.headers or {},
        }
      end
    end
    error("transport error: no answer for " .. key)
  end
end
kage.http.get = stub("GET")
kage.http.post = stub("POST")
"#;

/// The plugin loaded with `net` granted and its HTTP stubbed.
struct Plugin {
    rt: PluginRuntime,
}

impl Plugin {
    fn load() -> Self {
        Self::load_with(&source(), BTreeMap::new())
    }

    fn load_with(source: &str, config: BTreeMap<String, Value>) -> Self {
        let rt = granted(config);
        rt.eval_plugin(PLUGIN, source).expect("the plugin loads");
        rt.eval_plugin(PLUGIN, STUB).unwrap();
        Self { rt }
    }

    /// Answer requests whose `"<METHOD> <url>"` holds `matching`.
    fn answer(&self, matching: &str, status: u16, content_type: &str, body: &str) {
        self.answer_lua(&format!(
            "{{ match = {}, status = {status}, content_type = {}, body = {} }}",
            lua_str(matching),
            lua_str(content_type),
            lua_str(body),
        ));
    }

    /// Add a stub entry written as a Lua table.
    fn answer_lua(&self, entry: &str) {
        self.rt
            .eval_plugin(PLUGIN, &format!("answers[#answers + 1] = {entry}"))
            .unwrap();
    }

    fn call(&self, tool: &str, input: Value) -> ToolOutput {
        let cancel = CancelFlag::new();
        let workdir = PathBuf::from(".");
        tool_named(&self.rt, tool)
            .execute(input, &ToolContext::new(&workdir, &cancel))
            .unwrap()
    }

    /// Every request so far, as `"<METHOD> <url>"`.
    fn requests(&self) -> Vec<String> {
        lua_lines(
            &self.rt,
            "local keys = {} for _, r in ipairs(requests) do keys[#keys + 1] = r.key end \
             return table.concat(keys, '\\n')",
        )
    }

    /// The JSON body of request `n`, counted from 1.
    fn request_json(&self, n: usize) -> Value {
        let raw = self
            .rt
            .eval_plugin(PLUGIN, &format!("return requests[{n}].json"))
            .unwrap();
        serde_json::from_str(&raw.as_string().unwrap().to_string_lossy()).unwrap()
    }
}

/// A runtime that grants plugin `pkgforge-web` the network.
fn granted(config: BTreeMap<String, Value>) -> PluginRuntime {
    let mut caps = BTreeMap::new();
    caps.insert(PLUGIN.to_owned(), vec!["net".to_owned()]);
    PluginRuntime::builder()
        .capabilities(caps)
        .plugin_config(config)
        .build()
        .unwrap()
}

fn lua_lines(rt: &PluginRuntime, script: &str) -> Vec<String> {
    let joined = rt.eval_plugin(PLUGIN, script).unwrap();
    let joined = joined.as_string().unwrap().to_string_lossy();
    joined
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

fn tool_named(rt: &PluginRuntime, name: &str) -> std::sync::Arc<dyn Tool> {
    rt.registered_tools()
        .into_iter()
        .chain(rt.registered_tool_overrides())
        .find(|tool| tool.name() == name)
        .unwrap_or_else(|| panic!("{name} is registered"))
}

/// The text `/pkgforge-web-check` answers.
fn run_check(rt: &PluginRuntime) -> String {
    let cmd = rt
        .registered_commands()
        .into_iter()
        .find(|c| c.name() == "pkgforge-web-check")
        .expect("the check is registered");
    let bargs = match cmd.prepare_bridge("", &json!(null)).unwrap() {
        BridgePrep::Ready(bargs) => bargs,
        BridgePrep::ArgError(out) => panic!("unexpected arg error: {}", out.text),
    };
    match rt.bridge_call(&bargs.handler, &bargs.args).unwrap() {
        BridgeStep::Done(v) => CommandOutput::from_json(&v).text,
        BridgeStep::Suspended(_) => panic!("the check should not suspend"),
    }
}

// Registration

#[test]
fn registers_a_tool_for_every_table_entry() {
    let p = Plugin::load();
    let tools: Vec<(String, Risk)> =
        p.rt.registered_tools()
            .iter()
            .map(|t| (t.name().to_owned(), t.risk()))
            .collect();
    let expected = [
        ("web_read", Risk::Network),
        ("paper_search", Risk::Network),
        ("web_crawl", Risk::Network),
        ("web_archive", Risk::Network),
        ("web_archive_save", Risk::Write),
    ]
    .map(|(name, risk)| (name.to_owned(), risk));
    assert_eq!(tools, expected);
    let overrides: Vec<String> =
        p.rt.registered_tool_overrides()
            .iter()
            .map(|t| t.name().to_owned())
            .collect();
    assert_eq!(
        overrides,
        ["web_search"],
        "web_search replaces the built-in"
    );

    let schema = tool_named(&p.rt, "web_search").schema();
    assert_eq!(schema["type"], "object");
    assert_eq!(schema["required"], json!(["query"]));
    assert_eq!(
        schema["properties"]["freshness"]["enum"],
        json!(["day", "week", "month", "year"])
    );
    let schema = tool_named(&p.rt, "web_crawl").schema();
    assert_eq!(
        schema["properties"]["include_paths"]["items"]["type"],
        "string"
    );
    assert!(schema.get("required").is_none(), "{schema}");
}

#[derive(Clone, Default)]
struct Notes(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

impl HostLog for Notes {
    fn notify(&mut self, message: &str) {
        self.0.lock().unwrap().push(message.to_owned());
    }
    fn log(&mut self, _level: LogLevel, _message: &str) {}
}

#[test]
fn without_net_the_plugin_registers_nothing_and_says_why() {
    let notes = Notes::default();
    let sink: kage_plugin::SharedHostLog = std::sync::Arc::new(std::sync::Mutex::new(Box::new(
        notes.clone(),
    )
        as Box<dyn HostLog + Send>));
    let rt = PluginRuntime::builder().sink(sink).build().unwrap();
    rt.eval_plugin(PLUGIN, &source()).unwrap();
    assert!(rt.registered_tools().is_empty());
    assert!(rt.registered_tool_overrides().is_empty());
    let notes = notes.0.lock().unwrap();
    assert!(
        notes
            .iter()
            .any(|n| n.starts_with("pkgforge-web: disabled")),
        "{notes:?}"
    );
}

#[test]
fn disable_leaves_the_named_tools_out() {
    let config = BTreeMap::from([(
        PLUGIN.to_owned(),
        json!({ "disable": ["web_search", "web_archive_save"] }),
    )]);
    let p = Plugin::load_with(&source(), config);
    assert!(p.rt.registered_tool_overrides().is_empty());
    let names: Vec<String> =
        p.rt.registered_tools()
            .iter()
            .map(|t| t.name().to_owned())
            .collect();
    assert_eq!(
        names,
        ["web_read", "paper_search", "web_crawl", "web_archive"]
    );
}

#[test]
fn an_entry_added_to_the_table_is_a_new_tool() {
    let entry = r#"local TOOLS = {
  {
    name = "web_extra",
    description = "An entry added to the table.",
    input = { { name = "query", type = "string", required = true, doc = "What to search for." } },
    routes = {
      {
        host = SEARCH,
        method = "GET",
        path = "/search",
        query = { { "q", "query" } },
        show = { list = "results", title = "title", url = "url" },
      },
    },
  },"#;
    let source = source().replacen("local TOOLS = {", entry, 1);
    let p = Plugin::load_with(&source, BTreeMap::new());
    p.answer(
        "GET https://search.web.pkgforge.dev/search?q=",
        200,
        "application/json",
        &fixture("search.json"),
    );
    let out = p.call("web_extra", json!({ "query": "rust ureq" }));
    assert!(!out.is_error, "{}", out.text);
    assert!(
        out.text
            .starts_with("1. ureq - Rust\n   https://docs.rs/ureq/latest/ureq/"),
        "{}",
        out.text
    );
    assert_eq!(
        p.requests(),
        ["GET https://search.web.pkgforge.dev/search?q=rust%20ureq"]
    );
}

// Answers

#[test]
fn web_search_lists_results_with_how_they_were_served() {
    let p = Plugin::load();
    p.answer(
        "GET https://search.web.pkgforge.dev/search?",
        200,
        "application/json",
        &fixture("search.json"),
    );
    let out = p.call("web_search", json!({ "query": "rust ureq", "count": 2 }));
    assert!(!out.is_error, "{}", out.text);
    assert_eq!(
        p.requests(),
        ["GET https://search.web.pkgforge.dev/search?q=rust%20ureq&count=2"]
    );
    let lines: Vec<&str> = out.text.lines().collect();
    assert_eq!(lines[0], "1. ureq - Rust");
    assert_eq!(lines[1], "   https://docs.rs/ureq/latest/ureq/");
    assert!(
        lines[2].starts_with("   ureq - Rust # Crate ureq Copy item path"),
        "{}",
        lines[2]
    );
    assert!(
        lines[2].ends_with("..."),
        "a long snippet is cut: {}",
        lines[2]
    );
    assert!(lines[2].len() <= 3 + 500 + 3, "{}", lines[2].len());
    assert_eq!(lines[3], "2. ureq");
    assert_eq!(
        lines.last().unwrap(),
        &"search.web.pkgforge.dev: provider exa, cached false, rank reranked(bge-reranker-base@binding)"
    );
    // The desktop app reads a web_search call's results from here.
    let structured = out.structured.unwrap();
    assert_eq!(
        structured["results"][0]["url"],
        "https://docs.rs/ureq/latest/ureq/"
    );
    assert_eq!(structured["results"][1]["title"], "ureq");
}

#[test]
fn inputs_are_sent_as_the_service_spells_them() {
    let p = Plugin::load();
    p.answer(
        "GET https://search.web.pkgforge.dev/",
        200,
        "application/json",
        &fixture("search.json"),
    );
    let out = p.call(
        "web_search",
        json!({
            "query": " a&b=c ",
            "count": "50",
            "freshness": "week",
            "include_domains": ["docs.rs", "crates.io"],
            "exclude_domains": "",
        }),
    );
    assert!(!out.is_error, "{}", out.text);
    assert_eq!(
        p.requests(),
        [
            "GET https://search.web.pkgforge.dev/search?q=a%26b%3Dc&count=20&freshness=week&include_domains=docs.rs%2Ccrates.io"
        ]
    );
}

#[test]
fn a_field_the_table_does_not_know_changes_nothing() {
    let p = Plugin::load();
    p.answer(
        "GET https://search.web.pkgforge.dev/",
        200,
        "application/json",
        &fixture("search.json"),
    );
    let plain = p.call("web_search", json!({ "query": "rust ureq" })).text;

    let mut answer = fixture_json("search.json");
    answer["served_by"] = json!({ "region": "somewhere", "tier": 2 });
    answer["results"][0]["language"] = json!("en");
    let p = Plugin::load();
    p.answer(
        "GET https://search.web.pkgforge.dev/",
        200,
        "application/json",
        &answer.to_string(),
    );
    let out = p.call("web_search", json!({ "query": "rust ureq" }));
    assert!(!out.is_error);
    assert_eq!(out.text, plain);
}

#[test]
fn a_renamed_list_still_reaches_the_model() {
    let mut answer = fixture_json("search.json");
    let results = answer.as_object_mut().unwrap().remove("results").unwrap();
    answer["items"] = results;
    let p = Plugin::load();
    p.answer(
        "GET https://search.web.pkgforge.dev/",
        200,
        "application/json",
        &answer.to_string(),
    );
    let out = p.call("web_search", json!({ "query": "rust ureq" }));
    assert!(!out.is_error, "{}", out.text);
    assert!(
        out.text
            .starts_with("search.web.pkgforge.dev answered without `results`"),
        "{}",
        out.text
    );
    // A runner that assumed `results` would answer "No results." here.
    assert!(
        out.text.contains("https://docs.rs/ureq/latest/ureq/"),
        "{}",
        out.text
    );
}

#[test]
fn a_problem_names_its_reason_its_wait_and_the_providers_asked() {
    let p = Plugin::load();
    p.answer_lua(
        r#"{
          match = "GET https://search.web.pkgforge.dev/",
          status = 429,
          content_type = "application/problem+json",
          headers = { ["retry-after"] = "7" },
          body = [[{"status":429,"title":"every provider is held","detail":"every provider leg is held by a rate limit or a spent allowance; the earliest reopens in 7 s","see":"https://search.web.pkgforge.dev/llms.txt","reason":"held","errors":[{"provider":"one","reason":"rate-limited","resetAt":null},{"provider":"two","attempts":[{"account":"a","kind":"timeout"},{"account":"b","kind":"input-refused"}]}]}]],
        }"#,
    );
    let out = p.call("web_search", json!({ "query": "rust" }));
    assert!(out.is_error);
    let lines: Vec<&str> = out.text.lines().collect();
    assert_eq!(
        lines[0],
        "search.web.pkgforge.dev answered 429 held: every provider is held"
    );
    assert!(
        lines[1].starts_with("every provider leg is held"),
        "{}",
        lines[1]
    );
    assert_eq!(lines[2], "Retry after 7 s.");
    assert_eq!(
        lines[3],
        "Providers asked: one rate-limited; two timeout, input-refused."
    );
    assert_eq!(lines[4], "See https://search.web.pkgforge.dev/llms.txt");
}

#[test]
fn an_unreachable_service_fails_only_its_own_tools() {
    let p = Plugin::load();
    p.answer_lua(
        r#"{ match = "https://search.web.pkgforge.dev", raise = "http.get https://search.web.pkgforge.dev/search: transport error: dns lookup failed" }"#,
    );
    p.answer(
        "GET https://archive.web.pkgforge.dev/captures?",
        404,
        "text/html",
        "<html>nothing here</html>",
    );
    p.answer(
        "GET https://research.web.pkgforge.dev/works?",
        200,
        "application/json",
        &fixture("works.json"),
    );

    let out = p.call("web_search", json!({ "query": "rust" }));
    assert!(out.is_error);
    assert!(
        out.text
            .starts_with("search.web.pkgforge.dev could not be reached:"),
        "{}",
        out.text
    );
    assert!(out.text.contains("dns lookup failed"), "{}", out.text);
    assert!(
        out.text
            .contains("`disable` in [plugins.config.pkgforge-web]"),
        "{}",
        out.text
    );

    let out = p.call("web_archive", json!({ "url": "example.com" }));
    assert!(out.is_error);
    assert!(
        out.text.contains("may have moved or been removed"),
        "{}",
        out.text
    );

    let out = p.call("paper_search", json!({ "query": "attention" }));
    assert!(!out.is_error, "{}", out.text);
}

#[test]
fn bad_input_is_refused_before_any_request() {
    let p = Plugin::load();
    let cases = [
        ("web_search", json!({}), "web_search: query is required"),
        (
            "web_search",
            json!({ "query": "x", "freshness": "decade" }),
            "web_search: freshness must be one of day, week, month, year",
        ),
        (
            "web_search",
            json!({ "query": "x", "count": "many" }),
            "web_search: count must be a whole number",
        ),
        (
            "paper_search",
            json!({}),
            "paper_search: give exactly one of query or id",
        ),
        (
            "paper_search",
            json!({ "query": "x", "id": "arxiv:1706.03762" }),
            "paper_search: give exactly one of query or id",
        ),
        (
            "web_crawl",
            json!({ "limit": 3 }),
            "web_crawl: give exactly one of url or job_id",
        ),
        (
            "web_archive",
            json!({ "url": "example.com", "action": "capture" }),
            "web_archive: at is required here",
        ),
    ];
    for (tool, input, expected) in cases {
        let out = p.call(tool, input.clone());
        assert!(out.is_error, "{tool} {input}");
        assert_eq!(out.text, expected, "{tool} {input}");
    }
    assert!(p.requests().is_empty(), "{:?}", p.requests());
}

#[test]
fn paper_search_searches_by_query_and_looks_up_by_id() {
    let p = Plugin::load();
    p.answer(
        "GET https://research.web.pkgforge.dev/works?",
        200,
        "application/json",
        &fixture("works.json"),
    );
    p.answer(
        "GET https://research.web.pkgforge.dev/works/",
        200,
        "application/json",
        &fixture("work.json"),
    );

    let out = p.call(
        "paper_search",
        json!({ "query": "attention is all you need", "count": 50, "open_access": true }),
    );
    assert!(!out.is_error, "{}", out.text);
    let lines: Vec<&str> = out.text.lines().collect();
    assert_eq!(lines[0], "1. Attention Is All You Need");
    assert!(
        lines[1].starts_with("   year: 2025; authors: Ashish Vaswani, Noam Shazeer, Niki Parmar, Jakob Uszkoreit, Llion Jones, Aidan N. Gomez, and 2 more; type: preprint; doi: 10.65215/2q58a426; cited_by: 78013; oa_url: "),
        "{}",
        lines[1]
    );
    assert!(
        lines[2].starts_with("   The dominant sequence transduction models"),
        "{}",
        lines[2]
    );
    assert_eq!(
        lines.last().unwrap(),
        &"research.web.pkgforge.dev: provider openalex, cached false, rank provider(few), merged 0"
    );

    let out = p.call("paper_search", json!({ "id": "arxiv:1706.03762" }));
    assert!(!out.is_error, "{}", out.text);
    assert!(out.text.contains("year: 2017"), "{}", out.text);
    assert!(
        out.text
            .contains("ids: arxiv 1706.03762, doi 10.48550/arxiv.1706.03762, openalex W2626778328"),
        "{}",
        out.text
    );
    assert_eq!(
        out.text.lines().last().unwrap(),
        "research.web.pkgforge.dev: provider openalex, cached true, arxiv_check.status corrected"
    );
    assert_eq!(
        p.requests(),
        [
            "GET https://research.web.pkgforge.dev/works?q=attention%20is%20all%20you%20need&count=25&oa=true",
            "GET https://research.web.pkgforge.dev/works/arxiv%3A1706.03762",
        ]
    );
}

#[test]
fn web_read_answers_the_page_and_pages_a_long_one() {
    let p = Plugin::load();
    let page = fixture("read.md");
    p.answer(
        "GET https://read.web.pkgforge.dev/?url=https%3A%2F%2Fexample.com%2F&",
        200,
        "text/markdown; charset=utf-8",
        &page,
    );
    let long = format!("{page}{}", "word ".repeat(30_000));
    p.answer(
        "GET https://read.web.pkgforge.dev/?url=https%3A%2F%2Fexample.com%2Flong",
        200,
        "text/markdown; charset=utf-8",
        &long,
    );

    let out = p.call(
        "web_read",
        json!({ "url": "https://example.com/", "provider": "one" }),
    );
    assert!(!out.is_error, "{}", out.text);
    assert_eq!(out.text, page, "a page with front matter comes as it is");

    let first = p.call("web_read", json!({ "url": "https://example.com/long" }));
    assert!(first.text.starts_with(&page), "{}", &first.text[..200]);
    let note = format!(
        "[read.web.pkgforge.dev: bytes 0 to 60000 of {}; call web_read again with offset = 60000 for the rest]",
        long.len()
    );
    assert!(
        first.text.ends_with(&note),
        "{}",
        &first.text[first.text.len() - 200..]
    );
    let second = p.call(
        "web_read",
        json!({ "url": "https://example.com/long", "offset": 60000 }),
    );
    assert!(second.text.starts_with(&long[60000..60100]));
    let third = p.call(
        "web_read",
        json!({ "url": "https://example.com/long", "offset": 120_000 }),
    );
    assert_eq!(third.text, long[120_000..]);
    let past = p.call(
        "web_read",
        json!({ "url": "https://example.com/long", "offset": 10_000_000 }),
    );
    assert!(
        past.text.starts_with("offset 10000000 is past the end"),
        "{}",
        past.text
    );
    assert_eq!(
        p.requests()[0],
        "GET https://read.web.pkgforge.dev/?url=https%3A%2F%2Fexample.com%2F&provider=one"
    );
}

#[test]
fn large_answers_may_pass_the_default_byte_cap() {
    let p = Plugin::load();
    p.answer_lua(&format!(
        "{{ match = 'GET https://read.web.pkgforge.dev/', content_type = 'text/markdown', truncated = true, body = {} }}",
        lua_str(&fixture("read.md"))
    ));
    p.answer(
        "GET https://search.web.pkgforge.dev/",
        200,
        "application/json",
        &fixture("search.json"),
    );
    let out = p.call("web_read", json!({ "url": "https://example.com/" }));
    assert!(
        out.text.ends_with(
            "

[read.web.pkgforge.dev: the body was cut at 388 bytes]"
        ),
        "{}",
        out.text
    );
    p.call("web_search", json!({ "query": "rust" }));
    let caps =
        p.rt.eval_plugin(
            PLUGIN,
            "return tostring(requests[1].max_bytes) .. ' ' .. tostring(requests[2].max_bytes)",
        )
        .unwrap();
    assert_eq!(caps.as_string().unwrap().to_string_lossy(), "16000000 nil");
}

#[test]
fn web_read_passes_on_an_origins_own_error_page() {
    let p = Plugin::load();
    p.answer(
        "GET https://read.web.pkgforge.dev/",
        404,
        "text/markdown; charset=utf-8",
        "---\nupstream_status: 404\n---\nNot Found\n",
    );
    let out = p.call("web_read", json!({ "url": "https://example.com/missing" }));
    assert!(out.is_error);
    assert_eq!(
        out.text,
        "read.web.pkgforge.dev answered 404 with this page:\n\n---\nupstream_status: 404\n---\nNot Found\n"
    );
}

#[test]
fn web_archive_lists_captures_and_names_the_next_page() {
    let p = Plugin::load();
    p.answer(
        "GET https://archive.web.pkgforge.dev/captures?",
        200,
        "application/json",
        &fixture("captures.json"),
    );
    let out = p.call("web_archive", json!({ "url": "example.com", "limit": 2 }));
    assert!(!out.is_error, "{}", out.text);
    let lines: Vec<&str> = out.text.lines().collect();
    assert_eq!(lines[0], "1. 20020120142510");
    assert_eq!(lines[1], "   http://example.com/");
    assert_eq!(
        lines[2],
        "   status: 200; mimetype: text/html; replay: https://archive.web.pkgforge.dev/capture/20020120142510/http://example.com/?provider=wayback"
    );
    assert_eq!(lines[3], "2. 20020328012821");
    assert_eq!(
        lines[6],
        "More: call web_archive again with the same arguments and cursor = \"WyJ3YXliYWNrIiwiZUp4THpzX1ZTYTFJekMzSVNkWFVWekF5TURBeU1EYXlNREEwc2pBeUJBQl9ZQWVQIl0\", provider = \"wayback\"."
    );
    assert_eq!(
        lines[7],
        "archive.web.pkgforge.dev: provider wayback, cached false"
    );
    assert_eq!(
        p.requests(),
        ["GET https://archive.web.pkgforge.dev/captures?url=example.com&limit=2"]
    );
}

#[test]
fn web_archive_finds_the_closest_capture_and_returns_one() {
    let p = Plugin::load();
    p.answer(
        "GET https://archive.web.pkgforge.dev/closest?",
        200,
        "application/json",
        &fixture("closest.json"),
    );
    p.answer_lua(
        r#"{
          match = "GET https://archive.web.pkgforge.dev/capture/",
          content_type = "text/html",
          headers = {
            ["x-web-capture-timestamp"] = "20020120142510",
            ["x-web-original-url"] = "http://example.com/",
            ["x-web-origin-status"] = "200",
            ["x-web-provider"] = "wayback",
            ["content-type"] = "text/html",
          },
          body = "<HTML><TITLE>Reserved Domain Names</TITLE></HTML>",
        }"#,
    );
    let out = p.call(
        "web_archive",
        json!({ "action": "closest", "url": "example.com", "at": "2002" }),
    );
    assert!(!out.is_error, "{}", out.text);
    assert!(
        out.text
            .starts_with("1. 20020120142510\n   http://example.com/\n   status: 200; replay: "),
        "{}",
        out.text
    );

    let out = p.call(
        "web_archive",
        json!({ "action": "capture", "url": "http://example.com/", "at": "20020120142510" }),
    );
    assert!(!out.is_error, "{}", out.text);
    assert_eq!(
        out.text,
        "archive.web.pkgforge.dev: x-web-capture-timestamp 20020120142510, x-web-original-url http://example.com/, \
         x-web-origin-status 200, x-web-provider wayback, content-type text/html\n\n\
         <HTML><TITLE>Reserved Domain Names</TITLE></HTML>"
    );
    assert_eq!(
        p.requests(),
        [
            "GET https://archive.web.pkgforge.dev/closest?url=example.com&at=2002",
            "GET https://archive.web.pkgforge.dev/capture/20020120142510%2Fhttp%3A%2F%2Fexample.com%2F",
        ]
    );
}

#[test]
fn a_null_closest_capture_shows_the_answer() {
    let mut answer = fixture_json("closest.json");
    answer["closest"] = Value::Null;
    answer["misses"] = json!([{ "provider": "wayback", "reason": "none" }]);
    let p = Plugin::load();
    p.answer(
        "GET https://archive.web.pkgforge.dev/closest?",
        200,
        "application/json",
        &answer.to_string(),
    );
    let out = p.call(
        "web_archive",
        json!({ "action": "closest", "url": "example.com", "at": "1990" }),
    );
    assert!(!out.is_error, "{}", out.text);
    assert!(
        out.text
            .starts_with("archive.web.pkgforge.dev answered without `closest`"),
        "{}",
        out.text
    );
    assert!(out.text.contains(r#""reason":"none""#), "{}", out.text);
}

// Jobs

const RUNNING: &str = r#"{"job_id":"cj_d5d30f4fe0ac47c8a363ca3ec2a2f9b1","provider":"firecrawl","status":"running","reason":null,"url":"https://example.com/","limit":2,"pages":[],"dropped":{},"dropped_urls":{},"incomplete":0,"next_cursor":null,"usage":null,"progress":{"completed":0,"total":2},"started_at":"2026-10-20T23:27:36.522Z"}"#;

#[test]
fn web_crawl_submits_a_crawl_then_reads_its_pages() {
    let p = Plugin::load();
    p.answer(
        "POST https://crawl.web.pkgforge.dev/crawl",
        202,
        "application/json",
        &fixture("crawl-submit.json"),
    );
    p.answer_lua(&format!(
        "{{ match = 'GET https://crawl.web.pkgforge.dev/crawl/cj_', headers = {{ ['x-web-crawl-content'] = 'main', ['x-web-cache'] = 'miss' }}, body = {} }}",
        lua_str(&fixture("crawl-poll.json"))
    ));
    let out = p.call(
        "web_crawl",
        json!({ "url": "https://example.com", "limit": 2, "include_paths": ["/", "/docs/**"], "main_content": true }),
    );
    assert!(!out.is_error, "{}", out.text);
    let lines: Vec<&str> = out.text.lines().collect();
    assert_eq!(
        lines[0],
        "Crawl cj_d5d30f4fe0ac47c8a363ca3ec2a2f9b1 from https://example.com/: 1 page in this answer."
    );
    assert_eq!(lines[2], "## Example Domain");
    assert_eq!(lines[3], "https://example.com/ (status 200)");
    assert!(
        lines[5].starts_with("This domain is for use in documentation examples"),
        "{}",
        lines[5]
    );
    assert!(
        out.text
            .contains("\nDropped: duplicates 1, outside_paths 0.\n"),
        "{}",
        out.text
    );
    assert!(!out.text.contains("More pages"), "{}", out.text);
    assert_eq!(
        lines.last().unwrap(),
        &"crawl.web.pkgforge.dev: provider firecrawl, status completed, content main, cache miss"
    );
    assert_eq!(
        p.requests(),
        [
            "POST https://crawl.web.pkgforge.dev/crawl",
            "GET https://crawl.web.pkgforge.dev/crawl/cj_d5d30f4fe0ac47c8a363ca3ec2a2f9b1",
        ]
    );
    assert_eq!(
        p.request_json(1),
        json!({ "url": "https://example.com", "limit": 2, "include_paths": ["/", "/docs/**"], "main_content": true })
    );
}

#[test]
fn web_crawl_hands_back_a_running_job_and_reads_on_by_cursor() {
    let p = Plugin::load();
    p.answer(
        "GET https://crawl.web.pkgforge.dev/crawl/cj_1?cursor=",
        200,
        "application/json",
        &fixture("crawl-poll.json").replace(r#""next_cursor": null"#, r#""next_cursor": "cc_2""#),
    );
    p.answer(
        "GET https://crawl.web.pkgforge.dev/crawl/cj_1",
        200,
        "application/json",
        RUNNING,
    );
    let out = p.call("web_crawl", json!({ "job_id": "cj_1", "wait_secs": 0 }));
    assert!(!out.is_error, "{}", out.text);
    assert_eq!(
        out.text,
        "Crawl cj_1 is still running.\nProgress: completed 0, total 2.\n\
         Call web_crawl with job_id = \"cj_1\" to wait for it again.\n\
         crawl.web.pkgforge.dev: provider firecrawl, status running"
    );
    let out = p.call("web_crawl", json!({ "job_id": "cj_1", "cursor": "cc_1" }));
    assert!(
        out.text
            .contains("More pages: call web_crawl with job_id = \"cj_1\" and cursor = \"cc_2\"."),
        "{}",
        out.text
    );
    assert_eq!(
        p.requests(),
        [
            "GET https://crawl.web.pkgforge.dev/crawl/cj_1",
            "GET https://crawl.web.pkgforge.dev/crawl/cj_1?cursor=cc_1",
        ]
    );
}

#[test]
fn cancelling_a_crawl_stops_its_polls() {
    let p = Plugin::load();
    p.answer(
        "POST https://crawl.web.pkgforge.dev/crawl",
        202,
        "application/json",
        &fixture("crawl-submit.json"),
    );
    p.answer(
        "GET https://crawl.web.pkgforge.dev/crawl/",
        200,
        "application/json",
        RUNNING,
    );
    let tool = tool_named(&p.rt, "web_crawl");
    let cancel = CancelFlag::new();
    let canceller = {
        let cancel = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            cancel.cancel();
        })
    };
    let workdir = PathBuf::from(".");
    let err = tool
        .execute(
            json!({ "url": "https://example.com", "wait_secs": 120 }),
            &ToolContext::new(&workdir, &cancel),
        )
        .unwrap_err();
    assert!(matches!(err, ToolError::Cancelled));
    let cancelled_at = Instant::now();
    let requests = p.requests();
    let freed_after = cancelled_at.elapsed();
    canceller.join().unwrap();
    assert!(
        freed_after < Duration::from_secs(2),
        "the poll ran on for {freed_after:?}"
    );
    assert_eq!(requests.len(), 2, "one submit and one poll: {requests:?}");
}

#[test]
fn web_archive_save_waits_for_the_capture() {
    let p = Plugin::load();
    p.answer(
        "POST https://archive.web.pkgforge.dev/save?url=https%3A%2F%2Fexample.com%2F",
        202,
        "application/json",
        r#"{"job":"spn2-abc","url":"https://example.com/","status":"pending","poll":"/save/spn2-abc","provider":"wayback","account":"keyless"}"#,
    );
    p.answer(
        "GET https://archive.web.pkgforge.dev/save/spn2-abc",
        200,
        "application/json",
        r#"{"job":"spn2-abc","url":"https://example.com/","submitted_at":"2026-10-21T00:00:00Z","status":"success","timestamp":"20261021000001","capture":"https://archive.web.pkgforge.dev/capture/20261021000001/https://example.com/","status_url":"https://example.com/","error":null,"provider":"wayback"}"#,
    );
    let out = p.call("web_archive_save", json!({ "url": "https://example.com/" }));
    assert!(!out.is_error, "{}", out.text);
    assert_eq!(
        out.text,
        "Archived https://example.com/ at 20261021000001.\n\
         Capture: https://archive.web.pkgforge.dev/capture/20261021000001/https://example.com/\n\
         archive.web.pkgforge.dev: provider wayback, submitted_at 2026-10-21T00:00:00Z"
    );

    let p = Plugin::load();
    p.answer(
        "GET https://archive.web.pkgforge.dev/save/spn2-abc",
        200,
        "application/json",
        r#"{"job":"spn2-abc","url":"https://example.com/","submitted_at":"2026-10-21T00:00:00Z","status":"error","timestamp":null,"capture":null,"error":"error:too-many-daily-captures","provider":"wayback"}"#,
    );
    let out = p.call("web_archive_save", json!({ "job": "spn2-abc" }));
    assert!(out.is_error);
    assert!(
        out.text
            .starts_with("The save of https://example.com/ failed: error:too-many-daily-captures."),
        "{}",
        out.text
    );
}

// Drift

/// Serve each service's recorded openapi.json, after `edit`.
fn serve_specs(p: &Plugin, edit: impl FnOnce(&mut Value)) {
    let mut specs = fixture_json("openapi.json");
    edit(&mut specs);
    for (host, spec) in specs.as_object().unwrap() {
        p.answer(
            &format!("GET https://{host}/openapi.json"),
            200,
            "application/json",
            &spec.to_string(),
        );
    }
}

#[test]
fn the_check_passes_against_the_recorded_specs() {
    let p = Plugin::load();
    serve_specs(&p, |_| {});
    let out = run_check(&p.rt);
    assert!(
        out.starts_with("pkgforge-web: all ")
            && out.ends_with(" routes and parameters match the services' openapi.json"),
        "{out}"
    );
}

#[test]
fn the_check_names_a_renamed_parameter() {
    let p = Plugin::load();
    serve_specs(&p, |specs| {
        let params = specs["search.web.pkgforge.dev"]["paths"]["/search"]["get"]["parameters"]
            .as_array_mut()
            .unwrap();
        let freshness = params
            .iter_mut()
            .find(|p| p["name"] == "freshness")
            .unwrap();
        freshness["name"] = json!("recency");
    });
    let out = run_check(&p.rt);
    assert_eq!(
        out,
        "pkgforge-web: 1 mismatch\n\
         web_search: search.web.pkgforge.dev GET /search: no query parameter `freshness`"
    );
}

#[test]
fn the_check_names_a_dropped_route_body_field_and_enum_value() {
    let p = Plugin::load();
    serve_specs(&p, |specs| {
        let archive = specs["archive.web.pkgforge.dev"]["paths"]
            .as_object_mut()
            .unwrap();
        archive.remove("/closest");
        let freshness = specs["search.web.pkgforge.dev"]["paths"]["/search"]["get"]["parameters"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|p| p["name"] == "freshness")
            .unwrap();
        freshness["schema"]["enum"] = json!(["day", "week", "month"]);
        let crawl = &mut specs["crawl.web.pkgforge.dev"]["paths"]["/crawl"]["post"]["requestBody"]
            ["content"]["application/json"]["schema"]["properties"];
        crawl.as_object_mut().unwrap().remove("main_content");
    });
    let out = run_check(&p.rt);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "pkgforge-web: 3 mismatches", "{out}");
    assert!(
        lines.contains(
            &"web_search: search.web.pkgforge.dev GET /search: `freshness` no longer takes \"year\""
        ),
        "{out}"
    );
    assert!(
        lines.contains(
            &"web_crawl: crawl.web.pkgforge.dev POST /crawl: no body parameter `main_content`"
        ),
        "{out}"
    );
    assert!(
        lines.contains(&"web_archive: archive.web.pkgforge.dev GET /closest: no such route"),
        "{out}"
    );
}

// Live

/// The plugin as installed: `net` granted and nothing stubbed.
fn live() -> PluginRuntime {
    let rt = granted(BTreeMap::new());
    rt.eval_plugin(PLUGIN, &source()).expect("the plugin loads");
    rt
}

fn live_call(rt: &PluginRuntime, tool: &str, input: Value) -> ToolOutput {
    let cancel = CancelFlag::new();
    let workdir = PathBuf::from(".");
    let shown = input.to_string();
    let out = tool_named(rt, tool)
        .execute(input, &ToolContext::new(&workdir, &cancel))
        .unwrap();
    assert!(!out.is_error, "{tool} {shown}: {}", out.text);
    out
}

#[test]
#[ignore = "calls the live pkgforge web services"]
fn live_the_table_matches_the_services() {
    let out = run_check(&live());
    assert!(out.starts_with("pkgforge-web: all "), "{out}");
}

#[test]
#[ignore = "calls the live pkgforge web services"]
fn live_each_tool_answers() {
    let rt = live();
    let out = live_call(
        &rt,
        "web_search",
        json!({ "query": "rust ureq http client", "count": 3 }),
    );
    assert!(out.text.starts_with("1. "), "{}", out.text);
    assert!(
        out.text.contains("\nsearch.web.pkgforge.dev: provider "),
        "{}",
        out.text
    );
    let out = live_call(&rt, "web_read", json!({ "url": "https://example.com/" }));
    assert!(out.text.contains("Example Domain"), "{}", out.text);
    let out = live_call(&rt, "paper_search", json!({ "id": "arxiv:1706.03762" }));
    assert!(
        out.text.contains("Attention Is All You Need"),
        "{}",
        out.text
    );
    let out = live_call(
        &rt,
        "paper_search",
        json!({ "query": "crispr base editing", "count": 2 }),
    );
    assert!(out.text.starts_with("1. "), "{}", out.text);
    let out = live_call(
        &rt,
        "web_archive",
        json!({ "url": "example.com", "limit": 2 }),
    );
    assert!(out.text.starts_with("1. "), "{}", out.text);
    let out = live_call(
        &rt,
        "web_archive",
        json!({ "action": "closest", "url": "example.com", "at": "2002" }),
    );
    assert!(out.text.starts_with("1. 2002"), "{}", out.text);
    let out = live_call(
        &rt,
        "web_archive",
        json!({ "action": "capture", "url": "http://example.com/", "at": "20020120142510" }),
    );
    assert!(
        out.text.contains("x-web-capture-timestamp 20020120142510"),
        "{}",
        out.text
    );
}

#[test]
#[ignore = "calls the live pkgforge web services"]
fn live_a_crawl_ends_with_its_pages() {
    let rt = live();
    let out = live_call(
        &rt,
        "web_crawl",
        json!({ "url": "https://example.com", "limit": 1, "wait_secs": 120 }),
    );
    assert!(
        out.text.contains("page in this answer") || out.text.contains("pages in this answer"),
        "{}",
        out.text
    );
}
