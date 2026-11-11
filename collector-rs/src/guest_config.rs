//! Proxmox guest CPU configuration: pinned host threads and virtual CPU count.
//!
//! The configuration is read with the same fixed, read-only `pvesh get`
//! requests the collector already uses, one guest at a time, and is cached.
//! Configurations change rarely, so a guest is read at most once a minute and a
//! slow or failing guest cannot hold up the others or the live guest inventory.
use crate::{
    cpulist::parse_cpu_list,
    probes::{command, monotonic, num},
};
use anyhow::{bail, Result};
use serde_json::{json, Map, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

/// A guest's configuration is refreshed no more often than this.
pub const REFRESH_S: f64 = 60.;
/// A configuration that can no longer be read is trusted for this long, then
/// dropped so the guest's fields become unknown rather than stale.
pub const KEEP_S: f64 = 600.;
/// Guests read per run. Each read has its own timeout, so a run stays bounded
/// and the first reads after a start are spread over a few polls.
pub const READS_PER_RUN: usize = 4;
/// A pinned guest may name at most this many host threads; a longer list is
/// treated as unreadable rather than silently cut.
pub const MAX_AFFINITY: usize = 256;
/// Upper bound for a plausible virtual CPU count.
const MAX_VCPUS: u32 = 4096;
const RESPONSE_BOUND: usize = 128 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Qemu,
    Lxc,
}
impl Kind {
    pub fn api(self) -> &'static str {
        match self {
            Kind::Qemu => "qemu",
            Kind::Lxc => "lxc",
        }
    }
    /// The snapshot's guest `type` (`vm` or `lxc`).
    pub fn from_guest_type(kind: &str) -> Option<Self> {
        match kind {
            "vm" => Some(Kind::Qemu),
            "lxc" => Some(Kind::Lxc),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Affinity {
    /// The option was missing, malformed or too large: not known.
    Unknown,
    /// The guest may run on any host thread.
    Unpinned,
    /// Sorted, unique host logical CPU numbers.
    Pinned(Vec<u32>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuestConfig {
    pub affinity: Affinity,
    pub cpu_count: Option<u32>,
}
impl GuestConfig {
    /// Snapshot fields. `cpu_affinity` is `null` for an unpinned guest and
    /// absent when unknown; `cpu_count` is absent when unknown.
    pub fn fields(&self) -> Value {
        let mut fields = Map::new();
        match &self.affinity {
            Affinity::Unknown => {}
            Affinity::Unpinned => {
                fields.insert("cpu_affinity".into(), Value::Null);
            }
            Affinity::Pinned(cpus) => {
                fields.insert("cpu_affinity".into(), json!(cpus));
            }
        }
        if let Some(count) = self.cpu_count {
            fields.insert("cpu_count".into(), json!(count));
        }
        Value::Object(fields)
    }
}

fn whole(value: &Value) -> Option<u32> {
    num(value)
        .filter(|v| v.fract() == 0. && *v >= 1. && *v <= f64::from(MAX_VCPUS))
        .map(|v| v as u32)
}

fn affinity(config: &Value) -> Affinity {
    let text = match &config["affinity"] {
        Value::Null => return Affinity::Unpinned,
        Value::String(text) => text.clone(),
        Value::Number(number) if number.is_u64() => number.to_string(),
        _ => return Affinity::Unknown,
    };
    if text.trim().is_empty() {
        return Affinity::Unpinned;
    }
    match parse_cpu_list(&text) {
        Ok(cpus) if cpus.is_empty() => Affinity::Unpinned,
        Ok(cpus) if cpus.len() <= MAX_AFFINITY => Affinity::Pinned(cpus),
        _ => Affinity::Unknown,
    }
}

/// Reduce one `pvesh get .../config` document to the CPU fields. Only these
/// keys are read; nothing else of the configuration is retained.
///
/// QEMU: `affinity` is the pinned host threads, and the CPU count is `vcpus`
/// when set, else `cores` times `sockets` (each defaulting to one). LXC has no
/// affinity option, so its guests are unpinned; the count is `cores` when set.
pub fn parse_config(kind: Kind, config: &Value) -> GuestConfig {
    match kind {
        Kind::Qemu => {
            let count = if config.get("vcpus").is_some_and(|v| !v.is_null()) {
                whole(&config["vcpus"])
            } else {
                let field = |name: &str| {
                    if config.get(name).is_some_and(|v| !v.is_null()) {
                        whole(&config[name])
                    } else {
                        Some(1)
                    }
                };
                field("cores")
                    .zip(field("sockets"))
                    .map(|(cores, sockets)| cores * sockets)
                    .filter(|n| *n <= MAX_VCPUS)
            };
            GuestConfig {
                affinity: affinity(config),
                cpu_count: count,
            }
        }
        Kind::Lxc => GuestConfig {
            affinity: Affinity::Unpinned,
            cpu_count: config.get("cores").and_then(whole),
        },
    }
}

/// Fixed read-only request for one guest's configuration. No shell, a timeout,
/// and a bound on the response; the node name and ID never reach a shell.
pub fn read_config(node: &str, kind: Kind, vmid: u64) -> Result<Value> {
    if !(1..=999_999_999).contains(&vmid) {
        bail!("invalid guest ID");
    }
    let path = format!("/nodes/{node}/{}/{vmid}/config", kind.api());
    let output = command(
        "pvesh",
        &["get", &path, "--output-format", "json"],
        5.,
        false,
    )?;
    if output.stdout.len() > RESPONSE_BOUND {
        bail!("Guest configuration response exceeds bound");
    }
    let document: Value = serde_json::from_str(&output.stdout)?;
    if !document.is_object() {
        bail!("Guest configuration response is not an object");
    }
    Ok(document)
}

/// The guests present on the Proxmox node, written by the collector from the
/// live inventory and read by the configuration source.
pub type Roster = Arc<Mutex<Vec<(u64, Kind)>>>;

/// Record the live inventory's guests for the configuration source.
pub fn publish_roster(roster: &Roster, guests: &[Value]) {
    let list = guests
        .iter()
        .filter_map(|g| {
            Some((
                g["id"].as_u64()?,
                Kind::from_guest_type(g["type"].as_str()?)?,
            ))
        })
        .collect();
    if let Ok(mut current) = roster.lock() {
        *current = list;
    }
}

struct Entry {
    kind: Kind,
    attempted: f64,
    good: Option<(f64, GuestConfig)>,
}

pub struct GuestConfigReader {
    node: String,
    roster: Roster,
    cache: BTreeMap<u64, Entry>,
}
impl GuestConfigReader {
    pub fn new(node: String, roster: Roster) -> Self {
        Self {
            node,
            roster,
            cache: BTreeMap::new(),
        }
    }
    pub fn read(&mut self) -> Result<Value> {
        let node = self.node.clone();
        self.read_with(monotonic(), &mut |kind, id| read_config(&node, kind, id))
    }
    /// One run: refresh the guests that are due (oldest attempt first, at most
    /// [`READS_PER_RUN`]) and report every guest whose configuration is known.
    /// `error` is set only when guests exist and none of them could be read.
    pub fn read_with<F>(&mut self, mono: f64, fetch: &mut F) -> Result<Value>
    where
        F: FnMut(Kind, u64) -> Result<Value>,
    {
        let roster = self
            .roster
            .lock()
            .map_err(|_| anyhow::anyhow!("Guest roster unavailable"))?
            .clone();
        self.cache.retain(|id, e| {
            roster
                .iter()
                .any(|(rid, kind)| rid == id && *kind == e.kind)
        });
        let mut due = roster
            .iter()
            .filter(|(id, kind)| {
                self.cache
                    .get(id)
                    .is_none_or(|e| e.kind != *kind || mono - e.attempted >= REFRESH_S)
            })
            .map(|(id, kind)| {
                (
                    self.cache
                        .get(id)
                        .map_or(f64::NEG_INFINITY, |e| e.attempted),
                    *id,
                    *kind,
                )
            })
            .collect::<Vec<_>>();
        due.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        for (_, id, kind) in due.into_iter().take(READS_PER_RUN) {
            let parsed = fetch(kind, id).map(|document| parse_config(kind, &document));
            let entry = self.cache.entry(id).or_insert(Entry {
                kind,
                attempted: mono,
                good: None,
            });
            entry.attempted = mono;
            if let Ok(config) = parsed {
                entry.good = Some((mono, config));
            }
        }
        let mut configs = Map::new();
        for (id, _) in &roster {
            if let Some((at, config)) = self.cache.get(id).and_then(|e| e.good.as_ref()) {
                if mono - at <= KEEP_S {
                    configs.insert(id.to_string(), config.fields());
                }
            }
        }
        let error =
            (!roster.is_empty() && configs.is_empty()).then_some("Guest configuration unavailable");
        Ok(json!({"configs":configs,"error":error}))
    }
}

/// Merge the cached CPU fields into the snapshot's guests. A guest without a
/// known configuration is left exactly as it was.
pub fn apply(guests: &mut [Value], configs: &Value) {
    let Some(configs) = configs.as_object() else {
        return;
    };
    for guest in guests {
        let Some(fields) = guest["id"]
            .as_u64()
            .and_then(|id| configs.get(&id.to_string()))
            .and_then(Value::as_object)
        else {
            continue;
        };
        for (key, value) in fields {
            guest[key] = value.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    fn qemu(config: Value) -> GuestConfig {
        parse_config(Kind::Qemu, &config)
    }

    #[test]
    fn pinned_threads_come_from_the_guest_affinity_sorted_and_unique() {
        let c = qemu(json!({"name":"vm-a","cores":4,"sockets":1,"affinity":"20-23,4-7,5"}));
        assert_eq!(
            c.affinity,
            Affinity::Pinned(vec![4, 5, 6, 7, 20, 21, 22, 23])
        );
        assert_eq!(
            c.fields()["cpu_affinity"],
            json!([4, 5, 6, 7, 20, 21, 22, 23])
        );
        let single = qemu(json!({"affinity":"2"}));
        assert_eq!(single.affinity, Affinity::Pinned(vec![2]));
        let numeric = qemu(json!({"affinity":7}));
        assert_eq!(numeric.affinity, Affinity::Pinned(vec![7]));
    }

    #[test]
    fn unpinned_guest_reports_null_not_missing() {
        for config in [
            json!({"cores":2}),
            json!({"cores":2,"affinity":null}),
            json!({"affinity":""}),
            json!({"affinity":"  "}),
        ] {
            let c = qemu(config);
            assert_eq!(c.affinity, Affinity::Unpinned);
            let fields = c.fields();
            assert!(fields.as_object().unwrap().contains_key("cpu_affinity"));
            assert!(fields["cpu_affinity"].is_null());
        }
    }

    #[test]
    fn malformed_or_oversized_affinity_is_unknown_never_unpinned() {
        for bad in ["4-", "a,b", "7-3", "1,,2", "0-4096"] {
            let c = qemu(json!({"cores":2,"affinity":bad}));
            assert_eq!(c.affinity, Affinity::Unknown, "{bad}");
            assert!(!c.fields().as_object().unwrap().contains_key("cpu_affinity"));
        }
        assert_eq!(qemu(json!({"affinity":true})).affinity, Affinity::Unknown);
        assert_eq!(qemu(json!({"affinity":-3})).affinity, Affinity::Unknown);
        let too_many = format!("0-{}", MAX_AFFINITY);
        assert_eq!(
            qemu(json!({"affinity":too_many})).affinity,
            Affinity::Unknown
        );
        let just_fits = format!("0-{}", MAX_AFFINITY - 1);
        assert!(matches!(
            qemu(json!({"affinity":just_fits})).affinity,
            Affinity::Pinned(c) if c.len() == MAX_AFFINITY
        ));
    }

    #[test]
    fn cpu_count_is_vcpus_when_set_else_cores_times_sockets() {
        assert_eq!(qemu(json!({"cores":4,"sockets":2})).cpu_count, Some(8));
        assert_eq!(qemu(json!({"cores":4})).cpu_count, Some(4));
        assert_eq!(qemu(json!({"sockets":2})).cpu_count, Some(2));
        assert_eq!(qemu(json!({})).cpu_count, Some(1), "Proxmox defaults");
        assert_eq!(
            qemu(json!({"cores":8,"sockets":1,"vcpus":3})).cpu_count,
            Some(3)
        );
        assert_eq!(
            qemu(json!({"cores":"6","sockets":"1"})).cpu_count,
            Some(6),
            "numeric strings"
        );
        for bad in [
            json!({"cores":0}),
            json!({"cores":-2}),
            json!({"cores":2.5}),
            json!({"cores":"many"}),
            json!({"cores":4096,"sockets":2}),
            json!({"vcpus":0,"cores":4}),
        ] {
            assert_eq!(qemu(bad.clone()).cpu_count, None, "{bad}");
        }
    }

    #[test]
    fn lxc_has_no_affinity_and_counts_its_cores_when_limited() {
        let c = parse_config(
            Kind::Lxc,
            &json!({"hostname":"ct","cores":2,"affinity":"1-2"}),
        );
        assert_eq!(c.affinity, Affinity::Unpinned, "LXC has no affinity option");
        assert_eq!(c.cpu_count, Some(2));
        let unlimited = parse_config(Kind::Lxc, &json!({"hostname":"ct"}));
        assert_eq!(unlimited.cpu_count, None);
        assert!(!unlimited
            .fields()
            .as_object()
            .unwrap()
            .contains_key("cpu_count"));
        assert!(unlimited.fields()["cpu_affinity"].is_null());
    }

    #[test]
    fn nothing_but_cpu_fields_is_retained() {
        let c = qemu(
            json!({"cores":2,"cipassword":"hidden","description":"note","net0":"virtio=AA:BB"}),
        );
        let text = c.fields().to_string();
        assert!(!text.contains("hidden") && !text.contains("note") && !text.contains("AA:BB"));
    }

    fn roster(guests: &[(u64, Kind)]) -> Roster {
        Arc::new(Mutex::new(guests.to_vec()))
    }

    #[test]
    fn a_guest_is_read_at_most_once_a_minute() {
        let r = roster(&[(100, Kind::Qemu)]);
        let mut reader = GuestConfigReader::new("node".into(), r);
        let calls = Cell::new(0);
        let mut fetch = |_: Kind, _: u64| {
            calls.set(calls.get() + 1);
            Ok(json!({"cores":2,"affinity":"0-1"}))
        };
        for t in [0., 6., 30., 59.9] {
            let out = reader.read_with(t, &mut fetch).unwrap();
            assert_eq!(
                out["configs"]["100"]["cpu_affinity"],
                json!([0, 1]),
                "t={t}"
            );
        }
        assert_eq!(calls.get(), 1, "cached for the whole minute");
        reader.read_with(60., &mut fetch).unwrap();
        assert_eq!(calls.get(), 2);
        reader.read_with(119., &mut fetch).unwrap();
        assert_eq!(calls.get(), 2);
        reader.read_with(120., &mut fetch).unwrap();
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn a_failing_guest_does_not_stop_the_others_and_is_retried_each_minute() {
        let r = roster(&[(100, Kind::Qemu), (101, Kind::Qemu), (102, Kind::Lxc)]);
        let mut reader = GuestConfigReader::new("node".into(), r);
        let attempts = RefCell::new(BTreeMap::<u64, u32>::new());
        let mut fetch = |_: Kind, id: u64| {
            *attempts.borrow_mut().entry(id).or_default() += 1;
            if id == 101 {
                bail!("pvesh timed out")
            }
            Ok(json!({"cores":2}))
        };
        let out = reader.read_with(0., &mut fetch).unwrap();
        assert!(out["configs"].get("100").is_some());
        assert!(out["configs"].get("102").is_some());
        assert!(
            out["configs"].get("101").is_none(),
            "unreadable stays absent"
        );
        assert!(out["error"].is_null(), "some guests were read");
        reader.read_with(10., &mut fetch).unwrap();
        assert_eq!(
            attempts.borrow()[&101],
            1,
            "the failure also waits a minute"
        );
        reader.read_with(61., &mut fetch).unwrap();
        assert_eq!(attempts.borrow()[&101], 2);
        assert_eq!(attempts.borrow()[&100], 2);
    }

    #[test]
    fn last_good_configuration_is_kept_for_a_while_then_dropped() {
        let r = roster(&[(100, Kind::Qemu)]);
        let mut reader = GuestConfigReader::new("node".into(), r);
        let fail = Cell::new(false);
        let mut fetch = |_: Kind, _: u64| {
            if fail.get() {
                bail!("unavailable")
            }
            Ok(json!({"affinity":"3"}))
        };
        assert!(reader.read_with(0., &mut fetch).unwrap()["configs"]["100"].is_object());
        fail.set(true);
        let kept = reader.read_with(300., &mut fetch).unwrap();
        assert_eq!(kept["configs"]["100"]["cpu_affinity"], json!([3]));
        assert!(kept["error"].is_null());
        let gone = reader.read_with(KEEP_S + 1., &mut fetch).unwrap();
        assert!(gone["configs"].as_object().unwrap().is_empty());
        assert_eq!(gone["error"], "Guest configuration unavailable");
    }

    #[test]
    fn reads_per_run_are_bounded_and_oldest_first() {
        let guests = (100..110).map(|id| (id, Kind::Qemu)).collect::<Vec<_>>();
        let r = roster(&guests);
        let mut reader = GuestConfigReader::new("node".into(), r);
        let order = RefCell::new(Vec::new());
        let mut fetch = |_: Kind, id: u64| {
            order.borrow_mut().push(id);
            Ok(json!({"cores":1}))
        };
        reader.read_with(0., &mut fetch).unwrap();
        assert_eq!(order.borrow().len(), READS_PER_RUN);
        reader.read_with(6., &mut fetch).unwrap();
        reader.read_with(12., &mut fetch).unwrap();
        assert_eq!(
            *order.borrow(),
            (100..110).collect::<Vec<_>>(),
            "every guest read once, in turn"
        );
        reader.read_with(18., &mut fetch).unwrap();
        assert_eq!(order.borrow().len(), 10, "none due again within the minute");
    }

    #[test]
    fn departed_guests_are_forgotten_and_a_reused_id_is_read_afresh() {
        let r = roster(&[(100, Kind::Qemu), (101, Kind::Qemu)]);
        let mut reader = GuestConfigReader::new("node".into(), r.clone());
        let calls = Cell::new(0);
        let mut fetch = |kind: Kind, _: u64| {
            calls.set(calls.get() + 1);
            Ok(match kind {
                Kind::Qemu => json!({"cores":2}),
                Kind::Lxc => json!({"cores":1}),
            })
        };
        reader.read_with(0., &mut fetch).unwrap();
        *r.lock().unwrap() = vec![(101, Kind::Lxc)];
        let out = reader.read_with(1., &mut fetch).unwrap();
        assert!(out["configs"].get("100").is_none());
        assert_eq!(out["configs"]["101"]["cpu_count"], 1, "now a container");
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn no_guests_is_not_an_error() {
        let mut reader = GuestConfigReader::new("node".into(), roster(&[]));
        let out = reader
            .read_with(0., &mut |_, _| bail!("never called"))
            .unwrap();
        assert!(out["configs"].as_object().unwrap().is_empty());
        assert!(out["error"].is_null());
    }

    #[test]
    fn roster_is_built_from_the_live_inventory() {
        let r = roster(&[]);
        publish_roster(
            &r,
            &[
                json!({"id":100,"type":"vm"}),
                json!({"id":200,"type":"lxc"}),
                json!({"id":300,"type":"other"}),
                json!({"type":"vm"}),
            ],
        );
        assert_eq!(
            *r.lock().unwrap(),
            vec![(100, Kind::Qemu), (200, Kind::Lxc)]
        );
    }

    #[test]
    fn apply_adds_fields_only_to_guests_with_a_known_configuration() {
        let original = vec![
            json!({"id":100,"name":"a","mem_used_bytes":5}),
            json!({"id":101,"name":"b"}),
            json!({"id":102,"name":"c"}),
        ];
        let mut guests = original.clone();
        apply(
            &mut guests,
            &json!({"100":{"cpu_affinity":[4,5],"cpu_count":2},"102":{"cpu_affinity":null}}),
        );
        assert_eq!(guests[0]["cpu_affinity"], json!([4, 5]));
        assert_eq!(guests[0]["cpu_count"], 2);
        assert_eq!(guests[0]["mem_used_bytes"], 5, "existing fields stay");
        assert_eq!(guests[1], original[1], "unknown guest untouched");
        assert!(guests[2]["cpu_affinity"].is_null());
        assert!(guests[2].as_object().unwrap().contains_key("cpu_affinity"));
        assert!(guests[2].get("cpu_count").is_none());
    }

    #[test]
    fn snapshot_without_configuration_data_is_unchanged() {
        let original = vec![json!({"id":100,"name":"a"}), json!({"id":101,"name":"b"})];
        for configs in [
            json!({}),
            json!(null),
            json!([]),
            json!("x"),
            json!({"100":5}),
        ] {
            let mut guests = original.clone();
            apply(&mut guests, &configs);
            assert_eq!(guests, original, "{configs}");
        }
    }
}
