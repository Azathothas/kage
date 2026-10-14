//! The welcome pane: what the chat column shows while no session is
//! open. The wordmark over the pane's glow, the project pill and its
//! menu, the four suggestion cards and the hint line, as the web
//! client's `.welcome` and the reference shell drew them.
//!
//! The composer stays mounted at the column's bottom in the shell, so
//! a card only fills its draft: the [`WelcomeEvent::Fill`] event
//! carries the text and the one option the card holds for the session
//! the send opens, which [`Store::open_with_prompt`] then rides.

use crate::assets::GLYPH;
#[cfg(not(target_arch = "wasm32"))]
use crate::assets::{
    GLYPH_FACE_DAWN_PNG, GLYPH_FACE_SHADOW_PNG, WELCOME_GLOW_DAWN_PNG, WELCOME_GLOW_PNG,
};
use crate::icons::Icon;
use crate::icons::PaintSnapped as _;
use crate::store::Store;
use crate::theme::{
    FAMILY_BODY_EXTRABOLD, FAMILY_BODY_REGULAR, FAMILY_BODY_SEMIBOLD, FS_2XS, FS_SM, FS_XS,
    Palette, R_FULL, R_LG, R_MD, WELCOME_PAD_BOTTOM_SHARE, WELCOME_PAD_TOP, WELCOME_W, family,
};

/// The wordmark glyph's size, as the design draws it.
const GLYPH_PX: f32 = 76.0;
/// The wordmark's hard offset shadow, glyph and name alike.
const SHADOW_OFFSET: f32 = 6.0;
const NAME_OFFSET: f32 = 4.2;
/// The wordmark's size, as the design draws it.
const NAME_PX: f32 = 60.0;
/// The gap between the glyph and the name.
const WORDMARK_GAP: f32 = 16.0;
/// How far below the wordmark the rest of the stack sits.
const WORDMARK_MB: f32 = 30.0;
/// The pill's left inset in the column, as the reference shell indents it.
const PILL_ML: f32 = 12.0;
/// The gap between the pill and the composer under it.
const PILL_TO_COMPOSER: f32 = 8.0;
/// The gap between the composer and the cards.
const COMPOSER_TO_CARDS: f32 = 16.0;
/// How far the project menu hangs under the pill.
const MENU_DROP: f32 = 7.0;
/// How far the project menu sits in from the pill's left edge, where
/// the reference anchors it.
const MENU_INDENT: f32 = 10.0;
/// The project menu's width, as the reference shell draws it.
const MENU_W: f32 = 320.0;
/// How many projects the menu lists, as the reference shell took them.
const MENU_ROWS: usize = 8;
/// The viewport width under which the cards stack in one column.
const NARROW: f32 = 860.0;

/// One suggestion card: what it fills the draft with and the one
/// config option it sets, when the card carries a mode.
pub struct Suggestion {
    /// The stable id, for the rig and the hover cache.
    id: &'static str,
    /// The card's icon.
    icon: Icon,
    /// The text the card puts in the draft.
    text: &'static str,
    /// The option the card turns on, as the web client's cards do:
    /// swarm for the audit, plan for the planning prompt. `None` for
    /// the plain suggestions, which only fill the draft.
    option: Option<(&'static str, &'static str)>,
    /// The tag pill the card trails, if any: `swarm` or `plan`.
    tag: Option<&'static str>,
}

/// The cards of the welcome pane, the web client's four suggestions.
/// Every option here is one the agent can advertise; a card sets
/// nothing the session was never offered.
const SUGGESTIONS: [Suggestion; 4] = [
    Suggestion {
        id: "welcome-card-fix",
        icon: Icon::Lightbulb,
        text: "Find the failing tests and fix them",
        option: None,
        tag: None,
    },
    Suggestion {
        id: "welcome-card-swarm",
        icon: Icon::Waypoints,
        text: "Review every module for unhandled errors",
        option: Some(("swarm", "on")),
        tag: Some("swarm"),
    },
    Suggestion {
        id: "welcome-card-plan",
        icon: Icon::ListTodo,
        text: "Plan the next change before touching code",
        option: Some(("mode", "plan")),
        tag: Some("plan"),
    },
    Suggestion {
        id: "welcome-card-delegate",
        icon: Icon::Bot,
        text: "Delegate a docs check to subagents",
        option: None,
        tag: None,
    },
];

