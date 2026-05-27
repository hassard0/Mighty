//! `std.Vec[T]` — generic, growable array (v0.25 Track E).
//!
//! Until v0.25 the only "Vec-shaped" thing Mighty had was `&[T]`
//! borrowed slices plus the SIR interpreter's `Value::Array(Vec<Value>)`
//! for the dynamic-typed runtime path. That was enough for the early
//! v0.22 self-host LEB128 follow-up to limp along (the interpreter
//! ignores the lack of a concrete type), but two upcoming demos need
//! real generic vectors:
//!
//! 1. The v0.23 / v0.24 Tetris demo wants a `Vec[U32]` to back its
//!    flat 200-cell board (`row * 10 + col` indexing).
//! 2. The canvas-game agent the Mighty-source layer wants
//!    `Vec[U8]` for byte buffers (sprite data, network frames) and
//!    `Vec[String]` for collections of entity names.
//!
//! This module ships a `#[repr(transparent)]` wrapper over Rust's
//! `std::vec::Vec<T>` so the storage layout is identical to what the
//! wasm Component ABI's `list<T>` lowers to, while the **type** stays
//! Mighty's `std::Vec` so the typechecker can distinguish it from a
//! borrowed `[T]` slice.
//!
//! ## API
//!
//! All methods take their Rust analog's signature; everything is
//! generic over `T: Clone` so the SIR runtime can keep its
//! by-value-clone semantics. The interpreter doesn't need this Rust
//! impl directly (it stores Mighty `Vec`s as `Value::Array(Vec<Value>)`
//! and dispatches through the permissive method table in
//! `mty-types::prelude`), but the wasm-target self-host codegen and any
//! future native AOT backend lower `Vec.method(...)` calls straight to
//! these functions.
//!
//! ## Special cases
//!
//! Per the slice spec we explicitly verify three element types:
//!
//! - [`Vec<u8>`]    — byte buffers (the wasm `list<u8>` shape).
//! - [`Vec<u32>`]   — the Tetris cell array shape.
//! - [`Vec<crate::string::String>`] — collections of strings.
//!
//! Each is exercised in the test module below and in
//! `tests/vec_basic.rs`.

use std::fmt;
use std::ops::{Index, IndexMut};

/// Generic, growable array. Storage is identical to `std::vec::Vec<T>`
/// (see the module docs) so passing one across an FFI boundary just
/// hands over the (ptr, len, cap) triple.
#[derive(Default, Clone, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Vec<T> {
    inner: std::vec::Vec<T>,
}

impl<T> Vec<T> {
    /// Construct an empty `Vec`. No allocation.
    pub fn new() -> Self {
        Self {
            inner: std::vec::Vec::new(),
        }
    }

    /// Construct a `Vec` with at least `n` elements of capacity. The
    /// length is still zero.
    pub fn with_capacity(n: usize) -> Self {
        Self {
            inner: std::vec::Vec::with_capacity(n),
        }
    }

    /// Append a value at the end. Amortised O(1).
    pub fn push(&mut self, v: T) {
        self.inner.push(v);
    }

    /// Remove and return the last value, or `None` if the `Vec` is
    /// empty. The capacity is preserved.
    pub fn pop(&mut self) -> Option<T> {
        self.inner.pop()
    }

    /// Current element count.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// True iff the element count is zero.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Drop every element. Capacity is preserved so repeated
    /// build-and-clear cycles avoid reallocations.
    pub fn clear(&mut self) {
        self.inner.clear();
    }

    /// Borrow the element at `idx`, or `None` if out-of-bounds.
    pub fn get(&self, idx: usize) -> Option<&T> {
        self.inner.get(idx)
    }

    /// Mutably borrow the element at `idx`, or `None` if
    /// out-of-bounds.
    pub fn get_mut(&mut self, idx: usize) -> Option<&mut T> {
        self.inner.get_mut(idx)
    }

    /// Borrow every element in order. The iterator yields `&T`.
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.inner.iter()
    }

    /// Mutable counterpart of [`iter`].
    ///
    /// [`iter`]: Self::iter
    #[allow(clippy::iter_without_into_iter)] // &mut Vec<T> via inherent iter_mut is intentional; IntoIterator for &mut is a v0.26 follow-up
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, T> {
        self.inner.iter_mut()
    }

    /// Borrow the contents as a slice. Useful for FFI handoff.
    pub fn as_slice(&self) -> &[T] {
        self.inner.as_slice()
    }

    /// Mutable counterpart of [`as_slice`].
    ///
    /// [`as_slice`]: Self::as_slice
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        self.inner.as_mut_slice()
    }

    /// Number of elements the backing buffer can hold without
    /// reallocating.
    pub fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    /// Move the contents of `other` to the end of `self`, leaving
    /// `other` empty.
    pub fn append(&mut self, other: &mut Self) {
        self.inner.append(&mut other.inner);
    }

    /// Construct a `Vec` from an existing `std::vec::Vec<T>` without
    /// copying. Cheap: just rewraps the buffer.
    pub fn from_std_vec(v: std::vec::Vec<T>) -> Self {
        Self { inner: v }
    }

    /// Unwrap back to a `std::vec::Vec<T>` without copying.
    pub fn into_std_vec(self) -> std::vec::Vec<T> {
        self.inner
    }
}

