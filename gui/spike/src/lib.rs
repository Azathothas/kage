//! Throwaway egui probe for the GPUI exit spike (`specs/epics/E00-spike.md`).
//! Deleted at cutover (E11).

use std::collections::HashMap;
use std::time::Duration;

use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use egui_virtual_list::VirtualList;
use web_time::Instant;

#[cfg(target_arch = "wasm32")]
macro_rules! spike_log {
    ($($arg:tt)*) => { log::info!($($arg)*) };
}
#[cfg(not(target_arch = "wasm32"))]
macro_rules! spike_log {
    ($($arg:tt)*) => { eprintln!($($arg)*) };
}

/// Probe configuration, parsed from CLI flags in the binary.
#[derive(Debug, Clone)]
pub struct Options {
    /// Number of markdown items loaded up front.
    pub rows: usize,
    /// Run the 30 Hz streaming append probe.
    pub stream: bool,
    /// Streaming bench length in seconds.
    pub bench_secs: u64,
    /// Close the window when the bench completes (for automation).
    pub auto_exit: bool,
    /// Exit after this many seconds regardless of bench state.
    pub exit_after_secs: Option<u64>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            rows: 5000,
            stream: false,
            bench_secs: 10,
            auto_exit: false,
            exit_after_secs: None,
        }
    }
}

/// Parses `--rows N`, `--stream`, `--bench-secs N`, `--auto-exit`.
pub fn options_from_args() -> Options {
    let mut opts = Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--list" => {}
            "--stream" => opts.stream = true,
            "--auto-exit" => opts.auto_exit = true,
            "--exit-after-secs" => {
                opts.exit_after_secs = args.next().and_then(|v| v.parse().ok());
            }
            "--rows" => {
                opts.rows = args.next().and_then(|v| v.parse().ok()).unwrap_or(5000);
            }
            "--bench-secs" => {
                opts.bench_secs = args.next().and_then(|v| v.parse().ok()).unwrap_or(10);
            }
            other => {
                eprintln!("egui-spike: ignoring unknown flag {other}");
            }
        }
    }
    opts
}

struct StreamState {
    start: Instant,
    next_tick: Instant,
    expected: u32,
    appended: u32,
    late_ms: Vec<f64>,
    done: bool,
}

/// The probe application: a virtual list over markdown items with
/// instrumentation for frame times, render window and streaming appends.
pub struct ProbeApp {
    opts: Options,
    items: Vec<String>,
    caches: HashMap<usize, CommonMarkCache>,
    list: VirtualList,
    composer: String,
    extra_items: usize,
    parses_this_frame: u32,
    max_parses_per_frame: u32,
    frame_ms: Vec<f64>,
    last_frame: Instant,
    frame_no: u32,
    start: Instant,
    stream: Option<StreamState>,
    next_report: Instant,
    last_rss_mb: f64,
    composer_focused: bool,
    composer_rect: Option<egui::Rect>,
    item_rects: Vec<(usize, egui::Rect)>,
}

impl ProbeApp {
    /// Builds the probe with its item set and stream state.
    pub fn new(opts: Options) -> Self {
        let items = (0..opts.rows).map(varied_markdown).collect();
        // hide_on_resize repaints every frame for 100ms after first
        // layout, which trips kittest's max_steps guard in tests; the
        // probe does not need the flicker guard.
        let mut list = VirtualList::new();
        list.hide_on_resize(None);
        let stream = opts.stream.then(|| StreamState {
            start: Instant::now(),
            next_tick: Instant::now(),
            expected: (opts.bench_secs * 30) as u32,
            appended: 0,
            late_ms: Vec::new(),
            done: false,
        });
        Self {
            next_report: Instant::now() + Duration::from_secs(5),
            last_frame: Instant::now(),
            opts,
            items,
            caches: HashMap::new(),
            list,
            composer: String::new(),
            extra_items: 0,
            parses_this_frame: 0,
            max_parses_per_frame: 0,
            frame_ms: Vec::new(),
            frame_no: 0,
            start: Instant::now(),
            stream,
            last_rss_mb: 0.0,
            composer_focused: false,
            composer_rect: None,
            item_rects: Vec::new(),
        }
    }

    /// Renders one frame of the probe into `ui`.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let now = Instant::now();
        let dt_ms = (now - self.last_frame).as_secs_f64() * 1000.0;
        self.last_frame = now;
        self.frame_no += 1;
        if self.frame_no > 5 {
            self.frame_ms.push(dt_ms);
        }
        self.parses_this_frame = 0;
        self.last_rss_mb = rss_mb().unwrap_or(self.last_rss_mb);

