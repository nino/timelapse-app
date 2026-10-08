//! Boost: for a while, video conversion and OCR stop saving CPU.
//!
//! Normally both run at background priority (ffmpeg under `taskpolicy -b`,
//! the OCR thread at the background QoS class), batches start at most every
//! ten minutes, and nothing runs on battery. While a boost is on, ffmpeg and
//! OCR run at normal priority, batches follow each other straight away, and,
//! if the boost allows it, they also run on battery.
//!
//! A boost lasts until its end time or until it is stopped, and is forgotten
//! when the app quits. The workers ask `is_on`/`may_work` as they go, so an
//! expired boost takes effect at their next check; starting or stopping one
//! also wakes any worker sleeping in `sleep`/`wait`.

use chrono::{DateTime, Local};
use serde::Serialize;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// Longest boost the app accepts.
pub const MAX_BOOST: Duration = Duration::from_secs(24 * 60 * 60);

/// A boost in progress, as the Activity window shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoostStatus {
    pub until: DateTime<Local>,
    /// Whether conversion and OCR also run on battery.
    pub allow_battery: bool,
}

#[derive(Debug, Clone, Copy)]
struct Active {
    ends: Instant,
    allow_battery: bool,
}

#[derive(Default)]
struct Inner {
    active: Option<Active>,
    /// Bumped on every start and stop, so a sleeping worker can tell.
    generation: u64,
}

pub struct Boost {
    inner: Mutex<Inner>,
    changed: Condvar,
    changes: watch::Sender<u64>,
}

impl Default for Boost {
    fn default() -> Self {
        Boost {
            inner: Mutex::default(),
            changed: Condvar::new(),
            changes: watch::Sender::new(0),
        }
    }
}

impl Boost {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn set(&self, active: Option<Active>) {
        let generation = {
            let mut inner = self.lock();
            inner.active = active;
            inner.generation += 1;
            inner.generation
        };
        self.changed.notify_all();
        self.changes.send_replace(generation);
    }

    /// Boost for `duration` from now (at most `MAX_BOOST`), replacing any
    /// boost in progress.
    pub fn start(&self, duration: Duration, allow_battery: bool) {
        let ends = Instant::now() + duration.min(MAX_BOOST);
        self.set(Some(Active { ends, allow_battery }));
    }

    pub fn stop(&self) {
        self.set(None);
    }

    /// Let the boost in progress run on battery, or not. Does nothing when
    /// no boost is on.
    pub fn set_allow_battery(&self, allow_battery: bool) {
        if let Some(active) = self.active() {
            self.set(Some(Active { allow_battery, ..active }));
        }
    }

    fn active(&self) -> Option<Active> {
        self.lock().active.filter(|active| active.ends > Instant::now())
    }

    /// Whether a boost is on right now.
    pub fn is_on(&self) -> bool {
        self.active().is_some()
    }

    /// The boost in progress, if any.
    pub fn status(&self) -> Option<BoostStatus> {
        self.active().map(|active| {
            let left = active.ends.saturating_duration_since(Instant::now());
            BoostStatus {
                until: Local::now() + chrono::Duration::from_std(left).unwrap_or_default(),
                allow_battery: active.allow_battery,
            }
        })
    }

    /// Whether conversion and OCR may run now: on AC power, as
    /// `on_ac_power` says, or during a boost that allows battery (which
    /// doesn't ask `on_ac_power` at all).
    pub fn may_work(&self, on_ac_power: impl FnOnce() -> bool) -> bool {
        self.active().is_some_and(|active| active.allow_battery) || on_ac_power()
    }

    /// Sleep for `wait`, or until a boost starts or stops.
    pub fn sleep(&self, wait: Duration) {
        let inner = self.lock();
        let seen = inner.generation;
        let _ = self
            .changed
            .wait_timeout_while(inner, wait, |inner| inner.generation == seen);
    }

    /// For `wait`: a receiver whose `changed()` fires when a boost starts or
    /// stops after this call.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }
}

/// Sleep for `wait`, or until `changes` (from `Boost::subscribe`) reports a
/// boost starting or stopping since it was last woken.
pub async fn wait(changes: &mut watch::Receiver<u64>, wait: Duration) {
    tokio::select! {
        _ = tokio::time::sleep(wait) => {}
        // An error means the Boost is gone; then just sleep.
        result = changes.changed() => {
            if result.is_err() {
                tokio::time::sleep(wait).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn a_boost_is_on_until_it_ends_or_stops() {
        let boost = Boost::default();
        assert!(!boost.is_on());
        assert_eq!(boost.status(), None);

        boost.start(Duration::from_secs(20 * 60), false);
        assert!(boost.is_on());
        let status = boost.status().unwrap();
        assert!(!status.allow_battery);
        let left = (status.until - Local::now()).num_seconds();
        assert!((20 * 60 - 5..=20 * 60).contains(&left), "{left}s left");

        boost.stop();
        assert!(!boost.is_on());

        boost.start(Duration::ZERO, true);
        assert!(!boost.is_on(), "an expired boost is off");
    }

    #[test]
    fn caps_the_length_of_a_boost() {
        let boost = Boost::default();
        boost.start(Duration::from_secs(1000 * 24 * 60 * 60), false);
        let left = (boost.status().unwrap().until - Local::now()).num_seconds();
        assert!(left <= MAX_BOOST.as_secs() as i64);
    }

    #[test]
    fn works_on_battery_only_during_a_boost_that_allows_it() {
        let boost = Boost::default();
        assert!(boost.may_work(|| true));
        assert!(!boost.may_work(|| false));

        boost.start(Duration::from_secs(60), false);
        assert!(!boost.may_work(|| false));

        boost.start(Duration::from_secs(60), true);
        assert!(boost.may_work(|| panic!("no need to read the power source")));

        boost.set_allow_battery(false);
        assert!(!boost.may_work(|| false));
        assert!(boost.is_on());

        boost.stop();
        assert!(!boost.may_work(|| false));
        boost.set_allow_battery(true);
        assert!(!boost.is_on(), "allowing battery doesn't start a boost");
    }

    #[test]
    fn starting_a_boost_wakes_a_sleeping_thread() {
        let boost = Arc::new(Boost::default());
        let sleeper = {
            let boost = Arc::clone(&boost);
            std::thread::spawn(move || {
                let started = Instant::now();
                boost.sleep(Duration::from_secs(60));
                started.elapsed()
            })
        };
        std::thread::sleep(Duration::from_millis(100));
        boost.start(Duration::from_secs(60), true);
        assert!(sleeper.join().unwrap() < Duration::from_secs(10));
    }

    #[tokio::test]
    async fn starting_a_boost_wakes_a_waiting_task() {
        let boost = Arc::new(Boost::default());
        let mut changes = boost.subscribe();
        let waiter = tokio::spawn(async move {
            let started = Instant::now();
            wait(&mut changes, Duration::from_secs(60)).await;
            started.elapsed()
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        boost.start(Duration::from_secs(60), false);
        assert!(waiter.await.unwrap() < Duration::from_secs(10));
    }

    #[tokio::test]
    async fn a_change_while_busy_cuts_the_next_wait_short() {
        let boost = Boost::default();
        let mut changes = boost.subscribe();
        boost.stop();
        let started = Instant::now();
        wait(&mut changes, Duration::from_secs(60)).await;
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