/// What the pane asked the shell for this frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WelcomeEvent {
    /// A card was clicked: its text fills the composer draft, and the
    /// option it carries is held until the send opens the session.
    Fill {
        /// The prompt the card suggests.
        text: &'static str,
        /// The option to hold, when the card carries one.
        option: Option<(&'static str, &'static str)>,
    },
    /// The pill asked for the folder dialog.
    OpenFolder,
}

/// The welcome pane's state: whether the project menu shows, and the
/// stack height the last frame measured, which centers the stack
/// without a second layout pass.
#[derive(Debug, Default)]
pub struct Welcome {
    /// Whether the project menu shows.
    project_open: bool,
    /// The stack height the last frame drew, seeded with a first-frame
    /// estimate that one frame corrects.
    stack_h: f32,
}

impl Welcome {
    /// A welcome pane with its menu closed.
    pub fn new() -> Self {
        Self {
            project_open: false,
            stack_h: 420.0,
        }
    }

    /// Draws the pane over the column's free area and reports what the
    /// cards, the pill and the hosted composer asked for. The glow
    /// paints over the whole pane, and the composer mounts inside the
    /// stack between the pill and the cards, as the reference pane
    /// hosts it.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        store: &mut Store,
        composer: &mut crate::ui::composer::Composer,
        commands: &[serde_json::Value],
        running: bool,
        enter_sends: bool,
    ) -> (Vec<WelcomeEvent>, crate::ui::composer::ComposerEvent) {
        let mut events = Vec::new();
        let full = ui.available_rect_before_wrap();
        // The reference's chat band stays under the pane with no session,
        // collapsed to its 16px bottom padding, and the stack centers in
        // what is left above it.
        let pane = full.with_max_y(full.max.y - crate::theme::SP_6);
        let dark = ui.ctx().theme() == egui::Theme::Dark;
        let palette = Palette::active_of(ui.ctx());
        glow(ui, full, dark);

        // The column centers itself, and clamps to the padded pane the
        // way the web client's `@media (max-width: 860px)` pads it. The
        // floor keeps a degenerate first frame from collapsing the
        // cards while the panel widths settle.
        let col_w = WELCOME_W.min(pane.width() - 2.0 * 24.0).max(320.0);
        let left = pane.left() + (pane.width() - col_w) / 2.0;
        // The bottom pad is a share of the window's height, not the
        // pane's, as the reference measures it.
        let viewport = ui.ctx().viewport_rect().height();
        let inner_h = pane.height() - WELCOME_PAD_TOP - viewport * WELCOME_PAD_BOTTOM_SHARE;
        let offset = ((inner_h - self.stack_h) / 2.0).max(0.0);
        let mut stack = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(egui::Rect::from_min_size(
                    egui::pos2(left, pane.top()),
                    egui::vec2(col_w, pane.height()),
                ))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        // The reference snaps every box it lays out to a whole pixel.
        let top = crate::theme::snap(pane.top() + WELCOME_PAD_TOP + offset);
        stack.add_space(top - pane.top());
        // The height the next frame centers on starts here: the
        // leading pad and offset are the pane's, not the stack's, so
        // counting them would feed the offset back into itself and
        // settle the stack high of center.
        let content_top = stack.cursor().top();
        wordmark(&mut stack, palette, dark);
        stack.add_space(WORDMARK_MB);
        let pill = self.project_pill(&mut stack, store, palette);
        stack.add_space(PILL_TO_COMPOSER);
        let composer_event = composer.show(&mut stack, store, commands, running, enter_sends);
        stack.add_space(COMPOSER_TO_CARDS);
        events.extend(self.cards(&mut stack, palette));
        stack.add_space(18.0);
        hint(&mut stack, palette);
        self.stack_h = stack.cursor().top() - content_top;
        self.project_menu(&mut stack, store, palette, pill, &mut events);
        (events, composer_event)
    }

    /// The project pill: the folder and the name of the directory the
    /// next session opens in. Clicking it toggles the project menu.
    fn project_pill(&mut self, ui: &mut egui::Ui, store: &Store, palette: &Palette) -> egui::Rect {
        let id = egui::Id::new("welcome-project-pill");
        let hovered = ui.ctx().read_response(id).is_some_and(|r| r.hovered());
        // A flex row, so the pill keeps its own width instead of
        // stretching across the column, as the reference shell drew it.
        let mut rect = egui::Rect::NOTHING;
        ui.horizontal(|ui| {
            ui.add_space(PILL_ML);
            let project = crate::ui::sidebar::project_name(store.session_dir());
            let name = ui.painter().layout_no_wrap(
                project.clone(),
                egui::FontId::new(FS_SM, crate::theme::prose_family(ui.ctx())),
                palette.ink,
            );
            let width = 10.0 + 14.0 + 7.0 + name.size().x.ceil() + 7.0 + 12.0 + 10.0;
            let (pill, _) = ui.allocate_exact_size(egui::vec2(width, 28.0), egui::Sense::hover());
            let fill = if hovered {
                palette.hover
            } else {
                palette.surface
            };
            ui.painter()
                .rect_filled(pill, egui::CornerRadius::same(R_MD as u8), fill);
            let mid = pill.center().y;
            crate::icons::themed_image(Icon::Folder, 14.0, palette.muted).paint_snapped(
                ui,
                egui::Rect::from_min_size(
                    egui::pos2(pill.left() + 10.0, mid - 7.0),
                    egui::vec2(14.0, 14.0),
                ),
            );
            let text_x = pill.left() + 10.0 + 14.0 + 7.0;
            let text_w = name.size().x.ceil();
            // The line centers in the pill, a whole pixel from its top.
            let above = ((28.0 - crate::theme::line_h(FS_SM)) / 2.0).floor();
            ui.painter()
                .galley(egui::pos2(text_x, pill.top() + above), name, palette.ink);
            crate::icons::themed_image(Icon::ChevronDown, 12.0, palette.faint).paint_snapped(
                ui,
                egui::Rect::from_min_size(
                    egui::pos2(text_x + text_w + 7.0, mid - 6.0),
                    egui::vec2(12.0, 12.0),
                ),
            );
            let response = ui.interact(pill, id, egui::Sense::click());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &project)
            });
            if response.clicked() {
                self.project_open = !self.project_open;
            }
            rect = pill;
        });
        rect
    }

    /// The suggestion grid: two columns of cards, one under the
    /// viewport's narrow break. A click reports the card's fill.
    fn cards(&mut self, ui: &mut egui::Ui, palette: &Palette) -> Vec<WelcomeEvent> {
        let mut events = Vec::new();
        let narrow = ui.ctx().viewport_rect().width() <= NARROW;
        let cols = if narrow { 1 } else { 2 };
        let gap = CARD_GAP;
        let col_w = (ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32;
        ui.spacing_mut().item_spacing.y = 0.0;
        for (n, row) in SUGGESTIONS.chunks(cols).enumerate() {
            if n > 0 {
                ui.add_space(gap);
            }
            // A grid row is as tall as its tallest card, and every card
            // in it stretches to that height.
            let height = row
                .iter()
                .map(|card| {
                    let text = card_text(ui, card, col_w, palette.muted).size().y;
                    let tag = if card.tag.is_some() { tag_h() } else { 0.0 };
                    2.0 * 10.0 + 2.0 + text.max(tag).max(14.0)
                })
                .fold(0.0, f32::max);
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), height),
                egui::Sense::hover(),
            );
            for (m, card) in row.iter().enumerate() {
                let at = egui::Rect::from_min_size(
                    egui::pos2(rect.left() + m as f32 * (col_w + gap), rect.top()),
                    egui::vec2(col_w, height),
                );
                if let Some(event) = card_view(ui, card, palette, at) {
                    events.push(event);
                }
            }
        }
        events
    }

    /// The project menu over the pill: every directory a session
    /// worked in, most recent first, then the folder action. A click
    /// outside pill and menu closes it.
    fn project_menu(
        &mut self,
        ui: &mut egui::Ui,
        store: &mut Store,
        palette: &Palette,
        pill: egui::Rect,
        events: &mut Vec<WelcomeEvent>,
    ) {
        if !self.project_open {
            return;
        }
        let dirs = store
            .projects()
            .into_iter()
            .take(MENU_ROWS)
            .collect::<Vec<_>>();
        let current = store.session_dir().map(str::to_owned);
        let mut picked = None;
        let mut open_folder = false;
        let mut menu_rect = pill;
        egui::Window::new("welcome-project-menu")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .fixed_pos(pill.left_bottom() + egui::vec2(MENU_INDENT, MENU_DROP))
            .min_width(MENU_W)
            .order(egui::Order::Foreground)
            .frame(egui::Frame::NONE)
            .show(ui.ctx(), |ui| {
                crate::ui::toolbar::surface_in(ui, palette, palette.bg, 5, MENU_W - 10.0, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let (head, header) = ui.allocate_exact_size(
                        egui::vec2(
                            ui.available_width(),
                            6.0 + crate::theme::line_h(FS_2XS) + 3.0,
                        ),
                        egui::Sense::hover(),
                    );
                    header.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "PROJECT")
                    });
                    ui.painter().text(
                        egui::pos2(
                            head.left() + 9.0,
                            head.top() + 6.0 + crate::theme::line_h(FS_2XS) / 2.0,
                        ),
                        egui::Align2::LEFT_CENTER,
                        "PROJECT",
                        egui::FontId::new(FS_2XS, family(FAMILY_BODY_SEMIBOLD)),
                        palette.faint,
                    );
                    for dir in &dirs {
                        let id = egui::Id::new("welcome-project-row").with(dir.as_str());
                        let row =
                            project_row(ui, palette, id, dir, current.as_deref() == Some(dir));
                        if row {
                            picked = Some(dir.clone());
                        }
                    }
                    divider(ui, palette);
                    let open = menu_row(
                        ui,
                        palette,
                        egui::Id::new("welcome-project-open"),
                        Icon::Plus,
                        "Open folder\u{2026}",
                    );
                    if open {
                        open_folder = true;
                    }
                    menu_rect = ui.min_rect();
                });
            });
        let (clicked, at) = ui.input(|i| (i.pointer.any_click(), i.pointer.interact_pos()));
        if clicked && at.is_some_and(|pos| !pill.contains(pos) && !menu_rect.contains(pos)) {
            self.project_open = false;
        }
        if let Some(dir) = picked {
            store.set_project(Some(dir));
            self.project_open = false;
        }
        if open_folder {
            events.push(WelcomeEvent::OpenFolder);
            self.project_open = false;
        }
    }
}

