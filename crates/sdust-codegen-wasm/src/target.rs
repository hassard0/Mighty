//! Wasm target descriptor.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmTarget {
    /// wasm32-wasi (WASI preview1) — log/print routed to fd_write.
    Wasi,
    /// wasm32-web — browser host; log routed to imported `stardust.log`.
    Web,
}

impl WasmTarget {
    pub fn triple(self) -> &'static str {
        match self {
            WasmTarget::Wasi => "wasm32-wasi",
            WasmTarget::Web => "wasm32-web",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "wasm32-wasi" | "wasi" => WasmTarget::Wasi,
            "wasm32-web" | "web" | "browser" => WasmTarget::Web,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_triples() {
        assert_eq!(WasmTarget::parse("wasm32-wasi"), Some(WasmTarget::Wasi));
        assert_eq!(WasmTarget::parse("wasm32-web"), Some(WasmTarget::Web));
    }

    #[test]
    fn rejects_unknown() {
        assert!(WasmTarget::parse("garbage").is_none());
    }

    #[test]
    fn triples_round_trip() {
        for t in [WasmTarget::Wasi, WasmTarget::Web] {
            assert_eq!(WasmTarget::parse(t.triple()), Some(t));
        }
    }
}
