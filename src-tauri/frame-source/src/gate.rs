//! Limits how many callers do something at once, letting the newest waiter
//! in first.
//!
//! Probing a video reads the whole file, and every command that asks about a
//! day probes the day's videos on its own thread. Paging quickly through a
//! year of days therefore asks for hundreds of probes within a second or
//! two. Run all at once, that many ffmpeg processes reading whole files fill
//! the disk's queue, and everything else on the machine that touches the
//! disk waits behind them: WindowServer hung that way for over a minute.
//!
//! Newest first, because the newest request is for the day the viewer is on
//! now and the older ones are for days it has already left.

use std::sync::{Condvar, Mutex};

pub struct Gate {
    state: Mutex<State>,
    wake: Condvar,
}

struct State {
    free: usize,
    /// Tickets of the callers waiting, oldest first.
    waiting: Vec<u64>,
    next_ticket: u64,
}

/// A turn through the gate, given back when dropped.
pub struct Turn<'a> {
    gate: &'a Gate,
}

impl Gate {
    pub fn new(limit: usize) -> Self {
        Self {
            state: Mutex::new(State {
                free: limit,
                waiting: Vec::new(),
                next_ticket: 0,
            }),
            wake: Condvar::new(),
        }
    }

    /// Wait for a turn. While turns are taken, the caller that started
    /// waiting last gets the next one.
    pub fn enter(&self) -> Turn<'_> {
        let mut state = self.state.lock().unwrap();
        let ticket = state.next_ticket;
        state.next_ticket += 1;
        state.waiting.push(ticket);
        while state.free == 0 || state.waiting.last() != Some(&ticket) {
            state = self.wake.wait(state).unwrap();
        }
        state.waiting.pop();
        state.free -= 1;
        // Another turn may be free for the waiter now at the back.
        if state.free > 0 && !state.waiting.is_empty() {
            self.wake.notify_all();
        }
        Turn { gate: self }
    }
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        self.gate.state.lock().unwrap().free += 1;
        self.gate.wake.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    fn wait_for_waiters(gate: &Gate, count: usize) {
        while gate.state.lock().unwrap().waiting.len() < count {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn never_lets_more_than_its_limit_in() {
        let gate = Arc::new(Gate::new(2));
        let inside = Arc::new(AtomicUsize::new(0));
        let most = Arc::new(AtomicUsize::new(0));
        let threads: Vec<_> = (0..16)
            .map(|_| {
                let (gate, inside, most) = (gate.clone(), inside.clone(), most.clone());
                std::thread::spawn(move || {
                    let _turn = gate.enter();
                    most.fetch_max(inside.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(5));
                    inside.fetch_sub(1, Ordering::SeqCst);
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(most.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn lets_the_newest_waiter_in_first() {
        let gate = Arc::new(Gate::new(1));
        let order = Arc::new(Mutex::new(Vec::new()));
        let held = gate.enter();
        let threads: Vec<_> = (0..4)
            .map(|n| {
                let (waiter, order) = (gate.clone(), order.clone());
                let thread = std::thread::spawn(move || {
                    let _turn = waiter.enter();
                    order.lock().unwrap().push(n);
                });
                // Each one is waiting before the next starts, so their order
                // in the queue is known.
                wait_for_waiters(&gate, n + 1);
                thread
            })
            .collect();
        drop(held);
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(*order.lock().unwrap(), [3, 2, 1, 0]);
    }
}
