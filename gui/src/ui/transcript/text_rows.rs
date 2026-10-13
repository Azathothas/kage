//! The transcript's text-bearing rows, drawn with egui widgets.
//!
//! One renderer per row kind that carries words rather than tool
//! output: the user bubble, the prompt the engine built from agent
//! text, the assistant reply, the compaction summary, the thinking
//! block, the notice, the engine note, the compaction row, the
//! permission decision and the working row. The tool, group, plan,
//! agent and changes rows live in their own modules.
//!
//! Each renderer takes `&mut egui::Ui` and the data its row carries,
//! draws with the design's tokens and hands back an [`egui::Response`]
//! plus, where the row offers an action, the action itself: the caller
//! owns the store, the composer and the clipboard, so a renderer never
//! reaches past its own rect. The click that opens or closes a body
//! comes back through the returned response.

use std::f32::consts::TAU;
use std::time::Duration;

use kage_client::agent_text::{self, AgentText, Report};
use kage_client::wire::NoticeTone;
use kage_client::{Session, TranscriptItem};

use super::head::{self, Head, Lead, Target};
use super::model::last_sentence;
use crate::theme::{
    FAMILY_BODY_REGULAR, FAMILY_MONO_REGULAR, FS_2XS, FS_BASE, FS_SM, FS_XS, Palette, R_FULL, SP_1,
    SP_2, SP_3, SP_4, SP_5, family,
};

/// The activity row's height, the design's minimum row height.
const ROW_H: f32 = 28.0;
/// The user bubble's corner radius.
const BUBBLE_R: f32 = 18.0;
/// The user bubble's horizontal padding.
const BUBBLE_PX: f32 = 14.0;
/// The user bubble's vertical padding.
const BUBBLE_PY: f32 = 9.0;
/// The bubble's widest share of the column.
const BUBBLE_MAX: f32 = 0.85;
/// The left indent of a detail box or an expanded body.
const DETAIL_INDENT: f32 = 22.0;
/// The line height of the thinking body.
const THINK_LINE: f32 = 20.0;
/// The icon size of the design's small rows.
const ICON_SM: f32 = 14.0;
/// The icon size of the design's extra small rows.
const ICON_XS: f32 = 12.0;
/// One turn of the working spinner.
const SPIN_TURN: Duration = Duration::from_millis(900);
/// The steered bubble's dashed border, one dash and one gap.
const DASH: f32 = 4.0;

/// What a user row's hover actions ask the caller to do.
///
/// The renderer owns the drawing; the caller owns the composer and the
/// clipboard, so the actions travel back out rather than reaching into
/// the shell from inside a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserAction {
    /// Copy the message text to the clipboard.
    Copy(String),
    /// Put the message text in the composer to edit and resend.
    Edit(String),
    /// Fork a new session from this message's turn.
    Fork,
    /// Drop this message and everything after it.
    Rewind,
}

/// The names of a prompt's non-text blocks, for its attachment chips:
/// images and audio by their kind, a resource link by its name and an
/// embedded resource by the file its uri names.
fn attachment_names(content: &[kage_client::wire::ContentBlock]) -> Vec<String> {
    use kage_client::wire::ContentBlock;

    content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Image(_) => Some("image".to_owned()),
            ContentBlock::Audio(_) => Some("audio".to_owned()),
            ContentBlock::ResourceLink(link) => Some(link.name.clone()),
            ContentBlock::Resource(resource) => Some(
                resource.resource["uri"]
                    .as_str()
                    .map_or("resource", |uri| uri.rsplit('/').next().unwrap_or(uri))
                    .to_owned(),
            ),
            _ => None,
        })
        .collect()
}

/// The monospace font of the design's paths and commands.
fn mono(size: f32) -> egui::FontId {
    egui::FontId::new(size, family(FAMILY_MONO_REGULAR))
}

/// One icon of the design's catalog at its extra small size.
fn icon_xs(ui: &mut egui::Ui, icon: crate::icons::Icon, color: egui::Color32) -> egui::Response {
    crate::icons::icon(ui, icon, ICON_XS, color)
}

