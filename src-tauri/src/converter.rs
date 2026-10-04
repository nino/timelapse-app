//! Background conversion of screenshot PNGs into `.mov` timelapses.
//!
//! This replaces the `all-timelapses-to-video` / `timelapse-to-video` scripts,
//! which encoded whole days at once. Here the unit of work is one clock hour of
//! screenshots, and one batch runs at a time with a cool-down between batches,
//! so the machine never encodes for long stretches. Nothing is encoded unless
//! the machine is on AC power, and an encode in progress is killed if the
//! power is unplugged.
//!
//! An hour whose video exists counts as converted. Its PNGs are deleted, as
//! the scripts did, but only once `DeleteCheck` agrees: the PNGs have to be
//! read by OCR first, and nothing marks that yet, so for now the default check
//! keeps every PNG.
//!
//! A batch is converted like this:
//! 1. Hard-link its PNGs into `.cache/.convert-<video>/` as a gapless
//!    `00001.png, 00002.png, …` sequence. Screenshot numbering has gaps (black
//!    frames are deleted), and ffmpeg's `%05d` input stops at the first gap.
//! 2. Encode that folder into `.cache/.convert-<video>/out.mov`.
//! 3. Rename the finished video into the library root, where `useVideos`
//!    lists it.
//!
//! An interrupted batch leaves only a staging folder behind, which the next
//! run clears.

use chrono::{DateTime, Local, NaiveDate, Timelike};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// Frames per second of the output video, as in `timelapse-to-video`.
const FRAMERATE: &str = "15";

/// Upper bound on frames in one batch. At one capture per second an hour is at
/// most 3600 frames; this only matters when mtimes are unreliable (a library
/// copied from elsewhere can have a whole day stamped with one time).
const MAX_FRAMES_PER_BATCH: usize = 3600;

/// How long to rest after a batch before starting the next one.
const COOL_DOWN: Duration = Duration::from_secs(10 * 60);

/// How long to wait before looking again when there is nothing to convert.
const IDLE_POLL: Duration = Duration::from_secs(5 * 60);

/// How long to wait before re-checking the power source while on battery.
const ON_BATTERY_POLL: Duration = Duration::from_secs(60);

/// How often a running encode re-checks the power source and the stop flag.
const ENCODE_POWER_CHECK: Duration = Duration::from_secs(30);

/// Prefix of the staging folders under `.cache`.
const STAGING_PREFIX: &str = ".convert-";

/// Decides whether a converted batch's PNGs may be deleted. Every part of an
/// hour has to pass before any of that hour's PNGs go.
pub type DeleteCheck = Arc<dyn Fn(&HourBatch) -> bool + Send + Sync>;

/// One clock hour of screenshots from one day folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HourBatch {
    /// Day folder name, `YYYY-MM-DD`.
    pub day: String,
    /// Local hour of the frames' modification times, 0–23.
    pub hour: u32,
    /// Which slice of the hour this is, from 0. Only non-zero when the hour
    /// has more than `MAX_FRAMES_PER_BATCH` frames.
    pub part: usize,
    /// The PNGs, in frame-number order.
    pub frames: Vec<PathBuf>,
}

impl HourBatch {
    /// File name of the video, `YYYY-MM-DD--HH-00-00.mov`, with `-2`, `-3`, …
    /// before the extension for later parts of an oversized hour. This keeps
    /// the shape of the names `timelapse-to-video` produced, so old and new
    /// videos sort together in the dropdown.
    pub fn video_name(&self) -> String {
        match self.part {
            0 => format!("{}--{:02}-00-00.mov", self.day, self.hour),
            n => format!("{}--{:02}-00-00-{}.mov", self.day, self.hour, n + 1),
        }
    }
}

/// Why `convert_batch` did not produce a video.
#[derive(Debug, PartialEq, Eq)]
pub enum ConvertError {
    /// The encode was stopped because power was unplugged or the converter
    /// was stopped. The source frames are untouched.
    Interrupted,
    /// Anything else. The source frames are untouched.
    Failed(String),
}

impl std::fmt::Display for ConvertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConvertError::Interrupted => write!(f, "conversion interrupted"),
            ConvertError::Failed(reason) => write!(f, "{}", reason),
        }
    }
}

