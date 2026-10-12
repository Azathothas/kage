//! The desktop launch: flags, transport choice and the native window.
//!
//! One window over one transport: `kage rpc` as a child by default,
//! `kage serve` over WebSocket with `--ws` and `--token`, or the
//! recorded golden transcript with `--replay`. `--smoke` quits after
//! a delay, which is how the headless runs are automated (the smoke
//! report grows with the shell, E02). The browser build replaces this
//! module with the web entry, which returns with E08.

use std::time::Duration;

use kage_client::Frame;
#[cfg(not(target_arch = "wasm32"))]
use kage_client::unquote_and_trim;

use crate::store::Command;
#[cfg(not(target_arch = "wasm32"))]
use crate::transport::stdio::{Config as StdioConfig, StdioTransport};
#[cfg(not(target_arch = "wasm32"))]
use crate::transport::ws::WsTransport;
use crate::transport::{Event, Link, Transport};

/// Which transport the shell connects through. Native only: the
/// browser's endpoint comes from `boot.js`, not a flag.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum Wire {
    /// Spawn `kage rpc`, with an optional explicit binary.
    #[default]
    Stdio,
    /// Dial `kage serve` over WebSocket.
    WebSocket {
        /// The `ws://` endpoint, path included.
        url: String,
        /// The bearer token of the endpoint.
        token: String,
    },
    /// Play the recorded golden transcript.
    Replay,
}

/// Command line switches for automated runs and transport choice.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Default)]
struct Launch {
    wire: Wire,
    /// Quit this many milliseconds after launch, for smoke runs.
    smoke_millis: Option<u64>,
    /// Run the 30 updates per second stream on open.
    stream: bool,
    /// Why the switches do not fit together, if they do not.
    error: Option<String>,
    /// Raw flags, resolved by [`Launch::finalize`].
    ws_url: Option<String>,
    token: Option<String>,
    rpc_bin: Option<String>,
    replay: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl Launch {
    /// Records one problem, keeping the earlier ones, so the exit
    /// message can list every flag that did not fit.
    fn refuse(&mut self, problem: String) {
        match &mut self.error {
            Some(error) => {
                error.push_str("; ");
                error.push_str(&problem);
            }
            None => self.error = Some(problem),
        }
    }

    fn parse(args: impl Iterator<Item = String>) -> Self {
        let mut launch = Self::default();
        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--smoke" => {
                    let millis = args.peek().and_then(|value| value.parse::<u64>().ok());
                    if millis.is_some() {
                        args.next();
                    }
                    launch.smoke_millis = Some(millis.unwrap_or(0));
                }
                "--stream" => launch.stream = true,
                "--replay" => launch.replay = true,
                "--ws" => match args.next() {
                    Some(url) => launch.ws_url = Some(url),
                    None => launch.error = Some("--ws needs a ws:// URL".to_owned()),
                },
                "--token" => match args.next() {
                    Some(token) => launch.token = Some(token),
                    None => launch.error = Some("--token needs a value".to_owned()),
                },
                "--rpc-bin" => match args.next().as_deref().map(unquote_and_trim) {
                    Some("") => {}
                    Some(program) => launch.rpc_bin = Some(program.to_owned()),
                    None => launch.error = Some("--rpc-bin needs a path".to_owned()),
                },
                other => {
                    let known = other.split_once('=').is_some_and(|(name, _)| {
                        matches!(name, "--ws" | "--token" | "--rpc-bin" | "--smoke")
                    });
                    launch.refuse(if known {
                        format!("{other} is not a flag form here; pass the value after a space")
                    } else {
                        format!("unknown flag: {other}")
                    });
                }
            }
        }
        launch.finalize()
    }

    /// Resolves the raw flags into the wire to use.
    fn finalize(mut self) -> Self {
        if self.error.is_some() {
            return self;
        }
        self.wire = if let Some(url) = self.ws_url.take() {
            match self.token.take() {
                Some(token) => Wire::WebSocket { url, token },
                None => {
                    self.error =
                        Some("--ws needs --token; the token never rides the URL".to_owned());
                    return self;
                }
            }
        } else if self.token.take().is_some() {
            self.error = Some("--token needs --ws".to_owned());
            return self;
        } else if self.replay {
            Wire::Replay
        } else {
            Wire::Stdio
        };
        self
    }

    /// The transport the launch selected. Stdio runs `--rpc-bin`, else
    /// the binary the setup screen saved, else `kage` from the PATH.
    /// The browser has no local engine, so this never runs there.
    #[cfg(not(target_arch = "wasm32"))]
    fn transport(&self, saved: Option<&str>) -> Box<dyn Transport> {
        match &self.wire {
            Wire::Stdio => {
                let config = match self.rpc_bin.as_deref().or(saved) {
                    Some(program) => StdioConfig {
                        program: program.to_owned(),
                        args: vec!["rpc".to_owned()],
                    },
                    None => StdioConfig::engine(),
                };
                Box::new(StdioTransport::new(config))
            }
            Wire::WebSocket { url, token } => {
                Box::new(WsTransport::new(url.clone(), token.clone()))
            }
            Wire::Replay => Box::new(crate::transport::replay::ReplayTransport::new()),
        }
    }
}

/// The shell: the store on the UI thread and the transport behind it.
///
/// One pump thread reads the transport's events and hands them to the
/// UI thread through a `std` channel, asking egui for a repaint on
/// each. The UI thread drains the store on every frame, executes the
/// Where the transport's events land before the frame drains them.
///
/// Native transports speak from a background thread, so a pump thread
/// moves their events into a thread-safe inbox and asks egui for a
/// repaint. The browser transport calls back on the UI thread itself,
/// so there is nothing to move and no thread to start: the frame
/// drains the channel directly.
enum Inbox {
    /// The native pump's inbox.
    #[cfg(not(target_arch = "wasm32"))]
    Threaded(std::sync::mpsc::Receiver<Event>),
    /// The browser transport's own channel.
    #[cfg(target_arch = "wasm32")]
    Direct(async_channel::Receiver<Event>),
}

impl Inbox {
    /// Starts a native pump thread over `incoming` and asks `ctx` for
    /// a repaint on every event.
    ///
    /// The browser has no thread to start, so its variant below is
    /// the only one that compiles there.
    #[cfg(not(target_arch = "wasm32"))]
    fn pumped(
        incoming: async_channel::Receiver<Event>,
        ctx: &egui::Context,
    ) -> (Self, std::thread::JoinHandle<()>) {
        let (tx, rx) = std::sync::mpsc::channel::<Event>();
        let ctx = ctx.clone();
        let pump = std::thread::Builder::new()
            .name("kage-pump".to_owned())
            .spawn(move || {
                while let Ok(event) = incoming.recv_blocking() {
                    if tx.send(event).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                }
            })
            .expect("the pump thread starts");
        (Self::Threaded(rx), pump)
    }

    /// The browser's inbox: the transport's own channel.
    #[cfg(target_arch = "wasm32")]
    fn direct(incoming: async_channel::Receiver<Event>) -> Self {
        Self::Direct(incoming)
    }

    /// Takes one event, or `None` when the channel is empty for now.
    fn try_next(&self) -> Option<Event> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Self::Threaded(rx) => rx.try_recv().ok(),
            #[cfg(target_arch = "wasm32")]
            Self::Direct(rx) => rx.try_recv().ok(),
        }
    }
}