/// The glow backdrop behind the stack, stretched over the pane the way
/// the web client's `.welcome` fills it. The browser build carries no
/// PNG decoder, so it washes the pane with the palette instead.
fn glow(ui: &mut egui::Ui, pane: egui::Rect, dark: bool) {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = dark;
        let palette = crate::theme::Palette::active_of(ui.ctx());
        ui.painter().rect_filled(pane, 0, palette.sunken);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let ctx = ui.ctx();
        let bytes = if dark {
            WELCOME_GLOW_PNG
        } else {
            WELCOME_GLOW_DAWN_PNG
        };
        let tex = png_texture(
            ctx,
            egui::Id::new("welcome-glow").with(dark),
            "welcome-glow",
            bytes,
        );
        // The reference's image keeps the square art's aspect: as wide
        // as the pane and as tall as it is wide, from the pane's top,
        // clipped to the pane.
        let square = egui::Rect::from_min_size(pane.min, egui::vec2(pane.width(), pane.width()));
        ui.painter().with_clip_rect(pane).image(
            tex.id(),
            square,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
}

/// The wordmark: the kanji glyph over its hard offset shadow, then the
/// name over its own, centered in the column.
#[cfg_attr(target_arch = "wasm32", allow(unused_variables))] // `dark` picks the face PNG
fn wordmark(ui: &mut egui::Ui, palette: &Palette, dark: bool) {
    let font = egui::FontId::new(NAME_PX, family(FAMILY_BODY_EXTRABOLD));
    let name = ui
        .painter()
        .layout_no_wrap("kage".to_owned(), font.clone(), palette.ink_strong);
    let total = GLYPH_PX + WORDMARK_GAP + name.size().x.ceil();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), GLYPH_PX),
        egui::Sense::hover(),
    );
    let left = crate::theme::snap(rect.left() + (rect.width() - total) / 2.0);
    let glyph =
        egui::Rect::from_min_size(egui::pos2(left, rect.top()), egui::vec2(GLYPH_PX, GLYPH_PX));
    // The toolkit's SVG paints the path one flat color, and the file
    // carries none, so the shadow's fill rides in a patched copy: the
    // reference wordmark tints the same outline `accent_soft`. The uri keys
    // the loader's cache per color, so a theme swap re-decodes, and ends
    // in `.svg` so the SVG loader takes it.
    let [r, g, b, a] = palette.accent_soft.to_srgba_unmultiplied();
    let fill = format!("#{r:02x}{g:02x}{b:02x}");
    let opacity = f32::from(a) / 255.0;
    let shadow_uri = format!("bytes://kage-glyph-shadow-{r:02x}{g:02x}{b:02x}{a:02x}.svg");
    let patched = str::replace(
        str::from_utf8(GLYPH).unwrap_or(""),
        "<path ",
        &format!("<path fill=\"{fill}\" fill-opacity=\"{opacity}\" "),
    );
    egui::Image::new(egui::ImageSource::Bytes {
        uri: shadow_uri.into(),
        bytes: patched.into_bytes().into(),
    })
    .fit_to_exact_size(egui::vec2(GLYPH_PX, GLYPH_PX))
    .paint_snapped(
        ui,
        glyph.translate(egui::vec2(SHADOW_OFFSET, SHADOW_OFFSET)),
    );
    // The painted face is raster art; the browser keeps the tinted
    // outline alone.
    #[cfg(not(target_arch = "wasm32"))]
    {
        let face = png_texture(
            ui.ctx(),
            egui::Id::new("welcome-glyph-face").with(dark),
            "welcome-glyph-face",
            if dark {
                GLYPH_FACE_SHADOW_PNG
            } else {
                GLYPH_FACE_DAWN_PNG
            },
        );
        egui::Image::new((face.id(), egui::vec2(GLYPH_PX, GLYPH_PX))).paint_snapped(ui, glyph);
    }
    let name_top = glyph.center().y - name.size().y / 2.0;
    let name_left = glyph.right() + WORDMARK_GAP;
    let painter = ui.painter();
    painter.text(
        egui::pos2(name_left + NAME_OFFSET, name_top + NAME_OFFSET),
        egui::Align2::LEFT_TOP,
        "kage",
        font.clone(),
        palette.accent_soft,
    );
    painter.text(
        egui::pos2(name_left, name_top),
        egui::Align2::LEFT_TOP,
        "kage",
        font,
        palette.ink_strong,
    );
}

