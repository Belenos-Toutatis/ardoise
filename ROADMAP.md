# Ardoise — feuille de route

Chaque étape (« jalon ») se termine quand **toutes ses cases sont cochées** et que **ses critères de fin sont prouvés par une sortie de commande visible**. C'est ce qui permet de lancer une étape avec `/goal` (voir la fin du document).

Légende : **[matériel]** = nécessite la vraie tablette, donc l'utilisateur au travail. Ces cases ne bloquent jamais un `/goal` : elles sont regroupées dans des « points de contrôle matériel » (H1, H2, H3).

## Vérification commune à tous les jalons

`cargo xtask check` lance dans l'ordre : installation des dépendances et compilation de l'interface (nécessaire avant de compiler l'application), formatage (`cargo fmt --check`), `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, les vérifications et tests de l'interface, puis `cargo build --release`. Il se termine par la ligne `CHECK OK` (code de sortie 0) ou par la liste des échecs.

Règles permanentes :
- Ne jamais modifier `SPEC.md` sans l'accord de l'utilisateur ; une incohérence découverte est notée dans `docs/questions.md`.
- Ne jamais toucher aux données d'Inkspace ni aux captures de la tablette : travailler sur des copies.
- Mettre à jour la case et ajouter une ligne au `CHANGELOG.md` à chaque tâche terminée.

---

## M0 — Fondations

- [x] Espace de travail Cargo : `crates/` (voir SPEC §6), `app/` (Tauri 2 + interface Svelte/TypeScript), `xtask/`.
- [x] `cargo xtask check` opérationnel, avec la ligne finale `CHECK OK`.
- [x] `CLAUDE.md` : commandes, structure, conventions de code, règles des sous-agents.
- [x] Fenêtre Tauri vide qui s'ouvre ; option `ardoise.exe --self-test` : démarre, vérifie que l'interface est chargée, indique si un adaptateur Bluetooth LE est présent et allumé, quitte avec le code 0.
- [x] Exécutable autonome : CRT lié statiquement, interface embarquée, dossier WebView2 fixé dans `%LOCALAPPDATA%\Ardoise\webview` (SPEC §6.2).
- [x] `LICENSE` (GPL-3.0-or-later) ; `testdata/private/` ignoré par git.
- [x] Traductions FR/EN branchées (bibliothèque i18n, un texte de test dans chaque langue, test qui échoue si une clé manque dans une langue).
- [x] Réglages portables `ardoise.toml` (SPEC §3.1) avec tests : lecture, écriture atomique, section par machine, repli sur `%APPDATA%`.
- [x] Assistant de premier lancement : choix du dossier de bibliothèque.

**Critères de fin** : `cargo xtask check` affiche `CHECK OK` ; `target\release\ardoise.exe --self-test` affiche `SELF-TEST OK` et sort avec le code 0 ; l'exécutable fait moins de 30 Mo (taille affichée).

### Point de contrôle H0 [matériel]

- [ ] [matériel] Au travail : copier `ardoise.exe` dans le dossier Nextcloud, le lancer, noter si Windows le bloque, lancer `--self-test` et vérifier que le dongle Bluetooth est vu. Procédure courte dans `docs/hardware-tests.md` (H0).

## M1 — Protocole, simulateur et outil de diagnostic

- [x] `ardoise-proto` : codage/décodage de tous les messages du protocole (docs/research/protocol.md), sans aucune entrée/sortie ; tests pour chaque message.
- [x] Décodage des données de traits (pages stockées) et des paquets du mode direct ; tests avec les données d'exemple de Tuhi et des données synthétiques.
- [x] `ardoise-sim` : tablette simulée (enregistrement, connexion, infos, liste/téléchargement/suppression des pages, mode direct, bouton) + rejeu de captures `.jsonl`.
- [x] `ardoise-ble` : transport Bluetooth LE Windows (recherche, connexion, écriture, notifications, reconnexion) derrière la même interface que le simulateur.
- [x] Gestionnaire de tablette : enregistrement, connexion avec l'identifiant partagé (`.ardoise/device.json`), synchro, mode direct ; testé de bout en bout contre le simulateur.
- [x] Journal de diagnostic `.jsonl` (SPEC §4.8) ; un journal produit avec le simulateur se rejoue à l'identique.
- [x] Outil de test guidé accessible depuis l'application (ou `ardoise.exe --diagnostic`), avec rapport final.
- [x] `docs/hardware-tests.md` : procédure H1 pas à pas, en français, pour l'utilisateur.

**Critères de fin** : `CHECK OK` ; la sortie des tests montre le scénario complet contre le simulateur (enregistrement → synchro de 3 pages → suppression → mode direct 30 s → déconnexion) réussi ; `ardoise.exe --diagnostic --simulated` produit un rapport « 0 échec ».

### Point de contrôle H1 [matériel]

- [ ] [matériel] L'utilisateur suit `docs/hardware-tests.md` (H1) au travail ; les journaux arrivent via Nextcloud.
- [ ] [matériel] Analyse des journaux reçus, corrections du protocole, ajout des captures réelles aux tests de non-régression (nécessite les journaux de H1).

## M2 — Notes, bibliothèque, rendu et import Inkspace

- [ ] Modèle de note et format `.ardoise` (SPEC §3.3) + `docs/file-format.md` ; tests d'aller-retour et de compatibilité de version.
- [ ] Bibliothèque : lecture, écriture atomique, classeurs, étiquettes, corbeille, surveillance du dossier, détection des copies en conflit Nextcloud ; tests.
- [ ] Géométrie des traits (contour à épaisseur variable selon la pression) partagée par l'écran et les exports.
- [ ] Rendu : outils d'encre, couleurs, fonds, modèles de page, textures ; images de référence et test de comparaison.
- [ ] Import Inkspace (SPEC §6.3, `docs/research/inkspace-format.md`) : les 22 notes de cet ordinateur importées depuis une copie placée dans `testdata/private/` ; pour chacune, comparaison chiffrée avec l'aperçu PNG d'Inkspace (score de similarité affiché, seuil fixé dans le test).
- [ ] Test qui prouve que l'import ne lit jamais la clé `profile` de `LocalState\db` (jeton d'accès et e-mail).

