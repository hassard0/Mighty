//! Pre-allocated message-frame slab pool (spec §25.3, closes A40).
//!
//! Slice-7 mailboxes stored every [`crate::mailbox::MessageFrame`] on
//! the heap and let `tokio::sync::mpsc` own them — fine for low
//! throughput, but every fire-and-forget message went through a
//! `Vec<Value>` allocation for its args even when there were zero or
//! one.
//!
//! v0.3 introduces a per-mailbox slab pool of fixed-size *payload
//! slots*. Each slot has:
//!
//! - an inline byte payload `[u8; INLINE_BYTES]` (default 64) used
//!   whenever the encoded args fit;
//! - an optional `Box<[u8]>` overflow buffer for larger payloads;
//! - a free-list link used by the pool itself.
//!
//! The mailbox API surface is unchanged: senders construct
//! `MessageFrame`s and call `Mailbox::send`. Internally the frame
//! borrows a slot from the pool when it's enqueued; the slot is
//! returned on drop. This preserves FIFO ordering (slots are not
//! reordered relative to enqueue order — the mpsc channel still owns
//! ordering), bounds total in-flight allocation, and gives
//! backpressure semantics consistent with A40.
//!
//! ## Determinism
//!
//! Slot indices are *not* observable from user programs; the pool
//! uses a LIFO free-list which is stable per single-threaded run.
//! Multi-threaded sends interleave non-deterministically with respect
//! to slot indices but FIFO of *messages* into a mailbox is preserved
//! by the mpsc backbone.

use parking_lot::Mutex;
use std::sync::Arc;

/// Default inline payload size (bytes). Tunable per-pool via
/// [`SlabPool::with_layout`].
pub const DEFAULT_INLINE_BYTES: usize = 64;

/// Default number of pre-allocated slots per pool. Matches the
/// default mailbox depth in A40 (1024) so a busy mailbox does not
/// have to fall back to overflow allocation.
pub const DEFAULT_POOL_SIZE: usize = 1024;

/// A single slot in the pool. Holds either inline bytes or a heap
/// overflow buffer.
#[derive(Debug)]
pub struct Slot {
    pub inline: Vec<u8>,             // capacity == inline_bytes
    pub overflow: Option<Box<[u8]>>, // present when payload > inline_bytes
    pub used: usize,                 // bytes actually filled (inline + overflow)
}

impl Slot {
    fn empty(inline_bytes: usize) -> Self {
        Self {
            inline: Vec::with_capacity(inline_bytes),
            overflow: None,
            used: 0,
        }
    }

    fn reset(&mut self) {
        self.inline.clear();
        self.overflow = None;
        self.used = 0;
    }

    /// Bytes currently stored (inline + overflow).
    pub fn len(&self) -> usize {
        self.used
    }

    /// True when no bytes are stored.
    pub fn is_empty(&self) -> bool {
        self.used == 0
    }

    /// True if the payload spilled to the overflow buffer.
    pub fn spilled(&self) -> bool {
        self.overflow.is_some()
    }

    /// Read the bytes stored in this slot. Returns a borrowed slice
    /// whose lifetime is tied to `self`.
    pub fn bytes(&self) -> &[u8] {
        if let Some(b) = &self.overflow {
            &b[..self.used]
        } else {
            &self.inline[..self.used]
        }
    }

    /// Write `data` into the slot, spilling to overflow if it exceeds
    /// the inline capacity.
    pub fn write(&mut self, data: &[u8]) {
        self.reset();
        if data.len() <= self.inline.capacity() {
            self.inline.extend_from_slice(data);
        } else {
            self.overflow = Some(data.to_vec().into_boxed_slice());
        }
        self.used = data.len();
    }
}

#[derive(Debug)]
struct PoolInner {
    /// Slots indexed by id. Each slot is wrapped in a Mutex so a
    /// borrow can mutate without unlocking the outer pool.
    slots: Vec<Mutex<Slot>>,
    /// LIFO free-list of slot indices currently unused.
    free: Mutex<Vec<usize>>,
    /// Hard cap on the number of slots. Beyond this we still hand out
    /// `PooledFrame::overflow` handles, but they don't borrow a slot
    /// from the pool — they own a heap buffer directly.
    capacity: usize,
    inline_bytes: usize,
    /// Counts of (acquired, released) over the pool's lifetime. Used
    /// by benches and the leak-detection assertion in tests.
    acquired: parking_lot::Mutex<u64>,
    released: parking_lot::Mutex<u64>,
}