/// The design's hairline: a one pixel rule in the faintest line token.
fn rule(ui: &mut egui::Ui, palette: &Palette) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(0.0, 1.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, palette.subtle);
}

/// The body an expanded row drops under its head: indented, ruled on
/// its left edge in the design's border token.
fn detail_frame(ui: &mut egui::Ui, palette: &Palette, contents: impl FnOnce(&mut egui::Ui)) {
    let rule_x = ui.cursor().left() + DETAIL_INDENT + 1.0;
    let top = ui.cursor().top();
    let body = egui::Rect::from_min_max(
        egui::pos2(rule_x + 1.0 + SP_5, top),
        ui.max_rect().right_bottom(),
    );
    let inner = ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(body)
            .layout(egui::Layout::top_down(egui::Align::Min)),
        contents,
    );
    ui.painter().vline(
        rule_x,
        egui::Rangef::new(top, inner.response.rect.bottom()),
        egui::Stroke::new(2.0, palette.line),
    );
}

/// The steered bubble's border, drawn as dashes because egui strokes
/// carry no dash pattern.
fn dashed_rect(ui: &egui::Ui, rect: egui::Rect, radius: f32, color: egui::Color32) {
    let stroke = egui::Stroke::new(1.0, color);
    let dash = |from: egui::Pos2, to: egui::Pos2| {
        let along = to - from;
        let length = along.length();
        if length <= 0.0 {
            return Vec::new();
        }
        let step = along / length * DASH;
        let mut points = Vec::new();
        let mut at = from;
        let mut on = true;
        let mut drawn = 0.0;
        while drawn < length {
            let next = at + step;
            if on {
                points.push(at);
                points.push(next.min(to));
            }
            at = next;
            drawn += DASH;
            on = !on;
        }
        points
    };
    let inset = egui::Rect::from_min_max(
        rect.min + egui::vec2(radius, radius),
        rect.max - egui::vec2(radius, radius),
    );
    let top_left = inset.left_top();
    let top_right = inset.right_top();
    let bottom_right = inset.right_bottom();
    let bottom_left = inset.left_bottom();
    let sides = [
        (egui::pos2(rect.left(), inset.top()), top_left),
        (top_left, top_right),
        (top_right, egui::pos2(rect.right(), inset.top())),
        (
            egui::pos2(rect.right(), inset.top()),
            egui::pos2(rect.right(), inset.bottom()),
        ),
        (egui::pos2(rect.right(), inset.bottom()), bottom_right),
        (bottom_right, bottom_left),
        (bottom_left, egui::pos2(rect.left(), inset.bottom())),
        (
            egui::pos2(rect.left(), inset.bottom()),
            egui::pos2(rect.left(), inset.top()),
        ),
    ];
    for (from, to) in sides {
        let points = dash(from, to);
        for pair in points.chunks(2) {
            if let [from, to] = pair {
                ui.painter().line_segment([*from, *to], stroke);
            }
        }
    }
}

/// The working spinner: an arc turning once every [`SPIN_TURN`], asking
/// for the frames it turns through.
fn spinner(ui: &mut egui::Ui, color: egui::Color32) -> egui::Response {
    ui.ctx().request_repaint();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ICON_SM, ICON_SM), egui::Sense::hover());
    let turn =
        (ui.ctx().input(|i| i.time) % SPIN_TURN.as_secs_f64()) as f32 / SPIN_TURN.as_secs_f32();
    let radius = ICON_SM / 2.0 - 1.0;
    let center = rect.center();
    let steps = 24;
    let mut points = Vec::with_capacity(steps + 1);
    for step in 0..=steps {
        let angle = turn * TAU + TAU * 0.75 * (step as f32 / steps as f32);
        points.push(center + egui::vec2(radius * angle.cos(), radius * angle.sin()));
    }
    ui.painter().line(points, egui::Stroke::new(2.0, color));
    response
}