**Critères de fin** : `CHECK OK` ; la commande d'import sur une copie des données Inkspace affiche « 22/22 notes importées » et le tableau des scores de similarité, tous au-dessus du seuil.

## M3 — Interface : bibliothèque et éditeur

- [ ] Bibliothèque : grille de miniatures (cache local), tri, classeurs, glisser-déposer, étiquettes, favoris, recherche, sélection multiple, corbeille.
- [ ] Éditeur : zoom/déplacement, outils, couleurs, épaisseur, fonds, calques, lasso, gomme, annuler/rétablir, rotation, recadrage.
- [ ] Réglages : langue, thème, bibliothèque, tablette.
- [ ] Tests de l'interface (composants) et tests de bout en bout des parcours principaux.

**Critères de fin** : `CHECK OK` ; les tests de bout en bout affichent les parcours « ouvrir une note, la modifier, annuler, sauvegarder, la retrouver par recherche » réussis en français et en anglais.

## M4 — Synchro, mode direct et classe

- [ ] Tablette dans l'interface : assistant d'enregistrement, état, batterie, synchro automatique, réglages de la tablette.
- [ ] Mode direct : affichage temps réel, latence mesurée sur le simulateur (affichée), bouton = nouvelle page ou calque.
- [ ] Fenêtre de présentation plein écran sur l'écran choisi + fenêtre de commande ; pointeur laser ; tableau noir ; navigation entre pages.
- [ ] `docs/hardware-tests.md` : procédure H2.

**Critères de fin** : `CHECK OK` ; test de bout en bout « synchro de 3 pages depuis le simulateur → 3 notes dans la bibliothèque » réussi ; latence médiane du mode direct sur le simulateur affichée et inférieure à 30 ms.

### Point de contrôle H2 [matériel]

