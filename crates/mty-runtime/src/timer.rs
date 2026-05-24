//! Deadline helpers around tokio::time.

use crate::error::RuntimeError;
use std::future::Future;
use std::time::Duration;

pub async fn with_deadline<F, T>(d: Option<Duration>, fut: F) -> Result<T, RuntimeError>
where
    F: Future<Output = T>,
{
    match d {
        None => Ok(fut.await),
        Some(d) => tokio::time::timeout(d, fut)
            .await
            .map_err(|_| RuntimeError::DeadlineExceeded(d)),
    }
}
