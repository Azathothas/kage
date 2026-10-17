//! Provider logos for the settings screens, fetched once and cached
//! on disk.
//!
//! The logos come from kage-models, which snapshots one SVG per
//! provider and publishes it beside the model catalog, at
//! `https://models.kage.qaidvoid.dev/logos/<id>.svg`. kage-models takes
//! them from models.dev and rejects the two replies that are not logos:
//! the generic glyph models.dev serves with HTTP 200 for an id it does
//! not know, and anything that is not an SVG. So what arrives here and
//! is an SVG is a logo, and the check left to do is only that it is one
//! ([`kind`]).
//!
//! A provider kage-models does not know is a custom one, which is in no
//! catalog. Those fall back to the favicon of their endpoint's own site:
//! the endpoint host's `/favicon.ico`, reached by dropping a leading
//! `api.` label (`api.example.com` serves no icon; `example.com` does).
//!
//! Each logo is fetched at most once per run, on a thread of its own:
//! the memory cache holds the bytes, the disk cache under the kage cache
//! directory holds them across runs, and an id that resolves to nothing
//! stays `Missing` so it does not re-fetch on every frame.
//!
//! The web build has no threads and no filesystem, so it asks the
//! browser for the bytes and hands them to egui by the same
//! `ImageSource::Bytes` path. That needs `connect-src` to allow the
//! catalog's origin, which the server's CSP does grant.

use std::collections::BTreeMap;
#[cfg(not(target_arch = "wasm32"))]
use std::io::Read as _;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

/// The logo an id resolves to, after at most one fetch.
#[derive(Clone)]
enum Logo {
    /// SVG text, for the SVG loader.
    Svg(Arc<[u8]>),
    /// A decoded favicon, waiting for its texture.
    Raster(Arc<egui::ColorImage>),
    /// The favicon's texture, once the UI thread made it.
    Texture(egui::TextureHandle),
    /// No logo anywhere for the id, or every fetch failed this run;
    /// the caller shows its fallback.
    Missing,
}

/// One run's logo cache. `Missing` doubles as the fetch-in-progress
/// marker: a frame before the fetch lands also shows the fallback
/// instead of stacking fetches.
static CACHE: LazyLock<Mutex<BTreeMap<String, Logo>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

static OFFLINE: AtomicBool = AtomicBool::new(false);

/// Stops this process fetching logos, so an id it has not seen yet
/// resolves to nothing. The probes call it: they must not reach the
/// network or the user's cache, and a fetch that lands mid-frame
/// would repaint under them.
pub fn stay_offline() {
    OFFLINE.store(true, Ordering::Relaxed);
}

/// What kind of logo the bytes are: SVG text, or a Windows icon. `None`
/// for anything else.
///
/// The SVG test looks for the root tag rather than a leading `<`. A host
/// that parks its domain answers `/favicon.ico` with an HTML page and a
/// 200, which a prefix check accepts and egui then fails to rasterize,
/// leaving a blank tile where a logo should be. Verified against a live
/// `kenari.id`, which serves 2942 bytes of HTML there.
fn kind(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x00\x00\x01\x00") {
        Some("ico")
    } else if is_svg(bytes) {
        Some("svg")
    } else {
        None
    }
}

/// Whether the bytes open an SVG document, tolerating a byte-order mark
/// and an XML declaration ahead of the root tag.
fn is_svg(bytes: &[u8]) -> bool {
    let body = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let head = &body[..body.len().min(1024)];
    let mut from = 0;
    while let Some(offset) = head[from..].iter().position(|byte| *byte == b'<') {
        let tag = &head[from + offset..];
        if tag.starts_with(b"<svg") {
            return true;
        }
        // Resume past this `<`, so a declaration or comment ahead of the
        // root is skipped rather than ending the search.
        from += offset + 1;
    }
    false
}

/// The logo `bytes` make, decoding a favicon into pixels.
fn logo_of(bytes: Vec<u8>) -> Logo {
    match kind(&bytes) {
        Some("svg") => Logo::Svg(bytes.into()),
        #[cfg(not(target_arch = "wasm32"))]
        Some("ico") => match image::load_from_memory_with_format(&bytes, image::ImageFormat::Ico) {
            Ok(decoded) => {
                let rgba = decoded.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                Logo::Raster(Arc::new(egui::ColorImage::from_rgba_unmultiplied(
                    size,
                    rgba.as_raw(),
                )))
            }
            Err(_) => Logo::Missing,
        },
        _ => Logo::Missing,
    }
}

