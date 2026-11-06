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

const FR: [&str; 4] = [
    "ameliorer_note",
    "mettre_au_propre",
    "resume_seance",
    "titrer_et_etiqueter",
];
/// What a read-only server has: the prompt that writes is not there.
const FR_READ_ONLY: [&str; 3] = ["mettre_au_propre", "resume_seance", "titrer_et_etiqueter"];
const EN_READ_ONLY: [&str; 3] = ["clean_up", "lesson_summary", "title_and_tag"];
const EN: [&str; 4] = [
    "clean_up",
    "improve_note",
    "lesson_summary",
    "title_and_tag",
];

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
    // A read-only server has the ones that only read.
    let read_only = session_with(Language::Fr, ServerOptions { read_only: true }).await;
    assert_eq!(names(&read_only).await, FR_READ_ONLY);
    let read_only_en = session_with(Language::En, ServerOptions { read_only: true }).await;
    assert_eq!(names(&read_only_en).await, EN_READ_ONLY);

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

/// The names of the tools `session`'s server has.
async fn tools(session: &Session) -> Vec<String> {
    session
        .client
        .list_all_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name.to_string())
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn a_read_only_server_s_prompts_name_only_the_tools_it_has() {
    let every = tools(&session().await).await;
    for (language, clean, title, lesson, by_hand) in [
        (
            Language::Fr,
            "mettre_au_propre",
            "titrer_et_etiqueter",
            "resume_seance",
            "c'est l'utilisateur qui donne les titres et les étiquettes dans Ardoise",
        ),
        (
            Language::En,
            "clean_up",
            "title_and_tag",
            "lesson_summary",
            "the user gives the titles and tags in Ardoise",
        ),
    ] {
        let session = session_with(language, ServerOptions { read_only: true }).await;
        let present = tools(&session).await;
        let absent: Vec<&String> = every.iter().filter(|t| !present.contains(t)).collect();
        assert!(
            absent.iter().any(|t| *t == "update_note")
                && absent.iter().any(|t| *t == "last_live_note")
        );
        let mut texts = Vec::new();
        for format in ["markdown", "word", "latex"] {
            texts.push(session.prompt(clean, json!({ "format": format })).await);
            texts.push(
                session
                    .prompt(clean, json!({ "note": OPTIQUE, "format": format }))
                    .await,
            );
        }
        let titles = session.prompt(title, json!({})).await;
        assert!(titles.contains(by_hand), "{titles}");
        texts.push(titles);
        texts.push(
            session
                .prompt(lesson, json!({ "date": "2026-10-24" }))
                .await,
        );
        for text in &texts {
            for tool in &absent {
                assert!(
                    !text.contains(&format!("`{tool}`")),
                    "`{tool}` is not served read-only:\n{text}"
                );
            }
        }
        // The reading tools are still named.
        assert!(texts[0].contains("`render_note`") && texts[0].contains("`search_notes`"));
    }
}

// ---------------------------------------------------------------------------------------------
// ameliorer_note / improve_note