/// Whether the machine is drawing from AC power.
///
/// On macOS this asks `pmset`; if that fails the answer is `false`, so an
/// unreadable power state never leads to encoding on battery. Other platforms
/// have no battery check and always report `true`.
pub fn on_ac_power() -> bool {
    if cfg!(target_os = "macos") {
        Command::new("pmset")
            .args(["-g", "ps"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| parse_pmset_power_source(&String::from_utf8_lossy(&output.stdout)))
            .unwrap_or(false)
    } else {
        true
    }
}

/// Parse the first line of `pmset -g ps`, e.g. `Now drawing from 'AC Power'`.
/// Returns `None` when the output does not name a power source.
fn parse_pmset_power_source(output: &str) -> Option<bool> {
    let first_line = output.lines().next()?;
    let source = first_line.split('\'').nth(1)?;
    Some(source == "AC Power")
}

/// Is `name` a day folder name (`YYYY-MM-DD`)?
fn is_day_folder_name(name: &str) -> bool {
    name.len() == 10 && NaiveDate::parse_from_str(name, "%Y-%m-%d").is_ok()
}

/// Frame number of a screenshot file name such as `00042.png`.
fn frame_number(file_name: &str) -> Option<u32> {
    file_name.strip_suffix(".png")?.parse().ok()
}

/// Find every hour that is ready to convert, oldest first.
///
/// An hour is ready once it has ended (`now` is past the end of that hour) and
/// its video does not exist yet.
pub fn find_ready_batches(root: &Path, now: DateTime<Local>) -> std::io::Result<Vec<HourBatch>> {
    Ok(find_ended_hours(root, now)?
        .into_iter()
        .flat_map(|hour| hour.parts)
        .filter(|batch| !root.join(batch.video_name()).exists())
        .collect())
}

/// Find every hour whose PNGs may be deleted, oldest first, as the batches
/// that make it up.
///
/// That is an hour that has ended, has a video for every part, and passes
/// `may_delete` for every part. In today's folder the hour holding the
/// highest-numbered frame is kept regardless: `next_filename` numbers new
/// screenshots as `max + 1`, so deleting the newest frame would restart
/// today's numbering at `00001`.
pub fn find_deletable_hours(
    root: &Path,
    now: DateTime<Local>,
    may_delete: &DeleteCheck,
) -> std::io::Result<Vec<Vec<HourBatch>>> {
    Ok(find_ended_hours(root, now)?
        .into_iter()
        .filter(|hour| !hour.holds_todays_newest_frame)
        .filter(|hour| {
            hour.parts
                .iter()
                .all(|batch| root.join(batch.video_name()).exists() && may_delete(batch))
        })
        .map(|hour| hour.parts)
        .collect())
}

/// Delete the PNGs of a converted hour, and return how many went.
pub fn delete_frames(parts: &[HourBatch]) -> usize {
    let mut deleted = 0;
    for frame in parts.iter().flat_map(|batch| &batch.frames) {
        match std::fs::remove_file(frame) {
            Ok(()) => deleted += 1,
            Err(e) => eprintln!("Could not delete converted frame {:?}: {}", frame, e),
        }
    }
    deleted
}

/// One ended clock hour of one day folder.
struct EndedHour {
    /// The hour's batches, split at `MAX_FRAMES_PER_BATCH`.
    parts: Vec<HourBatch>,
    /// Whether this hour holds the highest-numbered frame of today's folder.
    holds_todays_newest_frame: bool,
}

/// Every hour that has ended, oldest first.
fn find_ended_hours(root: &Path, now: DateTime<Local>) -> std::io::Result<Vec<EndedHour>> {
    let today = now.format("%Y-%m-%d").to_string();
    let mut hours = Vec::new();

    let mut day_names: Vec<String> = std::fs::read_dir(root)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .filter(|name| is_day_folder_name(name))
        .collect();
    day_names.sort();

    for day in day_names {
        let day_dir = root.join(&day);

        // (frame number, path, hour start) for every screenshot in the folder.
        let mut frames: Vec<(u32, PathBuf, DateTime<Local>)> = Vec::new();
        for entry in std::fs::read_dir(&day_dir)?.filter_map(|entry| entry.ok()) {
            let Some(number) = entry.file_name().to_str().and_then(frame_number) else {
                continue;
            };
            let Ok(modified) = entry.metadata().and_then(|m| m.modified()) else {
                continue;
            };
            let modified: DateTime<Local> = modified.into();
            let Some(hour_start) = modified
                .with_minute(0)
                .and_then(|t| t.with_second(0))
                .and_then(|t| t.with_nanosecond(0))
            else {
                continue;
            };
            frames.push((number, entry.path(), hour_start));
        }
        frames.sort_by_key(|(number, _, _)| *number);

        let todays_newest = if day == today {
            frames.last().map(|(number, _, _)| *number)
        } else {
            None
        };

        let mut by_hour: BTreeMap<DateTime<Local>, Vec<(u32, PathBuf)>> = BTreeMap::new();
        for (number, path, hour_start) in frames {
            by_hour.entry(hour_start).or_default().push((number, path));
        }

        for (hour_start, hour_frames) in by_hour {
            if hour_start + chrono::Duration::hours(1) > now {
                continue;
            }
            let holds_todays_newest_frame =
                todays_newest.is_some_and(|newest| hour_frames.iter().any(|(n, _)| *n == newest));
            let paths: Vec<PathBuf> = hour_frames.into_iter().map(|(_, path)| path).collect();
            let parts = paths
                .chunks(MAX_FRAMES_PER_BATCH)
                .enumerate()
                .map(|(part, chunk)| HourBatch {
                    day: day.clone(),
                    hour: hour_start.hour(),
                    part,
                    frames: chunk.to_vec(),
                })
                .collect();
            hours.push(EndedHour {
                parts,
                holds_todays_newest_frame,
            });
        }
    }

    Ok(hours)
}

/// Remove day folders that are empty, except today's, which the photographer
/// is writing into. `all-timelapses-to-video` did the same.
pub fn remove_empty_day_folders(root: &Path, now: DateTime<Local>) -> std::io::Result<usize> {
    let today = now.format("%Y-%m-%d").to_string();
    let mut removed = 0;
    for entry in std::fs::read_dir(root)?.filter_map(|entry| entry.ok()) {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !is_day_folder_name(&name) || name == today || !entry.path().is_dir() {
            continue;
        }
        if std::fs::read_dir(entry.path())?.next().is_none() {
            std::fs::remove_dir(entry.path())?;
            removed += 1;
        }
    }
    Ok(removed)
}

/// Delete staging folders left by interrupted conversions.
fn clear_stale_staging(root: &Path) -> std::io::Result<()> {
    let cache_dir = root.join(".cache");
    if !cache_dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(&cache_dir)?.filter_map(|entry| entry.ok()) {
        let is_staging = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(STAGING_PREFIX));
        if is_staging && entry.path().is_dir() {
            std::fs::remove_dir_all(entry.path())?;
        }
    }
    Ok(())
}

