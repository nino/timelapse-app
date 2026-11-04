//! Builds preview packs (see `frame_source::previews`) for videos that don't
//! have one yet: the old script's videos, and hourly videos converted before
//! the converter wrote previews.
//!
//! One pack at a time, on its own thread, with ffmpeg at background
//! priority, and only while conversion may run (on AC power, or during a
//! boost that allows battery; never in Low power mode). The frame source
//! decides the order: the day the viewer is on first, outward from where it
//! is, keyframe packs before full ones, then the rest of the library, newest
//! first, until the store is nearly full.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use frame_source::{FrameSource, PackKind};

use crate::boost::Boost;
use crate::converter::on_ac_power;

/// How long to wait before looking again when nothing is missing, unless
/// the viewer opens another day first.
const IDLE_POLL: Duration = Duration::from_secs(10 * 60);

/// How long to wait before looking at the power again when it says no.
const ON_BATTERY_POLL: Duration = Duration::from_secs(60);

/// How long a power reading is trusted between packs, which can take a
/// fraction of a second each.
const POWER_CHECK: Duration = Duration::from_secs(30);

/// Start the builder. `on_built` is called with the day after each pack, so
/// the viewer can load it.
pub fn start(source: Arc<FrameSource>, boost: Arc<Boost>, on_built: impl Fn(&str) + Send + 'static) {
    let spawned = std::thread::Builder::new()
        .name("preview builder".into())
        .spawn(move || run(&source, &boost, &on_built));
    if let Err(e) = spawned {
        crate::diagnostics::error("previews", format!("Could not start the preview builder: {}", e)).record();
    }
}

fn run(source: &FrameSource, boost: &Boost, on_built: &dyn Fn(&str)) {
    // Packs that failed this run, so a broken video isn't retried forever.
    let mut failed: HashSet<(PathBuf, PackKind)> = HashSet::new();
    let mut power: Option<(Instant, bool)> = None;
    let mut seen_viewer = 0;
    loop {
        let may_work = boost.may_work(|| match power {
            Some((at, on_ac)) if at.elapsed() < POWER_CHECK => on_ac,
            _ => {
                let on_ac = on_ac_power();
                power = Some((Instant::now(), on_ac));
                on_ac
            }
        });
        if !may_work {
            // A boost or Low power mode starting or ending wakes this early.
            boost.sleep(ON_BATTERY_POLL);
            power = None;
            continue;
        }
        let job = source.next_preview_job(&|job| failed.contains(&(job.video.clone(), job.kind)));
        let Some(job) = job else {
            seen_viewer = source.wait_for_viewer(seen_viewer, IDLE_POLL);
            continue;
        };
        let started = Instant::now();
        match source.build_preview(&job) {
            Ok(images) => {
                println!(
                    "Built {} previews ({:?}) of {} in {:.1}s",
                    images,
                    job.kind,
                    job.video.display(),
                    started.elapsed().as_secs_f32()
                );
                on_built(&job.date);
            }
            Err(e) => {
                crate::diagnostics::warn("previews", format!("Could not build previews of {}: {}", job.video.display(), e))
                    .record();
                failed.insert((job.video, job.kind));
            }
        }
    }
}