/// The wrapped text of `card` in a column `width` wide: the room left
/// beside the icon and the tag, at the reference's 1.5 line height.
fn card_text(
    ui: &egui::Ui,
    card: &Suggestion,
    width: f32,
    color: egui::Color32,
) -> std::sync::Arc<egui::Galley> {
    let tag_space = card.tag.map_or(0.0, |tag| {
        tag_galley(ui, tag, color).size().x.ceil() + 14.0 + 10.0
    });
    let text_w = (width - 2.0 - 24.0 - 14.0 - 10.0 - tag_space).max(40.0);
    let format = egui::TextFormat {
        font_id: egui::FontId::new(FS_SM, family(FAMILY_BODY_REGULAR)),
        color,
        line_height: Some(crate::theme::snap(FS_SM * 1.5)),
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::single_section(card.text.to_owned(), format);
    job.wrap.max_width = text_w;
    ui.painter().layout_job(job)
}

/// One suggestion card painted into `rect`: the icon, the text and the
/// tag, centered on the row, lifting onto the hover fill.
fn card_view(
    ui: &mut egui::Ui,
    card: &Suggestion,
    palette: &Palette,
    rect: egui::Rect,
) -> Option<WelcomeEvent> {
    let response = ui
        .interact(
            rect,
            egui::Id::new("welcome-card").with(card.id),
            egui::Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, card.text));
    let hovered = response.hovered();
    let radius = egui::CornerRadius::same(R_LG as u8);
    if hovered {
        ui.painter().rect_filled(rect, radius, palette.hover);
    }
    let line = if hovered {
        palette.line_strong
    } else {
        palette.line
    };
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(1.0, line),
        egui::StrokeKind::Inside,
    );
    let ink = if hovered { palette.ink } else { palette.muted };
    let mid = rect.center().y;
    let left = rect.left() + 1.0 + 12.0;
    crate::icons::themed_image(card.icon, 14.0, palette.faint).paint_snapped(
        ui,
        egui::Rect::from_min_size(egui::pos2(left, mid - 7.0), egui::vec2(14.0, 14.0)),
    );
    let text = card_text(ui, card, rect.width(), ink);
    // egui sets a line's glyphs at its top; the reference centers them
    // in the taller line, so the baseline moves down by half the extra.
    let top = mid - text.size().y / 2.0;
    let y = text
        .rows
        .first()
        .and_then(|row| row.row.glyphs.first())
        .map_or(top, |glyph| {
            let lead = (glyph.line_height - glyph.font_height) / 2.0;
            crate::theme::snap(top) + crate::theme::snap(lead + glyph.font_face_ascent)
                - glyph.pos.y
        });
    ui.painter()
        .galley(egui::pos2(left + 14.0 + 10.0, y), text, ink);
    if let Some(tag) = card.tag {
        let (fg, bg) = if tag == "swarm" {
            (palette.done, palette.done_soft)
        } else {
            (palette.accent, palette.accent_soft)
        };
        paint_tag(ui, tag, fg, bg, rect);
    }
    response.clicked().then_some(WelcomeEvent::Fill {
        text: card.text,
        option: card.option,
    })
}

