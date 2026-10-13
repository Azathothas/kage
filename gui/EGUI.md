# EGUI: the desktop client's stack record

This file is the standing record for the egui/eframe rewrite of
`kage-desktop`. It supersedes the decision section of `SPIKE.md` (the
spike report stays as history). The migration's full numbers live in
`specs/epics/E10-perf-and-size.md` and the web target's in
`specs/epics/E08-web-target.md`.

## Decision

The client is egui 0.36 on eframe with the glow renderer, Wayland and
X11 backends, and AccessKit wired in. The framework-free spine that the
old stack already owned carried over unchanged: transports, store
logic, gate, prefs, clock, timing. Everything view-shaped was ported
behind the same public behavior; the port checklist and its test
counts are in `specs/epics/E09-testing-and-ci.md`.

Renderer: glow, not wgpu. The wgpu feature stays available as a
fallback (decision D1 in the spike) but nothing in the shipped build
needs Vulkan, and the packaged artifact no longer bundles a Vulkan
loader or drivers.

## Pins and upgrades

- The lockfile is the pin. `egui`, `egui_extras`, `egui_virtual_list`,
  `egui_commonmark` and `egui_kittest` move together: they are all
  0.36-line minors and a bump of one is a bump of all.
- `wasm-bindgen` and friends are exact-pinned (`=0.2.129`) so the CLI
  version and the crate can never drift. Upgrade both in the same
  commit.
- Upgrade procedure: bump the minor across the five crates, run the
  full suite (`cargo test`), re-baseline the snapshot rig with
  `UPDATE_SNAPSHOTS=1 cargo test --test parity_rig`, re-run the NFR-01
  smoke (`KAGE_FRAME_TIMES=1`, see below) and re-measure the binary
  and wasm sizes against the E10 table. Record the new numbers in the
  E10 results before merging.

## Building

Native:

    cargo build --release          # desktop binary, target/release/kage-desktop
    cargo test                     # 413 tests, 21 binaries

The web bundle (stable toolchain, no nightly anywhere):

    cargo build --release --target wasm32-unknown-unknown --no-default-features
    wasm-bindgen --out-dir web --target web \
      target/wasm32-unknown-unknown/release/kage_desktop.wasm

`gui/web/` then holds what `kage serve --web-dir gui/web` serves:
`index.html`, `boot.js`, `kage_desktop.js`, `kage_desktop_bg.wasm`,
`favicon.svg`. The generated glue and wasm are gitignored; the
hand-written page, form and `serve_web.py` (an iterating helper, not
the contract) are tracked. CI builds the same two commands on the
pinned stable toolchain.

Smoke run, headless-capable:

    xvfb-run -a ./target/debug/kage-desktop --replay --smoke 5000 --stream

winit needs a display; `xvfb-run` is what CI uses. On a live Wayland
desktop, focus the smoke window as it maps: the compositor throttles
occluded surfaces to 1 fps, which fakes dropped ticks (see the E10
method notes).

## Measured baselines (E10, 2026-10-13)

| NFR | Budget | Measured |
|---|---|---|
| NFR-01 30 Hz ticks, 10s | >= 99% | every due tick (303/303 in a 10.1s window) |
| NFR-02 idle RSS, 5k rows | <= 120 MB | 157 MB median, no leak (accepted, see E10) |
| NFR-03 cold start, Wayland | <= 200 ms | 148 ms median |
| NFR-04 release binary | <= 48.7 MB | 21.4 MB stripped |
| NFR-05 wasm, gzip -9 | <= 2 MiB | 5.08 MiB (accepted, see E08) |
| NFR-06 clean / incremental debug | <= 60s / <= 10s | 24.6s / 5.3s |

Release profile: fat LTO (`lto = true`, `codegen-units = 1`),
`panic = "abort"`, stripped. Fat beat thin 21.4 MB / 1m38s against
23.0 MB / 1m31s on the finished shell, so E01's choice stands.
`wasm-opt -Oz` buys about 1% on the shipped wasm and is not part of
the pipeline; `opt-level = "z"` buys about 10% gzip and costs runtime
speed, documented as the lever in the E08 results.
