//! The embedded brand art: logos and welcome art, included from
//! `assets/brand/`. The icon catalog (Lucide SVGs) arrives with S01.4.

/// The brand glyph, dark palette.
pub(crate) static GLYPH: &[u8] = include_bytes!("../assets/brand/kage-glyph.svg");
/// The brand glyph face, dark palette, as PNG.
pub(crate) static GLYPH_FACE_SHADOW_PNG: &[u8] =
    include_bytes!("../assets/brand/kage-glyph-shadow.png");
/// The brand glyph face, light palette, as PNG.
pub(crate) static GLYPH_FACE_DAWN_PNG: &[u8] =
    include_bytes!("../assets/brand/kage-glyph-dawn.png");
/// The welcome screen's glow backdrop, dark palette.
pub(crate) static WELCOME_GLOW_PNG: &[u8] = include_bytes!("../assets/brand/welcome-glow.png");
/// The welcome screen's glow backdrop, light palette.
pub(crate) static WELCOME_GLOW_DAWN_PNG: &[u8] =
    include_bytes!("../assets/brand/welcome-glow-dawn.png");
