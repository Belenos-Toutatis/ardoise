//! Errors of the tools: a stable `code` (the interface's `errorCodes.*`, plus the few codes of
//! the server itself) and an English `message`. A failing tool answers a normal result with
//! `isError: true` and `{ "code": ..., "message": ... }` as its content, so that the assistant
//! reads the reason (a protocol error would be shown to it as an opaque failure).

use ardoise_core::format::FormatError;
use ardoise_core::library::LibraryError;
use ardoise_hwr::HwrError;
use ardoise_render::RenderError;
use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::json;

/// Codes that only the MCP server uses (the others are those of `errorCodes.*` in the interface).
pub mod code {
    /// An argument is missing, out of range or not understood.
    pub const INVALID_ARGUMENTS: &str = "invalidArguments";
    /// The tool needs the running app (`app_state`, `open_note`…) and it is not open.
    pub const APP_NOT_RUNNING: &str = "appNotRunning";
    /// The server was started with `--read-only`.
    pub const READ_ONLY: &str = "readOnly";
    /// The app is open but did not answer in time (busy, or its window still loading), or the
    /// connection to it was lost.
    pub const APP_NOT_RESPONDING: &str = "appNotResponding";
    /// The app cannot do it now: unsaved changes in the editor, live mode on screen, a dialog
    /// open… (the message says what).
    pub const APP_BUSY: &str = "appBusy";
    /// The open app and this server speak different versions of the bridge.
    pub const BRIDGE_VERSION: &str = "bridgeVersion";
    /// A line received on the bridge that is not a request (unreadable, unknown kind, too long).
    pub const INVALID_REQUEST: &str = "invalidRequest";
}

/// A tool failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {message}")]
pub struct ToolError {
    pub code: &'static str,
    pub message: String,
}

impl ToolError {
    pub fn new(code: &'static str, message: impl ToString) -> Self {
        ToolError {
            code,
            message: message.to_string(),
        }
    }

    /// A bad argument.
    pub fn invalid(message: impl ToString) -> Self {
        ToolError::new(code::INVALID_ARGUMENTS, message)
    }

    /// The failed tool result: `isError: true` and the error as structured content and as text.
    pub fn into_result(self) -> CallToolResult {
        CallToolResult::structured_error(json!({ "code": self.code, "message": self.message }))
    }
}

impl From<LibraryError> for ToolError {
    fn from(error: LibraryError) -> Self {
        ToolError::new(error.code(), error)
    }
}

impl From<FormatError> for ToolError {
    fn from(error: FormatError) -> Self {
        ToolError::new(error.code(), error)
    }
}

impl From<HwrError> for ToolError {
    fn from(error: HwrError) -> Self {
        ToolError::new(error.code(), error)
    }
}

impl From<RenderError> for ToolError {
    fn from(error: RenderError) -> Self {
        ToolError::new("render", error)
    }
}

/// What a tool computes: a result, or an error that [`ToolError::into_result`] reports.
pub type ToolResult = Result<CallToolResult, ToolError>;

/// A successful result holding `value` (a JSON object) as text and as structured content.
pub fn json_result(value: impl serde::Serialize) -> ToolResult {
    let value = serde_json::to_value(value).map_err(|error| {
        ToolError::new("unexpected", format!("cannot encode the result: {error}"))
    })?;
    Ok(CallToolResult::structured(value))
}

/// A successful result made of several content blocks.
pub fn content_result(content: Vec<ContentBlock>) -> ToolResult {
    Ok(CallToolResult::success(content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_becomes_a_failed_result_with_its_code() {
        let result = ToolError::new("noteNotFound", "no such note").into_result();
        assert_eq!(result.is_error, Some(true));
        let value = result.structured_content.expect("structured");
        assert_eq!(value["code"], "noteNotFound");
        assert_eq!(value["message"], "no such note");
    }

    #[test]
    fn library_errors_keep_the_interface_codes() {
        let error = LibraryError::NotFound {
            path: "a.ardoise".into(),
        };
        assert_eq!(ToolError::from(error).code, "noteNotFound");
        assert_eq!(
            ToolError::from(HwrError::Unsupported).code,
            "handwritingUnsupported"
        );
    }
}
