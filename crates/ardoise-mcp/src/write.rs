//! The write tools (`create_note`, `update_note`, `move_note`, `trash_note`, `merge_notes`,
//! `split_note`, `straighten_shapes`, `export_note`, `set_note_text`): SPEC §4.9, design in
//! `docs/design/mcp.md`. Absent with `--read-only`.
//!
//! To add the tools:
//!
//! 1. put the app-side operations they need in [`WriteServices`] (the exports and the
//!    handwriting cache live in the app's executable; the library operations are
//!    `ardoise-core`'s and need no service), and implement the trait in
//!    `app/src-tauri/src/mcp.rs`, passing it with [`Host::with_write`](crate::host::Host::with_write);
//! 2. write the tools in `impl ArdoiseServer` blocks marked
//!    `#[tool_router(router = write_tools, vis = "pub(crate)")]` (see `read.rs`), with
//!    `annotations(read_only_hint = false, destructive_hint = false, ...)`, and add
//!    `+ Self::write_tools()` to [`ArdoiseServer::write_router`] below;
//! 3. nothing else: [`crate::server::build_tool_router`] already adds `write_router()` unless
//!    the server runs read-only.

use rmcp::handler::server::router::tool::ToolRouter;

use crate::server::ArdoiseServer;

/// What the write tools use from the app (nothing yet).
pub trait WriteServices: Send + Sync {}

/// The services of a host that has none: the library operations of `ardoise-core` need none.
pub struct NoWriteServices;

impl WriteServices for NoWriteServices {}

impl ArdoiseServer {
    /// The write tools (none yet).
    pub(crate) fn write_router() -> ToolRouter<ArdoiseServer> {
        ToolRouter::new()
    }
}
