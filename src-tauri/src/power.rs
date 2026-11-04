//! Whether the machine is on AC power. Conversion, OCR and the preview
//! builder run only then (unless a boost allows battery), and the Activity
//! window shows it.
//!
//! On macOS a reading runs `pmset`, so one reading is shared by every loop
//! that asks and reused for `READING_TTL`. Other platforms have no battery
//! check and always report AC power.

use std::process::Command;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a reading is reused before `pmset` runs again.
const READING_TTL: Duration = Duration::from_secs(10);

/// Whether the machine is drawing from AC power, read at most once every
/// `READING_TTL` across the app.
///
/// If `pmset` fails the answer is `false`, so an unreadable power state never
/// leads to working on battery.
pub fn on_ac_power() -> bool {
    if !cfg!(target_os = "macos") {
        return true;
    }
    static LAST: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
    // Held while reading, so loops asking at once run `pmset` only once.
    let mut last = LAST.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((at, on_ac)) = *last {
        if at.elapsed() < READING_TTL {
            return on_ac;
        }
    }
    let on_ac = read_power_source();
    *last = Some((Instant::now(), on_ac));
    on_ac
}

/// Ask `pmset`, and log any change in the answer.
fn read_power_source() -> bool {
    let reading = Command::new("pmset")
        .args(["-g", "ps"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| parse_pmset_power_source(&String::from_utf8_lossy(&output.stdout)));
    log_power_change(reading);
    reading.unwrap_or(false)
}

/// Log a change in the power source, or in whether it could be read at all.
fn log_power_change(reading: Option<bool>) {
    // 0: not read yet, 1: unreadable, 2: battery, 3: AC.
    static LAST: AtomicU8 = AtomicU8::new(0);
    let code = match reading {
        None => 1,
        Some(false) => 2,
        Some(true) => 3,
    };
    if LAST.swap(code, Ordering::Relaxed) != code {
        match reading {
            None => crate::diagnostics::warn("power", "Could not read the power source").record(),
            Some(false) => crate::diagnostics::info("power", "On battery").record(),
            Some(true) => crate::diagnostics::info("power", "On AC power").record(),
        }
    }
}

/// Parse the first line of `pmset -g ps`, e.g. `Now drawing from 'AC Power'`.
/// Returns `None` when the output does not name a power source.
fn parse_pmset_power_source(output: &str) -> Option<bool> {
    let first_line = output.lines().next()?;
    let source = first_line.split('\'').nth(1)?;
    Some(source == "AC Power")
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
