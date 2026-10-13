//! The settings dialog: a section nav on the left and one page on the
//! right, over the shell like the other dialogs.
//!
//! Pages render thin over the store. The client's own choices (theme,
//! Enter behavior, the Lab toggles, the archive) are preferences the
//! shell stores; everything about the engine is read from what the
//! connection reported and says unknown where it reported nothing.

use kage_client::wire::SessionConfigOption;

use crate::prefs::Prefs;
use crate::store::Store;
use crate::theme::{
    FAMILY_BODY_SEMIBOLD, FAMILY_MONO_REGULAR, FS_2XS, FS_SM, FS_XS, Palette, R_FULL, R_LG, R_MD,
    R_XL, family,
};
use crate::ui::settings_config as config;

/// The pages of the settings dialog, in nav order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    /// Theme and input behavior.
    General,
    /// The engine's agent and session options.
    Agents,
    /// The providers and their models.
    Providers,
    /// The MCP servers and their status.
    Mcp,
    /// The external ACP agents.
    Acp,
    /// The permission mode and the tool rules.
    Permissions,
    /// The plugin directory and grants.
    Plugins,
    /// The link to the engine.
    Connection,
    /// The shortcut table.
    Keyboard,
    /// Experiments behind toggles.
    Lab,
    /// Sessions taken out of the sidebar.
    Archived,
    /// The engine and protocol versions.
    About,
}

impl Section {
    /// Every section, in nav order.
    pub const ALL: [Section; 12] = [
        Section::General,
        Section::Agents,
        Section::Providers,
        Section::Mcp,
        Section::Acp,
        Section::Permissions,
        Section::Plugins,
        Section::Connection,
        Section::Keyboard,
        Section::Lab,
        Section::Archived,
        Section::About,
    ];

    /// The nav label and page title.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Section::General => "General",
            Section::Agents => "Agent & Sessions",
            Section::Providers => "Model Providers",
            Section::Mcp => "MCP Servers",
            Section::Acp => "ACP Agents",
            Section::Permissions => "Permissions",
            Section::Plugins => "Plugins",
            Section::Connection => "Connection",
            Section::Keyboard => "Keyboard",
            Section::Lab => "Lab",
            Section::Archived => "Archived Sessions",
            Section::About => "About",
        }
    }

    /// The icon the nav row leads with.
    #[must_use]
    pub fn icon(self) -> crate::icons::Icon {
        match self {
            Section::General => crate::icons::Icon::Settings2,
            Section::Agents => crate::icons::Icon::Bot,
            Section::Providers => crate::icons::Icon::Zap,
            Section::Mcp => crate::icons::Icon::Server,
            Section::Acp => crate::icons::Icon::Bot,
            Section::Permissions => crate::icons::Icon::ShieldCheck,
            Section::Plugins => crate::icons::Icon::LayoutDashboard,
            Section::Connection => crate::icons::Icon::Network,
            Section::Keyboard => crate::icons::Icon::Command,
            Section::Lab => crate::icons::Icon::Lightbulb,
            Section::Archived => crate::icons::Icon::Archive,
            Section::About => crate::icons::Icon::Info,
        }
    }

    /// Whether the page reads the engine's configuration snapshot, so
    /// opening it asks for a fresh one.
    #[must_use]
    pub fn reads_config(self) -> bool {
        matches!(
            self,
            Section::Providers
                | Section::Mcp
                | Section::Acp
                | Section::Permissions
                | Section::Plugins
        )
    }
}

/// One row of the shortcut table: what it does and its keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortcut {
    /// What the keys do.
    pub label: &'static str,
    /// The keys, as the key caps read.
    pub keys: String,
}

/// The label of an app action the shortcut table lists, by its name.
fn action_label(name: &str) -> Option<&'static str> {
    Some(match name {
        "OpenPalette" => "Command palette",
        "NewSession" => "New session",
        "ToggleSidebar" => "Toggle sidebar",
        "ToggleWorkbench" => "Toggle workbench",
        "OpenSettings" => "Settings",
        "OpenFind" => "Find in chat",
        "SendPrompt" => "Send, or steer the running turn",
        "Quit" => "Quit",
        _ => return None,
    })
}

/// The name of `action`, as the binding table spells it.
fn action_name(action: crate::ui::keys::Action) -> &'static str {
    use crate::ui::keys::Action;
    match action {
        Action::Quit => "Quit",
        Action::NewSession => "NewSession",
        Action::ToggleSidebar => "ToggleSidebar",
        Action::ToggleWorkbench => "ToggleWorkbench",
        Action::SendPrompt => "SendPrompt",
        Action::OpenFind => "OpenFind",
        Action::OpenPalette => "OpenPalette",
        Action::OpenSettings => "OpenSettings",
        Action::OpenProviders => "OpenProviders",
        Action::ToggleSwarm => "ToggleSwarm",
        Action::TogglePlan => "TogglePlan",
        Action::SetGoal => "SetGoal",
        Action::CloseDialog => "CloseDialog",
        Action::ShowAgents => "ShowAgents",
        Action::ReviewChanges => "ReviewChanges",
    }
}

/// The shell's shortcut table: one row per labeled action, in binding
/// order, with the ctrl (or cmd) modifier spelled for this platform.
#[must_use]
pub fn shortcuts() -> Vec<Shortcut> {
    let bindings = crate::ui::keys::bindings();
    let mut rows: Vec<Shortcut> = Vec::new();
    for binding in &bindings {
        let Some(label) = action_label(action_name(binding.action)) else {
            continue;
        };
        if rows.iter().any(|row| row.label == label) {
            continue;
        }
        rows.push(Shortcut {
            label,
            keys: key_caps(binding),
        });
    }
    rows
}

/// One keystroke as key caps: `ctrl-k` reads `Ctrl K`.
fn key_caps(binding: &crate::ui::keys::Binding) -> String {
    let mut caps: Vec<String> = Vec::new();
    if binding.mods.ctrl {
        caps.push(if cfg!(target_os = "macos") {
            "Cmd".to_owned()
        } else {
            "Ctrl".to_owned()
        });
    }
    if binding.mods.shift {
        caps.push("Shift".to_owned());
    }
    if binding.mods.alt {
        caps.push("Alt".to_owned());
    }
    let mut chars = binding.key.chars();
    caps.push(match (chars.next(), chars.next()) {
        (Some(c), None) => c.to_uppercase().collect(),
        _ => {
            let mut word = binding.key.to_owned();
            if let Some(first) = word.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            word
        }
    });
    caps.join(" ")
}

/// Which updater an install button is running, so a second click
/// cannot stack a second download.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Install {
    /// The engine binary behind `kage rpc`.
    Engine,
    /// This client, which takes effect at the next launch.
    Desktop,
}

