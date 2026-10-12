//! The settings pages read from the engine's `_kage/config/get`
//! snapshot: model providers, MCP servers, permissions and plugins.
//!
//! The pages say only what the snapshot and the live session carry. A
//! fact the snapshot leaves out reads unknown rather than guessed, and
//! nothing here writes config: the engine has no write method yet, so
//! the controls that would need one are left out.

use std::collections::BTreeMap;

use kage_client::wire::{McpServerStatus, SessionConfigOption};
use serde::Deserialize;

use crate::theme::{FAMILY_MONO_REGULAR, FS_SM, FS_XS, Palette, R_FULL, R_LG, family};
use crate::ui::settings::SettingsEvent;

/// The parts of the snapshot the pages read. Every field defaults, so a
/// section the engine leaves out reads as empty.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    providers: Providers,
    mcp: Mcp,
    permissions: Permissions,
    plugins: Plugins,
}

impl Snapshot {
    /// Reads a snapshot answer; one that does not parse is empty.
    #[must_use]
    pub fn parse(value: &serde_json::Value) -> Self {
        serde_json::from_value(value.clone()).unwrap_or_default()
    }

    /// Whether the snapshot carries no servers at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.mcp.servers.is_empty() && self.providers.custom.is_empty()
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Providers {
    custom: BTreeMap<String, CustomProvider>,
    #[serde(flatten)]
    overrides: BTreeMap<String, ProviderOverride>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CustomProvider {
    kind: Option<String>,
    base_url: String,
    display_name: Option<String>,
    api_key_env: Option<String>,
    headers: BTreeMap<String, String>,
    models: Vec<CustomModel>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CustomModel {
    id: String,
    name: String,
    context: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ProviderOverride {
    base_url: Option<String>,
    api_key_env: Option<String>,
    headers: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Mcp {
    servers: BTreeMap<String, McpServer>,
    allow_sampling: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct McpServer {
    command: Option<String>,
    args: Vec<String>,
    env: BTreeMap<String, String>,
    url: Option<String>,
    headers: BTreeMap<String, String>,
    disabled: bool,
    oauth: Option<serde_json::Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Permissions {
    confine_paths: bool,
    tools: BTreeMap<String, ToolRules>,
    mcp: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ToolRules {
    default: String,
    allow: Vec<String>,
    deny: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Plugins {
    dir: Option<String>,
    enabled: Vec<String>,
    capabilities: BTreeMap<String, Vec<String>>,
    config: BTreeMap<String, serde_json::Value>,
}

/// One provider row: a configured one, or one the session's model
/// option names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRow {
    /// The provider id.
    pub id: String,
    /// The name the picker shows.
    pub label: String,
    /// The wire protocol, when the snapshot names it.
    pub protocol: Option<String>,
    /// The endpoint, when the snapshot names it.
    pub base_url: Option<String>,
    /// Where the key comes from, when the snapshot says.
    pub key_env: Option<String>,
    /// The header names it sends.
    pub headers: Vec<String>,
    /// The models it serves, as the snapshot or the session lists them.
    pub models: Vec<String>,
    /// Whether the config file defines it.
    pub custom: bool,
}

/// The provider rows: custom providers and overrides from the snapshot,
/// joined with every provider the session's model option offers.
#[must_use]
pub fn provider_rows(snapshot: &Snapshot, model: Option<&SessionConfigOption>) -> Vec<ProviderRow> {
    let mut rows: Vec<ProviderRow> = Vec::new();
    for (id, custom) in &snapshot.providers.custom {
        rows.push(ProviderRow {
            id: id.clone(),
            label: custom.display_name.clone().unwrap_or_else(|| id.clone()),
            protocol: Some(custom.kind.clone().unwrap_or_else(|| "openai".to_owned())),
            base_url: Some(custom.base_url.clone()),
            key_env: custom.api_key_env.clone(),
            headers: custom.headers.keys().cloned().collect(),
            models: custom
                .models
                .iter()
                .map(|m| match m.context {
                    Some(context) => format!("{} \u{b7} {}k", m.name, context / 1000),
                    None => m.name.clone(),
                })
                .collect(),
            custom: true,
        });
    }
    let offered = model
        .map(|option| option.options.as_slice())
        .unwrap_or_default();
    for choice in offered {
        let Some((provider, _)) = choice.value.split_once(':') else {
            continue;
        };
        let name = choice.name.clone();
        match rows.iter_mut().find(|row| row.id == provider) {
            Some(row) if row.custom => {}
            Some(row) => row.models.push(name),
            None => rows.push(ProviderRow {
                id: provider.to_owned(),
                label: provider.to_owned(),
                protocol: None,
                base_url: None,
                key_env: None,
                headers: Vec::new(),
                models: vec![name],
                custom: false,
            }),
        }
    }
    for (id, over) in &snapshot.providers.overrides {
        if id == "custom" {
            continue;
        }
        let row = match rows.iter_mut().find(|row| &row.id == id) {
            Some(row) => row,
            None => {
                rows.push(ProviderRow {
                    id: id.clone(),
                    label: id.clone(),
                    protocol: None,
                    base_url: None,
                    key_env: None,
                    headers: Vec::new(),
                    models: Vec::new(),
                    custom: false,
                });
                rows.last_mut().expect("just pushed")
            }
        };
        if over.base_url.is_some() {
            row.base_url.clone_from(&over.base_url);
        }
        if over.api_key_env.is_some() {
            row.key_env.clone_from(&over.api_key_env);
        }
        row.headers.extend(over.headers.keys().cloned());
    }
    rows
}

/// The MCP servers the snapshot carries, with the live status the
/// active session reports, as one row per server.
#[must_use]
pub fn mcp_rows(snapshot: &Snapshot, live: &BTreeMap<String, McpServerStatus>) -> Vec<McpRow> {
    let mut rows: Vec<McpRow> = Vec::new();
    for (name, server) in &snapshot.mcp.servers {
        let transport = if server.url.is_some() {
            "http"
        } else {
            "stdio"
        };
        let status = match (server.disabled, live.get(name)) {
            (true, _) => McpStatus::Disabled,
            (false, Some(McpServerStatus::Connected)) => McpStatus::Connected,
            (false, Some(McpServerStatus::Failed { .. })) => McpStatus::Failed,
            (false, Some(McpServerStatus::NeedsAuth)) => McpStatus::NeedsAuth,
            (false, Some(McpServerStatus::Starting)) => McpStatus::Starting,
            (false, None) => McpStatus::Unknown,
        };
        let target = match (&server.url, &server.command) {
            (Some(url), _) => url.clone(),
            (None, Some(command)) => std::iter::once(command.clone())
                .chain(server.args.iter().cloned())
                .collect::<Vec<_>>()
                .join(" "),
            (None, None) => "no command or url".to_owned(),
        };
        let mut extras: Vec<String> = Vec::new();
        if !server.env.is_empty() {
            extras.push(format!(
                "env: {}",
                server.env.keys().cloned().collect::<Vec<_>>().join(", ")
            ));
        }
        if !server.headers.is_empty() {
            extras.push(format!(
                "headers: {}",
                server
                    .headers
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if server.oauth.is_some() {
            extras.push("OAuth".to_owned());
        }
        if let Some(McpServerStatus::Failed { error }) = live.get(name) {
            extras.push(error.clone());
        }
        rows.push(McpRow {
            name: name.clone(),
            transport: transport.to_owned(),
            status,
            target,
            extras,
        });
    }
    rows
}

/// One MCP server row: the name, its transport, its live status and
/// what it connects to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpRow {
    /// The server's name in config.toml.
    pub name: String,
    /// The transport it speaks: `http` or `stdio`.
    pub transport: String,
    /// What the session last reported about it.
    pub status: McpStatus,
    /// The command line or URL it connects to.
    pub target: String,
    /// The names of its environment and headers, OAuth, and any error.
    pub extras: Vec<String>,
}

/// What the session last reported about an MCP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpStatus {
    /// The server is live.
    Connected,
    /// The server is starting.
    Starting,
    /// The server failed to start or its transport died.
    Failed,
    /// The server needs an OAuth login.
    NeedsAuth,
    /// The config disables the server.
    Disabled,
    /// No session reported a status.
    Unknown,
}

impl McpStatus {
    /// The word the row shows.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            McpStatus::Connected => "connected",
            McpStatus::Starting => "starting",
            McpStatus::Failed => "failed",
            McpStatus::NeedsAuth => "needs auth",
            McpStatus::Disabled => "disabled",
            McpStatus::Unknown => "status unknown",
        }
    }
}

/// The permission mode cards the session offers, with the plan mode
/// taken out, as the settings page draws them.
#[must_use]
pub fn mode_cards(modes: Option<&SessionConfigOption>, current: Option<&str>) -> Vec<ModeCard> {
    let Some(option) = modes else {
        return Vec::new();
    };
    option
        .options
        .iter()
        .filter(|choice| choice.value != crate::modes::PLAN_MODE)
        .map(|choice| ModeCard {
            value: choice.value.clone(),
            name: choice.name.clone(),
            description: choice.description.clone(),
            on: current == Some(choice.value.as_str()),
        })
        .collect()
}

/// One permission mode card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeCard {
    /// The value sent back to the engine.
    pub value: String,
    /// The card's title.
    pub name: String,
    /// The card's explanation, when the engine offers one.
    pub description: Option<String>,
    /// Whether this is the session's current mode.
    pub on: bool,
}

/// The tool rules the snapshot carries, as one row per tool and one per
/// MCP server rule.
#[must_use]
pub fn rule_rows(snapshot: &Snapshot) -> Vec<RuleRow> {
    let mut rows: Vec<RuleRow> = Vec::new();
    for (tool, rule) in &snapshot.permissions.tools {
        rows.push(RuleRow {
            tool: tool.clone(),
            action: rule.default.clone(),
            allow: rule.allow.clone(),
            deny: rule.deny.clone(),
        });
    }
    for (server, action) in &snapshot.permissions.mcp {
        let label = if server == "*" {
            "every MCP server".to_owned()
        } else {
            format!("mcp: {server}")
        };
        rows.push(RuleRow {
            tool: label,
            action: action.clone(),
            allow: Vec::new(),
            deny: Vec::new(),
        });
    }
    rows
}

/// One tool rule row: the tool, its default action and its globs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleRow {
    /// The tool the rule names.
    pub tool: String,
    /// What no glob matches does: `allow`, `ask` or `deny`.
    pub action: String,
    /// The globs that always pass.
    pub allow: Vec<String>,
    /// The globs that always refuse.
    pub deny: Vec<String>,
}

/// The plugins the snapshot configures, as one row per plugin.
#[must_use]
pub fn plugin_rows(snapshot: &Snapshot) -> Vec<PluginRow> {
    let plugins = &snapshot.plugins;
    let mut names: Vec<&String> = plugins
        .capabilities
        .keys()
        .chain(plugins.config.keys())
        .collect();
    names.sort();
    names.dedup();
    names
        .into_iter()
        .map(|name| {
            let caps = plugins.capabilities.get(name).cloned().unwrap_or_default();
            PluginRow {
                name: name.clone(),
                caps,
                has_settings: plugins.config.contains_key(name),
            }
        })
        .collect()
}

/// One configured plugin: its name, its grants and whether it has
/// settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRow {
    /// The plugin's name.
    pub name: String,
    /// The capabilities config.toml grants it.
    pub caps: Vec<String>,
    /// Whether the snapshot carries settings for it.
    pub has_settings: bool,
}

/// The Model Providers page.
pub fn providers_page(
    ui: &mut egui::Ui,
    snapshot: &Snapshot,
    model: Option<&SessionConfigOption>,
    palette: &Palette,
) {
    let rows = provider_rows(snapshot, model);
    if rows.is_empty() {
        note(
            ui,
            "No providers configured and no session open to list its models.",
            palette,
        );
    } else {
        group(ui, &format!("Providers \u{b7} {}", rows.len()), palette);
        boxed(ui, palette, |ui| {
            for row in &rows {
                let mut name = row.label.clone();
                if let Some(protocol) = &row.protocol {
                    name = format!("{name}  {protocol}");
                }
                if row.custom {
                    name = format!("{name}  config.toml");
                }
                let mut parts: Vec<String> = Vec::new();
                if let Some(url) = &row.base_url {
                    parts.push(url.clone());
                }
                parts.push(match row.models.len() {
                    0 => "no models listed".to_owned(),
                    1 => "1 model".to_owned(),
                    n => format!("{n} models"),
                });
                if !row.headers.is_empty() {
                    parts.push(format!("headers: {}", row.headers.join(", ")));
                }
                let key = match &row.key_env {
                    Some(env) if env.is_empty() => "no key needed".to_owned(),
                    Some(env) => format!("key from {env}"),
                    None => "key source unknown".to_owned(),
                };
                list_row(
                    ui,
                    &initials(&row.id),
                    &name,
                    &parts.join(" \u{b7} "),
                    Some(&key),
                    palette,
                );
            }
        });
    }
    note(
        ui,
        "Custom providers and overrides come from config.toml; the others are the ones the session's model picker offers. Whether a key is present stays with the engine. Adding and editing providers waits for _kage/config/set.",
        palette,
    );
}

/// The MCP Servers page: configured servers with the live status the
/// active session reports.
pub fn mcp_page(
    ui: &mut egui::Ui,
    snapshot: &Snapshot,
    live: &BTreeMap<String, McpServerStatus>,
    palette: &Palette,
) {
    if snapshot.mcp.servers.is_empty() {
        note(
            ui,
            "No MCP servers configured. Add them under [mcp.servers] in config.toml.",
            palette,
        );
        return;
    }
    let rows = mcp_rows(snapshot, live);
    boxed(ui, palette, |ui| {
        for row in &rows {
            let name = format!("{}  {}  {}", row.name, row.transport, row.status.word());
            let extras = row.extras.join(" \u{b7} ");
            list_row(ui, "", &name, &row.target, Some(&extras), palette);
        }
    });
    note(
        ui,
        if snapshot.mcp.allow_sampling {
            "Servers start with the session. Sampling is allowed: a server may run completions on your default model."
        } else {
            "Servers start with the session. Header and environment values stay with the engine; only their names show here."
        },
        palette,
    );
}

/// The Permissions page: the session's mode cards and the configured
/// rules.
pub fn permissions_page(
    ui: &mut egui::Ui,
    snapshot: &Snapshot,
    modes: Option<&SessionConfigOption>,
    current: Option<&str>,
    palette: &Palette,
    events: &mut Vec<SettingsEvent>,
) {
    group(ui, "This session", palette);
    let cards = mode_cards(modes, current);
    if cards.is_empty() {
        note(
            ui,
            "Open a session to see and change its permission mode.",
            palette,
        );
    } else {
        ui.horizontal_wrapped(|ui| {
            ui.add_space(32.0);
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.spacing_mut().item_spacing.y = 10.0;
            for card in &cards {
                mode_card(ui, card, palette, events);
            }
        });
    }
    group(ui, "Tool rules", palette);
    let rules = rule_rows(snapshot);
    if rules.is_empty() {
        note(
            ui,
            "No rules configured: every tool follows the session's mode.",
            palette,
        );
    } else {
        boxed(ui, palette, |ui| {
            for rule in &rules {
                let mut parts: Vec<String> = Vec::new();
                parts.push(format!("default {}", rule.action));
                if !rule.allow.is_empty() {
                    parts.push(format!("allow {}", rule.allow.join(", ")));
                }
                if !rule.deny.is_empty() {
                    parts.push(format!("deny {}", rule.deny.join(", ")));
                }
                list_row(ui, "", &rule.tool, &parts.join(" \u{b7} "), None, palette);
            }
        });
    }
    note(
        ui,
        if snapshot.permissions.confine_paths {
            "File tools are confined to the working directory. Rules live in config.toml under [permissions.tools.<tool>]; deny beats allow, and the mode decides what no rule matches."
        } else {
            "Rules live in config.toml under [permissions.tools.<tool>]; deny beats allow, and the mode decides what no rule matches."
        },
        palette,
    );
}

/// One permission mode card, chosen on a click. The whole card
/// answers, as the design's control did.
fn mode_card(
    ui: &mut egui::Ui,
    card: &ModeCard,
    palette: &Palette,
    events: &mut Vec<SettingsEvent>,
) {
    let width = 200.0_f32.min(ui.available_width());
    let response = ui.add(
        egui::Button::new("")
            .fill(palette.surface)
            .stroke(egui::Stroke::new(
                1.0,
                if card.on {
                    palette.accent
                } else {
                    palette.line
                },
            ))
            .corner_radius(egui::CornerRadius::same(R_LG as u8))
            .min_size(egui::vec2(width, 84.0)),
    );
    let rect = response.rect;
    if response.hovered() && !card.on {
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(R_LG as u8),
            egui::Stroke::new(1.0, palette.line_strong),
            egui::StrokeKind::Inside,
        );
    }
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&card.name)
                        .size(FS_SM)
                        .color(palette.ink_strong),
                )
                .selectable(false),
            );
            if card.on {
                crate::icons::icon(ui, crate::icons::Icon::Check, 14.0, palette.accent);
            }
        });
        if let Some(description) = &card.description {
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.add_space(12.0);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(description)
                            .size(FS_XS)
                            .color(palette.muted),
                    )
                    .selectable(false),
                );
            });
        }
    });
    if response.clicked() {
        events.push(SettingsEvent::Mode(card.value.clone()));
    }
}

