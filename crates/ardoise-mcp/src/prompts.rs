//! The prompts (`mettre_au_propre` / `clean_up`, `titrer_et_etiqueter` / `title_and_tag`,
//! `resume_seance` / `lesson_summary`): they become slash commands in the assistant. Each is a
//! message that tells the assistant which tools to call, and in which order, for a French
//! teacher's notes.
//!
//! The names and the texts follow [`Environment::language`](crate::host::Environment::language)
//! as it is when the server starts: a French app serves the French names, an English one the
//! English names (the router is built once, [`ArdoiseServer::prompt_router`]). The prompts that
//! are about a set of notes put the list of those notes in the message, so that the assistant
//! does not have to look for them. A read-only server (`--read-only`) has neither the write
//! tools nor the bridge tools: its prompts name none of them, and leave to the user what would
//! change the library (the list of titles to apply by hand).

use ardoise_core::library::NoteEntry;
use ardoise_hwr::Language;
use rmcp::handler::server::router::prompt::PromptRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{GetPromptResult, PromptMessage, Role};
use rmcp::{ErrorData, prompt, prompt_router};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::host::Host;
use crate::notes::{binder_text, find_note, iso, parse_date};
use crate::read::{clean_binder, in_binder};
use crate::server::ArdoiseServer;

/// Notes listed in the message of a prompt (the others are found with `list_notes`).
const LISTED: usize = 40;