/// What the settings dialog asks the shell to carry out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsEvent {
    /// Apply and store this theme choice.
    Theme(crate::theme::ThemeChoice),
    /// Flip one boolean preference, named by the field it writes.
    Flip(&'static str),
    /// Store engine option `name` in the user config with `value`.
    EngineOption(String, serde_json::Value),
    /// Set the active session's permission mode to this value.
    Mode(String),
    /// Bring archived session `id` back to the sidebar.
    Restore(String),
    /// Open the form that edits this provider entry.
    ProviderForm(crate::ui::provider_form::Target),
    /// Open the form that edits this MCP server, or adds one.
    McpForm(Option<String>),
    /// Open the form that edits this ACP agent, or adds one.
    AcpForm(Option<String>),
    /// Open the provider directory picker; `named` reads one the user
    /// names instead of models.dev.
    Directory(bool),
    /// Follow this release channel from now on, and check it at once.
    Channel(crate::prefs::Channel),
    /// Run a release check now, past the daily throttle.
    CheckNow,
    /// Download the newest release of `which`.
    Install(Install),
    /// Open the releases page in the platform's browser.
    OpenReleases,
}

/// Which page of the settings dialog shows, and whether it is open.
#[derive(Debug)]
pub struct Settings {
    open: bool,
    section: Section,
    /// The forms open over their page, one at most.
    form: Option<Form>,
}

/// A form open over a settings page.
#[derive(Debug)]
enum Form {
    /// The provider form, over what it edits.
    Provider(Box<crate::ui::provider_form::ProviderForm>),
    /// The MCP server form.
    Mcp(Box<crate::ui::mcp_form::McpForm>),
    /// The ACP agent form.
    Acp(Box<crate::ui::acp_form::AcpForm>),
    /// The provider directory picker.
    Directory(Box<crate::ui::directory_picker::DirectoryPicker>),
}

impl Default for Settings {
    fn default() -> Self {
        Self::new()
    }
}

impl Settings {
    /// A closed settings dialog on the General page.
    #[must_use]
    pub fn new() -> Self {
        Self {
            open: false,
            section: Section::General,
            form: None,
        }
    }

    /// Whether the dialog shows.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Shows the dialog on `section`.
    pub fn open(&mut self, section: Section) {
        self.open = true;
        self.go(section);
    }

    /// Closes the dialog, and any form open over its page.
    pub fn close(&mut self) {
        self.open = false;
        self.form = None;
    }

    /// The page that shows.
    #[must_use]
    pub fn section(&self) -> Section {
        self.section
    }

    /// Shows `section`. A page read from the engine's configuration
    /// asks for a fresh snapshot, so an edit to config.toml shows on
    /// the next visit.
    pub fn go(&mut self, section: Section) {
        self.section = section;
        self.form = None;
    }

    /// Opens `form` over the page that shows.
    fn open_form(&mut self, form: Form) {
        self.form = Some(form);
    }

    /// Closes the form over the page, if one shows.
    fn close_form(&mut self) {
        self.form = None;
    }

    /// Follows the store: settles what the open form's writes did.
    fn follow(&mut self, store: &Store) {
        match &mut self.form {
            Some(Form::Provider(form)) => {
                form.follow(store);
            }
            Some(Form::Mcp(form)) => {
                form.follow(store);
            }
            Some(Form::Acp(form)) => {
                form.follow(store);
            }
            Some(Form::Directory(picker)) => picker.follow(store),
            None => {}
        }
    }

    /// Draws the page that shows, or the form open over it. Returns
    /// what the form answered with.
    fn show(
        &mut self,
        ui: &mut egui::Ui,
        store: &mut Store,
        palette: &Palette,
        updates: &UpdateState,
        events: &mut Vec<SettingsEvent>,
    ) -> FormOutcome {
        self.follow(store);
        match &mut self.form {
            Some(Form::Provider(form)) => FormOutcome::Done(form.show(ui, store)),
            Some(Form::Mcp(form)) => FormOutcome::Done(form.show(ui, store)),
            Some(Form::Acp(form)) => FormOutcome::Done(form.show(ui, store)),
            Some(Form::Directory(picker)) => match picker.show(ui, store) {
                Some(picked) if picked.entry.is_null() => FormOutcome::Done(true),
                Some(picked) => FormOutcome::Picked(picked),
                None => FormOutcome::Done(false),
            },
            None => {
                page(ui, store, self.section, palette, updates, events);
                FormOutcome::Done(false)
            }
        }
    }
}

/// What a form drew back.
#[derive(Debug)]
enum FormOutcome {
    /// The form asked to close or not.
    Done(bool),
    /// The directory picker chose a provider.
    Picked(crate::ui::directory_picker::Picked),
}

/// The settings dialog over the shell: a nav column, one page, and a
/// close button. Draws nothing while closed.
#[derive(Default)]
pub struct SettingsView {
    settings: Settings,
}

impl SettingsView {
    /// A closed settings dialog.
    #[must_use]
    pub fn new() -> Self {
        Self {
            settings: Settings::new(),
        }
    }