/// commands the store queued, and pushes the outgoing frames back
/// through the transport. The store never crosses the thread
/// boundary, matching the old single-threaded entity model.
pub struct Shell {
    /// The store, touched only on the UI thread.
    store: crate::store::Store,
    /// The transport behind the store.
    transport: Box<dyn Transport>,
    /// Where the transport's events land. A native transport hands
    /// them over on the pump thread and this is the thread-safe
    /// inbox; a browser transport calls back on the UI thread and
    /// this is the channel itself.
    inbox: Inbox,
    /// Set when `--smoke` asked for a timed quit.
    smoke_deadline: Option<(web_time::Instant, Duration)>,
    /// Whether the engine has been asked for its themes and config.
    themes_asked: bool,
    /// Synthetic chunks appended, for the 30 Hz stream measurement.
    streamed: usize,
    /// The keepalive handle of the pump thread, dropped with the
    /// shell. The browser has no pump thread: its transport calls back
    /// on the UI thread, so this stays `None` there.
    _pump: Option<std::thread::JoinHandle<()>>,
    /// The three-panel layout state.
    panels: crate::ui::shell::Panels,
    /// The gate banner's dismissal.
    gate_banner: crate::ui::status::Banner,
    /// The toast queue.
    toasts: crate::ui::toasts::Toasts,
    /// The command palette.
    palette: crate::ui::palette::Palette,
    /// The boot splash.
    splash: crate::ui::splash::Splash,
    /// When the eclipse started orbiting, for the working indicator.
    orbit: Option<web_time::Instant>,
    /// The transcript in the chat column.
    transcript: crate::ui::transcript::view::TranscriptView,
    /// The find bar over the chat column.
    find: crate::ui::find::FindBar,
    /// The composer under the chat column: the field, its toolbar and
    /// its suggestion menus.
    composer: crate::ui::composer::Composer,
    /// The sidebar's search query and collapsed projects.
    sidebar: crate::ui::sidebar::Sidebar,
    /// The right panel's tabs, filters and selections.
    workbench: crate::ui::workbench::Workbench,
    /// The modal dialog layer over the shell.
    dialogs: crate::ui::dialog::Dialogs,
    /// The approval card over the open permission asks.
    approval: crate::ui::approval::Approval,
    /// The settings dialog over the shell.
    settings: crate::ui::settings::SettingsView,
    /// The setup screen, shown in place of the shell while no engine
    /// could start. The browser has no local engine to point at, so
    /// the screen never shows there and the field stays unread.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    setup: crate::ui::setup::SetupView,
    /// Whether the shell replays a recording: the replay always
    /// connects, so it never shows setup.
    replay: bool,
}

impl Shell {
    /// Builds the shell: the store opens sessions in `cwd`, `replay`
    /// selects the scripted prompt of the recording, and `transport`
    /// starts on the pump thread right away. `ctx` is cloned into the
    /// pump so each transport event asks for a repaint.
    #[must_use]
    pub fn new(
        cwd: impl Into<String>,
        replay: bool,
        mut transport: Box<dyn Transport>,
        ctx: &egui::Context,
    ) -> Self {
        let (events, incoming) = async_channel::unbounded();
        transport.start(events.clone());
        let (inbox, pump) = Self::start_inbox(incoming, ctx);
        Self {
            store: crate::store::Store::new(cwd, replay),
            transport,
            inbox,
            smoke_deadline: None,
            streamed: 0,
            themes_asked: false,
            _pump: pump,
            panels: crate::ui::shell::Panels::default(),
            gate_banner: crate::ui::status::Banner::default(),
            toasts: crate::ui::toasts::Toasts::default(),
            palette: crate::ui::palette::Palette::default(),
            splash: crate::ui::splash::Splash::Showing,
            orbit: None,
            transcript: crate::ui::transcript::view::TranscriptView::new(),
            find: crate::ui::find::FindBar::new(),
            composer: crate::ui::composer::Composer::new(),
            sidebar: crate::ui::sidebar::Sidebar::new(),
            workbench: crate::ui::workbench::Workbench::new(),
            dialogs: crate::ui::dialog::Dialogs::new(),
            approval: crate::ui::approval::Approval::new(),
            settings: crate::ui::settings::SettingsView::new(),
            setup: crate::ui::setup::SetupView::new(),
            replay,
        }
    }

    /// Whether the setup screen shows instead of the shell: the link
    /// was refused and nothing else carries it. A replay never lands
    /// here: it connects on its own.
    #[must_use]
    pub fn needs_setup(&self) -> bool {
        !self.replay && matches!(self.store.connect(), crate::transport::State::Refused(_))
    }

    /// Starts `program` as the engine and lets it connect again.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn use_engine(&mut self, program: String, ctx: &egui::Context) {
        let config = StdioConfig {
            program,
            args: vec!["rpc".to_owned()],
        };
        self.replace_transport(Box::new(StdioTransport::new(config)), ctx);
    }

    /// Looks for `kage` again: the saved path if the setup screen once
    /// named one, else the PATH.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn retry_engine(&mut self, saved: Option<&str>, ctx: &egui::Context) {
        let config = match saved {
            Some(program) => StdioConfig {
                program: program.to_owned(),
                args: vec!["rpc".to_owned()],
            },
            None => StdioConfig::engine(),
        };
        self.replace_transport(
            Box::new(crate::transport::stdio::StdioTransport::new(config)),
            ctx,
        );
    }

    /// Swaps in `transport` and restarts the pump thread behind it.
    #[cfg(not(target_arch = "wasm32"))]
    fn replace_transport(&mut self, mut transport: Box<dyn Transport>, ctx: &egui::Context) {
        self._pump = None;
        let (events, incoming) = async_channel::unbounded();
        transport.start(events.clone());
        let (inbox, pump) = Self::start_inbox(incoming, ctx);
        self.transport = transport;
        self.inbox = inbox;
        self._pump = pump;
        self.store.set_connect(crate::transport::State::Connecting);
    }

    /// The inbox a fresh transport reports into, and the pump handle
    /// to keep on native. The browser has no pump thread, so its
    /// handle is `None`.
    fn start_inbox(
        incoming: async_channel::Receiver<Event>,
        ctx: &egui::Context,
    ) -> (Inbox, Option<std::thread::JoinHandle<()>>) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (inbox, pump) = Inbox::pumped(incoming, ctx);
            (inbox, Some(pump))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = ctx;
            (Inbox::direct(incoming), None)
        }
    }

    /// The connection's link, for the status bar.
    #[must_use]
    pub fn link(&self) -> Link {
        self.transport.link()
    }

    /// The store, for the views.
    pub const fn store(&self) -> &crate::store::Store {
        &self.store
    }

    /// Drains the pump's events into the store and carries out the
    /// commands it produced.
    fn on_events(&mut self) {
        let mut reconnecting = false;
        while let Some(event) = self.inbox.try_next() {
            match event {
                Event::Frame(frame) => {
                    self.store.absorb(frame);
                }
                Event::State(state) => {
                    if matches!(state, crate::transport::State::Reconnecting { .. }) {
                        reconnecting = true;
                    }
                    self.store.set_connect(state);
                }
            }
        }
        // A reconnect asks for the themes and the config again, and
        // brings a still-short gate report back.
        if reconnecting {
            self.gate_banner.reset();
            self.themes_asked = false;
        }
        let answered =
            self.store.state().agent.is_some() || self.store.state().capabilities.is_some();
        self.splash.follow(self.store.connect(), answered);
        let now = web_time::Instant::now();
        for note in self.store.take_notes() {
            self.toasts.push_note(&note, now);
        }
        let commands = self.store.take_commands();
        for command in commands {
            match command {
                Command::Handshake { replay_sessions } => {
                    self.store.handshake(replay_sessions);
                }
                Command::NewSession => {
                    self.store.new_session();
                }
                Command::ReplayPrompt => {
                    self.store.prompt(REPLAY_PROMPT);
                }
            }
        }
    }

    /// Drains the store's outgoing frames into the transport.
    /// Hands the store out for the frame's drawing and takes it
    /// back afterwards. The workbench's panes write to the store, so
    /// its closure needs it in a cell, and the cell outlives the
    /// closure only inside this frame.
    fn store_take(&mut self) -> crate::store::Store {
        std::mem::replace(&mut self.store, crate::store::Store::empty())
    }

    fn flush_outgoing(&mut self) {
        if !self.themes_asked && self.store.live() {
            self.themes_asked = true;
            self.store.ask_themes();
            self.store.ask_config();
        }
        for frame in self.store.take_outgoing() {
            self.transport.send(frame);
        }
    }

    /// The line automated runs print before quitting.
    #[must_use]
    pub fn smoke_line(&self) -> String {
        let state = self.store.state();
        let agent = state
            .agent
            .as_ref()
            .map(|agent| format!("{} {}", agent.name, agent.version.as_deref().unwrap_or("?")))
            .unwrap_or_else(|| "none".to_owned());
        let (items, used, size) = self
            .store
            .active_session()
            .map(|session| (session.items.len(), session.usage.used, session.usage.size))
            .unwrap_or((0, 0, 0));
        format!(
            "smoke: connect={} agent=({agent}) sessions={sessions} items={items} used={used}/{size} streamed={streamed} connection={connection}",
            self.store.connect().label(),
            sessions = state.sessions.len(),
            streamed = self.streamed,
            connection = self
                .transport
                .connection_id()
                .unwrap_or_else(|| "-".to_owned()),
        )
    }

    /// Appends one synthetic agent chunk, for the 30 updates per
    /// second measurement against the real client path.
    fn stream_chunk(&mut self) {
        let Some(id) = self.store.active_id().map(str::to_owned) else {
            return;
        };
        self.streamed += 1;
        let text = format!("stream chunk {}\n", self.streamed);
        let frame = Frame::Notification {
            method: "session/update".to_owned(),
            params: serde_json::json!({
                "sessionId": id,
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {"type": "text", "text": text},
                },
            }),
        };
        self.store.absorb(frame);
    }
}

