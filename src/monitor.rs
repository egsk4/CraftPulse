//! Sample only CraftPulse, every five seconds. No process-tree or system-wide scan.
use serde::Serialize;
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
#[derive(Clone, Default, Serialize)]
pub struct Sample {
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub uptime_seconds: u64,
    pub sampled: bool,
}
static SAMPLE: Mutex<Sample> = Mutex::new(Sample {
    cpu_percent: 0.0,
    memory_bytes: 0,
    uptime_seconds: 0,
    sampled: false,
});
pub fn sample() -> Sample {
    SAMPLE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}
pub fn start() {
    let _ = std::thread::Builder::new()
        .name("craftpulse-monitor".into())
        .spawn(|| {
            let Ok(pid) = sysinfo::get_current_pid() else {
                return;
            };
            let mut system = sysinfo::System::new();
            let start = Instant::now();
            let mut primed = false;
            loop {
                system.refresh_processes_specifics(
                    sysinfo::ProcessesToUpdate::Some(&[pid]),
                    true,
                    sysinfo::ProcessRefreshKind::nothing()
                        .with_cpu()
                        .with_memory(),
                );
                if let Some(process) = system.process(pid) {
                    *SAMPLE.lock().unwrap_or_else(|e| e.into_inner()) = Sample {
                        cpu_percent: process.cpu_usage(),
                        memory_bytes: process.memory(),
                        uptime_seconds: start.elapsed().as_secs(),
                        sampled: primed,
                    };
                }
                primed = true;
                std::thread::sleep(Duration::from_secs(5));
            }
        });
}
