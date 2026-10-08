//! Background OCR over the screenshot library.
//!
//! The worker walks every day folder in frame-number order, runs text
//! recognition on each PNG whose screen has changed since the last frame it
//! read, stores the text in `screenshots.db` for full-text search, and moves
//! that day's progress mark forward (see `ScreenshotDatabase::record_ocr_frame`).
//! It only works while the machine is on AC power (or during a boost that
//! allows battery, see `boost.rs`), like the video converter, and the converter in turn only converts and deletes PNGs that the progress
//! mark covers (`ocr_check`).
//!
//! Days that only exist as video (the old script's whole-day videos, whose
//! screenshots were deleted before OCR existed) are read from the videos
//! instead, a batch at a time whenever there are no new screenshots to read,
//! newest day first. There, frames are counted by their position in the day
//! (see `ScreenshotDatabase::record_video_ocr_frame`).
//!
//! Recognition itself is Apple's Vision framework, so the worker only runs on
//! macOS; everything else here is platform-independent and tested with a fake
//! recognizer.

use crate::activity::{Activity, State};
use crate::boost::Boost;
use crate::converter::{is_day_folder_name, on_ac_power, OcrCheck, HourBatch};
use crate::database::{LineBox, OcrProgress, ScreenshotDatabase};
use frame_source::{FrameSource, RawFrame};
use image::{imageops, GrayImage};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

/// How long to wait before looking again when there is nothing to read.
const IDLE_SLEEP: Duration = Duration::from_secs(60);

/// A pass at least this long is logged on its own (`log_pass`).
const LONG_PASS: Duration = Duration::from_secs(60);

/// How long to wait before re-checking the power source while on battery.
const BATTERY_SLEEP: Duration = Duration::from_secs(5 * 60);

/// How many frames to read between power-source checks. At ~130 ms per frame
/// that is a check every few seconds of work.
const FRAMES_PER_POWER_CHECK: usize = 30;

/// Most video frames handled per pass when there are no new screenshots, so
/// screenshots that arrive meanwhile wait at most a few minutes. Decoding
/// restarts at the progress mark with each batch.
const VIDEO_FRAMES_PER_PASS: usize = 1800;

/// How much decoded video the OCR worker's frame source may cache. It decodes
/// by streaming, which caches nothing, so this only bounds the folder.
const VIDEO_CACHE_CAP_BYTES: u64 = 64 * 1024 * 1024;

/// A frame younger than this may still be being written, or may be about to
/// be deleted as all-black, so the worker leaves it for the next pass.
const MIN_FRAME_AGE: Duration = Duration::from_secs(10);

/// Frames are compared at a quarter of their 1800×1124 size.
const THUMB_WIDTH: usize = 450;
const THUMB_HEIGHT: usize = 281;

/// A thumbnail pixel counts as changed when its grey level moves by more than
/// this (out of 255).
const PIXEL_CHANGE: u8 = 24;

/// A frame is read when more than this many thumbnail pixels changed since the
/// last frame that was read. 60 pixels at quarter size is roughly a short
/// word of new text; a blinking cursor or a ticking clock stays below it.
/// Because the comparison is against the last frame *read*, not the previous
/// frame, slow changes such as typing add up until they cross the threshold.
const CHANGED_PIXELS: usize = 60;

/// One line of recognized text. The box is normalized to the image size, with
/// the origin at the bottom-left corner, as Vision reports it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OcrLine {
    pub text: String,
    pub confidence: f32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

pub trait TextRecognizer: Send {
    fn recognize(&self, image: &Path) -> Result<Vec<OcrLine>, String>;
}

