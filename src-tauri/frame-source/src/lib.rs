//! Answers "frame N of day D" for a timelapse library.
//!
//! Callers never learn where a frame comes from. A day is served from its
//! screenshots (`<root>/YYYY-MM-DD/NNNNN.png`) when it has any, and otherwise
//! from its videos (`YYYY-MM-DD--HH-MM-SS.mov|mp4`, in the root or the day
//! folder), played back to back in start order. Video frames are decoded a
//! chunk at a time into a size-capped LRU cache, so scrubbing through a long
//! day only decodes the stretches actually looked at.
//!
//! Screenshots are preferred because they are never deleted (videos made from
//! them are copies), and because they are the only source for a day still
//! being captured.

mod cache;
mod library;
mod video;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use chrono::{DateTime, Duration, Local, NaiveDate};
use serde::Serialize;

use cache::ChunkCache;
use library::VideoFile;
pub use video::Tools;
use video::VideoInfo;

/// Frames decoded per ffmpeg run: 10 seconds of a 15 fps timelapse. Large
/// enough that arrow-key scrubbing rarely waits on ffmpeg, small enough that a
/// jump to a new spot comes back quickly.
pub const CHUNK_FRAMES: usize = 150;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("not a day: {0:?}")]
    NotADay(String),
    #[error("frame {index} is past the end of {day} ({count} frames)")]
    OutOfRange { day: String, index: usize, count: usize },
    #[error("{0}")]
    Tool(String),
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// Where a day's frames come from. Only for display; callers don't need it to
/// fetch frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Screenshots,
    Video,
    Empty,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaySummary {
    pub date: String,
    pub frame_count: usize,
    pub source: Source,
}

pub struct Frame {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameTime {
    /// ISO 8601. Has an offset when it came from the database or a file
    /// timestamp; a naive local time when estimated.
    pub local_time: String,
    /// False when estimated from a video's start time, assuming one capture
    /// per second with no gaps.
    pub exact: bool,
}

#[derive(Clone, Copy)]
struct Probe {
    len: u64,
    modified: SystemTime,
    fingerprint: (u64, u64),
    /// None for files ffprobe can't read.
    info: Option<VideoInfo>,
}

enum Plan {
    Screenshots(Arc<Vec<String>>),
    Videos(Vec<(VideoFile, VideoInfo)>),
}

pub struct FrameSource {
    root: PathBuf,
    tools: Tools,
    cache: ChunkCache,
    /// Per-video facts keyed by path, valid while size and mtime match.
    probes: Mutex<HashMap<PathBuf, Probe>>,
    /// Screenshot listings keyed by day, valid while the folder's mtime
    /// matches. Today's folder changes every second; older ones never do.
    screenshots: Mutex<HashMap<NaiveDate, (SystemTime, Arc<Vec<String>>)>>,
}

impl FrameSource {
    /// `cache_dir` should be outside the library: it is disposable, and the
    /// library may be synced.
    pub fn new(root: PathBuf, cache_dir: PathBuf, cache_cap_bytes: u64) -> std::io::Result<Self> {
        Self::with_tools(root, cache_dir, cache_cap_bytes, Tools::locate())
    }

    pub fn with_tools(
        root: PathBuf,
        cache_dir: PathBuf,
        cache_cap_bytes: u64,
        tools: Tools,
    ) -> std::io::Result<Self> {
        Ok(Self {
            root,
            tools,
            cache: ChunkCache::open(cache_dir, cache_cap_bytes)?,
            probes: Mutex::new(HashMap::new()),
            screenshots: Mutex::new(HashMap::new()),
        })
    }