        self.step_stream(ui);
        self.maybe_report(now);

        if false && self.frame_no == 1 {
            spike_log!("[spike] viewport={:?} pixels_per_point={:?} dark_mode={} panel_fill={:?} theme={:?}",
                ui.input(|i| i.viewport_rect()), ui.ctx().pixels_per_point(),
                ui.visuals().dark_mode, ui.visuals().panel_fill, ui.ctx().theme());
        }

        let streaming = self
            .stream
            .as_ref()
            .is_some_and(|st| !st.done);
        egui::Panel::bottom("spike-composer").show_inside(ui, |ui| {
            let response = egui::TextEdit::multiline(&mut self.composer)
                .hint_text("composer (spike)")
                .desired_rows(2)
                .show(ui);
            self.composer_focused = response.response.has_focus();
            self.composer_rect = Some(response.response.rect);
            response
                .response
                .widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::TextEdit,
                        true,
                        "spike-composer",
                    )
                });
        });
        let Self {
            items,
            caches,
            list,
            parses_this_frame,
            ..
        } = self;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(streaming)
            .show(ui, |ui| {
                list.ui_custom_layout(ui, items.len(), |ui, i| {
                    *parses_this_frame += 1;
                    let cache = caches.entry(i).or_default();
                    let top = if false { ui.cursor().top() } else { 0.0 };
                    CommonMarkViewer::new().show(ui, cache, &items[i]);
                    if false && self.frame_no <= 3 && i < 12 {
                        self.item_rects.push((i, egui::Rect::from_min_max(
                            egui::pos2(ui.cursor().left(), top),
                            egui::pos2(ui.cursor().left() + 100.0, ui.cursor().top()),
                        )));
                    }
                    1
                });
            });
        self.max_parses_per_frame = self.max_parses_per_frame.max(self.parses_this_frame);

        if self.opts.auto_exit && self.stream.as_ref().is_some_and(|st| st.done) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if let Some(secs) = self.opts.exit_after_secs {
            if Instant::now() - self.start > Duration::from_secs(secs) {
                spike_log!("[spike] exit-after {}: {}", secs, self.summary_line());
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn step_stream(&mut self, ui: &mut egui::Ui) {
        let due_count = {
            let Some(st) = self.stream.as_mut() else {
                return;
            };
            if st.done {
                return;
            }
            let tick = Duration::from_millis(1000 / 30);
            let mut now = Instant::now();
            let mut due: Vec<f64> = Vec::new();
            while st.appended + (due.len() as u32) < st.expected && now >= st.next_tick {
                due.push((now - st.next_tick).as_secs_f64() * 1000.0);
                st.next_tick += tick;
                now = Instant::now();
            }
            let due_count = due.len() as u32;
            st.late_ms.extend(due);
            st.appended += due_count;
            due_count
        };
        for _ in 0..due_count {
            self.append_chunk();
        }
        ui.ctx().request_repaint_after(Duration::from_millis(2));
        let just_finished = {
            let Some(st) = self.stream.as_mut() else {
                return;
            };
            if !st.done && st.appended >= st.expected {
                st.done = true;
                true
            } else {
                false
            }
        };
        if just_finished {
            spike_log!("[spike] bench done: {}", self.summary_line());
        }
    }

    fn append_chunk(&mut self) {
        if self.extra_items % 30 == 29 || self.items.is_empty() {
            self.items.push(varied_markdown(self.opts.rows + self.extra_items));
        } else if let Some(tail) = self.items.last_mut() {
            tail.push_str(stream_chunk(self.extra_items));
        }
        self.extra_items += 1;
    }

    fn maybe_report(&mut self, now: Instant) {
        if now < self.next_report {
            return;
        }
        self.next_report = now + Duration::from_secs(5);
        if !self.item_rects.is_empty() {
            let rects: Vec<String> = self.item_rects.iter()
                .map(|(i, r)| format!("i{i}: y {:.0}-{:.0}", r.top(), r.bottom()))
                .collect();
            spike_log!("[spike] item_rects frame={}: {}", self.frame_no, rects.join(", "));
            self.item_rects.clear();
        }
        spike_log!(
            "[spike] t={}s {} composer={:?} len={} focused={} rect={:?}",
            (now - self.start).as_secs_f64(),
            self.stats_line(),
            if self.composer.chars().count() > 24 { "<24+ chars>" } else { self.composer.as_str() },
            self.composer.len(),
            self.composer_focused,
            self.composer_rect,
        );
    }

    fn stats_line(&self) -> String {
        let (p50, p95, p99) = percentiles(&self.frame_ms);
        let frames = self.frame_ms.len();
        let fps = if frames == 0 {
            0.0
        } else {
            1000.0 / (self.frame_ms.iter().sum::<f64>() / frames as f64)
        };
        format!(
            "frames={frames} fps={fps:.1} p50={p50:.1}ms p95={p95:.1}ms p99={p99:.1}ms \
             laid_out_max={} rss={:.0}MB",
            self.max_parses_per_frame, self.last_rss_mb
        )
    }

    fn summary_line(&self) -> String {
        let (p50, p95, p99) = percentiles(&self.frame_ms);
        let stream = self.stream.as_ref();
        let (appended, expected, late_p95) = stream
            .map(|st| {
                (
                    st.appended,
                    st.expected,
                    percentiles(&st.late_ms).1,
                )
            })
            .unwrap_or((0, 0, 0.0));
        format!(
            "summary: appends={appended}/{expected} late_p95={late_p95:.1}ms \
             frames={} p50={p50:.1}ms p95={p95:.1}ms p99={p99:.1}ms \
             laid_out_max={} rss={:.0}MB",
            self.frame_ms.len(),
            self.max_parses_per_frame,
            self.last_rss_mb
        )
    }

    /// Total number of items currently in the list.
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// How many markdown items the last frame laid out (render-window probe).
    pub fn parses_last_frame(&self) -> u32 {
        self.parses_this_frame
    }

    /// Whether the streaming bench has run to completion.
    pub fn bench_done(&self) -> bool {
        self.stream.as_ref().is_some_and(|st| st.done)
    }

    /// One-line summary of the last run (frame percentiles, appends, rss).
    pub fn summary(&self) -> String {
        self.summary_line()
    }
}

impl Drop for ProbeApp {
    fn drop(&mut self) {
        spike_log!("[spike] {}", self.summary_line());
    }
}

impl eframe::App for ProbeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

/// Runs the probe in a native glow window.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_native(opts: Options) -> eframe::Result {
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 800.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "egui spike",
        native,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(ProbeApp::new(opts)))
        }),
    )
}

