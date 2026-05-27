//! Integration tests for the v0.25 Track E `std.Vec[T]` real impl.
//!
//! The inline `mod tests` in `src/vec.rs` exercises every element-type
//! special-case the slice spec calls out (`Vec[U8]`, `Vec[U32]`,
//! `Vec[String]`). This file pins down the *Mighty user contract* —
//! every assertion mirrors a call shape user source actually emits.
//!
//! Eight + tests, per the slice spec.

use mty_stdlib::vec::Vec as MtyVec;

#[test]
fn vec_new_empty() {
    let v: MtyVec<u32> = MtyVec::new();
    assert_eq!(v.len(), 0);
    assert!(v.is_empty());
}

#[test]
fn vec_push_pop_round_trip() {
    let mut v: MtyVec<u32> = MtyVec::new();
    v.push(10);
    v.push(20);
    v.push(30);
    assert_eq!(v.pop(), Some(30));
    assert_eq!(v.pop(), Some(20));
    assert_eq!(v.pop(), Some(10));
    assert_eq!(v.pop(), None);
}

#[test]
fn vec_len_tracks_pushes() {
    let mut v: MtyVec<u8> = MtyVec::new();
    assert_eq!(v.len(), 0);
    for i in 0..5u8 {
        v.push(i);
        assert_eq!(v.len(), (i + 1) as usize);
    }
}

#[test]
fn vec_index_read_write() {
    // `v[idx] = e` should be the same as Rust IndexMut.
    let mut v: MtyVec<u32> = MtyVec::from_elem(0u32, 4);
    v[0] = 5;
    v[3] = 9;
    assert_eq!(v[0], 5);
    assert_eq!(v[1], 0);
    assert_eq!(v[2], 0);
    assert_eq!(v[3], 9);
}

#[test]
fn vec_clear_resets_len_keeps_capacity() {
    let mut v: MtyVec<u32> = MtyVec::with_capacity(200);
    for i in 0..200u32 {
        v.push(i);
    }
    let cap = v.capacity();
    v.clear();
    assert_eq!(v.len(), 0);
    assert!(v.is_empty());
    assert_eq!(v.capacity(), cap);
}

#[test]
fn vec_get_oob_returns_none() {
    let v: MtyVec<u32> = (0..3u32).collect();
    assert_eq!(v.get(0), Some(&0));
    assert_eq!(v.get(2), Some(&2));
    assert_eq!(v.get(3), None);
    assert_eq!(v.get(usize::MAX), None);
}

#[test]
fn vec_iter_yields_every_element_in_order() {
    let v: MtyVec<u8> = (0..5u8).collect();
    let collected: std::vec::Vec<u8> = v.iter().copied().collect();
    assert_eq!(collected, vec![0, 1, 2, 3, 4]);
}

#[test]
fn vec_with_capacity_preserves_capacity_through_pushes() {
    // `Vec.with_capacity(200)` is the Tetris-board case — 200 cells
    // pre-allocated as a flat row*10+col array. If push triggers a
    // realloc inside the requested capacity, the impl is broken.
    let mut v: MtyVec<u32> = MtyVec::with_capacity(200);
    let cap = v.capacity();
    assert!(cap >= 200);
    for i in 0..200u32 {
        v.push(i);
    }
    assert_eq!(v.capacity(), cap, "no realloc inside requested capacity");
    assert_eq!(v.len(), 200);
}

#[test]
fn vec_u8_byte_buffer_shape() {
    // The wasm `list<u8>` shape: build a buffer, append more, hand
    // off as a slice. Mimics the canvas-game agent's sprite-data
    // path.
    let mut v: MtyVec<u8> = MtyVec::new();
    v.push(0xDE);
    v.push(0xAD);
    v.push(0xBE);
    v.push(0xEF);
    assert_eq!(v.as_slice(), &[0xDE, 0xAD, 0xBE, 0xEF]);
}

#[test]
fn vec_get_mut_supports_safe_mutation() {
    let mut v: MtyVec<u32> = (0..3u32).collect();
    if let Some(slot) = v.get_mut(1) {
        *slot = 99;
    }
    assert_eq!(v[1], 99);
    assert!(v.get_mut(7).is_none());
}

#[test]
fn vec_iter_mut_supports_in_place_update() {
    let mut v: MtyVec<u32> = (0..4u32).collect();
    for x in v.iter_mut() {
        *x *= 10;
    }
    assert_eq!(v.as_slice(), &[0, 10, 20, 30]);
}