/// The Plugins page: the directory, the allowlist and the grants.
pub fn plugins_page(ui: &mut egui::Ui, snapshot: &Snapshot, palette: &Palette) {
    let dir = snapshot
        .plugins
        .dir
        .clone()
        .unwrap_or_else(|| "~/.config/kage/plugins".to_owned());
    boxed(ui, palette, |ui| {
        list_row(ui, "", "Plugin directory", &dir, None, palette);
        let loaded = if snapshot.plugins.enabled.is_empty() {
            "Every plugin in the directory loads".to_owned()
        } else {
            format!("Only these load: {}", snapshot.plugins.enabled.join(", "))
        };
        list_row(ui, "", "Loaded plugins", &loaded, None, palette);
    });
    let rows = plugin_rows(snapshot);
    if !rows.is_empty() {
        group(ui, "Configured plugins", palette);
        boxed(ui, palette, |ui| {
            for row in &rows {
                let name = format!("{}  {}", row.name, row.caps.join(" "));
                let detail = match (row.caps.is_empty(), row.has_settings) {
                    (true, true) => "has settings",
                    (false, true) => "capabilities granted in config.toml \u{b7} has settings",
                    (false, false) => "capabilities granted in config.toml",
                    (true, false) => "",
                };
                list_row(ui, "", &name, detail, None, palette);
            }
        });
    }
    note(
        ui,
        "Lua plugins load from the plugin directory. Blocks and widgets a plugin draws carry its name as an owner badge. Enabling and installing wait for _kage/config/set.",
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
                .family(family(crate::theme::FAMILY_BODY_SEMIBOLD))
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

/// The rounded box rows sit in.
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

/// One list row: its leading initials, a name line, a detail line and
/// anything trailing.
fn list_row(
    ui: &mut egui::Ui,
    lead: &str,
    name: &str,
    detail: &str,
    trailing: Option<&str>,
    palette: &Palette,
) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().max(1.0), 44.0),
        egui::Sense::hover(),
    );
    let painter = ui.painter();
    painter.line_segment(
        [
            egui::pos2(rect.left(), rect.top()),
            egui::pos2(rect.right(), rect.top()),
        ],
        egui::Stroke::new(1.0, palette.subtle),
    );
    let mut text = name.to_owned();
    if let Some(trailing) = trailing.filter(|trailing| !trailing.is_empty()) {
        text = format!("{text}  {trailing}");
    }
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            ui.spacing_mut().item_spacing.x = 12.0;
            if !lead.is_empty() {
                let (badge, _) =
                    ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(badge, egui::CornerRadius::same(8), palette.fill);
                ui.scope_builder(egui::UiBuilder::new().max_rect(badge), |ui| {
                    ui.centered_and_justified(|ui| {
                        ui.label(egui::RichText::new(lead).size(10.5).color(palette.muted));
                    });
                });
            }
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(text).size(FS_SM).color(palette.ink));
                if !detail.is_empty() {
                    ui.label(
                        egui::RichText::new(detail)
                            .size(FS_XS)
                            .family(family(FAMILY_MONO_REGULAR))
                            .color(palette.muted),
                    );
                }
            });
        });
    });
}