/// Where the fetched logos live: `$XDG_CACHE_HOME/kage/logos`, else
/// the platform's own cache base. Native only; the web build keeps
/// its cache in this process alone.
#[cfg(not(target_arch = "wasm32"))]
fn disk_dir() -> Option<PathBuf> {
    let env = |key: &str| std::env::var_os(key).map(|value| value.to_string_lossy().into_owned());
    let home = env("HOME").map(PathBuf::from);
    let base = match std::env::consts::OS {
        "macos" => home.map(|home| home.join("Library").join("Caches")),
        "windows" => env("LOCALAPPDATA").map(PathBuf::from),
        _ => env("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| home.map(|home| home.join(".cache"))),
    }?;
    Some(base.join("kage").join("logos"))
}

/// The id's logo as an image source, fetching it in the background
/// when this run has not seen it yet. `site` is the provider's
/// endpoint URL when the config names one; its host's favicon is the
/// fallback source. `None` means the caller draws its fallback; a
/// fetch that lands asks `ctx` for a frame so the next one shows it.
#[must_use]
pub fn ensure(
    ctx: &egui::Context,
    id: &str,
    site: Option<&str>,
) -> Option<egui::ImageSource<'static>> {
    let mut cache = CACHE.lock().ok()?;
    match cache.get(id).cloned() {
        Some(Logo::Svg(bytes)) => {
            return Some(egui::ImageSource::Bytes {
                uri: format!("bytes://provider-logo-{id}.svg").into(),
                bytes: egui::load::Bytes::Shared(bytes),
            });
        }
        Some(Logo::Raster(image)) => {
            let texture = ctx.load_texture(
                format!("provider-logo-{id}"),
                (*image).clone(),
                egui::TextureOptions::LINEAR,
            );
            let source =
                egui::ImageSource::Texture(egui::load::SizedTexture::from_handle(&texture));
            cache.insert(id.to_owned(), Logo::Texture(texture));
            return Some(source);
        }
        Some(Logo::Texture(texture)) => {
            return Some(egui::ImageSource::Texture(
                egui::load::SizedTexture::from_handle(&texture),
            ));
        }
        Some(Logo::Missing) => return None,
        None => {}
    }
    cache.insert(id.to_owned(), Logo::Missing);
    drop(cache);
    if !OFFLINE.load(Ordering::Relaxed) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let id = id.to_owned();
            let site = site.map(str::to_owned);
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let logo = fetch(&id, site.as_deref(), disk_dir());
                if let Ok(mut cache) = CACHE.lock() {
                    cache.insert(id, logo);
                }
                ctx.request_repaint();
            });
        }
        // The browser build has no threads and no cache directory. It
        // asks for the SVG itself and lands the bytes in the same cache,
        // so `ensure` resolves on the frame after the request completes
        // exactly as the native build does.
        #[cfg(target_arch = "wasm32")]
        {
            let _ = site;
            web_fetch(ctx.clone(), id.to_owned(), kage_core::logo_url(id));
        }
    }
    None
}

/// Fetches the id's logo: the disk cache first, then kage-models, then
/// the endpoint host's favicon. Whatever fails, or is not a logo, ends
/// as [`Logo::Missing`].
#[cfg(not(target_arch = "wasm32"))]
fn fetch(id: &str, site: Option<&str>, dir: Option<PathBuf>) -> Logo {
    const LOGO_CAP: usize = 256 * 1024;
    if let Some(dir) = &dir {
        for ext in ["svg", "ico"] {
            if let Ok(bytes) = std::fs::read(dir.join(format!("{id}.{ext}")))
                && kind(&bytes).is_some()
            {
                return logo_of(bytes);
            }
        }
    }
    if let Some(bytes) =
        fetch_bytes(&kage_core::logo_url(id), LOGO_CAP).filter(|bytes| kind(bytes).is_some())
    {
        store(dir.as_deref(), id, "svg", &bytes);
        return logo_of(bytes);
    }
    // kage-models has no logo for this id, so it is a provider outside
    // the catalog. Its own site may still carry an icon. `api.example.com`
    // serves none, its web home does, so the machine label steps aside.
    if let Some(domain) = site.and_then(site_domain)
        && let Some(bytes) = fetch_bytes(&format!("https://{domain}/favicon.ico"), LOGO_CAP)
            .filter(|bytes| kind(bytes).is_some())
    {
        store(dir.as_deref(), id, "ico", &bytes);
        return logo_of(bytes);
    }
    Logo::Missing
}

