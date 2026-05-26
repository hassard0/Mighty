//! NUMA / socket-aware steal-order computation for the work-stealing
//! scheduler (Tier 5, v0.22).
//!
//! ## What this module does
//!
//! Each worker thread in the scheduler holds a `steal_order: Vec<WorkerId>`
//! — the order in which it will probe sibling stealers when its own
//! local deque is empty. v0.21 used a simple round-robin offset; v0.22
//! upgrades that to a **locality-preserving** order:
//!
//! 1. Workers on the **same NUMA node** are tried first.
//! 2. Then workers on the **same socket** but a different node.
//! 3. Then workers anywhere else.
//!
//! Steals across NUMA boundaries are expensive (cache-coherency miss,
//! cross-socket interconnect hop). On a 2-socket box with 24 cores per
//! socket, a cross-socket steal can be 3–10x slower than a within-node
//! steal. Preferring local first preserves cache warmth.
//!
//! ## Topology detection
//!
//! On Linux we read `/sys/devices/system/cpu/cpu*/topology/{physical_package_id,
//! core_id}` and `/sys/devices/system/node/node*/cpulist`. On Windows and
//! other platforms (or when `/sys` is unavailable inside a container)
//! we fall back to a **flat topology** — every worker reports as
//! `node=0, socket=0`, and the steal order degenerates to "anywhere".
//!
//! The fallback is safe: the work-stealing loop still functions, it
//! just doesn't get the locality-preserving micro-optimisation. The
//! crucial guarantee — *all tasks eventually run* — is preserved by
//! the "then anywhere else" final tier.
//!
//! ## Synthetic topologies
//!
//! For tests we expose [`Topology::synthetic`] which lets a unit test
//! manufacture a 4-worker / 2-node layout without touching `/sys`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// Topology slot for one worker: which NUMA node + socket it lives on.
///
/// `Default` reports `(node=0, socket=0)`, which is exactly the
/// "everyone is on the same node / socket" fallback used when topology
/// detection fails or the host has no NUMA hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WorkerLocality {
    /// NUMA node id. `0` on systems with no NUMA awareness.
    pub node: u32,
    /// Socket / physical package id. `0` when undetected.
    pub socket: u32,
}

/// Per-worker topology table. `topology.locals[i]` is the
/// [`WorkerLocality`] of worker `i`.
#[derive(Debug, Clone)]
pub struct Topology {
    pub locals: Vec<WorkerLocality>,
}

impl Topology {
    /// Detect topology for `n` workers. Tries Linux `/sys` first; on
    /// any failure (file missing, parse error, non-Linux platform)
    /// returns a flat topology with every worker on node 0, socket 0.
    pub fn detect(n: usize) -> Self {
        if let Some(t) = Self::detect_linux(n) {
            return t;
        }
        Self::flat(n)
    }

    /// Flat fallback — every worker has the same locality. Steal-order
    /// degenerates to a plain rotation, identical to the v0.21 behavior.
    pub fn flat(n: usize) -> Self {
        Self {
            locals: vec![WorkerLocality::default(); n],
        }
    }

    /// Build a synthetic topology from explicit (node, socket) pairs.
    /// Used by tests that want to assert steal-order preferences
    /// without depending on the host machine's actual topology.
    pub fn synthetic(slots: Vec<(u32, u32)>) -> Self {
        Self {
            locals: slots
                .into_iter()
                .map(|(node, socket)| WorkerLocality { node, socket })
                .collect(),
        }
    }

