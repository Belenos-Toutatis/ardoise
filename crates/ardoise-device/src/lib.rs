//! Tablet management: connection, sync, live mode and diagnostic log, over an abstract transport.
//!
//! - [`transport`]: the link abstraction shared with `ardoise-ble` and `ardoise-sim`.
//! - [`session`]: one authenticated conversation (register, connect, info, sync, live mode).
//! - [`manager`]: finds tablets, keeps the shared registration (`device.json`), reconnects.
//! - [`log`] and [`logging`]: the `.jsonl` diagnostic log and the transport that feeds it.
//! - [`diagnostic`]: the guided tablet test and its report.

pub mod diagnostic;
pub mod identity;
pub mod log;
pub mod logging;
pub mod manager;
mod preview;
mod report;
mod runner;
pub mod session;
pub mod transport;

pub use report::render as render_report;
