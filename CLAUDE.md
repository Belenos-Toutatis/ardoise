# Ardoise — consignes pour Claude

Application portable pour Windows (Rust + Tauri 2) qui remplace Wacom Inkspace pour la tablette Bamboo Slate.

- **Quoi construire** : [SPEC.md](SPEC.md)
- **Dans quel ordre, et quand une étape est finie** : [ROADMAP.md](ROADMAP.md)
- **Recherches qui justifient les choix** : [docs/research/](docs/research/) (protocole Bluetooth, plateforme Windows, vidéo/audio, format Inkspace, fonctions d'Inkspace)
- **Conception technique** (pour les développeurs, en anglais) : [docs/design/](docs/design/) — `device-stack.md` : protocole, transport, simulateur, journal, outil de diagnostic

## Commandes

- `cargo xtask check` : vérification complète (dépendances npm, compilation de l'interface, formatage, clippy sans avertissement, tests Rust, svelte-check + vitest, compilation release). Se termine par `CHECK OK`. `--quick` saute la compilation release.
- `cargo xtask selftest` : taille de `target\release\ardoise.exe` (< 30 Mo) puis `ardoise.exe --self-test`. Se termine par `SELFTEST OK`.
- `cargo fmt --all` avant de lancer la vérification.
- Une compilation release de l'app exige `--features custom-protocol` (sinon Tauri chargerait l'interface depuis le serveur de développement au lieu de l'embarquer ; une garde `compile_error!` l'empêche). `cargo xtask check` le fait.
- Interface seule (dans `app/`) : `npm run build`, `npm run check`, `npm test`. Application en développement : `npm run tauri dev` (dans `app/`).
- Les crates se testent une par une : `cargo test -p ardoise-core`.

## Structure

- `crates/ardoise-*` : bibliothèques Rust (rôle de chacune : SPEC §6.1 et en tête de chaque `lib.rs`).
- `app/src-tauri/` : l'exécutable `ardoise` (fenêtres, commandes appelées par l'interface, `--self-test`).
- `app/ui/src/` : interface Svelte 5 + TypeScript ; textes dans `locales/fr.json` et `locales/en.json` ; appels à Rust regroupés dans `lib/api.ts`.
- `xtask/` : automatisation (`check`, `selftest`).
- Erreurs envoyées à l'interface : `{ code, message }` ; le code choisit une phrase traduite `errorCodes.*`.

## Conventions

- Code, identifiants, commentaires et messages de commit en **anglais**. Documents de planification et de test (`SPEC.md`, `ROADMAP.md`, `docs/hardware-tests.md`) en **français**. Guide utilisateur en français et en anglais.
- Tout texte visible dans l'interface passe par les traductions **FR et EN** ; une clé manquante dans une langue fait échouer les tests.
- Bibliothèques : erreurs typées (`thiserror`) ; `anyhow` seulement dans l'exécutable et `xtask`. Pas de `unwrap()`/`expect()` hors des tests sur des données venant de la tablette, de fichiers ou de l'utilisateur.
- Toute écriture dans la bibliothèque de l'utilisateur est **atomique** (SPEC §3.4).
- Le protocole est réimplémenté à partir de `docs/research/protocol.md` et de nos captures, sans traduire le code de Tuhi ligne à ligne (SPEC §6.2).

## Règles

- Ne pas modifier `SPEC.md` sans l'accord de l'utilisateur. Une question ou une incohérence va dans `docs/questions.md`.
- **Vie privée** : aucune note personnelle, capture non anonymisée ou donnée d'Inkspace dans git. Les tests qui en ont besoin lisent `testdata/private/` (ignoré par git). Ne jamais lire la clé `profile` de `LocalState\db` d'Inkspace (jeton d'accès et e-mail).
- Ne jamais modifier le dossier d'Inkspace : travailler sur des copies.
- La tablette n'est pas sur le poste de développement : tout ce qui la concerne doit être testable avec `ardoise-sim` ou des captures rejouées. Les tests réels passent par `docs/hardware-tests.md` ; les journaux de diagnostic reviennent par Nextcloud dans `<bibliothèque>/_ardoise/diagnostics/`.
- Demander avant tout téléchargement d'outil (ex. `nasm`) ou installation globale.
- Git : commits **locaux** autorisés, un par tâche cochée ou par groupe cohérent de changements, message en anglais. **Jamais de push** ni de publication sans accord explicite de l'utilisateur.

## Sous-agents

Les utiliser largement, avec un modèle et un effort adaptés à la difficulté (profils dans `.claude/agents/`) :

| Profil | Modèle / effort | Pour |
|---|---|---|
| `explorateur` | Haiku / low | recherches en lecture seule, doc, journaux, tâches mécaniques |
| `developpeur` | Sonnet / high | implémentation standard (interface, exports, opérations, tests) |
| `specialiste` | Opus / xhigh | Bluetooth, simulateur, rendu des encres, vidéo, format de fichier, import Inkspace |
| `relecteur` | Opus / high | relecture indépendante à la fin de chaque jalon |
| `expert-retro` | Fable / xhigh | uniquement les cas les plus durs (rétro-ingénierie, bug tenace) |

- Les sous-agents qui écrivent en parallèle travaillent en isolation (`worktree`) et ne touchent jamais les mêmes fichiers.
- Toujours vérifier leur travail (`cargo xtask check`) avant de cocher une case de `ROADMAP.md`.
