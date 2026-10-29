//! The prompts, through rmcp's in-memory transport: listed with the names and the texts of the
//! app's language, each message names its tools in the order they are used, and the lists of
//! notes it carries are the library's.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use ardoise_hwr::Language;
use ardoise_mcp::ServerOptions;
use rmcp::model::{GetPromptRequestParams, Role};
use serde_json::{Value, json};
use support::*;

const FR: [&str; 3] = ["mettre_au_propre", "resume_seance", "titrer_et_etiqueter"];
const EN: [&str; 3] = ["clean_up", "lesson_summary", "title_and_tag"];

impl Session {
    /// The text of the prompt `name` with `arguments`.
    async fn prompt(&self, name: &str, arguments: Value) -> String {
        match self.try_prompt(name, arguments).await {
            Ok(text) => text,
            Err(error) => panic!("{name} failed: {error}"),
        }
    }

    async fn try_prompt(&self, name: &str, arguments: Value) -> Result<String, String> {
        let mut params = GetPromptRequestParams::new(name);
        if let Some(arguments) = arguments.as_object() {
            params = params.with_arguments(arguments.clone());
        }
        let result = self
            .client
            .get_prompt(params)
            .await
            .map_err(|e| e.to_string())?;
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].role, Role::User);
        assert!(result.description.is_some());
        Ok(result.messages[0]
            .content
            .as_text()
            .expect("a text message")
            .text
            .clone())
    }
}

/// The position of each of `needles` in `text`, which must be found in this order.
fn in_order(text: &str, needles: &[&str]) {
    let mut from = 0;
    for needle in needles {
        match text[from..].find(needle) {
            Some(at) => from += at + needle.len(),
            None => panic!("`{needle}` is missing or out of order in:\n{text}"),
        }
    }
}

async fn names(session: &Session) -> Vec<String> {
    let mut names: Vec<String> = session
        .client
        .list_all_prompts()
        .await
        .unwrap()
        .into_iter()
        .map(|p| p.name)
        .collect();
    names.sort();
    names
}