    /// Linux `/sys` parser. Returns `None` if the topology files
    /// aren't readable or yielded zero workers. Worker `i` is assigned
    /// to CPU `i mod online_cpus`; this matches the assumption that
    /// the work-stealing pool's worker count tracks
    /// `available_parallelism()` and so each worker maps to one CPU.
    /// When the pool is over-subscribed (`n > online_cpus`) workers
    /// wrap, and that's an honest reflection — the OS scheduler will
    /// timeshare those workers across the same CPUs anyway.
    fn detect_linux(n: usize) -> Option<Topology> {
        let cpu_root = Path::new("/sys/devices/system/cpu");
        if !cpu_root.exists() {
            return None;
        }
        // First pass: list CPU ids that have a topology dir.
        let mut cpu_ids: Vec<u32> = Vec::new();
        for entry in fs::read_dir(cpu_root).ok()? {
            let entry = entry.ok()?;
            let name = entry.file_name();
            let name = name.to_str()?;
            if let Some(rest) = name.strip_prefix("cpu") {
                if let Ok(id) = rest.parse::<u32>() {
                    let topo = entry.path().join("topology/physical_package_id");
                    if topo.exists() {
                        cpu_ids.push(id);
                    }
                }
            }
        }
        if cpu_ids.is_empty() {
            return None;
        }
        cpu_ids.sort_unstable();

        // Per-CPU (node, socket) lookup.
        let mut per_cpu: BTreeMap<u32, WorkerLocality> = BTreeMap::new();
        for &id in &cpu_ids {
            let cpu = cpu_root.join(format!("cpu{}", id));
            let socket = fs::read_to_string(cpu.join("topology/physical_package_id"))
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok())
                .unwrap_or(0);
            // NUMA node: walk /sys/devices/system/node and find which
            // node lists this CPU in its `cpulist`. Cheap because there
            // are typically <= 16 nodes.
            let node = numa_node_for_cpu(id).unwrap_or(0);
            per_cpu.insert(id, WorkerLocality { node, socket });
        }

        // Assign worker `i` to CPU `cpu_ids[i mod cpu_ids.len()]`.
        let locals = (0..n)
            .map(|i| {
                let cpu = cpu_ids[i % cpu_ids.len()];
                per_cpu.get(&cpu).copied().unwrap_or_default()
            })
            .collect();
        Some(Topology { locals })
    }
}

/// Walk `/sys/devices/system/node/node*/cpulist` to find which NUMA
/// node lists `cpu`. Returns `None` if no node claims it (rare —
/// typically only happens on systems without `numa` compiled in).
fn numa_node_for_cpu(cpu: u32) -> Option<u32> {
    let node_root = Path::new("/sys/devices/system/node");
    if !node_root.exists() {
        return None;
    }
    for entry in fs::read_dir(node_root).ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name();
        let name = name.to_str()?.to_string();
        let Some(node_id) = name
            .strip_prefix("node")
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(cpulist) = fs::read_to_string(entry.path().join("cpulist")) else {
            continue;
        };
        if parse_cpulist(&cpulist).contains(&cpu) {
            return Some(node_id);
        }
    }
    None
}

/// Parse the Linux "cpulist" format: comma-separated ranges, e.g.
/// `0-3,8,10-11` → `[0,1,2,3,8,10,11]`. Returns an empty vec on parse
/// failure; callers treat that as "this node lists no CPUs".
pub(crate) fn parse_cpulist(s: &str) -> Vec<u32> {
    let mut out = Vec::new();
    for chunk in s.trim().split(',') {
        let chunk = chunk.trim();
        if chunk.is_empty() {
            continue;
        }
        if let Some((lo, hi)) = chunk.split_once('-') {
            if let (Ok(lo), Ok(hi)) = (lo.parse::<u32>(), hi.parse::<u32>()) {
                if lo <= hi {
                    out.extend(lo..=hi);
                }
            }
        } else if let Ok(v) = chunk.parse::<u32>() {
            out.push(v);
        }
    }
    out
}

