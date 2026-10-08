//! Bounded seven-day journal/kernel fault history with duplicate suppression.
use crate::probes::{command, num, text, wall};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
const PATTERN: &str = r"segfault|general protection fault|invalid opcode|dumped core|core dumped|\[Hardware Error\]|Machine check events logged|Uncorrected.*(?:error|ECC)|Out of memory: Killed process|oom-kill:|soft lockup|hard LOCKUP|I/O error, dev|EXT4-fs error|XFS.*(?:corruption|Corruption)";
pub fn classify(message: &str) -> Option<&'static str> {
    for (pattern, kind) in [
        (r"(?i)segfault|SIGSEGV|signal 11", "segfault"),
        (r"(?i)general protection fault|invalid opcode", "trap"),
        (r"(?i)dumped core|core dumped", "crash"),
        (
            r"(?i)\[Hardware Error\]|Machine check events logged|Uncorrected.*(?:error|ECC)",
            "hardware",
        ),
        (r"(?i)Out of memory: Killed process|oom-kill:", "oom"),
        (r"(?i)soft lockup|hard LOCKUP", "lockup"),
        (r"(?i)I/O error, dev|EXT4-fs error|XFS.*corruption", "io"),
    ] {
        if regex::Regex::new(pattern).unwrap().is_match(message) {
            return Some(kind);
        }
    }
    None
}
pub fn event(message: &str, timestamp: f64, boot: &str, mono: Option<f64>) -> Option<Value> {
    let kind = classify(message)?;
    let boot = boot.replace('-', "");
    let display = regex::Regex::new(r"(?i)\b(?:ip|sp) [0-9a-f]{8,}\b")
        .unwrap()
        .replace_all(message, "");
    let display = display
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>();
    let display = display.split_whitespace().collect::<Vec<_>>().join(" ");
    let digest = Sha256::digest(format!("{boot}:{}:{message}", mono.unwrap_or(timestamp)));
    let id = format!("{digest:x}");
    Some(
        json!({"id":&id[..16],"timestamp":timestamp,"kind":kind,"message":text(&display,220),"_message":message,"_boot":boot,"_mono":mono}),
    )
}
pub fn parse_journal(input: &str) -> Vec<Value> {
    input
        .lines()
        .filter_map(|line| {
            let d = serde_json::from_str::<Value>(line).ok()?;
            let ts = num(&d["__REALTIME_TIMESTAMP"])? / 1e6;
            event(
                d["MESSAGE"].as_str()?,
                ts,
                d["_BOOT_ID"].as_str().unwrap_or(""),
                num(&d["__MONOTONIC_TIMESTAMP"]).map(|v| v / 1e6),
            )
        })
        .collect()
}
pub fn parse_dmesg(input: &str, now: f64, uptime: f64, boot: &str) -> Vec<Value> {
    let re = regex::Regex::new(r"^\[\s*(\d+(?:\.\d+)?)\]\s*(.*)").unwrap();
    input
        .lines()
        .filter_map(|line| {
            let m = re.captures(line)?;
            let mono = m[1].parse::<f64>().ok()?;
            event(&m[2], now - uptime + mono, boot, Some(mono))
        })
        .collect()
}
pub fn merge_events(journal: Vec<Value>, kernel: Vec<Value>, now: f64) -> Value {
    let mut events = journal.clone();
    for candidate in kernel {
        let duplicate=journal.iter().any(|e|e["_message"]==candidate["_message"]&&e["_boot"]==candidate["_boot"]&&matches!((num(&e["_mono"]),num(&candidate["_mono"])),(Some(a),Some(b)) if (a-b).abs()<2.));
        if !duplicate {
            events.push(candidate)
        }
    }
    events
        .retain(|e| num(&e["timestamp"]).is_some_and(|t| t >= now - 7. * 86400. && t <= now + 60.));
    events.sort_by(|a, b| {
        num(&b["timestamp"])
            .partial_cmp(&num(&a["timestamp"]))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let recent = events
        .iter()
        .filter(|e| num(&e["timestamp"]).is_some_and(|t| t >= now - 86400.))
        .collect::<Vec<_>>();
    let count = events.len();
    let last = events.first().map(|e| e["timestamp"].clone());
    let segfaults = recent.iter().filter(|e| e["kind"] == "segfault").count();
    let rec = recent.len();
    let clean = events
        .into_iter()
        .take(16)
        .map(|mut e| {
            e.as_object_mut()
                .unwrap()
                .retain(|k, _| !k.starts_with('_'));
            e
        })
        .collect::<Vec<_>>();
    json!({"lookback_days":7,"segfault_count_24h":segfaults,"event_count_24h":rec,"last_event_at":last,"history_event_count":count,"history_truncated":count.saturating_sub(16),"events":clean})
}
pub fn read_faults() -> anyhow::Result<Value> {
    let now = wall();
    let mut errors = vec![];
    let journal = match command(
        "journalctl",
        &[
            "--since",
            "-7 days",
            "--no-pager",
            "--output=json",
            "--grep",
            PATTERN,
            "--case-sensitive=no",
            "--lines=2000",
        ],
        8.,
        true,
    ) {
        Ok(r) if r.status == 0 => parse_journal(&r.stdout),
        _ => {
            errors.push("journal unavailable");
            vec![]
        }
    };
    let kernel = match command("dmesg", &["--color=never", "--time-format=raw"], 3., true) {
        Ok(r) if r.status == 0 => {
            let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id");
            let uptime = fs::read_to_string("/proc/uptime").ok().and_then(|s| {
                s.split_whitespace()
                    .next()
                    .and_then(|v| v.parse::<f64>().ok())
            });
            match (boot, uptime) {
                (Ok(b), Some(u)) => parse_dmesg(&r.stdout, now, u, b.trim()),
                _ => {
                    errors.push("kernel log unavailable");
                    vec![]
                }
            }
        }
        _ => {
            errors.push("kernel log unavailable");
            vec![]
        }
    };
    Ok(
        json!({"faults":merge_events(journal,kernel,now),"error":if errors.is_empty(){None}else{Some(errors.join("; "))}}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn benign_and_classifications() {
        assert_eq!(classify("PCI device error correction enabled"), None);
        assert_eq!(
            classify("foo segfault at 0 ip deadbeef sp deadbeef"),
            Some("segfault")
        );
        assert_eq!(
            classify("[Hardware Error] ECC uncorrected"),
            Some("hardware")
        );
        assert_eq!(classify("oom-kill: task=foo"), Some("oom"))
    }
    #[test]
    fn journal_dmesg_dedup_and_24h() {
        let now = 1000000.;
        let msg = "app[12]: segfault at 0 ip deadbeef sp deadbeef";
        let j = event(msg, now - 10., "a-b", Some(80.)).unwrap();
        let k = event(msg, now - 10., "ab", Some(80.5)).unwrap();
        let old = event("foo segfault", now - 90000., "old", Some(0.)).unwrap();
        let v = merge_events(vec![j, old], vec![k], now);
        assert_eq!(v["history_event_count"], 2);
        assert_eq!(v["segfault_count_24h"], 1);
        assert!(!v["events"][0]["message"]
            .as_str()
            .unwrap()
            .contains("deadbeef"));
        assert!(v["events"][0].get("_boot").is_none())
    }
}