/// Convert one batch. `encode` turns a folder of gapless `%05d.png` frames
/// into the video at the given path; it is a parameter so tests can run the
/// publishing and cleanup steps without ffmpeg.
///
/// Returns the published video's file name. The source frames are never
/// modified.
pub fn convert_batch(
    root: &Path,
    batch: &HourBatch,
    encode: impl FnOnce(&Path, &Path) -> Result<(), ConvertError>,
) -> Result<String, ConvertError> {
    let failed = |what: &str, e: std::io::Error| ConvertError::Failed(format!("{}: {}", what, e));

    let video_name = batch.video_name();
    let stem = video_name.trim_end_matches(".mov");
    let staging = root.join(".cache").join(format!("{}{}", STAGING_PREFIX, stem));
    if staging.exists() {
        std::fs::remove_dir_all(&staging).map_err(|e| failed("Failed to clear staging folder", e))?;
    }
    std::fs::create_dir_all(&staging).map_err(|e| failed("Failed to create staging folder", e))?;

    let result = (|| {
        for (index, frame) in batch.frames.iter().enumerate() {
            let link = staging.join(format!("{:05}.png", index + 1));
            // A hard link costs nothing and the staging folder sits in the
            // same library, so it is on the same filesystem. Copy if that
            // still fails.
            if std::fs::hard_link(frame, &link).is_err() {
                std::fs::copy(frame, &link).map_err(|e| failed("Failed to stage frame", e))?;
            }
        }

        let staged_video = staging.join("out.mov");
        encode(&staging, &staged_video)?;

        let produced = std::fs::metadata(&staged_video).map(|m| m.len() > 0).unwrap_or(false);
        if !produced {
            return Err(ConvertError::Failed(format!(
                "Encoding {} produced no video",
                stem
            )));
        }

        // `rename` replaces an existing file, and a video already at this
        // name may be one nothing else can regenerate.
        let published = root.join(&video_name);
        if published.exists() {
            return Err(ConvertError::Failed(format!("{} already exists", video_name)));
        }
        std::fs::rename(&staged_video, &published)
            .map_err(|e| failed("Failed to publish video", e))?;
        Ok(video_name.clone())
    })();

    let _ = std::fs::remove_dir_all(&staging);
    result
}

