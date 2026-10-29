# Ardoise

**Ardoise** est une application portable pour Windows qui remplace Wacom Inkspace pour la tablette **Bamboo Slate**.
Elle récupère les pages écrites sur la tablette, permet d'écrire en direct à l'écran et au vidéoprojecteur, range les
notes dans un dossier de votre choix (par exemple synchronisé par Nextcloud) et permet de les modifier, de les
rejouer et de les exporter.

*English summary at the end of this file.*

> « Ardoise » est un nom de travail ; il peut changer avant la publication.

## Fonctions principales

- **Tablette** : enregistrement de la Bamboo Slate en Bluetooth LE, connexion automatique, synchronisation des pages
  (chaque page devient une note), informations et réglages de la tablette, quatre positions de la tablette.
- **Mode direct** : le tracé de la tablette s'affiche en temps réel ; présentation en plein écran sur le
  vidéoprojecteur, tableau noir, pointeur laser, zoom, annotations par-dessus la présentation.
- **Bibliothèque** : un dossier ordinaire, une note = un fichier `.ardoise` ; classeurs (créer, renommer, supprimer),
  étiquettes en couleur, favoris, corbeille, tourner plusieurs notes d'un coup, « Annuler » après une mise à la
  corbeille, recherche par titre, étiquette et écriture manuscrite ; fonctionne avec Nextcloud sur plusieurs
  ordinateurs, avec des écritures atomiques et la détection des conflits.
- **Édition** : sept outils d'encre sensibles à la pression, gomme, lasso, calques, papiers et modèles de page,
  redressement des formes, format et orientation de la feuille, image ou PDF de fond à annoter, découpe et assemblage de
  notes, retour à l'original, copier-coller entre notes, stylet et écran tactile (Surface). Les modifications non
  enregistrées sont gardées en copie de secours sur le PC et peuvent être récupérées après un plantage.
- **Relecture et vidéo** : relecture animée du tracé, narration au micro, export en MP4, GIF, WebP et WebM.
- **Exports** : PNG, JPG, WebP, SVG, PDF, texte, Markdown et Word (à partir de la reconnaissance d'écriture de
  Windows), copie dans le presse-papiers, impression, glisser une note vers l'Explorateur ou Word.
- **Import** des notes de Wacom Inkspace, depuis l'application (« Importer depuis Inkspace… ») ou en ligne de commande.
- **Assistant** : un assistant comme Claude Code ou Claude Desktop peut lire, mettre au propre et ranger les notes par
  le protocole MCP (Ardoise elle-même ne contient aucune intelligence artificielle).
- **Diagnostic** : test guidé de la tablette et journaux détaillés, pour comprendre un problème à distance.
- Interface en **français et en anglais**.

## État du projet

Première version (0.1.0) en préparation. Le dépôt est **privé** et rien n'est encore publié. Toutes les fonctions
qui ne demandent pas la tablette sont vérifiées sous Windows par des tests automatiques ; quelques essais à la main
restent à faire (fermeture de session de Windows, import avec Inkspace ouvert, impression…). Les fonctions qui parlent
à la **vraie tablette** (enregistrement, synchronisation, mode direct) ont été vérifiées avec une tablette simulée et
restent à essayer avec une Bamboo Slate réelle. Voir [les notes de version](docs/notes-de-version.md) et
[la feuille de route](ROADMAP.md).

## Ce qu'il faut

