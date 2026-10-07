//! Decodes the video chunks a viewer is about to reach, one at a time, on a
//! background thread, so moving into the next chunk finds it already cached.
//!
//! Only the latest request's chunks are wanted: a new request replaces the
//! queue, so dragging across a day doesn't leave a backlog of stretches the
//! viewer has already passed. The chunk being decoded when a request arrives
//! is finished, not abandoned; a viewer who reaches it meanwhile waits on the
//! cache's fill lock for it rather than decoding it a second time.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use crate::cache::ChunkCache;
use crate::video::{self, Priority, Tools, VideoInfo};
use crate::CHUNK_FRAMES;

/// Called with the day once read-ahead has decoded one of its chunks.
pub type OnDecoded = Box<dyn Fn(&str) + Send + Sync>;

pub struct Job {
    pub date: String,
    pub key: String,
    pub chunk: usize,
    pub video: PathBuf,
    pub info: VideoInfo,
}

pub struct ReadAhead {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
}

#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
    closed: bool,
}

impl ReadAhead {
    pub fn start(cache: Arc<ChunkCache>, tools: Tools, on_decoded: OnDecoded) -> Self {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue::default()),
            wake: Condvar::new(),
        });
        let worker = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("frame read-ahead".into())
                .spawn(move || run(&shared, &cache, &tools, &on_decoded))
                .expect("could not start the read-ahead thread")
        };
        Self {
            shared,
            worker: Some(worker),
        }
    }

    /// Decode `jobs`, in order, instead of whatever was still queued.
    pub fn replace(&self, jobs: Vec<Job>) {
        self.shared.queue.lock().unwrap().jobs = jobs.into();
        self.shared.wake.notify_one();
    }
}

impl Drop for ReadAhead {
    fn drop(&mut self) {
        self.shared.queue.lock().unwrap().closed = true;
        self.shared.wake.notify_one();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run(shared: &Shared, cache: &ChunkCache, tools: &Tools, on_decoded: &OnDecoded) {
    loop {
        let job = {
            let mut queue = shared.queue.lock().unwrap();
            loop {
                if queue.closed {
                    return;
                }
                if let Some(job) = queue.jobs.pop_front() {
                    break job;
                }
                queue = shared.wake.wait(queue).unwrap();
            }
        };
        if cache.contains(&job.key, job.chunk) {
            continue;
        }
        let mut decoded = false;
        let result = cache.get_or_fill(&job.key, job.chunk, |out| {
            decoded = true;
            video::extract_frames(
                tools,
                &job.video,
                job.info,
                job.chunk * CHUNK_FRAMES,
                CHUNK_FRAMES,
                out,
                Priority::Background,
            )
            .map(|_| ())
        });
        match result {
            Ok(_) if decoded => on_decoded(&job.date),
            Ok(_) => {} // the viewer got there first
            Err(e) => eprintln!("Could not read ahead in {}: {e}", job.video.display()),
        }
    }
}
