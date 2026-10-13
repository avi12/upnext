//! A log line per request, with no query string and no headers.

use super::*;
use std::sync::Mutex;

/// What the log lines were written to.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn each_request_is_logged_without_its_query_or_headers() {
    let app = test_app().await;
    let out = Captured::default();
    let writer = out.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter("upnext=warn,upnext::request=info")
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .finish();
    let _logging = tracing::subscriber::set_default(subscriber);
    // Tracing caches, per log statement, whether anyone wants it. While this
    // test's subscriber is the only one, another test's thread that reaches
    // the request line first decides that for its own thread, which has no
    // subscriber, and the line is then skipped here too. So the cache is
    // rebuilt once the line has been reached, until a line comes through,
    // and only the lines after that count.
    let mut tries = 0;
    while out.0.lock().unwrap().is_empty() {
        tries += 1;
        assert!(tries <= 50, "no request line came through");
        app.get("/api/v1/stats").await;
        tracing::callsite::rebuild_interest_cache();
    }
    out.0.lock().unwrap().clear();

    let req = Request::get("/api/v1/games?q=portal&code=s3cret-code")
        .header(header::HOST, "127.0.0.1:8787")
        .header(header::AUTHORIZATION, "Bearer upnext_s3cret-token")
        .header(header::COOKIE, "upnext_device=s3cret-cookie")
        .body(Body::empty())
        .unwrap();
    let (status, _) = app.send(req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "a token nobody issued");
    app.get("/api/v1/stats").await;
    let (status, _) = app.call("GET", "/api/v1/no-such-route?key=s3cret-key", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let text = String::from_utf8(out.0.lock().unwrap().clone()).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "{text}");
    assert!(lines[0].contains("INFO") && lines[0].contains("GET /api/v1/games 401 "), "{text}");
    assert!(lines[1].contains("GET /api/v1/stats 200 ") && lines[1].ends_with(" ms"), "{text}");
    assert!(lines[2].contains("GET /api/v1/no-such-route 404 "), "{text}");
    assert!(!text.contains("s3cret") && !text.contains('?'), "no query string, token or cookie: {text}");
}
