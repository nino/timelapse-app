//! A size-capped, least-recently-used cache of decoded video chunks on disk.
//!
//! Layout: `<dir>/<video key>/<chunk:06>/0001.jpg …`. A chunk is decoded into
//! a `.<chunk>.partial` folder and renamed into place only once complete, so a
//! chunk folder that exists is always whole.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::Error;

pub struct ChunkCache {
    dir: PathBuf,
    cap_bytes: u64,
    state: Mutex<State>,
    /// One lock per chunk, so two requests for frames in the same chunk run
    /// ffmpeg once rather than racing to fill the same folder.
    fill_locks: Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>,
}

#[derive(Default)]
struct State {
    entries: HashMap<PathBuf, Entry>,
    total_bytes: u64,
    clock: u64,
}

struct Entry {
    bytes: u64,
    last_used: u64,
}

impl ChunkCache {
    /// Open (creating if needed) the cache at `dir`, picking up chunks left by
    /// earlier runs oldest-first and clearing interrupted ones.
    pub fn open(dir: PathBuf, cap_bytes: u64) -> std::io::Result<Self> {
        fs::create_dir_all(&dir)?;
        let mut found: Vec<(std::time::SystemTime, PathBuf, u64)> = Vec::new();
        for video in fs::read_dir(&dir)? {
            let video = video?;
            if !video.file_type()?.is_dir() {
                continue;
            }
            for chunk in fs::read_dir(video.path())? {
                let chunk = chunk?;
                let path = chunk.path();
                if chunk.file_name().to_string_lossy().starts_with('.') {
                    let _ = fs::remove_dir_all(&path);
                    continue;
                }
                let modified = chunk.metadata()?.modified()?;
                found.push((modified, path.clone(), dir_size(&path)?));
            }
        }
        found.sort();

        let mut state = State::default();
        for (_, path, bytes) in found {
            state.clock += 1;
            state.total_bytes += bytes;
            state.entries.insert(
                path,
                Entry {
                    bytes,
                    last_used: state.clock,
                },
            );
        }
        let cache = Self {
            dir,
            cap_bytes,
            state: Mutex::new(state),
            fill_locks: Mutex::new(HashMap::new()),
        };
        cache.evict(None);
        Ok(cache)
    }

    /// The folder holding chunk `chunk` of the video `key`, running `fill` on
    /// a staging folder first if the chunk isn't cached yet.
    pub fn get_or_fill(
        &self,
        key: &str,
        chunk: usize,
        fill: impl FnOnce(&Path) -> Result<(), Error>,
    ) -> Result<PathBuf, Error> {
        let chunk_dir = self.dir.join(key).join(format!("{chunk:06}"));
        let lock = self
            .fill_locks
            .lock()
            .unwrap()
            .entry(chunk_dir.clone())
            .or_default()
            .clone();
        let _guard = lock.lock().unwrap();

        if self.touch(&chunk_dir) {
            return Ok(chunk_dir);
        }

        let staging = self.dir.join(key).join(format!(".{chunk:06}.partial"));
        if staging.exists() {
            fs::remove_dir_all(&staging)?;
        }
        if let Err(e) = fill(&staging) {
            let _ = fs::remove_dir_all(&staging);
            return Err(e);
        }
        fs::rename(&staging, &chunk_dir)?;

        let bytes = dir_size(&chunk_dir)?;
        {
            let mut state = self.state.lock().unwrap();
            state.clock += 1;
            let last_used = state.clock;
            state.total_bytes += bytes;
            state
                .entries
                .insert(chunk_dir.clone(), Entry { bytes, last_used });
        }
        self.evict(Some(&chunk_dir));
        Ok(chunk_dir)
    }

    #[cfg(test)]
    pub fn total_bytes(&self) -> u64 {
        self.state.lock().unwrap().total_bytes
    }

    /// Mark a cached chunk as just used; false if it isn't cached.
    fn touch(&self, chunk_dir: &Path) -> bool {
        let mut state = self.state.lock().unwrap();
        state.clock += 1;
        let now = state.clock;
        match state.entries.get_mut(chunk_dir) {
            Some(entry) if chunk_dir.is_dir() => {
                entry.last_used = now;
                true
            }
            Some(_) => {
                // Deleted behind our back; forget it and decode again.
                let entry = state.entries.remove(chunk_dir).unwrap();
                state.total_bytes -= entry.bytes;
                false
            }
            None => false,
        }
    }