    /// Whether the dialog shows.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.settings.is_open()
    }

    /// Shows the dialog on `section`.
    pub fn open(&mut self, section: Section) {
        self.settings.open(section);
    }

    /// Closes the dialog.
    pub fn close(&mut self) {
        self.settings.close();
    }

    /// The page that shows.
    #[must_use]
    pub fn section(&self) -> Section {
        self.settings.section()
    }

    /// Whether a form shows over the page. Escape leaves the form
    /// before it leaves the dialog, and the dialog's own keys wait for
    /// it.
    #[must_use]
    pub fn form_open(&self) -> bool {
        self.settings.form.is_some()
    }

    /// Draws the dialog over `ui`, reporting what it asked for.
    /// `updates` is what the About page's release row reads.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        store: &mut Store,
        updates: &UpdateState,
    ) -> Vec<SettingsEvent> {
        if !self.settings.is_open() {
            return Vec::new();
        }
        // Escape leaves the form over the page first: the form is the
        // dialog's top layer, and the page reads the engine's
        // configuration again behind it.
        if self.settings.form.is_some() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.settings.close_form();
            if self.settings.section().reads_config() {
                store.ask_config();
            }
        }
        let palette = Palette::active_of(ui.ctx());
        let mut events = Vec::new();
        let mut close = false;
        let mut picked: Option<Section> = None;
        let mut outcome = FormOutcome::Done(false);
        egui::Area::new(egui::Id::new("kage-settings"))
            .anchor(egui::Align2::LEFT_TOP, egui::Vec2::ZERO)
            .order(egui::Order::Foreground)
            .interactable(true)
            .movable(false)
            .show(ui.ctx(), |ui| {
                let screen = ui.ctx().viewport_rect();
                ui.set_min_size(screen.size());
                let card = egui::Rect::from_center_size(
                    screen.center(),
                    egui::vec2(
                        960.0_f32.min(screen.width() * 0.96),
                        720.0_f32.min(screen.height() * 0.92),
                    ),
                );
                // The scrim covers the screen, so it must not swallow
                // the clicks the card's own widgets ask for: only a
                // click outside the card closes the dialog. Drawn
                // under the card, it answers only where the card is
                // not.
                let scrim = ui.allocate_response(screen.size(), egui::Sense::click());
                ui.painter().rect_filled(
                    scrim.rect,
                    egui::CornerRadius::ZERO,
                    egui::Color32::from_black_alpha(115),
                );
                if scrim.clicked() {
                    close = true;
                }
                ui.scope_builder(egui::UiBuilder::new().max_rect(card), |ui| {
                    egui::Frame::NONE
                        .fill(palette.bg)
                        .stroke(egui::Stroke::new(1.0, palette.line))
                        .corner_radius(egui::CornerRadius::same(R_XL as u8))
                        .shadow(palette.shadow_2)
                        .show(ui, |ui| {
                            ui.set_min_size(card.size());
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                nav(ui, &self.settings, palette, &mut picked);
                                ui.vertical(|ui| {
                                    ui.set_width(card.width() - 224.0);
                                    ui.set_min_height(card.height());
                                    ui.horizontal(|ui| {
                                        ui.add_space(ui.available_width() - 40.0);
                                        let response = ui.add(
                                            egui::Button::new(
                                                egui::RichText::new("\u{2715}")
                                                    .size(FS_SM)
                                                    .color(palette.muted),
                                            )
                                            .fill(egui::Color32::TRANSPARENT)
                                            .stroke(egui::Stroke::NONE)
                                            .corner_radius(egui::CornerRadius::same(R_MD as u8))
                                            .min_size(egui::vec2(28.0, 28.0)),
                                        );
                                        if response.clicked() {
                                            close = true;
                                        }
                                    });
                                    outcome = self.settings.show(
                                        ui,
                                        store,
                                        palette,
                                        updates,
                                        &mut events,
                                    );
                                });
                            });
                        });
                });
            });
        if let Some(section) = picked {
            self.settings.go(section);
            if section == Section::Agents {
                store.ask_engine_options();
            }
        }
        for event in &events {
            match event {
                SettingsEvent::ProviderForm(target) => {
                    let config = store.config().cloned().unwrap_or(serde_json::Value::Null);
                    let form = crate::ui::provider_form::ProviderForm::new(target.clone(), &config);
                    self.settings.open_form(Form::Provider(Box::new(form)));
                }
                SettingsEvent::McpForm(name) => {
                    let config = store.config().cloned().unwrap_or(serde_json::Value::Null);
                    let snapshot = crate::ui::settings_config::Snapshot::parse(&config);
                    let server = name.as_deref().and_then(|name| snapshot.mcp_server(name));
                    let form = crate::ui::mcp_form::McpForm::new(name.as_deref().zip(server));
                    self.settings.open_form(Form::Mcp(Box::new(form)));
                }
                SettingsEvent::AcpForm(name) => {
                    let config = store.config().cloned().unwrap_or(serde_json::Value::Null);
                    let snapshot = crate::ui::settings_config::Snapshot::parse(&config);
                    let agent = name
                        .as_deref()
                        .and_then(|name| snapshot.acp_agent(name))
                        .cloned();
                    let form =
                        crate::ui::acp_form::AcpForm::new(name.as_deref().zip(agent.as_ref()));
                    self.settings.open_form(Form::Acp(Box::new(form)));
                }
                SettingsEvent::Directory(named) => {
                    let picker = crate::ui::directory_picker::DirectoryPicker::new(store, *named);
                    self.settings.open_form(Form::Directory(Box::new(picker)));
                }
                _ => {}
            }
        }
        events.retain(|event| {
            !matches!(
                event,
                SettingsEvent::ProviderForm(_)
                    | SettingsEvent::McpForm(_)
                    | SettingsEvent::AcpForm(_)
                    | SettingsEvent::Directory(_)
            )
        });
        match outcome {
            FormOutcome::Picked(picked) => {
                let config = store.config().cloned().unwrap_or(serde_json::Value::Null);
                let form = crate::ui::provider_form::ProviderForm::new(
                    crate::ui::provider_form::Target::Picked(picked.id, picked.entry),
                    &config,
                );
                self.settings.open_form(Form::Provider(Box::new(form)));
            }
            FormOutcome::Done(true) => {
                self.settings.close_form();
                if self.settings.section().reads_config() {
                    store.ask_config();
                }
            }
            FormOutcome::Done(false) => {}
        }
        if close {
            self.settings.close();
        }
        events
    }
}

/// The nav column: the title and one row per section.
fn nav(ui: &mut egui::Ui, settings: &Settings, palette: &Palette, picked: &mut Option<Section>) {
    ui.set_width(224.0);
    ui.set_min_height(ui.available_height());
    let painter = ui.painter();
    let rect = egui::Rect::from_min_size(
        ui.min_rect().min,
        egui::vec2(224.0, ui.available_height().max(ui.min_rect().height())),
    );
    painter.rect_filled(rect, egui::CornerRadius::ZERO, palette.sidebar);
    painter.vline(
        rect.right(),
        egui::Rangef::new(rect.top(), rect.bottom()),
        egui::Stroke::new(1.0, palette.line),
    );
    ui.add_space(16.0);
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new("Settings")
                .size(16.0)
                .family(family(FAMILY_BODY_SEMIBOLD))
                .color(palette.ink_strong),
        );
    });
    ui.add_space(14.0);
    for section in Section::ALL {
        let on = section == settings.section();
        let (fg, fill) = if on {
            (palette.ink_strong, palette.selected)
        } else {
            (palette.muted, egui::Color32::TRANSPARENT)
        };
        let row = egui::Button::new(egui::RichText::new(section.label()).size(FS_SM).color(fg))
            .fill(fill)
            .stroke(egui::Stroke::NONE)
            .corner_radius(egui::CornerRadius::same(R_MD as u8))
            .min_size(egui::vec2(204.0, 36.0));
        let response = ui.add(row);
        let icon_rect = egui::Rect::from_min_size(
            egui::pos2(response.rect.left() + 10.0, response.rect.center().y - 7.0),
            egui::vec2(14.0, 14.0),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(icon_rect), |ui| {
            crate::icons::icon(ui, section.icon(), 14.0, fg);
        });
        if response.clicked() {
            *picked = Some(section);
        }
    }
}

