# packages

A plugin is either a script or a package.

- A **script** is one file, `~/.config/kage/plugins/<name>.lua`. It
  needs nothing else, which suits a quick local tweak.
- A **package** is a directory, `~/.config/kage/plugins/<name>/`. It
  carries a manifest, an entry file and any number of modules, which
  suits anything you want to share, version or split into files.

Packages need API 3 or later.

## layout

```
~/.config/kage/plugins/
  git-status.lua          a script
  web/                    a package
    plugin.toml           the manifest (required)
    init.lua              the entry (required)
    lua/                  modules for require (optional)
      web/
        tools.lua         require("web.tools")
        drift/init.lua    require("web.drift")
```

kage runs `init.lua` in the package's own environment, like a script.
Anything else in the directory, such as a README or tests, is ignored.

Both kinds load together in file-name order. A script and a package
with the same name are an error. Neither loads until you remove one.
Entries whose name starts with `.` are skipped.

## plugin.toml

```toml
name = "web"
version = "0.1.0"
api = 3
description = "Search and read the web."
capabilities = ["net"]
authors = ["you"]
license = "MIT"
homepage = "https://example.org/web"
repository = "https://github.com/you/kage-web"
tags = ["web", "search"]
```

| key | required | meaning |
| --- | --- | --- |
| `name` | yes | The package name. It must equal the directory's name and use only letters, digits, `-` and `_`. |
| `version` | yes | A [semantic version](https://semver.org). |
| `api` | yes | The lowest `kage.api_version()` the package needs. |
| `description` | no | One line on what the package does. |
| `capabilities` | no | Every [capability](/plugins/capabilities) the package may request. Defaults to none. |
| `authors`, `license`, `homepage`, `repository`, `tags` | no | Shown to people choosing a package. `license` is an SPDX expression. |

kage ignores keys it does not know, so a manifest written for a newer
kage still loads when its `api` allows.

## what the manifest enforces

The manifest is checked on every load, not just read.

- **`name`** must match the directory. A package copied under another
  name does not load.
- **`api`** is checked before any of the package's code runs. A
  package that needs a newer kage fails with a message naming the
  version it needs, instead of failing deep inside a missing function.
- **`capabilities`** is a ceiling. A package can hold only what its
  manifest declares *and* `[plugins.capabilities]` grants.
  `kage.request_capabilities` raises for a capability the manifest
  does not list, and a hook on an event that needs an undeclared
  capability is ignored with a warning. Granting more in config does
  not widen the ceiling.

## require

A package gets a `require` confined to its own `lua/` directory.

- Module names are dot-separated segments of letters, digits, `-` and
  `_`. `require("web.tools")` loads `lua/web/tools.lua`, then
  `lua/web/tools/init.lua`.
- A path that resolves outside `lua/`, through a symlink for example,
  is refused.
- Modules run in the package's environment, see what `init.lua` sees,
  and are cached until the next reload.
- A module that requires itself, directly or not, raises.

Put your modules under a directory named after the package
(`lua/web/...`). That keeps the names readable and leaves room to
share code later.

Scripts have no `require`.

## reload

The TUI reloads every plugin when a script, a package's `plugin.toml`,
or a `.lua` file anywhere inside a package changes. `/reload` does the
same by hand. A reload drops every module cache, so an edited module
takes effect at once.

## developing a package

Keep the package in its own checkout and link it into the plugins
directory:

```sh
ln -s ~/src/kage-web ~/.config/kage/plugins/web
```

The link's name is the package name, so it must match `name` in
`plugin.toml`. Removing the plugin from kage removes only the link,
never your checkout.

`[plugins] disabled`, `[plugins.capabilities]` and
`[plugins.config.<name>]` name a package by its directory, the same
way they name a script by its file stem.