- Windows 11 (Windows 10 est visé mais n'a pas été essayé), **sans droits d'administrateur**.
- Le moteur d'affichage **WebView2** de Microsoft (présent dans Windows 11).
- Pour la tablette : le **Bluetooth à basse consommation** (Bluetooth 4.0 ou plus), intégré ou par une clé USB.
- Tablette visée et testée : Bamboo Slate. Les autres modèles (Spark, Folio, Intuos Pro Paper) ne sont pas testés.

## Installation

Ardoise est **portable** : il n'y a pas d'installateur.

1. Décompressez l'archive de distribution `ardoise-<version>-windows-x64.zip` dans un dossier de votre choix (le dossier
   Nextcloud convient). Elle crée un dossier `ardoise-<version>` qui contient :
   - `ardoise.exe` : le programme, seul fichier indispensable ;
   - `LISEZMOI.txt` : l'essentiel, en français et en anglais ;
   - `LICENSE` et `THIRD-PARTY-NOTICES` : la licence d'Ardoise et celles des composants libres qu'elle utilise ;
   - `guide\` : le guide d'utilisation et le guide de l'assistant en pages HTML (`guide.fr.html`, `guide.en.html`,
     `assistant.fr.html`, `assistant.en.html`), à ouvrir dans un navigateur.
2. Lancez `ardoise.exe`. L'exécutable n'est pas signé : Windows peut afficher « Windows a protégé votre ordinateur » ;
   cliquez sur *Informations complémentaires*, puis *Exécuter quand même*.
3. Au premier lancement, choisissez le dossier de la bibliothèque (de préférence un dossier synchronisé).

Les réglages sont dans `ardoise.toml`, à côté de l'exécutable (repli sur `%APPDATA%\Ardoise`). Un second lancement du
même `ardoise.exe` ramène la fenêtre déjà ouverte au premier plan ; deux exemplaires dans deux dossiers fonctionnent côte
à côte.

## Guides

Dans l'archive de distribution, les guides sont les pages HTML du dossier `guide\` ; dans le dépôt, ce sont les
fichiers Markdown suivants.

- [Guide d'utilisation](docs/guide/guide.fr.md) · [User guide](docs/guide/guide.en.md)
- [L'assistant (MCP)](docs/guide/assistant.fr.md) · [The assistant (MCP)](docs/guide/assistant.en.md)
- [Notes de version](docs/notes-de-version.md)
- [Essais sur le matériel réel](docs/hardware-tests.md) (en français)

## Vie privée

Vos notes restent dans votre dossier : pas de compte, pas de nuage, aucun port réseau en écoute. La seule connexion
sortante possible est le téléchargement, à votre demande, de l'encodeur vidéo de secours OpenH264 chez Cisco.
La reconnaissance d'écriture est celle de Windows, hors connexion. Aucune note personnelle ne figure dans le dépôt.

## Licence et crédits

Ardoise est un logiciel libre, sous licence **GPL-3.0-or-later** (voir [LICENSE](LICENSE)). La liste des composants
tiers et de leurs licences est dans [THIRD-PARTY-NOTICES](THIRD-PARTY-NOTICES).

- **[Tuhi](https://github.com/tuhiproject/tuhi)** (GPL-2.0-or-later) : projet libre qui a documenté le protocole des
  tablettes Wacom ; le protocole d'Ardoise est réécrit à partir de cette documentation et de captures, sans reprendre
  son code.
- **OpenH264** (Cisco Systems, Inc.) : « OpenH264 Video Codec provided by Cisco Systems, Inc. ». Il n'est **jamais
  fourni** avec Ardoise : l'utilisateur le télécharge chez Cisco, seulement s'il en a besoin (MP4 sans l'encodeur de
  Windows).

## Développement

Les consignes de travail du projet sont dans [CLAUDE.md](CLAUDE.md) ; le cahier des charges est
[SPEC.md](SPEC.md) et la feuille de route [ROADMAP.md](ROADMAP.md). Les documents de conception (en anglais) sont dans
[docs/design/](docs/design/).

**Prérequis de compilation (Windows)**

- La chaîne d'outils **Rust** (stable, édition 2024, 1.90 au minimum) avec la cible `wasm32-unknown-unknown`
  (`rustup target add wasm32-unknown-unknown`) : le calcul des contours des traits est compilé en WebAssembly pour
  l'écran.
- **Node.js** 24 et npm (interface Svelte 5 + TypeScript).
- **nasm** (assembleur de l'encodeur AV1 `rav1e`, pour le WebM) : sous Windows, le zip portable décompressé dans
  `%LOCALAPPDATA%\ArdoiseDev\nasm-<version>\` est trouvé par `cargo xtask` ; pour une commande `cargo` directe, définir la
  variable `NASM`. Jamais nécessaire chez l'utilisateur.

**Commandes**

| Commande | Rôle |
|---|---|
| `cargo fmt --all` | Formatage, avant la vérification |
| `cargo xtask check` | Vérification complète (dépendances npm, interface, formatage, clippy sans avertissement, tests Rust, `svelte-check` et vitest, compilation release) ; se termine par `CHECK OK`. `--quick` saute la compilation release |
| `cargo xtask selftest` | Taille de `target\release\ardoise.exe` (moins de 30 Mo) puis `ardoise.exe --self-test` ; se termine par `SELFTEST OK` |
| `cargo xtask e2e` | Parcours de bout en bout sur l'exécutable de production, en français et en anglais ; se termine par `E2E OK` (lancer `check` avant) |
| `cargo xtask licenses [--write]` | Vérifie la licence de chaque dépendance (liste blanche compatible GPL v3, aucune AGPL) et que `THIRD-PARTY-NOTICES` est à jour ; `--write` le régénère |
| `cargo xtask privacy` | Parcourt tout l'historique git à la recherche de données personnelles ; se termine par `PRIVACY OK` |
| `cargo xtask perf` | Mesure les performances (SPEC §8) |
| `cargo xtask dist` | Fabrique l'archive de distribution `target\dist\ardoise-<version>-windows-x64.zip` : un dossier `ardoise-<version>` avec `ardoise.exe`, `LICENSE`, `THIRD-PARTY-NOTICES`, `LISEZMOI.txt` et `guide\*.html` (les guides de `docs/guide/`, convertis en HTML, liens entre pages compris) |

Une compilation release de l'application exige `--features custom-protocol` (sinon Tauri chargerait l'interface depuis le
serveur de développement) ; `cargo xtask check` s'en charge. Interface seule (dans `app/`) : `npm run build`,
`npm run check`, `npm test` ; application en développement : `npm run tauri dev`. Chaque crate se teste séparément :
`cargo test -p ardoise-core`.

Options de l'exécutable utiles au développement : `--self-test`, `--diagnostic [--simulated]`, `--simulated-tablet`,
`--create-demo-library DOSSIER`, `--import-inkspace`, `--create-demo-inkspace DOSSIER`, `--mcp [--read-only]`.

---

## English summary

**Ardoise** is a portable Windows application (Rust + Tauri 2) that replaces Wacom Inkspace for the **Bamboo Slate**
tablet. It receives the pages written on the tablet over Bluetooth LE, lets you write live on screen and on a
projector, keeps your notes as plain files in a folder of your choice (typically synced by Nextcloud), and lets you edit,
replay, and export them (PNG, JPG, WebP, SVG, PDF, text, Markdown, Word, MP4, GIF, WebP, WebM). It can import Inkspace
notes (from the application or the command line) and can be driven by an assistant such as Claude Code or Claude
Desktop through MCP. The interface is in French and English.

- **Status**: first version (0.1.0) in preparation; the repository is private. Everything that does not need the tablet
  is checked on Windows by automated tests; the parts that talk to the real tablet have been checked with a simulated
  tablet only.
- **Requirements**: Windows 11 (Windows 10 not tried), WebView2, and Bluetooth LE for the tablet. No administrator
  rights.
- **Installation**: unzip the portable archive `ardoise-<version>-windows-x64.zip` anywhere; the `ardoise-<version>`
  folder holds `ardoise.exe` (the only essential file), `LICENSE`, `THIRD-PARTY-NOTICES`, `LISEZMOI.txt` and the guides
  as HTML pages in `guide\`. Run `ardoise.exe` (unsigned: Windows SmartScreen may show a warning, choose *More info* then
  *Run anyway*). Settings are kept in `ardoise.toml` next to the executable.
- **Guides**: [user guide](docs/guide/guide.en.md), [assistant guide](docs/guide/assistant.en.md), [release
  notes](docs/notes-de-version.md) (French, with an English section). In the archive, the guides are the HTML pages of
  the `guide\` folder.
- **Privacy**: no account, no cloud, no listening network port; the only possible outgoing connection is the optional
  download of the OpenH264 encoder from Cisco. No personal notes are in the repository.
- **Licence**: GPL-3.0-or-later. Credits: the [Tuhi](https://github.com/tuhiproject/tuhi) project for its documentation
  of the Wacom protocol; OpenH264 by Cisco Systems, Inc., downloaded by the user and never shipped with Ardoise.
- **Development**: Rust (stable, 1.90 or later) with the `wasm32-unknown-unknown` target, Node.js 24, and `nasm`
  (portable zip under `%LOCALAPPDATA%\ArdoiseDev`). Run `cargo xtask check` (ends with `CHECK OK`), then
  `cargo xtask selftest` and `cargo xtask e2e`; also `cargo xtask licenses`, `cargo xtask privacy`, `cargo xtask perf`
  (performance measurements) and `cargo xtask dist` (distribution archive). See [CLAUDE.md](CLAUDE.md).
