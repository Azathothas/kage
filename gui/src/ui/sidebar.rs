//! The sidebar: the brand row, the New session and Search controls,
//! the session list with status badges and relative times, and the
//! connection footer.
//!
//! The list carries live sessions first, then the recorded directory
//! entries no live session holds, each tagged with the project its
//! working directory names. Rows nest under the session they were
//! forked from, with guide rails past later siblings. A child agent's
//! session is reached through its parent's card and the Agents tab,
//! never listed on its own.

use std::collections::{BTreeSet, HashMap};

use kage_client::Session;
use kage_client::wire::StopReason;

use crate::clock::unix_seconds;
use crate::store::Store;
use crate::theme::{
    FAMILY_BODY_EXTRABOLD, FAMILY_BODY_REGULAR, FAMILY_BODY_SEMIBOLD, FAMILY_MONO_REGULAR, FS_2XS,
    FS_BASE, FS_SM, FS_XS, Palette, R_MD, family,
};

const FOOTER_H: f32 = 52.0;

/// The brand row: the mark, the wordmark and the version.
const BRAND_H: f32 = 32.0;

/// The days from 1970-01-01 to a civil date.
#[must_use]
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let m = if month <= 2 { month + 9 } else { month - 3 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * i64::from(m) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The Unix seconds of an ISO 8601 timestamp, fractions dropped.
/// `Z` and numeric offsets in hours and minutes are honored.
#[must_use]
pub fn epoch_seconds(iso: &str) -> Option<i64> {
    let year: i64 = iso.get(0..4)?.parse().ok()?;
    let month: u32 = iso.get(5..7)?.parse().ok()?;
    let day: u32 = iso.get(8..10)?.parse().ok()?;
    let hour: i64 = iso.get(11..13)?.parse().ok()?;
    let minute: i64 = iso.get(14..16)?.parse().ok()?;
    let second: i64 = iso.get(17..19)?.parse().ok()?;
    let rest = iso.get(19..).unwrap_or("");
    let offset = match rest.chars().next() {
        Some('+') => -offset_seconds(rest.get(1..)?)?,
        Some('-') => offset_seconds(rest.get(1..)?)?,
        _ => 0,
    };
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second + offset)
}

/// The seconds a `hh:mm` UTC offset stands for.
fn offset_seconds(rest: &str) -> Option<i64> {
    let hours: i64 = rest.get(0..2)?.parse().ok()?;
    let minutes: i64 = rest.get(3..5).and_then(|s| s.parse().ok()).unwrap_or(0);
    Some(hours * 3_600 + minutes * 60)
}

/// The relative label the design's session rows carry: now, minutes,
/// hours or days, halves rounded up like the web client's.
#[must_use]
pub fn ago_label(seconds: i64) -> String {
    let s = seconds.max(0);
    if s < 45 {
        "now".to_owned()
    } else if s < 3_600 {
        format!("{}m", (s + 30) / 60)
    } else if s < 86_400 {
        format!("{}h", (s + 1_800) / 3_600)
    } else {
        format!("{}d", (s + 43_200) / 86_400)
    }
}

/// The relative label for an ISO 8601 timestamp against `now`.
#[must_use]
pub fn ago(iso: Option<&str>, now: i64) -> Option<String> {
    let updated = epoch_seconds(iso?)?;
    Some(ago_label(now - updated))
}

/// The project a working directory names: its last path segment, or
/// `local` when there is no directory.
#[must_use]
pub fn project_name(cwd: Option<&str>) -> String {
    let Some(cwd) = cwd else {
        return "local".to_owned();
    };
    let trimmed = cwd.trim_end_matches('/');
    let name = trimmed.rsplit('/').next().unwrap_or(trimmed);
    if name.is_empty() {
        "local".to_owned()
    } else {
        name.to_owned()
    }
}

/// What a session row leads or badges with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    /// A permission ask waits; the row badges Approve.
    Approve,
    /// A question waits for an answer; the row badges Answer.
    Answer,
    /// A plan waits for review; the row badges Review.
    Review,
    /// A turn is in flight; the row leads with the orbiting eclipse.
    Running,
    /// The last turn stopped in a refusal; the row badges Failed.
    Failed,
    /// Quiet: the row shows its relative time, or the swarm glyph in
    /// swarm mode.
    Idle,
}

/// Where session `session` stands, as its sidebar row shows it.
#[must_use]
pub fn row_state(session: &Session) -> RowState {
    if crate::store::plan_review(session).is_some() {
        RowState::Review
    } else if session
        .permissions
        .first()
        .is_some_and(|ask| ask.question.is_some())
    {
        RowState::Answer
    } else if !session.permissions.is_empty() {
        RowState::Approve
    } else if session.running || session.in_turn {
        RowState::Running
    } else if session.last_stop == Some(StopReason::Refusal) {
        RowState::Failed
    } else {
        RowState::Idle
    }
}