/// Builds one deterministic markdown item with varied shape and height.
pub fn varied_markdown(i: usize) -> String {
    match i % 5 {
        0 => format!("# Heading {i}\n\nA short paragraph with `inline code` and **bold** text."),
        1 => {
            let para = "A fairly long paragraph that wraps over several display lines on a \
                        typical transcript column, mixing *emphasis*, `code spans` and plain \
                        prose. ".repeat(6);
            format!("## Note {i}\n\n{para}")
        }
        2 => format!(
            "### Tool run {i}\n\n```rust\nfn example_{i}() -> u32 {{\n    {} \n}}\n```\n",
            (0..(4 + i % 8))
                .map(|n| format!("    let v{n} = {n} * {};", i % 97))
                .collect::<Vec<_>>()
                .join("\n")
        ),
        3 => {
            let items = (0..(3 + i % 8))
                .map(|n| format!("- list entry {n} for item {i}"))
                .collect::<Vec<_>>()
                .join("\n");
            format!("## Plan {i}\n\n{items}\n")
        }
        _ => format!(
            "# Mixed {i}\n\nText with `code`, then:\n\n```\noutput line {i}\n```\n\n- one\n- two\n\n[link](https://example.com/{i})"
        ),
    }
}

// Web entry for the spike web probe.
#[cfg(target_arch = "wasm32")]
mod web;

fn stream_chunk(n: usize) -> &'static str {
    const CHUNKS: [&str; 6] = [
        "streaming text arrives in small chunks, ",
        "with `inline code` and **emphasis**, ",
        "sometimes a longer run of plain prose to wrap lines, ",
        "plus `code` again, ",
        "and a [link](https://example.com) now and then, ",
        "ending sentences at varied lengths. ",
    ];
    CHUNKS[n % CHUNKS.len()]
}

fn percentiles(samples: &[f64]) -> (f64, f64, f64) {
    if samples.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let mut v: Vec<f64> = samples.iter().copied().collect();
    v.sort_by(|a, b| a.total_cmp(b));
    let pick = |frac: f64| {
        let idx = ((v.len() as f64 - 1.0) * frac).round() as usize;
        v[idx.min(v.len() - 1)]
    };
    (pick(0.50), pick(0.95), pick(0.99))
}

#[cfg(not(target_arch = "wasm32"))]
fn rss_mb() -> Option<f64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kb: f64 = rest.trim().trim_end_matches("kB").trim().parse().ok()?;
            return Some(kb / 1024.0);
        }
    }
    None
}

#[cfg(target_arch = "wasm32")]
fn rss_mb() -> Option<f64> {
    None
}