/// The platform's text recognizer, if it has one.
pub fn system_recognizer() -> Option<Box<dyn TextRecognizer>> {
    #[cfg(target_os = "macos")]
    {
        Some(Box::new(vision::VisionRecognizer))
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Whether OCR has handled every one of `frame_numbers` in `day`. This is the
/// check the video converter makes before deleting an hour's PNGs; an unknown
/// day, or a database error, reads as "not yet".
pub fn ocr_covers(
    db: &ScreenshotDatabase,
    day: &str,
    frame_numbers: impl IntoIterator<Item = u32>,
) -> bool {
    match db.ocr_done_through(day) {
        Ok(Some(last_frame)) => frame_numbers.into_iter().all(|frame| frame <= last_frame),
        _ => false,
    }
}

/// The video converter's `OcrCheck`: an hour is converted, and its PNGs may
/// go, once OCR has handled every one of them. It keeps its own connection to
/// the library's database; if that cannot be opened, nothing is converted or
/// deleted.
pub fn ocr_check(root: &Path) -> OcrCheck {
    let db = match ScreenshotDatabase::new(root.join("screenshots.db")) {
        Ok(db) => Mutex::new(db),
        Err(error) => {
            eprintln!("OCR: cannot open database for the delete check: {}", error);
            crate::diagnostics::error("ocr", format!("Cannot open database for the delete check: {}", error)).record();
            return Arc::new(|_| false);
        }
    };

    Arc::new(move |batch: &HourBatch| {
        let frame_numbers = batch.frames.iter().map(|frame| frame.number);
        db.lock()
            .map(|db| ocr_covers(&db, &batch.day, frame_numbers))
            .unwrap_or(false)
    })
}

/// The number in an `NNNNN.png` path.
fn frame_number(path: &Path) -> Option<u32> {
    path.file_name()?.to_str()?.strip_suffix(".png")?.parse().ok()
}

/// A greyscale thumbnail used to decide whether the screen changed.
#[derive(Debug, Clone, PartialEq)]
pub struct Thumbnail(Vec<u8>);

impl Thumbnail {
    pub fn of(image: &Path) -> Result<Thumbnail, String> {
        let grey = image::open(image)
            .map_err(|e| format!("Failed to read {}: {}", image.display(), e))?
            .into_luma8();
        Ok(Thumbnail::of_grey(&grey))
    }

    /// The thumbnail of a decoded video frame.
    pub fn of_rgb(frame: &RawFrame) -> Result<Thumbnail, String> {
        // The same luma weights as `into_luma8`, so video frames and
        // screenshots are judged alike.
        let luma = frame
            .rgb
            .chunks_exact(3)
            .map(|p| ((2126 * p[0] as u32 + 7152 * p[1] as u32 + 722 * p[2] as u32) / 10000) as u8)
            .collect();
        let grey = GrayImage::from_raw(frame.width, frame.height, luma)
            .ok_or("frame is smaller than its size says")?;
        Ok(Thumbnail::of_grey(&grey))
    }

    fn of_grey(grey: &GrayImage) -> Thumbnail {
        // `thumbnail` averages each block of source pixels, like a box filter.
        let small = imageops::thumbnail(grey, THUMB_WIDTH as u32, THUMB_HEIGHT as u32);
        Thumbnail(small.into_raw())
    }

    pub fn differs_from(&self, other: &Thumbnail) -> bool {
        let changed = self
            .0
            .iter()
            .zip(&other.0)
            .filter(|(a, b)| a.abs_diff(**b) > PIXEL_CHANGE)
            .count();
        changed > CHANGED_PIXELS
    }
}

/// What one pass over the library did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PassSummary {
    /// Frames run through text recognition.
    pub recognized: usize,
    /// Frames skipped because the screen had not changed.
    pub skipped: usize,
}

impl PassSummary {
    fn handled(&self) -> usize {
        self.recognized + self.skipped
    }
}

/// The state a pass needs from one call to the next.
pub struct OcrWorker {
    root: PathBuf,
    db: ScreenshotDatabase,
    recognizer: Box<dyn TextRecognizer>,
    /// Thumbnail of the last frame read in each day.
    last_read: HashMap<String, Thumbnail>,
    /// Text of the last frame recorded in each day.
    last_text: HashMap<String, String>,
    activity: Arc<Activity>,
    videos: Option<VideoReader>,
}
/// What the worker needs to read days that only exist as video.
pub struct VideoReader {
    source: FrameSource,
    /// Where a frame is written as a PNG for the recognizer, which reads
    /// files.
    scratch: PathBuf,
    /// Videos ffmpeg failed on, passed over until the app restarts rather than
    /// retried every pass.
    failed: HashSet<PathBuf>,
}

impl VideoReader {
    pub fn new(source: FrameSource, scratch: PathBuf) -> Self {
        VideoReader {
            source,
            scratch,
            failed: HashSet::new(),
        }
    }
}



impl OcrWorker {
    pub fn new(root: PathBuf, db: ScreenshotDatabase, recognizer: Box<dyn TextRecognizer>) -> Self {
        OcrWorker {
            root,
            db,
            recognizer,
            last_read: Default::default(),
            last_text: Default::default(),
            activity: Arc::default(),
            videos: None,
        }
    }

    /// Also read days that only exist as video (`run_video_pass`).
    pub fn with_videos(mut self, videos: VideoReader) -> Self {
        self.videos = Some(videos);
        self
    }

    /// Report progress to `activity`, for the Activity window.
    pub fn reporting_to(mut self, activity: Arc<Activity>) -> Self {
        self.activity = activity;
        self
    }

    /// Handle up to `max_frames` frames, oldest day first, and stop early when
    /// `keep_going` says so (it is asked every `FRAMES_PER_POWER_CHECK`
    /// frames). Frames are handled strictly in order within a day, so the
    /// progress mark never skips a frame: a frame that is too new stops that
    /// day's pass.
    pub fn run_pass(
        &mut self,
        now: SystemTime,
        max_frames: usize,
        keep_going: &mut dyn FnMut() -> bool,
    ) -> std::io::Result<PassSummary> {
        let mut summary = PassSummary::default();

        // Listed up front so the Activity window can say how much is left.
        let mut pending = Vec::new();
        for day in day_folders(&self.root)? {
            let done_through = self.db.ocr_done_through(&day).unwrap_or(None).unwrap_or(0);
            let frames = frames_after(&self.root.join(&day), done_through)?;
            pending.push((day, frames));
        }
        self.activity
            .ocr_pass_started(pending.iter().map(|(_, frames)| frames.len()).sum());

        for (day, frames) in pending {
            for (frame_number, path) in frames {
                if summary.handled() >= max_frames {
                    return Ok(summary);
                }
                if summary.handled() > 0
                    && summary.handled() % FRAMES_PER_POWER_CHECK == 0
                    && !keep_going()
                {
                    return Ok(summary);
                }
                if !old_enough(&path, now) {
                    break;
                }

                let recognized = match self.handle_frame(&day, frame_number, &path) {
                    Ok(recognized) => recognized,
                    Err(error) => {
                        // A frame that cannot be read (deleted meanwhile, or
                        // corrupt) is passed over rather than retried forever,
                        // which would hold back the rest of the day.
                        eprintln!("OCR: skipping {}: {}", path.display(), error);
                        self.activity
                            .ocr_failed(format!("Skipped {} frame {}: {}", day, frame_number, error));
                        self.db
                            .record_ocr_frame(&day, frame_number, None)
                            .map_err(std::io::Error::other)?;
                        false
                    }
                };
                if recognized {
                    summary.recognized += 1;
                } else {
                    summary.skipped += 1;
                }
                self.activity.ocr_frame_handled(&day, frame_number, recognized);
            }
        }

        Ok(summary)
    }

    /// Read one frame if the screen changed. Returns whether it was read.
    fn handle_frame(&mut self, day: &str, frame_number: u32, path: &Path) -> Result<bool, String> {
        let thumbnail = Thumbnail::of(path)?;
        self.read_if_changed(day, thumbnail, &|| Ok(path.to_path_buf()), &|db, result| {
            db.record_ocr_frame(day, frame_number, result)
        })
    }

    /// Run the image `image` gives through the recognizer if `thumbnail`
    /// differs from the last frame read in `day`, and store the result with
    /// `record`. Returns whether it was read.
    fn read_if_changed(
        &mut self,
        day: &str,
        thumbnail: Thumbnail,
        image: &dyn Fn() -> Result<PathBuf, String>,
        record: &dyn Fn(&ScreenshotDatabase, Option<(&str, &[LineBox])>) -> rusqlite::Result<()>,
    ) -> Result<bool, String> {
        let changed = self
            .last_read
            .get(day)
            .map_or(true, |last| thumbnail.differs_from(last));

        if !changed {
            record(&self.db, None).map_err(|e| e.to_string())?;
            return Ok(false);
        }

        let lines = self.recognizer.recognize(&image()?)?;
        // One line of text per box, so a line's own newlines go.
        let text = lines
            .iter()
            .map(|line| line.text.replace('\n', " "))
            .collect::<Vec<_>>()
            .join("\n");

        // Pixels change without the text changing (video, images, colours).
        // A row already stands for every frame up to the next one, so the
        // same text again needs no row of its own.
        let result = if self.last_text.get(day) == Some(&text) {
            None
        } else {
            Some(lines.iter().map(|l| [l.x, l.y, l.width, l.height]).collect::<Vec<_>>())
        };
        record(&self.db, result.as_deref().map(|boxes| (text.as_str(), boxes)))
            .map_err(|e| e.to_string())?;
        self.last_read.insert(day.to_string(), thumbnail);
        self.last_text.insert(day.to_string(), text);
        Ok(true)
    }

    /// Handle up to `max_frames` frames of days that only exist as video,
    /// newest day first, resuming each day at its progress mark and stopping
    /// early when `keep_going` says so, as `run_pass` does. Days OCR has
    /// started on from screenshots are left alone. Does nothing without
    /// `with_videos`.
    pub fn run_video_pass(
        &mut self,
        max_frames: usize,
        keep_going: &mut dyn FnMut() -> bool,
    ) -> Result<PassSummary, String> {
        let Some(mut videos) = self.videos.take() else {
            return Ok(PassSummary::default());
        };
        let result = self.video_pass(&mut videos, max_frames, keep_going);
        self.videos = Some(videos);
        result
    }

    fn video_pass(
        &mut self,
        videos: &mut VideoReader,
        max_frames: usize,
        keep_going: &mut dyn FnMut() -> bool,
    ) -> Result<PassSummary, String> {
        let mut summary = PassSummary::default();

        // Listed up front, newest first, so the Activity window can say how
        // many days are left. Probing a day's videos is cached after the
        // first pass, so this is cheap from then on.
        let mut pending = Vec::new();
        let mut days = videos.source.days().map_err(|e| e.to_string())?;
        days.reverse();
        for day in days {
            let done = match self.db.ocr_progress(&day).map_err(|e| e.to_string())? {
                None => 0,
                Some(OcrProgress::VideoPosition(last)) => last as usize,
                Some(OcrProgress::FrameNumber(_)) => continue,
            };
            let Some(day_videos) = videos.source.video_only_day(&day).map_err(|e| e.to_string())?
            else {
                continue;
            };
            let unread: Vec<_> = day_videos
                .into_iter()
                .filter(|video| video.first_index + video.frame_count > done)
                .filter(|video| !videos.failed.contains(&video.path))
                .collect();
            if !unread.is_empty() {
                pending.push((day, done, unread));
            }
        }
        self.activity.ocr_video_pass_started(pending.len());

        for (day, done, day_videos) in pending {
            for video in day_videos {
                let end = video.first_index + video.frame_count;
                let mut stopped = false;
                let mut failure = None;
                let scratch = videos.scratch.as_path();
                let streamed = videos.source.stream_video(
                    &video,
                    done.saturating_sub(video.first_index),
                    &mut |index, frame| {
                        if summary.handled() >= max_frames
                            || summary.handled() > 0
                                && summary.handled() % FRAMES_PER_POWER_CHECK == 0
                                && !keep_going()
                        {
                            stopped = true;
                            return false;
                        }
                        let position = (video.first_index + index + 1) as u32;
                        let recognized =
                            match self.handle_video_frame(&day, position, &frame, scratch) {
                                Ok(recognized) => recognized,
                                Err(error) => {
                                    // As for an unreadable screenshot: pass
                                    // over it so the rest of the day is not
                                    // held back.
                                    eprintln!("OCR: skipping frame {} of {}: {}", position, day, error);
                                    self.activity.ocr_failed(format!(
                                        "Skipped {} video frame {}: {}",
                                        day, position, error
                                    ));
                                    let recorded = self.db.record_video_ocr_frame(&day, position, None);
                                    if let Err(error) = recorded {
                                        failure = Some(error.to_string());
                                        return false;
                                    }
                                    false
                                }
                            };
                        if recognized {
                            summary.recognized += 1;
                        } else {
                            summary.skipped += 1;
                        }
                        self.activity.ocr_frame_handled(&day, position, recognized);
                        true
                    },
                );
                if let Some(error) = failure {
                    return Err(error);
                }
                if stopped {
                    return Ok(summary);
                }
                match streamed {
                    // The container can claim more frames than decode, so a
                    // finished video counts as done up to its end; otherwise
                    // the mark would stop short and the tail be decoded again
                    // every pass.
                    Ok(()) => self
                        .db
                        .record_video_ocr_frame(&day, end as u32, None)
                        .map_err(|e| e.to_string())?,
                    Err(error) => {
                        eprintln!("OCR: cannot decode {}: {}", video.path.display(), error);
                        self.activity
                            .ocr_failed(format!("Cannot decode {}: {}", video.path.display(), error));
                        videos.failed.insert(video.path.clone());
                        // Later videos would move the mark past this one's
                        // unread frames.
                        break;
                    }
                }
            }
        }

        Ok(summary)
    }

    fn handle_video_frame(
        &mut self,
        day: &str,
        position: u32,
        frame: &RawFrame,
        scratch: &Path,
    ) -> Result<bool, String> {
        let thumbnail = Thumbnail::of_rgb(frame)?;
        // Written only when it is going to be read.
        let image = || {
            let path = scratch.join("video-frame.png");
            write_png(&path, frame)?;
            Ok(path)
        };
        self.read_if_changed(day, thumbnail, &image, &|db, result| {
            db.record_video_ocr_frame(day, position, result)
        })
    }
}

/// Write a decoded frame as a PNG, favouring speed over size: it is read once
/// and overwritten by the next one.
fn write_png(path: &Path, frame: &RawFrame) -> Result<(), String> {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::ImageEncoder;

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let writer = std::io::BufWriter::new(file);
    PngEncoder::new_with_quality(writer, CompressionType::Fast, FilterType::Sub)
        .write_image(frame.rgb, frame.width, frame.height, image::ExtendedColorType::Rgb8)
        .map_err(|e| e.to_string())
}

/// Start reading `root` on a background thread that runs for the life of the
/// app. Returns `false` on platforms without a text recognizer. Days that
/// only exist as video are read too when there is a `work_dir` (outside the
/// library) for decoded frames.
pub fn start_background_ocr(
    root: PathBuf,
    work_dir: Option<PathBuf>,
    activity: Arc<Activity>,
    boost: Arc<Boost>,
) -> bool {
    let Some(recognizer) = system_recognizer() else {
        activity.ocr_unavailable();
        return false;
    };

    std::thread::Builder::new()
        .name("ocr".into())
        .spawn(move || {
            set_thread_priority(false);

            // A connection of its own, so OCR never waits on the capture
            // loop's lock.
            let db = match ScreenshotDatabase::new(root.join("screenshots.db")) {
                Ok(db) => db,
                Err(error) => {
                    eprintln!("OCR: cannot open database: {}", error);
                    crate::diagnostics::error("ocr", format!("Cannot open database: {}", error)).record();
                    return;
                }
            };
            let videos = work_dir.and_then(|dir| {
                let source = FrameSource::new(
                    root.clone(),
                    dir.join("chunks"),
                    VIDEO_CACHE_CAP_BYTES,
                    frame_source::Tools::new(crate::paths::ffmpeg()),
                );
                match source {
                    Ok(source) => Some(VideoReader::new(source, dir)),
                    Err(error) => {
                        eprintln!("OCR: cannot read videos: {}", error);
                        crate::diagnostics::error("ocr", format!("Cannot read videos: {}", error)).record();
                        None
                    }
                }
            });
            let mut worker = OcrWorker::new(root, db, recognizer).reporting_to(activity);
            if let Some(videos) = videos {
                worker = worker.with_videos(videos);
            }
            run_forever(&mut worker, &boost);
        })
        .is_ok()
}

/// Run this thread at background priority, as the converter runs ffmpeg: on
/// Apple silicon that keeps it on the efficiency cores and lets the system
/// throttle it, so a long backlog does not heat the machine. During a boost
/// (`boosted`) it runs at the user-initiated class instead, so it gets the
/// performance cores.
fn set_thread_priority(boosted: bool) {
    #[cfg(target_os = "macos")]
    // SAFETY: only changes the calling thread's own scheduling class.
    unsafe {
        let class = if boosted {
            libc::qos_class_t::QOS_CLASS_USER_INITIATED
        } else {
            libc::qos_class_t::QOS_CLASS_BACKGROUND
        };
        libc::pthread_set_qos_class_self_np(class, 0);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = boosted;
}

fn run_forever(worker: &mut OcrWorker, boost: &Boost) {
    let activity = Arc::clone(&worker.activity);
    // A boost starting or stopping ends the sleep early.
    let sleep = |state: State, wait: Duration| {
        activity.ocr_sleeps(state, wait);
        boost.sleep(wait);
    };
    let mut boosted = false;
    let mut tally = crate::diagnostics::Tally::new("ocr");
    // Asked before each pass and every `FRAMES_PER_POWER_CHECK` frames, so a
    // boost's start and end reach a pass in progress within seconds.
    let mut keep_going = || {
        if boost.is_on() != boosted {
            boosted = !boosted;
            set_thread_priority(boosted);
        }
        boost.may_work(on_ac_power)
    };
    loop {
        if !keep_going() {
            if let Some(left) = boost.low_power_left() {
                sleep(State::LowPower, left);
                continue;
            }
            let wait = if boost.is_on() { crate::boost::BOOSTED_POWER_POLL } else { BATTERY_SLEEP };
            sleep(State::OnBattery, wait);
            continue;
        }

        // New screenshots first: the converter is waiting on them.
        let started = Instant::now();
        let screenshots = worker.run_pass(SystemTime::now(), usize::MAX, &mut keep_going);
        let result = match screenshots {
            Ok(summary) if summary.handled() == 0 => worker
                .run_video_pass(VIDEO_FRAMES_PER_PASS, &mut keep_going)
                .map(|summary| ("video frames", summary)),
            Ok(summary) => Ok(("screenshots", summary)),
            Err(error) => Err(error.to_string()),
        };
        let took = started.elapsed();
        match result {
            Ok((kind, summary)) if summary.handled() > 0 => {
                println!(
                    "OCR: read {} {}, skipped {} unchanged",
                    summary.recognized, kind, summary.skipped
                );
                log_pass(&mut tally, kind, &summary, took, boost.is_on());
            }
            Ok(_) => {
                tally.report_if_due();
                sleep(State::Idle, IDLE_SLEEP)
            }
            Err(error) => {
                eprintln!("OCR pass failed: {}", error);
                activity.ocr_failed(format!("Pass failed: {}", error));
                sleep(State::Idle, IDLE_SLEEP);
            }
        }
    }
}

/// Passes that keep up with new screenshots go into the hourly summary; a
/// pass long enough to be a backlog, and every pass over video, is logged on
/// its own.
fn log_pass(tally: &mut crate::diagnostics::Tally, kind: &str, summary: &PassSummary, took: Duration, boosted: bool) {
    let video = kind == "video frames";
    if video || took >= LONG_PASS {
        crate::diagnostics::info("ocr", format!("Read {}", kind))
            .took(took)
            .data(serde_json::json!({
                "recognized": summary.recognized,
                "skipped": summary.skipped,
                "boosted": boosted,
            }))
            .record();
    } else {
        tally.count("recognized", summary.recognized as u64);
        tally.count("skipped", summary.skipped as u64);
        tally.time("pass", took);
    }
    tally.report_if_due();
}

/// Day folder names under `root`, oldest first.
fn day_folders(root: &Path) -> std::io::Result<Vec<String>> {
    let mut days: Vec<String> = std::fs::read_dir(root)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| is_day_folder_name(name))
        .collect();
    days.sort();
    Ok(days)
}

/// The `NNNNN.png` frames in `day_dir` numbered above `done_through`, in order.
fn frames_after(day_dir: &Path, done_through: u32) -> std::io::Result<Vec<(u32, PathBuf)>> {
    let mut frames: Vec<(u32, PathBuf)> = std::fs::read_dir(day_dir)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| Some((frame_number(&entry.path())?, entry.path())))
        .filter(|(number, _)| *number > done_through)
        .collect();
    frames.sort();
    Ok(frames)
}

