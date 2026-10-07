//! Answers "frame N of day D" for a timelapse library.
//!
//! Callers never learn where a frame comes from. A day is assembled hour by
//! hour: an hour that still has screenshots (`<root>/YYYY-MM-DD/NNNNN.png`) is
//! served from them, and an hour whose screenshots the converter has deleted
//! is served from its hourly video (`YYYY-MM-DD--HH-MM-SS--hourly[-N].mov`).
//! A day with neither falls back to the old script's whole-day videos
//! (`YYYY-MM-DD--HH-MM-SS.mov|mp4`), played back to back. Video frames are
//! decoded a chunk at a time into a size-capped LRU cache, so scrubbing
//! through a long day only decodes the stretches actually looked at.
//!
//! Screenshots win when both exist because they are the originals, and the
//! only source for the hour still being captured.

mod cache;
mod library;
mod video;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use chrono::{Duration, NaiveDate};
use serde::Serialize;

use cache::ChunkCache;
use library::{Shot, VideoFile, VideoKind};
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
    OutOfRange {
        day: String,
        index: usize,
        count: usize,
    },
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
    /// Some hours from screenshots, some from converted video.
    Mixed,
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
    /// timestamp; a naive local time when estimated from a video's name.
    pub local_time: String,
    /// False when estimated: an older hourly video's frame lined up with the
    /// database's rows for that hour, or failing that, the video's start
    /// time plus one second per frame.
    pub exact: bool,
}

#[derive(Clone, Copy)]
struct Probe {
    len: u64,
    modified: SystemTime,
    fingerprint: (u64, u64),
    /// None for files ffmpeg can't read.
    info: Option<VideoInfo>,
}

/// One run of a day's frames from a single source.
enum Segment {
    /// `shots[range]`, all from one clock hour.
    Screenshots {
        shots: Arc<Vec<Shot>>,
        range: std::ops::Range<usize>,
    },
    Video {
        file: VideoFile,
        info: VideoInfo,
    },
}

impl Segment {
    fn len(&self) -> usize {
        match self {
            Segment::Screenshots { range, .. } => range.len(),
            Segment::Video { info, .. } => info.frame_count,
        }
    }
}

pub struct FrameSource {
    root: PathBuf,
    tools: Tools,
    cache: ChunkCache,
    /// Per-video facts keyed by path, valid while size and mtime match.
    probes: Mutex<HashMap<PathBuf, Probe>>,
    /// Screenshot listings keyed by day, valid while the folder's mtime
    /// matches. Today's folder changes every second; older ones never do.
    screenshots: Mutex<HashMap<NaiveDate, (SystemTime, Arc<Vec<Shot>>)>>,
    /// Every capture time the database has for a day, in frame-number order.
    /// Only asked for days served from legacy videos, which never change.
    day_times: Mutex<HashMap<NaiveDate, Arc<Vec<String>>>>,
    /// Per hourly video, the capture times the converter recorded for its
    /// frames. See `recorded_times`.
    recorded_times: Mutex<HashMap<PathBuf, Arc<Vec<String>>>>,
    /// The capture times the database has around an hourly video's hour, in
    /// frame-number order. Only asked for videos the converter recorded no
    /// frames for; the hour has ended, so its rows no longer change.
    hour_times: Mutex<HashMap<PathBuf, Arc<Vec<String>>>>,
}

