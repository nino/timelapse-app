//! Text recognition in a process of its own.
//!
//! Vision can break for the rest of a process's life: every request then fails
//! with `CRImageReaderError error 1` (an E5RT error in the system log) while a
//! new process reads the same frame fine. It happened on Nino's Mac 40 minutes
//! and 1 h 42 min after launch, and nothing done inside the process (new
//! handlers and requests, `usesCPUOnly`, `setComputeDevice`) brings it back. So
//! the app runs Vision in a child process, its own executable started with
//! `HELPER_ARG`, and replaces that process when a request fails.
//!
//! A new helper does not always work at once: right after a failure the
//! system can spend minutes recompiling Vision's models, and the next
//! helper's first request can fail the same way while one a minute later
//! works. That is why the OCR worker retries a frame later instead of
//! passing over it (`State::Retrying` in `ocr.rs`).
//!
//! A helper can also hang: it stops answering without exiting, and before
//! answers had a time limit that stopped OCR until the app was restarted. So
//! an answer that takes longer than `ANSWER_TIMEOUT` (`FIRST_ANSWER_TIMEOUT`
//! for a new helper, whose first request can wait minutes on the model
//! compiling) counts as a failure, and the helper is killed and replaced.
//! Answers slower than `SLOW_ANSWER` are logged, to see how close to the
//! limits real ones come.
//!
//! The protocol is one JSON line each way per frame: a `Request` on the
//! helper's stdin, a `Response` on its stdout. The helper exits when its stdin
//! closes, so it never outlives the app.

// Only macOS has a recognizer to run in a helper.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use crate::ocr::{OcrLine, TextRecognizer};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The argument that starts the app's executable as the recognizer helper.
pub const HELPER_ARG: &str = "--ocr-helper";

/// How long a helper that has answered before gets to answer. An ordinary
/// frame takes well under a second and a big screen's, read in tiles, a few
/// seconds, even at background priority.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(2 * 60);

/// How long a new helper gets for its first answer. Right after a failure the
/// system can recompile Vision's models first, which took 7 minutes once.
const FIRST_ANSWER_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// An answer that takes longer than this is logged.
const SLOW_ANSWER: Duration = Duration::from_secs(30);

