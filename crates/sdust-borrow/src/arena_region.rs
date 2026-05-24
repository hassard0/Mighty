//! Arena region tracking. A monotone counter; each `arena <name> { ... }`
//! body pushes a fresh region id. Locals introduced inside the body carry
//! the region id. At the end of the body, the walker inspects the body's
//! tail expression and flags direct references to arena-local values.

use crate::state::ArenaRegionId;

#[derive(Debug, Default)]
pub struct ArenaCounter {
    next: u32,
}

impl ArenaCounter {
    pub fn fresh(&mut self) -> ArenaRegionId {
        let id = ArenaRegionId(self.next);
        self.next += 1;
        id
    }
}
