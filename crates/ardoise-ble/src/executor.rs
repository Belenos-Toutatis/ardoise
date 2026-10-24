//! The private tokio runtime that drives btleplug.
//!
//! btleplug is asynchronous; the transport contract is blocking. One small multi-thread
//! runtime is shared (through `Arc`) by a connector and every link it opened. Blocking calls
//! spawn their future on it and wait on a standard channel, which works from any thread,
//! including a thread that already runs another async runtime (where `block_on` would panic).

use std::future::Future;
use std::sync::mpsc;

use ardoise_device::transport::TransportError;
use tokio::runtime::{Builder, Runtime};
use tokio::task::JoinHandle;

/// Worker threads: one is enough for the traffic of a tablet, the second keeps notification
/// forwarding responsive while another task waits on a slow WinRT call.
const WORKER_THREADS: usize = 2;

pub(crate) struct Executor {
    /// `None` only while dropping.
    runtime: Option<Runtime>,
}

impl Executor {
    pub(crate) fn new() -> Result<Self, TransportError> {
        let runtime = Builder::new_multi_thread()
            .worker_threads(WORKER_THREADS)
            .max_blocking_threads(2)
            .thread_name("ardoise-ble")
            .enable_time()
            .build()
            .map_err(|e| {
                TransportError::Backend(format!("cannot start the Bluetooth runtime: {e}"))
            })?;
        Ok(Self {
            runtime: Some(runtime),
        })
    }

    fn runtime(&self) -> Result<&Runtime, TransportError> {
        self.runtime
            .as_ref()
            .ok_or_else(|| TransportError::Backend("the Bluetooth runtime is stopped".into()))
    }

    /// Runs `future` in the background.
    pub(crate) fn spawn<F>(&self, future: F) -> Result<JoinHandle<F::Output>, TransportError>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        Ok(self.runtime()?.spawn(future))
    }

    /// Runs `future` to completion and returns its output, blocking the calling thread.
    pub(crate) fn run<F, T>(&self, future: F) -> Result<T, TransportError>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let (sender, receiver) = mpsc::sync_channel(1);
        self.spawn(async move {
            let _ = sender.send(future.await);
        })?;
        receiver
            .recv()
            .map_err(|_| TransportError::Backend("a Bluetooth task stopped unexpectedly".into()))
    }
}

impl Drop for Executor {
    fn drop(&mut self) {
        // Never blocks, so dropping the last connector or transport is safe on any thread.
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_futures_to_completion() {
        let executor = Executor::new().unwrap();
        let value = executor
            .run(async {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                42
            })
            .unwrap();
        assert_eq!(value, 42);
    }

    #[test]
    fn works_from_inside_another_runtime() {
        let executor = Executor::new().unwrap();
        let outer = Builder::new_current_thread().build().unwrap();
        let value = outer.block_on(async { executor.run(async { 7 }).unwrap() });
        assert_eq!(value, 7);
        // Dropping the executor inside an async context must not panic either.
        outer.block_on(async move { drop(executor) });
    }
}