/// The window's body: the shell plus the frame clock.
pub struct ShellApp {
    /// The shell under the window.
    pub shell: Shell,
    /// The 30 Hz stream runs while this is set. Native only: it is
    /// `--stream`, a measurement path the browser never takes.
    pub stream: bool,
    /// The theme the window opened with. The settings dialog reports
    /// a change with the choice it stored; the shell applies it.
    pub prefs_theme: crate::theme::ThemeChoice,
    /// The 30 Hz ticker the `--stream` run drives, for the NFR-01
    /// measurement.
    stream_tick: crate::frame_times::Ticker,
    /// When the daily release check last ran, unix seconds.
    last_update_check: i64,
    /// The finished release check's inbox, while one is in flight.
    update_rx: Option<std::sync::mpsc::Receiver<Option<crate::update::CheckOutcome>>>,
    /// The finished install's inbox, while one is in flight. The
    /// browser never installs anything.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    install_rx: Option<
        std::sync::mpsc::Receiver<(
            crate::ui::settings::Install,
            Result<std::path::PathBuf, String>,
        )>,
    >,
    /// Which install the in-flight download is for.
    installing: Option<crate::ui::settings::Install>,
    /// What the last install ended with, as the Updates row reads it.
    install_note: Option<(bool, String)>,
    /// The actions the transcript's rows raised this frame, for the
    /// shell to carry out.
    row_actions: Vec<crate::ui::transcript::tool_rows::RowAction>,
    /// The window's context, for the modal key contexts.
    ctx: egui::Context,
    /// When this window was built, the clock the dialogs' fields and
    /// the constellation twinkle on.
    born: web_time::Instant,
    /// The frame-time recorder, off unless `KAGE_FRAME_TIMES` is set.
    frame_times: crate::frame_times::FrameTimes,
}

impl eframe::App for ShellApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // The frame clock starts here and stops around the draw. The
        // time between frames is idle time: egui schedules no repaint
        // while the app waits, so charging that gap to a frame is how
        // an idle window reports one-second frames.
        self.frame_times.begin_frame(web_time::Instant::now());
        self.shell.on_events();
        self.shell.flush_outgoing();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.pump_update_check();
            self.apply_update();
            self.apply_install();
        }
        self.route_keys(&ctx);
        if self.stream {
            // The ticker advances its clock by one period per tick
            // rather than to the current time: resetting to `now`
            // pushed every tick out by a frame and the 30 Hz rate
            // drifted below its NFR-01 budget however fast the frames
            // were (284 of 300 at 64 fps).
            let now = web_time::Instant::now();
            if self.stream_tick.tick(now) {
                self.shell.stream_chunk();
            }
            ctx.request_repaint_after(self.stream_tick.due_in(now));
        }
        if let Some((started, delay)) = self.shell.smoke_deadline.take() {
            if started.elapsed() >= delay {
                println!("{}", self.shell.smoke_line());
                // The frame began above and drew nothing: it found the
                // deadline and asked the window to close. Closing it
                // here would measure the whole idle wait as a draw.
                self.frame_times.report();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
            self.shell.smoke_deadline = Some((started, delay));
            // The smoke window runs without input, so keep the frame
            // clock alive until the deadline.
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        self.shell.toasts.expire(web_time::Instant::now());
        #[cfg(not(target_arch = "wasm32"))]
        if self.shell.needs_setup() {
            let draw_started = web_time::Instant::now();
            self.draw_setup(ui);
            let now = web_time::Instant::now();
            self.frame_times
                .end_frame(now, now.saturating_duration_since(draw_started));
            return;
        }
        let draw_started = web_time::Instant::now();
        self.draw(ui);
        let now = web_time::Instant::now();
        self.frame_times
            .end_frame(now, now.saturating_duration_since(draw_started));
        self.find_keys();
    }

    /// Prints the frame-time summary when the window closes, so a run
    /// that quits from the title bar or a signal still reports. It
    /// records no frame: every draw path already closed its own, and
    /// the time between the last frame and here is teardown.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.frame_times.report();
    }
}

impl ShellApp {
    /// Builds the window body over a transport: the native flags and
    /// the browser handoff both land here.
    #[must_use]
    pub fn new(
        cwd: String,
        replay: bool,
        transport: Box<dyn Transport>,
        ctx: &egui::Context,
    ) -> Self {
        crate::theme::install_fonts(ctx);
        crate::icons::install(ctx);
        Self {
            shell: Shell::new(cwd, replay, transport, ctx),
            stream: false,
            prefs_theme: crate::theme::ThemeChoice::System,
            stream_tick: crate::frame_times::Ticker::hz30(web_time::Instant::now()),
            last_update_check: crate::clock::unix_seconds(),
            update_rx: None,
            install_rx: None,
            installing: None,
            install_note: None,
            row_actions: Vec::new(),
            ctx: ctx.clone(),
            born: web_time::Instant::now(),
            frame_times: crate::frame_times::FrameTimes::from_env(),
        }
    }

