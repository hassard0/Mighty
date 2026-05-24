//! Deterministic-mode helpers (spec §25.5).

use std::time::Duration;

#[derive(Debug, Clone)]
pub struct SeededRng {
    state: u64,
}

impl SeededRng {
    pub fn new(seed: u64) -> Self {
        Self {
            // mix in a constant so seed=0 isn't degenerate
            state: seed.wrapping_add(0x9E37_79B9_7F4A_7C15),
        }
    }
    pub fn next_u64(&mut self) -> u64 {
        // Xorshift*
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        self.state.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LogicalClock {
    pub now_ns: u64,
}

impl LogicalClock {
    pub fn advance(&mut self, d: Duration) {
        self.now_ns = self.now_ns.wrapping_add(d.as_nanos() as u64);
    }
}
