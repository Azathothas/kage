//! Media the assistant produced, drawn in the transcript.
//!
//! An image is decoded from the transcript's base64 once and handed to
//! egui under a `bytes://` URI named after its content, so egui's image
//! loader decodes it once and caches the texture; later frames only
//! look the URI up. Media egui cannot draw, such as audio, shows as a
//! line naming its type and size.

use std::hash::{Hash as _, Hasher as _};

use base64::Engine as _;

use crate::theme::{FAMILY_BODY_REGULAR, FS_XS, Palette, R_MD, SP_1, family};

/// The tallest an image is drawn, in points.
const MAX_H: f32 = 420.0;

/// The widest an image is drawn, in points, on a wide column.
const MAX_W: f32 = 640.0;

/// One media item: the image scaled to the column with its type and
/// size under it and a button to save it into the workspace, or the
/// line alone for media egui cannot draw. The flag says the button was
/// pressed.
pub fn render_media(ui: &mut egui::Ui, mime: &str, data: &str) -> (egui::Response, bool) {
    let palette = Palette::active_of(ui.ctx());
    let caption = format!("{mime}, {}", size_label(data.len() / 4 * 3));
    if !mime.starts_with("image/") {
        return (caption_line(ui, &caption, palette.faint), false);
    }
    let uri = media_uri(mime, data);
    let seen = egui::Id::new(("kage-media", &uri));
    if !ui.ctx().data(|d| d.get_temp::<bool>(seen).unwrap_or(false)) {
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) else {
            let line = caption_line(
                ui,
                &format!("{caption}: the image data is damaged"),
                palette.danger,
            );
            return (line, false);
        };
        ui.ctx().include_bytes(uri.clone(), bytes);
        ui.ctx().data_mut(|d| d.insert_temp(seen, true));
    }
    let mut save = false;
    let response = ui
        .vertical(|ui| {
            let width = ui.available_width().min(MAX_W);
            ui.add(
                egui::Image::new(uri)
                    .max_width(width)
                    .max_height(MAX_H)
                    .corner_radius(R_MD),
            );
            ui.add_space(SP_1);
            ui.horizontal(|ui| {
                caption_line(ui, &caption, palette.faint);
                save = crate::ui::settings_config::btn_sm(
                    ui,
                    "Save to workspace",
                    Some(crate::icons::Icon::Download),
                    crate::ui::settings_config::BtnTone::Plain,
                    palette,
                )
                .on_hover_text("Write it under generated/ in the session's folder")
                .clicked();
            });
        })
        .response;
    (response, save)
}

/// The images a tool call's content carries, as `(mime, base64)`.
#[must_use]
pub fn tool_images(call: &kage_client::ToolCallItem) -> Vec<(&str, &str)> {
    call.content
        .iter()
        .filter_map(|content| match content {
            kage_client::wire::ToolCallContent::Content(chunk) => match &chunk.content {
                kage_client::wire::ContentBlock::Image(blob) => {
                    Some((blob.mime_type.as_str(), blob.data.as_str()))
                }
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// A `bytes://` URI that names the image by its content, so two images
/// never share a texture and one image keeps its texture across frames
/// and rebuilds. Hashing the ends and the length keeps it cheap on
/// megabytes of base64.
fn media_uri(mime: &str, data: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    data.len().hash(&mut hasher);
    data.as_bytes()[..data.len().min(512)].hash(&mut hasher);
    data.as_bytes()[data.len().saturating_sub(512)..].hash(&mut hasher);
    let ext = mime.split_once('/').map_or("png", |(_, sub)| {
        sub.split(['+', ';']).next().unwrap_or("png")
    });
    format!("bytes://kage-media/{:016x}.{ext}", hasher.finish())
}

/// `bytes` as a short size, such as `618 kB`.
fn size_label(bytes: usize) -> String {
    const KB: usize = 1000;
    match bytes {
        b if b < KB => format!("{b} B"),
        b if b < KB * KB => format!("{} kB", b / KB),
        b => format!("{:.1} MB", b as f64 / (KB * KB) as f64),
    }
}

fn caption_line(ui: &mut egui::Ui, text: &str, color: egui::Color32) -> egui::Response {
    ui.label(
        egui::RichText::new(text)
            .font(egui::FontId::new(FS_XS, family(FAMILY_BODY_REGULAR)))
            .color(color),
    )
}

#[cfg(test)]
mod tests {
    use super::{media_uri, size_label};

    #[test]
    fn an_image_uri_follows_its_content_and_type() {
        let a = media_uri("image/png", "aaaa");
        assert!(
            a.starts_with("bytes://kage-media/") && a.ends_with(".png"),
            "{a}"
        );
        assert_eq!(a, media_uri("image/png", "aaaa"));
        assert_ne!(a, media_uri("image/png", "aaab"));
        assert!(media_uri("image/svg+xml", "x").ends_with(".svg"));
    }

    #[test]
    fn sizes_read_short() {
        assert_eq!(size_label(512), "512 B");
        assert_eq!(size_label(618_432), "618 kB");
        assert_eq!(size_label(2_500_000), "2.5 MB");
    }
}
