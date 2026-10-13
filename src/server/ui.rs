//! The web UI: built into the binary from `web/dist` (rust-embed), or read
//! from the folder `UPNEXT_STATIC_DIR` names. A path that isn't a file gets
//! `index.html`, so the UI's own routes (`/settings`, `/later`) load it,
//! except under `assets/`: a page from before an upgrade asks for scripts
//! the new build doesn't have, and a 404 tells it so, where `index.html`
//! would fail as a script of the wrong type.
//!
//! Debug builds read `web/dist` from disk on each request, so a fresh
//! `npm run build` shows up without restarting the server. Release builds
//! carry it inside, and don't compile without it.

use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};

#[derive(rust_embed::RustEmbed)]
#[folder = "web/dist"]
#[cfg_attr(debug_assertions, allow_missing = true)]
struct Assets;

// A release build must carry the UI, never the placeholder page. If this
// line fails to compile, build the UI first: `cd web && npm ci && npm run build`.
#[cfg(not(debug_assertions))]
const _: &[u8] = include_bytes!("../../web/dist/index.html");

/// Whether this binary has a UI to serve (a debug build reads it from disk,
/// so it may not).
pub fn embedded() -> bool {
    Assets::get("index.html").is_some()
}

/// One file of the built-in UI, or `index.html` for any other path outside
/// `assets/`.
pub async fn embedded_ui(uri: Uri, headers: HeaderMap) -> Response {
    let wanted = uri.path().trim_start_matches('/');
    let found = if wanted.is_empty() { None } else { Assets::get(wanted).map(|f| (f, wanted)) };
    if found.is_none() && wanted.starts_with("assets/") {
        return missing_asset();
    }
    let Some((file, path)) = found.or_else(|| Assets::get("index.html").map(|f| (f, "index.html"))) else {
        return missing_ui().await;
    };
    let etag = format!("\"{}\"", file.metadata.sha256_hash().iter().map(|b| format!("{b:02x}")).collect::<String>());
    // Vite puts a hash in every name under assets/, so those never change.
    let cache = if path.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
        return (StatusCode::NOT_MODIFIED, [(header::ETAG, etag), (header::CACHE_CONTROL, cache.to_string())]).into_response();
    }
    let mime = file.metadata.mimetype().to_string();
    (StatusCode::OK, [(header::CONTENT_TYPE, mime), (header::ETAG, etag), (header::CACHE_CONTROL, cache.to_string())], file.data)
        .into_response()
}

/// A file under `assets/` that this build doesn't have, most likely one an
/// older build's page asks for.
pub fn missing_asset() -> Response {
    (StatusCode::NOT_FOUND, [(header::CONTENT_TYPE, "text/plain; charset=utf-8"), (header::CACHE_CONTROL, "no-cache")], "not found")
        .into_response()
}

/// The licences and notices of everything built into UpNext, made by
/// contrib/notices/generate.mjs. Every build carries them, with or without
/// the web UI, and Settings › About links to them.
const NOTICES: &str = include_str!("../../THIRD_PARTY_NOTICES.txt");

/// `/third-party-notices.txt`, open to anyone like the UI's own files.
pub async fn notices() -> Response {
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8"), (header::CACHE_CONTROL, "no-cache")], NOTICES).into_response()
}

/// The page when there's no UI to serve: the API still works.
pub async fn missing_ui() -> Response {
    (
        StatusCode::OK,
        [("content-type", "text/html; charset=utf-8")],
        "<!doctype html><meta charset=utf-8><title>UpNext</title>\
         <body style='font-family:system-ui;background:#0b0d12;color:#e6e6e6;padding:3rem'>\
         <h1>UpNext API is running</h1><p>The web UI is not built yet. Run <code>cd web &amp;&amp; npm ci &amp;&amp; npm run build</code>, \
         or set <code>UPNEXT_STATIC_DIR</code>.</p><p>API: <a href='/api/stats' style='color:#9cf'>/api/stats</a></p>",
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only when `web/dist` is built: the CI job for Rust doesn't build the UI.
    #[tokio::test]
    async fn the_built_in_ui_answers_every_page() {
        if !embedded() {
            return;
        }
        let page = embedded_ui("/settings".parse().unwrap(), HeaderMap::new()).await;
        assert_eq!(page.status(), StatusCode::OK);
        assert_eq!(page.headers()[header::CONTENT_TYPE], "text/html");
        assert_eq!(page.headers()[header::CACHE_CONTROL], "no-cache");
        let etag = page.headers()[header::ETAG].clone();
        let mut again = HeaderMap::new();
        again.insert(header::IF_NONE_MATCH, etag);
        assert_eq!(embedded_ui("/".parse().unwrap(), again).await.status(), StatusCode::NOT_MODIFIED);

        // Browsers install the UI only from a manifest served as one.
        let manifest = embedded_ui("/manifest.webmanifest".parse().unwrap(), HeaderMap::new()).await;
        assert_eq!(manifest.headers()[header::CONTENT_TYPE], "application/manifest+json");

        let script = Assets::iter().find(|f| f.starts_with("assets/") && f.ends_with(".js")).expect("a built script");
        let js = embedded_ui(format!("/{script}").parse().unwrap(), HeaderMap::new()).await;
        assert_eq!(js.status(), StatusCode::OK);
        assert_eq!(js.headers()[header::CONTENT_TYPE], "text/javascript");
        assert_eq!(js.headers()[header::CACHE_CONTROL], "public, max-age=31536000, immutable");

        // An older build's script is a 404, not the page.
        let gone = embedded_ui("/assets/SettingsPage-0ld0ld00.js".parse().unwrap(), HeaderMap::new()).await;
        assert_eq!(gone.status(), StatusCode::NOT_FOUND);
    }

    /// The notices name the crates and npm packages built in, and give
    /// their licence texts.
    #[tokio::test]
    async fn the_notices_cover_the_crates_and_the_ui() {
        let page = notices().await;
        assert_eq!(page.headers()[header::CONTENT_TYPE], "text/plain; charset=utf-8");
        for wanted in [
            "UpNext: third-party notices",
            "\naxum ",
            "\nrusqlite ",
            "\nreact ",
            "@fontsource-variable/figtree",
            "SIL OPEN FONT LICENSE",
            "Permission is hereby granted",
        ] {
            assert!(NOTICES.contains(wanted), "{wanted}");
        }
    }
}
