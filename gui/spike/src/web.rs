//! wasm entry for the spike web probe.

use eframe::egui;
use wasm_bindgen::prelude::*;

/// Handle to the running web app, held from JavaScript.
#[wasm_bindgen]
pub struct WebHandle {
    runner: eframe::WebRunner,
}

#[wasm_bindgen]
impl WebHandle {
    /// Installs the console logger, then returns.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        eframe::WebLogger::init(log::LevelFilter::Info).ok();
        Self {
            runner: eframe::WebRunner::new(),
        }
    }

    /// Starts the probe app on the given canvas.
    pub async fn start(
        &self,
        canvas: web_sys::HtmlCanvasElement,
    ) -> Result<(), wasm_bindgen::JsValue> {
        let web_options = eframe::WebOptions {
            renderer: eframe::Renderer::Glow,
            ..Default::default()
        };
        self.runner
            .start(
                canvas,
                web_options,
                Box::new(|cc| {
                    cc.egui_ctx.set_theme(egui::ThemePreference::Dark);
                    Ok(Box::new(crate::ProbeApp::new(crate::Options::default())))
                }),
            )
            .await
    }
}