- [ ] [matériel] Au travail : synchro réelle, mode direct, projecteur en bureau étendu (procédure H2).
- [ ] Corrections issues du retour de l'utilisateur.

## M5 — Découper, assembler, exporter

- [ ] Découpe par instant (ligne du temps) et par zone ; assemblage superposé et bout à bout ; tests.
- [ ] Exports PNG, JPG, SVG, PDF (une ou plusieurs notes), copie dans le presse-papiers ; tests (fichiers valides, dimensions, nombre de pages).
- [ ] Export automatique PDF/PNG après synchro (réglage).

**Critères de fin** : `CHECK OK` ; un test exporte une note de référence dans les 5 formats et affiche la validation de chaque fichier.

## M6 — Relecture, vidéo et narration

- [ ] Relecture animée : ligne du temps, vitesses, suppression des temps morts.
- [ ] Essai préalable : écriture d'un MP4 H.264 + AAC par Media Foundation (`IMFSinkWriter`) depuis Rust ; résultat noté dans `docs/research/media-stack.md`. Si l'essai échoue, demander à l'utilisateur avant de choisir une autre solution.
- [ ] Avant de télécharger `nasm` (assembleur nécessaire pour un WebM rapide), demander l'accord de l'utilisateur.
- [ ] Exports MP4 (Media Foundation ; secours OpenH264 2.6.0 téléchargé chez Cisco seulement si Media Foundation manque, empreinte vérifiée, mention et licence de Cisco affichées, désactivable et supprimable), GIF, WebP, WebM ; réglages de résolution et de vitesse ; durée estimée affichée avant l'export.
- [ ] Narration : liste des micros, choix, niveau, enregistrement pendant relecture ou mode direct, son dans MP4 et WebM.

**Critères de fin** : `CHECK OK` ; un test produit les 4 formats animés à partir d'une note de référence et affiche, pour chacun, durée, dimensions et nombre d'images relus depuis le fichier ; l'export MP4 avec narration simulée contient une piste audio de la bonne durée ; le secours OpenH264 est testé en forçant son utilisation.

## M7 — Fonctions supplémentaires (P2)

- [ ] Tablette comme stylet Windows (avec pression) sur l'écran choisi.
- [ ] Reconnaissance d'écriture FR/EN hors ligne, recherche dans le texte, export texte.
- [ ] Redressement des formes ; fond image/PDF ; revenir à l'original ; glisser une note vers l'Explorateur.

**Critères de fin** : `CHECK OK` ; tests de chaque fonction réussis (injection de stylet vérifiée par un test qui lit les évènements reçus par une fenêtre de test ; reconnaissance vérifiée sur des notes de référence avec le taux de mots reconnus affiché).

## M8 — Finition et publication

- [ ] Relecture complète par un agent relecteur ; corrections des problèmes bloquants et importants.
- [ ] Performances (SPEC §8) mesurées et affichées.
- [ ] Vérification automatique des licences de toutes les dépendances (aucune AGPL ni incompatible GPL v3) ; `THIRD-PARTY-NOTICES` et écran « À propos » (crédits Tuhi, mention Cisco).
- [ ] Vérification qu'aucune donnée personnelle (notes, captures non anonymisées, jeton Inkspace) n'est dans l'historique git.
- [ ] Guide utilisateur FR/EN, README, notes de version.
- [ ] Archive de distribution (`ardoise.exe` + licence + guide).

### Point de contrôle H3 [matériel]

- [ ] [matériel] Test complet au travail et en classe (procédure H3).
- [ ] Publication sur GitHub **après accord explicite de l'utilisateur**.

---

## Lancer une étape avec `/goal`

Exemple pour M1 (remplacer `M1` par l'étape voulue) :

```
/goal Le jalon M1 de ROADMAP.md est terminé : toutes ses cases sont cochées sauf celles marquées [matériel] (affiche la section pour le prouver), `cargo xtask check` se termine par CHECK OK (sortie affichée), et chaque critère de fin du jalon est prouvé par une sortie de commande affichée. Ne modifie pas SPEC.md. Ou arrête-toi après 60 tours.
```
