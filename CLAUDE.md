# Ardoise — consignes pour Claude

Application portable pour Windows (Rust + Tauri 2) qui remplace Wacom Inkspace pour la tablette Bamboo Slate.

- **Quoi construire** : [SPEC.md](SPEC.md)
- **Dans quel ordre, et quand une étape est finie** : [ROADMAP.md](ROADMAP.md)
- **Recherches qui justifient les choix** : [docs/research/](docs/research/) (protocole Bluetooth, plateforme Windows, vidéo/audio, format Inkspace, fonctions d'Inkspace)
- **Conception technique** (pour les développeurs, en anglais) : [docs/design/](docs/design/) — `device-stack.md` : protocole, transport, simulateur, journal, outil de diagnostic

## Commandes

- `cargo xtask check` : vérification complète (dépendances npm, compilation de l'interface, formatage, clippy sans avertissement, tests Rust, svelte-check + vitest, compilation release). Se termine par `CHECK OK`. `--quick` saute la compilation release.
- `cargo xtask selftest` : taille de `target\release\ardoise.exe` (< 30 Mo) puis `ardoise.exe --self-test`. Se termine par `SELFTEST OK`.
- `cargo xtask e2e` : parcours de bout en bout sur l'exe de production (bibliothèque de démonstration, fenêtre pilotée par le port de débogage de WebView2), en français et en anglais. Se termine par `E2E OK`. Lancer `cargo xtask check` avant.
- `cargo xtask licenses [--write]` : vérifie la licence de chaque dépendance Rust et npm (liste blanche compatible GPL v3, aucune AGPL ; se termine par `LICENSES OK`) et que `THIRD-PARTY-NOTICES` est à jour ; `--write` le régénère (à committer). `check` le lance.
- `cargo xtask privacy` : parcourt tout l'historique git (chemins, adresses e-mail, jetons, adresses Bluetooth, identifiants lus dans `testdata/private/`) et échoue si une donnée personnelle y figure ; liste aussi les identités des commits (information) ; se termine par `PRIVACY OK`. `check` le lance.
- `ardoise.exe --create-demo-library DOSSIER` : bibliothèque de démonstration (notes synthétiques).
- Prérequis de compilation : la cible Rust `wasm32-unknown-unknown` (le calcul des contours des traits est compilé en WebAssembly pour l'écran ; `npm run wasm`, lancé automatiquement par les scripts npm).
- Prérequis de compilation : `nasm` (assembleur de l'encodeur AV1 `rav1e`, pour le WebM). Sous Windows, le zip portable décompressé dans `%LOCALAPPDATA%\ArdoiseDev\nasm-<version>\` : `cargo xtask` le trouve tout seul ; pour une commande `cargo` directe, définir `NASM` (PowerShell : `$env:NASM="$env:LOCALAPPDATA\ArdoiseDev\nasm-3.02\nasm.exe"`). Sous Linux, le paquet apt `nasm`. Jamais nécessaire chez l'utilisateur.
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
- Git : un commit par tâche cochée ou par groupe cohérent de changements, message en anglais. Le dépôt GitHub **privé** `origin` (Belenos-Toutatis/ardoise) a été créé avec l'accord de l'utilisateur : y pousser des branches de travail est autorisé, et `main` seulement après la vérification Windows (`cargo xtask check` et `cargo xtask e2e` réussis sur le PC). **Jamais** rendre le dépôt public, créer une release ou publier quoi que ce soit sans accord explicite de l'utilisateur.

## Sessions cloud (Linux)

Une session Claude Code dans le cloud tourne sous Ubuntu, pas sous Windows.

- `scripts/cloud-setup.sh` (lancé au démarrage par `.claude/settings.json`) installe Rust 1.98.1 avec la cible WebAssembly, Node 24 et les bibliothèques WebKitGTK dont Tauri a besoin sous Linux. Sa dernière ligne dit `ready` ou ce qui a échoué.
- Possible : `cargo xtask check` (l'app se compile pour Linux ; le code propre à Windows a ses variantes `cfg(not(windows))`), les tests Rust et ceux de l'interface, `cargo xtask golden`.
- Impossible : `cargo xtask selftest`, `cargo xtask e2e`, et tout ce qui demande Windows (WebView2, Bluetooth Windows, Media Foundation, reconnaissance d'écriture, presse-papiers et fenêtres réelles). Les écrire quand même, avec des tests Linux de la logique, et lister dans le compte rendu final ce qui reste à vérifier sous Windows.
- `testdata/private/` n'existe pas : les tests qui en ont besoin se sautent d'eux-mêmes.
- Une case de `ROADMAP.md` n'est cochée qu'après la vérification Windows, faite par une session sur le PC qui fusionne la branche.
- Travailler sur la branche de la session, la pousser sur `origin`, ne jamais pousser sur `main`. Les sous-agents en `worktree` restent possibles ; fusionner leurs branches dans celle de la session.

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