    /// Draws the setup screen in place of the shell, and carries what
    /// it asked for: a probed binary becomes the engine, a retry looks
    /// on the PATH again, and a chosen path is remembered so the next
    /// launch starts without the screen. The browser has no local
    /// engine to point at, so it never gets here.
    #[cfg(not(target_arch = "wasm32"))]
    fn draw_setup(&mut self, ui: &mut egui::Ui) {
        egui::Area::new(egui::Id::new("kage-setup"))
            .anchor(egui::Align2::LEFT_TOP, egui::Vec2::ZERO)
            .order(egui::Order::Foreground)
            .interactable(true)
            .movable(false)
            .show(ui.ctx(), |ui| {
                let palette = crate::theme::Palette::active_of(ui.ctx());
                ui.set_min_size(ui.ctx().viewport_rect().size());
                egui::Frame::NONE.fill(palette.bg).show(ui, |ui| {
                    ui.centered_and_justified(|ui| {
                        self.shell.setup.show(ui);
                    });
                });
            });
        let saved = self.shell.store.prefs().kage_path.clone();
        match self.shell.setup.take_asked() {
            Some(crate::ui::setup::SetupEvent::Use(program)) => {
                self.shell
                    .store
                    .update_prefs(|prefs| prefs.kage_path = Some(program.clone()));
                self.shell.use_engine(program, ui.ctx());
            }
            Some(crate::ui::setup::SetupEvent::Retry) => {
                self.shell.retry_engine(saved.as_deref(), ui.ctx());
            }
            None => {}
        }
    }