/// Compute the steal-order list for worker `self_id` against the given
/// `topology`. The output is a `Vec<usize>` of worker indices to try in
/// order. `self_id` is **never** included.
///
/// Tier 1: same NUMA node, same socket.
/// Tier 2: same socket, different node (NUMA-distant but socket-local).
/// Tier 3: different socket entirely.
///
/// Within each tier we order by `(id - self_id) mod n` so the rotation
/// pattern stays balanced — without this, every worker on a node would
/// always probe its neighbours in the same order and pile up contention
/// on whichever sibling held work first.
pub fn build_steal_order(self_id: usize, topology: &Topology) -> Vec<usize> {
    let n = topology.locals.len();
    if n <= 1 {
        return Vec::new();
    }
    let me = topology.locals[self_id];
    let mut tier_same_node: Vec<usize> = Vec::new();
    let mut tier_same_socket: Vec<usize> = Vec::new();
    let mut tier_anywhere: Vec<usize> = Vec::new();
    for offset in 1..n {
        let id = (self_id + offset) % n;
        let other = topology.locals[id];
        if other.node == me.node && other.socket == me.socket {
            tier_same_node.push(id);
        } else if other.socket == me.socket {
            tier_same_socket.push(id);
        } else {
            tier_anywhere.push(id);
        }
    }
    let mut out = Vec::with_capacity(n - 1);
    out.append(&mut tier_same_node);
    out.append(&mut tier_same_socket);
    out.append(&mut tier_anywhere);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_topology_size() {
        let t = Topology::flat(4);
        assert_eq!(t.locals.len(), 4);
        for l in &t.locals {
            assert_eq!(l.node, 0);
            assert_eq!(l.socket, 0);
        }
    }

    #[test]
    fn synthetic_round_trip() {
        let t = Topology::synthetic(vec![(0, 0), (0, 0), (1, 0), (1, 0)]);
        assert_eq!(t.locals.len(), 4);
        assert_eq!(t.locals[2].node, 1);
    }

    #[test]
    fn steal_order_excludes_self() {
        let t = Topology::flat(4);
        for id in 0..4 {
            let order = build_steal_order(id, &t);
            assert!(
                !order.contains(&id),
                "self {} appeared in own steal order",
                id
            );
            assert_eq!(order.len(), 3);
        }
    }

    #[test]
    fn steal_order_prefers_same_node() {
        // 4 workers: 0,1 on node 0 / socket 0; 2,3 on node 1 / socket 0.
        // Worker 0 should probe worker 1 *before* worker 2 or 3.
        let t = Topology::synthetic(vec![(0, 0), (0, 0), (1, 0), (1, 0)]);
        let order = build_steal_order(0, &t);
        assert_eq!(order.first().copied(), Some(1));
        // The two node-1 workers come after.
        assert!(order.contains(&2));
        assert!(order.contains(&3));
        let pos_1 = order.iter().position(|&x| x == 1).unwrap();
        let pos_2 = order.iter().position(|&x| x == 2).unwrap();
        let pos_3 = order.iter().position(|&x| x == 3).unwrap();
        assert!(pos_1 < pos_2, "same-node should precede different-node");
        assert!(pos_1 < pos_3, "same-node should precede different-node");
    }

    #[test]
    fn steal_order_prefers_same_socket_over_anywhere() {
        // 4 workers across two sockets, three nodes:
        //   w0: node=0, socket=0
        //   w1: node=1, socket=0  (same socket, different node)
        //   w2: node=2, socket=1  (different socket)
        //   w3: node=3, socket=1  (different socket)
        // From w0: order should be [1, then 2,3 in some rotation].
        let t = Topology::synthetic(vec![(0, 0), (1, 0), (2, 1), (3, 1)]);
        let order = build_steal_order(0, &t);
        assert_eq!(order.first().copied(), Some(1));
        let pos_1 = order.iter().position(|&x| x == 1).unwrap();
        let pos_2 = order.iter().position(|&x| x == 2).unwrap();
        let pos_3 = order.iter().position(|&x| x == 3).unwrap();
        assert!(pos_1 < pos_2);
        assert!(pos_1 < pos_3);
    }

    #[test]
    fn cpulist_parser_handles_ranges_and_singletons() {
        assert_eq!(parse_cpulist("0-3"), vec![0, 1, 2, 3]);
        assert_eq!(parse_cpulist("0-3,8,10-11"), vec![0, 1, 2, 3, 8, 10, 11]);
        assert_eq!(parse_cpulist(""), Vec::<u32>::new());
        assert_eq!(parse_cpulist("garbage"), Vec::<u32>::new());
    }

    #[test]
    fn empty_topology_yields_empty_order() {
        let t = Topology::flat(1);
        assert!(build_steal_order(0, &t).is_empty());
    }

    #[test]
    fn detect_returns_something_useful() {
        // `detect` must always return a topology of length n, even if
        // /sys is unavailable (we fall back to flat).
        let t = Topology::detect(8);
        assert_eq!(t.locals.len(), 8);
    }
}