/// One page of the dialog, scrolling inside the page column.
fn page(
    ui: &mut egui::Ui,
    store: &mut Store,
    section: Section,
    palette: &Palette,
    updates: &UpdateState,
    events: &mut Vec<SettingsEvent>,
) {
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add_space(22.0);
            ui.horizontal(|ui| {
                ui.add_space(32.0);
                ui.label(
                    egui::RichText::new(section.label())
                        .size(17.0)
                        .family(family(FAMILY_BODY_SEMIBOLD))
                        .color(palette.ink_strong),
                );
            });
            ui.add_space(6.0);
            match section {
                Section::General => general(ui, store, palette, events),
                Section::Agents => agents(ui, store, palette, events),
                Section::Providers
                | Section::Mcp
                | Section::Acp
                | Section::Permissions
                | Section::Plugins => config_page(ui, store, section, palette, events),
                Section::Connection => connection(ui, store, palette),
                Section::Keyboard => keyboard(ui, store, palette),
                Section::Lab => lab(ui, store, palette, events),
                Section::Archived => archived(ui, store, palette, events),
                Section::About => about(ui, store, palette, updates, events),
            }
        });
}

/// The General page: the theme cards and the input and session toggles.
fn general(ui: &mut egui::Ui, store: &Store, palette: &Palette, events: &mut Vec<SettingsEvent>) {
    let prefs = store.prefs();
    group(ui, "Appearance", palette);
    ui.horizontal(|ui| {
        ui.add_space(32.0);
        ui.spacing_mut().item_spacing.x = 10.0;
        for choice in [
            crate::theme::ThemeChoice::System,
            crate::theme::ThemeChoice::Named("kage-shadow".to_owned()),
            crate::theme::ThemeChoice::Named("kage-dawn".to_owned()),
        ] {
            theme_card(ui, choice, prefs, palette, events);
        }
    });
    group(ui, "Input", palette);
    let mut vim = prefs.vim;
    toggle_row(
        ui,
        "Vim mode",
        "Normal-mode motions over the transcript (j k gg G za / n), a : command line and a modeline. Esc leaves the composer.",
        &mut vim,
        palette,
    );
    if vim != prefs.vim {
        events.push(SettingsEvent::Flip("vim"));
    }
    let mut enter = prefs.enter_sends;
    toggle_row(
        ui,
        "Enter sends",
        "Off: Enter adds a newline and Ctrl+Enter sends",
        &mut enter,
        palette,
    );
    if enter != prefs.enter_sends {
        events.push(SettingsEvent::Flip("enter_sends"));
    }
    group(ui, "Sessions", palette);
    let mut group_by = prefs.group_by_project;
    toggle_row(
        ui,
        "Group sessions by project",
        "Show sessions under their project folder in the sidebar",
        &mut group_by,
        palette,
    );
    if group_by != prefs.group_by_project {
        events.push(SettingsEvent::Flip("group_by_project"));
    }
    let mut confirm = prefs.confirm_swarm;
    toggle_row(
        ui,
        "Ask before enabling swarm",
        "Confirm when turning swarm mode on",
        &mut confirm,
        palette,
    );
    if confirm != prefs.confirm_swarm {
        events.push(SettingsEvent::Flip("confirm_swarm"));
    }
    group(ui, "Chat", palette);
    let mut smooth = prefs.smooth_stream;
    toggle_row(
        ui,
        "Smooth streaming",
        "Show a reply that arrives in bursts at a steady pace, a moment behind",
        &mut smooth,
        palette,
    );
    if smooth != prefs.smooth_stream {
        events.push(SettingsEvent::Flip("smooth_stream"));
    }
    let mut rail = prefs.rail;
    toggle_row(
        ui,
        "Turn timeline rail",
        "A minimap beside the transcript with ticks for turns, edits, approvals, failures and swarms",
        &mut rail,
        palette,
    );
    if rail != prefs.rail {
        events.push(SettingsEvent::Flip("rail"));
    }
}

/// The Agent & Sessions page: the engine's options, grouped as the
/// design groups them. Each row writes its option back to the user
/// config.
fn agents(ui: &mut egui::Ui, store: &Store, palette: &Palette, events: &mut Vec<SettingsEvent>) {
    let Some(options) = store.engine_options() else {
        note(ui, "Reading the engine's options\u{2026}", palette);
        return;
    };
    let find = |name: &str| options.iter().find(|option| option.name == name);
    let groups: [(&str, &[(&str, &str)]); 4] = [
        (
            "Session defaults",
            &[
                ("thinking_level", "Thinking by default"),
                ("compaction_threshold", "Compact at"),
            ],
        ),
        (
            "Subagents",
            &[
                ("agent_max_depth", "Max depth"),
                ("agent_max_running", "Max running"),
                ("agent_max_turns", "Turn limit"),
                ("agent_timeout", "Run timeout"),
                ("agent_budget", "Token budget"),
            ],
        ),
        (
            "Swarm",
            &[
                ("swarm_max_items", "Max items per swarm"),
                ("swarm_timeout_ms", "Worker timeout"),
            ],
        ),
        ("", &[]),
    ];
    for (title, rows) in groups.iter().take(3) {
        group(ui, title, palette);
        boxed(ui, palette, |ui| {
            for (name, label) in rows.iter() {
                let Some(option) = find(name) else {
                    continue;
                };
                row(
                    ui,
                    label,
                    &option.doc,
                    |ui| option_control(ui, option, palette, events),
                    palette,
                );
            }
        });
    }
    note(
        ui,
        "These live in the user config.toml; a project config that sets the same key still wins. Changes apply to sessions started afterwards.",
        palette,
    );
}

/// The control of one engine option: segments for a choice, a stepper
/// for a number or a fraction.
fn option_control(
    ui: &mut egui::Ui,
    option: &kage_client::wire::OptionEntry,
    palette: &Palette,
    events: &mut Vec<SettingsEvent>,
) {
    if option.kind == "choice" {
        let current = option.value.as_str().unwrap_or_default();
        for value in &option.values {
            let label = match value.as_str() {
                "" => "Auto".to_owned(),
                "xhigh" => "Max".to_owned(),
                other => {
                    let mut word = other.to_owned();
                    word.get_mut(0..1).map(|first| first.make_ascii_uppercase());
                    word
                }
            };
            if ui.selectable_label(current == value, label).clicked() {
                events.push(SettingsEvent::EngineOption(
                    option.name.clone(),
                    serde_json::Value::from(value.as_str()),
                ));
            }
        }
        return;
    }
    let fraction = option.kind == "fraction";
    let current = option.value.as_f64().unwrap_or(0.0);
    let (step, shown) = match (fraction, option.name.as_str()) {
        (true, _) => (0.05, format!("{}%", (current * 100.0).round())),
        (false, "swarm_timeout_ms") => (60_000.0, format!("{} min", current as i64 / 60_000)),
        (false, "agent_max_turns") => (10.0, off_or(current, current.to_string())),
        (false, "agent_timeout") => (
            60.0,
            off_or(current, format!("{} min", current as i64 / 60)),
        ),
        (false, "agent_budget") => (
            100_000.0,
            off_or(current, format!("{}k", current as i64 / 1000)),
        ),
        _ => (1.0, (current as i64).to_string()),
    };
    let (min, max) = if fraction {
        (0.0, 1.0)
    } else {
        (
            option.min.unwrap_or(i64::MIN) as f64,
            option.max.unwrap_or(i64::MAX) as f64,
        )
    };
    let down = (current - step).clamp(min, max);
    let up = (current + step).clamp(min, max);
    let mut set = |next: f64| {
        let value = if fraction {
            serde_json::Value::from((next * 100.0).round() / 100.0)
        } else {
            serde_json::Value::from(next as i64)
        };
        events.push(SettingsEvent::EngineOption(option.name.clone(), value));
    };
    if stepper_button(ui, crate::icons::Icon::Plus, up != current, palette) {
        set(up);
    }
    ui.label(
        egui::RichText::new(shown)
            .size(FS_XS)
            .family(family(FAMILY_MONO_REGULAR))
            .color(palette.ink),
    );
    if stepper_button(ui, crate::icons::Icon::Minus, down != current, palette) {
        set(down);
    }
}