/// Thread-safe pool. Cloning is cheap (an Arc handle).
#[derive(Debug, Clone)]
pub struct SlabPool {
    inner: Arc<PoolInner>,
}

impl Default for SlabPool {
    fn default() -> Self {
        Self::new(DEFAULT_POOL_SIZE)
    }
}

impl SlabPool {
    /// Build a pool with `capacity` slots and the default inline
    /// payload size.
    pub fn new(capacity: usize) -> Self {
        Self::with_layout(capacity, DEFAULT_INLINE_BYTES)
    }

    /// Build a pool with explicit capacity and inline-bytes layout.
    pub fn with_layout(capacity: usize, inline_bytes: usize) -> Self {
        let cap = capacity.max(1);
        let mut slots = Vec::with_capacity(cap);
        let mut free = Vec::with_capacity(cap);
        for i in 0..cap {
            slots.push(Mutex::new(Slot::empty(inline_bytes)));
            free.push(cap - 1 - i); // initial LIFO order so id 0 is popped first
        }
        Self {
            inner: Arc::new(PoolInner {
                slots,
                free: Mutex::new(free),
                capacity: cap,
                inline_bytes,
                acquired: parking_lot::Mutex::new(0),
                released: parking_lot::Mutex::new(0),
            }),
        }
    }

    /// Capacity (slot count).
    pub fn capacity(&self) -> usize {
        self.inner.capacity
    }

    /// Inline byte capacity per slot.
    pub fn inline_bytes(&self) -> usize {
        self.inline_bytes_inner()
    }

    fn inline_bytes_inner(&self) -> usize {
        self.inner.inline_bytes
    }

    /// Currently free slot count.
    pub fn free_count(&self) -> usize {
        self.inner.free.lock().len()
    }

    /// Currently used slot count.
    pub fn used_count(&self) -> usize {
        self.inner.capacity - self.free_count()
    }

    /// Lifetime counters used by tests/benches.
    pub fn stats(&self) -> (u64, u64) {
        (*self.inner.acquired.lock(), *self.inner.released.lock())
    }

    /// Try to acquire a free slot. Returns `None` if the pool is
    /// exhausted — callers can decide whether to allocate an
    /// `overflow` frame or backpressure.
    pub fn try_acquire(&self, bytes: &[u8]) -> Option<PooledFrame> {
        let idx = self.inner.free.lock().pop()?;
        {
            let mut slot = self.inner.slots[idx].lock();
            slot.write(bytes);
        }
        *self.inner.acquired.lock() += 1;
        Some(PooledFrame {
            pool: self.inner.clone(),
            slot_idx: Some(idx),
            overflow: None,
            len: bytes.len(),
        })
    }

    /// Acquire — falling back to a standalone overflow frame when the
    /// pool is full. Always succeeds.
    pub fn acquire_or_overflow(&self, bytes: &[u8]) -> PooledFrame {
        // v0.8 fast path (A40+): empty payloads bypass the per-slot
        // lock entirely. They still produce a valid PooledFrame so
        // callers can treat the slab handle uniformly, but no slot is
        // borrowed and no `inline` Vec is written. This skips the
        // free-list pop, slot lock, write, and the eventual release —
        // a ~6-instruction path for the common fire-and-forget case
        // (SmallPayload::Empty) that dominates agent_send_latency.
        if bytes.is_empty() {
            return PooledFrame {
                pool: self.inner.clone(),
                slot_idx: None,
                overflow: None,
                len: 0,
            };
        }
        if let Some(p) = self.try_acquire(bytes) {
            return p;
        }
        // Pool exhausted: allocate a standalone buffer.
        let buf: Box<[u8]> = bytes.to_vec().into_boxed_slice();
        PooledFrame {
            pool: self.inner.clone(),
            slot_idx: None,
            overflow: Some(buf),
            len: bytes.len(),
        }
    }

    /// v0.8 fast-path: same as [`acquire_or_overflow`] but skips the
    /// slot write when the caller knows the payload is metadata-only.
    /// Used by the mailbox admit path when the `SmallPayload::Empty`
    /// case is detected at the call site. Cheaper than going through
    /// `acquire_or_overflow(&[])` because it avoids the empty-slice
    /// length check + the per-call `inner.clone()` indirection.
    #[inline]
    pub fn acquire_empty(&self) -> PooledFrame {
        PooledFrame {
            pool: self.inner.clone(),
            slot_idx: None,
            overflow: None,
            len: 0,
        }
    }
}