#[tokio::test(flavor = "current_thread")]
async fn improving_a_note_goes_through_the_six_steps_in_order() {
    for (language, name, steps, rules) in [
        (
            Language::Fr,
            "ameliorer_note",
            [
                "1. Regarde.",
                "`get_note`",
                "`render_note`",
                "`ink_lines`",
                "`get_strokes`",
                "2. Dis ce que tu prévois",
                "3. Pour une lettre",
                "`copy`",
                "`glyph`",
                "4. Donne `after`",
                "`retime`",
                "5. `preview_revision`",
                "6. Quand l'image est bonne",
                "`create_revision`",
            ],
            &[
                "liste des fautes d'orthographe",
                "Ne corrige que celles que l'utilisateur valide",
                "Ne change jamais le sens",
                "rien n'est corrigé ni déplacé en silence",
                "reste sobre",
                "la même écriture dans cette note",
                "une note peut contenir deux écritures",
                "morceaux de cette écriture",
                "marque le mot en couleur",
                "Jamais de lettre prise dans une autre note",
                "la liste entière",
                "une sélection de la mauvaise taille est l'erreur la plus courante",
                "la `slope` que tu vois",
                "ne se range pas ligne par ligne",
                "`scale: 0.9`",
                "chaque lettre fabriquée",
                "l'original est intact",
                "`labels: true`",
                "`background.ruling`",
            ][..],
        ),
        (
            Language::En,
            "improve_note",
            [
                "1. Look.",
                "`get_note`",
                "`render_note`",
                "`ink_lines`",
                "`get_strokes`",
                "2. Say what you plan",
                "3. For a letter",
                "`copy`",
                "`glyph`",
                "4. Give `after`",
                "`retime`",
                "5. `preview_revision`",
                "6. When the picture is right",
                "`create_revision`",
            ],
            &[
                "spelling mistakes",
                "Correct only those the user validates",
                "Never change the meaning",
                "nothing is corrected or moved silently",
                "stays sober",
                "the same handwriting in this note",
                "a note can hold two hands",
                "pieces of that handwriting",
                "mark the word in colour",
                "Never a letter taken from another note",
                "the whole list",
                "a selection of the wrong size is the usual mistake",
                "give the `slope` you see",
                "not tidied line by line",
                "`scale: 0.9`",
                "each letter you made",
                "the original is intact",
                "`labels: true`",
                "`background.ruling`",
            ][..],
        ),
    ] {
        let session = session_with(language, ServerOptions::default()).await;
        let text = session
            .prompt(
                name,
                json!({ "note": OPTIQUE, "goals": "ranger l'écriture" }),
            )
            .await;
        in_order(&text, &steps);
        for rule in rules {
            assert!(text.contains(rule), "`{rule}` is missing:\n{text}");
        }
        // The note and the request are in the message; the tools named exist on this server.
        assert!(text.contains(OPTIQUE) && text.contains("ranger l'écriture"));
        let every = tools(&session).await;
        for tool in [
            "get_note",
            "render_note",
            "ink_lines",
            "get_strokes",
            "get_note_text",
            "preview_revision",
            "create_revision",
            "edit_revision",
            "last_live_note",
            "search_notes",
            "list_notes",
        ] {
            assert!(every.iter().any(|t| t == tool), "{tool} is not served");
        }
        // An ordinary note is copied, not edited.
        assert!(!text.contains("déjà une copie") && !text.contains("already is a retouched copy"));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn improving_a_note_asks_for_what_is_missing() {
    let session = session_with(Language::Fr, ServerOptions::default()).await;
    // No note: the last live session's, or the user's choice.
    let text = session.prompt("ameliorer_note", json!({})).await;
    assert!(text.contains("`last_live_note`"), "{text}");
    assert!(text.contains("demande-le-lui"), "no goals: {text}");
    // A note that does not exist.
    let text = session
        .prompt(
            "ameliorer_note",
            json!({ "note": "Inconnue", "goals": "x" }),
        )
        .await;
    assert!(text.contains("n'existe pas dans la bibliothèque"), "{text}");
    assert!(!text.contains("demande-le-lui"));
    // A blank request is no request.
    let text = session
        .prompt("ameliorer_note", json!({ "note": OPTIQUE, "goals": "   " }))
        .await;
    assert!(text.contains("demande-le-lui"));

    let en = session_with(Language::En, ServerOptions::default()).await;
    let text = en.prompt("improve_note", json!({})).await;
    assert!(text.contains("`last_live_note`") && text.contains("ask them"));
    // Both languages take the same arguments.
    for (session, name) in [(&session, "ameliorer_note"), (&en, "improve_note")] {
        let prompts = session.client.list_all_prompts().await.unwrap();
        let mut arguments: Vec<_> = prompts
            .iter()
            .find(|p| p.name == name)
            .unwrap()
            .arguments
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.name, a.required.unwrap_or(false)))
            .collect();
        arguments.sort();
        assert_eq!(
            arguments,
            [("goals".to_owned(), false), ("note".to_owned(), false)]
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_retouched_copy_is_edited_not_copied_again() {
    let session = session().await;
    let made = session
        .ok(
            "create_revision",
            json!({
                "note": OPTIQUE,
                "orders": [{ "op": "recolor", "select": { "ids": [0] }, "color": "#ff0000" }],
                "summary": "Rouge",
            }),
        )
        .await;
    let id = made["note"]["id"].as_str().unwrap();
    let text = session
        .prompt("improve_note", json!({ "note": id, "goals": "more" }))
        .await;
    assert!(
        text.contains("already is a retouched copy") && text.contains("`edit_revision`"),
        "{text}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_read_only_server_has_no_prompt_that_writes() {
    for (language, name) in [
        (Language::Fr, "ameliorer_note"),
        (Language::En, "improve_note"),
    ] {
        let session = session_with(language, ServerOptions { read_only: true }).await;
        assert!(session.try_prompt(name, json!({})).await.is_err());
        let listed = names(&session).await;
        assert!(!listed.iter().any(|n| n == name));
    }
}
