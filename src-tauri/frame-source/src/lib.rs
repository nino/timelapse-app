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

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime};
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

/// Day-wide frame indices `start..end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FrameRange {
    pub start: usize,
    pub end: usize,
}

/// What `FrameSource::pending` found, with the frame count it found it
/// against, so callers can scale the ranges even if the day has changed since
/// they last asked for its summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingFrames {
    pub frame_count: usize,
    pub ranges: Vec<FrameRange>,
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
    /// Every frame number and capture time the database has for a day, in
    /// frame-number order. Only asked for days served from legacy videos,
    /// which never change.
    day_times: Mutex<HashMap<NaiveDate, Arc<Vec<(u32, String)>>>>,
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

    /// The stretches of `date` whose frames would have to be decoded from
    /// video before they can be shown, in order and merged where they touch.
    /// Screenshots are always ready.
    pub fn pending(&self, date: &str) -> Result<PendingFrames, Error> {
        let segments = self.plan(parse_date(date)?)?;
        let mut pending: Vec<FrameRange> = Vec::new();
        let mut offset = 0;
        for segment in &segments {
            if let Segment::Video { file, info } = segment {
                let key = cache_key(file)?;
                for chunk in 0..info.frame_count.div_ceil(CHUNK_FRAMES) {
                    if self.cache.contains(&key, chunk) {
                        continue;
                    }
                    let start = offset + chunk * CHUNK_FRAMES;
                    let end = offset + ((chunk + 1) * CHUNK_FRAMES).min(info.frame_count);
                    match pending.last_mut() {
                        Some(last) if last.end == start => last.end = end,
                        _ => pending.push(FrameRange { start, end }),
                    }
                }
            }
            offset += segment.len();
        }
        Ok(PendingFrames {
            frame_count: offset,
            ranges: pending,
        })
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
                    local_time: times[index].1.clone(),
                    exact: true,
                }))
            }
            Segment::Video { file, .. } => {
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

    /// Where frames recorded by number (as OCR records them) sit in the day:
    /// for each of `numbers`, the day-wide index of screenshot `NNNNN`, or
    /// `None` if the day no longer has that frame.
    ///
    /// A screenshot still on disk is found exactly. Once its hour has been
    /// converted, the hourly video holds the hour's screenshots in frame-number
    /// order, so the frame's rank among the database rows for that hour is its
    /// place in the video. That is an estimate: a row and its file can fall in
    /// different hours by a fraction of a second.
    ///
    /// A day served only from legacy videos holds every screenshot of the day
    /// in frame-number order, so there a frame's rank among the day's
    /// database rows is its index, but only when the counts agree, the same
    /// rule `frame_time` uses for those days.
    pub fn indices_of_frames(
        &self,
        date: &str,
        numbers: &[u32],
    ) -> Result<Vec<Option<usize>>, Error> {
        let day = parse_date(date)?;
        let segments = self.plan(day)?;
        let total: usize = segments.iter().map(Segment::len).sum();
        let legacy_only = segments.iter().all(
            |s| matches!(s, Segment::Video { file, .. } if file.kind == VideoKind::Legacy),
        );
        if legacy_only {
            let rows = self.day_times(day)?;
            if rows.len() != total {
                return Ok(vec![None; numbers.len()]);
            }
            let mut index_of = HashMap::new();
            for (index, (number, _)) in rows.iter().enumerate() {
                index_of.entry(*number).or_insert(index);
            }
            return Ok(numbers.iter().map(|n| index_of.get(n).copied()).collect());
        }

        let mut by_number = HashMap::new();
        // Clock hour -> (first index, length) of each part of its video.
        let mut video_hours: HashMap<NaiveDateTime, Vec<(usize, usize)>> = HashMap::new();
        let mut start = 0;
        for segment in &segments {
            match segment {
                Segment::Screenshots { shots, range } => {
                    for (i, shot) in shots[range.clone()].iter().enumerate() {
                        by_number.insert(shot.number, start + i);
                    }
                }
                Segment::Video { file, .. } => {
                    video_hours
                        .entry(file.hour())
                        .or_default()
                        .push((start, segment.len()));
                }
            }
            start += segment.len();
        }

        let needs_db =
            !video_hours.is_empty() && numbers.iter().any(|n| !by_number.contains_key(n));
        let hours = if needs_db {
            self.frame_hours(date)?
        } else {
            FrameHours::default()
        };

        Ok(numbers
            .iter()
            .map(|n| {
                if let Some(&index) = by_number.get(n) {
                    return Some(index);
                }
                let hour = hours.hour_of.get(n)?;
                let mut rank = hours.numbers[hour].binary_search(n).ok()?;
                let parts = video_hours.get(hour)?;
                for &(first, len) in parts {
                    if rank < len {
                        return Some(first + rank);
                    }
                    rank -= len;
                }
                // More rows than the video has frames: the end of the hour is
                // the closest place there is.
                parts.last().and_then(|&(first, len)| (first + len).checked_sub(1))
            })
            .collect())
    }

    /// The day's database rows grouped by the local clock hour they were
    /// captured in.
    fn frame_hours(&self, date: &str) -> Result<FrameHours, Error> {
        let mut hours = FrameHours::default();
        let Some(conn) = self.open_db()? else {
            return Ok(hours);
        };
        // Keyed on the day folder the frame was written to, like the OCR rows
        // being looked up: a capture just before midnight can be stamped
        // with the next day's time.
        let mut statement = conn.prepare(
            "SELECT frame_number, local_time FROM screenshots
             WHERE day = ?1
             ORDER BY frame_number",
        )?;
        let rows = statement.query_map([date], |row| {
            Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (number, time) = row?;
            let Some(hour) = local_hour(&time) else {
                continue;
            };
            // A number listed twice (an old library's reused black frame)
            // counts once, in the hour it was first captured.
            if let std::collections::hash_map::Entry::Vacant(entry) = hours.hour_of.entry(number) {
                entry.insert(hour);
                hours.numbers.entry(hour).or_default().push(number);
            }
        }
        Ok(hours)
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

    fn day_times(&self, day: NaiveDate) -> Result<Arc<Vec<(u32, String)>>, Error> {
        if let Some(times) = self.day_times.lock().unwrap().get(&day) {
            return Ok(times.clone());
        }
        let Some(conn) = self.open_db()? else {
            return Ok(Arc::default());
        };
        let mut statement = conn.prepare(
            "SELECT frame_number, local_time FROM screenshots
             WHERE substr(local_time, 1, 10) = ?1
             ORDER BY frame_number, id",
        )?;
        let times = statement
            .query_map([day.format("%Y-%m-%d").to_string()], |row| {
                Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let times = Arc::new(times);
        self.day_times.lock().unwrap().insert(day, times.clone());
        Ok(times)
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

/// Frame numbers of one day by capture hour; see `FrameSource::frame_hours`.
#[derive(Default)]
struct FrameHours {
    hour_of: HashMap<u32, NaiveDateTime>,
    /// Ascending within each hour.
    numbers: HashMap<NaiveDateTime, Vec<u32>>,
}

/// The local clock hour of a database timestamp: RFC 3339 with an offset, as
/// the capture loop writes it, or a naive local time.
fn local_hour(time: &str) -> Option<NaiveDateTime> {
    let local = DateTime::parse_from_rfc3339(time)
        .map(|t| t.naive_local())
        .or_else(|_| NaiveDateTime::parse_from_str(time, "%Y-%m-%dT%H:%M:%S%.f"))
        .ok()?;
    Some(library::truncate_to_hour(local))
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