    /// Draws the frame: the gate banner over the three panels, the
    /// status bar under them, and the chrome (splash, palette,
    /// toasts) over the top.
    fn draw(&mut self, ui: &mut egui::Ui) {
        // The key context: a modal sits above the shell.
        let context = self.context();
        let gate = self.shell.store.gate().clone();
        let mut banner = self.shell.gate_banner.clone();
        let mut panels = self.shell.panels.clone();
        let mut palette = self.shell.palette.clone();
        let mut toasts = std::mem::take(&mut self.shell.toasts);
        let splash = self.shell.splash;
        let orbit = self.shell.orbit;

        banner.show(ui, &gate);

        // The panel draws the sidebar twice while narrow, so its state
        // rides in a cell the closure borrows once. The rows the
        // sidebar lists are read once, before the chat column takes
        // `self` for its mutable state, and handed to the closure as
        // data; the closure's own events are carried out afterwards.
        let sidebar = std::cell::RefCell::new(std::mem::take(&mut self.shell.sidebar));
        let sidebar_rows = crate::ui::sidebar::sidebar_rows(&self.shell.store);
        let sidebar_grouped = self.shell.store.prefs().group_by_project;
        let (link_name, link_status) = crate::ui::sidebar::connect_lines(&self.shell.store);
        let sidebar_palette = crate::theme::Palette::active_of(ui.ctx());
        let (dot, halo) = crate::ui::sidebar::connect_dot(sidebar_palette, &self.shell.store);
        let sidebar_link = (link_name, link_status, dot, halo);
        let sidebar_events: std::cell::RefCell<Vec<crate::ui::sidebar::SidebarEvent>> =
            std::cell::RefCell::new(Vec::new());
        // The workbench and the store ride in cells the way the
        // sidebar does: its closure must stay `Fn`, because the shell
        // draws the panel twice while narrow, and its events are
        // carried out once the panels are drawn.
        let workbench_state = std::cell::RefCell::new(std::mem::take(&mut self.shell.workbench));
        // The workbench's panes read and write the store, so it rides
        // in a cell the closure borrows once, the way the sidebar's
        // state does. The shell takes it back when the panels are
        // drawn.
        let store = std::cell::RefCell::new(self.shell.store_take());
        let workbench_events: std::cell::RefCell<Vec<crate::ui::workbench::WorkbenchEvent>> =
            std::cell::RefCell::new(Vec::new());
        let sidebar_draw = |ui: &mut egui::Ui| {
            let events = crate::ui::sidebar::show_rows(
                ui,
                &mut sidebar.borrow_mut(),
                &sidebar_rows,
                sidebar_grouped,
                (
                    sidebar_link.0.as_str(),
                    sidebar_link.1.as_str(),
                    sidebar_link.2,
                    sidebar_link.3,
                ),
            );
            sidebar_events.borrow_mut().extend(events);
        };
        let workbench = |ui: &mut egui::Ui| {
            let events = crate::ui::workbench::show(
                ui,
                &mut workbench_state.borrow_mut(),
                &mut store.borrow_mut(),
            );
            workbench_events.borrow_mut().extend(events);
        };
        let chat = |ui: &mut egui::Ui| {
            // The column is the transcript above and the composer
            // below: the transcript takes what is left, and the
            // composer and its dock keep their own height. With no
            // session the welcome column shows instead, and the
            // composer still mounts so a draft never resets.
            //
            // The store comes from the cell the panels share, not
            // from the shell's field: `store_take` has moved the real
            // store out for the draw, so a read through the field
            // would see the empty placeholder and draw the welcome
            // over a live transcript forever.
            let store = &mut store.borrow_mut();
            ui.vertical(|ui| {
                if store.active_id().is_some() {
                    // The transcript takes what is left of the column;
                    // the dock and the composer keep their own height
                    // under it, so the field sits on the column's
                    // bottom edge the way the old shell pinned it.
                    let composer_h = COMPOSER_H;
                    let dock_h = DOCK_H;
                    let top_h = (ui.available_height() - composer_h - dock_h).max(0.0);
                    let (top_rect, _) = ui.allocate_at_least(
                        egui::vec2(ui.available_width(), top_h),
                        egui::Sense::hover(),
                    );
                    let mut top = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(top_rect)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    let mut events = Vec::new();
                    self.shell.transcript.show(&mut top, store, &mut events);
                    self.row_actions.extend(events);
                } else {
                    // The welcome column: the brand mark, the eclipse
                    // while a turn runs, and the connection line.
                    let palette = crate::theme::Palette::active_of(ui.ctx());
                    let running = store
                        .active_session()
                        .is_some_and(|session| session.in_turn || session.running);
                    if running && self.shell.orbit.is_none() {
                        self.shell.orbit = Some(web_time::Instant::now());
                    } else if !running {
                        self.shell.orbit = None;
                    }
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() / 2.0 - 60.0);
                        crate::ui::eclipse::eclipse(ui, 44.0, self.shell.orbit);
                        ui.add_space(14.0);
                        ui.label(
                            egui::RichText::new("kage")
                                .size(40.0)
                                .color(palette.ink_strong)
                                .family(crate::theme::family(crate::theme::FAMILY_BODY_EXTRABOLD)),
                        );
                        ui.label(
                            egui::RichText::new(self.welcome_line(store))
                                .size(crate::theme::FS_SM)
                                .color(palette.muted),
                        );
                    });
                }
                self.show_dock(ui, store);
                self.show_approval(ui, store);
                self.show_composer(ui, store);
            });
        };
        crate::ui::shell::show(ui, &mut panels, sidebar_draw, chat, workbench);
        self.shell.sidebar = sidebar.into_inner();
        self.shell.store = store.into_inner();
        self.shell.workbench = workbench_state.into_inner();
        self.carry_sidebar(sidebar_events.into_inner());
        self.carry_workbench(workbench_events.into_inner());

        crate::ui::status::status_bar(ui, self.shell.store());

        let rows = self.shell.transcript.searchable_rows(self.shell.store());
        let find_action = self.shell.find.show(ui, &rows);
        if let Some(crate::ui::find::RowAction::Reveal(key)) = find_action {
            self.shell.transcript.scroll_to_row(key);
        }

        if self.shell.palette.is_open() {
            let registry = crate::ui::palette::RegistryOwned::of(self.shell.store());
            let rows = registry.view().rows(self.shell.palette.query());
            if let Some(entry) = palette.show(ui, &rows) {
                let _ = entry;
            }
        }
        // The dialog layer sits above every panel, over the palette
        // too: a dialog the palette opens must not answer beneath it.
        if let Some(id) = self.shell.dialogs.browsing()
            && let Some(read) = self.shell.store.folders(id).cloned()
        {
            self.shell.dialogs.answer_folders(id, read);
        }
        if self.shell.dialogs.is_open() {
            let events = self.shell.dialogs.show(ui, &self.shell.store, self.born);
            self.carry_dialog(events);
        }
        // The settings dialog sits above the dialog layer: a dialog it
        // opens from the palette must not answer beneath it.
        if self.shell.settings.is_open() {
            let updates = crate::ui::settings::UpdateState {
                installing: self.installing,
                checking: self.update_rx.is_some(),
                install_note: self.install_note.clone(),
            };
            let events = self
                .shell
                .settings
                .show(ui, &mut self.shell.store, &updates);
            self.carry_settings(events, ui.ctx());
        }
        toasts.show(ui);
        splash.show(ui, orbit);
        let _ = context;

        self.shell.gate_banner = banner;
        self.shell.panels = panels;
        self.shell.palette = palette;
        self.shell.toasts = toasts;
    }

    /// Carries out what the sidebar's controls asked for.
    fn carry_sidebar(&mut self, events: Vec<crate::ui::sidebar::SidebarEvent>) {
        for event in events {
            match event {
                crate::ui::sidebar::SidebarEvent::NewSession(dir) => {
                    if let Some(dir) = dir {
                        self.shell.store.set_project(Some(dir));
                    }
                    self.shell.store.show_welcome();
                }
                crate::ui::sidebar::SidebarEvent::Activate(id) => {
                    self.shell.store.set_active(id);
                }
                crate::ui::sidebar::SidebarEvent::OpenPalette => {
                    self.shell.palette.open();
                }
                crate::ui::sidebar::SidebarEvent::OpenSettings => {}
                crate::ui::sidebar::SidebarEvent::Collapse => {
                    self.shell.panels.toggle_sidebar();
                }
                crate::ui::sidebar::SidebarEvent::ToggleGrouping => {
                    self.shell.store.update_prefs(|prefs| {
                        prefs.group_by_project = !prefs.group_by_project;
                    });
                }
            }
        }
    }

    /// Carries out what the workbench's panes asked for.
    /// Carries out what the dialogs asked for.
    fn carry_dialog(&mut self, events: Vec<crate::ui::dialog::DialogEvent>) {
        for event in events {
            match event {
                crate::ui::dialog::DialogEvent::SaveGoal(text) => {
                    let _ = self.shell.store.set_option("goal", &text);
                }
                crate::ui::dialog::DialogEvent::EnableSwarm(permission) => {
                    if let Some(permission) = &permission {
                        let _ = self.shell.store.set_permission(permission);
                    }
                    let _ = self.shell.store.set_option("swarm", "on");
                }
                crate::ui::dialog::DialogEvent::Rewind(prompt) => {
                    let _ = self.shell.store.rewind(prompt);
                }
                crate::ui::dialog::DialogEvent::SetProject(path) => {
                    self.shell.store.set_project(Some(path));
                }
                crate::ui::dialog::DialogEvent::Browse(path) => {
                    let id = self.shell.store.ask_folders(path.as_deref());
                    self.shell.dialogs.browse(id);
                }
            }
        }
    }

    /// Carries out what the settings dialog asked for. A page the
    /// engine's configuration backs asks for a fresh snapshot, so an
    /// edit to config.toml shows on the next visit.
    fn carry_settings(
        &mut self,
        events: Vec<crate::ui::settings::SettingsEvent>,
        ctx: &egui::Context,
    ) {
        for event in events {
            match event {
                crate::ui::settings::SettingsEvent::Theme(choice) => {
                    self.prefs_theme = choice.clone();
                    self.shell
                        .store
                        .update_prefs(|prefs| prefs.theme = choice.clone());
                    crate::theme::apply_choice(&choice, ctx);
                }
                crate::ui::settings::SettingsEvent::Flip(field) => {
                    self.shell.store.update_prefs(|prefs| match field {
                        "enter_sends" => prefs.enter_sends = !prefs.enter_sends,
                        "group_by_project" => prefs.group_by_project = !prefs.group_by_project,
                        "confirm_swarm" => prefs.confirm_swarm = !prefs.confirm_swarm,
                        "constellation" => prefs.constellation = !prefs.constellation,
                        "fuel" => prefs.fuel = !prefs.fuel,
                        _ => {}
                    });
                }
                crate::ui::settings::SettingsEvent::Mode(value) => {
                    let _ = self.shell.store.set_permission(&value);
                }
                crate::ui::settings::SettingsEvent::Restore(id) => {
                    self.shell.store.restore(&id);
                }
                crate::ui::settings::SettingsEvent::Channel(channel) => {
                    self.shell
                        .store
                        .update_prefs(|prefs| prefs.channel = channel);
                    self.check_releases(true);
                }
                crate::ui::settings::SettingsEvent::CheckNow => self.check_releases(true),
                #[cfg(target_arch = "wasm32")]
                crate::ui::settings::SettingsEvent::Install(_)
                | crate::ui::settings::SettingsEvent::OpenReleases => {}
                #[cfg(not(target_arch = "wasm32"))]
                crate::ui::settings::SettingsEvent::Install(which) => {
                    self.start_install(which);
                }
                #[cfg(not(target_arch = "wasm32"))]
                crate::ui::settings::SettingsEvent::OpenReleases => {
                    crate::update::open_releases();
                }
                crate::ui::settings::SettingsEvent::ProviderForm(_)
                | crate::ui::settings::SettingsEvent::McpForm(_)
                | crate::ui::settings::SettingsEvent::AcpForm(_)
                | crate::ui::settings::SettingsEvent::Directory(_) => {}
            }
        }
        if self.shell.settings.is_open() && self.shell.settings.section().reads_config() {
            self.shell.store.ask_config();
        }
    }

    fn carry_workbench(&mut self, events: Vec<crate::ui::workbench::WorkbenchEvent>) {
        for event in events {
            match event {
                crate::ui::workbench::WorkbenchEvent::Mention(path) => {
                    self.shell.composer.mention(&path);
                }
                crate::ui::workbench::WorkbenchEvent::OpenAgent(id) => {
                    self.shell.store.load_child(&id);
                    self.shell.store.set_active(id);
                }
                crate::ui::workbench::WorkbenchEvent::StopAgent(id) => {
                    self.shell.store.cancel_session(&id);
                }
                crate::ui::workbench::WorkbenchEvent::AskFiles => {
                    if let Some(id) = self.shell.store.active_id().map(str::to_owned) {
                        self.shell.store.fs_list(&id);
                        self.shell.workbench.mark_listing_asked(&id);
                    }
                }
                crate::ui::workbench::WorkbenchEvent::ReadFile(path) => {
                    self.shell.store.fs_read(&path);
                }
            }
        }
    }

    /// Draws the dock row: the pills the session's state names, over
    /// the composer. The rows carry the shell's follow-up actions.
    /// Draws the approval card over the dock row: the oldest open
    /// permission ask of the active session and its subagents, with
    /// the digit keys while it holds them.
    fn show_approval(&mut self, ui: &mut egui::Ui, store: &mut crate::store::Store) {
        let open = store.active_asks().len();
        let events = self.shell.approval.show(ui, store);
        self.carry_approval(store, events);
        // A first ask takes the digit keys; the last one closing hands
        // them back to the shell.
        if open > 0 {
            if let Some(ask) = store.active_asks().first() {
                let request = ask.1.request_id.clone();
                self.shell.approval.focus(&request);
            }
        } else {
            self.shell.approval.blur();
        }
    }

    /// Carries out what the approval card asked for.
    fn carry_approval(
        &mut self,
        store: &mut crate::store::Store,
        events: Vec<crate::ui::approval::ApprovalEvent>,
    ) {
        for event in events {
            match event {
                crate::ui::approval::ApprovalEvent::Reply(session, request, decision) => {
                    let _ = store.reply_permission(&session, request, &decision);
                }
            }
        }
    }

    fn show_dock(&mut self, ui: &mut egui::Ui, store: &mut crate::store::Store) {
        let actions = crate::ui::dock::show(ui, store);
        for action in actions {
            match action {
                crate::ui::dock::DockAction::Steer(index) => {
                    let _ = store.steer_queued(index);
                }
                crate::ui::dock::DockAction::Edit(index) => {
                    if let Some(text) = store
                        .active_session()
                        .and_then(|session| session.queue.get(index))
                        .map(|queued| {
                            queued
                                .prompt
                                .iter()
                                .filter_map(|block| block.as_text())
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                    {
                        self.shell.composer.draft.text = text;
                        self.shell.composer.draft.caret = self.shell.composer.draft.text.len();
                        store.withdraw_queued(index);
                    }
                }
                crate::ui::dock::DockAction::Remove(index) => {
                    store.withdraw_queued(index);
                }
                crate::ui::dock::DockAction::ScrollToPlan => {
                    self.scroll_to_plan(store);
                }
                crate::ui::dock::DockAction::OpenBackgroundAgents => {
                    // The workbench's agents list, which E06 mounts.
                }
                crate::ui::dock::DockAction::OpenGoal => {
                    // The goal editor is the plus menu's dialog, which
                    // E07 mounts; the pill reports the ask for now.
                }
            }
        }
    }

    /// Brings the plan card under review into view, or the transcript's
    /// last plan item when no review waits.
    fn scroll_to_plan(&mut self, store: &crate::store::Store) {
        let Some(session) = store.active_session() else {
            return;
        };
        let plan_row = session
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| matches!(item, kage_client::TranscriptItem::Plan { .. }))
            .map(|(ix, _)| ix)
            .next_back();
        if let Some(ix) = plan_row {
            self.shell
                .transcript
                .scroll_to_row(crate::ui::transcript::model::RowKey::Item(ix));
        }
    }

    /// Draws the composer: the field, its toolbar and the dock row's
    /// goal popover. A send takes the draft out to the store.
    fn show_composer(&mut self, ui: &mut egui::Ui, store: &mut crate::store::Store) {
        let running = store
            .active_session()
            .is_some_and(|session| session.running);
        let enter_sends = store.prefs().enter_sends;
        let commands = store.composer_commands().to_vec();
        let listing = store
            .active_id()
            .and_then(|id| store.fs_listing(id))
            .cloned();
        self.shell.composer.set_listing(listing);
        let steer_available = store.state().steer_available();
        let event = self.shell.composer.show(ui, store, &commands, running, enter_sends);
        match event {
            crate::ui::composer::ComposerEvent::OpenGoal => {
                self.shell
                    .dialogs
                    .show_dialog(crate::ui::dialog::DialogKind::Goal, store);
            }
            crate::ui::composer::ComposerEvent::ConfirmSwarm => {
                self.shell
                    .dialogs
                    .show_dialog(crate::ui::dialog::DialogKind::ConfirmSwarm, store);
            }
            _ => {}
        }
        if let crate::ui::composer::ComposerEvent::Send { steer } = event
            && let Some(text) = self.shell.composer.take_prompt()
        {
            let builtin = crate::ui::composer::builtin(&text);
            let handled =
                builtin.is_some_and(|command| self.run_builtin(store, command.name, command.args));
            if !handled {
                // Ctrl+Enter steers a running turn; the engine
                // advertises the capability, so a steer that cannot go
                // out falls back to a plain submit.
                if steer && running && steer_available {
                    let _ = store.steer(&text);
                } else {
                    let _ = store.submit(&text);
                }
            }
        }
    }

    /// Runs a built-in composer command. Returns whether the client
    /// ran it, so an unhandled one goes to the agent as plain text.
    fn run_builtin(&mut self, store: &mut crate::store::Store, name: &str, args: &str) -> bool {
        match (name, args) {
            ("new", _) => {
                store.show_welcome();
                true
            }
            ("plan", "on") => store.enter_plan(),
            ("plan", "off") => store.exit_plan(),
            ("plan", _) => {
                if store.plan_on() {
                    store.exit_plan()
                } else {
                    store.enter_plan()
                }
            }
            ("goal", "clear") => crate::ui::dock::save_goal(store, ""),
            ("goal", text) => crate::ui::dock::save_goal(store, text),
            ("swarm", "off") => store.set_option("swarm", "off"),
            ("swarm", "on") => true,
            _ => false,
        }
    }

    /// The line the welcome column shows under the wordmark. The
    /// connect problems name themselves; once the link is up the line
    /// is the design's static hint, as the old welcome drew it.
    fn welcome_line(&self, store: &crate::store::Store) -> String {
        match store.connect() {
            crate::transport::State::Connecting => "connecting".to_owned(),
            crate::transport::State::Refused(why) => format!("refused: {why}"),
            _ => "Ctrl K commands and sessions".to_owned(),
        }
    }

    /// The active key context: a modal above the shell.
    fn context(&self) -> crate::ui::keys::Context {
        if self.shell.settings.is_open() {
            return crate::ui::keys::Context::Settings;
        }
        if self.shell.dialogs.is_open() {
            return crate::ui::keys::Context::Dialog;
        }
        if self.shell.palette.is_open() {
            return crate::ui::keys::Context::Palette;
        }
        if self.shell.find.is_open() {
            return crate::ui::keys::Context::Find;
        }
        if self.shell.store.prefs().vim {
            return crate::ui::keys::Context::VimNormal;
        }
        crate::ui::keys::Context::Shell
    }

    /// Reads the frame's key presses and carries out the actions the
    /// router dispatches. A modal context answers only its own keys.
    fn route_keys(&mut self, ctx: &egui::Context) {
        use crate::ui::keys::Press;
        let events: Vec<egui::Event> = ctx.input(|i| i.events.clone());
        for event in events {
            let egui::Event::Key {
                key,
                modifiers,
                pressed: true,
                ..
            } = event
            else {
                continue;
            };
            let press = Press {
                key: key_name(&key),
                mods: crate::ui::keys::Mods {
                    ctrl: modifiers.ctrl || modifiers.command,
                    shift: modifiers.shift,
                    alt: modifiers.alt,
                },
                repeat: false,
            };
            let Some(action) = crate::ui::keys::dispatch(self.context(), &press) else {
                // The approval card's digits are not the shell's
                // bindings: they answer the oldest open ask while the
                // card holds the keys and no field has them.
                if press.mods == crate::ui::keys::Mods::default()
                    && let Some(digit) = press.key.parse::<usize>().ok()
                    && self.approve_digit(digit)
                {
                    continue;
                }
                continue;
            };
            self.carry(action);
        }
    }

    /// Answers the oldest open permission ask with the option the
    /// 1-based `digit` names. Reports whether the card took it, so a
    /// digit aimed at a field or at the shell is left alone.
    fn approve_digit(&mut self, digit: usize) -> bool {
        let Some((session_id, ask)) = self
            .shell
            .store
            .active_asks()
            .first()
            .map(|(session_id, ask)| ((*session_id).to_owned(), (*ask).clone()))
        else {
            return false;
        };
        let Some(action) = self.shell.approval.digit(digit, &ask) else {
            return false;
        };
        let decision = match action {
            crate::ui::approval::DigitAction::Answer(index) => ask
                .options
                .get(index)
                .map(|option| kage_client::PermissionDecision::Option(option.option_id.clone())),
            crate::ui::approval::DigitAction::Toggle(index) => {
                self.shell.approval.toggle(ask.request_id.clone(), index);
                return true;
            }
        };
        if let Some(decision) = decision {
            let _ =
                self.shell
                    .store
                    .reply_permission(&session_id, ask.request_id.clone(), &decision);
            self.shell.approval.forget(&ask.request_id);
        }
        true
    }

    /// Carries out one routed action.
    fn carry(&mut self, action: crate::ui::keys::Action) {
        use crate::ui::keys::Action;
        match action {
            Action::Quit => {
                self.shell.smoke_deadline =
                    Some((web_time::Instant::now(), Duration::from_millis(0)));
            }
            Action::NewSession => self.shell.store.show_welcome(),
            Action::ToggleSidebar => self.shell.panels.toggle_sidebar(),
            Action::ToggleWorkbench => self.shell.panels.toggle_workbench(),
            Action::OpenPalette => self.shell.palette.open(),
            Action::OpenFind => self.shell.find.open(),
            Action::OpenSettings => self
                .shell
                .settings
                .open(crate::ui::settings::Section::General),
            Action::OpenProviders => self
                .shell
                .settings
                .open(crate::ui::settings::Section::Providers),
            Action::CloseDialog => {
                if self.shell.settings.is_open() {
                    // A form over the page answers the key in its own
                    // draw, so the dialog waits for the form to be
                    // gone before it closes.
                    if !self.shell.settings.form_open() {
                        self.shell.settings.close();
                    }
                } else {
                    self.shell.dialogs.close();
                }
            }
            Action::SendPrompt
            | Action::ToggleSwarm
            | Action::TogglePlan
            | Action::SetGoal
            | Action::ShowAgents
            | Action::ReviewChanges => {}
        }
    }

    /// The find bar's keys, in its own context: Enter steps forward,
    /// Shift+Enter back, Escape closes. The keys answer only while the
    /// bar is open, so they never reach the shell's own bindings.
    fn find_keys(&mut self) {
        if !self.shell.find.is_open() {
            return;
        }
        let ctx = self.ctx.clone();
        let events: Vec<egui::Event> = ctx.input(|i| i.events.clone());
        for event in events {
            let egui::Event::Key {
                key,
                modifiers,
                pressed: true,
                ..
            } = event
            else {
                continue;
            };
            let back = modifiers.shift;
            match key {
                egui::Key::Enter => {
                    if let Some(key) = self.shell.find.step(back) {
                        self.shell.transcript.scroll_to_row(key);
                    }
                }
                egui::Key::Escape => self.shell.find.close(),
                _ => {}
            }
        }
    }

    /// Runs the daily release check on its own thread and applies the
    /// answer on the UI thread: the check blocks on the network, so it
    /// never sits in the frame. The browser build never checks.
    #[cfg(not(target_arch = "wasm32"))]
    fn pump_update_check(&mut self) {
        let now = crate::clock::unix_seconds();
        if now < self.last_update_check + 24 * 60 * 60 {
            return;
        }
        self.last_update_check = now;
        let prefs = self.shell.store.prefs().clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("kage-update-check".to_owned())
            .spawn(move || {
                let outcome = crate::update::run_check(&prefs, false);
                let _ = tx.send(outcome);
            })
            .expect("the check thread starts");
        self.update_rx = Some(rx);
    }

    /// Runs a release check on its own thread when the daily throttle
    /// allows it, or past it when `force` is set, and applies the
    /// answer on the UI thread.
    fn check_releases(&mut self, force: bool) {
        if self.update_rx.is_some() {
            return;
        }
        let now = crate::clock::unix_seconds();
        if !force && now < self.last_update_check + 24 * 60 * 60 {
            return;
        }
        self.last_update_check = now;
        let prefs = self.shell.store.prefs().clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("kage-update-check".to_owned())
            .spawn(move || {
                let outcome = crate::update::run_check(&prefs, force);
                let _ = tx.send(outcome);
            })
            .expect("the check thread starts");
        self.update_rx = Some(rx);
    }

    /// Downloads the newest release of `which` on its own thread: the
    /// download blocks on the network, so it never sits in the frame.
    /// One at a time; a second click while one runs is ignored. The
    /// browser never installs anything, so it never gets here.
    #[cfg(not(target_arch = "wasm32"))]
    fn start_install(&mut self, which: crate::ui::settings::Install) {
        if self.installing.is_some() {
            return;
        }
        self.installing = Some(which);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("kage-install".to_owned())
            .spawn(move || {
                let result = match which {
                    crate::ui::settings::Install::Engine => crate::update::install_latest(),
                    crate::ui::settings::Install::Desktop => {
                        crate::update::install_desktop_latest()
                    }
                };
                let _ = tx.send((which, result));
            })
            .expect("the install thread starts");
        self.install_rx = Some(rx);
    }

    /// Applies a finished install: the engine is restarted on the new
    /// binary, the desktop waits for the next launch, and a refusal
    /// rides a toast.
    #[cfg(not(target_arch = "wasm32"))]
    fn apply_install(&mut self) {
        let Some(rx) = self.install_rx.take() else {
            return;
        };
        let Ok((which, result)) = rx.try_recv() else {
            self.install_rx = Some(rx);
            return;
        };
        self.installing = None;
        let ctx = self.ctx.clone();
        let (tone, text) = match result {
            Ok(path) if which == crate::ui::settings::Install::Engine => {
                let program = path.display().to_string();
                self.shell
                    .store
                    .update_prefs(|prefs| prefs.kage_path = Some(program.clone()));
                self.shell.use_engine(program, &ctx);
                (
                    kage_client::wire::NoticeTone::Success,
                    "engine installed; restarting it".to_owned(),
                )
            }
            Ok(path) => (
                kage_client::wire::NoticeTone::Success,
                format!(
                    "desktop installed at {}; restart kage to apply",
                    path.display()
                ),
            ),
            Err(why) => (kage_client::wire::NoticeTone::Error, why),
        };
        self.install_note = Some((
            matches!(tone, kage_client::wire::NoticeTone::Success),
            text.clone(),
        ));
        self.shell.store.note(tone, text);
    }

    /// Applies a finished release check to the preferences, once. The
    /// browser never checks, so this is native-only too.
    #[cfg(not(target_arch = "wasm32"))]
    fn apply_update(&mut self) {
        let Some(rx) = self.update_rx.take() else {
            return;
        };
        let Ok(outcome) = rx.try_recv() else {
            self.update_rx = Some(rx);
            return;
        };
        let Some(outcome) = outcome else {
            return;
        };
        let engine = self
            .shell
            .store
            .state()
            .agent
            .as_ref()
            .and_then(|agent| agent.version.clone());
        let prefs = self.shell.store.prefs().clone();
        let mut prefs = prefs;
        crate::update::apply_outcome(&mut prefs, outcome, engine.as_deref());
        self.shell.store.update_prefs(|current| *current = prefs);
    }
}