/// One session row before it lands in the list: the facts the rows
/// and the project grouping both read.
#[derive(Debug, Clone)]
pub struct RowItem {
    /// The session's id.
    pub id: String,
    /// The row's title, or `None` for an untitled one.
    pub title: Option<String>,
    /// ISO 8601 time of the last activity.
    pub updated_at: Option<String>,
    /// What the row leads or badges with.
    pub state: RowState,
    /// Whether the row is the active session.
    pub active: bool,
    /// The project the row groups under.
    pub project: String,
    /// The directory the session works in.
    pub dir: Option<String>,
    /// Whether the session works in swarm mode.
    pub swarm: bool,
    /// Whether it moved since the user last looked.
    pub unread: bool,
    /// The session it was forked from.
    pub parent: Option<String>,
    /// Whether the user pinned it to the top.
    pub pinned: bool,
}

/// Where a row sits in the fork forest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// The row's index in the list the forest was built from.
    pub ix: usize,
    /// How many forks deep it sits.
    pub depth: usize,
    /// Whether it is its parent's last fork.
    pub last: bool,
    /// Per depth, whether a guide rail runs past the row there because
    /// a later sibling of an ancestor follows.
    pub rails: Vec<bool>,
    /// How many forks it has.
    pub kids: usize,
}

/// `parents` as a forest: each row is followed by its forks, depth
/// first. A row whose parent is not in the list is a root. A row in a
/// cycle emits neither itself nor its cycle, because neither has a
/// root to hang from.
#[must_use]
pub fn forest(parents: &[Option<&str>], ids: &[&str]) -> Vec<Place> {
    fn walk(
        ix: usize,
        depth: usize,
        last: bool,
        rails: Vec<bool>,
        kids_of: &[Vec<usize>],
        out: &mut Vec<Place>,
    ) {
        let kids = &kids_of[ix];
        out.push(Place {
            ix,
            depth,
            last,
            rails: rails.clone(),
            kids: kids.len(),
        });
        for (n, &kid) in kids.iter().enumerate() {
            let mut rails = rails.clone();
            rails.resize(depth + 2, false);
            rails[depth + 1] = n + 1 < kids.len();
            walk(kid, depth + 1, n + 1 == kids.len(), rails, kids_of, out);
        }
    }
    let mut at_of: HashMap<&str, usize> = HashMap::with_capacity(ids.len());
    for (ix, id) in ids.iter().copied().enumerate() {
        at_of.entry(id).or_insert(ix);
    }
    let mut kids_of = vec![Vec::new(); ids.len()];
    let mut roots = Vec::new();
    for (ix, parent) in parents.iter().enumerate() {
        match parent
            .and_then(|id| at_of.get(id).copied())
            .filter(|&at| at != ix)
        {
            Some(at) => kids_of[at].push(ix),
            None => roots.push(ix),
        }
    }
    let mut out = Vec::with_capacity(ids.len());
    for root in roots {
        walk(root, 0, true, Vec::new(), &kids_of, &mut out);
    }
    out
}

/// The fork forest of `rows`.
#[must_use]
pub fn placed(rows: &[&RowItem]) -> Vec<Place> {
    let parents: Vec<Option<&str>> = rows.iter().map(|row| row.parent.as_deref()).collect();
    let ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
    forest(&parents, &ids)
}

/// Whether the session works in swarm mode.
#[must_use]
pub fn swarm_on(session: &Session) -> bool {
    session
        .config_options
        .iter()
        .any(|option| option.id == "swarm" && option.current_value == "on")
}

/// The rows the sidebar lists, in the order the design draws them:
/// pinned first, then newest first, with a live session that has no
/// time yet counted as the newest.
#[must_use]
pub fn sidebar_rows(store: &Store) -> Vec<RowItem> {
    let state = store.state();
    let mut items: Vec<RowItem> = Vec::new();
    for (id, session) in state.sessions.iter().filter(|(_, s)| s.parent.is_none()) {
        items.push(RowItem {
            id: id.clone(),
            title: Some(store.display_title(id)),
            updated_at: session.updated_at.clone(),
            state: row_state(session),
            active: store.active_id() == Some(id.as_str()),
            project: project_name(session.cwd.as_deref()),
            dir: session.cwd.clone(),
            swarm: swarm_on(session),
            unread: store.is_unread(id),
            parent: store.fork_parent(id).map(str::to_owned),
            pinned: store.prefs().pinned.contains(id),
        });
    }
    for info in &state.directory {
        if state.sessions.contains_key(&info.session_id) {
            continue;
        }
        items.push(RowItem {
            id: info.session_id.clone(),
            title: info.title.clone(),
            updated_at: info.updated_at.clone(),
            state: RowState::Idle,
            active: store.active_id() == Some(info.session_id.as_str()),
            project: project_name(Some(&info.cwd)),
            dir: Some(info.cwd.clone()),
            swarm: false,
            unread: false,
            parent: store.fork_parent(&info.session_id).map(str::to_owned),
            pinned: store.prefs().pinned.contains(&info.session_id),
        });
    }
    // Archived sessions leave the list until restored.
    items.retain(|item| !store.prefs().archived.contains(&item.id));
    items.sort_by_key(|item| {
        (
            !item.pinned,
            std::cmp::Reverse(
                item.updated_at
                    .as_deref()
                    .and_then(epoch_seconds)
                    .unwrap_or(i64::MAX),
            ),
        )
    });
    items
}