    /// Every day with screenshots or videos, oldest first, as `YYYY-MM-DD`.
    /// Cheap: it reads directory names only.
    pub fn days(&self) -> Result<Vec<String>, Error> {
        match library::list_days(&self.root) {
            Ok(days) => Ok(days.iter().map(|d| d.format("%Y-%m-%d").to_string()).collect()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn day(&self, date: &str) -> Result<DaySummary, Error> {
        let (frame_count, source) = match self.plan(parse_date(date)?)? {
            Plan::Screenshots(files) => (files.len(), Source::Screenshots),
            Plan::Videos(videos) if videos.is_empty() => (0, Source::Empty),
            Plan::Videos(videos) => (
                videos.iter().map(|(_, info)| info.frame_count).sum(),
                Source::Video,
            ),
        };
        Ok(DaySummary { date: date.to_owned(), frame_count, source })
    }

    pub fn frame(&self, date: &str, index: usize) -> Result<Frame, Error> {
        let day = parse_date(date)?;
        match self.plan(day)? {
            Plan::Screenshots(files) => {
                let name = files.get(index).ok_or(Error::OutOfRange {
                    day: date.to_owned(),
                    index,
                    count: files.len(),
                })?;
                Ok(Frame {
                    bytes: fs::read(self.day_dir(day).join(name))?,
                    mime: "image/png",
                })
            }
            Plan::Videos(videos) => {
                let (video, info, local) = locate(&videos, date, index)?;
                let chunk = local / CHUNK_FRAMES;
                let file = format!("{:04}.jpg", local % CHUNK_FRAMES + 1);
                let key = cache_key(video)?;
                let chunk_dir = || {
                    self.cache.get_or_fill(&key, chunk, |out| {
                        video::extract_frames(
                            &self.tools,
                            &video.path,
                            info,
                            chunk * CHUNK_FRAMES,
                            CHUNK_FRAMES,
                            out,
                        )
                        .map(|_| ())
                    })
                };
                let dir = chunk_dir()?;
                let bytes = match fs::read(dir.join(&file)) {
                    Ok(bytes) => bytes,
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                    // The chunk is whole but shorter than the container's frame
                    // count claimed, so this frame doesn't exist.
                    Err(_) if dir.is_dir() => {
                        return Err(Error::OutOfRange {
                            day: date.to_owned(),
                            index,
                            count: videos.iter().map(|(_, i)| i.frame_count).sum(),
                        })
                    }
                    // Evicted between being handed out and being read: decode
                    // it again.
                    Err(_) => fs::read(chunk_dir()?.join(&file))?,
                };
                Ok(Frame { bytes, mime: "image/jpeg" })
            }
        }
    }

    pub fn frame_time(&self, date: &str, index: usize) -> Result<Option<FrameTime>, Error> {
        let day = parse_date(date)?;
        match self.plan(day)? {
            Plan::Screenshots(files) => {
                let Some(name) = files.get(index) else { return Ok(None) };
                if let Some(number) = library::parse_screenshot_name(name) {
                    if let Some(local_time) = self.db_time(date, number)? {
                        return Ok(Some(FrameTime { local_time, exact: true }));
                    }
                }
                // Not in the database (it only goes back to late 2025): the
                // file's mtime is when it was written, which is when it was
                // captured.
                let modified = fs::metadata(self.day_dir(day).join(name))?.modified()?;
                Ok(Some(FrameTime {
                    local_time: DateTime::<Local>::from(modified).to_rfc3339(),
                    exact: true,
                }))
            }
            Plan::Videos(videos) => {
                let Ok((video, _, local)) = locate(&videos, date, index) else {
                    return Ok(None);
                };
                let estimate = video.start + Duration::seconds(local as i64);
                Ok(Some(FrameTime {
                    local_time: estimate.format("%Y-%m-%dT%H:%M:%S").to_string(),
                    exact: false,
                }))
            }
        }
    }

    fn day_dir(&self, day: NaiveDate) -> PathBuf {
        self.root.join(day.format("%Y-%m-%d").to_string())
    }

    fn plan(&self, day: NaiveDate) -> Result<Plan, Error> {
        let shots = self.list_screenshots(day)?;
        if !shots.is_empty() {
            return Ok(Plan::Screenshots(shots));
        }
        let mut videos = Vec::new();
        let mut seen = Vec::new();
        for video in library::list_videos(&self.root, day)? {
            let probe = self.probe(&video.path)?;
            let Some(info) = probe.info else { continue };
            if seen.contains(&probe.fingerprint) {
                continue;
            }
            seen.push(probe.fingerprint);
            videos.push((video, info));
        }
        Ok(Plan::Videos(videos))
    }

    fn list_screenshots(&self, day: NaiveDate) -> Result<Arc<Vec<String>>, Error> {
        let dir = self.day_dir(day);
        let modified = match fs::metadata(&dir) {
            Ok(meta) => meta.modified()?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Arc::default()),
            Err(e) => return Err(e.into()),
        };
        if let Some((stamp, files)) = self.screenshots.lock().unwrap().get(&day) {
            if *stamp == modified {
                return Ok(files.clone());
            }
        }
        let files = Arc::new(library::list_screenshots(&dir)?);
        self.screenshots.lock().unwrap().insert(day, (modified, files.clone()));
        Ok(files)
    }

    fn probe(&self, path: &Path) -> Result<Probe, Error> {
        let meta = fs::metadata(path)?;
        let (len, modified) = (meta.len(), meta.modified()?);
        if let Some(probe) = self.probes.lock().unwrap().get(path) {
            if probe.len == len && probe.modified == modified {
                return Ok(*probe);
            }
        }
        let probe = Probe {
            len,
            modified,
            fingerprint: library::fingerprint(path)?,
            info: video::probe(&self.tools, path)?,
        };
        self.probes.lock().unwrap().insert(path.to_owned(), probe);
        Ok(probe)
    }

    fn db_time(&self, date: &str, frame_number: u32) -> Result<Option<String>, Error> {
        let db = self.root.join("screenshots.db");
        if !db.is_file() {
            return Ok(None);
        }
        let conn = rusqlite::Connection::open_with_flags(
            &db,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        // Frame numbers restart every day, so the date has to be part of the
        // key. `local_time` is ISO 8601 in local time, so its first ten
        // characters are the day folder's name.
        let result = conn.query_row(
            "SELECT local_time FROM screenshots
             WHERE frame_number = ?1 AND substr(local_time, 1, 10) = ?2
             ORDER BY id DESC LIMIT 1",
            rusqlite::params![frame_number, date],
            |row| row.get::<_, String>(0),
        );
        match result {
            Ok(time) => Ok(Some(time)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

fn parse_date(date: &str) -> Result<NaiveDate, Error> {
    library::parse_day(date).ok_or_else(|| Error::NotADay(date.to_owned()))
}

/// The video holding day-wide frame `index`, and the index within it.
fn locate<'a>(
    videos: &'a [(VideoFile, VideoInfo)],
    date: &str,
    index: usize,
) -> Result<(&'a VideoFile, VideoInfo, usize), Error> {
    let mut local = index;
    for (video, info) in videos {
        if local < info.frame_count {
            return Ok((video, *info, local));
        }
        local -= info.frame_count;
    }
    Err(Error::OutOfRange {
        day: date.to_owned(),
        index,
        count: videos.iter().map(|(_, i)| i.frame_count).sum(),
    })
}

/// Cache folder name for a video: its stem plus its size, so a re-rendered
/// video with the same name doesn't reuse stale frames.
fn cache_key(video: &VideoFile) -> Result<String, Error> {
    let stem = video.path.file_stem().unwrap_or_default().to_string_lossy();
    Ok(format!("{stem}-{}", fs::metadata(&video.path)?.len()))
}