/// The prompt the recording expects, once its session is open.
const REPLAY_PROMPT: &str = "fix the null check";

/// The composer column's height: the field's minimum, its toolbar and
/// the hint line under it.
const COMPOSER_H: f32 = 132.0;

/// The dock row's height: the pills, with their wrap room.
const DOCK_H: f32 = 72.0;

/// The name the router spells a key by: the key's own name, lowercased
/// for the letter keys, as egui prints it.
fn key_name(key: &egui::Key) -> &'static str {
    match key {
        egui::Key::Enter => "Enter",
        egui::Key::Escape => "Escape",
        egui::Key::Tab => "Tab",
        egui::Key::ArrowUp => "ArrowUp",
        egui::Key::ArrowDown => "ArrowDown",
        egui::Key::ArrowLeft => "ArrowLeft",
        egui::Key::ArrowRight => "ArrowRight",
        other => {
            let name = other.name();
            // The router's table spells the letter keys uppercase, and
            // the punctuation by its own character.
            name
        }
    }
}

/// Parses the flags and runs until quit. Native only: the browser
/// entry is [`crate::web::start`].
#[cfg(not(target_arch = "wasm32"))]
pub fn run() {
    let launch = Launch::parse(std::env::args().skip(1));
    if let Some(error) = &launch.error {
        eprintln!("kage-desktop: {error}");
        std::process::exit(2);
    }
    let smoke_millis = launch.smoke_millis;
    let stream = launch.stream;
    let replay = launch.wire == Wire::Replay;
    let prefs = crate::prefs::load();
    let cwd = std::env::current_dir()
        .map(|dir| dir.to_string_lossy().into_owned())
        .unwrap_or_else(|_| ".".to_owned());
    let saved = prefs.kage_path.clone();
    let transport = launch.transport(saved.as_deref());

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("kage client")
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([960.0, 640.0])
            // The runtime icon applies on X11 only; the other
            // platforms take the icon from the packaged resources.
            .with_icon(crate::icon::window_icon().unwrap_or_default()),
        ..Default::default()
    };
    eframe::run_native(
        "kage client",
        options,
        Box::new(move |cc| {
            let mut app = ShellApp::new(cwd, replay, transport, &cc.egui_ctx);
            app.stream = stream;
            app.prefs_theme = prefs.theme.clone();
            crate::theme::apply_choice(&prefs.theme, &cc.egui_ctx);
            if let Some(millis) = smoke_millis {
                app.shell.smoke_deadline =
                    Some((web_time::Instant::now(), Duration::from_millis(millis)));
            }
            Ok(Box::new(app))
        }),
    )
    .expect("failed to open the window");
}