/// The ffmpeg invocation from `timelapse-to-video`, reading `frames_dir` and
/// writing `output`.
fn ffmpeg_command(frames_dir: &Path, output: &Path) -> Command {
    // On macOS, run ffmpeg under the background QoS policy: the scheduler
    // throttles it and, on Apple Silicon, keeps it on the efficiency cores,
    // which is most of what keeps the machine cool while it encodes.
    let mut command = if cfg!(target_os = "macos") {
        let mut command = Command::new("taskpolicy");
        command.arg("-b").arg("ffmpeg");
        command
    } else {
        Command::new("ffmpeg")
    };
    command
        .args(["-y", "-loglevel", "error", "-framerate", FRAMERATE, "-i"])
        .arg(frames_dir.join("%05d.png"))
        .args(["-c:v", "libx265", "-crf", "28", "-preset", "veryslow", "-vf"])
        .arg(
            "scale=1800:1124:force_original_aspect_ratio=decrease,\
             pad=1800:1124:(ow-iw)/2:(oh-ih)/2:black,format=yuv420p",
        )
        .args(["-tag:v", "hvc1"])
        .arg(output);
    command
}

/// Wait for `child`, killing it if `should_stop` turns true. `should_stop` is
/// checked every `check_every`; the child's exit is checked every second.
fn wait_or_kill(
    mut child: Child,
    check_every: Duration,
    mut should_stop: impl FnMut() -> bool,
) -> Result<(), ConvertError> {
    let mut last_check = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                let mut stderr = String::new();
                if let Some(mut pipe) = child.stderr.take() {
                    use std::io::Read;
                    let _ = pipe.read_to_string(&mut stderr);
                }
                return Err(ConvertError::Failed(format!("ffmpeg exited with {}: {}", status, stderr.trim())));
            }
            Ok(None) => {}
            Err(e) => return Err(ConvertError::Failed(format!("Failed to wait for ffmpeg: {}", e))),
        }

        if last_check.elapsed() >= check_every {
            last_check = Instant::now();
            if should_stop() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ConvertError::Interrupted);
            }
        }

        std::thread::sleep(Duration::from_secs(1).min(check_every));
    }
}

/// Encode with ffmpeg, giving up if power is unplugged or `running` clears.
fn encode_with_ffmpeg(frames_dir: &Path, output: &Path, running: &AtomicBool) -> Result<(), ConvertError> {
    let child = ffmpeg_command(frames_dir, output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        // `-loglevel error` keeps stderr short enough that it cannot fill the
        // pipe while nothing reads it.
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            ConvertError::Failed(format!(
                "Failed to run ffmpeg: {}. Make sure ffmpeg is installed and in PATH.",
                e
            ))
        })?;

    wait_or_kill(child, ENCODE_POWER_CHECK, || {
        !running.load(Ordering::SeqCst) || !on_ac_power()
    })
}

/// Runs batches in the background for as long as it is started.
pub struct Converter {
    root: PathBuf,
    running: Arc<AtomicBool>,
    may_delete: DeleteCheck,
}

impl Converter {
    /// A converter that keeps every PNG. Screenshots must be OCR'd before
    /// they are deleted, and until OCR records which hours it has read there
    /// is nothing to check against, so nothing is deleted.
    pub fn new_in(root: PathBuf) -> Self {
        Self::with_delete_check(root, Arc::new(|_| false))
    }

    pub fn with_delete_check(root: PathBuf, may_delete: DeleteCheck) -> Self {
        Converter {
            root,
            running: Arc::new(AtomicBool::new(false)),
            may_delete,
        }
    }

    /// Spawn the conversion loop. Like `Photographer::start`, this calls
    /// `tokio::spawn`; the loop exits the first time it is polled after
    /// `stop()`.
    pub fn start(&self) {
        self.running.store(true, Ordering::SeqCst);
        let root = self.root.clone();
        let running = Arc::clone(&self.running);
        let may_delete = Arc::clone(&self.may_delete);

        tokio::spawn(async move {
            println!("Starting video conversion background task...");
            if let Err(e) = clear_stale_staging(&root) {
                eprintln!("Failed to clear stale conversion folders: {}", e);
            }

            while running.load(Ordering::SeqCst) {
                let wait = Self::run_once(&root, &running, &may_delete).await;
                tokio::time::sleep(wait).await;
            }

            println!("Video conversion background task stopped.");
        });
    }