#[tokio::test(flavor = "current_thread")]
async fn the_prompts_have_the_names_of_the_apps_language() {
    let fr = session_with(Language::Fr, ServerOptions::default()).await;
    assert_eq!(names(&fr).await, FR);
    let en = session_with(Language::En, ServerOptions::default()).await;
    assert_eq!(names(&en).await, EN);
    // A read-only server has them too: they only read.
    let read_only = session_with(Language::Fr, ServerOptions { read_only: true }).await;
    assert_eq!(names(&read_only).await, FR);

    for (session, language) in [(&fr, "fr"), (&en, "en")] {
        let prompts = session.client.list_all_prompts().await.unwrap();
        for prompt in &prompts {
            assert!(
                prompt.description.as_deref().is_some_and(|d| d.len() > 30),
                "{} has no description",
                prompt.name
            );
        }
        let arguments = |name: &str| -> Vec<(String, bool)> {
            prompts
                .iter()
                .find(|p| p.name == name)
                .unwrap()
                .arguments
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(|a| (a.name, a.required.unwrap_or(false)))
                .collect()
        };
        let (clean, title, lesson) = if language == "fr" {
            ("mettre_au_propre", "titrer_et_etiqueter", "resume_seance")
        } else {
            ("clean_up", "title_and_tag", "lesson_summary")
        };
        // Every argument is optional: `format` and `note` of the cleaning, `binder`, `date`.
        let mut clean_arguments = arguments(clean);
        clean_arguments.sort();
        assert_eq!(
            clean_arguments,
            [("format".to_owned(), false), ("note".to_owned(), false)]
        );
        assert_eq!(arguments(title), [("binder".to_owned(), false)]);
        let mut lesson_arguments = arguments(lesson);
        lesson_arguments.sort();
        assert_eq!(
            lesson_arguments,
            [("binder".to_owned(), false), ("date".to_owned(), false)]
        );
    }
    // The other language's name does not exist.
    assert!(fr.try_prompt("clean_up", json!({})).await.is_err());
    assert!(en.try_prompt("mettre_au_propre", json!({})).await.is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn cleaning_up_a_note_names_the_tools_in_the_order_of_the_work() {
    let en = session().await;
    let text = en
        .prompt("clean_up", json!({ "note": OPTIQUE, "format": "latex" }))
        .await;
    in_order(
        &text,
        &[
            "Cours d'optique",
            "get_note",
            "render_note",
            "region",
            "get_note_text",
            "Invent nothing",
            "export_note",
            "uncertain",
        ],
    );
    assert!(text.contains(OPTIQUE));
    assert!(text.contains("Physique/4e/2026-10-24 Cours d'optique.ardoise"));
    // LaTeX: LuaLaTeX and the packages of the lessons.
    for package in [
        "LuaLaTeX",
        "circuitikz",
        "mhchem",
        "siunitx",
        "pgfplots",
        "chemfig",
    ] {
        assert!(text.contains(package), "{package}");
    }
    // The note is named: no search for the last live note.
    assert!(!text.contains("last_live_note"));

    let fr = session_with(Language::Fr, ServerOptions::default()).await;
    let text = fr
        .prompt(
            "mettre_au_propre",
            json!({ "note": CHIMIE, "format": "word" }),
        )
        .await;
    in_order(
        &text,
        &[
            "Chimie des métaux",
            "get_note",
            "render_note",
            "get_note_text",
            "N'invente rien",
            "export_note",
            "incertain",
        ],
    );
    assert!(text.contains("Word"));
    assert!(!text.contains("circuitikz"), "only for LaTeX");
    let markdown = fr
        .prompt("mettre_au_propre", json!({ "note": OPTIQUE }))
        .await;
    assert!(markdown.contains("Markdown"));
}

#[tokio::test(flavor = "current_thread")]
async fn without_a_note_the_prompt_looks_for_the_last_live_one() {
    let en = session().await;
    let text = en.prompt("clean_up", json!({})).await;
    in_order(&text, &["last_live_note", "ask the user", "get_note"]);
    let fr = session_with(Language::Fr, ServerOptions::default()).await;
    let text = fr.prompt("mettre_au_propre", json!({})).await;
    in_order(
        &text,
        &["last_live_note", "demande à l'utilisateur", "get_note"],
    );

    // A note that does not exist: the prompt says to look for it, not to go on.
    let unknown = en.prompt("clean_up", json!({ "note": "Absente/x" })).await;
    assert!(unknown.contains("not in the library"));
    assert!(!unknown.contains("last_live_note"));
}

#[tokio::test(flavor = "current_thread")]
async fn untitled_notes_are_listed_and_changed_only_after_the_users_agreement() {
    let en = session().await;
    let text = en.prompt("title_and_tag", json!({})).await;
    in_order(
        &text,
        &[
            "render_note",
            "get_note_text",
            "list_tags",
            "ask for their agreement",
            "update_note",
        ],
    );
    // The only untitled note of the library is listed, the titled ones are not.
    assert!(text.contains(VIDE), "{text}");
    assert!(text.contains("Untitled notes in the library: 1"));
    assert!(!text.contains(OPTIQUE) && !text.contains(CHIMIE));

    // In a binder without such a note: none.
    let physique = en
        .prompt("title_and_tag", json!({ "binder": "Physique" }))
        .await;
    assert!(physique.contains("binder \"Physique\": 0"));
    assert!(physique.contains("(none)"));

    // Once the note has a title it is no longer listed.
    en.ok("update_note", json!({ "note": VIDE, "title": "Vide" }))
        .await;
    let after = en.prompt("title_and_tag", json!({})).await;
    assert!(after.contains("Untitled notes in the library: 0"));

    let fr = session_with(Language::Fr, ServerOptions::default()).await;
    let text = fr.prompt("titrer_et_etiqueter", json!({})).await;
    in_order(
        &text,
        &[
            "render_note",
            "get_note_text",
            "list_tags",
            "accord",
            "update_note",
        ],
    );
    assert!(text.contains(VIDE));
    assert!(text.contains("sans titre"));
}

/// The date (`YYYY-MM-DD`) of a note, as the tools show it.
async fn day_of(session: &Session, id: &str) -> String {
    let listed = session.ok("get_note", json!({ "note": id })).await;
    listed["note"]["created"].as_str().unwrap()[..10].to_owned()
}

#[tokio::test(flavor = "current_thread")]
async fn a_lesson_summary_carries_the_notes_of_the_day() {
    let en = session().await;
    let day = day_of(&en, OPTIQUE).await;
    let text = en.prompt("lesson_summary", json!({ "date": day })).await;
    in_order(
        &text,
        &[
            "class log",
            "get_note_text",
            "render_note",
            "Homework",
            "Change no note",
        ],
    );
    assert!(text.contains(OPTIQUE), "{text}");
    assert!(text.contains(&format!("Date of the lesson: {day}")));
    assert!(!text.contains(CHIMIE), "created another day");

    // A binder alone: its most recent day.
    let latest = en
        .prompt("lesson_summary", json!({ "binder": "Physique" }))
        .await;
    assert!(latest.contains(CHIMIE), "{latest}");
    assert!(!latest.contains(OPTIQUE));
    assert!(latest.contains("Class (binder): Physique"));

    // A day without a note, today by default: the prompt says so.
    let none = en
        .prompt("lesson_summary", json!({ "date": "2001-01-01" }))
        .await;
    assert!(none.contains("No note was found"));
    let today = en.prompt("lesson_summary", json!({})).await;
    let local_today = chrono::Local::now().format("%Y-%m-%d").to_string();
    assert!(
        today.contains(&format!("Date of the lesson: {local_today}")),
        "{today}"
    );

    // A date that is not one is an error of the request.
    let error = en
        .try_prompt("lesson_summary", json!({ "date": "demain" }))
        .await
        .unwrap_err();
    assert!(error.contains("ISO date"), "{error}");

    let fr = session_with(Language::Fr, ServerOptions::default()).await;
    let day = day_of(&fr, DEVOIR).await;
    let text = fr.prompt("resume_seance", json!({ "date": day })).await;
    in_order(
        &text,
        &[
            "cahier de textes",
            "get_note_text",
            "render_note",
            "Contenu de la séance",
            "Travail à faire",
        ],
    );
    assert!(text.contains(DEVOIR));
}