impl FrameSource {
    /// `cache_dir` should be outside the library: it is disposable, and the
    /// library may be synced. `tools` says which ffmpeg to run; the app passes
    /// its bundled one.
    pub fn new(
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
            day_times: Mutex::new(HashMap::new()),
            recorded_times: Mutex::new(HashMap::new()),
            hour_times: Mutex::new(HashMap::new()),
        })
    }

    /// Every day with screenshots or videos, oldest first, as `YYYY-MM-DD`.
    /// Cheap: it reads directory names only.
    pub fn days(&self) -> Result<Vec<String>, Error> {
        match library::list_days(&self.root) {
            Ok(days) => Ok(days
                .iter()
                .map(|d| d.format("%Y-%m-%d").to_string())
                .collect()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn day(&self, date: &str) -> Result<DaySummary, Error> {
        let segments = self.plan(parse_date(date)?)?;
        let has = |video: bool| {
            segments
                .iter()
                .any(|s| matches!(s, Segment::Video { .. }) == video)
        };
        let source = match (has(false), has(true)) {
            (false, false) => Source::Empty,
            (true, false) => Source::Screenshots,
            (false, true) => Source::Video,
            (true, true) => Source::Mixed,
        };
        Ok(DaySummary {
            date: date.to_owned(),
            frame_count: segments.iter().map(Segment::len).sum(),
            source,
        })
    }

    pub fn frame(&self, date: &str, index: usize) -> Result<Frame, Error> {
        let day = parse_date(date)?;
        let segments = self.plan(day)?;
        let (segment, local) = locate(&segments, date, index)?;
        match segment {
            Segment::Screenshots { shots, range } => Ok(Frame {
                bytes: fs::read(self.day_dir(day).join(&shots[range.start + local].name))?,
                mime: "image/png",
            }),
            Segment::Video { file, info } => {
                let chunk = local / CHUNK_FRAMES;
                let name = format!("{:04}.jpg", local % CHUNK_FRAMES + 1);
                let key = cache_key(file)?;
                let chunk_dir = || {
                    self.cache.get_or_fill(&key, chunk, |out| {
                        video::extract_frames(
                            &self.tools,
                            &file.path,
                            *info,
                            chunk * CHUNK_FRAMES,
                            CHUNK_FRAMES,
                            out,
                        )
                        .map(|_| ())
                    })
                };
                let dir = chunk_dir()?;
                let bytes = match fs::read(dir.join(&name)) {
                    Ok(bytes) => bytes,
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                    // The chunk is whole but shorter than the container's frame
                    // count claimed, so this frame doesn't exist.
                    Err(_) if dir.is_dir() => {
                        return Err(Error::OutOfRange {
                            day: date.to_owned(),
                            index,
                            count: segments.iter().map(Segment::len).sum(),
                        })
                    }
                    // Evicted between being handed out and being read: decode
                    // it again.
                    Err(_) => fs::read(chunk_dir()?.join(&name))?,
                };
                Ok(Frame {
                    bytes,
                    mime: "image/jpeg",
                })
            }
        }
    }

    pub fn frame_time(&self, date: &str, index: usize) -> Result<Option<FrameTime>, Error> {
        let day = parse_date(date)?;
        let segments = self.plan(day)?;
        let Ok((segment, local)) = locate(&segments, date, index) else {
            return Ok(None);
        };
        match segment {
            Segment::Screenshots { shots, range } => {
                let shot = &shots[range.start + local];
                if let Some(local_time) = self.db_time(date, shot.number)? {
                    return Ok(Some(FrameTime {
                        local_time,
                        exact: true,
                    }));
                }
                // Not in the database (it only goes back to late 2025): the
                // file's mtime is when it was written, which is when it was
                // captured.
                Ok(Some(FrameTime {
                    local_time: shot.modified.format("%Y-%m-%dT%H:%M:%S").to_string(),
                    exact: true,
                }))
            }
            Segment::Video { file, .. } if file.kind == VideoKind::Legacy => {
                // The old script named its videos after the capture day and the
                // time *it ran*, so the name says nothing about when frames
                // were taken. It did encode every screenshot of the day in
                // frame-number order, so when the database has exactly as many
                // rows for the day as the videos have frames, row N is frame N.
                let times = self.day_times(day)?;
                let total: usize = segments.iter().map(Segment::len).sum();
                Ok((times.len() == total).then(|| FrameTime {
                    local_time: times[index].clone(),
                    exact: true,
                }))
            }
            Segment::Video { file, info } => {
                // The converter records which screenshot each frame of an
                // hourly video was made from, and when it was taken. A record
                // that doesn't match the video frame for frame is not used.
                let recorded = self.recorded_times(file);
                if recorded.len() == info.frame_count {
                    return Ok(Some(FrameTime {
                        local_time: recorded[local].clone(),
                        exact: true,
                    }));
                }
                // Videos converted before it did: the video holds the hour's
                // screenshots in frame-number order, and so does the
                // database. The video is named after its first frame's file
                // time, and a row is written just after its file, more than
                // a second after the row before; so the first frame's row is
                // the first one not before that second. Rows of screenshots
                // deleted before the conversion would shift the frames after
                // them, so this is still an estimate.
                let rows = self.hour_times(file);
                let start = file.start.format("%Y-%m-%dT%H:%M:%S").to_string();
                let first = rows.partition_point(|time| time.get(..19).is_some_and(|t| t < start.as_str()));
                if let Some(time) = rows.get(first + local) {
                    return Ok(Some(FrameTime {
                        local_time: time.clone(),
                        exact: false,
                    }));
                }
                // An hourly video is named after its first frame. One capture
                // per second is the norm, but black frames and pauses leave
                // gaps a video can't record.
                let estimate = file.start + Duration::seconds(local as i64);
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

    /// The day's frames as an ordered list of segments. See the module docs
    /// for which source wins.
    fn plan(&self, day: NaiveDate) -> Result<Vec<Segment>, Error> {
        let shots = self.list_screenshots(day)?;

        let mut videos = Vec::new();
        let mut seen = Vec::new();
        for file in library::list_videos(&self.root, day)? {
            let probe = self.probe(&file.path)?;
            let Some(info) = probe.info else { continue };
            if seen.contains(&probe.fingerprint) {
                continue;
            }
            seen.push(probe.fingerprint);
            videos.push((file, info));
        }

        // (hour, start, segment): sorted by hour, then by when it starts, so
        // the parts of a split hour stay in order.
        let mut timeline = Vec::new();
        let mut start = 0;
        while start < shots.len() {
            let hour = shots[start].hour();
            let mut end = start + 1;
            while end < shots.len() && shots[end].hour() == hour {
                end += 1;
            }
            timeline.push((
                hour,
                shots[start].modified,
                Segment::Screenshots {
                    shots: shots.clone(),
                    range: start..end,
                },
            ));
            start = end;
        }
        let hours_with_shots: Vec<_> = timeline.iter().map(|(hour, _, _)| *hour).collect();
        let mut legacy = Vec::new();
        for (file, info) in videos {
            match file.kind {
                VideoKind::Hourly { .. } if !hours_with_shots.contains(&file.hour()) => {
                    timeline.push((file.hour(), file.start, Segment::Video { file, info }));
                }
                VideoKind::Hourly { .. } => {} // its screenshots are still here
                VideoKind::Legacy => legacy.push(Segment::Video { file, info }),
            }
        }
        if timeline.is_empty() {
            // Only the old script's videos: they are already in start order.
            return Ok(legacy);
        }
        timeline.sort_by_key(|(hour, start, _)| (*hour, *start));
        Ok(timeline
            .into_iter()
            .map(|(_, _, segment)| segment)
            .collect())
    }

    fn list_screenshots(&self, day: NaiveDate) -> Result<Arc<Vec<Shot>>, Error> {
        let dir = self.day_dir(day);
        let modified = match fs::metadata(&dir) {
            Ok(meta) => meta.modified()?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Arc::default()),
            Err(e) => return Err(e.into()),
        };
        if let Some((stamp, shots)) = self.screenshots.lock().unwrap().get(&day) {
            if *stamp == modified {
                return Ok(shots.clone());
            }
        }
        let shots = Arc::new(library::list_screenshots(&dir)?);
        self.screenshots
            .lock()
            .unwrap()
            .insert(day, (modified, shots.clone()));
        Ok(shots)
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

    fn day_times(&self, day: NaiveDate) -> Result<Arc<Vec<String>>, Error> {
        if let Some(times) = self.day_times.lock().unwrap().get(&day) {
            return Ok(times.clone());
        }
        let Some(conn) = self.open_db()? else {
            return Ok(Arc::default());
        };
        let mut statement = conn.prepare(
            "SELECT local_time FROM screenshots
             WHERE substr(local_time, 1, 10) = ?1
             ORDER BY frame_number, id",
        )?;
        let times = statement
            .query_map([day.format("%Y-%m-%d").to_string()], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let times = Arc::new(times);
        self.day_times.lock().unwrap().insert(day, times.clone());
        Ok(times)
    }

    /// The capture times of the screenshots in `video`'s clock hour, in
    /// frame-number order. The row of a screenshot is written after its file,
    /// so the hour's last rows can fall in the first minute of the next hour
    /// (or day). Empty when the database can't say.
    fn hour_times(&self, video: &VideoFile) -> Arc<Vec<String>> {
        if let Some(times) = self.hour_times.lock().unwrap().get(&video.path) {
            return times.clone();
        }
        let format = |time: chrono::NaiveDateTime| time.format("%Y-%m-%dT%H:%M:%S").to_string();
        let hour = video.hour();
        let query = |conn: &rusqlite::Connection| -> rusqlite::Result<Vec<String>> {
            // `local_time` is ISO 8601, so comparing it as text against the
            // hour's bounds compares times.
            conn.prepare(
                "SELECT local_time FROM screenshots
                 WHERE local_time >= ?1 AND local_time < ?2
                 ORDER BY frame_number, id",
            )?
            .query_map(
                rusqlite::params![format(hour), format(hour + Duration::minutes(61))],
                |row| row.get::<_, String>(0),
            )?
            .collect()
        };
        let Ok(Some(Ok(times))) = self.open_db().map(|conn| conn.map(|conn| query(&conn))) else {
            return Arc::default();
        };
        let times = Arc::new(times);
        self.hour_times
            .lock()
            .unwrap()
            .insert(video.path.clone(), times.clone());
        times
    }

    /// The capture time of each frame of `video`, as the converter recorded
    /// it. Empty for a video converted before it recorded frames, for a
    /// database from before it did, and when the database can't be read.
    fn recorded_times(&self, video: &VideoFile) -> Arc<Vec<String>> {
        if let Some(times) = self.recorded_times.lock().unwrap().get(&video.path) {
            return times.clone();
        }
        let Some(name) = video.path.file_name().and_then(|name| name.to_str()) else {
            return Arc::default();
        };
        let query = |conn: &rusqlite::Connection| -> rusqlite::Result<Vec<String>> {
            let has_table: bool = conn.query_row(
                "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'video_frames')",
                [],
                |row| row.get(0),
            )?;
            if !has_table {
                return Ok(Vec::new());
            }
            conn.prepare("SELECT local_time FROM video_frames WHERE video = ?1 ORDER BY frame_index")?
                .query_map([name], |row| row.get::<_, String>(0))?
                .collect()
        };
        let Ok(Some(Ok(times))) = self.open_db().map(|conn| conn.map(|conn| query(&conn))) else {
            return Arc::default();
        };
        // The record is written before the video is published, or, for an
        // older video, before its screenshots are deleted, which is the
        // first time the video is served. Either way it is final by now.
        let times = Arc::new(times);
        self.recorded_times
            .lock()
            .unwrap()
            .insert(video.path.clone(), times.clone());
        times
    }

    fn open_db(&self) -> Result<Option<rusqlite::Connection>, Error> {
        let db = self.root.join("screenshots.db");
        if !db.is_file() {
            return Ok(None);
        }
        Ok(Some(rusqlite::Connection::open_with_flags(
            &db,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?))
    }

    fn db_time(&self, date: &str, frame_number: u32) -> Result<Option<String>, Error> {
        let Some(conn) = self.open_db()? else {
            return Ok(None);
        };
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

/// The segment holding day-wide frame `index`, and the index within it.
fn locate<'a>(
    segments: &'a [Segment],
    date: &str,
    index: usize,
) -> Result<(&'a Segment, usize), Error> {
    let mut local = index;
    for segment in segments {
        if local < segment.len() {
            return Ok((segment, local));
        }
        local -= segment.len();
    }
    Err(Error::OutOfRange {
        day: date.to_owned(),
        index,
        count: segments.iter().map(Segment::len).sum(),
    })
}

/// Cache folder name for a video: its stem plus its size, so a re-rendered
/// video with the same name doesn't reuse stale frames.
fn cache_key(video: &VideoFile) -> Result<String, Error> {
    let stem = video.path.file_stem().unwrap_or_default().to_string_lossy();
    Ok(format!("{stem}-{}", fs::metadata(&video.path)?.len()))
}
