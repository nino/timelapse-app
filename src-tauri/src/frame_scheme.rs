//! The `frames://localhost/<YYYY-MM-DD>/<index>` URI scheme
//! (`http://frames.localhost/…` on Windows) that the viewer's `<img>` loads,
//! and the preview packs at `frames://localhost/previews/<name>`.

use crate::scrub_log;
use frame_source::FrameSource;
use std::time::Instant;
use tauri::http::{header, Response, StatusCode};

/// The response for a `frames://localhost/<YYYY-MM-DD>/<index>` request, or
/// for a preview pack at `frames://localhost/previews/<name>`.
pub(crate) fn frame_response(source: Option<&FrameSource>, path: &str) -> Response<Vec<u8>> {
    let reply = |status: StatusCode, message: String| {
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "text/plain")
            .body(message.into_bytes())
            .unwrap()
    };
    let Some(source) = source else {
        return reply(StatusCode::SERVICE_UNAVAILABLE, "frame source not ready".into());
    };
    let mut parts = path.trim_start_matches('/').split('/');
    let (Some(date), Some(index), None) = (parts.next(), parts.next(), parts.next()) else {
        return reply(StatusCode::BAD_REQUEST, format!("expected /<day>/<index>, got {path}"));
    };
    if date == "previews" {
        let (status, content_type, body) = match source.preview_pack(index) {
            Ok(Some(bytes)) => (StatusCode::OK, "application/octet-stream", bytes),
            Ok(None) => (StatusCode::NOT_FOUND, "text/plain", format!("no preview pack {index}").into_bytes()),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, "text/plain", e.to_string().into_bytes()),
        };
        return Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, content_type)
            // The page keeps the packs it loaded itself.
            .header(header::CACHE_CONTROL, "no-store")
            // The page `fetch`es packs from its own origin (tauri://),
            // which is another origin to this scheme; an `<img>` needs no
            // permission, but a fetch does, failures included, or the page
            // sees a network error instead of the status.
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .body(body)
            .unwrap();
    }
    let Ok(index) = index.parse::<usize>() else {
        return reply(StatusCode::BAD_REQUEST, format!("not a frame index: {index}"));
    };
    let started = Instant::now();
    let frame = source.frame(date, index);
    scrub_log::step("frameRequest", started.elapsed());
    if let Ok(frame) = &frame {
        scrub_log::count(if frame.mime == "image/png" { "pngFrames" } else { "videoFrames" }, 1);
        scrub_log::count("bytes", frame.bytes.len() as u64);
    }
    if chrono::Local::now().format("%Y-%m-%d").to_string() == date {
        scrub_log::count("todayFrames", 1);
    }
    match frame {
        Ok(frame) => Response::builder()
            .header(header::CONTENT_TYPE, frame.mime)
            // The same URL can name a different file if a black frame is
            // deleted mid-day; the frame source's own cache is what makes
            // repeat requests cheap.
            .header(header::CACHE_CONTROL, "no-store")
            .body(frame.bytes)
            .unwrap(),
        Err(e @ (frame_source::Error::NotADay(_) | frame_source::Error::OutOfRange { .. })) => {
            reply(StatusCode::NOT_FOUND, e.to_string())
        }
        Err(e) => reply(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths;
    use frame_source::{PreviewStore, Tools};
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn frame_source_in(root: &Path) -> (TempDir, FrameSource) {
        let cache = TempDir::new().unwrap();
        let source =
            FrameSource::new(root.to_path_buf(), cache.path().to_path_buf(), u64::MAX, Tools::new(paths::ffmpeg()))
                .unwrap();
        (cache, source)
    }

    #[test]
    fn test_frame_response_serves_a_screenshot() {
        let library = TempDir::new().unwrap();
        fs::create_dir(library.path().join("2026-10-04")).unwrap();
        fs::write(library.path().join("2026-10-04/00001.png"), b"png bytes").unwrap();
        let (_cache, source) = frame_source_in(library.path());

        let response = frame_response(Some(&source), "/2026-10-04/0");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "image/png");
        assert_eq!(response.body(), b"png bytes");
    }

    #[test]
    fn test_frame_response_serves_preview_packs() {
        let library = TempDir::new().unwrap();
        let (_cache, source) = frame_source_in(library.path());
        let (_none, without) = frame_source_in(library.path());
        let previews = TempDir::new().unwrap();
        let store = PreviewStore::open(previews.path().to_path_buf(), u64::MAX).unwrap();
        store.write("v-9", frame_source::PackKind::Full, &[(0, b"jpeg".to_vec())]).unwrap();
        let source = source.with_previews(Arc::new(store));

        let response = frame_response(Some(&source), "/previews/v-9.full.pack");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
        let pack = frame_source::previews::decode_pack(response.body()).unwrap();
        assert_eq!(pack, vec![(0, b"jpeg".to_vec())]);

        for path in ["/previews/v-8.full.pack", "/previews/..%2Fv-9.full.pack", "/previews/v-9.full"] {
            assert_eq!(frame_response(Some(&source), path).status(), StatusCode::NOT_FOUND, "{path}");
        }
        assert_eq!(
            frame_response(Some(&without), "/previews/v-9.full.pack").status(),
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn test_frame_response_rejects_bad_requests() {
        let library = TempDir::new().unwrap();
        let (_cache, source) = frame_source_in(library.path());

        assert_eq!(
            frame_response(None, "/2026-10-04/0").status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        for (path, status) in [
            ("/2026-10-04", StatusCode::BAD_REQUEST),
            ("/2026-10-04/abc", StatusCode::BAD_REQUEST),
            ("/2026-10-04/0/extra", StatusCode::BAD_REQUEST),
            ("/..%2Fetc/0", StatusCode::NOT_FOUND),
            ("/2026-10-04/0", StatusCode::NOT_FOUND),
        ] {
            assert_eq!(frame_response(Some(&source), path).status(), status, "{path}");
        }
    }
}
