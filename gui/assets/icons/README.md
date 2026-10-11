# Icons

Lucide SVGs vendored for the client, one file per icon the app draws.
Source: the `gpui-kit-assets` 0.7.0 crate copy of the Lucide catalog
(`assets/icons/`), pinned by the Cargo.lock of the E00 spike. License:
ISC (see LICENSE-LUCIDE); a portion of the catalog derives from the
Feather project, noted in that file. Icons are loaded through
egui_extras' SVG loader at runtime; nothing fetches from the network.
