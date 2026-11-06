//! The prompts (`mettre_au_propre` / `clean_up`, `titrer_et_etiqueter` / `title_and_tag`,
//! `ameliorer_note` / `improve_note` (the one that writes: absent with `--read-only`),
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

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ImproveArgs {
    /// Note to improve: its id or its path in the library. Omitted: the note of the last live session.
    pub note: Option<String>,
    /// What the user wants improved, in their words (tidy the writing, redraw the diagram, correct the spelling...). Omitted: the assistant asks.
    pub goals: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// What a prompt knows of the library

/// The note a prompt is about, as the library shows it.
struct NoteRef {
    id: String,
    path: String,
    title: String,
    /// The note is a retouched copy (`edit_revision` applies to it).
    revision: bool,
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

/// The message of `ameliorer_note` / `improve_note`: retouching a note on a copy, in the six
/// steps of `docs/design/revision.md`, with what the trial on four real notes taught. Only a
/// server that can write has this prompt, so it names the write tools freely.
fn improve_text(
    language: Language,
    note: Option<&NoteRef>,
    asked: bool,
    goals: Option<&str>,
) -> String {
    match language {
        Language::Fr => {
            let which = match (note, asked) {
                (Some(note), _) => format!(
                    "La note à améliorer : « {} » (id `{}`, chemin `{}`).{}",
                    if note.title.trim().is_empty() { "sans titre" } else { note.title.trim() },
                    note.id,
                    note.path,
                    if note.revision {
                        " C'est déjà une copie retouchée : modifie-la en place avec `edit_revision` (ses traits sont numérotés comme `get_note` les donne maintenant, avec ce que l'utilisateur a pu changer à la main) au lieu de `create_revision`, qui fait une nouvelle copie."
                    } else {
                        ""
                    }
                ),
                (None, true) => "La note demandée n'existe pas dans la bibliothèque : retrouve-la avec `search_notes` ou `list_notes`, ou demande à l'utilisateur laquelle il veut, avant de continuer.".to_owned(),
                (None, false) => "Aucune note n'est désignée : appelle l'outil `last_live_note` pour trouver la note de la dernière séance en direct. S'il n'y en a pas (ou si Ardoise n'est pas ouverte), demande à l'utilisateur laquelle améliorer (`search_notes` et `list_notes` aident à la retrouver) avant de continuer.".to_owned(),
            };
            let goals = match goals {
                Some(goals) => format!("Ce que l'utilisateur demande : {goals}"),
                None => "L'utilisateur n'a pas dit ce qu'il veut améliorer : demande-le-lui (ranger l'écriture, redresser ou redessiner un schéma, corriger des fautes, mettre en couleur...) avant de préparer quoi que ce soit.".to_owned(),
            };
            format!(
                "Tu aides un enseignant de physique-chimie (collège) à améliorer une note manuscrite d'Ardoise, sur une copie. {which}\n{goals}\n\n\
                 Le principe. Ardoise ne contient aucune IA : tu regardes la page et tu décides, Ardoise exécute une liste d'ordres sur les traits (tu n'envoies jamais les points d'une page). L'original n'est jamais modifié : `preview_revision` essaie les ordres en mémoire (rien n'est écrit), `create_revision` écrit le résultat dans une copie à côté de l'original, que l'éditeur présente comme une copie retouchée, avec ce qui a changé. Les ordres ne peuvent désigner que des traits de cette note.\n\n\
                 Procède dans cet ordre.\n\
                 1. Regarde. `get_note` (avec `strokes: \"summary\"`) : la page, les calques, les traits, et `background.ruling` si le papier est réglé (première ligne et écart entre deux lignes, en millimètres, pour poser l'écriture dessus). `render_note` pour la page entière (dpi 100 à 150), puis zoome avec `region` et `labels: true` (dpi 220 à 300 : à la taille de la page, les numéros se chevauchent et cachent les petits traits) sur ce qui va changer : les numéros écrits en rouge à côté des traits sont ceux des ordres. `ink_lines` donne les lignes écrites trouvées dans l'encre même (la reconnaissance en fusionne), `get_note_text` le texte reconnu, `get_strokes` les points exacts d'un trait (les `from` et `to` de `glyph`, pour prendre une lettre dans un mot écrit d'un seul trait).\n\
                 2. Dis ce que tu prévois, et donne à part la liste des fautes d'orthographe que tu vois. Ne corrige que celles que l'utilisateur valide, et attends sa réponse avant de fabriquer une lettre. Ne change jamais le sens ; rien n'est corrigé ni déplacé en silence. Un document dont les mots comptent (compte rendu, fiche d'incident, dossier) reste sobre : aucune couleur, sauf si l'utilisateur en demande.\n\
                 3. Pour une lettre à fabriquer ou à remplacer : d'abord, recopie-la depuis la même écriture dans cette note (`copy` pour une lettre écrite d'un trait à part, `glyph` avec `from` et `to` pour une lettre prise dans un mot écrit d'un seul trait : elle ne prend que le temps de ce morceau) ; compare les lettres avant de choisir, car une note peut contenir deux écritures, et préfère une lettre proche, de la même couleur. Sinon, fabrique-la avec des morceaux de cette écriture (`glyph` : le rond d'un « q » et la queue d'un « j » font un « g », un « w » agrandi un « W », un « u » retourné un « n »). Sinon, marque le mot en couleur (`add` avec le surligneur) et dis-le. Jamais de lettre prise dans une autre note, même en recopiant ses points dans une forme : l'utilisateur confie parfois son calepin à un élève.\n\
                 4. Donne `after` à chaque lettre recopiée ou fabriquée, pour qu'elle prenne sa place dans le temps de l'écriture (le rejeu), et `retime` à un mot déplacé dans sa phrase. Si la note a une narration, rien ne se décale : la lettre est tracée plus vite, dans le temps disponible.\n\
                 5. `preview_revision` avec la liste d'ordres. Regarde l'image (zoome avec `region` et `labels`), lis le rapport (ce que chaque ordre a sélectionné : une sélection de la mauvaise taille est l'erreur la plus courante ; pour une ligne rangée, la pente et d'où elle vient ; les avertissements), corrige, recommence. Envoie toujours la liste entière, construite à partir de la note telle qu'elle est, pas les changements depuis le dernier essai. L'aperçu donne aussi `version`, le nom de la note telle qu'il l'a vue.\n\
                 6. Quand l'image est bonne, appelle `create_revision` (ou `edit_revision`) avec la liste, `expected_version` (la `version` du dernier aperçu), un `summary` (le compte rendu, dans la langue de l'utilisateur) et, si besoin, un `title`. Si la réponse est `noteConflict`, la note a changé depuis (l'utilisateur, un autre ordinateur) : ses traits peuvent avoir d'autres numéros, alors regarde-la de nouveau et refais la liste. Puis dis à l'utilisateur tout ce qui a changé, chaque lettre fabriquée (une lettre fabriquée se lit bien mais reste un peu serrée à côté de la vraie écriture) et ce qui reste incertain. Rappelle que l'original est intact, et que l'éditeur propose « Ouvrir l'original », « Ce qui a changé » et « Comparer ».\n\n\
                 Ce que l'essai sur de vraies notes a appris.\n\
                 - Une sélection de la mauvaise taille est l'erreur habituelle : un rectangle prend un trait voisin (le « on » d'un « rayon » rouge posé juste au-dessus). Ajoute un filtre `color`, `layer`, `min_size` ou `max_size`, et compare les numéros du rapport (`selected_strokes`) à l'image.\n\
                 - Un trait nouveau ne garde pas la couleur d'avant : donne `color`.\n\
                 - `replace` trace la forme au moment du premier trait remplacé, dans le temps que le stylo a mis sur tous, sans dépasser le trait suivant de la note. Si les traits ont été écrits à des moments éloignés, le rapport le dit : c'est souvent le signe d'une sélection trop large, sinon `retime` la forme si le premier moment ne convient pas. `time` dans le rapport donne les heures.\n\
                 - `ink_lines` est fiable sur du texte ; sur une page de formules, il peut réunir deux formules ou donner une pente aberrante. Vérifie sur l'image et, pour une formule, choisis toi-même ses traits.\n\
                 - Sur une ligne avec des exposants ou des indices, la pente ajustée est fausse : donne la `slope` que tu vois, et `mode: \"rigid\"` pour une formule dont un exposant doit rester au-dessus. Une ligne courte prend une pente nulle.\n\
                 - Une page entière de schéma ne se range pas ligne par ligne : sélectionne le texte (`ink_lines` met les dessins à part).\n\
                 - Sur du papier réglé, `scale: 0.9` peut être nécessaire : l'écriture était plus espacée que les lignes et les descendantes se touchaient.\n\
                 - `tidy_line` change peu une écriture déjà régulière, beaucoup une page désordonnée.\n\
                 - Plusieurs calques : les traits nouveaux vont sur le calque de ceux qu'ils remplacent. L'image ne montre pas les calques masqués : un rectangle ou une sélection sans lieu ne prend pas leurs traits (nomme `layer` ou `ids` pour les toucher, le rapport le signale).\n\n\
                 Les notes peuvent contenir des noms d'élèves : ne les répète que dans ta réponse à l'utilisateur."
            )
        }
        Language::En => {
            let which = match (note, asked) {
                (Some(note), _) => format!(
                    "The note to improve: \"{}\" (id `{}`, path `{}`).{}",
                    if note.title.trim().is_empty() { "untitled" } else { note.title.trim() },
                    note.id,
                    note.path,
                    if note.revision {
                        " It already is a retouched copy: change it in place with `edit_revision` (its strokes are numbered as `get_note` gives them now, with what the user may have changed by hand) instead of `create_revision`, which makes a new copy."
                    } else {
                        ""
                    }
                ),
                (None, true) => "The note asked for is not in the library: find it with `search_notes` or `list_notes`, or ask the user which one they mean, before going on.".to_owned(),
                (None, false) => "No note is named: call the tool `last_live_note` to find the note of the last live session. If there is none (or Ardoise is not open), ask the user which note to improve (`search_notes` and `list_notes` help to find it) before going on.".to_owned(),
            };
            let goals = match goals {
                Some(goals) => format!("What the user asks for: {goals}"),
                None => "The user has not said what to improve: ask them (tidy the writing, straighten or redraw a diagram, correct mistakes, add colour...) before preparing anything.".to_owned(),
            };
            format!(
                "You are helping a physics and chemistry teacher (lower secondary school) improve a handwritten Ardoise note, on a copy. {which}\n{goals}\n\n\
                 The principle. Ardoise contains no AI: you look at the page and decide, Ardoise carries out a list of orders on the strokes (you never send the points of a page). The original is never changed: `preview_revision` tries the orders in memory (nothing is written), `create_revision` writes the result to a copy next to the original, which the editor presents as a retouched copy, with what changed. The orders can only name strokes of this note.\n\n\
                 Proceed in this order.\n\
                 1. Look. `get_note` (with `strokes: \"summary\"`): the page, the layers, the strokes, and `background.ruling` if the paper is ruled (the first rule and the distance between two rules, in millimetres, to put the writing on them). `render_note` for the whole page (100 to 150 dpi), then zoom with `region` and `labels: true` (220 to 300 dpi: at page size the numbers overlap and hide small strokes) on what will change: the red numbers written next to the strokes are the ones of the orders. `ink_lines` gives the written lines found in the ink itself (the recogniser merges lines), `get_note_text` the recognised text, `get_strokes` the exact points of a stroke (the `from` and `to` of `glyph`, to take a letter out of a word written in one stroke).\n\
                 2. Say what you plan, and give the spelling mistakes you see as a separate list. Correct only those the user validates, and wait for their answer before making a letter. Never change the meaning; nothing is corrected or moved silently. A document whose words matter (a report, an incident record, a file) stays sober: no colour, unless the user asks for it.\n\
                 3. For a letter to make or to replace: first copy it from the same handwriting in this note (`copy` for a letter that is a stroke of its own, `glyph` with `from` and `to` for a letter taken out of a word written in one stroke: it only takes the time of that piece); compare the letters before choosing, since a note can hold two hands, and prefer a close one, of the same colour. Otherwise make it from pieces of that handwriting (`glyph`: the bowl of a \"q\" and the tail of a \"j\" make a \"g\", a bigger \"w\" a \"W\", a turned \"u\" an \"n\"). Otherwise mark the word in colour (`add` with the highlighter) and say so. Never a letter taken from another note, not even by retyping its points in a shape: the user sometimes lends the notebook to a pupil.\n\
                 4. Give `after` to every letter you copy or make, so that it takes its place in the time of the writing (the replay), and `retime` to a word moved into its sentence. If the note has a narration nothing moves in time: the letter is drawn faster, in the time available.\n\
                 5. `preview_revision` with the list of orders. Look at the picture (zoom with `region` and `labels`), read the report (what each order selected: a selection of the wrong size is the usual mistake; for a tidied line, the slope and where it comes from; the warnings), correct, and again. Always send the whole list, built from the note as it is, not the changes since the last try. The preview also gives `version`, the name of the note as it saw it.\n\
                 6. When the picture is right, call `create_revision` (or `edit_revision`) with the list, `expected_version` (the `version` of the last preview), a `summary` (the account, in the user's language) and a `title` if needed. If the answer is `noteConflict`, the note changed since (the user, another computer): its strokes may have other numbers, so look at it again and redo the list. Then tell the user everything that changed, each letter you made (a made letter reads well but stays a little tight next to real writing) and what is uncertain. Remind them that the original is intact, and that the editor offers \"Open the original\", \"What changed\" and \"Compare\".\n\n\
                 What the trial on real notes taught.\n\
                 - A selection of the wrong size is the usual mistake: a rectangle takes a neighbouring stroke (the \"on\" of a red \"rayon\" lying just above). Add a filter `color`, `layer`, `min_size` or `max_size`, and compare the numbers of the report (`selected_strokes`) with the picture.\n\
                 - A new stroke does not keep the colour of before: give `color`.\n\
                 - `replace` draws the shape at the moment of the first stroke replaced, in the time the pen spent on them all, without going past the next stroke of the note. If the strokes were written far apart, the report says so: often a sign of a selection that is too wide, otherwise `retime` the shape if the first moment does not suit. `time` in the report gives the times.\n\
                 - `ink_lines` is reliable on text; on a page of formulas it can join two formulas or give a wild slope. Check against the picture and, for a formula, choose its strokes yourself.\n\
                 - On a line with exponents or subscripts the fitted slope is wrong: give the `slope` you see, and `mode: \"rigid\"` for a formula whose exponent must stay above. A short line takes slope 0.\n\
                 - A whole page of diagram is not tidied line by line: select the text (`ink_lines` sets the drawings apart).\n\
                 - On ruled paper `scale: 0.9` may be needed: the writer's own line spacing was wider than the rules and descenders touched.\n\
                 - `tidy_line` shows little on writing that is already regular, and much on a messy page.\n\
                 - With several layers, new strokes go on the layer of the ones they replace. The picture does not show hidden layers: a rectangle or a selection with no place does not take their strokes (name `layer` or `ids` to touch them, the report says so).\n\n\
                 Notes may contain students' names: repeat them only in your answer to the user."
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

/// The note named `reference`, if the library has it; `with_revision` reads the file to tell
/// whether it is a retouched copy.
fn note_ref(host: &Host, reference: &str, with_revision: bool) -> Option<NoteRef> {
    let library = host.library().ok()?;
    let entry = find_note(&library, reference).ok()?;
    let revision = with_revision
        && library
            .read(&entry.path)
            .is_ok_and(|n| n.file.revision.is_some());
    Some(NoteRef {
        path: crate::notes::relative_path(&library, &entry.path),
        id: entry.id,
        title: entry.title,
        revision,
    })
}

fn clean_up(host: &Host, language: Language, read_only: bool, args: &CleanUpArgs) -> String {
    let asked = args.note.as_deref().is_some_and(|n| !n.trim().is_empty());
    let note = args
        .note
        .as_deref()
        .filter(|_| asked)
        .and_then(|reference| note_ref(host, reference, false));
    clean_up_text(
        language,
        note.as_ref(),
        asked,
        args.format.unwrap_or_default(),
        read_only,
    )
}

fn improve(host: &Host, language: Language, args: &ImproveArgs) -> String {
    let asked = args.note.as_deref().is_some_and(|n| !n.trim().is_empty());
    let note = args
        .note
        .as_deref()
        .filter(|_| asked)
        .and_then(|reference| note_ref(host, reference, true));
    let goals = args
        .goals
        .as_deref()
        .map(str::trim)
        .filter(|g| !g.is_empty());
    improve_text(language, note.as_ref(), asked, goals)
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
/// The prompt that writes (it names the tools of the write group): absent with `--read-only`.
const FR_WRITING: &str = "ameliorer_note";
const EN_WRITING: &str = "improve_note";
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

    /// Améliore une note manuscrite sur une copie : l'original n'est jamais modifié.
    #[prompt(
        name = "ameliorer_note",
        description = "Améliore une note manuscrite sur une copie (écriture rangée, schéma redressé, fautes corrigées avec la propre écriture de la note) : l'original n'est jamais modifié. Absente en lecture seule."
    )]
    async fn ameliorer_note(
        &self,
        Parameters(args): Parameters<ImproveArgs>,
    ) -> Result<GetPromptResult, ErrorData> {
        let text = self
            .prompt_on_host(move |host| improve(host, Language::Fr, &args))
            .await?;
        Ok(result("Améliorer une note sur une copie", text))
    }

    #[prompt(
        name = "improve_note",
        description = "Improve a handwritten note on a copy (tidier writing, straightened diagrams, spelling corrected with the note's own handwriting): the original is never changed. Absent when read-only."
    )]
    async fn improve_note(
        &self,
        Parameters(args): Parameters<ImproveArgs>,
    ) -> Result<GetPromptResult, ErrorData> {
        let text = self
            .prompt_on_host(move |host| improve(host, Language::En, &args))
            .await?;
        Ok(result("Improve a note on a copy", text))
    }
}

impl ArdoiseServer {
    /// The prompts of the language of the app: the French names for a French app, else the
    /// English ones. A read-only server leaves out the prompt that writes.
    pub(crate) fn prompt_router(
        language: Language,
        read_only: bool,
    ) -> PromptRouter<ArdoiseServer> {
        let mut router = Self::prompts();
        let (other, other_writing, writing) = match language {
            Language::Fr => (EN_NAMES, EN_WRITING, FR_WRITING),
            Language::En => (FR_NAMES, FR_WRITING, EN_WRITING),
        };
        for name in other {
            router.remove_route(name);
        }
        router.remove_route(other_writing);
        if read_only {
            router.remove_route(writing);
        }
        router
    }
}
