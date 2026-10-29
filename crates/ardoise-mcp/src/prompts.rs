//! The prompts (`mettre_au_propre` / `clean_up`, `titrer_et_etiqueter` / `title_and_tag`,
//! `resume_seance` / `lesson_summary`): they become slash commands in the assistant. Texts in
//! French and English following [`Environment::language`](crate::host::Environment::language),
//! each naming the tools to use.
//!
//! To add them: write `#[prompt]` functions in an
//! `#[prompt_router(router = prompts, vis = "pub(crate)")] impl ArdoiseServer` block and make
//! [`ArdoiseServer::prompt_router`] below return `Self::prompts()`; [`crate::server`] already
//! serves that router.

use rmcp::handler::server::router::prompt::PromptRouter;

use crate::server::ArdoiseServer;

impl ArdoiseServer {
    /// The prompts (none yet).
    pub(crate) fn prompt_router() -> PromptRouter<ArdoiseServer> {
        PromptRouter::new()
    }
}