/// Asks the browser for `url` and files the answer under `id`.
///
/// The bytes go into the same cache the native thread fills, so both
/// builds hand egui one `ImageSource::Bytes` and rasterize one way. A
/// non-2xx status, a network failure, or a body that is not a logo all
/// leave [`Logo::Missing`], and `Missing` is never retried within a run.
#[cfg(target_arch = "wasm32")]
fn web_fetch(ctx: egui::Context, id: String, url: String) {
    use wasm_bindgen::JsCast as _;
    use wasm_bindgen_futures::JsFuture;

    let settled = move |bytes: Option<Vec<u8>>| {
        let logo = bytes.map_or(Logo::Missing, logo_of);
        if let Ok(mut cache) = CACHE.lock() {
            cache.insert(id, logo);
        }
        ctx.request_repaint();
    };

    let future = async move {
        let Some(window) = web_sys::window() else {
            settled(None);
            return;
        };
        // `fetch_with_str` hands back a bare `Promise`, so the `Response`
        // arrives from awaiting it rather than from the call. The
        // `JsFuture` type argument is spelled out because `from` is
        // otherwise ambiguous: `Result<T, E>: From<T>` also applies and
        // inference picks that one, which has no `Future` impl.
        let promise = window.fetch_with_str(&url);
        let response = JsFuture::<wasm_bindgen::JsValue>::from(promise)
            .await
            .ok()
            .and_then(|value| value.dyn_into::<web_sys::Response>().ok())
            .filter(web_sys::Response::ok);
        let Some(response) = response else {
            settled(None);
            return;
        };
        // Unlike `fetch_with_str`, `array_buffer` returns the promise
        // inside a `Result`, so a thrown or rejected call needs no await
        // to report itself.
        let Some(promise) = response.array_buffer().ok() else {
            settled(None);
            return;
        };
        let bytes = JsFuture::<wasm_bindgen::JsValue>::from(promise)
            .await
            .ok()
            .map(|buffer| js_sys::Uint8Array::new(&buffer).to_vec());
        // A favicon fallback would need a second request; the catalog
        // covers every provider kage ships, so a missing logo here is a
        // provider with no mark and the caller's own glyph stands in.
        settled(bytes);
    };
    wasm_bindgen_futures::spawn_local(future);
}

/// The host of an endpoint URL, port and path dropped, a leading
/// `api.` label with them.
fn site_domain(site: &str) -> Option<&str> {
    let rest = site.split_once("://").map(|(_, rest)| rest).unwrap_or(site);
    let host = rest.split('/').next()?;
    let host = host.split(':').next()?;
    let host = host.strip_prefix("api.").unwrap_or(host);
    (!host.is_empty()).then_some(host)
}

/// Gets a URL's body as bytes, refusing past `cap`. Any failure,
/// including a non-2xx status, reads as `None`.
#[cfg(not(target_arch = "wasm32"))]
fn fetch_bytes(url: &str, cap: usize) -> Option<Vec<u8>> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent(concat!("kage-desktop/", env!("CARGO_PKG_VERSION")))
        .build();
    let response = agent.get(url).call().ok()?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(cap as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    (!bytes.is_empty()).then_some(bytes)
}

/// Writes the fetched bytes into the disk cache, best effort.
#[cfg(not(target_arch = "wasm32"))]
fn store(dir: Option<&std::path::Path>, id: &str, ext: &str, bytes: &[u8]) {
    if let Some(dir) = dir
        && std::fs::create_dir_all(dir).is_ok()
    {
        let _ = std::fs::write(dir.join(format!("{id}.{ext}")), bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svg_bytes_read_as_a_logo_and_other_markup_does_not() {
        assert!(kind(b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>").is_some());
        // A host that parks its domain serves HTML for /favicon.ico with
        // a 200. A leading-`<` test accepts this and egui then fails to
        // rasterize it, so the root tag is what counts.
        assert!(kind(b"<!DOCTYPE html><html lang=\"id\"><head>").is_none());
        assert!(kind(b"<html><body>404</body></html>").is_none());
        assert!(kind(b"{\"error\":\"not found\"}").is_none());
    }

    #[test]
    fn a_declaration_or_bom_ahead_of_the_root_still_reads_as_svg() {
        assert!(kind(b"<?xml version=\"1.0\"?><svg/>").is_some());
        assert!(kind(b"\xef\xbb\xbf<svg/>").is_some());
        // models.dev's own logos open with a newline.
        assert!(kind(b"\n<svg viewBox=\"0 0 24 24\"/>").is_some());
    }

    #[test]
    fn favicon_bytes_read_as_a_logo_and_other_magic_does_not() {
        let ico = [0u8, 0, 1, 0, 2, 0].into_iter().chain([0u8; 32]);
        assert!(kind(&ico.collect::<Vec<_>>()).is_some());
        assert!(kind(b"").is_none());
        assert!(kind(b"GIF89a").is_none());
    }

    #[test]
    fn an_ico_is_not_mistaken_for_svg_and_the_reverse_holds_too() {
        let ico = [0u8, 0, 1, 0, 2, 0].into_iter().chain([0u8; 32]);
        let ico = ico.collect::<Vec<_>>();
        assert_eq!(kind(&ico), Some("ico"));
        assert_eq!(kind(b"<svg/>"), Some("svg"));
    }

    #[test]
    fn site_domains_drop_scheme_path_port_and_api_label() {
        assert_eq!(
            site_domain("https://api.example.com/v1"),
            Some("example.com")
        );
        assert_eq!(site_domain("http://localhost:11434"), Some("localhost"));
        assert_eq!(site_domain("example.com/v1"), Some("example.com"));
        assert_eq!(site_domain("https://"), None);
    }
}