/// A user message: its attachments, a right-aligned bubble, the steered
/// tag when it rode into a running turn, and hover actions.
pub fn render_user(
    ui: &mut egui::Ui,
    text: &str,
    attached: &[String],
    steered: bool,
    lineage: bool,
) -> (egui::Response, Vec<UserAction>) {
    let palette = Palette::active_of(ui.ctx());
    let mut actions = Vec::new();
    let top = ui.cursor().top();
    let row = ui
        .with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if !attached.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = SP_3;
                    for name in attached {
                        egui::Frame::NONE
                            .stroke(egui::Stroke::new(1.0, palette.line))
                            .corner_radius(egui::CornerRadius::same(R_FULL as u8))
                            .inner_margin(egui::Margin::symmetric(SP_4 as i8, 0))
                            .show(ui, |ui| {
                                ui.set_min_height(24.0);
                                ui.spacing_mut().item_spacing.x = SP_2;
                                icon_xs(ui, crate::icons::Icon::Paperclip, palette.muted);
                                ui.label(
                                    egui::RichText::new(name).size(FS_XS).color(palette.muted),
                                );
                            });
                    }
                });
                ui.add_space(SP_3);
            }
            let bubble = egui::Frame::NONE
                .fill(if steered {
                    egui::Color32::TRANSPARENT
                } else {
                    palette.bubble
                })
                .corner_radius(egui::CornerRadius::same(BUBBLE_R as u8))
                .inner_margin(egui::Margin::symmetric(BUBBLE_PX as i8, BUBBLE_PY as i8))
                .show(ui, |ui| {
                    ui.set_max_width((ui.available_width() * BUBBLE_MAX).max(1.0));
                    ui.label(
                        egui::RichText::new(text)
                            .size(FS_BASE)
                            .line_height(Some(FS_BASE * 1.5))
                            .color(if steered {
                                palette.ink
                            } else {
                                palette.ink_strong
                            }),
                    );
                });
            if steered {
                dashed_rect(ui, bubble.response.rect, BUBBLE_R, palette.line_strong);
                ui.add_space(SP_2);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = SP_2;
                    icon_xs(ui, crate::icons::Icon::CornerDownRight, palette.faint);
                    ui.label(
                        egui::RichText::new("steered into the running turn")
                            .size(FS_2XS)
                            .color(palette.faint),
                    );
                });
            }
            ui.add_space(SP_2);
            let (strip, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), head::ICON_BTN),
                egui::Sense::hover(),
            );
            let over = egui::Rect::from_min_max(egui::pos2(strip.left(), top), strip.max);
            if ui.rect_contains_pointer(over) {
                let mut buttons = vec![
                    (
                        crate::icons::Icon::Copy,
                        "Copy",
                        UserAction::Copy(text.to_owned()),
                    ),
                    (
                        crate::icons::Icon::Pencil,
                        "Edit and resend",
                        UserAction::Edit(text.to_owned()),
                    ),
                ];
                if lineage {
                    buttons.push((
                        crate::icons::Icon::GitFork,
                        "Fork from here",
                        UserAction::Fork,
                    ));
                    buttons.push((
                        crate::icons::Icon::Undo2,
                        "Rewind to here",
                        UserAction::Rewind,
                    ));
                }
                let step = head::ICON_BTN + SP_1;
                let mut x = strip.right() - step * buttons.len() as f32 + SP_1;
                for (glyph, tip, action) in buttons {
                    let at = egui::Rect::from_min_size(
                        egui::pos2(x, strip.top()),
                        egui::vec2(head::ICON_BTN, head::ICON_BTN),
                    );
                    if head::icon_button(ui, at, glyph, tip, palette).clicked() {
                        actions.push(action);
                    }
                    x += step;
                }
            }
        })
        .response;
    (row, actions)
}

/// Draws one agent report, the card the caller's agent module owns.
pub type ReportDraw<'a> = &'a mut dyn FnMut(&mut egui::Ui, usize, &Report);

