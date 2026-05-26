//! Budget + sandbox enforcement (spec §16.2).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::error::RuntimeError;

#[derive(Debug, Clone, Default)]
pub struct Budget {
    pub cpu: Option<Duration>,
    pub wall: Option<Duration>,
    pub mem_bytes: Option<u64>,
    pub mailbox: Option<u64>,
    pub spawned: Option<u64>,
    pub hosts: Option<Vec<String>>,
    pub read_paths: Option<Vec<String>>,
    pub write_paths: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub enum BudgetBreach {
    Cpu(Duration),
    Wall(Duration),
    Mem(u64),
    Mailbox(u64),
    Spawned(u64),
    Host(String),
    Path(String),
}

impl BudgetBreach {
    pub fn into_runtime_error(self) -> RuntimeError {
        match self {
            BudgetBreach::Cpu(d) => RuntimeError::BudgetExceeded(format!("cpu {:?}", d)),
            BudgetBreach::Wall(d) => RuntimeError::BudgetExceeded(format!("wall {:?}", d)),
            BudgetBreach::Mem(n) => RuntimeError::BudgetExceeded(format!("mem {} B", n)),
            BudgetBreach::Mailbox(n) => RuntimeError::BudgetExceeded(format!("mailbox {}", n)),
            BudgetBreach::Spawned(n) => RuntimeError::BudgetExceeded(format!("spawned {}", n)),
            BudgetBreach::Host(h) => RuntimeError::CapabilityOutsideSandbox(format!("net {}", h)),
            BudgetBreach::Path(p) => RuntimeError::CapabilityOutsideSandbox(format!("fs {}", p)),
        }
    }
}

#[derive(Debug)]
pub struct BudgetTracker {
    budget: Budget,
    cpu_ns: AtomicU64,
    mem: AtomicU64,
    spawned: AtomicU64,
    start: std::time::Instant,
}

impl BudgetTracker {
    pub fn new(budget: Budget) -> Self {
        Self {
            budget,
            cpu_ns: AtomicU64::new(0),
            mem: AtomicU64::new(0),
            spawned: AtomicU64::new(0),
            start: std::time::Instant::now(),
        }
    }

    pub fn budget(&self) -> &Budget {
        &self.budget
    }

    /// v0.16 introspection: read CPU nanoseconds consumed so far.
    pub fn cpu_ns_used(&self) -> u64 {
        self.cpu_ns.load(Ordering::Relaxed)
    }

    /// v0.16 introspection: read memory bytes consumed so far.
    pub fn mem_used(&self) -> u64 {
        self.mem.load(Ordering::Relaxed)
    }

    /// v0.16 introspection: wall-clock ms elapsed since the tracker was created.
    pub fn elapsed_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    pub fn record_cpu(&self, d: Duration) {
        self.cpu_ns
            .fetch_add(d.as_nanos() as u64, Ordering::Relaxed);
    }

    pub fn record_mem(&self, bytes: u64) {
        self.mem.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn record_spawn(&self) -> Result<(), BudgetBreach> {
        let n = self.spawned.fetch_add(1, Ordering::Relaxed) + 1;
        if let Some(limit) = self.budget.spawned {
            if n > limit {
                return Err(BudgetBreach::Spawned(n));
            }
        }
        Ok(())
    }

    pub fn check(&self) -> Result<(), BudgetBreach> {
        if let Some(limit) = self.budget.cpu {
            let used = Duration::from_nanos(self.cpu_ns.load(Ordering::Relaxed));
            if used > limit {
                return Err(BudgetBreach::Cpu(used));
            }
        }
        if let Some(limit) = self.budget.wall {
            let elapsed = self.start.elapsed();
            if elapsed > limit {
                return Err(BudgetBreach::Wall(elapsed));
            }
        }
        if let Some(limit) = self.budget.mem_bytes {
            let used = self.mem.load(Ordering::Relaxed);
            if used > limit {
                return Err(BudgetBreach::Mem(used));
            }
        }
        Ok(())
    }

    pub fn check_mailbox_depth(&self, depth: u64) -> Result<(), BudgetBreach> {
        if let Some(limit) = self.budget.mailbox {
            if depth > limit {
                return Err(BudgetBreach::Mailbox(depth));
            }
        }
        Ok(())
    }

    pub fn check_host(&self, host: &str) -> Result<(), BudgetBreach> {
        if let Some(allow) = &self.budget.hosts {
            if !allow.iter().any(|h| h == host) {
                return Err(BudgetBreach::Host(host.into()));
            }
        }
        Ok(())
    }

    pub fn check_read_path(&self, path: &str) -> Result<(), BudgetBreach> {
        check_path(path, self.budget.read_paths.as_deref())
    }

    pub fn check_write_path(&self, path: &str) -> Result<(), BudgetBreach> {
        check_path(path, self.budget.write_paths.as_deref())
    }
}

fn check_path(path: &str, allow: Option<&[String]>) -> Result<(), BudgetBreach> {
    let Some(list) = allow else { return Ok(()) };
    let ok = list.iter().any(|p| path == p || path.starts_with(p));
    if ok {
        Ok(())
    } else {
        Err(BudgetBreach::Path(path.into()))
    }
}
