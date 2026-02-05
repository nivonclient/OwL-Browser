use scheduler::MemoryPressure;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

/// Source abstraction for memory pressure.
///
/// Implementations must be cheap to call and should not allocate per sample.
pub trait MemoryPressureSource {
    /// Samples current memory pressure. Returns `None` if the source is unavailable.
    fn sample(&mut self, thresholds: &MemoryPressureThresholds) -> Option<MemoryPressureReading>;
}

/// Pressure thresholds expressed as headroom per-mille (0-1000).
///
/// Rationale:
/// - Moderate starts below 20% headroom. This is where Linux reclaim activity is common.
/// - Severe starts below 10% headroom. This is a conservative guard for imminent pressure.
#[derive(Debug, Clone, Copy)]
pub struct MemoryPressureThresholds {
    pub moderate_headroom_per_mille: u16,
    pub severe_headroom_per_mille: u16,
}

impl Default for MemoryPressureThresholds {
    fn default() -> Self {
        Self {
            moderate_headroom_per_mille: 200,
            severe_headroom_per_mille: 100,
        }
    }
}

/// Polling and smoothing configuration for the memory pressure monitor.
#[derive(Debug, Clone, Copy)]
pub struct MemoryPressureMonitorConfig {
    /// Polling cadence for the source worker.
    pub sample_interval: Duration,
    /// Minimum time a pressure level must persist before being lowered.
    pub monotonic_window: Duration,
}

impl Default for MemoryPressureMonitorConfig {
    fn default() -> Self {
        Self {
            sample_interval: Duration::from_secs(1),
            monotonic_window: Duration::from_secs(3),
        }
    }
}

/// Result of a memory pressure sample.
#[derive(Debug, Clone, Copy)]
pub struct MemoryPressureReading {
    pub pressure: MemoryPressure,
    pub headroom_per_mille: u16,
    pub source: MemoryPressureSourceKind,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum MemoryPressureSourceKind {
    CgroupV2,
    SystemMemInfo,
    ProcessRss,
}

/// Background monitor that emits memory pressure updates via a channel.
///
/// The worker performs blocking I/O off the UI thread. The receiver can be
/// polled non-blockingly from the main loop.
pub struct MemoryPressureReceiver {
    receiver: Receiver<MemoryPressure>,
}

impl MemoryPressureReceiver {
    pub fn start<S: MemoryPressureSource + Send + 'static>(
        mut source: S,
        thresholds: MemoryPressureThresholds,
        config: MemoryPressureMonitorConfig,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut smoother = PressureSmoother::new(config.monotonic_window);
            loop {
                if let Some(reading) = source.sample(&thresholds) {
                    let now = Instant::now();
                    let pressure = smoother.filter(reading.pressure, now);
                    if sender.send(pressure).is_err() {
                        break;
                    }
                }
                thread::sleep(config.sample_interval);
            }
        });

        Self { receiver }
    }

    /// Returns the latest pressure update if one is available.
    pub fn drain_latest(&self) -> Option<MemoryPressure> {
        let mut latest = None;
        while let Ok(pressure) = self.receiver.try_recv() {
            latest = Some(pressure);
        }
        latest
    }
}

/// Composite source that prefers cgroup v2, then system meminfo, then RSS.
#[derive(Debug)]
pub struct DefaultMemoryPressureSource {
    cgroup: Option<CgroupV2Source>,
    system: SystemMemSource,
    rss: ProcessRssSource,
}

impl DefaultMemoryPressureSource {
    pub fn new() -> Self {
        Self {
            cgroup: CgroupV2Source::new(),
            system: SystemMemSource::new(),
            rss: ProcessRssSource::new(),
        }
    }
}