/// Draws the delivered prompt: the typed words as their own bubble when
/// there are any, then each agent text part in order.
///
/// `report` draws one agent report, the card the caller's agent module
/// owns; `None` leaves a quiet line naming the agent instead, so no
/// delivered part is dropped.
pub fn render_delivered(
    ui: &mut egui::Ui,
    words: &str,
    parts: &[AgentText],
    mut report: Option<ReportDraw<'_>>,
    lineage: bool,
) -> (egui::Response, Vec<UserAction>) {
    let palette = Palette::active_of(ui.ctx());
    let mut actions = Vec::new();
    let mut row = None;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = SP_3;
        if !words.is_empty() {
            let (words_row, words_actions) = render_user(ui, words, &[], false, lineage);
            row = Some(words_row);
            actions.extend(words_actions);
        }
        for (n, part) in parts.iter().enumerate() {
            match part {
                AgentText::Report(report_text) => match report.as_mut() {
                    Some(draw) => draw(ui, n, report_text),
                    None => {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} {}",
                                    report_text.name, report_text.state
                                ))
                                .size(FS_XS)
                                .color(palette.faint),
                            );
                        });
                    }
                },
                AgentText::Mail { from, body } => {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = SP_3;
                        icon_xs(ui, crate::icons::Icon::MessageSquare, palette.faint);
                        ui.label(
                            egui::RichText::new(format!("Message from {from}: {body}"))
                                .size(FS_XS)
                                .color(palette.muted),
                        );
                    });
                }
            };
            row = Some(ui.response());
        }
    });
    let response =
        row.unwrap_or_else(|| ui.allocate_response(egui::Vec2::ZERO, egui::Sense::hover()));
    (response, actions)
}

/// Assistant reply text as markdown, with a caret while it streams. The
/// pipeline owns the parse caches, so the caller threads its own in.
pub fn render_assistant(
    ui: &mut egui::Ui,
    pipeline: &mut super::markdown::Pipeline,
    ix: usize,
    text: &str,
    live: bool,
) -> egui::Response {
    let palette = Palette::active_of(ui.ctx());
    let id = format!("assistant-{ix}");
    let top = ui.cursor().top();
    pipeline.show(&id, text, live, ui);
    if live {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(7.0, 14.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::ZERO, palette.ink_strong);
        return response;
    }
    // A finished reply offers the hover copy: the markdown stripped to
    // plain text, so a paste carries the words and not the syntax.
    let (rect, response) = ui.allocate_exact_size(egui::vec2(0.0, 0.0), egui::Sense::hover());
    let _ = rect;
    crate::ui::copy::hover_copy(ui, egui::Rect::everything_below(top), text);
    response
}

/// The summary a compaction left in place of the history before it,
/// folded under one line and read as markdown when opened.
pub fn render_summary(
    ui: &mut egui::Ui,
    pipeline: &mut super::markdown::Pipeline,
    ix: usize,
    summary: &str,
    expanded: bool,
) -> egui::Response {
    let palette = Palette::active_of(ui.ctx());
    let response = head::show(
        ui,
        Head {
            lead: Lead::Icon(crate::icons::Icon::Layers, palette.muted),
            verb: "Context summary".to_owned(),
            verb_color: palette.ink,
            target: None,
            meta: Vec::new(),
            chevron: Some(expanded),
            nested: false,
        },
    );
    if expanded {
        ui.add_space(SP_2);
        let id = format!("summary-{ix}");
        detail_frame(ui, palette, |ui| {
            pipeline.show(&id, summary, false, ui);
        });
    }
    response
}

/// Assistant reasoning: a live peek, then a collapsible body.
pub fn render_thinking(
    ui: &mut egui::Ui,
    text: &str,
    live: bool,
    expanded: bool,
    duration: Option<Duration>,
) -> egui::Response {
    let palette = Palette::active_of(ui.ctx());
    let label = match (live, duration) {
        (true, _) => "Thinking".to_owned(),
        (false, Some(span)) => format!("Thought for {}s", span.as_secs().max(1)),
        (false, None) => "Thought".to_owned(),
    };
    let peek = (live && !expanded).then(|| Target {
        text: last_sentence(text),
        font: egui::FontId::new(FS_SM, family(FAMILY_BODY_REGULAR)),
        color: palette.faint,
        italics: true,
    });
    let response = head::show(
        ui,
        Head {
            lead: if live {
                Lead::Eclipse
            } else {
                Lead::Icon(crate::icons::Icon::Lightbulb, palette.muted)
            },
            verb: label,
            verb_color: if live { palette.muted } else { palette.ink },
            target: peek,
            meta: Vec::new(),
            chevron: Some(expanded),
            nested: false,
        },
    );
    if expanded {
        ui.add_space(SP_2);
        detail_frame(ui, palette, |ui| {
            ui.label(
                egui::RichText::new(text)
                    .size(FS_SM)
                    .color(palette.muted)
                    .line_height(Some(THINK_LINE)),
            );
        });
    }
    response
}