/// The rows `query` keeps: the title and the project both match, case
/// insensitively, and an empty query keeps every row.
#[must_use]
pub fn filter_rows(rows: &[RowItem], query: &str) -> Vec<RowItem> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return rows.to_vec();
    }
    rows.iter()
        .filter(|row| {
            let title = row.title.as_deref().unwrap_or("untitled session");
            format!("{title} {}", row.project)
                .to_lowercase()
                .contains(&needle)
        })
        .cloned()
        .collect()
}

/// The projects `rows` group under, in first-seen order, each with the
/// rows under it.
#[must_use]
pub fn project_groups(rows: &[RowItem]) -> Vec<(String, Vec<RowItem>)> {
    let mut groups: Vec<(String, Vec<RowItem>)> = Vec::new();
    for row in rows {
        if let Some(group) = groups.iter_mut().find(|(name, _)| *name == row.project) {
            group.1.push(row.clone());
        } else {
            groups.push((row.project.clone(), vec![row.clone()]));
        }
    }
    groups
}

/// The connection footer's two lines: the link's name, and the state
/// the transport stands in.
#[must_use]
pub fn connect_lines(store: &Store) -> (String, String) {
    let link = store.link();
    let status = match store.connect() {
        crate::transport::State::Connected => link.detail.clone(),
        crate::transport::State::Connecting => "connecting\u{2026}".to_owned(),
        crate::transport::State::Reconnecting { .. } => "reconnecting\u{2026}".to_owned(),
        crate::transport::State::Refused(_) | crate::transport::State::Closed => {
            "not running".to_owned()
        }
    };
    (link.name.to_owned(), status)
}

/// The connection lamp's color, and whether it carries a soft halo.
#[must_use]
pub fn connect_dot(palette: &Palette, store: &Store) -> (egui::Color32, bool) {
    match store.connect() {
        crate::transport::State::Connected => (palette.ok, true),
        crate::transport::State::Connecting | crate::transport::State::Reconnecting { .. } => {
            (palette.warn, false)
        }
        crate::transport::State::Refused(_) => (palette.danger, false),
        crate::transport::State::Closed => (palette.faint, false),
    }
}

/// What the sidebar asks the shell to carry out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidebarEvent {
    /// Open a new session in `dir`, or the shell's own when `None`.
    NewSession(Option<String>),
    /// Make `id` the active session.
    Activate(String),
    /// Open the command palette.
    OpenPalette,
    /// Open the settings page.
    OpenSettings,
    /// Collapse the sidebar.
    Collapse,
    /// Toggle the project grouping.
    ToggleGrouping,
}

/// The sidebar's state: the search query and the collapsed projects.
#[derive(Debug, Default)]
pub struct Sidebar {
    /// The query the list narrows over.
    query: String,
    /// The projects whose groups are collapsed, keyed by name.
    collapsed: BTreeSet<String>,
}

impl Sidebar {
    /// A closed sidebar over an empty query.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The query the list narrows over.
    pub fn query(&mut self) -> &mut String {
        &mut self.query
    }

    /// Whether `project`'s group is collapsed.
    #[must_use]
    pub fn collapsed(&self, project: &str) -> bool {
        self.collapsed.contains(project)
    }

    /// Collapses or opens `project`'s group.
    pub fn toggle_project(&mut self, project: &str) {
        if !self.collapsed.remove(project) {
            self.collapsed.insert(project.to_owned());
        }
    }
}

/// Draws the sidebar and reports what its controls asked for.
pub fn show(ui: &mut egui::Ui, sidebar: &mut Sidebar, store: &Store) -> Vec<SidebarEvent> {
    let rows = sidebar_rows(store);
    let grouped = store.prefs().group_by_project;
    let (name, status) = connect_lines(store);
    let palette = Palette::active_of(ui.ctx());
    let (dot, halo) = connect_dot(palette, store);
    let version = store
        .state()
        .agent
        .as_ref()
        .and_then(|agent| agent.version.clone());
    show_rows(
        ui,
        sidebar,
        &rows,
        grouped,
        (&name, &status, dot, halo),
        version.as_deref(),
    )
}

/// Draws the sidebar over `rows`, which the caller read from the store
/// once. The panel draws the sidebar twice while narrow, so the rows
/// ride in as data rather than being read per draw.
pub fn show_rows(
    ui: &mut egui::Ui,
    sidebar: &mut Sidebar,
    rows: &[RowItem],
    grouped: bool,
    link: (&str, &str, egui::Color32, bool),
    version: Option<&str>,
) -> Vec<SidebarEvent> {
    let palette = Palette::active_of(ui.ctx());
    let mut events = Vec::new();
    let now = unix_seconds();

    let (name, status, dot, halo) = link;
    ui.vertical(|ui| {
        ui.set_width(ui.available_width());
        brand_row(ui, version, &mut events);
        ui.add_space(6.0);
        nav_rows(ui, &mut events);
        list_header(ui, grouped, &mut events);
        list_body(ui, sidebar, rows, grouped, now, &mut events);
        connect_footer(ui, palette, name, status, dot, halo, &mut events);
    });
    events
}