/// `text` for a value, or "Off" when the value is zero.
fn off_or(value: f64, text: String) -> String {
    if value == 0.0 { "Off".to_owned() } else { text }
}

/// One stepper arrow, dimmed and inert when it would not move the value.
fn stepper_button(
    ui: &mut egui::Ui,
    icon: crate::icons::Icon,
    enabled: bool,
    palette: &Palette,
) -> bool {
    let tint = if enabled {
        palette.muted
    } else {
        palette.faint
    };
    ui.add_enabled(
        enabled,
        egui::Button::image(crate::icons::themed_image(icon, 12.0, tint))
            .fill(egui::Color32::TRANSPARENT)
            .min_size(egui::vec2(26.0, 26.0)),
    )
    .clicked()
}

/// One theme card: the swatch and the name, chosen on a click.
fn theme_card(
    ui: &mut egui::Ui,
    choice: crate::theme::ThemeChoice,
    prefs: &Prefs,
    palette: &Palette,
    events: &mut Vec<SettingsEvent>,
) {
    let on = prefs.theme == choice;
    let (side, main, accent) = match &choice {
        crate::theme::ThemeChoice::System => {
            let (shadow, dawn) = (
                crate::theme::Palette::shadow(),
                crate::theme::Palette::dawn(),
            );
            (shadow.bg, dawn.bg, shadow.accent)
        }
        crate::theme::ThemeChoice::Named(name) if name == "kage-dawn" => {
            let dawn = crate::theme::Palette::dawn();
            (dawn.bg, dawn.surface, dawn.accent)
        }
        crate::theme::ThemeChoice::Named(_) => {
            let shadow = crate::theme::Palette::shadow();
            (shadow.bg, shadow.surface, shadow.accent)
        }
    };
    let width = (ui.available_width() - 20.0).max(120.0) / 3.0;
    let card = egui::Button::new("")
        .fill(palette.surface)
        .stroke(egui::Stroke::new(
            1.0,
            if on { palette.accent } else { palette.line },
        ))
        .corner_radius(egui::CornerRadius::same(R_LG as u8))
        .min_size(egui::vec2(width, 92.0));
    let response = ui.add(card);
    let rect = response.rect;
    if response.hovered() && !on {
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(R_LG as u8),
            egui::Stroke::new(1.0, palette.line_strong),
            egui::StrokeKind::Inside,
        );
    }
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.add_space(8.0);
        let swatch = egui::Rect::from_min_size(
            egui::pos2(rect.left() + 8.0, rect.top() + 8.0),
            egui::vec2(rect.width() - 16.0, 58.0),
        );
        let _ = ui.allocate_rect(swatch, egui::Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(swatch, egui::CornerRadius::same(R_MD as u8), main);
        painter.rect_filled(
            egui::Rect::from_min_size(
                swatch.min,
                egui::vec2(swatch.width() * 0.28, swatch.height()),
            ),
            egui::CornerRadius::same(R_MD as u8),
            side,
        );
        painter.rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(swatch.left() + 8.0, swatch.bottom() - 16.0),
                egui::vec2(swatch.width() * 0.4, 8.0),
            ),
            egui::CornerRadius::same(4),
            accent,
        );
        ui.add_space(66.0);
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(choice.label())
                        .size(FS_XS)
                        .color(palette.ink),
                )
                .selectable(false),
            );
        });
    });
    if response.clicked() {
        events.push(SettingsEvent::Theme(choice));
    }
}

/// One page the configuration snapshot backs.
fn config_page(
    ui: &mut egui::Ui,
    store: &Store,
    section: Section,
    palette: &Palette,
    events: &mut Vec<SettingsEvent>,
) {
    let Some(config) = store.config() else {
        note(ui, "Reading the engine's configuration\u{2026}", palette);
        return;
    };
    let snapshot = config::Snapshot::parse(config);
    let session = store.active_session();
    let option = |id: &str| {
        session.and_then(|session| session.config_options.iter().find(|option| option.id == id))
    };
    match section {
        Section::Providers => {
            config::providers_page(ui, &snapshot, option("model"), palette, events);
        }
        Section::Mcp => {
            let live = session
                .map(|session| session.mcp.clone())
                .unwrap_or_default();
            config::mcp_page(ui, &snapshot, &live, palette, events);
        }
        Section::Acp => {
            config::acp_page(ui, &snapshot, palette, events);
        }
        Section::Permissions => {
            let modes: Option<SessionConfigOption> = option("mode").map(|option| {
                let mut option = option.clone();
                option
                    .options
                    .retain(|choice| choice.value != crate::modes::PLAN_MODE);
                option
            });
            let current = store.permission_mode();
            config::permissions_page(
                ui,
                &snapshot,
                modes.as_ref(),
                current.as_deref(),
                palette,
                events,
            );
        }
        Section::Plugins => config::plugins_page(ui, &snapshot, palette),
        _ => {}
    }
}

