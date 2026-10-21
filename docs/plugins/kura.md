# kura

**kura** (蔵, storehouse) is kage's plugin registry. It holds reviewed
[packages](/plugins/packages), and `kage plugin` installs them by name.
The packages live in
[QaidVoid/kage-kura](https://github.com/QaidVoid/kage-kura), which is
also where you add your own.

## finding a package

```sh
kage plugin search web          # name, description and tags
kage plugin info web            # releases, capabilities, links
```

Both read the registry's index, cached for an hour in
`~/.cache/kage/kura/`. `--refresh` fetches it again. When the registry
cannot be reached, a cached index stands in and kage says so.

## installing

```sh
kage plugin install web         # the newest release this kage can load
kage plugin install web@0.2.0   # one release
```

An install:

1. picks the newest release that is not yanked and whose `api` this
   kage provides;
2. downloads every file the index lists for it, and checks each
   file's size and sha256 against the index;
3. checks that the package's `plugin.toml` names the same package and
   version;
4. writes it to a hidden staging directory and renames it into place,
   so a running session never loads half a package;
5. records it in `kura.lock` beside your plugins.

Anything that does not match the index stops the install before a file
is written.

An install never replaces what you own unless you pass `--force`: a
script of the same name, a package you put there yourself, or an
installed package you edited. A package that is a symlink, such as a
checkout you are developing, is never replaced.

A running TUI loads the new package on its own. Other sessions load it
when they next start.

## capabilities

kura packages declare their [capabilities](/plugins/capabilities) in
`plugin.toml`, and an install lists the ones not granted yet:

```text
installed web 0.2.0
web asks for:
  net           make outbound HTTP requests
Grant them? [y/N]
```

Yes writes them to `[plugins.capabilities]`. `--grant` grants without
asking, and without a terminal nothing is granted. A package can never
hold more than its manifest declares, whatever the config grants.

```sh
kage plugin grant web           # everything web declares
kage plugin grant web net       # just net
kage plugin revoke web          # take everything back
```

## keeping packages current

```sh
kage plugin list                # what is installed, and what has an update
kage plugin update              # every package installed from kura
kage plugin update web          # just web
```

`list` shows each plugin's kind, version and source (`kura` or
`local`), and flags one that is disabled, has an update, was yanked,
or was edited since it was installed. `update` skips an edited package
unless you pass `--force`, which discards your edits. When a release
you have is yanked, `update` says why and how to move off it.

## turning plugins off

```sh
kage plugin disable web         # keep it, but do not load it
kage plugin enable web
kage plugin remove web          # delete it (a linked package loses only the link)
```

`disable` and `enable` edit `[plugins] disabled` in your config. They
work for scripts and packages alike, installed from kura or not.

## kura.lock

`kura.lock` in the plugins directory records each package installed
from a registry: its version, the registry, the commit its files came
from, and the sha256 of every file. It is how kage tells kura's
packages from your own, and an untouched package from an edited one.
Keep it with your plugins if you sync them between machines. Edit it
with `kage plugin`, not by hand.

## another registry

`[plugins] registry` in your user config points `kage plugin` at
another index, for a mirror or a private registry built with kura's
tooling:

```toml
[plugins]
registry = "https://plugins.example.org/"
```

A project's `.kage/config.toml` cannot set it.