/// The gap between the suggestion cards, both ways.
const CARD_GAP: f32 = 8.0;

/// The tag galley, shared by the space reservation and the paint.
/// A card tag's height: its line and a pixel above and below.
fn tag_h() -> f32 {
    crate::theme::line_h(FS_2XS) + 2.0
}

fn tag_galley(ui: &egui::Ui, tag: &str, fg: egui::Color32) -> std::sync::Arc<egui::Galley> {
    ui.painter().layout_no_wrap(
        tag.to_owned(),
        egui::FontId::new(FS_2XS, crate::theme::prose_family(ui.ctx())),
        fg,
    )
}

/// The rounded tag pill at the card's right edge, over the row the
/// frame drew.
fn paint_tag(ui: &mut egui::Ui, tag: &str, fg: egui::Color32, bg: egui::Color32, card: egui::Rect) {
    let galley = tag_galley(ui, tag, fg);
    let width = galley.size().x.ceil() + 14.0;
    let rect = egui::Rect::from_center_size(
        egui::pos2(card.right() - 13.0 - width / 2.0, card.center().y),
        egui::vec2(width, tag_h()),
    );
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(R_FULL as u8), bg);
    ui.painter()
        .galley(rect.min + egui::vec2(7.0, 1.0), galley, fg);
}