    // Nothing stops the converter yet: it runs for the life of the app. This
    // is here for the same reason as `Photographer::stop`, for whatever
    // command or test needs it first.
    #[allow(dead_code)]
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Delete the PNGs of every hour that may go, convert at most one batch,
    /// and return how long to wait before the next attempt.
    async fn run_once(root: &Path, running: &Arc<AtomicBool>, may_delete: &DeleteCheck) -> Duration {
        let now = Local::now();

        // Deleting is cheap, so it does not wait for AC power.
        match find_deletable_hours(root, now, may_delete) {
            Ok(hours) => {
                for parts in hours {
                    let deleted = delete_frames(&parts);
                    println!("Deleted {} converted frames from {} {:02}:00", deleted, parts[0].day, parts[0].hour);
                }
            }
            Err(e) => eprintln!("Failed to scan for converted screenshots: {}", e),
        }
        if let Err(e) = remove_empty_day_folders(root, now) {
            eprintln!("Failed to remove empty day folders: {}", e);
        }

        if !on_ac_power() {
            return ON_BATTERY_POLL;
        }

        let batches = match find_ready_batches(root, now) {
            Ok(batches) => batches,
            Err(e) => {
                eprintln!("Failed to scan for screenshots to convert: {}", e);
                return IDLE_POLL;
            }
        };
        let Some(batch) = batches.into_iter().next() else {
            return IDLE_POLL;
        };

        println!(
            "Converting {} frames from {} {:02}:00",
            batch.frames.len(),
            batch.day,
            batch.hour
        );
        let started = SystemTime::now();
        let root_owned = root.to_path_buf();
        let running_owned = Arc::clone(running);
        let result = tokio::task::spawn_blocking(move || {
            convert_batch(&root_owned, &batch, |frames_dir, output| {
                encode_with_ffmpeg(frames_dir, output, &running_owned)
            })
        })
        .await
        .unwrap_or_else(|e| Err(ConvertError::Failed(format!("Conversion task panicked: {}", e))));

        match result {
            Ok(video_name) => {
                let took = started.elapsed().unwrap_or_default().as_secs();
                println!("Published {} after {}s", video_name, took);
                COOL_DOWN
            }
            Err(ConvertError::Interrupted) => {
                println!("Video conversion interrupted; the frames were kept");
                ON_BATTERY_POLL
            }
            Err(e) => {
                eprintln!("Video conversion failed, frames kept: {}", e);
                COOL_DOWN
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::fs::{self, File, FileTimes};
    use tempfile::TempDir;

    fn at(day: &str, hour: u32, minute: u32) -> DateTime<Local> {
        let date = NaiveDate::parse_from_str(day, "%Y-%m-%d").unwrap();
        Local
            .from_local_datetime(&date.and_hms_opt(hour, minute, 0).unwrap())
            .single()
            .unwrap()
    }

    /// Write a screenshot `<root>/<day>/<number>.png` stamped with `when`.
    fn frame(root: &Path, day: &str, number: u32, when: DateTime<Local>) -> PathBuf {
        let dir = root.join(day);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{:05}.png", number));
        fs::write(&path, format!("frame {}", number)).unwrap();
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(FileTimes::new().set_modified(SystemTime::from(when)))
            .unwrap();
        path
    }

    /// An encoder that writes a placeholder video and records how many frames
    /// it was handed.
    fn fake_encode(seen: &mut usize) -> impl FnOnce(&Path, &Path) -> Result<(), ConvertError> + '_ {
        move |frames_dir, output| {
            *seen = fs::read_dir(frames_dir).unwrap().count();
            fs::write(output, b"video").unwrap();
            Ok(())
        }
    }

    #[test]
    fn parses_pmset_power_source() {
        let ac = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=1)\t100%; charged; 0:00 remaining present: true\n";
        let battery = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=1)\t80%; discharging\n";
        let ups = "Now drawing from 'UPS Power'\n";
        assert_eq!(parse_pmset_power_source(ac), Some(true));
        assert_eq!(parse_pmset_power_source(battery), Some(false));
        assert_eq!(parse_pmset_power_source(ups), Some(false));
        assert_eq!(parse_pmset_power_source(""), None);
        assert_eq!(parse_pmset_power_source("something else"), None);
    }