/// The brand row: the eclipse mark, the wordmark, the engine version
/// and the collapse control.
fn brand_row(ui: &mut egui::Ui, version: Option<&str>, events: &mut Vec<SidebarEvent>) {
    let palette = Palette::active_of(ui.ctx());
    let (rect, _) = ui.allocate_at_least(
        egui::vec2(ui.available_width(), BRAND_H),
        egui::Sense::hover(),
    );
    let painter = ui.painter();
    let radius = 11.0;
    let center = egui::pos2(rect.left() + 6.0 + radius, rect.center().y);
    painter.add(orb(center, radius, palette.orb_1, palette.orb_2));
    painter.circle_filled(center + egui::vec2(4.1, 0.2), 6.8, palette.sidebar);
    let wordmark = painter.text(
        egui::pos2(center.x + radius + 9.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "kage",
        egui::FontId::new(15.0, family(FAMILY_BODY_EXTRABOLD)),
        palette.ink_strong,
    );
    if let Some(version) = version {
        painter.text(
            egui::pos2(wordmark.right() + 6.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            version,
            egui::FontId::new(FS_XS, family(FAMILY_MONO_REGULAR)),
            palette.faint,
        );
    }
    let collapse = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 22.0, rect.center().y),
        egui::vec2(28.0, 28.0),
    );
    if ui
        .put(
            collapse,
            egui::Button::image(crate::icons::themed_image(
                crate::icons::Icon::PanelLeft,
                16.0,
                palette.muted,
            ))
            .fill(egui::Color32::TRANSPARENT)
            .corner_radius(egui::CornerRadius::same(R_MD as u8)),
        )
        .on_hover_text("Collapse sidebar (Ctrl \\)")
        .clicked()
    {
        events.push(SidebarEvent::Collapse);
    }
}

/// A disc filled with a diagonal gradient from `from` at its top left
/// to `to` at its bottom right.
fn orb(center: egui::Pos2, radius: f32, from: egui::Color32, to: egui::Color32) -> egui::Shape {
    const STEPS: usize = 48;
    let mut mesh = egui::Mesh::default();
    let shade = |at: egui::Pos2| {
        let t = ((at.x - center.x) + (at.y - center.y)) / (4.0 * radius) + 0.5;
        from.lerp_to_gamma(to, t.clamp(0.0, 1.0))
    };
    mesh.colored_vertex(center, shade(center));
    for step in 0..=STEPS {
        let angle = step as f32 / STEPS as f32 * std::f32::consts::TAU;
        let at = center + egui::vec2(angle.cos(), angle.sin()) * radius;
        mesh.colored_vertex(at, shade(at));
    }
    for step in 0..STEPS as u32 {
        mesh.add_triangle(0, step + 1, step + 2);
    }
    egui::Shape::mesh(mesh)
}

/// The New session and Search nav rows.
fn nav_rows(ui: &mut egui::Ui, events: &mut Vec<SidebarEvent>) {
    let palette = Palette::active_of(ui.ctx());
    ui.spacing_mut().item_spacing.y = 1.0;
    if nav_row(
        ui,
        palette,
        crate::icons::Icon::SquarePen,
        "New session",
        "Ctrl N",
    ) {
        events.push(SidebarEvent::NewSession(None));
    }
    if nav_row(ui, palette, crate::icons::Icon::Search, "Search", "Ctrl K") {
        events.push(SidebarEvent::OpenPalette);
    }
}