#[cfg(test)]
mod tests {
    use super::{Launch, Wire};

    fn parse(args: &[&str]) -> Launch {
        Launch::parse(args.iter().map(|arg| arg.to_string()))
    }

    #[test]
    fn defaults_have_no_smoke_or_stream_and_speak_stdio() {
        let launch = parse(&[]);
        assert_eq!(launch.smoke_millis, None);
        assert!(!launch.stream);
        assert_eq!(launch.wire, Wire::Stdio);
        assert!(launch.rpc_bin.is_none());
    }

    #[test]
    fn smoke_takes_an_optional_millis_value() {
        assert_eq!(parse(&["--smoke", "2500"]).smoke_millis, Some(2500));
        assert_eq!(parse(&["--smoke"]).smoke_millis, Some(0));
    }

    #[test]
    fn stream_flag_stands_alone() {
        let launch = parse(&["--stream"]);
        assert!(launch.stream);
        assert_eq!(launch.smoke_millis, None);
    }

    #[test]
    fn replay_selects_the_recording() {
        assert_eq!(parse(&["--replay"]).wire, Wire::Replay);
    }

    #[test]
    fn ws_needs_a_token_and_the_token_needs_ws() {
        let launch = parse(&["--ws", "ws://127.0.0.1:7433/acp"]);
        assert!(launch.error.is_some(), "a tokenless ws is refused");

        let launch = parse(&["--token", "t"]);
        assert!(launch.error.is_some(), "a token without ws is refused");

        let launch = parse(&["--ws", "ws://127.0.0.1:7433/acp", "--token", "t"]);
        assert_eq!(
            launch.wire,
            Wire::WebSocket {
                url: "ws://127.0.0.1:7433/acp".to_owned(),
                token: "t".to_owned(),
            }
        );
    }