impl Default for DefaultMemoryPressureSource {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryPressureSource for DefaultMemoryPressureSource {
    fn sample(&mut self, thresholds: &MemoryPressureThresholds) -> Option<MemoryPressureReading> {
        if let Some(source) = self.cgroup.as_mut() {
            if let Some(reading) = source.sample(thresholds) {
                return Some(reading);
            }
        }

        if let Some(reading) = self.system.sample(thresholds) {
            return Some(reading);
        }

        self.rss.sample(thresholds)
    }
}

#[derive(Debug)]
struct CgroupV2Source {
    memory_max: PathBuf,
    memory_current: PathBuf,
    buffer_max: Vec<u8>,
    buffer_current: Vec<u8>,
}

impl CgroupV2Source {
    fn new() -> Option<Self> {
        let cgroup_path = cgroup_v2_path()?;
        Some(Self {
            memory_max: cgroup_path.join("memory.max"),
            memory_current: cgroup_path.join("memory.current"),
            buffer_max: Vec::with_capacity(128),
            buffer_current: Vec::with_capacity(128),
        })
    }
}

impl MemoryPressureSource for CgroupV2Source {
    fn sample(&mut self, thresholds: &MemoryPressureThresholds) -> Option<MemoryPressureReading> {
        let limit =
            read_u64_from_file(&self.memory_max, &mut self.buffer_max).ok().flatten()?;
        if limit == 0 {
            return None;
        }
        let current =
            read_u64_from_file(&self.memory_current, &mut self.buffer_current).ok().flatten()?;
        let headroom = limit.saturating_sub(current);
        let headroom_per_mille = headroom_per_mille(headroom, limit)?;
        Some(MemoryPressureReading {
            pressure: map_headroom_per_mille(headroom_per_mille, thresholds),
            headroom_per_mille,
            source: MemoryPressureSourceKind::CgroupV2,
        })
    }
}

#[derive(Debug)]
struct SystemMemSource {
    buffer: Vec<u8>,
}

impl SystemMemSource {
    fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(4096),
        }
    }
}

impl MemoryPressureSource for SystemMemSource {
    fn sample(&mut self, thresholds: &MemoryPressureThresholds) -> Option<MemoryPressureReading> {
        let (total, available) = read_meminfo(&mut self.buffer)?;
        let headroom_per_mille = headroom_per_mille(available, total)?;
        Some(MemoryPressureReading {
            pressure: map_headroom_per_mille(headroom_per_mille, thresholds),
            headroom_per_mille,
            source: MemoryPressureSourceKind::SystemMemInfo,
        })
    }
}

#[derive(Debug)]
struct ProcessRssSource {
    buffer: Vec<u8>,
    meminfo_buffer: Vec<u8>,
}

impl ProcessRssSource {
    fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(2048),
            meminfo_buffer: Vec::with_capacity(4096),
        }
    }
}

