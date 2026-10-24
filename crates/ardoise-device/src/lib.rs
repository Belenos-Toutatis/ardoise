//! Tablet management: connection, sync, live mode and diagnostic log, over an abstract transport.
//!
//! - [`transport`]: the link abstraction shared with `ardoise-ble` and `ardoise-sim`.
//! - [`session`]: one authenticated conversation (register, connect, info, sync, live mode).
//! - [`manager`]: finds tablets, keeps the shared registration (`device.json`), reconnects.
//! - [`log`] and [`logging`]: the `.jsonl` diagnostic log and the transport that feeds it.
//! - [`diagnostic`]: the guided tablet test and its report.
//! - [`pages`]: synced pages become notes of the library (then the automatic export).
//! - [`service`]: the tablet as the application sees it (auto-connect, sync, registration, live).

pub mod diagnostic;
pub mod identity;
pub mod log;
pub mod logging;
pub mod manager;
pub mod pages;
mod preview;
mod report;
mod runner;
pub mod service;
pub mod session;
pub mod transport;

#[cfg(test)]
mod testutil;

pub use report::render as render_report;
