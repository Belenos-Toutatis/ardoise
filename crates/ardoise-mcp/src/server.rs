//! The MCP server: which tools it serves, and the handler rmcp calls.
//!
//! Tools come in groups, each in its own file with its own router (`read.rs`, `write.rs`,
//! `bridge.rs`); [`build_tool_router`] is the one place that decides which groups a server
//! has, so the `--read-only` switch is enforced here and nowhere else.

use std::sync::Arc;

use rmcp::handler::server::router::prompt::PromptRouter;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ServerHandler, prompt_handler, tool_handler};

use crate::error::{ToolError, ToolResult};
use crate::host::Host;

/// How the server runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ServerOptions {
    /// `--read-only`: only the read tools are served.
    pub read_only: bool,
}

/// What the assistant is told when it connects.
const INSTRUCTIONS: &str = "Ardoise is a notebook app for handwritten notes (Wacom Bamboo Slate). \
These tools read the user's library: list and search notes, read a note's data, its recognised \
text with positions, and render it as an image so you can see drawings, diagrams and layout \
that the text recognition misses. Notes are named by `id` or by their path relative to the \
library. Coordinates are millimetres from the top-left corner of the page. What you read is \
sent to your provider: the notes may hold names of people. Unless the server is read-only, \
other tools create, rename, tag, move, merge, split, straighten and export notes and store a \
corrected transcription for the search: nothing is overwritten or deleted for good (a trashed \
note stays restorable from the app's trash; an export that would overwrite a file gets (2) \
added to its name). The ink of a note is never changed in the note itself: `preview_revision` \
tries a list of orders in memory, `create_revision` writes them on a copy next to the note \
(straightening shapes does the same), and only a copy made that way can be edited again, with \
`edit_revision` (its version before goes to the trash). Prompts (clean up a note, title and tag \
untitled notes, summarise a lesson, improve a note on a copy) say which tools to call and in \
which order.";

/// The tools of the groups a server with `options` has.
pub fn build_tool_router(options: ServerOptions) -> ToolRouter<ArdoiseServer> {
    let router = ArdoiseServer::read_router();
    if options.read_only {
        return router;
    }
    router + ArdoiseServer::write_router() + ArdoiseServer::bridge_router()
}

/// The server: the host's services, the options and the routers.
#[derive(Clone)]
pub struct ArdoiseServer {
    pub(crate) host: Arc<Host>,
    options: ServerOptions,
    tool_router: ToolRouter<ArdoiseServer>,
    prompt_router: PromptRouter<ArdoiseServer>,
}

impl ArdoiseServer {
    pub fn new(host: Host, options: ServerOptions) -> ArdoiseServer {
        // The prompts follow the language of the app as it is now.
        let language = host.environment.language();
        ArdoiseServer {
            host: Arc::new(host),
            options,
            tool_router: build_tool_router(options),
            prompt_router: Self::prompt_router(language, options.read_only),
        }
    }

    pub fn options(&self) -> ServerOptions {
        self.options
    }

    /// Runs `work` on a thread of the blocking pool (the library, the recognition and the
    /// rendering are synchronous and may take seconds: the protocol must keep answering) and
    /// turns its failure into a failed tool result.
    pub(crate) async fn blocking(
        &self,
        work: impl FnOnce(&Host) -> ToolResult + Send + 'static,
    ) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
        let host = Arc::clone(&self.host);
        let outcome = tokio::task::spawn_blocking(move || work(&host))
            .await
            .map_err(|error| rmcp::ErrorData::internal_error(error.to_string(), None))?;
        Ok(outcome.unwrap_or_else(ToolError::into_result))
    }
}

#[tool_handler(router = self.tool_router)]
#[prompt_handler(router = self.prompt_router)]
impl ServerHandler for ArdoiseServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .build(),
        )
        .with_server_info(Implementation::new("ardoise", env!("CARGO_PKG_VERSION")))
        .with_instructions(INSTRUCTIONS)
    }
}