impl<T: Clone> Vec<T> {
    /// Build a `Vec` of `n` copies of `value`. Mirrors
    /// `std::vec::Vec::resize` for a fresh allocation.
    pub fn from_elem(value: T, n: usize) -> Self {
        Self {
            inner: std::vec::from_elem(value, n),
        }
    }
}

impl<T> Index<usize> for Vec<T> {
    type Output = T;
    fn index(&self, idx: usize) -> &T {
        &self.inner[idx]
    }
}

impl<T> IndexMut<usize> for Vec<T> {
    fn index_mut(&mut self, idx: usize) -> &mut T {
        &mut self.inner[idx]
    }
}

impl<T: fmt::Debug> fmt::Debug for Vec<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T> From<std::vec::Vec<T>> for Vec<T> {
    fn from(v: std::vec::Vec<T>) -> Self {
        Self::from_std_vec(v)
    }
}

impl<T> From<Vec<T>> for std::vec::Vec<T> {
    fn from(v: Vec<T>) -> Self {
        v.into_std_vec()
    }
}

impl<T> FromIterator<T> for Vec<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self {
            inner: iter.into_iter().collect(),
        }
    }
}

impl<T> IntoIterator for Vec<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;
    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a Vec<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::string::String as MtyString;

    // ---- Vec<u8> (byte buffer shape — wasm `list<u8>`). ------

    #[test]
    fn u8_new_is_empty() {
        let v: Vec<u8> = Vec::new();
        assert_eq!(v.len(), 0);
        assert!(v.is_empty());
    }

    #[test]
    fn u8_push_pop_round_trip() {
        let mut v: Vec<u8> = Vec::new();
        v.push(1);
        v.push(2);
        v.push(3);
        assert_eq!(v.len(), 3);
        assert_eq!(v.pop(), Some(3));
        assert_eq!(v.pop(), Some(2));
        assert_eq!(v.pop(), Some(1));
        assert_eq!(v.pop(), None);
    }

    #[test]
    fn u8_index_read_write() {
        let mut v: Vec<u8> = Vec::from_elem(0u8, 4);
        v[0] = 5;
        v[3] = 9;
        assert_eq!(v[0], 5);
        assert_eq!(v[1], 0);
        assert_eq!(v[3], 9);
    }

    // ---- Vec<u32> (Tetris board shape). -----------------------

    #[test]
    fn u32_with_capacity_preserves_capacity() {
        let mut v: Vec<u32> = Vec::with_capacity(200);
        assert!(v.capacity() >= 200);
        assert_eq!(v.len(), 0);
        for i in 0..200u32 {
            v.push(i);
        }
        assert_eq!(v.len(), 200);
        assert_eq!(v[7], 7);
        assert_eq!(v[199], 199);
    }

    #[test]
    fn u32_clear_resets_len() {
        let mut v: Vec<u32> = Vec::from_elem(0u32, 200);
        assert_eq!(v.len(), 200);
        let cap = v.capacity();
        v.clear();
        assert_eq!(v.len(), 0);
        assert!(v.is_empty());
        assert_eq!(v.capacity(), cap, "capacity preserved across clear");
    }

    // ---- Vec<String> (collections of strings). ----------------

    #[test]
    fn string_collection_round_trips() {
        let mut v: Vec<MtyString> = Vec::new();
        v.push(MtyString::from_str("alpha"));
        v.push(MtyString::from_str("beta"));
        v.push(MtyString::from_str("gamma"));
        assert_eq!(v.len(), 3);
        assert_eq!(v[1].as_str(), "beta");
        let popped = v.pop().expect("non-empty");
        assert_eq!(popped.as_str(), "gamma");
    }

    // ---- Iteration shapes. ------------------------------------

    #[test]
    fn iter_yields_borrowed_refs() {
        let v: Vec<u32> = (0..5u32).collect();
        let sum: u32 = v.iter().sum();
        assert_eq!(sum, 1 + 2 + 3 + 4);
    }

    #[test]
    fn iter_mut_allows_in_place_mutation() {
        let mut v: Vec<u32> = (0..3u32).collect();
        for x in v.iter_mut() {
            *x *= 10;
        }
        assert_eq!(v[0], 0);
        assert_eq!(v[1], 10);
        assert_eq!(v[2], 20);
    }

    #[test]
    fn get_oob_returns_none() {
        let v: Vec<u32> = (0..3u32).collect();
        assert_eq!(v.get(0), Some(&0));
        assert_eq!(v.get(2), Some(&2));
        assert_eq!(v.get(3), None);
        assert_eq!(v.get(usize::MAX), None);
    }

    #[test]
    fn get_mut_allows_safe_mutation() {
        let mut v: Vec<u32> = (0..3u32).collect();
        if let Some(slot) = v.get_mut(1) {
            *slot = 99;
        }
        assert_eq!(v[1], 99);
        assert!(v.get_mut(7).is_none());
    }

    // ---- Conversions. -----------------------------------------

    #[test]
    fn from_std_vec_and_back_is_zero_copy() {
        let std_vec = vec![1u8, 2, 3, 4];
        let mty = Vec::from_std_vec(std_vec);
        assert_eq!(mty.len(), 4);
        let std_again: std::vec::Vec<u8> = mty.into_std_vec();
        assert_eq!(std_again, vec![1, 2, 3, 4]);
    }

    #[test]
    fn from_iter_collects() {
        let v: Vec<u32> = (10..13u32).collect();
        assert_eq!(v.as_slice(), &[10, 11, 12]);
    }
}