/// One full-width nav row: the icon, the label and the shortcut chip.
/// Reports whether it was clicked.
fn nav_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: crate::icons::Icon,
    label: &str,
    keys: &str,
) -> bool {
    let (rect, response) =
        ui.allocate_at_least(egui::vec2(ui.available_width(), 34.0), egui::Sense::click());
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, egui::CornerRadius::same(R_MD as u8), palette.hover);
    }
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 8.0 + 8.0, rect.center().y),
        egui::vec2(16.0, 16.0),
    );
    crate::icons::themed_image(icon, 16.0, palette.ink).paint_at(ui, icon_rect);
    let painter = ui.painter();
    painter.text(
        egui::pos2(rect.left() + 35.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::new(FS_BASE, family(FAMILY_BODY_REGULAR)),
        palette.ink,
    );
    let chip_text = painter.layout_no_wrap(
        keys.to_owned(),
        egui::FontId::new(FS_2XS, family(FAMILY_MONO_REGULAR)),
        palette.faint,
    );
    let chip = egui::Rect::from_min_size(
        egui::pos2(
            rect.right() - chip_text.size().x - 20.0,
            rect.center().y - 9.0,
        ),
        egui::vec2(chip_text.size().x + 12.0, 18.0),
    );
    painter.rect_filled(chip, egui::CornerRadius::same(5), palette.fill);
    painter.rect_stroke(
        chip,
        egui::CornerRadius::same(5),
        egui::Stroke::new(1.0, palette.line),
        egui::StrokeKind::Inside,
    );
    painter.galley(
        chip.center() - chip_text.size() / 2.0,
        chip_text,
        palette.faint,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    response.clicked()
}

/// The list header: the SESSIONS label and the grouping toggle.
fn list_header(ui: &mut egui::Ui, grouped: bool, events: &mut Vec<SidebarEvent>) {
    let palette = Palette::active_of(ui.ctx());
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        ui.label(
            egui::RichText::new("SESSIONS")
                .size(FS_XS)
                .family(family(FAMILY_BODY_SEMIBOLD))
                .color(palette.faint),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(8.0);
            if ui
                .add(
                    egui::Button::new(
                        egui::RichText::new(if grouped { "\u{2261}" } else { "\u{1F5C1}" })
                            .size(FS_XS)
                            .color(palette.faint),
                    )
                    .fill(egui::Color32::TRANSPARENT)
                    .corner_radius(egui::CornerRadius::same(R_MD as u8)),
                )
                .on_hover_text(if grouped {
                    "Show as one list"
                } else {
                    "Group by project"
                })
                .clicked()
            {
                events.push(SidebarEvent::ToggleGrouping);
            }
        });
    });
}

/// The list body: groups under project rows, or one flat list.
fn list_body(
    ui: &mut egui::Ui,
    sidebar: &mut Sidebar,
    rows: &[RowItem],
    grouped: bool,
    now: i64,
    events: &mut Vec<SidebarEvent>,
) {
    let palette = Palette::active_of(ui.ctx());
    let filtered = filter_rows(rows, &sidebar.query().clone());
    let body_height = (ui.available_height() - FOOTER_H).max(0.0);
    egui::ScrollArea::vertical()
        .max_height(body_height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if grouped {
                for (project, members) in project_groups(&filtered) {
                    project_row(ui, sidebar, palette, &project, &members, events);
                    if sidebar.collapsed(&project) {
                        continue;
                    }
                    if members.is_empty() {
                        ui.label(
                            egui::RichText::new("No conversations yet")
                                .size(FS_SM)
                                .color(palette.faint),
                        );
                        continue;
                    }
                    let refs: Vec<&RowItem> = members.iter().collect();
                    for place in placed(&refs) {
                        session_row(ui, palette, &members[place.ix], &place, true, now, events);
                    }
                }
            } else {
                let refs: Vec<&RowItem> = filtered.iter().collect();
                for place in placed(&refs) {
                    session_row(ui, palette, &filtered[place.ix], &place, false, now, events);
                }
            }
        });
}

/// One project group row, with its count and its new-session control.
fn project_row(
    ui: &mut egui::Ui,
    sidebar: &mut Sidebar,
    palette: &Palette,
    project: &str,
    members: &[RowItem],
    events: &mut Vec<SidebarEvent>,
) {
    let open = !sidebar.collapsed(project);
    let dir = members.iter().find_map(|item| item.dir.clone());
    ui.horizontal(|ui| {
        let plus_width = if dir.is_some() { 28.0 } else { 0.0 };
        let response = ui.add(
            egui::Button::new((
                crate::icons::themed_image(
                    if open {
                        crate::icons::Icon::FolderOpen
                    } else {
                        crate::icons::Icon::Folder
                    },
                    FS_SM,
                    palette.muted,
                ),
                egui::RichText::new(project)
                    .size(FS_SM)
                    .color(palette.muted),
                egui::Atom::grow(),
                egui::RichText::new(members.len().to_string())
                    .size(FS_XS)
                    .color(palette.faint),
            ))
            .fill(egui::Color32::TRANSPARENT)
            .stroke(egui::Stroke::NONE)
            .corner_radius(egui::CornerRadius::same(R_MD as u8))
            .min_size(egui::vec2(ui.available_width() - plus_width, 32.0)),
        );
        if response.clicked() {
            sidebar.toggle_project(project);
        }
        if let Some(dir) = dir
            && ui
                .add(
                    egui::Button::new(egui::RichText::new("+").size(FS_XS).color(palette.faint))
                        .fill(egui::Color32::TRANSPARENT)
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(egui::CornerRadius::same(R_MD as u8)),
                )
                .on_hover_text("New session")
                .clicked()
        {
            events.push(SidebarEvent::NewSession(Some(dir)));
        }
    });
}