impl MemoryPressureSource for ProcessRssSource {
    fn sample(&mut self, thresholds: &MemoryPressureThresholds) -> Option<MemoryPressureReading> {
        let total = read_meminfo_total(&mut self.meminfo_buffer)?;
        let rss = read_vm_rss(&mut self.buffer)?;
        let headroom = total.saturating_sub(rss);
        let headroom_per_mille = headroom_per_mille(headroom, total)?;
        Some(MemoryPressureReading {
            pressure: map_headroom_per_mille(headroom_per_mille, thresholds),
            headroom_per_mille,
            source: MemoryPressureSourceKind::ProcessRss,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct PressureSmoother {
    last: MemoryPressure,
    last_change: Instant,
    monotonic_window: Duration,
}

impl PressureSmoother {
    fn new(monotonic_window: Duration) -> Self {
        let now = Instant::now();
        Self {
            last: MemoryPressure::Low,
            last_change: now,
            monotonic_window,
        }
    }

    fn filter(&mut self, next: MemoryPressure, now: Instant) -> MemoryPressure {
        // Avoid short-interval flapping by only allowing decreases after the window.
        if pressure_rank(next) > pressure_rank(self.last) {
            self.last = next;
            self.last_change = now;
            return next;
        }

        if pressure_rank(next) < pressure_rank(self.last)
            && now.duration_since(self.last_change) < self.monotonic_window
        {
            return self.last;
        }

        self.last = next;
        self.last_change = now;
        next
    }
}

fn pressure_rank(pressure: MemoryPressure) -> u8 {
    match pressure {
        MemoryPressure::Low => 0,
        MemoryPressure::Moderate => 1,
        MemoryPressure::Severe => 2,
    }
}

fn map_headroom_per_mille(
    headroom_per_mille: u16,
    thresholds: &MemoryPressureThresholds,
) -> MemoryPressure {
    if headroom_per_mille <= thresholds.severe_headroom_per_mille {
        MemoryPressure::Severe
    } else if headroom_per_mille <= thresholds.moderate_headroom_per_mille {
        MemoryPressure::Moderate
    } else {
        MemoryPressure::Low
    }
}

fn headroom_per_mille(available: u64, total: u64) -> Option<u16> {
    if total == 0 {
        return None;
    }
    let ratio = available.saturating_mul(1000) / total;
    Some(ratio.min(1000) as u16)
}

fn read_to_buffer<'a>(path: &Path, buffer: &'a mut Vec<u8>) -> io::Result<&'a [u8]> {
    buffer.clear();
    let mut file = File::open(path)?;
    file.read_to_end(buffer)?;
    Ok(buffer.as_slice())
}

fn read_u64_from_file(path: &Path, buffer: &mut Vec<u8>) -> io::Result<Option<u64>> {
    let bytes = read_to_buffer(path, buffer)?;
    parse_cgroup_value(bytes)
}

fn parse_cgroup_value(bytes: &[u8]) -> io::Result<Option<u64>> {
    let mut value: u64 = 0;
    let mut saw_digit = false;
    for byte in bytes.iter().copied() {
        if byte.is_ascii_digit() {
            saw_digit = true;
            value = value
                .saturating_mul(10)
                .saturating_add((byte - b'0') as u64);
        } else if byte == b'm' || byte == b'M' {
            return Ok(None);
        } else if saw_digit {
            break;
        }
    }
    if saw_digit {
        Ok(Some(value))
    } else {
        Ok(None)
    }
}

fn cgroup_v2_path() -> Option<PathBuf> {
    let mut buffer = Vec::with_capacity(256);
    let bytes = read_to_buffer(Path::new("/proc/self/cgroup"), &mut buffer).ok()?;
    for line in bytes.split(|b| *b == b'\n') {
        if line.starts_with(b"0::") {
            let path = &line[3..];
            let path = if path.is_empty() { "/" } else { std::str::from_utf8(path).ok()? };
            return Some(PathBuf::from("/sys/fs/cgroup").join(path));
        }
    }
    None
}

fn read_meminfo(buffer: &mut Vec<u8>) -> Option<(u64, u64)> {
    let bytes = read_to_buffer(Path::new("/proc/meminfo"), buffer).ok()?;
    let mut total: Option<u64> = None;
    let mut available: Option<u64> = None;

    for line in bytes.split(|b| *b == b'\n') {
        if total.is_none() && line.starts_with(b"MemTotal:") {
            total = parse_kb_value(line).map(|v| v.saturating_mul(1024));
        } else if available.is_none() && line.starts_with(b"MemAvailable:") {
            available = parse_kb_value(line).map(|v| v.saturating_mul(1024));
        }

        if total.is_some() && available.is_some() {
            break;
        }
    }

    Some((total?, available?))
}

fn read_meminfo_total(buffer: &mut Vec<u8>) -> Option<u64> {
    let bytes = read_to_buffer(Path::new("/proc/meminfo"), buffer).ok()?;
    for line in bytes.split(|b| *b == b'\n') {
        if line.starts_with(b"MemTotal:") {
            return parse_kb_value(line).map(|v| v.saturating_mul(1024));
        }
    }
    None
}

fn read_vm_rss(buffer: &mut Vec<u8>) -> Option<u64> {
    let bytes = read_to_buffer(Path::new("/proc/self/status"), buffer).ok()?;
    for line in bytes.split(|b| *b == b'\n') {
        if line.starts_with(b"VmRSS:") {
            return parse_kb_value(line).map(|v| v.saturating_mul(1024));
        }
    }
    None
}

fn parse_kb_value(line: &[u8]) -> Option<u64> {
    let mut value: u64 = 0;
    let mut saw_digit = false;
    for byte in line.iter().copied() {
        if byte.is_ascii_digit() {
            saw_digit = true;
            value = value
                .saturating_mul(10)
                .saturating_add((byte - b'0') as u64);
        } else if saw_digit {
            break;
        }
    }
    if saw_digit {
        Some(value)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_headroom_to_pressure() {
        let thresholds = MemoryPressureThresholds {
            moderate_headroom_per_mille: 200,
            severe_headroom_per_mille: 100,
        };

        assert_eq!(map_headroom_per_mille(250, &thresholds), MemoryPressure::Low);
        assert_eq!(map_headroom_per_mille(150, &thresholds), MemoryPressure::Moderate);
        assert_eq!(map_headroom_per_mille(100, &thresholds), MemoryPressure::Severe);
        assert_eq!(map_headroom_per_mille(5, &thresholds), MemoryPressure::Severe);
    }
}