/// The two letters a provider's initials mark carries.
fn initials(id: &str) -> String {
    id.chars()
        .filter(char::is_ascii_alphabetic)
        .take(2)
        .collect::<String>()
        .to_uppercase()
}

/// One outlined badge drawn at the cursor.
#[allow(dead_code)]
fn badge(ui: &mut egui::Ui, text: &str, palette: &Palette) {
    egui::Frame::NONE
        .fill(palette.fill)
        .stroke(egui::Stroke::new(1.0, palette.line))
        .corner_radius(egui::CornerRadius::same(R_FULL as u8))
        .inner_margin(egui::Margin::symmetric(7, 1))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(10.5).color(palette.muted));
        });
}

#[cfg(test)]
mod tests {
    use super::{McpStatus, Snapshot, mcp_rows, mode_cards, plugin_rows, provider_rows, rule_rows};

    #[test]
    fn providers_join_the_snapshot_with_the_offered_models() {
        let snapshot = Snapshot::parse(&serde_json::json!({
            "providers": {
                "custom": {"lab": {"base_url": "http://lab:8080", "api_key_env": "LAB_KEY",
                    "models": [{"id": "m1", "name": "Lab One", "context": 128000}]}},
                "deepseek": {"api_key_env": "DS", "headers": {"X-Org": "x"}}
            }
        }));
        let model: kage_client::wire::SessionConfigOption =
            serde_json::from_value(serde_json::json!({
                "id": "model", "name": "Model", "type": "select", "currentValue": "anthropic:a",
                "options": [
                    {"value": "anthropic:a", "name": "Claude A"},
                    {"value": "anthropic:b", "name": "Claude B"},
                    {"value": "lab:m1", "name": "Lab One"}
                ]
            }))
            .unwrap();
        let rows = provider_rows(&snapshot, Some(&model));
        let ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, ["lab", "anthropic", "deepseek"]);
        assert_eq!(
            rows[0].models,
            ["Lab One \u{b7} 128k"],
            "the custom list wins"
        );
        assert_eq!(rows[1].models.len(), 2);
        assert_eq!(rows[1].key_env, None, "unknown, never guessed");
        assert_eq!(rows[2].key_env.as_deref(), Some("DS"));
        assert_eq!(rows[2].headers, ["X-Org"]);
    }

    #[test]
    fn a_snapshot_that_does_not_parse_reads_empty() {
        let snapshot = Snapshot::parse(&serde_json::json!({"providers": 3}));
        assert!(snapshot.mcp.servers.is_empty());
        assert!(provider_rows(&snapshot, None).is_empty());
    }

    #[test]
    fn mcp_rows_carry_the_live_status_and_the_transport() {
        let snapshot = Snapshot::parse(&serde_json::json!({
            "mcp": {"servers": {
                "lab": {"command": "lab-mcp", "args": ["--port", "9"], "env": {"K": "v"}},
                "web": {"url": "https://mcp.example.com", "headers": {"A": "b"}},
                "off": {"command": "x", "disabled": true}
            }}
        }));
        let mut live = std::collections::BTreeMap::new();
        live.insert(
            "lab".to_owned(),
            kage_client::wire::McpServerStatus::Connected,
        );
        live.insert(
            "web".to_owned(),
            kage_client::wire::McpServerStatus::Failed {
                error: "handshake refused".to_owned(),
            },
        );
        let rows = mcp_rows(&snapshot, &live);
        let lab = rows.iter().find(|row| row.name == "lab").expect("lab");
        assert_eq!(lab.transport, "stdio");
        assert_eq!(lab.status, McpStatus::Connected);
        assert_eq!(lab.target, "lab-mcp --port 9");
        assert_eq!(lab.extras, ["env: K"]);
        let web = rows.iter().find(|row| row.name == "web").expect("web");
        assert_eq!(web.transport, "http");
        assert_eq!(web.status, McpStatus::Failed);
        assert_eq!(web.target, "https://mcp.example.com");
        let off = rows.iter().find(|row| row.name == "off").expect("off");
        assert_eq!(off.status, McpStatus::Disabled);
        assert_eq!(off.status.word(), "disabled");
    }

    #[test]
    fn an_unreported_server_reads_status_unknown() {
        let snapshot = Snapshot::parse(&serde_json::json!({
            "mcp": {"servers": {"lab": {"command": "lab-mcp"}}}
        }));
        let rows = mcp_rows(&snapshot, &std::collections::BTreeMap::new());
        assert_eq!(rows[0].status, McpStatus::Unknown);
        assert_eq!(rows[0].status.word(), "status unknown");
    }

    #[test]
    fn mode_cards_drop_the_plan_mode_and_mark_the_current_one() {
        let modes: kage_client::wire::SessionConfigOption =
            serde_json::from_value(serde_json::json!({
                "id": "mode", "name": "Mode", "type": "select", "currentValue": "ask",
                "options": [
                    {"value": "plan", "name": "Plan"},
                    {"value": "ask", "name": "Ask"},
                    {"value": "allow", "name": "Allow", "description": "Let it run"}
                ]
            }))
            .unwrap();
        let cards = mode_cards(Some(&modes), Some("allow"));
        let values: Vec<&str> = cards.iter().map(|card| card.value.as_str()).collect();
        assert_eq!(values, ["ask", "allow"]);
        assert!(!cards[0].on);
        assert!(cards[1].on);
        assert_eq!(cards[1].description.as_deref(), Some("Let it run"));
        assert!(mode_cards(None, None).is_empty());
    }

    #[test]
    fn rule_rows_list_tools_then_mcp_servers() {
        let snapshot = Snapshot::parse(&serde_json::json!({
            "permissions": {
                "tools": {"shell": {"default": "ask", "allow": ["cargo *"], "deny": ["rm *"]}},
                "mcp": {"*": "ask", "lab": "deny"}
            }
        }));
        let rows = rule_rows(&snapshot);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].tool, "shell");
        assert_eq!(rows[0].action, "ask");
        assert_eq!(rows[0].allow, ["cargo *"]);
        assert_eq!(rows[0].deny, ["rm *"]);
        assert_eq!(rows[1].tool, "every MCP server");
        assert_eq!(rows[2].tool, "mcp: lab");
        assert_eq!(rows[2].action, "deny");
    }

    #[test]
    fn plugin_rows_merge_the_grants_with_the_settings() {
        let snapshot = Snapshot::parse(&serde_json::json!({
            "plugins": {
                "enabled": ["lab"],
                "capabilities": {"lab": ["net"], "web": ["fs"]},
                "config": {"lab": {"key": 1}}
            }
        }));
        let rows = plugin_rows(&snapshot);
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["lab", "web"]);
        assert_eq!(rows[0].caps, ["net"]);
        assert!(rows[0].has_settings);
        assert!(!rows[1].has_settings);
    }
}