    #[test]
    fn rpc_bin_cleans_quotes_spaces_and_unicode() {
        assert_eq!(
            parse(&["--rpc-bin", " \"/opt/my dir/kage\" "])
                .rpc_bin
                .as_deref(),
            Some("/opt/my dir/kage")
        );
        assert_eq!(
            parse(&["--rpc-bin", "'/opt/kage'"]).rpc_bin.as_deref(),
            Some("/opt/kage")
        );
        assert_eq!(
            parse(&["--rpc-bin", " ~/k\u{e4}g\u{e9} "])
                .rpc_bin
                .as_deref(),
            Some("~/k\u{e4}g\u{e9}")
        );
        assert_eq!(
            parse(&["--rpc-bin", "\"/opt/kage"]).rpc_bin.as_deref(),
            Some("\"/opt/kage"),
            "an unmatched quote stays"
        );
    }

    #[test]
    fn an_rpc_bin_that_cleans_to_empty_is_absent() {
        let launch = parse(&["--rpc-bin", "\"\""]);
        assert!(launch.rpc_bin.is_none());
        assert!(launch.error.is_none());
        assert_eq!(launch.wire, Wire::Stdio);
    }

    #[test]
    fn rpc_bin_overrides_the_engine_binary() {
        assert_eq!(
            parse(&["--rpc-bin", "/opt/kage"]).rpc_bin.as_deref(),
            Some("/opt/kage")
        );
        assert_eq!(parse(&["--rpc-bin", "/opt/kage"]).wire, Wire::Stdio);
    }

    #[test]
    fn unknown_flags_are_refused_and_listed() {
        let launch = parse(&["--typo"]);
        let error = launch.error.expect("the typo is refused");
        assert!(error.contains("--typo"), "{error}");

        let launch = parse(&["--first-typo", "--second-typo"]);
        let error = launch.error.expect("both typos are listed");
        assert!(
            error.contains("--first-typo") && error.contains("--second-typo"),
            "{error}"
        );
    }

    #[test]
    fn an_equals_form_value_flag_is_refused_with_the_space_form() {
        for arg in ["--ws=ws://127.0.0.1:7433/acp", "--token=t"] {
            let launch = parse(&[arg]);
            let error = launch.error.unwrap_or_else(|| panic!("{arg} is refused"));
            assert!(error.contains("space"), "{error}");
        }

        let launch = parse(&["--ws=", "--token=t"]);
        assert!(launch.error.is_some(), "an empty equals value is refused");
        assert!(launch.ws_url.is_none(), "nothing became a URL");
        assert_eq!(launch.wire, Wire::Stdio, "the launch never misroutes");
    }
}
