//! The daemon's watch over its own CPU and memory: a use that grows out of the ordinary is
//! written to the daemon log while it happens, so it can still be traced after the OS has
//! killed the daemon.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tokio::runtime::Handle;

/// How often the daemon measures itself.
const INTERVAL: Duration = Duration::from_secs(60);
/// The share of one CPU over an interval from which the daemon is busy.
const BUSY: f64 = 0.5;
/// The peak resident memory written first; each line after is written at twice the last.
const FIRST_MEMORY: u64 = 256 * 1024 * 1024;

/// Measures the daemon at every interval on a thread of its own, which goes on while the
/// runtime's workers are all held, and writes a line when the daemon turns busy or calm and
/// each time its peak memory doubles.
pub fn watch(runtime: Handle) -> Result<()> {
    std::thread::Builder::new()
        .name("daemon-usage".to_owned())
        .spawn(move || {
            let started = Instant::now();
            let mut assessment = Assessment::new(cpu_time());
            let mut sampled = started;
            loop {
                std::thread::sleep(INTERVAL);
                // A thread woken late measures the time that really passed.
                let elapsed = sampled.elapsed();
                sampled = Instant::now();
                let sample = Sample {
                    cpu: cpu_time(),
                    peak_memory: peak_memory(),
                    tasks: runtime.metrics().num_alive_tasks(),
                };
                if let Some(line) = assessment.assess(&sample, elapsed) {
                    eprintln!("Feld daemon: {line} (up {}s)", started.elapsed().as_secs());
                }
            }
        })
        .context("start the daemon usage watch")?;
    Ok(())
}

struct Sample {
    cpu: Duration,
    peak_memory: u64,
    tasks: usize,
}

struct Assessment {
    cpu: Duration,
    busy: bool,
    next_memory: u64,
}

impl Assessment {
    fn new(cpu: Duration) -> Self {
        Self {
            cpu,
            busy: false,
            next_memory: FIRST_MEMORY,
        }
    }

    /// The line `sample`, taken `elapsed` after the one before, writes to the log.
    fn assess(&mut self, sample: &Sample, elapsed: Duration) -> Option<String> {
        let share =
            sample.cpu.saturating_sub(self.cpu).as_secs_f64() / elapsed.as_secs_f64().max(1e-9);
        self.cpu = sample.cpu;
        let busy = share >= BUSY;
        let mut events = Vec::new();
        if busy != self.busy {
            self.busy = busy;
            events.push(if busy { "busy" } else { "calm again" });
        }
        if sample.peak_memory >= self.next_memory {
            while sample.peak_memory >= self.next_memory {
                self.next_memory = self.next_memory.saturating_mul(2);
            }
            events.push("memory grew");
        }
        (!events.is_empty()).then(|| {
            format!(
                "{}: CPU {:.0}% of one core over the last {}s, peak RSS {} MiB, {} tasks",
                events.join(", "),
                share * 100.0,
                elapsed.as_secs(),
                sample.peak_memory / (1024 * 1024),
                sample.tasks,
            )
        })
    }
}

fn usage() -> libc::rusage {
    // SAFETY: getrusage fills the zeroed struct it is handed and has no other effect.
    unsafe {
        let mut usage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut usage);
        usage
    }
}

/// The CPU time the daemon has used, in user and system mode.
fn cpu_time() -> Duration {
    let usage = usage();
    let time = |value: libc::timeval| {
        Duration::new(value.tv_sec as u64, 0) + Duration::from_micros(value.tv_usec as u64)
    };
    time(usage.ru_utime) + time(usage.ru_stime)
}

/// The most resident memory the daemon has held, in bytes.
fn peak_memory() -> u64 {
    let peak = usage().ru_maxrss as u64;
    // macOS counts it in bytes, and Linux in KiB.
    if cfg!(target_os = "macos") {
        peak
    } else {
        peak * 1024
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: u64 = 1024 * 1024;

    fn sample(cpu: u64, peak_memory: u64) -> Sample {
        Sample {
            cpu: Duration::from_secs(cpu),
            peak_memory,
            tasks: 7,
        }
    }

    #[test]
    fn an_idle_daemon_writes_nothing() {
        let mut assessment = Assessment::new(Duration::ZERO);
        for minute in 1..10 {
            assert_eq!(assessment.assess(&sample(minute, 40 * MIB), INTERVAL), None);
        }
    }

    #[test]
    fn turning_busy_and_calm_is_written_once_each() {
        let mut assessment = Assessment::new(Duration::ZERO);
        assert_eq!(
            assessment
                .assess(&sample(54, 40 * MIB), INTERVAL)
                .as_deref(),
            Some("busy: CPU 90% of one core over the last 60s, peak RSS 40 MiB, 7 tasks")
        );
        assert_eq!(assessment.assess(&sample(108, 40 * MIB), INTERVAL), None);
        assert!(
            assessment
                .assess(&sample(109, 40 * MIB), INTERVAL)
                .unwrap()
                .starts_with("calm again: CPU 2%")
        );
    }

    #[test]
    fn memory_is_written_each_time_its_peak_doubles() {
        let mut assessment = Assessment::new(Duration::ZERO);
        let written = [100, 300, 400, 520, 1000, 3000, 3100].map(|peak| {
            assessment
                .assess(&sample(0, peak * MIB), INTERVAL)
                .is_some()
        });
        assert_eq!(written, [false, true, false, true, false, true, false]);
    }
}