/// The hint line under the cards: the key cap and what it opens,
/// centered as one row.
fn hint(ui: &mut egui::Ui, palette: &Palette) {
    let label = ui.painter().layout_no_wrap(
        "commands and sessions".to_owned(),
        egui::FontId::new(FS_XS, family(crate::theme::FAMILY_BODY_REGULAR)),
        palette.faint,
    );
    // The row is one line tall, the key cap centered on it and 4px
    // from the words.
    let line = crate::theme::line_h(FS_XS);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), line), egui::Sense::hover());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "commands and sessions")
    });
    let chip_w = crate::ui::palette::kbd_width(ui, "Ctrl K", palette);
    let width = chip_w + 4.0 + label.size().x.ceil();
    let left = crate::theme::snap(rect.left() + (rect.width() - width) / 2.0);
    crate::ui::palette::paint_kbd(
        ui,
        egui::pos2(
            left + chip_w,
            crate::theme::snap(rect.top() + (line - 18.0) / 2.0),
        ),
        "Ctrl K",
        palette,
    );
    let y = crate::theme::line_y(rect.top(), line, &label);
    ui.painter()
        .galley(egui::pos2(left + chip_w + 4.0, y), label, palette.faint);
}

/// One project menu row: the folder, its name over its path, and the
/// check when the session opens there now.
fn project_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    id: egui::Id,
    dir: &str,
    checked: bool,
) -> bool {
    let name_h = crate::theme::line_h(FS_SM);
    let path_h = crate::theme::line_h(FS_XS);
    let name = crate::ui::sidebar::project_name(Some(dir));
    let (rect, response) = menu_rect(ui, id, name_h + path_h, &name, palette);
    let mid = rect.center().y;
    let mut right = rect.right() - 9.0;
    if checked {
        right -= 14.0;
        crate::icons::themed_image(Icon::Check, 14.0, palette.accent).paint_snapped(
            ui,
            egui::Rect::from_min_size(egui::pos2(right, mid - 7.0), egui::vec2(14.0, 14.0)),
        );
        right -= 10.0;
    }
    let x = paint_menu_icon(ui, rect, Icon::Folder, palette);
    let top = mid - (name_h + path_h) / 2.0;
    let lines = [
        (name, FS_SM, palette.ink, top, name_h),
        (dir.to_owned(), FS_XS, palette.faint, top + name_h, path_h),
    ];
    for (text, size, color, top, height) in lines {
        let galley = crate::ui::transcript::head::one_line(
            ui,
            text,
            egui::TextFormat::simple(egui::FontId::new(size, family(FAMILY_BODY_REGULAR)), color),
            right - x,
        );
        ui.painter().galley(
            egui::pos2(x, top + (height - galley.size().y) / 2.0),
            galley,
            color,
        );
    }
    response.clicked()
}