/// The design's notice row: the text between two flanking hairlines, in
/// the tone's color. An error rides in a danger pill.
pub fn render_notice(ui: &mut egui::Ui, ix: usize, tone: NoticeTone, text: &str) -> egui::Response {
    let palette = Palette::active_of(ui.ctx());
    let (color, pill) = match tone {
        NoticeTone::Info => (palette.faint, false),
        NoticeTone::Warn => (palette.warn, false),
        NoticeTone::Error => (palette.danger, true),
        NoticeTone::Success => (palette.ok, false),
    };
    let response = ui.allocate_response(egui::Vec2::ZERO, egui::Sense::hover());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = SP_4;
        ui.set_min_height(ROW_H);
        rule(ui, palette);
        egui::Frame::NONE
            .fill(if pill {
                palette.danger_soft
            } else {
                egui::Color32::TRANSPARENT
            })
            .corner_radius(egui::CornerRadius::same(R_FULL as u8))
            .inner_margin(egui::Margin::symmetric(10, SP_2 as i8))
            .show(ui, |ui| {
                ui.label(egui::RichText::new(text).size(FS_XS).color(color));
            });
        rule(ui, palette);
    });
    let _ = ix;
    response
}

/// A note the engine added as a user message, such as swarm mode
/// switching on: one quiet line, not a bubble the user typed.
pub fn render_engine_note(ui: &mut egui::Ui, ix: usize, note: &str) -> egui::Response {
    let palette = Palette::active_of(ui.ctx());
    let response = ui.allocate_response(egui::Vec2::ZERO, egui::Sense::hover());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = SP_2;
        ui.set_min_height(ROW_H);
        icon_xs(ui, crate::icons::Icon::Info, palette.faint);
        ui.label(egui::RichText::new(note).size(FS_XS).color(palette.faint));
    });
    let _ = ix;
    response
}

/// A compaction row with the tokens the summary saved, between the same
/// flanking hairlines the notices use.
pub fn render_compaction(
    ui: &mut egui::Ui,
    ix: usize,
    kept: u64,
    before: u64,
    after: u64,
) -> egui::Response {
    let palette = Palette::active_of(ui.ctx());
    let response = ui.allocate_response(egui::Vec2::ZERO, egui::Sense::hover());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = SP_4;
        ui.set_min_height(ROW_H);
        rule(ui, palette);
        ui.label(
            egui::RichText::new(format!(
                "context compacted: kept {kept} turns, {before} -> {after} tokens"
            ))
            .size(FS_XS)
            .color(palette.faint),
        );
        rule(ui, palette);
    });
    let _ = ix;
    response
}

/// The one-line record of an answered permission ask: what the ask was
/// about, the chosen label as offered, and the feedback quoted when it
/// rode along.
pub fn render_decision(
    ui: &mut egui::Ui,
    ix: usize,
    subject: &str,
    label: &str,
    allowed: bool,
    feedback: Option<&str>,
) -> egui::Response {
    let palette = Palette::active_of(ui.ctx());
    let color = if allowed { palette.ok } else { palette.danger };
    let glyph = if allowed {
        crate::icons::Icon::ShieldCheck
    } else {
        crate::icons::Icon::ShieldX
    };
    let response = ui.allocate_response(egui::Vec2::ZERO, egui::Sense::hover());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = SP_4;
        ui.set_min_height(ROW_H);
        icon_xs(ui, glyph, color);
        ui.label(egui::RichText::new(label).size(FS_XS).color(palette.muted));
        ui.add(
            egui::Label::new(
                egui::RichText::new(subject)
                    .font(mono(12.0))
                    .color(palette.ink),
            )
            .truncate(),
        );
        if let Some(feedback) = feedback {
            ui.label(
                egui::RichText::new(format!("\"{feedback}\""))
                    .size(FS_XS)
                    .color(palette.faint)
                    .italics(),
            );
        }
    });
    let _ = ix;
    response
}

