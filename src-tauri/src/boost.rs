//! Boost: for a while, video conversion and OCR stop saving CPU.
//!
//! Normally both run at background priority (ffmpeg under `taskpolicy -b`,
//! the OCR thread at the background QoS class), batches start at most every
//! ten minutes, and nothing runs on battery. While a boost is on, ffmpeg and
//! OCR run at normal priority, batches follow each other straight away, and,
//! if the boost allows it, they also run on battery.
//!
//! Low-power mode is the opposite: for a while, conversion and OCR don't run
//! at all, even on AC power, and a running encode is paused as it is on
//! battery. Starting either ends the other.
//!
//! Both last until their end time or until they are stopped, and are
//! forgotten when the app quits. The workers ask `is_on`/`may_work` as they
//! go, so an expired boost or low-power mode takes effect at their next
//! check; starting or stopping one also wakes any worker sleeping in
//! `sleep`/`wait`.

use chrono::{DateTime, Local};
use serde::Serialize;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// Longest boost the app accepts.
pub const MAX_BOOST: Duration = Duration::from_secs(24 * 60 * 60);

/// How often the workers re-check the power source during a boost that
/// doesn't allow battery, so plugging in starts the work within seconds.
pub const BOOSTED_POWER_POLL: Duration = Duration::from_secs(10);

/// A boost in progress, as the Activity window shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoostStatus {
    pub until: DateTime<Local>,
    /// Whether conversion and OCR also run on battery.
    pub allow_battery: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Boost { allow_battery: bool },
    LowPower,
}

#[derive(Debug, Clone, Copy)]
struct Active {
    ends: Instant,
    kind: Kind,
}

impl Active {
    fn left(&self) -> Duration {
        self.ends.saturating_duration_since(Instant::now())
    }

    fn until(&self) -> DateTime<Local> {
        Local::now() + chrono::Duration::from_std(self.left()).unwrap_or_default()
    }
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

    fn start_kind(&self, duration: Duration, kind: Kind) {
        let ends = Instant::now() + duration.min(MAX_BOOST);
        self.set(Some(Active { ends, kind }));
    }

    /// Boost for `duration` from now (at most `MAX_BOOST`), replacing any
    /// boost or low-power mode in progress.
    pub fn start(&self, duration: Duration, allow_battery: bool) {
        self.start_kind(duration, Kind::Boost { allow_battery });
    }

    /// Keep conversion and OCR off for `duration` from now (at most
    /// `MAX_BOOST`), replacing any boost or low-power mode in progress.
    pub fn start_low_power(&self, duration: Duration) {
        self.start_kind(duration, Kind::LowPower);
    }

    /// End the boost in progress. A low-power mode is left alone.
    pub fn stop(&self) {
        if self.boost().is_some() {
            self.set(None);
        }
    }

    /// End the low-power mode in progress. A boost is left alone.
    pub fn stop_low_power(&self) {
        if self.is_low_power() {
            self.set(None);
        }
    }

    /// Let the boost in progress run on battery, or not. Does nothing when
    /// no boost is on.
    pub fn set_allow_battery(&self, allow_battery: bool) {
        if let Some(active) = self.boost() {
            self.set(Some(Active { kind: Kind::Boost { allow_battery }, ..active }));
        }
    }

    fn active(&self) -> Option<Active> {
        self.lock().active.filter(|active| active.ends > Instant::now())
    }

    fn boost(&self) -> Option<Active> {
        self.active().filter(|active| matches!(active.kind, Kind::Boost { .. }))
    }

    /// Whether a boost is on right now.
    pub fn is_on(&self) -> bool {
        self.boost().is_some()
    }

    /// The boost in progress, if any.
    pub fn status(&self) -> Option<BoostStatus> {
        self.active().and_then(|active| match active.kind {
            Kind::Boost { allow_battery } => Some(BoostStatus { until: active.until(), allow_battery }),
            Kind::LowPower => None,
        })
    }

    /// Whether low-power mode is on right now.
    pub fn is_low_power(&self) -> bool {
        self.low_power_left().is_some()
    }

    /// How long the low-power mode in progress has left, if one is on.
    pub fn low_power_left(&self) -> Option<Duration> {
        self.active().filter(|active| active.kind == Kind::LowPower).map(|active| active.left())
    }

    /// When the low-power mode in progress ends, if one is on.
    pub fn low_power_until(&self) -> Option<DateTime<Local>> {
        self.active().filter(|active| active.kind == Kind::LowPower).map(|active| active.until())
    }

    /// Whether conversion and OCR may run now: never in low-power mode;
    /// otherwise on AC power, as `on_ac_power` says, or during a boost that
    /// allows battery. Only asks `on_ac_power` when that decides it.
    pub fn may_work(&self, on_ac_power: impl FnOnce() -> bool) -> bool {
        match self.active().map(|active| active.kind) {
            Some(Kind::LowPower) => false,
            Some(Kind::Boost { allow_battery: true }) => true,
            _ => on_ac_power(),
        }
    }

    /// Sleep for `wait`, or until a boost or low-power mode starts or stops.
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
    fn low_power_stops_all_work_until_it_ends_or_stops() {
        let boost = Boost::default();
        boost.start_low_power(Duration::from_secs(10 * 60));
        assert!(boost.is_low_power());
        assert!(!boost.is_on());
        assert_eq!(boost.status(), None);
        assert!(!boost.may_work(|| panic!("no need to read the power source")));
        let left = boost.low_power_left().unwrap().as_secs();
        assert!((10 * 60 - 5..=10 * 60).contains(&left), "{left}s left");
        assert!(boost.low_power_until().is_some());

        boost.stop();
        assert!(boost.is_low_power(), "stopping a boost leaves low power alone");
        boost.set_allow_battery(true);
        assert!(!boost.may_work(|| true));

        boost.stop_low_power();
        assert!(!boost.is_low_power());
        assert_eq!(boost.low_power_until(), None);
        assert!(boost.may_work(|| true));
    }

    #[test]
    fn boost_and_low_power_replace_each_other() {
        let boost = Boost::default();
        boost.start(Duration::from_secs(60), true);
        boost.start_low_power(Duration::from_secs(60));
        assert!(!boost.is_on());
        assert!(boost.is_low_power());

        boost.stop_low_power();
        boost.start_low_power(Duration::from_secs(60));
        boost.start(Duration::from_secs(60), false);
        assert!(boost.is_on());
        assert!(!boost.is_low_power());
        boost.stop_low_power();
        assert!(boost.is_on(), "ending low power leaves a boost alone");
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
        boost.start_low_power(Duration::from_secs(60));
        let started = Instant::now();
        wait(&mut changes, Duration::from_secs(60)).await;
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
