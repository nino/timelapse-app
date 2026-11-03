//! Serves the library's videos to the viewer's beta video player, as
//! `videos://localhost/<file name>` (`http://videos.localhost/…` on Windows).
//!
//! WebKit's media loader reads a video in byte ranges, starting with a probe
//! for its first two bytes, and won't play one from a server that can't
//! serve ranges. So this answers `Range` requests, reading only the bytes
//! asked for, at most `MAX_RANGE_BYTES` at a time like Tauri's own asset
//! protocol: the loader asks again for the rest. Only bare file names of
//! videos in the library root are served (`FrameSource::video_path`).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use tauri::http::{header, Response, StatusCode};

/// The most one response carries. An hour of video is tens of megabytes,
/// and WebKit asks for the whole rest of the file when it starts.
pub const MAX_RANGE_BYTES: u64 = 4 * 1024 * 1024;

/// The bytes `start..=end` that a `Range` header asks for, in a file of
/// `len` bytes. `Ok(None)` without a header or one this doesn't understand
/// (the whole file is sent); `Err(())` for a range past the end. Only the
/// first of several ranges is served, which the client can tell from
/// `Content-Range`.
fn byte_range(header: Option<&str>, len: u64) -> Result<Option<(u64, u64)>, ()> {
    let Some(spec) = header.and_then(|h| h.trim().strip_prefix("bytes=")) else {
        return Ok(None);
    };
    let first = spec.split(',').next().unwrap_or("").trim();
    let Some((from, to)) = first.split_once('-') else {
        return Ok(None);
    };
    let (from, to) = (from.trim(), to.trim());
    let (start, end) = if from.is_empty() {
        // `bytes=-N`: the last N bytes.
        let Ok(suffix) = to.parse::<u64>() else {
            return Ok(None);
        };
        if suffix == 0 || len == 0 {
            return Err(());
        }
        (len.saturating_sub(suffix), len - 1)
    } else {
        let Ok(start) = from.parse::<u64>() else {
            return Ok(None);
        };
        let end = if to.is_empty() {
            len.saturating_sub(1)
        } else {
            match to.parse::<u64>() {
                Ok(end) if end >= start => end.min(len.saturating_sub(1)),
                _ => return Ok(None),
            }
        };
        (start, end)
    };
    if start >= len {
        return Err(());
    }
    Ok(Some((start, end.min(start + MAX_RANGE_BYTES - 1))))
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("mp4") => "video/mp4",
        _ => "video/quicktime",
    }
}

fn plain(status: StatusCode, message: String) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(message.into_bytes())
        .unwrap()
}

/// The response for a request for the video at `path` (None when the name
/// isn't one of the library's videos) with the given `Range` header.
pub fn video_response(path: Option<&Path>, range: Option<&str>) -> Response<Vec<u8>> {
    let Some(path) = path else {
        return plain(StatusCode::NOT_FOUND, "no such video".into());
    };
    let read = || -> std::io::Result<Response<Vec<u8>>> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        let builder = Response::builder()
            .header(header::CONTENT_TYPE, content_type(path))
            .header(header::ACCEPT_RANGES, "bytes");
        match byte_range(range, len) {
            Err(()) => Ok(builder
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{len}"))
                .body(Vec::new())
                .unwrap()),
            Ok(None) => {
                let mut body = Vec::with_capacity(len as usize);
                file.read_to_end(&mut body)?;
                Ok(builder
                    .header(header::CONTENT_LENGTH, body.len())
                    .body(body)
                    .unwrap())
            }
            Ok(Some((start, end))) => {
                file.seek(SeekFrom::Start(start))?;
                let mut body = vec![0; (end - start + 1) as usize];
                file.read_exact(&mut body)?;
                Ok(builder
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"))
                    .header(header::CONTENT_LENGTH, body.len())
                    .body(body)
                    .unwrap())
            }
        }
    };
    match read() {
        Ok(response) => response,
        // Deleted since the viewer was told about it (another hour may have
        // been converted and the day replanned).
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => plain(StatusCode::NOT_FOUND, e.to_string()),
        Err(e) => plain(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const M: u64 = MAX_RANGE_BYTES;

    #[test]
    fn reads_the_ranges_webkit_asks_for() {
        // The first request WebKit's media loader makes.
        assert_eq!(byte_range(Some("bytes=0-1"), 100), Ok(Some((0, 1))));
        assert_eq!(byte_range(Some("bytes=10-"), 100), Ok(Some((10, 99))));
        assert_eq!(byte_range(Some("bytes=-10"), 100), Ok(Some((90, 99))));
        assert_eq!(byte_range(Some("bytes=-500"), 100), Ok(Some((0, 99))));
        // An end past the file stops at the file.
        assert_eq!(byte_range(Some("bytes=90-200"), 100), Ok(Some((90, 99))));
        // Only the first of several ranges.
        assert_eq!(byte_range(Some("bytes=0-1, 5-6"), 100), Ok(Some((0, 1))));
    }

    #[test]
    fn caps_long_ranges() {
        let len = 10 * M;
        assert_eq!(byte_range(Some("bytes=0-"), len), Ok(Some((0, M - 1))));
        assert_eq!(byte_range(Some("bytes=5-"), len), Ok(Some((5, M + 4))));
        assert_eq!(byte_range(Some(&format!("bytes=0-{}", len - 1)), len), Ok(Some((0, M - 1))));
        // The last stretch is shorter than the cap.
        assert_eq!(byte_range(Some(&format!("bytes={}-", len - 3)), len), Ok(Some((len - 3, len - 1))));
    }

    #[test]
    fn rejects_ranges_past_the_end_and_ignores_nonsense() {
        assert_eq!(byte_range(Some("bytes=100-"), 100), Err(()));
        assert_eq!(byte_range(Some("bytes=-0"), 100), Err(()));
        assert_eq!(byte_range(Some("bytes=0-"), 0), Err(()));
        assert_eq!(byte_range(None, 100), Ok(None));
        assert_eq!(byte_range(Some("items=0-1"), 100), Ok(None));
        assert_eq!(byte_range(Some("bytes=5-2"), 100), Ok(None));
        assert_eq!(byte_range(Some("bytes=x-"), 100), Ok(None));
    }

    #[test]
    fn serves_partial_content() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("2026-10-04--09-00-00--hourly.mov");
        std::fs::write(&path, b"0123456789").unwrap();

        let response = video_response(Some(&path), Some("bytes=2-4"));
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.body(), b"234");
        let headers = response.headers();
        assert_eq!(headers[header::CONTENT_RANGE], "bytes 2-4/10");
        assert_eq!(headers[header::CONTENT_LENGTH], "3");
        assert_eq!(headers[header::ACCEPT_RANGES], "bytes");
        assert_eq!(headers[header::CONTENT_TYPE], "video/quicktime");

        let whole = video_response(Some(&path), None);
        assert_eq!(whole.status(), StatusCode::OK);
        assert_eq!(whole.body(), b"0123456789");

        let past = video_response(Some(&path), Some("bytes=10-"));
        assert_eq!(past.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(past.headers()[header::CONTENT_RANGE], "bytes */10");
    }

    #[test]
    fn answers_not_found_for_unknown_or_deleted_videos() {
        let dir = TempDir::new().unwrap();
        assert_eq!(video_response(None, None).status(), StatusCode::NOT_FOUND);
        let gone = dir.path().join("2026-10-04--09-00-00--hourly.mov");
        assert_eq!(video_response(Some(&gone), Some("bytes=0-1")).status(), StatusCode::NOT_FOUND);
    }
}