fn old_enough(path: &Path, now: SystemTime) -> bool {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map(|modified| now.duration_since(modified).unwrap_or_default() >= MIN_FRAME_AGE)
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
mod vision {
    use super::{OcrLine, TextRecognizer};
    use objc2::rc::{autoreleasepool, Retained};
    use objc2::AnyThread;
    use objc2_foundation::{NSArray, NSDictionary, NSString, NSURL};
    use objc2_vision::{
        VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel,
    };
    use std::path::Path;

    /// Vision's text recognizer in accurate mode. Fast mode was tried on real
    /// captures and garbles too much text to be worth indexing.
    pub struct VisionRecognizer;

    impl TextRecognizer for VisionRecognizer {
        fn recognize(&self, image: &Path) -> Result<Vec<OcrLine>, String> {
            let path = image.to_str().ok_or("image path is not valid UTF-8")?;

            autoreleasepool(|_| {
                let url = NSURL::fileURLWithPath(&NSString::from_str(path));
                let options = NSDictionary::new();
                // SAFETY: an empty options dictionary is valid for any key and
                // value type.
                let handler = unsafe {
                    VNImageRequestHandler::initWithURL_options(
                        VNImageRequestHandler::alloc(),
                        &url,
                        &options,
                    )
                };

                let request = VNRecognizeTextRequest::new();
                request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
                request.setUsesLanguageCorrection(true);

                let as_request: Retained<VNRequest> =
                    Retained::into_super(Retained::into_super(request.clone()));
                let requests = NSArray::from_retained_slice(&[as_request]);

                handler
                    .performRequests_error(&requests)
                    .map_err(|error| error.localizedDescription().to_string())?;

                let mut lines = Vec::new();
                for observation in request.results().iter().flat_map(|results| results.iter()) {
                    let Some(best) = observation.topCandidates(1).firstObject() else {
                        continue;
                    };
                    // SAFETY: a plain property read on a finished observation.
                    let bounds = unsafe { observation.boundingBox() };
                    lines.push(OcrLine {
                        text: best.string().to_string(),
                        confidence: best.confidence(),
                        x: bounds.origin.x,
                        y: bounds.origin.y,
                        width: bounds.size.width,
                        height: bounds.size.height,
                    });
                }
                Ok(lines)
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::converter::Frame;
    use image::{GrayImage, Luma};
    use std::cell::RefCell;
    use std::fs::{File, FileTimes};
    use std::rc::Rc;
    use tempfile::TempDir;

    const DAY_1: &str = "2024-01-01";
    const DAY_2: &str = "2024-01-02";

    /// Returns one line naming the file it was given, and remembers the call.
    struct FakeRecognizer {
        seen: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl TextRecognizer for FakeRecognizer {
        fn recognize(&self, image: &Path) -> Result<Vec<OcrLine>, String> {
            let name = image.file_name().unwrap().to_str().unwrap().to_string();
            self.seen.lock().unwrap().push(name.clone());
            Ok(vec![OcrLine {
                text: format!("text of {}", name),
                confidence: 1.0,
                x: 0.1,
                y: 0.2,
                width: 0.3,
                height: 0.04,
            }])
        }
    }

    /// Write a white image of the app's capture size with black rectangles,
    /// each `(x, y, width, height)` in full-size pixels.
    fn write_image(path: &Path, rects: &[(u32, u32, u32, u32)]) {
        let mut canvas = GrayImage::from_pixel(1800, 1124, Luma([255]));
        for &(x, y, width, height) in rects {
            for py in y..y + height {
                for px in x..x + width {
                    canvas.put_pixel(px, py, Luma([0]));
                }
            }
        }
        canvas.save(path).unwrap();
    }

    /// A white frame with a black square of side `block` at `(x, y)`, or no
    /// square for 0.
    fn write_frame(path: &Path, block: u32, x: u32, y: u32) {
        let rects: &[(u32, u32, u32, u32)] = if block > 0 { &[(x, y, block, block)] } else { &[] };
        write_image(path, rects);
    }

    fn set_age(path: &Path, age: Duration) {
        let time = SystemTime::now() - age;
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(FileTimes::new().set_modified(time).set_accessed(time))
            .unwrap();
    }

    /// Write frame `number` of `day`, a minute old unless `fresh`.
    fn frame(root: &Path, day: &str, number: u32, block: u32, fresh: bool) -> PathBuf {
        let dir = root.join(day);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{:05}.png", number));
        write_frame(&path, block, 100, 100);
        if !fresh {
            set_age(&path, Duration::from_secs(60));
        }
        path
    }

    struct Library {
        _temp_dir: TempDir,
        root: PathBuf,
        seen: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        worker: OcrWorker,
    }

    impl Library {
        fn new() -> Library {
            let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            Library::with(seen.clone(), Box::new(FakeRecognizer { seen }))
        }

        fn with(
            seen: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
            recognizer: Box<dyn TextRecognizer>,
        ) -> Library {
            let temp_dir = TempDir::new().unwrap();
            let root = temp_dir.path().to_path_buf();
            let db = ScreenshotDatabase::new(root.join("screenshots.db")).unwrap();
            let worker = OcrWorker::new(root.clone(), db, recognizer);
            Library { _temp_dir: temp_dir, root, seen, worker }
        }

        fn pass(&mut self) -> PassSummary {
            self.worker
                .run_pass(SystemTime::now(), usize::MAX, &mut || true)
                .unwrap()
        }

        fn seen(&self) -> Vec<String> {
            self.seen.lock().unwrap().clone()
        }

        fn done_through(&self, day: &str) -> Option<u32> {
            self.worker.db.ocr_done_through(day).unwrap()
        }
    }

    #[test]
    fn reports_progress_to_the_activity_window() {
        let mut library = Library::new();
        let activity = Arc::new(Activity::default());
        let db = ScreenshotDatabase::new(library.root.join("screenshots.db")).unwrap();
        library.worker = OcrWorker::new(library.root.clone(), db, Box::new(SameText))
            .reporting_to(Arc::clone(&activity));
        frame(&library.root, DAY_1, 1, 0, false);
        frame(&library.root, DAY_1, 2, 0, false);
        frame(&library.root, DAY_1, 3, 0, true);

        library.pass();

        let ocr = activity.snapshot(false, || true).ocr;
        assert_eq!((ocr.recognized, ocr.skipped), (1, 1));
        assert_eq!(ocr.current.map(|f| (f.day, f.number)), Some((DAY_1.to_string(), 2)));
        // The fresh frame waits for the next pass.
        assert_eq!(ocr.remaining, 1);
    }

    #[test]
    fn small_changes_do_not_count_but_bigger_ones_do() {
        let temp_dir = TempDir::new().unwrap();
        let path = |name: &str| temp_dir.path().join(name);

        write_frame(&path("blank.png"), 0, 0, 0);
        write_frame(&path("blank_again.png"), 0, 0, 0);
        // About the size of a text cursor: 2 px wide, 18 px tall.
        write_image(&path("cursor.png"), &[(500, 500, 2, 18)]);
        // A block of new text, roughly a word or two of 13 px type.
        write_frame(&path("word.png"), 40, 300, 300);

        let blank = Thumbnail::of(&path("blank.png")).unwrap();
        assert!(!Thumbnail::of(&path("blank_again.png")).unwrap().differs_from(&blank));
        assert!(!Thumbnail::of(&path("cursor.png")).unwrap().differs_from(&blank));
        assert!(Thumbnail::of(&path("word.png")).unwrap().differs_from(&blank));
    }

    #[test]
    fn reads_changed_frames_and_skips_unchanged_ones() {
        let mut library = Library::new();
        frame(&library.root, DAY_1, 1, 0, false);
        frame(&library.root, DAY_1, 2, 0, false);
        frame(&library.root, DAY_1, 3, 200, false);
        frame(&library.root, DAY_1, 4, 200, false);

        let summary = library.pass();

        assert_eq!(summary, PassSummary { recognized: 2, skipped: 2 });
        assert_eq!(library.seen(), vec!["00001.png", "00003.png"]);
        assert_eq!(library.done_through(DAY_1), Some(4));

        let hits = library.worker.db.search_ocr("00003", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!((hits[0].day.as_str(), hits[0].frame_number), (DAY_1, 3));

        // Nothing new, nothing done.
        assert_eq!(library.pass(), PassSummary::default());
    }

    /// Reads the same text off every frame.
    struct SameText;

    impl TextRecognizer for SameText {
        fn recognize(&self, _image: &Path) -> Result<Vec<OcrLine>, String> {
            Ok(vec![OcrLine {
                text: "a video\nplaying".into(),
                confidence: 1.0,
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            }])
        }
    }

    #[test]
    fn same_text_again_gets_no_row() {
        let mut library = Library::with(Default::default(), Box::new(SameText));
        frame(&library.root, DAY_1, 1, 0, false);
        frame(&library.root, DAY_1, 2, 200, false);
        frame(&library.root, DAY_1, 3, 0, false);
        frame(&library.root, DAY_2, 1, 0, false);

        let summary = library.pass();

        // The pixels changed each time, so every frame was read.
        assert_eq!(summary, PassSummary { recognized: 4, skipped: 0 });
        assert_eq!(library.done_through(DAY_1), Some(3));
        let db = &library.worker.db;
        // Vision's one line with a newline in it stays one line.
        assert_eq!(
            db.ocr_lines(DAY_1, 1).unwrap(),
            Some(("a video playing".to_string(), vec![[0.0, 0.0, 1.0, 1.0]]))
        );
        assert_eq!(db.ocr_lines(DAY_1, 2).unwrap(), None);
        assert_eq!(db.ocr_lines(DAY_1, 3).unwrap(), None);
        assert!(db.ocr_lines(DAY_2, 1).unwrap().is_some());
    }

    #[test]
    fn stops_at_a_frame_that_may_still_be_written() {
        let mut library = Library::new();
        frame(&library.root, DAY_1, 1, 0, false);
        frame(&library.root, DAY_1, 2, 200, true);
        frame(&library.root, DAY_1, 3, 0, false);

        library.pass();
        assert_eq!(library.done_through(DAY_1), Some(1));

        set_age(&library.root.join(DAY_1).join("00002.png"), Duration::from_secs(60));
        library.pass();
        assert_eq!(library.done_through(DAY_1), Some(3));
        assert_eq!(library.seen(), vec!["00001.png", "00002.png", "00003.png"]);
    }

    #[test]
    fn walks_days_in_order_and_ignores_other_files() {
        let mut library = Library::new();
        frame(&library.root, DAY_2, 1, 0, false);
        frame(&library.root, DAY_1, 1, 0, false);
        // Gaps in the numbering, as all-black frames leave.
        frame(&library.root, DAY_1, 5, 200, false);
        std::fs::write(library.root.join(DAY_1).join("notes.txt"), "x").unwrap();
        std::fs::create_dir_all(library.root.join(".cache").join("00001.png")).unwrap();
        std::fs::create_dir_all(library.root.join("not-a-day")).unwrap();

        library.pass();

        // Each day starts with no previous frame to compare against, so its
        // first frame is always read.
        assert_eq!(library.seen(), vec!["00001.png", "00005.png", "00001.png"]);
        assert_eq!(library.done_through(DAY_1), Some(5));
        assert_eq!(library.done_through(DAY_2), Some(1));
    }

    #[test]
    fn passes_over_an_unreadable_frame() {
        let mut library = Library::new();
        frame(&library.root, DAY_1, 1, 0, false);
        let broken = library.root.join(DAY_1).join("00002.png");
        std::fs::write(&broken, "not a png").unwrap();
        set_age(&broken, Duration::from_secs(60));
        frame(&library.root, DAY_1, 3, 200, false);

        let summary = library.pass();

        assert_eq!(summary, PassSummary { recognized: 2, skipped: 1 });
        assert_eq!(library.done_through(DAY_1), Some(3));
    }

    #[test]
    fn resumes_from_the_progress_mark() {
        let mut library = Library::new();
        frame(&library.root, DAY_1, 1, 0, false);
        frame(&library.root, DAY_1, 2, 200, false);
        library.worker.db.record_ocr_frame(DAY_1, 1, None).unwrap();

        library.pass();

        assert_eq!(library.seen(), vec!["00002.png"]);
    }

    #[test]
    fn stops_when_told_to() {
        let mut library = Library::new();
        for number in 1..=FRAMES_PER_POWER_CHECK as u32 + 5 {
            frame(&library.root, DAY_1, number, 0, false);
        }

        let asked = Rc::new(RefCell::new(0));
        let asked_clone = Rc::clone(&asked);
        let summary = library
            .worker
            .run_pass(SystemTime::now(), usize::MAX, &mut || {
                *asked_clone.borrow_mut() += 1;
                false
            })
            .unwrap();

        assert_eq!(*asked.borrow(), 1);
        assert_eq!(summary.handled(), FRAMES_PER_POWER_CHECK);
        assert_eq!(library.done_through(DAY_1), Some(FRAMES_PER_POWER_CHECK as u32));
    }

    #[test]
    fn ocr_covers_only_frames_up_to_the_mark() {
        let temp_dir = TempDir::new().unwrap();
        let db = ScreenshotDatabase::new(temp_dir.path().join("screenshots.db")).unwrap();

        assert!(!ocr_covers(&db, DAY_1, [1, 2]));

        db.record_ocr_frame(DAY_1, 10, None).unwrap();
        assert!(ocr_covers(&db, DAY_1, [3, 7, 10]));
        assert!(!ocr_covers(&db, DAY_1, [9, 10, 11]));
        assert!(!ocr_covers(&db, DAY_2, [1]));
    }

    #[test]
    fn ocr_check_waits_for_ocr() {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();
        let check = ocr_check(root);
        let batch = |numbers: &[u32]| HourBatch {
            day: DAY_1.to_string(),
            hour: 9,
            start: chrono::Local::now().naive_local(),
            part: 0,
            frames: numbers
                .iter()
                .map(|&number| Frame {
                    number,
                    path: root.join(DAY_1).join(format!("{:05}.png", number)),
                    modified: chrono::Local::now(),
                })
                .collect(),
        };

        assert!(!check(&batch(&[1, 2])));

        // OCR's own connection records progress; the check sees it.
        let db = ScreenshotDatabase::new(root.join("screenshots.db")).unwrap();
        db.record_ocr_frame(DAY_1, 2, None).unwrap();

        assert!(check(&batch(&[1, 2])));
        assert!(!check(&batch(&[2, 3])));
    }

    /// Encode `blocks` (see `write_frame`) as a 15 fps video at `path`, or
    /// return false when there is no ffmpeg to do it.
    fn write_video(path: &Path, blocks: &[u32]) -> bool {
        let frames = TempDir::new().unwrap();
        for (i, &block) in blocks.iter().enumerate() {
            write_frame(&frames.path().join(format!("{:05}.png", i + 1)), block, 100, 100);
        }
        std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-framerate", "15", "-i"])
            .arg(frames.path().join("%05d.png"))
            .args(["-c:v", "libx264", "-qp", "0", "-pix_fmt", "yuv420p"])
            .arg(path)
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    impl Library {
        fn with_videos(mut self) -> Library {
            let source = FrameSource::new(
                self.root.clone(),
                self.root.join(".ocr").join("chunks"),
                VIDEO_CACHE_CAP_BYTES,
                frame_source::Tools::new("ffmpeg"),
            )
            .unwrap();
            let scratch = self.root.join(".ocr");
            self.worker = self.worker.with_videos(VideoReader::new(source, scratch));
            self
        }

        fn video_pass(&mut self, max_frames: usize) -> PassSummary {
            self.worker.run_video_pass(max_frames, &mut || true).unwrap()
        }

        fn progress(&self, day: &str) -> Option<OcrProgress> {
            self.worker.db.ocr_progress(day).unwrap()
        }
    }

    #[test]
    fn reads_days_that_only_exist_as_video() {
        let mut library = Library::new().with_videos();
        // Day 1 as two of the old script's videos, day 2 as one.
        if !write_video(&library.root.join("2024-01-01--18-00-00.mov"), &[0, 0, 200])
            || !write_video(&library.root.join("2024-01-01--23-00-00.mov"), &[200, 0])
            || !write_video(&library.root.join("2024-01-02--20-00-00.mov"), &[0, 300])
        {
            eprintln!("skipping: ffmpeg with libx264 is not available");
            return;
        }

        let summary = library.video_pass(usize::MAX);

        // Newest day first; within a day, only frames that changed.
        assert_eq!(summary, PassSummary { recognized: 5, skipped: 2 });
        assert_eq!(library.seen().len(), 5);
        assert_eq!(library.progress(DAY_1), Some(OcrProgress::VideoPosition(5)));
        assert_eq!(library.progress(DAY_2), Some(OcrProgress::VideoPosition(2)));
        let mut hits: Vec<_> = library
            .worker
            .db
            .search_ocr("video", 10)
            .unwrap()
            .into_iter()
            .map(|hit| (hit.day, hit.frame_number, hit.from_video))
            .collect();
        hits.sort();
        // The fake reads the same text off every video frame, so each day
        // keeps only its first row.
        assert_eq!(
            hits,
            vec![(DAY_1.to_string(), 1, true), (DAY_2.to_string(), 1, true)]
        );

        // Done is done.
        assert_eq!(library.video_pass(usize::MAX), PassSummary::default());
    }

    #[test]
    fn video_passes_stop_and_resume() {
        let mut library = Library::new().with_videos();
        if !write_video(&library.root.join("2024-01-01--18-00-00.mov"), &[0, 200, 0, 200, 0]) {
            eprintln!("skipping: ffmpeg with libx264 is not available");
            return;
        }

        assert_eq!(library.video_pass(2).handled(), 2);
        assert_eq!(library.progress(DAY_1), Some(OcrProgress::VideoPosition(2)));

        assert_eq!(library.video_pass(usize::MAX).handled(), 3);
        assert_eq!(library.progress(DAY_1), Some(OcrProgress::VideoPosition(5)));
        // Every frame differs from the one before it, so all are read.
        assert_eq!(library.seen().len(), 5);
    }

    #[test]
    fn leaves_days_with_screenshots_to_the_screenshot_pass() {
        let mut library = Library::new().with_videos();
        // An hour converted to video, with OCR already done from its PNGs.
        if !write_video(&library.root.join("2024-01-01--09-00-00--hourly.mov"), &[0, 200]) {
            eprintln!("skipping: ffmpeg with libx264 is not available");
            return;
        }
        library.worker.db.record_ocr_frame(DAY_1, 2, None).unwrap();
        // And a day still in screenshots.
        frame(&library.root, DAY_2, 1, 0, false);

        assert_eq!(library.video_pass(usize::MAX), PassSummary::default());
        assert_eq!(library.progress(DAY_1), Some(OcrProgress::FrameNumber(2)));
        assert_eq!(library.progress(DAY_2), None);
    }

    /// Runs the real Vision recognizer on a capture named by `OCR_TEST_IMAGE`:
    /// `OCR_TEST_IMAGE=~/Timelapse/2024-01-01/00001.png cargo test vision -- --ignored --nocapture`
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn vision_reads_a_real_capture() {
        let image = std::env::var("OCR_TEST_IMAGE").expect("set OCR_TEST_IMAGE");
        let started = std::time::Instant::now();
        let lines = system_recognizer().unwrap().recognize(Path::new(&image)).unwrap();
        println!("{} lines in {:?}", lines.len(), started.elapsed());
        for line in lines.iter().take(20) {
            println!("{:.2} {}", line.confidence, line.text);
        }
        assert!(!lines.is_empty());
    }
    /// Runs a real video pass over a library named by `OCR_TEST_LIBRARY`,
    /// reading at most `OCR_TEST_FRAMES` frames (default 900) of its newest
    /// video-only day. The library is only read; the database and decoded
    /// frames go to a temporary folder.
    /// `OCR_TEST_LIBRARY=~/Timelapse cargo test vision_reads_real_videos -- --ignored --nocapture`
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn vision_reads_real_videos() {
        let library = std::env::var("OCR_TEST_LIBRARY").expect("set OCR_TEST_LIBRARY");
        let library = PathBuf::from(library);
        let frames = std::env::var("OCR_TEST_FRAMES").map_or(900, |n| n.parse().unwrap());
        let work = TempDir::new().unwrap();
        let db = ScreenshotDatabase::new(work.path().join("screenshots.db")).unwrap();
        let source = FrameSource::new(
            library.clone(),
            work.path().join("chunks"),
            VIDEO_CACHE_CAP_BYTES,
            frame_source::Tools::new("ffmpeg"),
        )
        .unwrap();
        let mut worker = OcrWorker::new(library, db, system_recognizer().unwrap())
            .with_videos(VideoReader::new(source, work.path().to_path_buf()));

        let started = std::time::Instant::now();
        let summary = worker.run_video_pass(frames, &mut || true).unwrap();
        let elapsed = started.elapsed();
        println!(
            "{:?} in {:?}: {:.1} frames/s",
            summary,
            elapsed,
            summary.handled() as f64 / elapsed.as_secs_f64()
        );

        let (day, frame, text): (String, u32, String) =
            rusqlite::Connection::open(work.path().join("screenshots.db"))
                .unwrap()
                .query_row(
                    "SELECT day, frame_number, text FROM ocr_frames
                     ORDER BY length(text) DESC LIMIT 1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
        println!("Longest text, frame {} of {}:", frame, day);
        for line in text.lines().take(15) {
            println!("  {}", line);
        }
        assert!(summary.recognized > 0);
    }
}