/// RAII handle to a payload slot (or a standalone overflow buffer).
/// Returned to the pool on drop.
#[derive(Debug)]
pub struct PooledFrame {
    pool: Arc<PoolInner>,
    slot_idx: Option<usize>,
    overflow: Option<Box<[u8]>>,
    len: usize,
}

impl PooledFrame {
    /// True if this frame's bytes live in the slab (not a standalone
    /// overflow buffer).
    pub fn is_pooled(&self) -> bool {
        self.slot_idx.is_some()
    }

    /// True if the payload spilled to a heap buffer (inside the slot
    /// or as a standalone overflow frame).
    pub fn spilled(&self) -> bool {
        if let Some(idx) = self.slot_idx {
            self.pool.slots[idx].lock().spilled()
        } else {
            true
        }
    }

    /// Bytes stored in this frame.
    pub fn with_bytes<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R {
        if let Some(b) = &self.overflow {
            f(&b[..self.len])
        } else if let Some(idx) = self.slot_idx {
            let slot = self.pool.slots[idx].lock();
            // Copy out of the lock guard for the closure.
            let owned: Vec<u8> = slot.bytes().to_vec();
            drop(slot);
            f(&owned)
        } else {
            f(&[])
        }
    }

    /// Cheap accessor returning an owned copy of the payload bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.with_bytes(|b| b.to_vec())
    }

    /// Length in bytes.
    pub fn len(&self) -> usize {
        self.len
    }

    /// True when this frame carries no payload.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Drop for PooledFrame {
    fn drop(&mut self) {
        if let Some(idx) = self.slot_idx.take() {
            {
                let mut slot = self.pool.slots[idx].lock();
                slot.reset();
            }
            self.pool.free.lock().push(idx);
            *self.pool.released.lock() += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_release_roundtrip() {
        let pool = SlabPool::new(2);
        assert_eq!(pool.free_count(), 2);
        let a = pool.try_acquire(b"hi").unwrap();
        assert_eq!(pool.free_count(), 1);
        let b = pool.try_acquire(b"yo").unwrap();
        assert_eq!(pool.free_count(), 0);
        assert!(pool.try_acquire(b"x").is_none());
        drop(a);
        assert_eq!(pool.free_count(), 1);
        drop(b);
        assert_eq!(pool.free_count(), 2);
    }

    #[test]
    fn inline_then_overflow() {
        let pool = SlabPool::with_layout(2, 4);
        let small = pool.try_acquire(b"abcd").unwrap();
        assert!(!small.spilled());
        assert_eq!(small.to_bytes(), b"abcd");

        let big = pool.try_acquire(b"abcdefgh").unwrap();
        assert!(big.spilled());
        assert_eq!(big.to_bytes(), b"abcdefgh");
    }

    #[test]
    fn overflow_when_pool_full() {
        let pool = SlabPool::new(1);
        let a = pool.acquire_or_overflow(b"hello");
        assert!(a.is_pooled());
        let b = pool.acquire_or_overflow(b"world");
        assert!(!b.is_pooled());
        assert!(b.spilled());
        assert_eq!(b.to_bytes(), b"world");
        // Pool acquired count counts only pooled grabs, not overflow.
        let (acq, rel) = pool.stats();
        assert_eq!(acq, 1);
        assert_eq!(rel, 0);
        drop(a);
        drop(b);
        let (acq, rel) = pool.stats();
        assert_eq!(acq, 1);
        assert_eq!(rel, 1);
    }

    #[test]
    fn lifo_reuse_pattern() {
        // After acquire/release loops, FIFO of *messages* (sends) is
        // preserved by the mpsc channel that owns enqueue order; the
        // slab pool itself reuses indices LIFO for cache locality.
        let pool = SlabPool::new(3);
        let mut handles = vec![];
        for i in 0..3 {
            handles.push(pool.try_acquire(&[i as u8]).unwrap());
        }
        drop(handles); // releases in reverse order
                       // Re-acquire 3 more; pool should hand out without overflow
        let a = pool.try_acquire(b"x").unwrap();
        let b = pool.try_acquire(b"y").unwrap();
        let c = pool.try_acquire(b"z").unwrap();
        assert_eq!(pool.free_count(), 0);
        drop((a, b, c));
        assert_eq!(pool.free_count(), 3);
    }
}
