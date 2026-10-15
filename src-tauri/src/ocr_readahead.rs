//! Reading screenshots ahead, several at once, during a boost.
//!
//! Vision reads one frame at a time, but several helper processes reading at
//! once get through more: the parallel read test on Nino's Mac (2026-10-15,
//! 2560×1440 frames read in tiles) read 1.34 frames a second with one helper
//! and 3.85 with four. So while a boost is on, the OCR worker hands the
//! screenshots it is about to read to a pool of helpers ahead of time, and
//! picks up each frame's text when it gets there.
//!
//! Which frames get read depends only on their thumbnails, never on their
//! text, so the worker knows in advance which ones it will read. It still
//! handles every frame in order exactly as it does without a pool
//! (`OcrWorker::run_pass`); a frame read ahead just has its text waiting. If a
//! guess turns out wrong (after a failure), a frame that wasn't read ahead is
//! read directly, and text read for nothing is dropped.

use crate::ocr::{OcrLine, TextRecognizer};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

type Answer = Result<Vec<OcrLine>, String>;
type Job = (PathBuf, Sender<Answer>);

/// A pool of recognizers, each on a thread of its own, reading the frames
/// handed to it.
pub struct ReadAhead {
    jobs: Sender<Job>,
    size: usize,
    /// The answer to come for each frame handed over and not yet taken.
    pending: HashMap<PathBuf, Receiver<Answer>>,
}

impl ReadAhead {
    /// A pool of `recognizers`. Dropping it lets each thread finish the frame
    /// it is reading and then end, dropping its recognizer (which ends its
    /// helper process).
    pub fn new(recognizers: Vec<Box<dyn TextRecognizer>>) -> std::io::Result<ReadAhead> {
        let size = recognizers.len();
        let (jobs, queue) = mpsc::channel::<Job>();
        let queue = Arc::new(Mutex::new(queue));
        for recognizer in recognizers {
            let queue = Arc::clone(&queue);
            std::thread::Builder::new().name("ocr-read-ahead".into()).spawn(move || loop {
                let job = queue.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).recv();
                let Ok((image, answer)) = job else {
                    break;
                };
                // Nobody may be waiting any more; that's fine.
                let _ = answer.send(recognizer.recognize(&image));
            })?;
        }
        Ok(ReadAhead { jobs, size, pending: HashMap::new() })
    }

    /// How many recognizers it reads with.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Frames handed over whose text hasn't been taken yet.
    pub fn in_flight(&self) -> usize {
        self.pending.len()
    }

    /// Start reading `image`.
    pub fn submit(&mut self, image: PathBuf) {
        let (answer, receive) = mpsc::channel();
        if self.jobs.send((image.clone(), answer)).is_ok() {
            self.pending.insert(image, receive);
        }
    }

    /// The text of `image`, waiting for it if it is still being read, or
    /// `None` if it wasn't handed over.
    pub fn take(&mut self, image: &Path) -> Option<Answer> {
        let receive = self.pending.remove(image)?;
        Some(receive.recv().unwrap_or_else(|_| Err("The read-ahead thread stopped".into())))
    }

    /// Drop every answer still to come.
    pub fn clear(&mut self) {
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    /// Answers with the image's name, after `delay`, counting the reads.
    struct Named {
        delay: Duration,
        reads: Arc<AtomicUsize>,
    }

    impl TextRecognizer for Named {
        fn recognize(&self, image: &Path) -> Result<Vec<OcrLine>, String> {
            std::thread::sleep(self.delay);
            self.reads.fetch_add(1, Ordering::Relaxed);
            let text = image.to_string_lossy().into_owned();
            if text == "bad" {
                return Err("unreadable".into());
            }
            Ok(vec![OcrLine { text, confidence: 1.0, x: 0.0, y: 0.0, width: 1.0, height: 1.0 }])
        }
    }

    fn pool(size: usize, delay: Duration) -> (ReadAhead, Arc<AtomicUsize>) {
        let reads = Arc::new(AtomicUsize::new(0));
        let recognizers = (0..size)
            .map(|_| Box::new(Named { delay, reads: Arc::clone(&reads) }) as Box<dyn TextRecognizer>)
            .collect();
        (ReadAhead::new(recognizers).unwrap(), reads)
    }

    fn text(answer: Option<Answer>) -> Result<String, String> {
        answer.expect("handed over").map(|lines| lines[0].text.clone())
    }

    #[test]
    fn each_frame_handed_over_gets_its_own_text() {
        let (mut pool, reads) = pool(3, Duration::from_millis(5));
        for name in ["a", "b", "bad", "c"] {
            pool.submit(name.into());
        }
        assert_eq!(pool.in_flight(), 4);
        assert_eq!(text(pool.take(Path::new("c"))), Ok("c".into()));
        assert_eq!(text(pool.take(Path::new("bad"))), Err("unreadable".into()));
        assert_eq!(text(pool.take(Path::new("a"))), Ok("a".into()));
        assert_eq!(text(pool.take(Path::new("b"))), Ok("b".into()));
        assert_eq!(reads.load(Ordering::Relaxed), 4);
        assert_eq!(pool.in_flight(), 0);

        // Not handed over, or already taken.
        assert!(pool.take(Path::new("a")).is_none());
    }

    #[test]
    fn frames_are_read_at_the_same_time() {
        let (mut pool, _) = pool(4, Duration::from_millis(200));
        let started = std::time::Instant::now();
        for name in ["a", "b", "c", "d"] {
            pool.submit(name.into());
        }
        for name in ["a", "b", "c", "d"] {
            assert!(text(pool.take(Path::new(name))).is_ok());
        }
        // One at a time would take 800 ms.
        assert!(started.elapsed() < Duration::from_millis(600), "{:?}", started.elapsed());
    }

    #[test]
    fn cleared_answers_are_dropped() {
        let (mut pool, _) = pool(1, Duration::from_millis(1));
        pool.submit("a".into());
        pool.clear();
        assert_eq!(pool.in_flight(), 0);
        assert!(pool.take(Path::new("a")).is_none());
    }
}
