//! The MCP server of Ardoise (SPEC §4.9, design in `docs/design/mcp.md`): the user's own
//! assistant (Claude Code, Claude Desktop…) reads the library, sees the notes as images and
//! reads their recognised text, and (with the write tools) files and exports them. Ardoise
//! contains no AI; the assistant brings it.
//!
//! - [`server`]: the handler and the one place where tool groups are chosen (`--read-only`);
//! - [`host`]: the services the server takes from the app around it (library, handwriting
//!   cache, and the write and bridge services of later groups);
//! - [`read`], [`write`], [`bridge`], [`prompts`]: one file per group of tools or prompts;
//! - [`notes`], [`strokes`]: naming notes (id or path), millimetres, dates, stroke summaries;
//! - [`error`]: tool failures, `{ code, message }` with the interface's `errorCodes.*`.
//!
//! The transport is standard input/output (JSON-RPC, one message per line): nothing but protocol
//! may be written to stdout.

pub mod bridge;
pub mod error;
pub mod host;
pub mod notes;
pub mod prompts;
pub mod read;
pub mod server;
pub mod strokes;
pub mod write;

use rmcp::RoleServer;
use rmcp::service::ServiceExt;
use rmcp::transport::IntoTransport;

pub use error::{ToolError, ToolResult};
pub use host::{Environment, Handwriting, Host, TextHit, TextSearch, TextStatus};
pub use server::{ArdoiseServer, ServerOptions, build_tool_router};

/// The server could not start, or stopped on a failure.
#[derive(Debug, thiserror::Error)]
#[error("MCP server: {0}")]
pub struct ServeError(pub String);

/// Serves the MCP protocol on `transport` until the client closes it.
pub async fn serve<T, E, A>(
    host: Host,
    options: ServerOptions,
    transport: T,
) -> Result<(), ServeError>
where
    T: IntoTransport<RoleServer, E, A>,
    E: std::error::Error + Send + Sync + 'static,
{
    let running = ArdoiseServer::new(host, options)
        .serve(transport)
        .await
        .map_err(|error| ServeError(error.to_string()))?;
    running
        .waiting()
        .await
        .map_err(|error| ServeError(error.to_string()))?;
    Ok(())
}

/// Serves on standard input/output until stdin closes.
pub async fn serve_stdio(host: Host, options: ServerOptions) -> Result<(), ServeError> {
    serve(host, options, rmcp::transport::stdio()).await
}
