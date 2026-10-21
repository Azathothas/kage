# pkgforge web

`plugins/pkgforge-web.lua` adds tools for the free, keyless web
services at [web.pkgforge.dev](https://web.pkgforge.dev): web search, a
page as Markdown, scholarly works, site crawls and web archives. Every
answer says which provider served it and whether it came from a cache.

## install

Copy the file into your plugins directory and grant it the network:

```sh
cp plugins/pkgforge-web.lua ~/.config/kage/plugins/
```

```toml
[plugins.capabilities]
pkgforge-web = ["net"]
```

Without the grant the plugin registers nothing and says so.

## tools

| Tool | Service | What it does |
| --- | --- | --- |
| `web_search` | `search.web.pkgforge.dev` | web search, in place of the built-in `web_search` |
| `web_read` | `read.web.pkgforge.dev` | a page, a PDF or an office document as Markdown |
| `paper_search` | `research.web.pkgforge.dev` | papers by query, or one by DOI, arXiv id, PMID, PMCID or OpenAlex id |
| `web_crawl` | `crawl.web.pkgforge.dev` | the pages of a site from one start URL, as Markdown |
| `web_archive` | `archive.web.pkgforge.dev` | a URL's archived captures, the one nearest an instant, or one capture's bytes |
| `web_archive_save` | `archive.web.pkgforge.dev` | a public capture of a page, made now |

Every tool but `web_archive_save` has network risk, so it runs in plan
mode and in a read-only session the way `web_fetch` does.
`web_archive_save` makes a public capture, so it declares write risk:
plan mode and a read-only session refuse it, as they refuse `write`.

`web_search` keeps the built-in name, so the `explore` agent, whose
tool list names `web_search`, uses it as well. The other tools reach
the `general` agent and any agent definition without a `tools` list.

A long answer comes in pieces and says how to read on: `web_read` and
a `capture` name the `offset` of the rest, a finished crawl the
`cursor` of its next pages, and a capture listing its next page.

`web_crawl` and `web_archive_save` start a job and wait for it,
`wait_secs` long (30 seconds by default, at most 120). A job still
running then comes back with its id, and a later call with the id waits
again. The wait holds kage's Lua thread, so other plugin work queues
behind it. Cancelling the call ends the wait within half a second, or
once a request already in flight returns.

## configuration

`disable` leaves tools out:

```toml
[plugins.config.pkgforge-web]
# keep kage's own web_search, and no public captures
disable = ["web_search", "web_archive_save"]
```

## when a service changes

The services choose their upstream providers and answer in one shape,
so the plugin names no provider: a provider added or dropped upstream
needs no change here. An answer in a shape the plugin does not know is
passed on as it came, up to 4000 bytes, rather than failing the call.
A service that cannot be reached fails only its own tools, and the
error says how to leave them out.

`/pkgforge-web-check` fetches each service's `openapi.json` and lists
every route, parameter and enum value the plugin sends that the service
no longer declares.

The `TOOLS` table at the top of the file is the only code that is
specific to one service. An entry is one tool: its input schema, the
routes it calls, and how its answers read. Adding an entry adds a tool.