#[derive(Debug, Serialize, Deserialize)]
struct Request {
    image: PathBuf,
    /// Whether the OCR worker is boosted, so the helper runs at the same
    /// priority as the thread waiting on it.
    #[serde(default)]
    boosted: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Response {
    Lines(Vec<OcrLine>),
    Error(String),
}

/// The helper's side: answer each request on `input` with `recognizer`, until
/// `input` closes.
pub fn serve(
    recognizer: &dyn TextRecognizer,
    input: impl BufRead,
    mut output: impl Write,
    set_priority: impl Fn(bool),
) -> std::io::Result<()> {
    for line in input.lines() {
        let line = line?;
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(request) => {
                set_priority(request.boosted);
                match recognizer.recognize(&request.image) {
                    Ok(lines) => Response::Lines(lines),
                    Err(error) => Response::Error(error),
                }
            }
            Err(error) => Response::Error(format!("Bad request: {}", error)),
        };
        serde_json::to_writer(&mut output, &response)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
    Ok(())
}

/// How often a helper checks that the app that started it is still running.
const PARENT_CHECK: Duration = Duration::from_secs(10);

/// Exit the helper once the app that started it has gone. The helper also
/// exits when its stdin closes, but only between requests: one stuck inside
/// Vision never reads stdin again, and was left running after the app quit.
#[cfg(unix)]
pub fn exit_with_parent() {
    // SAFETY: getppid has no preconditions.
    let parent = unsafe { libc::getppid() };
    let _ = std::thread::Builder::new().name("ocr-helper-parent".into()).spawn(move || loop {
        std::thread::sleep(PARENT_CHECK);
        // SAFETY: as above. An orphan is adopted by launchd, so its parent
        // changes.
        if unsafe { libc::getppid() } != parent {
            std::process::exit(0);
        }
    });
}

/// The app's side: a `TextRecognizer` that hands each frame to a helper
/// process, starting one when there is none and replacing it after a failure.
pub struct HelperRecognizer {
    command: Box<dyn Fn() -> Command + Send + Sync>,
    boosted: Box<dyn Fn() -> bool + Send + Sync>,
    helper: Mutex<Option<Helper>>,
    /// `ANSWER_TIMEOUT` and `FIRST_ANSWER_TIMEOUT`, shorter in tests.
    timeouts: (Duration, Duration),
}

struct Helper {
    child: Child,
    stdin: ChildStdin,
    /// The helper's stdout, a line at a time, from a thread that reads it so
    /// that waiting for an answer can time out. It ends when the helper
    /// exits.
    answers: Receiver<std::io::Result<String>>,
    /// Whether it has answered a request yet.
    answered: bool,
}

impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl HelperRecognizer {
    /// `command` starts a helper; `boosted` says whether the OCR worker is
    /// boosted at the time of each request.
    pub fn new(
        command: impl Fn() -> Command + Send + Sync + 'static,
        boosted: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Self {
        HelperRecognizer {
            command: Box::new(command),
            boosted: Box::new(boosted),
            helper: Mutex::new(None),
            timeouts: (ANSWER_TIMEOUT, FIRST_ANSWER_TIMEOUT),
        }
    }

    #[cfg(test)]
    fn with_timeouts(mut self, answer: Duration, first_answer: Duration) -> Self {
        self.timeouts = (answer, first_answer);
        self
    }

    fn start(&self) -> Result<Helper, String> {
        let mut child = (self.command)()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Cannot start the text recognizer: {}", e))?;
        let stdin = child.stdin.take().ok_or("The text recognizer has no stdin")?;
        let stdout = child.stdout.take().ok_or("The text recognizer has no stdout")?;
        let (send, answers) = mpsc::channel();
        let reader = std::thread::Builder::new().name("ocr-helper-reader".into()).spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let failed = line.is_err();
                if send.send(line).is_err() || failed {
                    break;
                }
            }
        });
        if let Err(error) = reader {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("Cannot read from the text recognizer: {}", error));
        }
        Ok(Helper { child, stdin, answers, answered: false })
    }

    /// One request to `helper`. An error is the recognizer's answer or a
    /// broken helper; either way the helper is not to be trusted again.
    fn ask(&self, helper: &mut Helper, image: &Path) -> Result<Vec<OcrLine>, String> {
        let request = Request { image: image.to_path_buf(), boosted: (self.boosted)() };
        let mut line = serde_json::to_string(&request).map_err(|e| e.to_string())?;
        line.push('\n');
        helper
            .stdin
            .write_all(line.as_bytes())
            .and_then(|_| helper.stdin.flush())
            .map_err(|e| format!("The text recognizer stopped: {}", e))?;

        let first = !helper.answered;
        let timeout = if first { self.timeouts.1 } else { self.timeouts.0 };
        let asked = Instant::now();
        let answer = match helper.answers.recv_timeout(timeout) {
            Ok(Ok(answer)) => answer,
            Ok(Err(error)) => return Err(format!("The text recognizer stopped: {}", error)),
            Err(RecvTimeoutError::Disconnected) => return Err("The text recognizer stopped".into()),
            Err(RecvTimeoutError::Timeout) => {
                return Err(format!(
                    "The text recognizer did not answer within {} s{}",
                    timeout.as_secs(),
                    if first { " of starting" } else { "" }
                ))
            }
        };
        helper.answered = true;
        let took = asked.elapsed();
        if took >= SLOW_ANSWER {
            crate::diagnostics::warn("ocr", "The text recognizer was slow to answer")
                .took(took)
                .data(serde_json::json!({ "first": first, "boosted": request.boosted }))
                .record();
        }
        match serde_json::from_str(&answer) {
            Ok(Response::Lines(lines)) => Ok(lines),
            Ok(Response::Error(error)) => Err(error),
            Err(error) => Err(format!("The text recognizer gave a bad answer: {}", error)),
        }
    }
}

