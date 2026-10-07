//! Background OCR over the screenshot library.
//!
//! The worker walks every day folder in frame-number order, runs text
//! recognition on each PNG whose screen has changed since the last frame it
//! read, stores the text in `screenshots.db` for full-text search, and moves
//! that day's progress mark forward (see `ScreenshotDatabase::record_ocr_frame`).
//! It only works while the machine is on AC power, like the video converter,
//! and the converter in turn only converts and deletes PNGs that the progress
//! mark covers (`ocr_check`).
//!
//! Recognition itself is Apple's Vision framework, so the worker only runs on
//! macOS; everything else here is platform-independent and tested with a fake
//! recognizer.

use crate::activity::{Activity, State};
use crate::converter::{is_day_folder_name, on_ac_power, OcrCheck, HourBatch};
use crate::database::ScreenshotDatabase;
use image::imageops;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

/// How long to wait before looking again when there is nothing to read.
const IDLE_SLEEP: Duration = Duration::from_secs(60);

/// How long to wait before re-checking the power source while on battery.
const BATTERY_SLEEP: Duration = Duration::from_secs(5 * 60);

/// How many frames to read between power-source checks. At ~130 ms per frame
/// that is a check every few seconds of work.
const FRAMES_PER_POWER_CHECK: usize = 30;

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
        // `thumbnail` averages each block of source pixels, like a box filter.
        let small = imageops::thumbnail(&grey, THUMB_WIDTH as u32, THUMB_HEIGHT as u32);
        Ok(Thumbnail(small.into_raw()))
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
    /// Thumbnail of the last frame read in each day folder.
    last_read: std::collections::HashMap<String, Thumbnail>,
    /// Text of the last frame recorded in each day folder.
    last_text: std::collections::HashMap<String, String>,
    activity: Arc<Activity>,
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
        }
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
        let changed = self
            .last_read
            .get(day)
            .map_or(true, |last| thumbnail.differs_from(last));

        if !changed {
            self.db
                .record_ocr_frame(day, frame_number, None)
                .map_err(|e| e.to_string())?;
            return Ok(false);
        }

        let lines = self.recognizer.recognize(path)?;
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
        self.db
            .record_ocr_frame(day, frame_number, result.as_deref().map(|boxes| (text.as_str(), boxes)))
            .map_err(|e| e.to_string())?;
        self.last_read.insert(day.to_string(), thumbnail);
        self.last_text.insert(day.to_string(), text);
        Ok(true)
    }
}

/// Start reading `root` on a background thread that runs for the life of the
/// app. Returns `false` on platforms without a text recognizer.
pub fn start_background_ocr(root: PathBuf, activity: Arc<Activity>) -> bool {
    let Some(recognizer) = system_recognizer() else {
        activity.ocr_unavailable();
        return false;
    };

    std::thread::Builder::new()
        .name("ocr".into())
        .spawn(move || {
            lower_thread_priority();

            // A connection of its own, so OCR never waits on the capture
            // loop's lock.
            let db = match ScreenshotDatabase::new(root.join("screenshots.db")) {
                Ok(db) => db,
                Err(error) => {
                    eprintln!("OCR: cannot open database: {}", error);
                    return;
                }
            };
            let mut worker = OcrWorker::new(root, db, recognizer).reporting_to(activity);
            run_forever(&mut worker);
        })
        .is_ok()
}

/// Run this thread at background priority, as the converter runs ffmpeg: on
/// Apple silicon that keeps it on the efficiency cores and lets the system
/// throttle it, so a long backlog does not heat the machine.
fn lower_thread_priority() {
    #[cfg(target_os = "macos")]
    // SAFETY: only changes the calling thread's own scheduling class.
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_BACKGROUND, 0);
    }
}

fn run_forever(worker: &mut OcrWorker) {
    let activity = Arc::clone(&worker.activity);
    let sleep = |state: State, wait: Duration| {
        activity.ocr_sleeps(state, wait);
        std::thread::sleep(wait);
    };
    loop {
        if !on_ac_power() {
            sleep(State::OnBattery, BATTERY_SLEEP);
            continue;
        }

        match worker.run_pass(SystemTime::now(), usize::MAX, &mut on_ac_power) {
            Ok(summary) if summary.handled() > 0 => println!(
                "OCR: read {} frames, skipped {} unchanged",
                summary.recognized, summary.skipped
            ),
            Ok(_) => sleep(State::Idle, IDLE_SLEEP),
            Err(error) => {
                eprintln!("OCR pass failed: {}", error);
                activity.ocr_failed(format!("Pass failed: {}", error));
                sleep(State::Idle, IDLE_SLEEP);
            }
        }
    }
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
}