/// One session row: title, then the attention badge, the working
/// eclipse or the swarm glyph, else the relative time. An unread row
/// reads bold.
fn session_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    item: &RowItem,
    place: &Place,
    grouped: bool,
    now: i64,
    events: &mut Vec<SidebarEvent>,
) {
    let base = if grouped { 32.0 } else { 8.0 };
    let indent = base + place.depth as f32 * 16.0;
    let fg = if item.active {
        palette.ink_strong
    } else {
        palette.muted
    };
    let title = item
        .title
        .clone()
        .unwrap_or_else(|| "untitled session".to_owned());
    let response = ui.add(
        egui::Button::new("")
            .fill(if item.active {
                palette.selected
            } else {
                egui::Color32::TRANSPARENT
            })
            .stroke(egui::Stroke::NONE)
            .corner_radius(egui::CornerRadius::same(R_MD as u8))
            .min_size(egui::vec2(ui.available_width(), 32.0)),
    );
    let rect = response.rect;
    // The fork guides: a rail per depth, with the elbow at the row's
    // own depth.
    for depth in 1..=place.depth {
        let left = rect.left() + base - 14.0 + (depth - 1) as f32 * 16.0;
        if depth < place.depth {
            if place.rails.get(depth).copied().unwrap_or(false) {
                ui.painter().vline(
                    left + 6.0,
                    egui::Rangef::new(rect.top(), rect.bottom()),
                    egui::Stroke::new(1.0, palette.line_strong),
                );
            }
            continue;
        }
        let color = if item.active {
            palette.accent_bd
        } else {
            palette.line_strong
        };
        ui.painter().hline(
            egui::Rangef::new(left, left + 9.0),
            rect.center().y,
            egui::Stroke::new(1.0, color),
        );
        ui.painter().vline(
            left,
            egui::Rangef::new(rect.top(), rect.center().y),
            egui::Stroke::new(1.0, palette.line_strong),
        );
        if !place.last {
            ui.painter().vline(
                left,
                egui::Rangef::new(rect.center().y, rect.bottom()),
                egui::Stroke::new(1.0, palette.line_strong),
            );
        }
    }
    // The trailing fact: the badge, the eclipse, the swarm glyph, the
    // fork count, or the relative time.
    let trailing = match item.state {
        RowState::Approve => Some(("Approve".to_owned(), palette.ok, palette.ok_soft)),
        RowState::Answer => Some(("Answer".to_owned(), palette.accent, palette.accent_soft)),
        RowState::Review => Some(("Review".to_owned(), palette.ok, palette.ok_soft)),
        RowState::Failed => Some(("Failed".to_owned(), palette.danger, palette.danger_soft)),
        RowState::Running => None,
        RowState::Idle if item.swarm => None,
        RowState::Idle => ago(item.updated_at.as_deref(), now)
            .map(|when| (when, palette.faint, egui::Color32::TRANSPARENT)),
    };
    let badge = trailing.map(|(label, fg, bg)| {
        let galley = ui.painter().layout_no_wrap(
            label,
            egui::FontId::new(FS_2XS, family(FAMILY_BODY_SEMIBOLD)),
            fg,
        );
        (galley, fg, bg)
    });
    let reserved = match &badge {
        Some((galley, _, _)) if place.kids > 0 => galley.size().x + 52.0,
        Some((galley, _, _)) => galley.size().x + 28.0,
        None if item.state == RowState::Running || place.kids > 0 => 28.0,
        None => 8.0,
    };
    let mut job = egui::text::LayoutJob::simple_singleline(
        title,
        egui::FontId::new(
            FS_SM,
            if item.unread && !item.active {
                family(FAMILY_BODY_SEMIBOLD)
            } else {
                family(FAMILY_BODY_REGULAR)
            },
        ),
        fg,
    );
    job.wrap.max_width = (rect.right() - rect.left() - indent - reserved).max(0.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    let title_galley = ui.painter().layout_job(job);
    let title_y = rect.center().y - title_galley.size().y / 2.0;
    ui.painter()
        .galley(egui::pos2(rect.left() + indent, title_y), title_galley, fg);
    let mut fork_count_right = rect.right() - 12.0;
    if let Some((galley, fg, bg)) = badge {
        let width = galley.size().x + 12.0;
        let pill_rect = egui::Rect::from_min_size(
            egui::pos2(rect.right() - width - 8.0, rect.center().y - 9.0),
            egui::vec2(width, 18.0),
        );
        ui.painter()
            .rect_filled(pill_rect, egui::CornerRadius::same(9), bg);
        ui.painter()
            .galley(pill_rect.center() - galley.size() / 2.0, galley, fg);
        fork_count_right = pill_rect.left() - 6.0;
    }
    if item.state == RowState::Running {
        // The working eclipse: a filled disc with a crescent bitten out
        // of it, at the row's right edge.
        let center = egui::pos2(rect.right() - 19.0, rect.center().y);
        let radius = 7.0;
        ui.painter()
            .circle_filled(center, radius, palette.ink_strong);
        ui.painter().circle_filled(
            center + egui::vec2(2.0, -1.0),
            radius * 0.8,
            palette.sidebar,
        );
    }
    if place.kids > 0 {
        ui.painter().text(
            egui::pos2(fork_count_right, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            format!("\u{2387} {}", place.kids),
            egui::FontId::new(10.5, family(FAMILY_MONO_REGULAR)),
            palette.faint,
        );
    }
    if item.pinned {
        ui.painter().text(
            egui::pos2(rect.right() - 12.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            "\u{1F4CC}",
            egui::FontId::new(11.0, family(FAMILY_BODY_REGULAR)),
            palette.faint,
        );
    }
    if response.clicked() {
        events.push(SidebarEvent::Activate(item.id.clone()));
    }
}

/// The connection footer: the lamp, the link's two lines and the
/// settings control.
fn connect_footer(
    ui: &mut egui::Ui,
    palette: &Palette,
    name: &str,
    status: &str,
    dot: egui::Color32,
    halo: bool,
    events: &mut Vec<SidebarEvent>,
) {
    let (rect, _) = ui.allocate_at_least(
        egui::vec2(ui.available_width(), FOOTER_H),
        egui::Sense::hover(),
    );
    ui.painter().hline(
        rect.x_range(),
        rect.top(),
        egui::Stroke::new(1.0, palette.line),
    );
    let center = egui::pos2(rect.left() + 18.0, rect.center().y);
    if halo {
        ui.painter().circle_filled(center, 7.0, palette.ok_soft);
    }
    ui.painter().circle_filled(center, 4.0, dot);
    ui.painter().text(
        egui::pos2(rect.left() + 34.0, rect.center().y - 7.0),
        egui::Align2::LEFT_CENTER,
        name,
        egui::FontId::new(FS_SM, family(FAMILY_BODY_REGULAR)),
        palette.ink,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 34.0, rect.center().y + 8.0),
        egui::Align2::LEFT_CENTER,
        status,
        egui::FontId::new(11.0, family(FAMILY_BODY_REGULAR)),
        palette.faint,
    );
    let gear = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 24.0, rect.center().y),
        egui::vec2(28.0, 28.0),
    );
    if ui
        .put(
            gear,
            egui::Button::image(crate::icons::themed_image(
                crate::icons::Icon::Settings,
                16.0,
                palette.muted,
            ))
            .fill(egui::Color32::TRANSPARENT)
            .corner_radius(egui::CornerRadius::same(R_MD as u8)),
        )
        .on_hover_text("Settings (Ctrl ,)")
        .clicked()
    {
        events.push(SidebarEvent::OpenSettings);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RowState, ago, ago_label, days_from_civil, epoch_seconds, filter_rows, forest, placed,
        project_name, row_state, sidebar_rows, swarm_on,
    };
    use crate::clock::unix_seconds;
    use kage_client::Session;

    fn session(id: &str) -> Session {
        Session {
            id: id.to_owned(),
            opened: true,
            parent: None,
            cwd: None,
            title: None,
            updated_at: None,
            items: Vec::new(),
            usage: Default::default(),
            mode: None,
            config_options: Vec::new(),
            commands: Vec::new(),
            mcp: Default::default(),
            permissions: Vec::new(),
            queue: Vec::new(),
            running: false,
            in_turn: false,
            agents: Default::default(),
            plan: None,
            last_stop: None,
            draft: None,
        }
    }

    #[test]
    fn civil_dates_map_to_unix_days() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2026, 9, 30), 20_726);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
    }

    #[test]
    fn iso_timestamps_parse_with_zones_and_fractions() {
        assert_eq!(epoch_seconds("2026-09-30T12:34:56Z"), Some(1_790_771_696));
        assert_eq!(
            epoch_seconds("2026-09-30T12:34:56+02:00"),
            Some(1_790_771_696 - 7_200)
        );
        assert_eq!(
            epoch_seconds("2026-09-30T12:34:56.250Z"),
            Some(1_790_771_696)
        );
        assert_eq!(epoch_seconds("not a time"), None);
        assert_eq!(epoch_seconds(""), None);
    }

    #[test]
    fn relative_labels_round_half_up_like_the_web_client() {
        assert_eq!(ago_label(0), "now");
        assert_eq!(ago_label(44), "now");
        assert_eq!(ago_label(45), "1m");
        assert_eq!(ago_label(3_599), "60m");
        assert_eq!(ago_label(3_600), "1h");
        assert_eq!(ago_label(86_399), "24h");
        assert_eq!(ago_label(86_400), "1d");
        assert_eq!(ago_label(-5), "now");
    }

    #[test]
    fn ago_reports_the_gap_between_the_timestamp_and_now() {
        let now = epoch_seconds("2026-09-30T12:34:56Z").unwrap();
        assert_eq!(
            ago(Some("2026-09-30T12:34:30Z"), now).as_deref(),
            Some("now")
        );
        assert_eq!(
            ago(Some("2026-09-30T12:04:56Z"), now).as_deref(),
            Some("30m")
        );
        assert_eq!(
            ago(Some("2026-09-28T12:34:56Z"), now).as_deref(),
            Some("2d")
        );
        assert_eq!(ago(None, now), None);
    }

    #[test]
    fn project_names_take_the_last_path_segment() {
        assert_eq!(project_name(Some("/home/u/dev/kage")), "kage");
        assert_eq!(project_name(Some("/home/u/dev/kage/")), "kage");
        assert_eq!(project_name(None), "local");
        assert_eq!(project_name(Some("/")), "local");
    }

    #[test]
    fn row_states_read_the_session() {
        let mut session = session("s1");
        assert_eq!(row_state(&session), RowState::Idle);
        session.running = true;
        assert_eq!(row_state(&session), RowState::Running);
        session.running = false;
        session.last_stop = Some(kage_client::wire::StopReason::Refusal);
        assert_eq!(row_state(&session), RowState::Failed);
        session.last_stop = Some(kage_client::wire::StopReason::Cancelled);
        assert_eq!(row_state(&session), RowState::Idle);
        session.in_turn = true;
        assert_eq!(row_state(&session), RowState::Running);
    }

    #[test]
    fn swarm_mode_reads_the_swarm_option() {
        let mut session = session("s1");
        assert!(!swarm_on(&session));
        session
            .config_options
            .push(kage_client::wire::SessionConfigOption {
                id: "swarm".to_owned(),
                name: "Swarm".to_owned(),
                description: None,
                category: None,
                kind: kage_client::wire::SessionConfigKind::Select,
                current_value: "on".to_owned(),
                options: Vec::new(),
            });
        assert!(swarm_on(&session));
    }

    #[test]
    fn forks_nest_under_their_source_with_rails_past_later_siblings() {
        let ids = ["a", "b", "c", "d", "e"];
        let parents = [None, Some("a"), Some("a"), Some("b"), Some("gone")];
        let placed = forest(&parents, &ids);
        let order: Vec<(&str, usize, bool, usize)> = placed
            .iter()
            .map(|place| (ids[place.ix], place.depth, place.last, place.kids))
            .collect();
        assert_eq!(
            order,
            [
                ("a", 0, true, 2),
                ("b", 1, false, 1),
                ("d", 2, true, 0),
                ("c", 1, true, 0),
                ("e", 0, true, 0),
            ]
        );
        assert_eq!(
            placed[2].rails.get(1),
            Some(&true),
            "b's later sibling keeps a rail running past d"
        );
    }

    #[test]
    fn duplicate_ids_nest_under_the_first_occurrence() {
        let ids = ["a", "b", "a"];
        let parents = [None, Some("a"), None];
        let placed = forest(&parents, &ids);
        let order: Vec<&str> = placed.iter().map(|place| ids[place.ix]).collect();
        assert_eq!(order, ["a", "b", "a"], "every row lands exactly once");
        assert_eq!(
            (placed[1].depth, placed[1].last, placed[1].kids),
            (1, true, 0),
            "the first row carrying the id is the parent"
        );
    }

    #[test]
    fn a_parent_outside_the_list_is_a_root() {
        let ids = ["a", "b"];
        let parents = [Some("ghost"), None];
        let placed = forest(&parents, &ids);
        let order: Vec<(&str, usize)> = placed
            .iter()
            .map(|place| (ids[place.ix], place.depth))
            .collect();
        assert_eq!(order, [("a", 0), ("b", 0)], "both rows show as roots");
    }

    #[test]
    fn a_two_cycle_emits_neither_row() {
        let ids = ["a", "b"];
        let parents = [Some("b"), Some("a")];
        let placed = forest(&parents, &ids);
        assert!(placed.is_empty(), "a cycle legitimately emits neither row");
    }

    #[test]
    fn the_filter_keeps_the_rows_the_query_names() {
        let rows = vec![
            row("s1", Some("fix the flake"), "kage"),
            row("s2", Some("write the docs"), "docs"),
            row("s3", Some("tend the garden"), "docs"),
            row("s4", None, "kage"),
        ];
        assert_eq!(filter_rows(&rows, "").len(), 4, "an empty query keeps all");
        assert_eq!(filter_rows(&rows, "flake").len(), 1);
        assert_eq!(
            filter_rows(&rows, "FLAKE").len(),
            1,
            "the query is case insensitive"
        );
        assert_eq!(
            filter_rows(&rows, "docs").len(),
            2,
            "the project field is searched as well as the title"
        );
        assert_eq!(
            filter_rows(&rows, "garden").len(),
            1,
            "a row's own title matches"
        );
        assert_eq!(
            filter_rows(&rows, "untitled").len(),
            1,
            "an untitled row matches its fallback"
        );
        assert_eq!(filter_rows(&rows, "zzz").len(), 0);
    }

    /// A row with the given title and project.
    fn row(id: &str, title: Option<&str>, project: &str) -> super::RowItem {
        super::RowItem {
            id: id.to_owned(),
            title: title.map(str::to_owned),
            updated_at: None,
            state: RowState::Idle,
            active: false,
            project: project.to_owned(),
            dir: None,
            swarm: false,
            unread: false,
            parent: None,
            pinned: false,
        }
    }

    #[test]
    fn the_sidebar_rows_come_from_the_store() {
        let store = crate::store::Store::new("/w", false);
        let rows = sidebar_rows(&store);
        assert!(rows.is_empty(), "an empty store lists no rows");
        let _ = placed(&rows.iter().collect::<Vec<_>>());
    }

    #[test]
    fn the_clock_reads_unix_seconds() {
        assert!(unix_seconds() > 1_700_000_000);
    }
}