    /// Drop least-recently-used chunks until the cache fits its cap. `keep` is
    /// the chunk about to be read, which must survive even if it alone is
    /// larger than the cap.
    fn evict(&self, keep: Option<&Path>) {
        let mut state = self.state.lock().unwrap();
        while state.total_bytes > self.cap_bytes {
            let oldest = state
                .entries
                .iter()
                .filter(|(path, _)| Some(path.as_path()) != keep)
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(path, _)| path.clone());
            let Some(oldest) = oldest else { break };
            let entry = state.entries.remove(&oldest).unwrap();
            state.total_bytes -= entry.bytes;
            if let Err(e) = fs::remove_dir_all(&oldest) {
                eprintln!("Could not evict {}: {e}", oldest.display());
            }
        }
    }
}

fn dir_size(dir: &Path) -> std::io::Result<u64> {
    let mut total = 0;
    for entry in fs::read_dir(dir)? {
        total += entry?.metadata()?.len();
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fill_with(bytes: usize) -> impl FnOnce(&Path) -> Result<(), Error> {
        move |dir: &Path| {
            fs::create_dir_all(dir)?;
            fs::write(dir.join("0001.jpg"), vec![0u8; bytes])?;
            Ok(())
        }
    }

    #[test]
    fn fills_once_then_hits() {
        let tmp = TempDir::new().unwrap();
        let cache = ChunkCache::open(tmp.path().to_path_buf(), 1_000).unwrap();

        let dir = cache.get_or_fill("v", 0, fill_with(10)).unwrap();
        assert!(dir.join("0001.jpg").is_file());

        let again = cache
            .get_or_fill("v", 0, |_| {
                panic!("a cached chunk must not be decoded again")
            })
            .unwrap();
        assert_eq!(dir, again);
        assert_eq!(cache.total_bytes(), 10);
    }

    #[test]
    fn evicts_least_recently_used_over_cap() {
        let tmp = TempDir::new().unwrap();
        let cache = ChunkCache::open(tmp.path().to_path_buf(), 25).unwrap();

        let a = cache.get_or_fill("v", 0, fill_with(10)).unwrap();
        let b = cache.get_or_fill("v", 1, fill_with(10)).unwrap();
        // Use `a` again so `b` becomes the oldest.
        cache.get_or_fill("v", 0, |_| unreachable!()).unwrap();
        let c = cache.get_or_fill("v", 2, fill_with(10)).unwrap();

        assert!(a.is_dir());
        assert!(
            !b.exists(),
            "the least recently used chunk should be evicted"
        );
        assert!(c.is_dir());
        assert_eq!(cache.total_bytes(), 20);
    }

    #[test]
    fn keeps_an_oversized_chunk_it_was_asked_for() {
        let tmp = TempDir::new().unwrap();
        let cache = ChunkCache::open(tmp.path().to_path_buf(), 5).unwrap();
        let dir = cache.get_or_fill("v", 0, fill_with(10)).unwrap();
        assert!(dir.join("0001.jpg").is_file());
    }

    #[test]
    fn failed_fill_leaves_nothing_behind() {
        let tmp = TempDir::new().unwrap();
        let cache = ChunkCache::open(tmp.path().to_path_buf(), 1_000).unwrap();
        let result = cache.get_or_fill("v", 0, |dir| {
            fs::create_dir_all(dir)?;
            fs::write(dir.join("0001.jpg"), b"half")?;
            Err(Error::Tool("ffmpeg died".into()))
        });
        assert!(result.is_err());
        assert!(!tmp.path().join("v/000000").exists());
        assert!(!tmp.path().join("v/.000000.partial").exists());
        assert_eq!(cache.total_bytes(), 0);
    }

    #[test]
    fn reopening_picks_up_chunks_and_clears_partials() {
        let tmp = TempDir::new().unwrap();
        {
            let cache = ChunkCache::open(tmp.path().to_path_buf(), 1_000).unwrap();
            cache.get_or_fill("v", 0, fill_with(10)).unwrap();
        }
        fs::create_dir_all(tmp.path().join("v/.000001.partial")).unwrap();

        let cache = ChunkCache::open(tmp.path().to_path_buf(), 1_000).unwrap();
        assert_eq!(cache.total_bytes(), 10);
        assert!(!tmp.path().join("v/.000001.partial").exists());
        cache.get_or_fill("v", 0, |_| unreachable!()).unwrap();
    }
}
