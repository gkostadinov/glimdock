//! Explicit administrator-owned native agent → Proxmox guest memory bindings.
//! A publisher cannot choose a VM ID. Only configured authenticated feeds are
//! considered, and stale/offline/stopped guests never fall back to host usage.
use crate::config::Config;
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestMemorySource {
    pub node_id: String,
    pub guest_id: u64,
    pub agent_node_id: String,
}

pub fn validate(c: &Config) -> Result<()> {
    ensure!(
        c.guest_memory_sources.len() <= 48,
        "Too many native guest memory bindings"
    );
    let mut targets = HashSet::new();
    let mut publishers = HashSet::new();
    for b in &c.guest_memory_sources {
        let local = b.node_id == c.local_node_id() && c.local_type == "proxmox";
        let remote = c
            .remote_collectors
            .iter()
            .any(|n| format!("remote:{}", n.id) == b.node_id && n.node_type == "proxmox");
        let source = c.push_agents.iter().any(|a| a.id == b.agent_node_id)
            || c.remote_collectors
                .iter()
                .any(|n| format!("remote:{}", n.id) == b.agent_node_id && n.node_type == "server");
        ensure!(
            (local || remote)
                && source
                && (100..=999999999).contains(&b.guest_id)
                && targets.insert((b.node_id.clone(), b.guest_id))
                && publishers.insert(b.agent_node_id.clone()),
            "Invalid or duplicate native guest memory binding"
        );
        ensure!(
            !local
                || (!c.qga_guest_ids.contains(&b.guest_id)
                    && (c.truenas_ssh_host.is_empty() || c.truenas_guest_id != b.guest_id)),
            "Guest memory must have a single source"
        );
    }
    Ok(())
}

fn ttl(c: &Config, id: &str) -> f64 {
    c.push_agents
        .iter()
        .find(|a| a.id == id)
        .map(|a| a.ttl_s)
        .or_else(|| {
            c.remote_collectors
                .iter()
                .find(|n| format!("remote:{}", n.id) == id)
                .map(|n| n.ttl_s)
        })
        .unwrap_or(0.)
}

fn memory(source: &Value, ttl: f64, now: f64) -> Result<(Value, f64)> {
    let s = &source["snapshot"];
    let state = &s["sources"]["memory"];
    let stamp = state["updated_at"]
        .as_f64()
        .filter(|t| t.is_finite() && *t > 0. && *t <= now + 30. && now - *t <= ttl);
    ensure!(
        state["ok"] == true && state["enabled"] != false && stamp.is_some(),
        "Native memory probe expired or unavailable"
    );
    for feed in ["agent_push", "remote_feed"] {
        if let Some(f) = s["sources"].get(feed) {
            ensure!(f["ok"] == true, "Native agent feed expired or unavailable");
        }
    }
    let h = &s["host"];
    let total = h["mem_total_bytes"].as_u64().filter(|v| *v > 0);
    let used = h["mem_used_bytes"].as_u64();
    ensure!(
        total.zip(used).is_some_and(|(t, u)| u <= t),
        "Invalid native memory accounting"
    );
    let (total, used) = (total.unwrap(), used.unwrap());
    let mac = source["platform"] == "macos";
    let mut m = json!({"mem_total_bytes":total,"mem_used_bytes":used,"mem_available_bytes":total-used,
        "memory_basis":if mac {"guest-os-macos"} else {"guest-os"},"memory_source_node":source["id"]});
    if mac {
        let app = h["mem_app_bytes"].as_u64();
        let wired = h["mem_wired_bytes"].as_u64();
        let compressed = h["mem_compressed_bytes"].as_u64();
        ensure!(
            h["memory_basis"] == "macos-native"
                && app
                    .zip(wired)
                    .zip(compressed)
                    .and_then(|((a, w), z)| a.checked_add(w).and_then(|v| v.checked_add(z)))
                    == Some(used),
            "Native macOS accounting unavailable"
        );
        for field in [
            "mem_app_bytes",
            "mem_wired_bytes",
            "mem_compressed_bytes",
            "mem_cache_bytes",
        ] {
            m[field] = json!(h[field].as_u64().filter(|v| *v <= total));
        }
        m["memory_pressure"] = json!(h["memory_pressure"]
            .as_str()
            .filter(|v| matches!(*v, "normal" | "warning" | "critical")));
    }
    let swap = h["swap_total_bytes"]
        .as_u64()
        .zip(h["swap_used_bytes"].as_u64())
        .filter(|(t, u)| u <= t);
    m["swap_total_bytes"] = json!(swap.map(|v| v.0));
    m["swap_used_bytes"] = json!(swap.map(|v| v.1));
    Ok((m, stamp.unwrap()))
}

pub fn apply(nodes: &mut [Value], c: &Config, now: f64) {
    for b in &c.guest_memory_sources {
        let source = nodes.iter().find(|n| n["id"] == b.agent_node_id);
        let is_mac = source.is_some_and(|n| n["platform"] == "macos");
        let sample = source.map(|s| memory(s, ttl(c, &b.agent_node_id), now));
        let Some(node) = nodes.iter_mut().find(|n| n["id"] == b.node_id) else {
            continue;
        };
        let pve_ok = node["snapshot"]["sources"]["proxmox"]["ok"] == true
            && node["snapshot"]["sources"]
                .get("remote_feed")
                .is_none_or(|s| s["ok"] == true);
        let Some(guests) = node["snapshot"]["guests"].as_array_mut() else {
            continue;
        };
        let Some(g) = guests.iter_mut().find(|g| g["id"] == b.guest_id) else {
            continue;
        };
        for field in [
            "mem_used_bytes",
            "mem_total_bytes",
            "mem_available_bytes",
            "mem_cache_bytes",
            "mem_noncache_used_bytes",
            "mem_app_bytes",
            "mem_wired_bytes",
            "mem_compressed_bytes",
            "memory_pressure",
            "swap_used_bytes",
            "swap_total_bytes",
            "mem_updated_at",
            "mem_age_s",
        ] {
            g[field] = Value::Null;
        }
        g["memory_basis"] = json!(if is_mac { "guest-os-macos" } else { "guest-os" });
        g["memory_source_node"] = json!(b.agent_node_id);
        let error = if g["status"] != "running" {
            Some("Guest is not running".to_owned())
        } else if !pve_ok {
            Some("Proxmox guest state unavailable".to_owned())
        } else {
            match sample {
                Some(Ok((m, stamp))) => {
                    g.as_object_mut()
                        .unwrap()
                        .extend(m.as_object().unwrap().clone());
                    g["mem_updated_at"] = json!(stamp);
                    g["mem_age_s"] = json!((now - stamp).max(0.));
                    None
                }
                Some(Err(e)) => Some(e.to_string()),
                None => Some("Native guest agent unavailable".to_owned()),
            }
        };
        g["mem_error"] = json!(error);
    }
}