/// The Connection page: the transport and its status.
fn connection(ui: &mut egui::Ui, store: &Store, palette: &Palette) {
    let link = store.link();
    let state = store.state();
    let (word, dot) = match store.connect() {
        crate::transport::State::Connected => ("Connected", palette.ok),
        crate::transport::State::Connecting => ("Connecting", palette.warn),
        crate::transport::State::Reconnecting { .. } => ("Reconnecting", palette.warn),
        crate::transport::State::Refused(_) => ("Refused", palette.danger),
        crate::transport::State::Closed => ("Closed", palette.faint),
    };
    let version = state
        .agent
        .as_ref()
        .and_then(|agent| agent.version.clone())
        .map_or_else(|| "unknown".to_owned(), |v| format!("kage {v}"));
    let mut features: Vec<&str> = Vec::new();
    if state.steer_available() {
        features.push("steer");
    }
    if state
        .capabilities
        .as_ref()
        .is_some_and(|caps| caps.load_session)
    {
        features.push("session/load");
    }
    let open = state.sessions.values().filter(|s| s.opened).count();
    let detail = format!(
        "{} \u{b7} {version} \u{b7} {open} sessions{}",
        link.detail,
        if features.is_empty() {
            String::new()
        } else {
            format!(" \u{b7} {}", features.join(", "))
        }
    );
    group(ui, "Transport", palette);
    boxed(ui, palette, |ui| {
        row(
            ui,
            "Connect with",
            "",
            |ui| {
                badge(ui, link.name, palette);
            },
            palette,
        );
        row(
            ui,
            "Endpoint",
            "",
            |ui| {
                ui.label(
                    egui::RichText::new(link.detail.clone())
                        .size(FS_XS)
                        .family(family(FAMILY_MONO_REGULAR))
                        .color(palette.ink),
                );
            },
            palette,
        );
    });
    group(ui, "Status", palette);
    boxed(ui, palette, |ui| {
        row(
            ui,
            word,
            &detail,
            |ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.0, dot);
            },
            palette,
        );
    });
}

/// The Keyboard page: the app bindings and the composer's keys.
fn keyboard(ui: &mut egui::Ui, store: &Store, palette: &Palette) {
    let enter_sends = store.prefs().enter_sends;
    let mut composer: Vec<(&str, &str)> = if enter_sends {
        vec![
            ("Send", "Enter"),
            ("Newline", "Shift Enter"),
            ("Queue while running", "Enter"),
        ]
    } else {
        vec![
            ("Send", "Ctrl Enter"),
            ("Newline", "Enter"),
            ("Queue while running", "Ctrl Enter"),
        ]
    };
    composer.push(("Interrupt", "Esc Esc"));
    composer.push(("Answer approval", "1 2 3"));
    group(ui, "Shortcuts", palette);
    let app: Vec<(String, String)> = shortcuts()
        .into_iter()
        .map(|row| (row.label.to_owned(), row.keys))
        .collect();
    table(ui, &app, palette);
    group(ui, "Composer", palette);
    let owned: Vec<(String, String)> = composer
        .into_iter()
        .map(|(a, b)| (a.to_owned(), b.to_owned()))
        .collect();
    table(ui, &owned, palette);
}

/// The Lab page: the experiments behind toggles.
fn lab(ui: &mut egui::Ui, store: &Store, palette: &Palette, events: &mut Vec<SettingsEvent>) {
    let prefs = store.prefs();
    note(ui, "Experiments that may change or go away.", palette);
    let mut constellation = prefs.constellation;
    toggle_row(
        ui,
        "Swarm constellation",
        "Swarm cards draw one star per worker, lit by state",
        &mut constellation,
        palette,
    );
    if constellation != prefs.constellation {
        events.push(SettingsEvent::Flip("constellation"));
    }
    let mut fuel = prefs.fuel;
    toggle_row(
        ui,
        "Context gauge",
        "The context ring opens a gauge with Compact now",
        &mut fuel,
        palette,
    );
    if fuel != prefs.fuel {
        events.push(SettingsEvent::Flip("fuel"));
    }
}

/// The Archived Sessions page: the archive, with a restore each.
fn archived(ui: &mut egui::Ui, store: &Store, palette: &Palette, events: &mut Vec<SettingsEvent>) {
    let ids: Vec<String> = store.prefs().archived.iter().cloned().collect();
    if ids.is_empty() {
        note(
            ui,
            "Nothing archived. Archive a session from its \u{2026} menu.",
            palette,
        );
        return;
    }
    note(
        ui,
        "The archive is this client's own mark; the session stays recorded on the engine.",
        palette,
    );
    boxed(ui, palette, |ui| {
        for id in ids {
            let title = store
                .session_title(&id)
                .unwrap_or("untitled session")
                .to_owned();
            let response = ui.add(
                egui::Button::new(egui::RichText::new(&title).size(FS_SM).color(palette.ink))
                    .fill(egui::Color32::TRANSPARENT)
                    .stroke(egui::Stroke::NONE)
                    .corner_radius(egui::CornerRadius::ZERO)
                    .min_size(egui::vec2(ui.available_width(), 44.0)),
            );
            if response.clicked() {
                events.push(SettingsEvent::Restore(id.clone()));
            }
        }
    });
}

/// The About page: the mark, the versions and the protocol.
fn about(
    ui: &mut egui::Ui,
    store: &Store,
    palette: &Palette,
    update_state: &UpdateState,
    events: &mut Vec<SettingsEvent>,
) {
    let state = store.state();
    let engine = state
        .agent
        .as_ref()
        .and_then(|agent| agent.version.clone())
        .map_or_else(|| "engine unknown".to_owned(), |v| format!("engine {v}"));
    let protocol = state
        .protocol_version
        .map_or_else(|| "unknown".to_owned(), |v| format!("ACP v{v}"));
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(32.0);
        ui.spacing_mut().item_spacing.x = 14.0;
        crate::ui::eclipse::eclipse(ui, 48.0, None);
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new("kage")
                    .size(18.0)
                    .family(family(FAMILY_BODY_SEMIBOLD))
                    .color(palette.ink_strong),
            );
            ui.label(
                egui::RichText::new(format!(
                    "desktop {} \u{b7} {engine}",
                    env!("CARGO_PKG_VERSION")
                ))
                .size(FS_SM)
                .color(palette.muted),
            );
        });
    });
    ui.add_space(18.0);
    boxed(ui, palette, |ui| {
        row(
            ui,
            "Protocol",
            "",
            |ui| {
                badge(ui, &protocol, palette);
            },
            palette,
        );
    });
    updates(ui, store, palette, update_state, events);
}
/// What the release check and the installs in the Updates row are
/// doing right now. The shell owns it; the page only reads it, so a
/// second install cannot stack on a first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdateState {
    /// Which install the in-flight download is for.
    pub installing: Option<Install>,
    /// Whether a release check is running, past the daily throttle.
    pub checking: bool,
    /// What the last install ended with: true when it worked.
    pub install_note: Option<(bool, String)>,
}