/// The document the clean version is written as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CleanFormat {
    /// Markdown (default).
    #[default]
    Markdown,
    /// Word document (.docx).
    Word,
    /// LaTeX document, compiled with LuaLaTeX.
    Latex,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct CleanUpArgs {
    /// Note to clean up: its id or its path in the library. Omitted: the note of the last live session.
    pub note: Option<String>,
    /// `markdown` (default), `word` or `latex`.
    pub format: Option<CleanFormat>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct TitleAndTagArgs {
    /// Only the notes of this binder and its sub-binders. Omitted: the whole library.
    pub binder: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct LessonSummaryArgs {
    /// The day of the lesson, `2026-10-12` (default: today).
    pub date: Option<String>,
    /// The class's binder. Alone: its most recent lesson.
    pub binder: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// What a prompt knows of the library

/// The note a prompt is about, as the library shows it.
struct NoteRef {
    id: String,
    path: String,
    title: String,
}

/// How a note is named in a list: its title, or its date when it has none.
fn label(entry: &NoteEntry) -> String {
    match entry.title.trim() {
        "" => iso(entry.created)
            .chars()
            .take(16)
            .collect::<String>()
            .replace('T', " "),
        title => title.to_owned(),
    }
}

/// A line of a list of notes.
fn line_of(host: &Host, entry: &NoteEntry) -> String {
    let path = host
        .library()
        .map(|library| crate::notes::relative_path(&library, &entry.path))
        .unwrap_or_default();
    format!(
        "- {path} (id {}) - {}, {}",
        entry.id,
        label(entry),
        iso(entry.created)
    )
}

// ---------------------------------------------------------------------------------------------
// Texts

fn format_name(language: Language, format: CleanFormat) -> &'static str {
    match (language, format) {
        (_, CleanFormat::Markdown) => "Markdown",
        (Language::Fr, CleanFormat::Word) => "Word (.docx)",
        (Language::En, CleanFormat::Word) => "Word (.docx)",
        (_, CleanFormat::Latex) => "LaTeX (LuaLaTeX)",
    }
}

/// What the format asks for in step 3; `read_only`: without `export_note`, the drawings are
/// exported by the user from the app.
fn format_rules(language: Language, format: CleanFormat, read_only: bool) -> String {
    match (language, format) {
        (Language::Fr, CleanFormat::Markdown) => format!(
            "Format Markdown : titres `#`, listes, tableaux Markdown, formules en LaTeX entre `$...$` (et `$$...$$` pour une formule seule sur sa ligne), unités écrites correctement. Les schémas simples se redessinent en `mermaid` ou en tableau ; pour les autres, laisse un emplacement `![description](schema-N.png)` et dis à la fin quels schémas sont à insérer ({}).",
            if read_only {
                "l'utilisateur peut exporter les dessins tels qu'Ardoise les voit avec l'export du texte en Markdown de l'application"
            } else {
                "`export_note` au format `md` écrit les dessins tels qu'Ardoise les voit, dans un dossier à côté"
            }
        ),
        (Language::En, CleanFormat::Markdown) => format!(
            "Markdown format: `#` headings, lists, Markdown tables, formulas in LaTeX between `$...$` (and `$$...$$` for a formula on its own line), units written properly. Simple diagrams can be redrawn as `mermaid` or as a table; for the others leave a placeholder `![description](diagram-N.png)` and say at the end which diagrams have to be inserted ({}).",
            if read_only {
                "the user can export the drawings as Ardoise sees them with the app's Markdown text export"
            } else {
                "`export_note` with format `md` writes the drawings as Ardoise sees them, in a folder next to the file"
            }
        ),
        (Language::Fr, CleanFormat::Word) => format!(
            "Format Word : produis un fichier .docx si tes outils le permettent (styles Titre 1, Titre 2, listes, tableaux, équations), sinon donne le contenu en Markdown propre, prêt à être converti (par exemple avec pandoc), et dis-le. Pour un schéma que tu ne peux pas redessiner, mets un emplacement et signale-le ; {}",
            if read_only {
                "l'export du texte en Word de l'application garde les dessins tels qu'Ardoise les voit."
            } else {
                "`export_note` au format `docx` garde les dessins tels qu'Ardoise les voit."
            }
        ),
        (Language::En, CleanFormat::Word) => format!(
            "Word format: produce a .docx file if your tools allow it (Heading 1, Heading 2 styles, lists, tables, equations), otherwise give the content as clean Markdown ready to convert (for example with pandoc), and say so. For a diagram you cannot redraw, leave a placeholder and flag it; {}",
            if read_only {
                "the app's Word text export keeps the drawings as Ardoise sees them."
            } else {
                "`export_note` with format `docx` keeps the drawings as Ardoise sees them."
            }
        ),
        (Language::Fr, CleanFormat::Latex) => {
            "Format LaTeX : un document complet qui se compile avec LuaLaTeX (classe `article` ou `scrartcl`, `fontspec`, `babel` en français). Paquets : `siunitx` pour les nombres et les unités (`\\num`, `\\unit`, `\\qty`), `mhchem` pour les formules et équations chimiques (`\\ce{H2O}`), `circuitikz` pour les schémas électriques, `pgfplots` pour les courbes et graphiques, `chemfig` pour les formules développées, `tikz` pour les autres schémas. Les schémas sont redessinés en code (TikZ, circuitikz...), pas laissés en image. Si tu peux compiler, compile et corrige les erreurs avant de répondre.".to_owned()
        }
        (Language::En, CleanFormat::Latex) => {
            "LaTeX format: a complete document that compiles with LuaLaTeX (`article` or `scrartcl` class, `fontspec`, `babel`). Packages: `siunitx` for numbers and units (`\\num`, `\\unit`, `\\qty`), `mhchem` for chemical formulas and equations (`\\ce{H2O}`), `circuitikz` for circuit diagrams, `pgfplots` for curves and graphs, `chemfig` for structural formulas, `tikz` for the other diagrams. Diagrams are redrawn as code (TikZ, circuitikz...), not left as images. If you can compile, compile and fix the errors before answering.".to_owned()
        }
    }
}

fn clean_up_text(
    language: Language,
    note: Option<&NoteRef>,
    asked: bool,
    format: CleanFormat,
    read_only: bool,
) -> String {
    let format_name = format_name(language, format);
    let rules = format_rules(language, format, read_only);
    match language {
        Language::Fr => {
            let which = match (note, asked) {
                (Some(note), _) => format!(
                    "La note à mettre au propre : « {} » (id `{}`, chemin `{}`).",
                    if note.title.trim().is_empty() { "sans titre" } else { note.title.trim() },
                    note.id,
                    note.path
                ),
                (None, true) => "La note demandée n'existe pas dans la bibliothèque : retrouve-la avec `search_notes` ou `list_notes`, ou demande à l'utilisateur laquelle il veut, avant de continuer.".to_owned(),
                (None, false) if read_only => "Aucune note n'est désignée : demande à l'utilisateur laquelle mettre au propre (`search_notes` et `list_notes` aident à la retrouver ; `list_notes` donne les plus récentes d'abord) avant de continuer.".to_owned(),
                (None, false) => "Aucune note n'est désignée : appelle l'outil `last_live_note` pour trouver la note de la dernière séance en direct. S'il n'y en a pas (ou si Ardoise n'est pas ouverte), demande à l'utilisateur laquelle mettre au propre (`search_notes` et `list_notes` aident à la retrouver) avant de continuer.".to_owned(),
            };
            let export = if read_only {
                "Ce serveur est en lecture seule : il n'écrit aucun fichier."
            } else {
                "`export_note` peut aussi fournir un point de départ (formats `md` ou `docx`) ; il n'écrase jamais un fichier et rend les chemins écrits."
            };
            format!(
                "Tu aides un enseignant de physique-chimie (collège) à mettre au propre une note manuscrite d'Ardoise. {which}\n\
                 Format demandé : {format_name}.\n\n\
                 Procède dans cet ordre.\n\
                 1. Regarde la note. Appelle `get_note` (avec `strokes: \"summary\"`) pour la taille de la page, les calques et les traits, puis `render_note` pour la page entière à une définition lisible (dpi 100 à 150). Zoome ensuite avec `render_note` et `region` (en millimètres depuis le coin haut gauche de la page) sur les zones denses, les formules, les schémas et les graphiques, avec un dpi plus élevé (200 à 300). La reconnaissance d'écriture se trompe sur les formules, les symboles et les schémas : l'image fait foi.\n\
                 2. Lis le texte reconnu avec `get_note_text` : chaque ligne a sa position en millimètres, ce qui permet de la retrouver sur l'image, et la structure (paragraphes, listes, dessins avec leur zone et leurs traits) aide à retrouver la mise en page. Corrige-le d'après l'image.\n\
                 3. Écris un document propre et fidèle. N'invente rien : ne complète pas le cours, ne corrige pas silencieusement une erreur de fond (signale-la à la fin), ne change ni les valeurs ni les unités. Garde l'ordre et la hiérarchie de la page : titres, listes, tableaux, formules, unités ; les schémas (circuits, montages, rayons lumineux, molécules, courbes) sont redessinés avec les moyens du format. {rules}\n\
                 4. Enregistre le document avec tes propres outils de fichiers si tu en as, à l'endroit que l'utilisateur indique (par défaut son dossier de travail), sans écraser un fichier existant. Sinon donne-le dans ta réponse. {export}\n\
                 5. Termine par la liste de ce qui reste incertain : mots illisibles, symboles ou valeurs devinés, schémas simplifiés, erreurs de fond rencontrées, avec leur position sur la page (en millimètres) pour que l'utilisateur vérifie.\n\n\
                 Les notes peuvent contenir des noms d'élèves : ne les recopie que s'ils font partie du document demandé."
            )
        }
        Language::En => {
            let which = match (note, asked) {
                (Some(note), _) => format!(
                    "The note to clean up: \"{}\" (id `{}`, path `{}`).",
                    if note.title.trim().is_empty() { "untitled" } else { note.title.trim() },
                    note.id,
                    note.path
                ),
                (None, true) => "The note asked for is not in the library: find it with `search_notes` or `list_notes`, or ask the user which one they mean, before going on.".to_owned(),
                (None, false) if read_only => "No note is named: ask the user which note to clean up (`search_notes` and `list_notes` help to find it; `list_notes` gives the newest first) before going on.".to_owned(),
                (None, false) => "No note is named: call the tool `last_live_note` to find the note of the last live session. If there is none (or Ardoise is not open), ask the user which note to clean up (`search_notes` and `list_notes` help to find it) before going on.".to_owned(),
            };
            let export = if read_only {
                "This server is read-only: it writes no file."
            } else {
                "`export_note` can also give a starting point (formats `md` or `docx`); it never overwrites a file and returns the paths written."
            };
            format!(
                "You are helping a physics and chemistry teacher (lower secondary school) turn a handwritten Ardoise note into a clean document. {which}\n\
                 Requested format: {format_name}.\n\n\
                 Proceed in this order.\n\
                 1. Look at the note. Call `get_note` (with `strokes: \"summary\"`) for the page size, the layers and the strokes, then `render_note` for the whole page at a readable resolution (100 to 150 dpi). Then zoom with `render_note` and `region` (in millimetres from the top-left corner of the page) on dense areas, formulas, diagrams and graphs, at a higher dpi (200 to 300). Handwriting recognition gets formulas, symbols and diagrams wrong: the image is the reference.\n\
                 2. Read the recognised text with `get_note_text`: each line has its position in millimetres, which lets you find it on the image, and the structure (paragraphs, list items, drawings with their area and strokes) helps to recover the layout. Correct it from the image.\n\
                 3. Write a faithful, clean document. Invent nothing: do not complete the lesson, do not silently fix a mistake of substance (flag it at the end), change neither values nor units. Keep the order and hierarchy of the page: titles, lists, tables, formulas, units; diagrams (circuits, set-ups, light rays, molecules, curves) are redrawn with the means of the format. {rules}\n\
                 4. Save the document with your own file tools if you have them, where the user says (by default their working folder), without overwriting an existing file. Otherwise give it in your reply. {export}\n\
                 5. End with the list of what remains uncertain: unreadable words, guessed symbols or values, simplified diagrams, mistakes of substance, with their position on the page (in millimetres) so that the user can check.\n\n\
                 Notes may contain students' names: copy them only if they belong in the requested document."
            )
        }
    }
}

fn list_text(language: Language, lines: &[String], total: usize) -> String {
    if total == 0 {
        return match language {
            Language::Fr => "(aucune)".to_owned(),
            Language::En => "(none)".to_owned(),
        };
    }
    let mut text = lines.join("\n");
    if total > lines.len() {
        text.push_str(&match language {
            Language::Fr => format!(
                "\n(... et {} autres : `list_notes` les donne)",
                total - lines.len()
            ),
            Language::En => format!(
                "\n(... and {} more: `list_notes` gives them)",
                total - lines.len()
            ),
        });
    }
    text
}

fn title_and_tag_text(
    language: Language,
    binder: &str,
    lines: &[String],
    total: usize,
    read_only: bool,
) -> String {
    let list = list_text(language, lines, total);
    match language {
        Language::Fr => {
            let scope = if binder.is_empty() {
                "de la bibliothèque".to_owned()
            } else {
                format!("du classeur « {binder} »")
            };
            let apply = if read_only {
                "3. Montre la liste complète à l'utilisateur (note, titre proposé, étiquettes proposées), prête à appliquer : ce serveur est en lecture seule, c'est l'utilisateur qui donne les titres et les étiquettes dans Ardoise (le titre et les étiquettes de chaque note, dans la bibliothèque ou l'éditeur). Il peut corriger ou retirer des lignes. Ne propose rien pour les notes qui ont déjà un titre.\n\
                 4. Termine en rappelant que rien n'a été modifié."
            } else {
                "3. Montre la liste complète à l'utilisateur (note, titre proposé, étiquettes proposées) et demande-lui son accord : il peut corriger ou retirer des lignes. N'appelle `update_note` (avec `title` et `add_tags`) qu'après son accord, une fois par note. Ne touche pas aux notes qui ont déjà un titre.\n\
                 4. Résume ce qui a été appliqué."
            };
            format!(
                "Tu aides à donner un titre et des étiquettes aux notes d'Ardoise qui n'en ont pas. Une note « sans titre » a un titre vide (l'application affiche alors sa date de création).\n\n\
                 Notes sans titre {scope} : {total}, les plus récentes d'abord.\n{list}\n\n\
                 Procède dans cet ordre.\n\
                 1. Pour chaque note de la liste (si elle est incomplète, `list_notes` avec `binder` donne les autres : ne garde que celles dont `title` est vide), appelle `render_note` (page entière, dpi 80 à 100) pour la voir et `get_note_text` pour son texte reconnu. L'image fait foi : la reconnaissance se trompe sur les formules et les schémas. Une note vide ou illisible n'a rien à recevoir : dis-le.\n\
                 2. Propose un titre court et précis (3 à 8 mots, sans la date, qui dit de quoi parle la note : « Loi d'Ohm - exercices ») et 1 à 4 étiquettes courtes en minuscules. Regarde d'abord les étiquettes qui existent (`list_tags`) et réutilise-les plutôt que d'en créer de proches. N'utilise aucun nom d'élève.\n\
                 {apply}"
            )
        }
        Language::En => {
            let scope = if binder.is_empty() {
                "in the library".to_owned()
            } else {
                format!("in the binder \"{binder}\"")
            };
            let apply = if read_only {
                "3. Show the user the complete list (note, proposed title, proposed tags), ready to apply: this server is read-only, so the user gives the titles and tags in Ardoise (each note's title and tags, in the library or the editor). They can correct or remove lines. Propose nothing for notes that already have a title.\n\
                 4. End by recalling that nothing was changed."
            } else {
                "3. Show the user the complete list (note, proposed title, proposed tags) and ask for their agreement: they can correct or remove lines. Call `update_note` (with `title` and `add_tags`) only after they agree, once per note. Do not touch notes that already have a title.\n\
                 4. Summarise what was applied."
            };
            format!(
                "You are helping to give a title and tags to the Ardoise notes that have none. An \"untitled\" note has an empty title (the app then shows its creation date).\n\n\
                 Untitled notes {scope}: {total}, newest first.\n{list}\n\n\
                 Proceed in this order.\n\
                 1. For each note of the list (if it is incomplete, `list_notes` with `binder` gives the others: keep only those whose `title` is empty), call `render_note` (whole page, 80 to 100 dpi) to see it and `get_note_text` for its recognised text. The image is the reference: recognition gets formulas and diagrams wrong. An empty or unreadable note has nothing to receive: say so.\n\
                 2. Propose a short, precise title (3 to 8 words, without the date, saying what the note is about: \"Ohm's law - exercises\") and 1 to 4 short lowercase tags. Look at the existing tags first (`list_tags`) and reuse them instead of creating close ones. Use no student's name.\n\
                 {apply}"
            )
        }
    }
}

fn lesson_summary_text(
    language: Language,
    day: &str,
    binder: &str,
    lines: &[String],
    total: usize,
) -> String {
    let list = list_text(language, lines, total);
    match language {
        Language::Fr => {
            let class = if binder.is_empty() {
                String::new()
            } else {
                format!(" Classe (classeur) : {binder}.")
            };
            let none = if total == 0 {
                "\nAucune note n'a été trouvée pour cette séance : dis-le à l'utilisateur et demande-lui une autre date ou un autre classeur (`list_binders` et `list_notes` aident à les retrouver) avant de continuer.\n"
            } else {
                ""
            };
            format!(
                "Tu rédiges, pour le cahier de textes, un court résumé d'une séance d'après les notes d'Ardoise. Date de la séance : {day}.{class}\n\n\
                 Notes de la séance, dans l'ordre où elles ont été prises ({total}) :\n{list}\n{none}\n\
                 Procède dans cet ordre.\n\
                 1. Pour chaque note, lis `get_note_text` (structure comprise) et regarde `render_note` pour les schémas, formules et tableaux : l'image fait foi, la reconnaissance se trompe sur les formules et les symboles.\n\
                 2. Rédige, prêt à copier :\n\
                 - une ligne de titre pour la séance ;\n\
                 - « Contenu de la séance » : 3 à 6 puces courtes (notions, définitions, expériences, exercices), au plus près de ce qui est écrit, avec le vocabulaire du programme de collège ;\n\
                 - « Travail à faire » et « À retenir » seulement si c'est écrit dans les notes ; sinon n'en invente pas et dis qu'il n'y en a pas.\n\
                 Phrases courtes, au présent, environ 80 mots en tout, aucun nom d'élève.\n\
                 3. Termine par ce qui reste incertain (mots illisibles, valeurs douteuses) en une ligne. Ne modifie aucune note et n'enregistre rien."
            )
        }
        Language::En => {
            let class = if binder.is_empty() {
                String::new()
            } else {
                format!(" Class (binder): {binder}.")
            };
            let none = if total == 0 {
                "\nNo note was found for this lesson: tell the user and ask for another date or binder (`list_binders` and `list_notes` help to find them) before going on.\n"
            } else {
                ""
            };
            format!(
                "You are writing, for the class log (cahier de textes), a short summary of a lesson from the Ardoise notes. Date of the lesson: {day}.{class}\n\n\
                 Notes of the lesson, in the order they were taken ({total}):\n{list}\n{none}\n\
                 Proceed in this order.\n\
                 1. For each note, read `get_note_text` (structure included) and look at `render_note` for diagrams, formulas and tables: the image is the reference, recognition gets formulas and symbols wrong.\n\
                 2. Write, ready to copy:\n\
                 - one title line for the lesson;\n\
                 - \"Lesson content\": 3 to 6 short bullets (concepts, definitions, experiments, exercises), as close as possible to what is written, in the vocabulary of the lower secondary curriculum;\n\
                 - \"Homework\" and \"To remember\" only if they are written in the notes; otherwise invent none and say there are none.\n\
                 Short sentences, in the present tense, about 80 words in all, no student's name.\n\
                 3. End with what remains uncertain (unreadable words, doubtful values) in one line. Change no note and save nothing."
            )
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Building the messages

fn result(description: &str, text: String) -> GetPromptResult {
    GetPromptResult::new(vec![PromptMessage::new_text(Role::User, text)])
        .with_description(description)
}

fn clean_up(host: &Host, language: Language, read_only: bool, args: &CleanUpArgs) -> String {
    let asked = args.note.as_deref().is_some_and(|n| !n.trim().is_empty());
    let note = args
        .note
        .as_deref()
        .filter(|_| asked)
        .and_then(|reference| {
            let library = host.library().ok()?;
            let entry = find_note(&library, reference).ok()?;
            Some(NoteRef {
                path: crate::notes::relative_path(&library, &entry.path),
                id: entry.id,
                title: entry.title,
            })
        });
    clean_up_text(
        language,
        note.as_ref(),
        asked,
        args.format.unwrap_or_default(),
        read_only,
    )
}

/// The untitled notes of the library (or of `binder`), newest first.
fn untitled(host: &Host, binder: &str) -> Vec<NoteEntry> {
    let Ok(library) = host.library() else {
        return Vec::new();
    };
    let mut notes: Vec<NoteEntry> = library
        .list()
        .notes
        .into_iter()
        .filter(|note| note.title.trim().is_empty())
        .filter(|note| binder.is_empty() || in_binder(&binder_text(&note.binder), binder))
        .collect();
    notes.sort_by(|a, b| b.created.cmp(&a.created).then(a.path.cmp(&b.path)));
    notes
}

fn title_and_tag(
    host: &Host,
    language: Language,
    read_only: bool,
    args: &TitleAndTagArgs,
) -> String {
    let binder = clean_binder(args.binder.as_deref().unwrap_or_default());
    let notes = untitled(host, &binder);
    let lines: Vec<String> = notes
        .iter()
        .take(LISTED)
        .map(|entry| line_of(host, entry))
        .collect();
    title_and_tag_text(language, &binder, &lines, notes.len(), read_only)
}

/// The notes of a lesson: those created on `date`, in `binder` if given; with a binder alone,
/// those of its most recent day. Returns the day (`YYYY-MM-DD`) and the notes, oldest first.
fn lesson(
    host: &Host,
    date: Option<&str>,
    binder: &str,
) -> Result<(String, Vec<NoteEntry>), ErrorData> {
    let invalid = |e: crate::error::ToolError| ErrorData::invalid_params(e.message, None);
    let Ok(library) = host.library() else {
        return Ok((date.unwrap_or_default().to_owned(), Vec::new()));
    };
    let mut notes: Vec<NoteEntry> = library
        .list()
        .notes
        .into_iter()
        .filter(|note| binder.is_empty() || in_binder(&binder_text(&note.binder), binder))
        .collect();
    let day_of = |created: i64| iso(created).chars().take(10).collect::<String>();
    let day = match (
        date.map(str::trim).filter(|d| !d.is_empty()),
        binder.is_empty(),
    ) {
        (Some(date), _) => {
            let start = parse_date(date, false).map_err(invalid)?;
            let end = parse_date(date, true).map_err(invalid)?;
            notes.retain(|note| note.created >= start && note.created < end);
            day_of(start)
        }
        (None, false) => {
            let latest = notes.iter().map(|n| n.created).max();
            let day = latest.map(day_of).unwrap_or_default();
            notes.retain(|note| day_of(note.created) == day);
            day
        }
        (None, true) => {
            let today = day_of(chrono::Utc::now().timestamp_millis());
            let start = parse_date(&today, false).map_err(invalid)?;
            let end = parse_date(&today, true).map_err(invalid)?;
            notes.retain(|note| note.created >= start && note.created < end);
            today
        }
    };
    notes.sort_by(|a, b| a.created.cmp(&b.created).then(a.path.cmp(&b.path)));
    Ok((day, notes))
}

fn lesson_summary(
    host: &Host,
    language: Language,
    args: &LessonSummaryArgs,
) -> Result<String, ErrorData> {
    let binder = clean_binder(args.binder.as_deref().unwrap_or_default());
    let (day, notes) = lesson(host, args.date.as_deref(), &binder)?;
    let lines: Vec<String> = notes
        .iter()
        .take(LISTED)
        .map(|entry| line_of(host, entry))
        .collect();
    Ok(lesson_summary_text(
        language,
        &day,
        &binder,
        &lines,
        notes.len(),
    ))
}

impl ArdoiseServer {
    /// Runs `work` on the blocking pool (it reads the library).
    async fn prompt_on_host<T: Send + 'static>(
        &self,
        work: impl FnOnce(&Host) -> T + Send + 'static,
    ) -> Result<T, ErrorData> {
        let host = std::sync::Arc::clone(&self.host);
        tokio::task::spawn_blocking(move || work(&host))
            .await
            .map_err(|error| ErrorData::internal_error(error.to_string(), None))
    }
}

const FR_NAMES: [&str; 3] = ["mettre_au_propre", "titrer_et_etiqueter", "resume_seance"];
const EN_NAMES: [&str; 3] = ["clean_up", "title_and_tag", "lesson_summary"];

#[prompt_router(router = "prompts", vis = "pub(crate)")]
impl ArdoiseServer {
    /// Met au propre une note manuscrite (Markdown, Word ou LaTeX) : l'assistant regarde la page, lit le texte reconnu et rédige un document fidèle.
    #[prompt(
        name = "mettre_au_propre",
        description = "Met au propre une note manuscrite en Markdown, Word ou LaTeX : l'assistant regarde la page, lit le texte reconnu et rédige un document fidèle."
    )]
    async fn mettre_au_propre(
        &self,
        Parameters(args): Parameters<CleanUpArgs>,
    ) -> Result<GetPromptResult, ErrorData> {
        let read_only = self.options().read_only;
        let text = self
            .prompt_on_host(move |host| clean_up(host, Language::Fr, read_only, &args))
            .await?;
        Ok(result("Mettre au propre une note", text))
    }

    #[prompt(
        name = "clean_up",
        description = "Turn a handwritten note into a clean Markdown, Word or LaTeX document: the assistant looks at the page, reads the recognised text and writes a faithful document."
    )]
    async fn clean_up(
        &self,
        Parameters(args): Parameters<CleanUpArgs>,
    ) -> Result<GetPromptResult, ErrorData> {
        let read_only = self.options().read_only;
        let text = self
            .prompt_on_host(move |host| clean_up(host, Language::En, read_only, &args))
            .await?;
        Ok(result("Clean up a note", text))
    }

    #[prompt(
        name = "titrer_et_etiqueter",
        description = "Propose un titre et des étiquettes pour les notes sans titre, d'après leur texte et leur image, puis les applique après accord (en lecture seule : la liste à appliquer à la main)."
    )]
    async fn titrer_et_etiqueter(
        &self,
        Parameters(args): Parameters<TitleAndTagArgs>,
    ) -> Result<GetPromptResult, ErrorData> {
        let read_only = self.options().read_only;
        let text = self
            .prompt_on_host(move |host| title_and_tag(host, Language::Fr, read_only, &args))
            .await?;
        Ok(result("Titrer et étiqueter les notes sans titre", text))
    }

    #[prompt(
        name = "title_and_tag",
        description = "Propose a title and tags for the untitled notes from their text and image, then apply them once the user agrees (read-only: the list to apply by hand)."
    )]
    async fn title_and_tag(
        &self,
        Parameters(args): Parameters<TitleAndTagArgs>,
    ) -> Result<GetPromptResult, ErrorData> {
        let read_only = self.options().read_only;
        let text = self
            .prompt_on_host(move |host| title_and_tag(host, Language::En, read_only, &args))
            .await?;
        Ok(result("Title and tag untitled notes", text))
    }

    #[prompt(
        name = "resume_seance",
        description = "Résume en quelques lignes les notes d'une séance, pour le cahier de textes."
    )]
    async fn resume_seance(
        &self,
        Parameters(args): Parameters<LessonSummaryArgs>,
    ) -> Result<GetPromptResult, ErrorData> {
        let text = self
            .prompt_on_host(move |host| lesson_summary(host, Language::Fr, &args))
            .await??;
        Ok(result("Résumé de séance", text))
    }

    #[prompt(
        name = "lesson_summary",
        description = "Summarise the notes of a lesson in a few lines, for the class log (cahier de textes)."
    )]
    async fn lesson_summary(
        &self,
        Parameters(args): Parameters<LessonSummaryArgs>,
    ) -> Result<GetPromptResult, ErrorData> {
        let text = self
            .prompt_on_host(move |host| lesson_summary(host, Language::En, &args))
            .await??;
        Ok(result("Lesson summary", text))
    }
}

impl ArdoiseServer {
    /// The prompts of the language of the app: the French names for a French app, else the
    /// English ones.
    pub(crate) fn prompt_router(language: Language) -> PromptRouter<ArdoiseServer> {
        let mut router = Self::prompts();
        let other = match language {
            Language::Fr => EN_NAMES,
            Language::En => FR_NAMES,
        };
        for name in other {
            router.remove_route(name);
        }
        router
    }
}
