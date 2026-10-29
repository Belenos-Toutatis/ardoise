//! The bridge to the open app (`app_state`, `open_note`, `presentation`, `last_live_note`): the
//! client side of the named pipe that the running app serves (`docs/design/mcp.md`).
//!
//! To add the tools:
//!
//! 1. define the requests in [`AppBridge`] (the app answers them in `app/src-tauri`), and the
//!    pipe client implementing it in this file (or a sub-module); [`NoApp`] answers
//!    [`code::APP_NOT_RUNNING`](crate::error::code::APP_NOT_RUNNING);
//! 2. write the tools in an `impl ArdoiseServer` block marked
//!    `#[tool_router(router = bridge_tools, vis = "pub(crate)")]` (see `read.rs`) and add
//!    `+ Self::bridge_tools()` to [`ArdoiseServer::bridge_router`] below;
//! 3. the host gets its bridge with [`Host::with_app`](crate::host::Host::with_app) in
//!    `app/src-tauri/src/mcp.rs`; [`crate::server::build_tool_router`] already adds
//!    `bridge_router()` unless the server runs read-only.

use rmcp::handler::server::router::tool::ToolRouter;

use crate::server::ArdoiseServer;

/// The requests the open app answers (none yet).
pub trait AppBridge: Send + Sync {}

/// No app is open: every request fails with `appNotRunning`.
pub struct NoApp;

impl AppBridge for NoApp {}

impl ArdoiseServer {
    /// The bridge tools (none yet).
    pub(crate) fn bridge_router() -> ToolRouter<ArdoiseServer> {
        ToolRouter::new()
    }
}