/// The Updates row: what the last release check saw for the engine and
/// this client, the channel choice, a manual re-check, and the install
/// buttons a newer build puts on screen.
fn updates(
    ui: &mut egui::Ui,
    store: &Store,
    palette: &Palette,
    update_state: &UpdateState,
    events: &mut Vec<SettingsEvent>,
) {
    let prefs = store.prefs();
    let engine = store
        .state()
        .agent
        .as_ref()
        .and_then(|agent| agent.version.clone());
    let desktop = env!("CARGO_PKG_VERSION");
    let nightly = prefs.channel == crate::prefs::Channel::Nightly;
    let (engine_line, desktop_line) = if nightly {
        let engine_line = match prefs.latest_nightly.as_ref() {
            Some(build) => format!(
                "engine: nightly {} ({}) published",
                build.date,
                &build.commit[..build.commit.len().min(7)]
            ),
            None => "engine: no nightly seen yet".to_owned(),
        };
        let desktop_line = match prefs.latest_nightly.as_ref() {
            Some(build) if option_env!("KAGE_BUILD_SHA").is_some_and(|sha| sha == build.commit) => {
                format!("desktop {desktop}; on the {} nightly", build.date)
            }
            Some(build) => format!("desktop {desktop}; nightly {} available", build.date),
            None => "desktop: no nightly seen yet".to_owned(),
        };
        (engine_line, desktop_line)
    } else {
        let engine_line = match (prefs.latest_cli_version.as_deref(), engine.as_deref()) {
            (Some(latest), Some(now)) if crate::update::is_newer(latest, now) => {
                format!("engine {now}; {latest} is available")
            }
            (Some(latest), Some(now)) => format!("engine {now}; current with {latest}"),
            (Some(latest), None) => format!("engine version unknown; {latest} is available"),
            (None, _) => "engine: no check has run yet".to_owned(),
        };
        let desktop_line = match prefs.latest_desktop_version.as_deref() {
            Some(latest) if crate::update::is_newer(latest, desktop) => {
                format!("desktop {desktop}; {latest} is available")
            }
            Some(latest) => format!("desktop {desktop}; current with {latest}"),
            None => "desktop: no check has run yet".to_owned(),
        };
        (engine_line, desktop_line)
    };
    let engine_behind = if nightly {
        prefs.latest_nightly.is_some()
    } else {
        matches!(
            (prefs.latest_cli_version.as_deref(), engine.as_deref()),
            (Some(latest), Some(now)) if crate::update::is_newer(latest, now)
        )
    };
    let desktop_behind = if nightly {
        prefs.latest_nightly.as_ref().is_some_and(|build| {
            option_env!("KAGE_BUILD_SHA").is_some_and(|sha| sha != build.commit)
        })
    } else {
        prefs
            .latest_desktop_version
            .as_deref()
            .is_some_and(|latest| crate::update::is_newer(latest, desktop))
    };
    row(
        ui,
        "Updates",
        "The public releases, checked at most once a day",
        |ui| {
            ui.vertical(|ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(&engine_line).size(FS_XS))
                        .selectable(false),
                );
                ui.add(
                    egui::Label::new(egui::RichText::new(&desktop_line).size(FS_XS))
                        .selectable(false),
                );
                if let Some(why) = &prefs.check_error {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(format!("last check failed: {why}"))
                                .size(FS_XS)
                                .color(palette.warn),
                        )
                        .selectable(false),
                    );
                }
                ui.horizontal(|ui| {
                    let stable = crate::ui::forms::button(
                        ui,
                        "Stable",
                        if nightly {
                            crate::ui::forms::Tone::Plain
                        } else {
                            crate::ui::forms::Tone::Primary
                        },
                        palette,
                    );
                    if stable.clicked() {
                        events.push(SettingsEvent::Channel(crate::prefs::Channel::Latest));
                    }
                    let nightly_button = crate::ui::forms::button(
                        ui,
                        "Nightly",
                        if nightly {
                            crate::ui::forms::Tone::Primary
                        } else {
                            crate::ui::forms::Tone::Plain
                        },
                        palette,
                    );
                    if nightly_button.clicked() {
                        events.push(SettingsEvent::Channel(crate::prefs::Channel::Nightly));
                    }
                    let check = crate::ui::forms::button(
                        ui,
                        if update_state.checking {
                            "Checking\u{2026}"
                        } else {
                            "Check now"
                        },
                        crate::ui::forms::Tone::Plain,
                        palette,
                    );
                    if check.clicked() {
                        events.push(SettingsEvent::CheckNow);
                    }
                    if engine_behind {
                        let label = if update_state.installing == Some(Install::Engine) {
                            "Installing\u{2026}"
                        } else {
                            "Install engine"
                        };
                        let install = crate::ui::forms::button(
                            ui,
                            label,
                            crate::ui::forms::Tone::Primary,
                            palette,
                        );
                        if install.clicked() {
                            events.push(SettingsEvent::Install(Install::Engine));
                        }
                    }
                    if desktop_behind {
                        let label = if update_state.installing == Some(Install::Desktop) {
                            "Installing\u{2026}"
                        } else {
                            "Install desktop"
                        };
                        let install = crate::ui::forms::button(
                            ui,
                            label,
                            crate::ui::forms::Tone::Primary,
                            palette,
                        );
                        if install.clicked() {
                            events.push(SettingsEvent::Install(Install::Desktop));
                        }
                    }
                    if crate::ui::forms::button(
                        ui,
                        "Releases",
                        crate::ui::forms::Tone::Plain,
                        palette,
                    )
                    .clicked()
                    {
                        events.push(SettingsEvent::OpenReleases);
                    }
                });
                if let Some((ok, text)) = &update_state.install_note {
                    ui.add(
                        egui::Label::new(egui::RichText::new(text).size(FS_XS).color(if *ok {
                            palette.ok
                        } else {
                            palette.warn
                        }))
                        .selectable(false),
                    );
                }
            });
        },
        palette,
    );
}

/// A group heading above a page section.
fn group(ui: &mut egui::Ui, text: &str, palette: &Palette) {
    ui.add_space(22.0);
    ui.horizontal(|ui| {
        ui.add_space(32.0);
        ui.label(
            egui::RichText::new(text)
                .size(FS_SM)
                .family(family(FAMILY_BODY_SEMIBOLD))
                .color(palette.ink_strong),
        );
    });
    ui.add_space(8.0);
}

/// A muted note under a page section.
fn note(ui: &mut egui::Ui, text: &str, palette: &Palette) {
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        ui.add_space(32.0);
        ui.label(egui::RichText::new(text).size(FS_XS).color(palette.faint));
    });
    ui.add_space(8.0);
}

/// The rounded box rows sit in; each row after the first carries a
/// hairline the box paints over its top.
fn boxed(ui: &mut egui::Ui, palette: &Palette, add: impl FnOnce(&mut egui::Ui)) {
    let width = ui.available_width();
    egui::Frame::NONE
        .fill(palette.surface)
        .stroke(egui::Stroke::new(1.0, palette.subtle))
        .corner_radius(egui::CornerRadius::same(R_LG as u8))
        .inner_margin(egui::Margin::symmetric(16, 0))
        .show(ui, |ui| {
            ui.set_min_width(width - 32.0);
            add(ui);
        });
    ui.add_space(8.0);
}

