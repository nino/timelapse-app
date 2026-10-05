//! The only place that runs ffmpeg. Only `ffmpeg` itself is used, not
//! `ffprobe`, because ffmpeg is the one tool the app bundles.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Error;

/// Where to find ffmpeg.
#[derive(Debug, Clone)]
pub struct Tools {
    pub ffmpeg: PathBuf,
}

impl Tools {
    /// The copy bundled with the app (Tauri installs `externalBin` sidecars
    /// next to the executable, without the target-triple suffix), else the
    /// usual Homebrew paths, since macOS GUI apps don't inherit the shell's
    /// PATH, else whatever `ffmpeg` is on PATH.
    pub fn locate() -> Self {
        let exe_dir = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf));
        Self { ffmpeg: locate(exe_dir.as_deref(), "ffmpeg") }
    }
}

fn locate(exe_dir: Option<&Path>, name: &str) -> PathBuf {
    let dirs = exe_dir.into_iter().chain([Path::new("/opt/homebrew/bin"), Path::new("/usr/local/bin")]);
    for dir in dirs {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return candidate;
        }
    }
    PathBuf::from(name)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoInfo {
    pub frame_count: usize,
    pub fps: f64,
}

/// Frame count and rate of the first video stream. `Ok(None)` means ffmpeg
/// could not make sense of the file (the legacy library has a few truncated
/// stubs), which callers treat as "this video contributes no frames".
///
/// The count comes from stream-copying the video into ffmpeg's `framecrc`
/// muxer, which writes one line per packet without decoding anything: exact
/// for any container, and cached by the caller per file. (`-f null` with
/// `-progress` reports no frame count for a stream copy.)
pub fn probe(tools: &Tools, path: &Path) -> Result<Option<VideoInfo>, Error> {
    let output = Command::new(&tools.ffmpeg)
        .args(["-hide_banner", "-nostdin", "-nostats", "-i"])
        .arg(path)
        .args(["-map", "0:v:0", "-c", "copy", "-f", "framecrc", "-"])
        .output()
        .map_err(|e| Error::Tool(format!("could not run ffmpeg: {e}")))?;
    if !output.status.success() {
        return Ok(None);
    }
    let frame_count = output
        .stdout
        .split(|&b| b == b'\n')
        .filter(|line| !line.is_empty() && !line.starts_with(b"#"))
        .count();
    let fps = parse_fps(&String::from_utf8_lossy(&output.stderr));
    match (frame_count, fps) {
        (frame_count, Some(fps)) if frame_count > 0 => Ok(Some(VideoInfo { frame_count, fps })),
        _ => Ok(None),
    }
}

/// The frame rate from the input's stream line in ffmpeg's log, e.g.
/// `Stream #0:0[0x1](und): Video: hevc (Main) (hvc1 / 0x31637668), yuv420p, 1800x1124, 2386 kb/s, 15 fps, 15 tbr, 15360 tbn`.
/// `fps` is the average rate; `tbr` is the fallback ffmpeg itself uses.
fn parse_fps(log: &str) -> Option<f64> {
    let line = log.lines().find(|line| line.contains("Stream #0:") && line.contains("Video:"))?;
    let rate = |unit: &str| {
        line.split(", ")
            .find_map(|part| part.trim().strip_suffix(unit))
            .and_then(|value| parse_rate(value.trim()))
    };
    rate(" fps").or_else(|| rate(" tbr"))
}

fn parse_rate(value: &str) -> Option<f64> {
    // ffmpeg writes "15", "29.97", or "15k" for 15000.
    let (value, scale) = match value.strip_suffix('k') {
        Some(v) => (v, 1000.0),
        None => (value, 1.0),
    };
    let rate: f64 = value.parse().ok()?;
    (rate > 0.0).then_some(rate * scale)
}

/// Decode `count` frames starting at frame `first` into `out_dir` as
/// `0001.jpg`, `0002.jpg`, …, returning how many were written (fewer than
/// `count` at the end of the video).
pub fn extract_frames(
    tools: &Tools,
    video: &Path,
    info: VideoInfo,
    first: usize,
    count: usize,
    out_dir: &Path,
) -> Result<usize, Error> {
    fs::create_dir_all(out_dir)?;
    // Seek a quarter frame early. ffmpeg keeps frames whose timestamp is at or
    // after the seek point, so landing a hair past frame `first` (float
    // rounding of first / fps) would silently start one frame late.
    let seek = if first == 0 { 0.0 } else { (first as f64 - 0.25) / info.fps };
    let output = Command::new(&tools.ffmpeg)
        .args(["-v", "error", "-nostdin", "-ss", &format!("{seek:.6}")])
        .arg("-i")
        .arg(video)
        .args(["-frames:v", &count.to_string(), "-q:v", "3"])
        .arg(out_dir.join("%04d.jpg"))
        .output()
        .map_err(|e| Error::Tool(format!("could not run ffmpeg: {e}")))?;
    if !output.status.success() {
        return Err(Error::Tool(format!(
            "ffmpeg failed on {}: {}",
            video.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let written = fs::read_dir(out_dir)?
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().ends_with(".jpg"))
        .count();
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frame_rates_from_the_log() {
        let hevc = "  Stream #0:0[0x1](und): Video: hevc (Main) (hvc1 / 0x31637668), yuv420p(tv), 1800x1124, 2386 kb/s, 15 fps, 15 tbr, 15360 tbn (default)";
        assert_eq!(parse_fps(&format!("Input #0\n{hevc}\n")), Some(15.0));
        let ntsc = "  Stream #0:0: Video: h264, yuv420p, 640x480, 29.97 fps, 29.97 tbr, 30k tbn";
        assert_eq!(parse_fps(ntsc), Some(29.97));
        let no_fps = "  Stream #0:0: Video: mjpeg, yuvj420p, 640x480, 25 tbr, 25 tbn";
        assert_eq!(parse_fps(no_fps), Some(25.0));
        assert_eq!(parse_fps("  Stream #0:1: Audio: aac, 44100 Hz"), None);
        assert_eq!(parse_rate("30k"), Some(30000.0));
        assert_eq!(parse_rate("0"), None);
    }

    #[test]
    fn prefers_a_bundled_ffmpeg_next_to_the_executable() {
        let dir = tempfile::TempDir::new().unwrap();
        assert_ne!(locate(Some(dir.path()), "ffmpeg"), dir.path().join("ffmpeg"));
        fs::write(dir.path().join("ffmpeg"), b"").unwrap();
        assert_eq!(locate(Some(dir.path()), "ffmpeg"), dir.path().join("ffmpeg"));
    }
}
