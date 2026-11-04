//! Small preview images of video frames, for showing something at once while
//! the viewer drags the scrubber across video it hasn't decoded.
//!
//! Each video gets one pack file: a header listing which of the video's
//! frames it holds, followed by those frames as 480-wide JPEGs. A full pack
//! holds every `PREVIEW_STEP`-th frame. Before one exists, a coarse pack holds
//! only the video's keyframes, which ffmpeg can pull out in a fraction of a
//! second because nothing between them has to be decoded.
//!
//! Layout: `<dir>/<video key>.full.pack` and `<dir>/<video key>.keys.pack`,
//! where the key is the chunk cache's (stem plus size). A pack is written to a
//! `.<name>.<n>.tmp` file and renamed into place, so a pack that exists is whole.
//! Packs are disposable: they can be rebuilt from the videos at any time, so
//! they live in the cache directory with their own size cap, and the least
//! recently used go first when it is full.
//!
//! The pack format, all integers little-endian `u32`:
//! `b"TLPV"`, version (1), entry count, then per entry the frame's index in
//! the video and its JPEG's length in bytes, then the JPEGs back to back. The
//! page reads it as it is (`src/previews.ts`).

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use crate::video::{self, Tools, VideoInfo};
use crate::Error;

/// A full pack holds one frame in this many: one per second of a 15 fps
/// video, which at one capture a second is one every 15 seconds of the day.
pub const PREVIEW_STEP: usize = 15;

/// Width of a preview image, in pixels.
pub const PREVIEW_WIDTH: u32 = 480;

/// JPEG quality for previews (ffmpeg's `-q:v`, 2 best to 31 worst). They are
/// only on screen while the scrubber moves, so a middling quality is plenty.
pub const PREVIEW_QUALITY: &str = "5";

const MAGIC: &[u8; 4] = b"TLPV";
const VERSION: u32 = 1;

/// How fine-grained a pack is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PackKind {
    /// Every `PREVIEW_STEP`-th frame.
    Full,
    /// Only the keyframes: a stopgap until the full pack is built.
    Keyframes,
}

impl PackKind {
    fn suffix(self) -> &'static str {
        match self {
            PackKind::Full => "full",
            PackKind::Keyframes => "keys",
        }
    }
}

/// The file name of `key`'s pack of `kind`.
pub fn pack_name(key: &str, kind: PackKind) -> String {
    format!("{key}.{}.pack", kind.suffix())
}

pub struct PreviewStore {
    dir: PathBuf,
    cap_bytes: u64,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// Keyed by file name.
    entries: HashMap<String, Entry>,
    total_bytes: u64,
    clock: u64,
}

struct Entry {
    bytes: u64,
    last_used: u64,
}