/// One labeled row with its control drawn by `control` at the far end.
fn row(
    ui: &mut egui::Ui,
    label: &str,
    hint: &str,
    control: impl FnOnce(&mut egui::Ui),
    palette: &Palette,
) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width.max(1.0), 58.0), egui::Sense::hover());
    ui.painter().line_segment(
        [
            egui::pos2(rect.left(), rect.top()),
            egui::pos2(rect.right(), rect.top()),
        ],
        egui::Stroke::new(1.0, palette.subtle),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            ui.spacing_mut().item_spacing.x = 16.0;
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(label).size(FS_SM).color(palette.ink));
                if !hint.is_empty() {
                    ui.label(egui::RichText::new(hint).size(FS_XS).color(palette.muted));
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(16.0);
                control(ui);
            });
        });
    });
}

/// One toggle row: the label, its hint and the switch, whose new value
/// the caller compares against the stored one. The whole row answers,
/// as the design's control did.
fn toggle_row(ui: &mut egui::Ui, label: &str, hint: &str, on: &mut bool, palette: &Palette) {
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width.max(1.0), 58.0), egui::Sense::click());
    ui.painter().line_segment(
        [
            egui::pos2(rect.left(), rect.top()),
            egui::pos2(rect.right(), rect.top()),
        ],
        egui::Stroke::new(1.0, palette.subtle),
    );
    let track = egui::Rect::from_min_size(
        egui::pos2(rect.right() - 52.0, rect.center().y - 10.0),
        egui::vec2(36.0, 20.0),
    );
    let painter = ui.painter();
    painter.rect_filled(
        track,
        egui::CornerRadius::same(R_FULL as u8),
        if *on {
            palette.accent
        } else {
            palette.fill_hover
        },
    );
    let knob = egui::pos2(
        if *on {
            track.right() - 10.0
        } else {
            track.left() + 10.0
        },
        track.center().y,
    );
    painter.circle_filled(knob, 8.0, egui::Color32::WHITE);
    if response.clicked() {
        *on = !*on;
    }
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            ui.vertical(|ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(label).size(FS_SM).color(palette.ink))
                        .selectable(false),
                );
                ui.add(
                    egui::Label::new(egui::RichText::new(hint).size(FS_XS).color(palette.muted))
                        .selectable(false),
                );
            });
        });
    });
}

/// A shortcut table: one row per entry, the keys as caps at the right.
fn table(ui: &mut egui::Ui, rows: &[(String, String)], palette: &Palette) {
    boxed(ui, palette, |ui| {
        for (label, keys) in rows {
            let width = ui.available_width();
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(width.max(1.0), 34.0), egui::Sense::hover());
            ui.painter().line_segment(
                [
                    egui::pos2(rect.left(), rect.top()),
                    egui::pos2(rect.right(), rect.top()),
                ],
                egui::Stroke::new(1.0, palette.subtle),
            );
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.label(egui::RichText::new(label).size(FS_SM).color(palette.ink));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(16.0);
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for cap in keys.split(' ') {
                            key_cap(ui, cap, palette);
                        }
                    });
                });
            });
        }
    });
}

/// One key cap: a hairline box in the mono family.
fn key_cap(ui: &mut egui::Ui, text: &str, palette: &Palette) {
    let width = text.len() as f32 * 7.0 + 10.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 18.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(4), palette.fill);
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(4),
        egui::Stroke::new(1.0, palette.line),
        egui::StrokeKind::Inside,
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new(text)
                    .size(10.5)
                    .family(family(FAMILY_MONO_REGULAR))
                    .color(palette.muted),
            );
        });
    });
}

/// One outlined badge.
fn badge(ui: &mut egui::Ui, text: &str, palette: &Palette) {
    egui::Frame::NONE
        .fill(palette.fill)
        .stroke(egui::Stroke::new(1.0, palette.line))
        .corner_radius(egui::CornerRadius::same(R_FULL as u8))
        .inner_margin(egui::Margin::symmetric(7, 1))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(FS_2XS).color(palette.muted));
        });
}

#[cfg(test)]
mod tests {
    use super::{Section, Settings, SettingsEvent, shortcuts};

    #[test]
    fn every_section_has_a_label_and_an_icon() {
        for section in Section::ALL {
            assert!(!section.label().is_empty(), "{section:?} has a label");
            let _ = section.icon();
        }
        assert_eq!(Section::ALL.len(), 12);
    }

    #[test]
    fn the_config_sections_ask_for_a_snapshot() {
        for section in Section::ALL {
            let reads = matches!(
                section,
                Section::Providers
                    | Section::Mcp
                    | Section::Acp
                    | Section::Permissions
                    | Section::Plugins
            );
            assert_eq!(section.reads_config(), reads, "{section:?}");
        }
        assert!(Section::Providers.reads_config());
        assert!(Section::Acp.reads_config());
        assert!(!Section::General.reads_config());
    }

    #[test]
    fn the_dialog_opens_on_a_section_and_closes() {
        let mut settings = Settings::new();
        assert!(!settings.is_open());
        settings.open(Section::Providers);
        assert!(settings.is_open());
        assert_eq!(settings.section(), Section::Providers);
        settings.go(Section::About);
        assert_eq!(settings.section(), Section::About);
        settings.close();
        assert!(!settings.is_open());
    }

    #[test]
    fn the_shortcut_table_lists_every_app_binding_once() {
        let rows = shortcuts();
        let labels: Vec<&str> = rows.iter().map(|row| row.label).collect();
        for wanted in [
            "Command palette",
            "New session",
            "Toggle sidebar",
            "Toggle workbench",
            "Settings",
            "Find in chat",
            "Send, or steer the running turn",
            "Quit",
        ] {
            assert_eq!(
                labels.iter().filter(|label| **label == wanted).count(),
                1,
                "{wanted} in {labels:?}"
            );
        }
        let palette = rows
            .iter()
            .find(|row| row.label == "Command palette")
            .expect("the palette is bound");
        let expected = if cfg!(target_os = "macos") {
            "Cmd K"
        } else {
            "Ctrl K"
        };
        assert_eq!(palette.keys, expected);
        let settings = rows
            .iter()
            .find(|row| row.label == "Settings")
            .expect("settings is bound");
        assert_eq!(settings.keys, "Ctrl ,");
    }

    #[test]
    fn the_events_name_their_preference_field() {
        assert_eq!(
            SettingsEvent::Flip("enter_sends"),
            SettingsEvent::Flip("enter_sends")
        );
        assert_ne!(
            SettingsEvent::Flip("enter_sends"),
            SettingsEvent::Flip("fuel")
        );
    }
}
