//! The browser entry: boots the shell once `web/boot.js` has drawn
//! its connection form.
//!
//! `boot.js` owns the page: it renders the server URL and token form,
//! reads the `?ws=` developer override, and only then loads this
//! bundle. It hands both values over through `window.__kageConnect`,
//! so the token never rides a URL. This module reads that handoff,
//! dials the WebSocket subprotocol with it, and starts the same shell
//! the native binary runs.
//!
//! Everything target-specific is behind a cfg in [`crate::launch`]:
//! the browser has no local engine, so the setup screen never shows
//! and the release check never runs.

use wasm_bindgen::JsCast as _;

use crate::launch;

/// What `boot.js` hands over once the connection form is submitted.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Handshake {
    /// The `ws://` endpoint of a `kage serve`.
    server: String,
    /// The token the endpoint expects, carried as a subprotocol.
    token: String,
}

/// Reads `window.__kageConnect`, or `None` while the form still shows.
///
/// The handoff is a plain object literal from `boot.js`, so it is read
/// by key through `Reflect` rather than bound to a struct: a missing
/// or non-string key refuses the dial instead of panicking in the
/// browser.
fn handshake() -> Option<Handshake> {
    let value: wasm_bindgen::JsValue = web_sys::window()?
        .get("__kageConnect")
        .map(wasm_bindgen::JsValue::from)?;
    if value.is_null() || value.is_undefined() {
        return None;
    }
    let field = |name: &str| {
        let key: wasm_bindgen::JsValue = name.into();
        js_sys::Reflect::get(&value, &key)
            .ok()
            .filter(|found| !found.is_null() && !found.is_undefined())
            .and_then(|found| found.as_string())
    };
    Some(Handshake {
        server: field("server")?,
        token: field("token")?,
    })
}

/// Starts the shell on `canvas`. The future resolves once the canvas
/// carries the app.
pub async fn start(canvas: web_sys::HtmlCanvasElement) {
    let runner = eframe::WebRunner::new();
    let result = runner
        .start(
            canvas,
            eframe::WebOptions::default(),
            Box::new(move |cc| {
                let prefs = crate::prefs::load();
                crate::theme::install_fonts(&cc.egui_ctx);
                crate::icons::install(&cc.egui_ctx);
                crate::theme::apply_choice(&prefs.theme, &cc.egui_ctx);
                // Until the form is submitted there is no endpoint, so
                // the shell gets the recording: the canvas stays alive
                // and empty, and boot.js keeps owning the page. Once
                // the handoff lands the browser transport dials it.
                let (transport, replay) = match handshake() {
                    Some(handshake) => (
                        Box::new(crate::transport::web::WebTransport::new(
                            handshake.server,
                            handshake.token,
                        )) as Box<dyn crate::transport::Transport>,
                        false,
                    ),
                    None => (
                        Box::new(crate::transport::replay::ReplayTransport::new())
                            as Box<dyn crate::transport::Transport>,
                        true,
                    ),
                };
                Ok(Box::new(launch::ShellApp::new(
                    String::new(),
                    replay,
                    transport,
                    &cc.egui_ctx,
                )))
            }),
        )
        .await;
    if let Err(why) = result {
        crate::warn(&format!("the browser runner did not start: {why:?}"));
    }
}

/// The wasm entry the generated glue calls: finds the page canvas by
/// id and starts the shell on it.
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn boot() {
    let canvas: Option<web_sys::HtmlCanvasElement> = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id("kage-canvas"))
        .map(|element| element.dyn_into::<web_sys::HtmlCanvasElement>().ok())
        .flatten();
    let Some(canvas) = canvas else {
        crate::warn("boot.js did not put a canvas in the page");
        return;
    };
    wasm_bindgen_futures::spawn_local(async move {
        start(canvas).await;
    });
}