impl PreviewStore {
    /// Open (creating if needed) the store at `dir`, picking up packs left by
    /// earlier runs, oldest first, and clearing interrupted writes.
    pub fn open(dir: PathBuf, cap_bytes: u64) -> std::io::Result<Self> {
        fs::create_dir_all(&dir)?;
        let mut found: Vec<(SystemTime, String, u64)> = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                let path = entry.path();
                let _ = if path.is_dir() {
                    fs::remove_dir_all(&path)
                } else {
                    fs::remove_file(&path)
                };
                continue;
            }
            if !name.ends_with(".pack") {
                continue;
            }
            let meta = entry.metadata()?;
            found.push((meta.modified()?, name, meta.len()));
        }
        found.sort();
        let mut state = State::default();
        for (_, name, bytes) in found {
            state.clock += 1;
            state.total_bytes += bytes;
            let last_used = state.clock;
            state.entries.insert(name, Entry { bytes, last_used });
        }
        let store = Self {
            dir,
            cap_bytes,
            state: Mutex::new(state),
        };
        store.evict(None);
        Ok(store)
    }

    /// Whether `key` has a pack of `kind`. A pack deleted behind the store's
    /// back counts as missing.
    pub fn has(&self, key: &str, kind: PackKind) -> bool {
        let name = pack_name(key, kind);
        self.state.lock().unwrap().entries.contains_key(&name) && self.dir.join(&name).is_file()
    }

    /// The pack of `key` the viewer should use: the full one if there is
    /// one, else the keyframe one.
    pub fn best(&self, key: &str) -> Option<(String, PackKind)> {
        [PackKind::Full, PackKind::Keyframes]
            .into_iter()
            .find(|&kind| self.has(key, kind))
            .map(|kind| (pack_name(key, kind), kind))
    }

    /// Whether the store is close enough to its cap that building packs for
    /// videos nobody is looking at would only evict others.
    pub fn nearly_full(&self) -> bool {
        self.state.lock().unwrap().total_bytes >= self.cap_bytes / 10 * 9
    }

    /// The bytes of the pack called `name`, counting as a use of it. `None`
    /// for a name that isn't a pack in the store.
    pub fn read(&self, name: &str) -> Result<Option<Vec<u8>>, Error> {
        if name.starts_with('.') || name.contains(['/', '\\']) || !name.ends_with(".pack") {
            return Ok(None);
        }
        let path = self.dir.join(name);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.forget(name);
                return Ok(None);
            }
            Err(e) => return Err(e.into()),
        };
        {
            let mut state = self.state.lock().unwrap();
            state.clock += 1;
            let now = state.clock;
            if let Some(entry) = state.entries.get_mut(name) {
                entry.last_used = now;
            }
        }
        // So the next launch's `open` sees it as recently used too.
        if let Ok(file) = fs::File::options().write(true).open(&path) {
            let _ = file.set_modified(SystemTime::now());
        }
        Ok(Some(bytes))
    }

    /// Store `frames` (index in the video, JPEG bytes; in index order) as
    /// `key`'s pack of `kind`. A full pack replaces the keyframe one.
    pub fn write(&self, key: &str, kind: PackKind, frames: &[(usize, Vec<u8>)]) -> Result<(), Error> {
        let name = pack_name(key, kind);
        // Unique per write: the converter and the preview builder can write
        // the same pack at once, and must not interleave in one file.
        static WRITES: AtomicU64 = AtomicU64::new(0);
        let staging = self.dir.join(format!(".{name}.{}.tmp", WRITES.fetch_add(1, Ordering::Relaxed)));
        fs::write(&staging, encode_pack(frames)?)?;
        let path = self.dir.join(&name);
        if let Err(e) = fs::rename(&staging, &path) {
            let _ = fs::remove_file(&staging);
            return Err(e.into());
        }
        let bytes = fs::metadata(&path)?.len();
        {
            let mut state = self.state.lock().unwrap();
            state.clock += 1;
            let last_used = state.clock;
            if let Some(old) = state.entries.insert(name.clone(), Entry { bytes, last_used }) {
                state.total_bytes -= old.bytes;
            }
            state.total_bytes += bytes;
        }
        if kind == PackKind::Full {
            let keys = pack_name(key, PackKind::Keyframes);
            if self.forget(&keys) {
                let _ = fs::remove_file(self.dir.join(&keys));
            }
        }
        self.evict(Some(&name));
        Ok(())
    }

    /// Read the JPEGs ffmpeg wrote into `dir` as `00001.jpg`, `00002.jpg`, …
    /// and store them as `key`'s pack of `kind`, the n-th file standing for
    /// frame `indices(n)` of the video (n from 0).
    pub fn write_from_dir(
        &self,
        key: &str,
        kind: PackKind,
        dir: &Path,
        indices: impl Fn(usize) -> Option<usize>,
    ) -> Result<usize, Error> {
        let frames = read_numbered_jpegs(dir, indices)?;
        if frames.is_empty() {
            return Err(Error::Tool(format!("no preview images in {}", dir.display())));
        }
        self.write(key, kind, &frames)?;
        Ok(frames.len())
    }

    /// Decode `video` and store its pack of `kind` under `key`. ffmpeg runs
    /// at background priority. `info` is only needed for a keyframe pack,
    /// to turn timestamps into frame indices.
    pub fn build(
        &self,
        tools: &Tools,
        key: &str,
        video: &Path,
        kind: PackKind,
        info: Option<VideoInfo>,
    ) -> Result<usize, Error> {
        let staging = self.dir.join(format!(".{key}.{}.partial", kind.suffix()));
        if staging.exists() {
            fs::remove_dir_all(&staging)?;
        }
        fs::create_dir_all(&staging)?;
        let result = (|| match kind {
            PackKind::Full => {
                extract(tools, video, &staging, false)?;
                self.write_from_dir(key, kind, &staging, |n| Some(n * PREVIEW_STEP))
            }
            PackKind::Keyframes => {
                let fps = info
                    .map(|info| info.fps)
                    .ok_or_else(|| Error::Tool("a keyframe pack needs the frame rate".into()))?;
                let log = extract(tools, video, &staging, true)?;
                let times = frame_times(&log);
                let first = times.first().copied().unwrap_or(0.0);
                let indices: Vec<usize> = times
                    .iter()
                    .map(|t| ((t - first) * fps).round().max(0.0) as usize)
                    .collect();
                self.write_from_dir(key, kind, &staging, |n| indices.get(n).copied())
            }
        })();
        let _ = fs::remove_dir_all(&staging);
        result
    }

    /// Forget `name`; true if it was known.
    fn forget(&self, name: &str) -> bool {
        let mut state = self.state.lock().unwrap();
        match state.entries.remove(name) {
            Some(entry) => {
                state.total_bytes -= entry.bytes;
                true
            }
            None => false,
        }
    }

    /// Drop least-recently-used packs until the store fits its cap, never
    /// `keep`, the pack just written.
    fn evict(&self, keep: Option<&str>) {
        let mut state = self.state.lock().unwrap();
        while state.total_bytes > self.cap_bytes {
            let oldest = state
                .entries
                .iter()
                .filter(|(name, _)| Some(name.as_str()) != keep)
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(name, _)| name.clone());
            let Some(oldest) = oldest else { break };
            let entry = state.entries.remove(&oldest).unwrap();
            state.total_bytes -= entry.bytes;
            if let Err(e) = fs::remove_file(self.dir.join(&oldest)) {
                eprintln!("Could not evict {oldest}: {e}");
            }
        }
    }

    #[cfg(test)]
    fn total_bytes(&self) -> u64 {
        self.state.lock().unwrap().total_bytes
    }
}

