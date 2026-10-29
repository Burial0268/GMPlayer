//! Output delay as the host reports it, filtered so a bad stretch of timestamps
//! cannot move the lyric clock.
//!
//! The callback records one raw sample per period, wait-free; `settle` runs off
//! the real-time thread and publishes the median only once the recent window
//! agrees with itself. A route change (e.g. connecting Bluetooth) therefore
//! keeps the previous value through the switch and moves once, to the new one.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

const WINDOW: usize = 64;
/// At least half the window must be real readings before anything is published.
const MIN_VALID: usize = WINDOW / 2;
/// Below this the "delay" is the callback's own clock read: AAudio's
/// `getTimestamp` failed and CPAL fell back to `now()`.
const MIN_AHEAD_US: u32 = 1_000;
/// Above this it is a broken timestamp, not a device.
const MAX_PLAUSIBLE_US: u32 = 2_000_000;
/// Interquartile spread the window may have and still count as settled.
const MAX_SPREAD_US: u32 = 20_000;

/// One raw reading, or `0` when the host gave no usable timestamp.
///
/// `repeated` means the playback instant equals the previous callback's: CPAL
/// clamps a failed `getTimestamp` to the last value, so it is a stale reading.
pub(super) fn sample_us(ahead_us: u32, buffer_us: u32, repeated: bool) -> u32 {
    let total = ahead_us.saturating_add(buffer_us);
    if repeated || ahead_us < MIN_AHEAD_US || total > MAX_PLAUSIBLE_US {
        0
    } else {
        total
    }
}

pub(crate) struct LatencyMeter {
    samples: [AtomicU32; WINDOW],
    next: AtomicUsize,
    settled: AtomicU32,
}

impl LatencyMeter {
    pub(crate) fn new() -> Self {
        Self {
            samples: std::array::from_fn(|_| AtomicU32::new(0)),
            next: AtomicUsize::new(0),
            settled: AtomicU32::new(0),
        }
    }

    /// Audio callback only. Invalid readings are recorded too, so they push
    /// stale ones out of the window instead of leaving it looking settled.
    pub(super) fn record(&self, us: u32) {
        let slot = self.next.fetch_add(1, Ordering::Relaxed) % WINDOW;
        self.samples[slot].store(us, Ordering::Relaxed);
    }

    /// Last published delay in microseconds; `0` until the first one settles.
    pub(crate) fn settled_us(&self) -> u32 {
        self.settled.load(Ordering::Relaxed)
    }

    /// Off the real-time thread: publish the window's median if it is settled,
    /// otherwise keep the previous value. Returns what is published.
    pub(crate) fn settle(&self) -> u32 {
        let mut valid = [0u32; WINDOW];
        let mut count = 0;
        for sample in &self.samples {
            let us = sample.load(Ordering::Relaxed);
            if us != 0 {
                valid[count] = us;
                count += 1;
            }
        }
        if count >= MIN_VALID {
            let valid = &mut valid[..count];
            valid.sort_unstable();
            let median = valid[count / 2];
            let spread = valid[count * 3 / 4] - valid[count / 4];
            // Hosts that vary the period add that variation to every reading,
            // so a long pipeline is allowed a proportionally wider spread.
            if spread <= MAX_SPREAD_US.max(median / 4) {
                self.settled.store(median, Ordering::Relaxed);
            }
        }
        self.settled_us()
    }
}

impl std::fmt::Debug for LatencyMeter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LatencyMeter")
            .field("settled_us", &self.settled_us())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(meter: &LatencyMeter, readings: impl IntoIterator<Item = u32>) {
        for us in readings {
            meter.record(us);
        }
    }

    #[test]
    fn failed_or_clamped_timestamps_are_not_readings() {
        assert_eq!(sample_us(30_000, 10_000, false), 40_000);
        assert_eq!(sample_us(30_000, 10_000, true), 0);
        assert_eq!(sample_us(5, 10_000, false), 0);
        assert_eq!(sample_us(2_500_000, 10_000, false), 0);
    }

    #[test]
    fn jittery_readings_settle_on_the_median() {
        let meter = LatencyMeter::new();
        feed(&meter, (0..WINDOW as u32).map(|i| 40_000 + (i % 5) * 2_000));
        assert_eq!(meter.settle(), 44_000);
    }

    #[test]
    fn intermittent_failures_do_not_pull_the_value_down() {
        let meter = LatencyMeter::new();
        feed(
            &meter,
            (0..WINDOW).map(|i| if i % 3 == 0 { 0 } else { 220_000 }),
        );
        assert_eq!(meter.settle(), 220_000);
    }

    #[test]
    fn a_route_change_keeps_the_old_value_then_moves_once() {
        let meter = LatencyMeter::new();
        feed(&meter, [40_000; WINDOW]);
        assert_eq!(meter.settle(), 40_000);

        // The switch: timestamps fail or jump around.
        feed(
            &meter,
            (0..WINDOW as u32).map(|i| match i % 4 {
                0 | 1 => 0,
                2 => 15_000,
                _ => 260_000,
            }),
        );
        assert_eq!(meter.settle(), 40_000);

        feed(
            &meter,
            (0..WINDOW as u32).map(|i| 230_000 + (i % 3) * 5_000),
        );
        assert_eq!(meter.settle(), 235_000);
    }

    #[test]
    fn nothing_is_published_without_enough_real_readings() {
        let meter = LatencyMeter::new();
        feed(
            &meter,
            (0..WINDOW).map(|i| if i < MIN_VALID - 1 { 40_000 } else { 0 }),
        );
        assert_eq!(meter.settle(), 0);
    }
}
