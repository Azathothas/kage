//! The window icon, decoded from the embedded brand art.
//!
//! eframe applies `ViewportBuilder::with_icon` on X11; Windows, macOS
//! and Wayland take the icon from the packaged resources (the `.ico`
//! embed and the hicolor set) instead. This module is therefore best
//! effort: a failed decode costs the icon, never the launch.

/// The pixel size the icon ships at, which window managers scale down.
const ICON_SIZE: u32 = 256;

/// The window icon for `ViewportBuilder::with_icon`: the embedded
/// dark-palette glyph resized to 256 px, or `None` when the art fails
/// to decode. Applies on X11 only.
#[must_use]
pub fn window_icon() -> Option<egui::IconData> {
    let decoded = image::load_from_memory(crate::assets::GLYPH_FACE_SHADOW_PNG)
        .ok()?
        .to_rgba8();
    let resized = image::imageops::resize(
        &decoded,
        ICON_SIZE,
        ICON_SIZE,
        image::imageops::FilterType::Lanczos3,
    );
    Some(egui::IconData {
        width: resized.width(),
        height: resized.height(),
        rgba: resized.into_raw(),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn embedded_brand_art_decodes_non_empty() {
        let icon = super::window_icon().expect("the embedded glyph decodes");
        assert_eq!(icon.width, 256);
        assert_eq!(icon.height, 256);
        assert!(!icon.rgba.is_empty());
    }
}