impl TextRecognizer for HelperRecognizer {
    /// After a failure the helper is replaced and the frame tried again in
    /// the new one, unless the helper that failed was new already: then the
    /// failure is the frame's, or the system's, and goes to the caller.
    fn recognize(&self, image: &Path) -> Result<Vec<OcrLine>, String> {
        let mut slot = self.helper.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut fresh = false;
        loop {
            let helper = match slot.as_mut() {
                Some(helper) => helper,
                None => {
                    fresh = true;
                    slot.insert(self.start()?)
                }
            };
            match self.ask(helper, image) {
                Ok(lines) => return Ok(lines),
                Err(error) => {
                    // Dropping it kills it.
                    *slot = None;
                    if fresh {
                        return Err(error);
                    }
                    crate::diagnostics::warn(
                        "ocr",
                        format!("Restarting the text recognizer after: {}", error),
                    )
                    .record();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tempfile::TempDir;

    struct Fixed(Result<Vec<OcrLine>, String>);

    impl TextRecognizer for Fixed {
        fn recognize(&self, _image: &Path) -> Result<Vec<OcrLine>, String> {
            self.0.clone()
        }
    }

    fn line(text: &str) -> OcrLine {
        OcrLine { text: text.into(), confidence: 0.5, x: 0.1, y: 0.2, width: 0.3, height: 0.04 }
    }

    fn served(recognizer: &dyn TextRecognizer, requests: &str) -> Vec<String> {
        let mut output = Vec::new();
        serve(recognizer, Cursor::new(requests), &mut output, |_| {}).unwrap();
        String::from_utf8(output).unwrap().lines().map(String::from).collect()
    }

    #[test]
    fn the_helper_answers_each_request_with_its_lines_or_error() {
        let lines = served(&Fixed(Ok(vec![line("hello")])), "{\"image\":\"/a.png\"}\n{\"image\":\"/b.png\"}\n");
        assert_eq!(lines.len(), 2);
        let response: Response = serde_json::from_str(&lines[0]).unwrap();
        assert!(matches!(response, Response::Lines(l) if l == vec![line("hello")]));

        let lines = served(&Fixed(Err("broken".into())), "{\"image\":\"/a.png\"}\n");
        let response: Response = serde_json::from_str(&lines[0]).unwrap();
        assert!(matches!(response, Response::Error(e) if e == "broken"));

        let lines = served(&Fixed(Ok(vec![])), "not json\n");
        assert!(matches!(serde_json::from_str(&lines[0]).unwrap(), Response::Error(_)));
    }

    #[test]
    fn the_helper_is_told_whether_ocr_is_boosted() {
        let seen = std::cell::RefCell::new(Vec::new());
        let input = "{\"image\":\"/a.png\",\"boosted\":true}\n{\"image\":\"/a.png\"}\n";
        serve(&Fixed(Ok(vec![])), Cursor::new(input), std::io::sink(), |boosted| {
            seen.borrow_mut().push(boosted)
        })
        .unwrap();
        assert_eq!(*seen.borrow(), vec![true, false]);
    }

    /// A shell helper that answers from `answers` (one per line, in turn,
    /// shared by every helper started) and counts how many were started.
    struct FakeHelper {
        dir: TempDir,
    }

    const OK: &str = r#"{"lines":[{"text":"hi","confidence":1.0,"x":0.0,"y":0.0,"width":1.0,"height":1.0}]}"#;
    const FAIL: &str = r#"{"error":"CRImageReaderError error 1"}"#;

    impl FakeHelper {
        fn new(answers: &[&str]) -> Self {
            let dir = TempDir::new().unwrap();
            std::fs::write(dir.path().join("answers"), answers.join("\n") + "\n").unwrap();
            std::fs::write(dir.path().join("asked"), "0").unwrap();
            FakeHelper { dir }
        }

        /// Each request takes the next answer: the file's line numbered by
        /// the count of requests so far, across every helper.
        fn recognizer(&self) -> HelperRecognizer {
            let dir = self.dir.path().to_path_buf();
            HelperRecognizer::new(
                move || {
                    let mut command = Command::new("sh");
                    command.current_dir(&dir).arg("-c").arg(
                        "echo started >> starts; \
                         while read request; do \
                           n=$(( $(cat asked) + 1 )); echo $n > asked; \
                           sed -n \"${n}p\" answers; \
                         done",
                    );
                    command
                },
                || false,
            )
        }

        fn starts(&self) -> usize {
            std::fs::read_to_string(self.dir.path().join("starts")).map_or(0, |s| s.lines().count())
        }
    }

    #[test]
    fn one_helper_reads_frame_after_frame() {
        let fake = FakeHelper::new(&[OK, OK, OK]);
        let recognizer = fake.recognizer();
        for _ in 0..3 {
            assert_eq!(recognizer.recognize(Path::new("/a.png")).unwrap().len(), 1);
        }
        assert_eq!(fake.starts(), 1);
    }

    #[test]
    fn a_failing_helper_is_replaced_and_the_frame_tried_again() {
        let fake = FakeHelper::new(&[OK, FAIL, OK]);
        let recognizer = fake.recognizer();
        recognizer.recognize(Path::new("/a.png")).unwrap();
        assert_eq!(recognizer.recognize(Path::new("/b.png")).unwrap().len(), 1);
        assert_eq!(fake.starts(), 2);
    }

    #[test]
    fn a_new_helper_failing_is_reported_without_another_start() {
        let fake = FakeHelper::new(&[FAIL, OK]);
        let recognizer = fake.recognizer();
        assert_eq!(recognizer.recognize(Path::new("/a.png")), Err("CRImageReaderError error 1".into()));
        assert_eq!(fake.starts(), 1);
        // The next frame gets a new helper.
        recognizer.recognize(Path::new("/b.png")).unwrap();
        assert_eq!(fake.starts(), 2);
    }

    #[test]
    fn a_helper_failing_twice_in_a_row_is_reported() {
        let fake = FakeHelper::new(&[OK, FAIL, FAIL]);
        let recognizer = fake.recognizer();
        recognizer.recognize(Path::new("/a.png")).unwrap();
        assert!(recognizer.recognize(Path::new("/b.png")).is_err());
        assert_eq!(fake.starts(), 2);
    }

    /// A recognizer whose helpers run `script` in `sh`, numbered by `$n`
    /// (1 for the first helper started), with short timeouts.
    fn numbered_helpers(dir: &Path, script: &str) -> HelperRecognizer {
        let dir = dir.to_path_buf();
        let script = format!("echo started >> starts; n=$(wc -l < starts); {}", script);
        HelperRecognizer::new(
            move || {
                let mut command = Command::new("sh");
                command.current_dir(&dir).arg("-c").arg(&script);
                command
            },
            || false,
        )
        .with_timeouts(Duration::from_millis(300), Duration::from_secs(5))
    }

    #[test]
    fn a_helper_that_stops_answering_is_replaced() {
        let dir = TempDir::new().unwrap();
        // The first helper answers once, then hangs without exiting.
        let script = format!(
            "if [ $n -eq 1 ]; then read r; echo '{OK}'; read r; sleep 600; fi; \
             while read r; do echo '{OK}'; done"
        );
        let recognizer = numbered_helpers(dir.path(), &script);
        recognizer.recognize(Path::new("/a.png")).unwrap();
        let started = Instant::now();
        assert_eq!(recognizer.recognize(Path::new("/b.png")).unwrap().len(), 1);
        assert!(started.elapsed() < Duration::from_secs(5));
        let starts = std::fs::read_to_string(dir.path().join("starts")).unwrap();
        assert_eq!(starts.lines().count(), 2);
    }

    #[test]
    fn a_new_helper_gets_longer_for_its_first_answer() {
        let dir = TempDir::new().unwrap();
        // Each answer takes longer than a helper that has answered may take,
        // but within a new helper's limit.
        let script = format!("while read r; do sleep 1; echo '{OK}'; done");
        let recognizer = numbered_helpers(dir.path(), &script);
        assert_eq!(recognizer.recognize(Path::new("/a.png")).unwrap().len(), 1);
        // The second answer is too slow, so a new helper reads the frame.
        assert_eq!(recognizer.recognize(Path::new("/b.png")).unwrap().len(), 1);
        let starts = std::fs::read_to_string(dir.path().join("starts")).unwrap();
        assert_eq!(starts.lines().count(), 2);
    }

    #[test]
    fn a_new_helper_that_never_answers_is_reported() {
        let dir = TempDir::new().unwrap();
        let recognizer = numbered_helpers(dir.path(), "sleep 600")
            .with_timeouts(Duration::from_millis(100), Duration::from_millis(300));
        let error = recognizer.recognize(Path::new("/a.png")).unwrap_err();
        assert!(error.contains("did not answer"), "{}", error);
        let starts = std::fs::read_to_string(dir.path().join("starts")).unwrap();
        assert_eq!(starts.lines().count(), 1);
    }

    #[test]
    fn a_helper_that_exits_is_replaced() {
        let dir = TempDir::new().unwrap();
        let starts = dir.path().join("starts");
        let recognizer = HelperRecognizer::new(
            {
                let starts = starts.clone();
                move || {
                    let mut command = Command::new("sh");
                    // The first helper answers once and exits; later ones
                    // keep answering.
                    command.arg("-c").arg(format!(
                        "echo started >> '{starts}'; \
                         if [ $(wc -l < '{starts}') -eq 1 ]; then read r; echo '{OK}'; exit 0; fi; \
                         while read r; do echo '{OK}'; done",
                        starts = starts.display()
                    ));
                    command
                }
            },
            || false,
        );
        recognizer.recognize(Path::new("/a.png")).unwrap();
        recognizer.recognize(Path::new("/b.png")).unwrap();
        recognizer.recognize(Path::new("/c.png")).unwrap();
        assert_eq!(std::fs::read_to_string(&starts).unwrap().lines().count(), 2);
    }
}
