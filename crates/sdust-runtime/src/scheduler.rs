//! Tokio executor wrapper. Multi-thread by default; single-thread
//! current-thread when deterministic mode is requested.

use std::sync::Arc;
use tokio::runtime::{Builder, Runtime as TokioRt};

#[derive(Debug)]
pub struct Scheduler {
    pub rt: Arc<TokioRt>,
    pub deterministic: bool,
}

impl Scheduler {
    pub fn multi_thread(threads: usize) -> Self {
        let rt = Builder::new_multi_thread()
            .worker_threads(threads.max(1))
            .enable_all()
            .build()
            .expect("tokio runtime");
        Self {
            rt: Arc::new(rt),
            deterministic: false,
        }
    }

    pub fn current_thread() -> Self {
        let rt = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        Self {
            rt: Arc::new(rt),
            deterministic: true,
        }
    }
}
