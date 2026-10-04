//! The only place that runs ffprobe/ffmpeg.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Error;

/// Where to find the ffmpeg tools. macOS GUI apps don't inherit the shell's
/// PATH, so a bare `ffmpeg` often isn't found from the bundled app even when it
/// works in a terminal; `Tools::locate` also checks the usual Homebrew paths.
#[derive(Debug, Clone)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

impl Tools {
    pub fn locate() -> Self {
        Self {
            ffmpeg: locate("ffmpeg"),
            ffprobe: locate("ffprobe"),
        }
    }
}

fn locate(name: &str) -> PathBuf {
    for dir in ["/opt/homebrew/bin", "/usr/local/bin"] {
        let candidate = Path::new(dir).join(name);
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

/// Frame count and rate of the first video stream. `Ok(None)` means ffprobe
/// could not make sense of the file (the legacy library has a few truncated
/// stubs), which callers treat as "this video contributes no frames".
pub fn probe(tools: &Tools, path: &Path) -> Result<Option<VideoInfo>, Error> {
    let output = Command::new(&tools.ffprobe)
        .args(["-v", "error", "-select_streams", "v:0"])
        .args(["-show_entries", "stream=nb_frames,avg_frame_rate", "-of", "default=nw=1"])
        .arg(path)
        .output()
        .map_err(|e| Error::Tool(format!("could not run ffprobe: {e}")))?;
    if !output.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut frame_count = None;
    let mut fps = None;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("nb_frames=") {
            frame_count = value.trim().parse::<usize>().ok();
        } else if let Some(value) = line.strip_prefix("avg_frame_rate=") {
            fps = parse_rate(value.trim());
        }
    }
    let Some(fps) = fps else { return Ok(None) };
    let frame_count = match frame_count {
        Some(n) => n,
        // Some containers don't record a frame count; counting packets reads
        // the whole file, so it is only the fallback.
        None => match count_packets(tools, path)? {
            Some(n) => n,
            None => return Ok(None),
        },
    };
    Ok((frame_count > 0).then_some(VideoInfo { frame_count, fps }))
}

fn count_packets(tools: &Tools, path: &Path) -> Result<Option<usize>, Error> {
    let output = Command::new(&tools.ffprobe)
        .args(["-v", "error", "-select_streams", "v:0", "-count_packets"])
        .args(["-show_entries", "stream=nb_read_packets", "-of", "csv=p=0"])
        .arg(path)
        .output()
        .map_err(|e| Error::Tool(format!("could not run ffprobe: {e}")))?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().parse().ok())
}

fn parse_rate(value: &str) -> Option<f64> {
    let (num, den) = value.split_once('/').unwrap_or((value, "1"));
    let (num, den): (f64, f64) = (num.parse().ok()?, den.parse().ok()?);
    (num > 0.0 && den > 0.0).then(|| num / den)
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
    fn parses_frame_rates() {
        assert_eq!(parse_rate("15/1"), Some(15.0));
        assert_eq!(parse_rate("30000/1001").map(|r| (r * 100.0).round()), Some(2997.0));
        assert_eq!(parse_rate("0/0"), None);
        assert_eq!(parse_rate("25"), Some(25.0));
    }
}