/// One simple menu row: an icon and a label.
fn menu_row(ui: &mut egui::Ui, palette: &Palette, id: egui::Id, icon: Icon, label: &str) -> bool {
    let (rect, response) = menu_rect(ui, id, crate::theme::line_h(FS_SM), label, palette);
    let x = paint_menu_icon(ui, rect, icon, palette);
    ui.painter().text(
        egui::pos2(x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::new(FS_SM, family(FAMILY_BODY_REGULAR)),
        palette.ink,
    );
    response.clicked()
}

/// A menu row's rect around `content_h` of text, with its hover fill.
fn menu_rect(
    ui: &mut egui::Ui,
    id: egui::Id,
    content_h: f32,
    label: &str,
    palette: &Palette,
) -> (egui::Rect, egui::Response) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), content_h + 14.0),
        egui::Sense::hover(),
    );
    let response = ui
        .interact(rect, id, egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(R_MD as u8), palette.selected);
    }
    (rect, response)
}

/// Paints a menu row's leading icon and gives back where its text
/// starts.
fn paint_menu_icon(ui: &egui::Ui, rect: egui::Rect, icon: Icon, palette: &Palette) -> f32 {
    let at = egui::pos2(rect.left() + 9.0, rect.center().y - 7.0);
    crate::icons::themed_image(icon, 14.0, palette.muted)
        .paint_snapped(ui, egui::Rect::from_min_size(at, egui::vec2(14.0, 14.0)));
    at.x + 14.0 + 10.0
}

/// The hairline between the menu's rows and its action.
fn divider(ui: &mut egui::Ui, palette: &Palette) {
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width() - 4.0, 1.0),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(rect, 0, palette.subtle);
    ui.add_space(4.0);
}

/// Decodes an embedded PNG once and keeps the texture in the context,
/// so a repaint never re-decodes brand art. The lookups stay outside
/// `load_texture`: the context's data lock and the texture manager's
/// lock do not nest.
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::map_clone)] // `.cloned()` is ambiguous on IdTypeMap's two Option impls
fn png_texture(ctx: &egui::Context, id: egui::Id, name: &str, bytes: &[u8]) -> egui::TextureHandle {
    if let Some(tex) = ctx.data_mut(|data| data.get_temp(id).map(Clone::clone)) {
        return tex;
    }
    let decoded = image::load_from_memory(bytes).expect("the embedded brand art decodes");
    let size = [decoded.width() as usize, decoded.height() as usize];
    let tex = ctx.load_texture(
        name,
        egui::ColorImage::from_rgba_unmultiplied(size, decoded.to_rgba8().as_raw()),
        egui::TextureOptions::LINEAR,
    );
    ctx.data_mut(|data| data.insert_temp(id, tex.clone()));
    tex
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cards_fill_the_draft_and_only_hold_advertised_options() {
        let [fix, swarm, plan, delegate] = SUGGESTIONS;
        assert_eq!(fix.option, None, "the plain card holds nothing");
        assert_eq!(delegate.option, None, "the plain card holds nothing");
        assert_eq!(swarm.option, Some(("swarm", "on")));
        assert_eq!(swarm.tag, Some("swarm"));
        assert_eq!(plan.option, Some(("mode", "plan")));
        assert_eq!(plan.tag, Some("plan"));
    }

    #[test]
    fn the_fill_event_carries_its_card() {
        let [_, swarm, ..] = SUGGESTIONS;
        let event = WelcomeEvent::Fill {
            text: swarm.text,
            option: swarm.option,
        };
        assert_eq!(
            event,
            WelcomeEvent::Fill {
                text: "Review every module for unhandled errors",
                option: Some(("swarm", "on")),
            }
        );
    }
}
