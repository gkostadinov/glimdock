//! Optional Apple silicon SMC temperature and IOReport package-power estimates.
//! macmon stays inside one dedicated thread; CoreFoundation handles never cross
//! threads. Only bounded scalar readings leave it, with measurement freshness.
use crate::protocol::source_status;
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
struct Reading {
    at: f64,
    received: Instant,
    cpu_w: Option<f64>,
    cpu_c: Option<f64>,
}
pub struct Hardware {
    latest: Arc<Mutex<Option<Reading>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

fn temperature(value: Option<f32>) -> Option<f64> {
    value
        .map(f64::from)
        .filter(|v| v.is_finite() && (-10. ..=130.).contains(v))
}
fn power(value: f32) -> Option<f64> {
    // macmon uses zero as the unavailable fallback. Never publish it as a
    // measured idle CPU power, including stalled legacy Energy Model counters.
    let v = f64::from(value);
    (v.is_finite() && v > 0. && v <= 1000.).then_some(v)
}
impl Default for Hardware {
    fn default() -> Self {
        Self::new()
    }
}
impl Hardware {
    pub fn new() -> Self {
        let latest = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let out = latest.clone();
        let stopped = stop.clone();
        let worker = thread::spawn(move || {
            let Ok(mut sampler) = macmon::Sampler::new() else {
                return;
            };
            while !stopped.load(Ordering::Relaxed) {
                let reading = sampler.get_metrics(1000).ok().map(|m| Reading {
                    at: crate::epoch(),
                    received: Instant::now(),
                    cpu_w: power(m.cpu_power),
                    cpu_c: temperature(m.temp.cpu_temp_avg),
                });
                if let Ok(mut slot) = out.lock() {
                    *slot = reading;
                }
                for _ in 0..20 {
                    if stopped.load(Ordering::Relaxed) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
        });
        Self {
            latest,
            stop,
            worker: Some(worker),
        }
    }
    pub fn apply(&self, now: f64, snapshot: &mut Value) {
        let reading = self
            .latest
            .lock()
            .ok()
            .and_then(|r| r.clone())
            .filter(|r| r.received.elapsed() <= Duration::from_secs(10));
        let stamp = reading.as_ref().map(|r| r.at).unwrap_or(now);
        let cpu_w = reading.as_ref().and_then(|r| r.cpu_w);
        let cpu_c = reading.as_ref().and_then(|r| r.cpu_c);
        snapshot["power"]["package_w"] = json!(cpu_w);
        snapshot["power"]["cpu_power_basis"] = json!("macos-ioreport-estimate");
        snapshot["sources"]["cpu_power"] = source_status(
            stamp,
            cpu_w
                .is_none()
                .then_some("Apple silicon power estimate unavailable"),
            true,
        );
        snapshot["sources"]["cpu_power"]["age_s"] = json!((now - stamp).max(0.));
        snapshot["sources"]["cpu_temperature"] = source_status(
            stamp,
            cpu_c.is_none().then_some("CPU temperature unavailable"),
            true,
        );
        snapshot["sources"]["cpu_temperature"]["age_s"] = json!((now - stamp).max(0.));
        if let Some(value) = cpu_c {
            snapshot["power"]["cpu_temp_c"] = json!(value);
        }
        for (id, name, kind, unit, value, source) in [
            (
                "macos/cpu-temperature",
                "CPU temperature",
                "temperature",
                "°C",
                cpu_c,
                "cpu_temperature",
            ),
            (
                "macos/cpu-power",
                "CPU package (estimate)",
                "power",
                "W",
                cpu_w,
                "cpu_power",
            ),
        ] {
            snapshot["sensors"].as_array_mut().unwrap().push(json!({"id":id,"name":name,"chip":"Apple silicon","kind":kind,"unit":unit,"value":value,"source":source,"updated_at":stamp,"age_s":(now-stamp).max(0.),"crit":null,"high":null,"alarm":false,"fault":false}));
        }
    }
}
impl Drop for Hardware {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unavailable_zero_and_invalid_hardware_are_unknown() {
        assert_eq!(power(0.), None);
        assert_eq!(power(-1.), None);
        assert_eq!(power(f32::NAN), None);
        assert_eq!(power(3.), Some(3.));
        assert_eq!(temperature(Some(300.)), None);
        assert_eq!(temperature(None), None);
    }
}