/// The ffmpeg video filter that scales a frame down to a preview, keeping its
/// aspect ratio, for the end of a filter chain.
pub fn preview_scale() -> String {
    format!("scale={PREVIEW_WIDTH}:-2,setsar=1")
}

/// Have ffmpeg write `video`'s previews into `out` as numbered JPEGs: every
/// `PREVIEW_STEP`-th frame, or with `keyframes_only` only the keyframes, in
/// which case it returns ffmpeg's log, whose `showinfo` lines say when each
/// was.
fn extract(tools: &Tools, video: &Path, out: &Path, keyframes_only: bool) -> Result<String, Error> {
    let mut command = Command::new(&tools.ffmpeg);
    video::idle_priority(&mut command);
    command.args(["-hide_banner", "-nostdin", "-nostats"]);
    if keyframes_only {
        // `showinfo` logs at info level; nothing else does with -nostats.
        command.args(["-v", "info", "-skip_frame", "nokey"]);
    } else {
        command.args(["-v", "error"]);
    }
    command.arg("-i").arg(video);
    let filter = if keyframes_only {
        format!("showinfo,{}", preview_scale())
    } else {
        format!("select=not(mod(n\\,{PREVIEW_STEP})),{}", preview_scale())
    };
    let mut child = command
        .args(["-map", "0:v:0", "-vf", &filter, "-fps_mode", "passthrough"])
        .args(["-q:v", PREVIEW_QUALITY])
        .arg(out.join("%05d.jpg"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Tool(format!("could not run ffmpeg: {e}")))?;
    // Read the log on its own thread so a full pipe never stalls ffmpeg.
    let stderr = child.stderr.take();
    let reader = std::thread::spawn(move || {
        let mut log = Vec::new();
        if let Some(mut pipe) = stderr {
            let _ = pipe.read_to_end(&mut log);
        }
        log
    });
    let status = wait_registered(child);
    let log = String::from_utf8_lossy(&reader.join().unwrap_or_default()).into_owned();
    let status = status.ok_or_else(|| Error::Tool(format!("ffmpeg was stopped on {}", video.display())))?;
    if !status.success() {
        let tail: String = log.lines().rev().take(5).collect::<Vec<_>>().join(" / ");
        return Err(Error::Tool(format!("ffmpeg failed on {}: {tail}", video.display())));
    }
    Ok(log)
}

/// The preview builds' ffmpegs while they run, by id, so
/// `kill_running_build` can stop them when the app quits; otherwise they
/// would outlive the app.
static RUNNING_BUILDS: Mutex<Vec<(u64, Child)>> = Mutex::new(Vec::new());

/// Kill the preview builds in progress, if any. The app calls this when it
/// quits or restarts; their packs are built again on a later launch.
pub fn kill_running_build() {
    let mut running = RUNNING_BUILDS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    for (_, mut child) in running.drain(..) {
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// Wait for `child` while it sits in `RUNNING_BUILDS`. `None` if
/// `kill_running_build` took it first.
fn wait_registered(child: Child) -> Option<ExitStatus> {
    static IDS: AtomicU64 = AtomicU64::new(0);
    let id = IDS.fetch_add(1, Ordering::Relaxed);
    RUNNING_BUILDS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push((id, child));
    loop {
        {
            let mut running = RUNNING_BUILDS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let at = running.iter().position(|(other, _)| *other == id)?;
            match running[at].1.try_wait() {
                Ok(None) => {}
                Ok(Some(status)) => {
                    running.remove(at);
                    return Some(status);
                }
                Err(_) => {
                    let (_, mut child) = running.remove(at);
                    let _ = child.kill();
                    return child.wait().ok();
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The `pts_time` of each frame in `showinfo`'s log lines, in order.
fn frame_times(log: &str) -> Vec<f64> {
    log.lines()
        .filter(|line| line.contains("Parsed_showinfo") && line.contains(" n:"))
        .filter_map(|line| {
            let rest = &line[line.find("pts_time:")? + "pts_time:".len()..];
            rest.split_whitespace().next()?.parse().ok()
        })
        .collect()
}

/// `dir`'s `00001.jpg`, `00002.jpg`, … in order, each with the frame index
/// `indices` gives its position (from 0). Stops at the first missing number
/// or the first position `indices` has no index for.
fn read_numbered_jpegs(
    dir: &Path,
    indices: impl Fn(usize) -> Option<usize>,
) -> Result<Vec<(usize, Vec<u8>)>, Error> {
    let mut frames = Vec::new();
    for n in 0.. {
        let Some(index) = indices(n) else { break };
        match fs::read(dir.join(format!("{:05}.jpg", n + 1))) {
            Ok(bytes) => frames.push((index, bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(frames)
}

fn encode_pack(frames: &[(usize, Vec<u8>)]) -> Result<Vec<u8>, Error> {
    let too_big = || Error::Tool("preview pack too large".into());
    let u32_of = |n: usize| u32::try_from(n).map_err(|_| too_big());
    let data: usize = frames.iter().map(|(_, jpeg)| jpeg.len()).sum();
    let mut out = Vec::with_capacity(12 + frames.len() * 8 + data);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&u32_of(frames.len())?.to_le_bytes());
    for (index, jpeg) in frames {
        out.extend_from_slice(&u32_of(*index)?.to_le_bytes());
        out.extend_from_slice(&u32_of(jpeg.len())?.to_le_bytes());
    }
    for (_, jpeg) in frames {
        out.extend_from_slice(jpeg);
    }
    Ok(out)
}

/// The frame indices and JPEGs in a pack, for tests.
pub fn decode_pack(bytes: &[u8]) -> Option<Vec<(usize, Vec<u8>)>> {
    let word = |at: usize| -> Option<usize> {
        Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?) as usize)
    };
    if bytes.get(..4)? != MAGIC || word(4)? != VERSION as usize {
        return None;
    }
    let count = word(8)?;
    let mut at = 12 + count * 8;
    let mut frames = Vec::with_capacity(count);
    for i in 0..count {
        let (index, len) = (word(12 + i * 8)?, word(16 + i * 8)?);
        frames.push((index, bytes.get(at..at + len)?.to_vec()));
        at += len;
    }
    Some(frames)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn frames(sizes: &[usize]) -> Vec<(usize, Vec<u8>)> {
        sizes
            .iter()
            .enumerate()
            .map(|(i, &size)| (i * PREVIEW_STEP, vec![i as u8; size]))
            .collect()
    }

    #[test]
    fn packs_round_trip() {
        let frames = frames(&[3, 0, 5]);
        assert_eq!(decode_pack(&encode_pack(&frames).unwrap()), Some(frames));
        assert_eq!(decode_pack(b"nope"), None);
    }

    #[test]
    fn a_full_pack_replaces_the_keyframe_one() {
        let tmp = TempDir::new().unwrap();
        let store = PreviewStore::open(tmp.path().to_path_buf(), 1_000_000).unwrap();
        store.write("v-1", PackKind::Keyframes, &frames(&[10])).unwrap();
        assert_eq!(store.best("v-1").unwrap().1, PackKind::Keyframes);
        store.write("v-1", PackKind::Full, &frames(&[10, 10])).unwrap();
        assert_eq!(store.best("v-1"), Some(("v-1.full.pack".into(), PackKind::Full)));
        assert!(!tmp.path().join("v-1.keys.pack").exists());
        assert_eq!(store.total_bytes(), fs::metadata(tmp.path().join("v-1.full.pack")).unwrap().len());
    }

    #[test]
    fn evicts_the_least_recently_read_over_its_cap() {
        let tmp = TempDir::new().unwrap();
        let one = encode_pack(&frames(&[100])).unwrap().len() as u64;
        let store = PreviewStore::open(tmp.path().to_path_buf(), one * 2 + one / 2).unwrap();
        store.write("a", PackKind::Full, &frames(&[100])).unwrap();
        store.write("b", PackKind::Full, &frames(&[100])).unwrap();
        store.read("a.full.pack").unwrap().unwrap();
        store.write("c", PackKind::Full, &frames(&[100])).unwrap();
        assert!(store.has("a", PackKind::Full));
        assert!(!store.has("b", PackKind::Full));
        assert!(!tmp.path().join("b.full.pack").exists());
        assert!(store.has("c", PackKind::Full));
    }

    #[test]
    fn reopening_picks_up_packs_and_clears_partial_writes() {
        let tmp = TempDir::new().unwrap();
        {
            let store = PreviewStore::open(tmp.path().to_path_buf(), u64::MAX).unwrap();
            store.write("v", PackKind::Full, &frames(&[10])).unwrap();
        }
        fs::write(tmp.path().join(".w.full.pack.tmp"), b"half").unwrap();
        fs::create_dir(tmp.path().join(".w.full.partial")).unwrap();
        let store = PreviewStore::open(tmp.path().to_path_buf(), u64::MAX).unwrap();
        assert!(store.has("v", PackKind::Full));
        assert!(!tmp.path().join(".w.full.pack.tmp").exists());
        assert!(!tmp.path().join(".w.full.partial").exists());
    }

    #[test]
    fn reads_only_pack_names() {
        let tmp = TempDir::new().unwrap();
        let store = PreviewStore::open(tmp.path().to_path_buf(), u64::MAX).unwrap();
        store.write("v", PackKind::Full, &frames(&[10])).unwrap();
        assert!(store.read("v.full.pack").unwrap().is_some());
        assert!(store.read("../v.full.pack").unwrap().is_none());
        assert!(store.read("v.full").unwrap().is_none());
        assert!(store.read("missing.full.pack").unwrap().is_none());
    }

    #[test]
    fn reads_showinfo_times() {
        let log = "[Parsed_showinfo_0 @ 0x1] config in time_base: 1/15360\n\
                   [Parsed_showinfo_0 @ 0x1] n:   0 pts:      0 pts_time:0       duration: 1024\n\
                   [Parsed_showinfo_0 @ 0x1] n:   1 pts:  65536 pts_time:4.26667 duration: 1024\n";
        assert_eq!(frame_times(log), vec![0.0, 4.26667]);
    }
}
