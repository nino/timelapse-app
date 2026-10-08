//! The only place that runs ffmpeg. Only `ffmpeg` itself is used, not
//! `ffprobe`, because ffmpeg is the one tool the app bundles.

use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::Error;

/// Which ffmpeg to run. The crate doesn't look for one itself: the app passes
/// its bundled sidecar (`paths::ffmpeg()`), so there is one rule for where
/// ffmpeg comes from and it never falls back to a system install.
#[derive(Debug, Clone)]
pub struct Tools {
    pub ffmpeg: PathBuf,
}

impl Tools {
    pub fn new(ffmpeg: impl Into<PathBuf>) -> Self {
        Self {
            ffmpeg: ffmpeg.into(),
        }
    }
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
    let line = log
        .lines()
        .find(|line| line.contains("Stream #0:") && line.contains("Video:"))?;
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

/// How urgently a decode is wanted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    /// Someone is waiting for this frame.
    Now,
    /// Read-ahead: runs at a lower CPU priority (nice 10), so it gives way to
    /// decodes someone is waiting for and to the capture loop. Not
    /// background-throttled like the converter's encodes, because the viewer
    /// may reach the chunk and wait for it to finish.
    Background,
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
    priority: Priority,
) -> Result<usize, Error> {
    fs::create_dir_all(out_dir)?;
    let seek = seek_to(first, info);
    let mut command = Command::new(&tools.ffmpeg);
    if priority == Priority::Background {
        lower_priority(&mut command);
    }
    let output = command
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

/// One decoded frame: `width` × `height` pixels of 8-bit RGB, row by row.
pub struct RawFrame<'a> {
    pub width: u32,
    pub height: u32,
    pub rgb: &'a [u8],
}

/// Decode every frame of `video` from frame `first` on, handing each to
/// `on_frame` with its index in the video, without writing anything to disk.
/// Stops early, killing ffmpeg, when `on_frame` returns false.
///
/// This is for bulk work that reads a whole video once, such as OCR, so
/// ffmpeg always runs at background priority.
pub fn stream_frames(
    tools: &Tools,
    video: &Path,
    info: VideoInfo,
    first: usize,
    on_frame: &mut dyn FnMut(usize, RawFrame) -> bool,
) -> Result<(), Error> {
    let mut command = Command::new(&tools.ffmpeg);
    lower_priority(&mut command);
    // PPM rather than raw video: each frame carries its own size, so nothing
    // has to be probed first.
    let mut child = command
        .args(["-v", "error", "-nostdin", "-ss", &format!("{:.6}", seek_to(first, info))])
        .arg("-i")
        .arg(video)
        .args(["-f", "image2pipe", "-c:v", "ppm", "-pix_fmt", "rgb24", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Tool(format!("could not run ffmpeg: {e}")))?;

    // Drained on its own thread so a stream of decode errors can't fill the
    // pipe and stall ffmpeg while this thread waits on its frames.
    let mut stderr = child.stderr.take().expect("stderr is piped");
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });

    let mut stdout = BufReader::new(child.stdout.take().expect("stdout is piped"));
    let mut buffer = Vec::new();
    let mut index = first;
    let finished = loop {
        let Some((width, height)) = read_ppm_header(&mut stdout)? else {
            break true;
        };
        buffer.resize(width as usize * height as usize * 3, 0);
        stdout.read_exact(&mut buffer)?;
        let frame = RawFrame {
            width,
            height,
            rgb: &buffer,
        };
        if !on_frame(index, frame) {
            break false;
        }
        index += 1;
    };

    if !finished {
        let _ = child.kill();
    }
    let status = child.wait()?;
    let errors = errors.join().unwrap_or_default();
    if finished && !status.success() {
        return Err(Error::Tool(format!(
            "ffmpeg failed on {}: {}",
            video.display(),
            errors.trim()
        )));
    }
    Ok(())
}

/// The size from a binary PPM header (`P6 <width> <height> 255` and one
/// whitespace byte), or `None` at the end of the stream.
fn read_ppm_header(input: &mut impl BufRead) -> Result<Option<(u32, u32)>, Error> {
    let mut fields = Vec::new();
    while fields.len() < 4 {
        let mut field = Vec::new();
        loop {
            let mut byte = [0u8];
            if input.read(&mut byte)? == 0 {
                if fields.is_empty() && field.is_empty() {
                    return Ok(None);
                }
                return Err(Error::Tool("ffmpeg's output ended inside a frame header".into()));
            }
            if byte[0].is_ascii_whitespace() {
                if field.is_empty() {
                    continue;
                }
                break;
            }
            field.push(byte[0]);
        }
        fields.push(String::from_utf8_lossy(&field).into_owned());
    }
    let number = |field: &str| field.parse::<u32>().ok();
    match (fields[0].as_str(), number(&fields[1]), number(&fields[2]), fields[3].as_str()) {
        ("P6", Some(width), Some(height), "255") => Ok(Some((width, height))),
        _ => Err(Error::Tool(format!("unexpected frame header from ffmpeg: {fields:?}"))),
    }
}

/// Where to seek to start at frame `first`: a quarter frame early. ffmpeg
/// keeps frames whose timestamp is at or after the seek point, so landing a
/// hair past frame `first` (float rounding of first / fps) would silently
/// start one frame late.
fn seek_to(first: usize, info: VideoInfo) -> f64 {
    if first == 0 {
        0.0
    } else {
        (first as f64 - 0.25) / info.fps
    }
}

#[cfg(unix)]
fn lower_priority(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: setpriority is a single system call, safe between fork and exec.
    unsafe {
        command.pre_exec(|| {
            if libc::setpriority(libc::PRIO_PROCESS, 0, 10) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn lower_priority(_command: &mut Command) {}

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
    fn reads_ppm_headers() {
        let mut stream: &[u8] = b"P6\n3 2\n255\nxyz";
        assert_eq!(read_ppm_header(&mut stream).unwrap(), Some((3, 2)));
        assert_eq!(stream, b"xyz");
        assert_eq!(read_ppm_header(&mut &b""[..]).unwrap(), None);
        assert!(read_ppm_header(&mut &b"P6\n3"[..]).is_err());
        assert!(read_ppm_header(&mut &b"P5 3 2 255\n"[..]).is_err());
    }
}
