//! Real bumpalo-backed arena allocator (slice 8, A50).
//!
//! Replaces slice-7's "approximate" byte counter. Each `arena {}` scope
//! pushes a new `Bump`; `Drop` pops it and frees all allocations made
//! in that frame at once.
//!
//! The codegen-cranelift runtime ABI bridge ([`crate::codegen_abi`])
//! routes `ArenaPush` / `ArenaPop` / `alloc` statements through this
//! type. Outside a compiled-handler context the slice-6 interpreter
//! still uses its own byte-counter; the two paths are interchangeable.

use bumpalo::Bump;

#[derive(Default)]
pub struct ArenaStack {
    frames: Vec<Bump>,
}

impl ArenaStack {
    /// Push a new arena frame. Returns the new depth (1-based).
    pub fn push(&mut self) -> usize {
        self.frames.push(Bump::new());
        self.frames.len()
    }

    /// Pop the top arena frame. Drops all allocations within it.
    /// Returns the new depth (0 if empty).
    pub fn pop(&mut self) -> usize {
        let _ = self.frames.pop();
        self.frames.len()
    }

    /// Allocate `size` bytes with `align` on the top arena. Returns
    /// `None` if no arena is active, otherwise the raw byte pointer.
    pub fn alloc(&mut self, size: usize, align: usize) -> Option<*mut u8> {
        let top = self.frames.last_mut()?;
        let align = align.max(1);
        let layout = std::alloc::Layout::from_size_align(size, align).ok()?;
        let buf = top.alloc_layout(layout);
        Some(buf.as_ptr())
    }

    pub fn depth(&self) -> usize {
        self.frames.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_stack_alloc_returns_none() {
        let mut s = ArenaStack::default();
        assert!(s.alloc(64, 8).is_none());
    }

    #[test]
    fn push_then_alloc_succeeds() {
        let mut s = ArenaStack::default();
        s.push();
        let p = s.alloc(32, 8);
        assert!(p.is_some());
    }

    #[test]
    fn pop_reduces_depth() {
        let mut s = ArenaStack::default();
        s.push();
        s.push();
        assert_eq!(s.depth(), 2);
        s.pop();
        assert_eq!(s.depth(), 1);
        s.pop();
        assert_eq!(s.depth(), 0);
    }

    #[test]
    fn many_allocations_in_one_frame() {
        let mut s = ArenaStack::default();
        s.push();
        for _ in 0..100 {
            assert!(s.alloc(8, 8).is_some());
        }
        s.pop();
    }
}