/// The working row: a spinner and a quiet label while the run goes on
/// with nothing streaming yet.
pub fn render_waiting(ui: &mut egui::Ui) -> egui::Response {
    let palette = Palette::active_of(ui.ctx());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = SP_4;
        ui.set_min_height(ROW_H);
        spinner(ui, palette.accent);
        ui.label(
            egui::RichText::new("Working")
                .size(FS_SM)
                .color(palette.muted),
        );
    })
    .response
}

/// What one prompt delivered as agent text, for the rows that read it:
/// the typed words and the parts after them, or `None` for a prompt
/// that is plain words.
#[must_use]
pub fn split_prompt(text: &str) -> Option<(&str, Vec<AgentText>)> {
    agent_text::split(text)
}

/// What a note the engine added as a user message says, or `None` for
/// any other text.
#[must_use]
pub fn note_of(text: &str) -> Option<String> {
    agent_text::engine_note(text)
}

/// The body of the summary the engine wrote in place of compacted
/// history, or `None` for any other text.
#[must_use]
pub fn summary_of(text: &str) -> Option<&str> {
    agent_text::compaction_summary(text)
}

/// The names of a prompt's non-text blocks, for its attachment chips.
#[must_use]
pub fn attachments(session: &Session, ix: usize) -> Vec<String> {
    match session.items.get(ix) {
        Some(TranscriptItem::User { content, .. }) => attachment_names(content),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BUBBLE_MAX, DASH, ICON_SM, ROW_H, SPIN_TURN, THINK_LINE, UserAction, attachment_names, mono,
    };
    use kage_client::wire::{BlobContent, ContentBlock};

    #[test]
    fn attachment_names_read_each_non_text_block() {
        let image = ContentBlock::Image(BlobContent {
            data: String::new(),
            mime_type: "image/png".into(),
            uri: None,
        });
        let audio = ContentBlock::Audio(BlobContent {
            data: String::new(),
            mime_type: "audio/wav".into(),
            uri: None,
        });
        assert_eq!(
            attachment_names(&[image, audio]),
            ["image".to_owned(), "audio".to_owned()]
        );
    }

    #[test]
    fn attachment_names_ignore_text_and_unknown_blocks() {
        let plain = [ContentBlock::text("plain"), ContentBlock::Unknown];
        assert!(attachment_names(&plain).is_empty());
    }

    #[test]
    fn an_embedded_resource_reads_the_file_its_uri_names() {
        let resource = ContentBlock::Resource(kage_client::wire::EmbeddedResource {
            resource: serde_json::json!({"uri": "file:///w/src/main.rs"}),
        });
        assert_eq!(attachment_names(&[resource]), ["main.rs".to_owned()]);
    }

    #[test]
    fn the_user_row_offers_copy_and_edit_for_its_text() {
        let copy = UserAction::Copy("hello".to_owned());
        let edit = UserAction::Edit("hello".to_owned());
        assert_eq!(copy, UserAction::Copy("hello".to_owned()));
        assert_eq!(edit, UserAction::Edit("hello".to_owned()));
        assert_ne!(copy, edit);
    }

    #[test]
    fn the_design_metrics_hold_their_tokens() {
        const _: () = assert!(BUBBLE_MAX > 0.5 && BUBBLE_MAX < 1.0);
        assert_eq!(ROW_H, 28.0);
        assert_eq!(ICON_SM, 14.0);
        assert_eq!(THINK_LINE, 20.0);
        assert_eq!(SPIN_TURN.as_millis(), 900);
        assert_eq!(DASH, 4.0);
        assert_eq!(mono(12.0).size, 12.0);
    }
}