    #[test]
    fn groups_frames_by_hour_and_skips_the_hour_in_progress() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        frame(root, "2026-10-01", 1, at("2026-10-01", 9, 10));
        frame(root, "2026-10-01", 2, at("2026-10-01", 9, 50));
        frame(root, "2026-10-01", 4, at("2026-10-01", 10, 5));
        frame(root, "2026-10-01", 5, at("2026-10-01", 11, 30));

        let batches = find_ready_batches(root, at("2026-10-01", 11, 45)).unwrap();

        // Hours that ended convert; 11:00 is still going.
        let summary: Vec<(u32, usize)> = batches.iter().map(|b| (b.hour, b.frames.len())).collect();
        assert_eq!(summary, vec![(9, 2), (10, 1)]);
        assert_eq!(batches[0].video_name(), "2026-10-01--09-00-00.mov");
    }

    #[test]
    fn skips_hours_that_already_have_a_video() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        frame(root, "2026-10-04", 1, at("2026-10-04", 8, 0));
        frame(root, "2026-10-04", 2, at("2026-10-04", 9, 0));
        fs::write(root.join("2026-10-04--08-00-00.mov"), b"done").unwrap();

        let batches = find_ready_batches(root, at("2026-10-04", 12, 0)).unwrap();

        let hours: Vec<u32> = batches.iter().map(|b| b.hour).collect();
        assert_eq!(hours, vec![9]);
    }

    fn always(answer: bool) -> DeleteCheck {
        Arc::new(move |_| answer)
    }

    #[test]
    fn deletes_only_converted_hours_that_pass_the_check() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let converted = frame(root, "2026-10-01", 1, at("2026-10-01", 8, 0));
        frame(root, "2026-10-01", 2, at("2026-10-01", 9, 0));
        fs::write(root.join("2026-10-01--08-00-00.mov"), b"video").unwrap();
        let now = at("2026-10-02", 0, 0);

        assert!(
            find_deletable_hours(root, now, &always(false)).unwrap().is_empty(),
            "nothing goes until the check agrees"
        );

        let hours = find_deletable_hours(root, now, &always(true)).unwrap();
        assert_eq!(hours.len(), 1, "09:00 has no video yet");
        assert_eq!(delete_frames(&hours[0]), 1);
        assert!(!converted.exists());
        assert!(
            find_ready_batches(root, now).unwrap().iter().all(|b| b.hour != 8),
            "a deleted hour is not converted again"
        );
    }

    #[test]
    fn keeps_the_hour_with_todays_newest_frame() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        frame(root, "2026-10-04", 1, at("2026-10-04", 8, 0));
        frame(root, "2026-10-04", 2, at("2026-10-04", 9, 0));
        fs::write(root.join("2026-10-04--08-00-00.mov"), b"video").unwrap();
        fs::write(root.join("2026-10-04--09-00-00.mov"), b"video").unwrap();

        // Both hours are converted, but deleting 00002.png would make the next
        // screenshot 00001.png again.
        let hours = find_deletable_hours(root, at("2026-10-04", 12, 0), &always(true)).unwrap();
        let deletable: Vec<u32> = hours.iter().map(|parts| parts[0].hour).collect();
        assert_eq!(deletable, vec![8]);

        // The same layout on a past day can go entirely.
        let hours = find_deletable_hours(root, at("2026-10-05", 12, 0), &always(true)).unwrap();
        let deletable: Vec<u32> = hours.iter().map(|parts| parts[0].hour).collect();
        assert_eq!(deletable, vec![8, 9]);
    }

    #[test]
    fn an_oversized_hour_is_deleted_only_once_every_part_is_converted() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let when = at("2026-10-01", 9, 0);
        for n in 1..=(MAX_FRAMES_PER_BATCH as u32 + 1) {
            frame(root, "2026-10-01", n, when);
        }
        fs::write(root.join("2026-10-01--09-00-00.mov"), b"part 1").unwrap();
        let now = at("2026-10-02", 0, 0);

        assert!(find_deletable_hours(root, now, &always(true)).unwrap().is_empty());

        fs::write(root.join("2026-10-01--09-00-00-2.mov"), b"part 2").unwrap();
        let hours = find_deletable_hours(root, now, &always(true)).unwrap();
        assert_eq!(hours.len(), 1);
        assert_eq!(hours[0].len(), 2);
    }

    #[test]
    fn orders_frames_by_number_and_ignores_other_files() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let when = at("2026-10-01", 9, 0);
        let f10 = frame(root, "2026-10-01", 10, when);
        let f9 = frame(root, "2026-10-01", 9, when);
        fs::write(root.join("2026-10-01").join("notes.txt"), b"x").unwrap();
        fs::create_dir_all(root.join(".cache").join("2026-10-01")).unwrap();
        fs::create_dir_all(root.join("not-a-day")).unwrap();
        fs::write(root.join("2026-09-30--10-00-00.mov"), b"video").unwrap();

        let batches = find_ready_batches(root, at("2026-10-02", 0, 0)).unwrap();

        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].frames, vec![f9, f10]);
    }

    #[test]
    fn splits_oversized_hours() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let when = at("2026-10-01", 9, 0);
        for n in 1..=(MAX_FRAMES_PER_BATCH as u32 + 5) {
            frame(root, "2026-10-01", n, when);
        }

        let batches = find_ready_batches(root, at("2026-10-02", 0, 0)).unwrap();

        let sizes: Vec<usize> = batches.iter().map(|b| b.frames.len()).collect();
        assert_eq!(sizes, vec![MAX_FRAMES_PER_BATCH, 5]);
        let names: Vec<String> = batches.iter().map(HourBatch::video_name).collect();
        assert_eq!(names, vec!["2026-10-01--09-00-00.mov", "2026-10-01--09-00-00-2.mov"]);
    }

    #[test]
    fn converting_publishes_the_video_and_keeps_the_frames() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let a = frame(root, "2026-10-01", 1, at("2026-10-01", 9, 0));
        let b = frame(root, "2026-10-01", 7, at("2026-10-01", 9, 1));
        let keep = frame(root, "2026-10-01", 8, at("2026-10-01", 10, 0));
        let batch = find_ready_batches(root, at("2026-10-02", 0, 0)).unwrap().remove(0);

        let mut seen = 0;
        let name = convert_batch(root, &batch, fake_encode(&mut seen)).unwrap();

        assert_eq!(name, "2026-10-01--09-00-00.mov");
        assert_eq!(seen, 2, "the gap between 1 and 7 should be closed up");
        assert_eq!(fs::read(root.join(&name)).unwrap(), b"video");
        assert_eq!(fs::read(&a).unwrap(), b"frame 1", "converting never deletes frames");
        assert_eq!(fs::read(&b).unwrap(), b"frame 7");
        assert!(keep.exists());
        assert!(
            find_ready_batches(root, at("2026-10-02", 0, 0)).unwrap().iter().all(|b| b.hour != 9),
            "a converted hour is not offered again"
        );
        assert!(
            fs::read_dir(root.join(".cache")).unwrap().next().is_none(),
            "the staging folder is cleaned up"
        );
    }

    #[test]
    fn failed_encode_keeps_the_frames() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let a = frame(root, "2026-10-01", 1, at("2026-10-01", 9, 0));
        let batch = find_ready_batches(root, at("2026-10-02", 0, 0)).unwrap().remove(0);

        let result = convert_batch(root, &batch, |_, _| Err(ConvertError::Interrupted));

        assert_eq!(result, Err(ConvertError::Interrupted));
        assert!(a.exists());
        assert!(!root.join("2026-10-01--09-00-00.mov").exists());
        assert!(fs::read_dir(root.join(".cache")).unwrap().next().is_none());
    }

    #[test]
    fn empty_output_is_not_published() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let a = frame(root, "2026-10-01", 1, at("2026-10-01", 9, 0));
        let batch = find_ready_batches(root, at("2026-10-02", 0, 0)).unwrap().remove(0);

        let result = convert_batch(root, &batch, |_, output| {
            fs::write(output, b"").unwrap();
            Ok(())
        });

        assert!(matches!(result, Err(ConvertError::Failed(_))));
        assert!(a.exists());
        assert!(!root.join("2026-10-01--09-00-00.mov").exists());
    }

    #[test]
    fn an_existing_video_is_never_overwritten() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        frame(root, "2026-10-01", 1, at("2026-10-01", 9, 0));
        let batch = find_ready_batches(root, at("2026-10-02", 0, 0)).unwrap().remove(0);
        // Appears while the batch is encoding.
        fs::write(root.join("2026-10-01--09-00-00.mov"), b"earlier").unwrap();

        let mut seen = 0;
        let result = convert_batch(root, &batch, fake_encode(&mut seen));

        assert!(matches!(result, Err(ConvertError::Failed(_))));
        assert_eq!(fs::read(root.join("2026-10-01--09-00-00.mov")).unwrap(), b"earlier");
    }

    #[test]
    fn removes_empty_past_day_folders_only() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        fs::create_dir_all(root.join("2026-10-01")).unwrap();
        fs::create_dir_all(root.join("2026-10-04")).unwrap();
        frame(root, "2026-10-02", 1, at("2026-10-02", 9, 0));
        fs::create_dir_all(root.join(".cache")).unwrap();

        let removed = remove_empty_day_folders(root, at("2026-10-04", 12, 0)).unwrap();

        assert_eq!(removed, 1);
        assert!(!root.join("2026-10-01").exists());
        assert!(root.join("2026-10-04").exists(), "today's folder stays");
        assert!(root.join("2026-10-02").exists(), "folders with frames stay");
        assert!(root.join(".cache").exists());
    }

    #[test]
    fn clears_stale_staging_but_not_frame_caches() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let stale = root.join(".cache").join(".convert-2026-10-01--09-00-00");
        let frames = root.join(".cache").join("2026-09-30--10-00-00");
        fs::create_dir_all(&stale).unwrap();
        fs::create_dir_all(&frames).unwrap();

        clear_stale_staging(root).unwrap();

        assert!(!stale.exists());
        assert!(frames.exists());
    }

    #[test]
    fn wait_or_kill_stops_a_running_process() {
        let child = Command::new("sleep").arg("30").spawn().unwrap();
        let started = Instant::now();

        let result = wait_or_kill(child, Duration::from_millis(50), || true);

        assert_eq!(result, Err(ConvertError::Interrupted));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn wait_or_kill_reports_failure() {
        let child = Command::new("false").stderr(Stdio::piped()).spawn().unwrap();
        let result = wait_or_kill(child, Duration::from_secs(60), || false);
        assert!(matches!(result, Err(ConvertError::Failed(_))));
    }

    /// End to end with the real ffmpeg command, when ffmpeg (with libx265) is
    /// installed. Skipped otherwise, since the rest of the suite does not need
    /// it.
    #[test]
    fn encodes_a_real_batch_with_ffmpeg() {
        let has_x265 = Command::new("ffmpeg")
            .args(["-hide_banner", "-encoders"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("libx265"))
            .unwrap_or(false);
        if !has_x265 || cfg!(target_os = "macos") && Command::new("taskpolicy").output().is_err() {
            eprintln!("skipping: ffmpeg with libx265 is not available");
            return;
        }

        let temp = TempDir::new().unwrap();
        let root = temp.path();
        let day_dir = root.join("2026-10-01");
        fs::create_dir_all(&day_dir).unwrap();
        let status = Command::new("ffmpeg")
            .args(["-loglevel", "error", "-f", "lavfi", "-i", "testsrc=size=320x200:rate=15", "-frames:v", "6"])
            .arg(day_dir.join("%05d.png"))
            .status()
            .unwrap();
        assert!(status.success());
        // Leave a gap, as a deleted black frame would.
        fs::remove_file(day_dir.join("00003.png")).unwrap();
        for n in [1, 2, 4, 5, 6] {
            let path = day_dir.join(format!("{:05}.png", n));
            File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_times(FileTimes::new().set_modified(SystemTime::from(at("2026-10-01", 9, n))))
                .unwrap();
        }
        let batch = find_ready_batches(root, at("2026-10-02", 0, 0)).unwrap().remove(0);

        let running = AtomicBool::new(true);
        let name = convert_batch(root, &batch, |dir, out| encode_with_ffmpeg(dir, out, &running)).unwrap();

        let probe = Command::new("ffprobe")
            .args(["-v", "error", "-count_frames", "-select_streams", "v:0", "-show_entries", "stream=codec_name,width,height,nb_read_frames", "-of", "csv=p=0"])
            .arg(root.join(&name))
            .output()
            .unwrap();
        let probe = String::from_utf8_lossy(&probe.stdout);
        assert_eq!(probe.trim(), "hevc,1800,1124,5", "all five frames, gap closed");
        assert_eq!(fs::read_dir(&day_dir).unwrap().count(), 5, "the PNGs are kept");
    }
}
