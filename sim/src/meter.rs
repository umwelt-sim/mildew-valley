//! Tick cost reporting for the region server.
//!
//! The simulation hands a [`TickReport`] to its observer every tick. This
//! accumulates them and prints a summary line at a fixed interval.
//!
//! The region is paced with [`Overrun::Dilate`](umwelt::Overrun::Dilate): a
//! tick that runs long is not dropped and not made up. An overloaded region
//! does not error, it runs the world slower than real time. `late` is the
//! only signal for that.

use std::time::{Duration, Instant};

use umwelt::{RunSummary, TickReport};

/// Accumulates ticks and prints a line every `every`.
pub struct Meter {
    every: Duration,
    since: Instant,
    /// Tick durations in the current window. Kept whole so percentiles are
    /// exact.
    took: Vec<Duration>,
    late: Vec<Duration>,
    viewers: u64,
    candidates: u64,
    records: u64,
    bytes: u64,
    new_ghosts: u64,
    departed: u64,
    /// Nanoseconds inside PayloadSink::send, summed across workers. Divided by
    /// the worker count it is what the tick spent writing to NATS, which
    /// separates a region that is computing from one that is waiting on I/O.
    sink_nanos: u64,
    threads: usize,
}

impl Meter {
    pub fn new(every: Duration) -> Meter {
        Meter {
            every,
            since: Instant::now(),
            took: Vec::new(),
            late: Vec::new(),
            viewers: 0,
            candidates: 0,
            records: 0,
            bytes: 0,
            new_ghosts: 0,
            departed: 0,
            sink_nanos: 0,
            threads: 1,
        }
    }

    /// Worker count, for turning summed sink time back into per-tick cost.
    pub fn with_threads(mut self, threads: usize) -> Meter {
        self.threads = threads.max(1);
        self
    }

    /// Folds in one tick. Prints and resets when the window is up.
    pub fn observe(&mut self, report: &TickReport) {
        self.took.push(report.took);
        if !report.late.is_zero() {
            self.late.push(report.late);
        }
        self.viewers += report.stats.viewers;
        self.candidates += report.stats.candidates;
        self.records += report.stats.records;
        self.bytes += report.stats.bytes;
        self.new_ghosts += report.stats.new_ghosts;
        self.departed += report.stats.departed;
        self.sink_nanos += report.stats.sink_nanos;

        if self.since.elapsed() >= self.every {
            self.print();
            self.reset();
        }
    }

    fn print(&mut self) {
        let window = self.since.elapsed().as_secs_f64();
        if self.took.is_empty() || window <= 0.0 {
            return;
        }
        self.took.sort_unstable();
        let ticks = self.took.len();

        // candidates are everything a viewer could have been told about.
        // records are what fitted the packet budget. The ratio is how much
        // priority scoring discarded.
        // Summed across workers, so divide to get what one tick paid.
        let sink_ms =
            self.sink_nanos as f64 / 1e6 / self.threads as f64 / ticks as f64;

        let kept = if self.candidates > 0 {
            100.0 * self.records as f64 / self.candidates as f64
        } else {
            100.0
        };

        println!(
            "mv-sim: {ticks} ticks/{window:.1}s | tick p50 {:.2}ms p99 {:.2}ms max {:.2}ms | \
             late {}/{ticks} worst {:.1}ms | viewers {:.0} | \
             candidates {:.0}/s -> records {:.0}/s ({kept:.0}% kept) | {:.1} MB/s | \
             sink {:.2}ms/tick ({:.0}% of tick) | ghosts +{} -{}",
            ms(pct(&self.took, 0.50)),
            ms(pct(&self.took, 0.99)),
            ms(*self.took.last().expect("non-empty")),
            self.late.len(),
            ms(self.late.iter().copied().max().unwrap_or_default()),
            self.viewers as f64 / ticks as f64,
            self.candidates as f64 / window,
            self.records as f64 / window,
            self.bytes as f64 / window / 1_000_000.0,
            sink_ms,
            100.0 * sink_ms / ms(pct(&self.took, 0.50)).max(f64::MIN_POSITIVE),
            self.new_ghosts,
            self.departed,
        );
    }

    fn reset(&mut self) {
        self.since = Instant::now();
        self.took.clear();
        self.late.clear();
        self.viewers = 0;
        self.candidates = 0;
        self.records = 0;
        self.bytes = 0;
        self.new_ghosts = 0;
        self.departed = 0;
        self.sink_nanos = 0;
    }

    /// One line for a whole run, for a sweep to collect.
    pub fn summarize(summary: &RunSummary) {
        println!(
            "mv-sim: run of {} ticks in {:.1}s | late {} ({:.1}%) | dropped {} | \
             worst tick {:.2}ms | worst late {:.1}ms",
            summary.ticks,
            summary.elapsed.as_secs_f64(),
            summary.late,
            100.0 * summary.late as f64 / summary.ticks.max(1) as f64,
            summary.dropped,
            ms(summary.worst_tick),
            ms(summary.worst_late),
        );
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1_000.0
}

/// The value at `q` of a sorted slice, by nearest rank.
fn pct(sorted: &[Duration], q: f64) -> Duration {
    if sorted.is_empty() {
        return Duration::ZERO;
    }
    let rank = (q * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    #[test]
    fn a_percentile_picks_a_measured_value() {
        let sorted = vec![d(1), d(2), d(3), d(4), d(5), d(6), d(7), d(8), d(9), d(10)];
        assert_eq!(pct(&sorted, 0.50), d(5));
        assert_eq!(pct(&sorted, 0.99), d(10));
        assert_eq!(pct(&sorted, 0.0), d(1));
    }

    #[test]
    fn a_percentile_of_nothing_is_zero_rather_than_a_panic() {
        assert_eq!(pct(&[], 0.5), Duration::ZERO);
    }

    #[test]
    fn one_sample_is_every_percentile() {
        assert_eq!(pct(&[d(7)], 0.5), d(7));
        assert_eq!(pct(&[d(7)], 0.99), d(7));
    }

    /// An average would hide one tick that blew its deadline among ninety-nine
    /// that did not.
    #[test]
    fn a_single_slow_tick_shows_up_in_the_maximum() {
        let mut took: Vec<Duration> = (0..99).map(|_| d(1)).collect();
        took.push(d(500));
        took.sort_unstable();
        assert_eq!(pct(&took, 0.50), d(1));
        assert_eq!(pct(&took, 0.99), d(1));
        assert_eq!(*took.last().unwrap(), d(500));
    }
}
