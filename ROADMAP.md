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
- [x] Gestionnaire de tablette : enregistrement, connexion avec l'identifiant partagé (`_ardoise/device.json`), synchro, mode direct ; testé de bout en bout contre le simulateur.
- [x] Journal de diagnostic `.jsonl` (SPEC §4.8) ; un journal produit avec le simulateur se rejoue à l'identique.
- [x] Outil de test guidé accessible depuis l'application (ou `ardoise.exe --diagnostic`), avec rapport final.
- [x] `docs/hardware-tests.md` : procédure H1 pas à pas, en français, pour l'utilisateur.

**Critères de fin** : `CHECK OK` ; la sortie des tests montre le scénario complet contre le simulateur (enregistrement → synchro de 3 pages → suppression → mode direct 30 s → déconnexion) réussi ; `ardoise.exe --diagnostic --simulated` produit un rapport « 0 échec ».

### Point de contrôle H1 [matériel]

- [x] [matériel] L'utilisateur suit `docs/hardware-tests.md` (H1) au travail ; les journaux arrivent via Nextcloud.
- [x] [matériel] Analyse des journaux reçus, corrections du protocole, ajout des captures réelles aux tests de non-régression (nécessite les journaux de H1).

Fait le 2026-11-02 avec la vraie Slate A4 de l'utilisateur. Les journaux ont montré :
- une version du firmware mal lue (un octet par chiffre hexadécimal) ;
- une synchronisation refusée (`ec` envoyé après le mode papier) ;
- une recherche qui se rabattait sur n'importe quelle tablette ;
- l'évènement de batterie.

Tout est corrigé (`docs/research/protocol.md` §5.3 et §12). Le test guidé suivant a réussi ses 8 étapes. Les captures réelles sont rejouées par `crates/ardoise-sim/tests/real_captures.rs` : elles sont lues dans `testdata/private/captures/`, et le test se saute de lui-même sans elles. Reste à confirmer à l'usage, avec la section H1 bis de `docs/hardware-tests.md` : une deuxième synchronisation sur la même connexion, et la baisse du pourcentage de batterie.

## M2 — Notes, bibliothèque, rendu et import Inkspace

- [x] Modèle de note et format `.ardoise` (SPEC §3.3) + `docs/file-format.md` ; tests d'aller-retour et de compatibilité de version.
- [x] Bibliothèque : lecture, écriture atomique, classeurs, étiquettes, corbeille, surveillance du dossier, détection des copies en conflit Nextcloud ; tests.
- [x] Géométrie des traits (contour à épaisseur variable selon la pression) partagée par l'écran et les exports.
- [x] Rendu : outils d'encre, couleurs, fonds, modèles de page, textures ; images de référence et test de comparaison.
- [x] Import Inkspace (SPEC §6.3, `docs/research/inkspace-format.md`) : les 22 notes de cet ordinateur importées depuis une copie placée dans `testdata/private/` ; pour chacune, comparaison chiffrée avec l'aperçu PNG d'Inkspace (score de similarité affiché, seuil fixé dans le test).
- [x] Test qui prouve que l'import ne lit jamais la clé `profile` de `LocalState\db` (jeton d'accès et e-mail).

**Critères de fin** : `CHECK OK` ; la commande d'import sur une copie des données Inkspace affiche « 22/22 notes importées » et le tableau des scores de similarité, tous au-dessus du seuil.

## M3 — Interface : bibliothèque et éditeur

- [x] Bibliothèque : grille de miniatures (cache local), tri, classeurs, glisser-déposer, étiquettes, favoris, recherche, sélection multiple, corbeille.
- [x] Éditeur : zoom/déplacement, outils, couleurs, épaisseur, fonds, calques, lasso, gomme, annuler/rétablir, rotation, recadrage.
- [x] Réglages : langue, thème, bibliothèque, tablette.
- [x] Tests de l'interface (composants) et tests de bout en bout des parcours principaux.

**Critères de fin** : `CHECK OK` ; les tests de bout en bout affichent les parcours « ouvrir une note, la modifier, annuler, sauvegarder, la retrouver par recherche » réussis en français et en anglais.

## M4 — Synchro, mode direct et classe

- [x] Tablette dans l'interface : assistant d'enregistrement, état, batterie, synchro automatique, réglages de la tablette.
- [x] Mode direct : affichage temps réel, latence mesurée sur le simulateur (affichée), bouton = nouvelle page ou calque.
- [x] Fenêtre de présentation plein écran sur l'écran choisi + fenêtre de commande ; pointeur laser ; tableau noir ; navigation entre pages.
- [x] `docs/hardware-tests.md` : procédure H2.

**Critères de fin** : `CHECK OK` ; test de bout en bout « synchro de 3 pages depuis le simulateur → 3 notes dans la bibliothèque » réussi ; latence médiane du mode direct sur le simulateur affichée et inférieure à 30 ms.

### Point de contrôle H2 [matériel]

- [ ] [matériel] Au travail : synchro réelle, mode direct, projecteur en bureau étendu (procédure H2).
- [ ] Corrections issues du retour de l'utilisateur.

## M5 — Découper, assembler, exporter

- [x] Découpe par instant (ligne du temps) et par zone ; assemblage superposé et bout à bout ; tests.
- [x] Exports PNG, JPG, SVG, PDF (une ou plusieurs notes), copie dans le presse-papiers ; tests (fichiers valides, dimensions, nombre de pages).
- [x] Export automatique PDF/PNG après synchro (réglage).

**Critères de fin** : `CHECK OK` ; un test exporte une note de référence dans les 5 formats et affiche la validation de chaque fichier.

## M6 — Relecture, vidéo et narration

- [x] Relecture animée : ligne du temps, vitesses, suppression des temps morts.
- [x] Essai préalable : écriture d'un MP4 H.264 + AAC par Media Foundation (`IMFSinkWriter`) depuis Rust ; résultat noté dans `docs/research/media-stack.md`. Si l'essai échoue, demander à l'utilisateur avant de choisir une autre solution.
- [x] Avant de télécharger `nasm` (assembleur nécessaire pour un WebM rapide), demander l'accord de l'utilisateur. Accord donné le 24 octobre 2026 pour `nasm` (poste de développement) et pour la DLL OpenH264 2.6.0 de Cisco (test du secours).
- [x] Exports MP4 (Media Foundation ; secours OpenH264 2.6.0 téléchargé chez Cisco seulement si Media Foundation manque, empreinte vérifiée, mention et licence de Cisco affichées, désactivable et supprimable), GIF, WebP, WebM ; réglages de résolution et de vitesse ; durée estimée affichée avant l'export.
- [x] Narration : liste des micros, choix, niveau, enregistrement pendant relecture ou mode direct, son dans MP4 et WebM.

**Critères de fin** : `CHECK OK` ; un test produit les 4 formats animés à partir d'une note de référence et affiche, pour chacun, durée, dimensions et nombre d'images relus depuis le fichier ; l'export MP4 avec narration simulée contient une piste audio de la bonne durée ; le secours OpenH264 est testé en forçant son utilisation.

## M7 — Fonctions supplémentaires (P2)

- [x] Reconnaissance d'écriture FR/EN hors ligne, recherche dans le texte, export texte.
- [x] Redressement des formes ; fond image/PDF ; revenir à l'original ; glisser une note vers l'Explorateur.

**Critères de fin** : `CHECK OK` ; tests de chaque fonction réussis (reconnaissance vérifiée sur des notes de référence avec le taux de mots reconnus affiché).

Vérifié sous Windows le 2026-10-25 : `CHECK OK`, `E2E OK` (14 parcours, FR et EN), reconnaissance en français 100 % des mots des notes de référence. Restent à essayer à la main, faute de pouvoir les automatiser ici : l'anglais (le module d'écriture anglais de Windows n'est pas installé sur ce PC), un vrai dépôt d'une note glissée dans l'Explorateur, Word ou OneNote, et la pause de la lecture de l'écriture pendant la présentation.

## M8 — Retours des premiers essais

Remarques de l'utilisateur après ses premiers essais sans la tablette (25 octobre 2026), à faire en une seule série.

- [x] Feuille : choisir portrait ou paysage sans tourner l'écriture ; changer le format du papier (A4, A5, Letter, personnalisé) en gardant l'écriture ; tourner seulement l'image ou la page de PDF importée ; une note faite à partir d'un PDF prend l'orientation de sa page ; copier-coller des traits d'une note à l'autre.
- [x] Orientation du bloc-notes : les quatre positions d'Inkspace (paysage ou portrait, dans un sens ou dans l'autre) à la place de droitier / gaucher, appliquées à la synchronisation et au mode direct, avec un bouton rapide dans la fenêtre du direct.
- [x] Stylet et tactile : le dos du stylet Surface gomme ; pression, bouton latéral, main posée sur l'écran ignorée ; pincer pour zoomer, glisser pour déplacer ; zoom « ajuster à la largeur » partout où l'on zoome ; écrire par-dessus la présentation au stylet ou au doigt.
- [x] Exports : WebP dans l'export image ; export texte en Markdown et en Word avec la structure tirée de l'écriture (paragraphes, listes, dessins) ; impression directe.

**Critères de fin** : `CHECK OK` et `E2E OK` ; tests de chaque fonction ; stylet et écran tactile essayés à la main sur un PC tactile.

Vérifié sous Windows le 2026-10-29 : `CHECK OK`, `E2E OK` (26 parcours, FR et EN), `SELFTEST OK`, après une relecture indépendante dont les remarques sont corrigées. Restent à essayer à la main : le stylet et l'écran tactile sur un PC tactile (`docs/hardware-tests.md`, H3 ; le dos du stylet ne peut pas être simulé par les tests), les quatre positions avec la vraie tablette (C3), l'impression sur une vraie imprimante, l'export Word ouvert dans Word et la mise en forme Markdown/Word sur une vraie écriture.

## M9 — Pilotage par un assistant (MCP)

- [x] Serveur MCP `ardoise.exe --mcp` (SDK Rust officiel de MCP, licence vérifiée) ; outils de lecture : liste, recherche, note, texte reconnu avec positions, rendu en image d'une page ou d'une zone.
- [x] Outils d'écriture (créer, renommer, étiqueter, ranger, assembler, découper, redresser, exporter, texte corrigé), sans suppression définitive ; invites prêtes à l'emploi.
- [x] Passerelle vers l'application ouverte : ouvrir une note, lancer la présentation, changer de page, version propre à la fin du mode direct.
- [x] Guide : déclarer Ardoise dans Claude Code et Claude Desktop ; rappel sur la vie privée.

**Critères de fin** : `CHECK OK` et `E2E OK` ; un test lance `ardoise.exe --mcp`, liste ses outils et appelle chacun sur la bibliothèque de démonstration ; essai réel depuis Claude Code (mettre au propre une note de démonstration).

Vérifié sous Windows le 2026-10-29 : `CHECK OK`, `E2E OK` (30 parcours, FR et EN ; le serveur MCP y est lancé, liste ses outils et appelle chacun des 20 sur la bibliothèque de démonstration, et récupère les 3 invites dans chaque langue), `SELFTEST OK`, après une relecture indépendante dont les remarques sont corrigées. Reste l'essai réel depuis Claude Code, qui attend l'accord de l'utilisateur pour déclarer Ardoise dans sa configuration, puis à essayer à la main : la commande sous PowerShell 7 (`pwsh`, absent de ce PC), la présentation sur un second écran pilotée par l'assistant et la note du dernier mode direct retrouvée après un vrai redémarrage.

## M10 — Finition et publication

- [x] Relecture complète par un agent relecteur ; corrections des problèmes bloquants et importants.
- [x] Performances (SPEC §8) mesurées et affichées.
- [x] Vérification automatique des licences de toutes les dépendances (aucune AGPL ni incompatible GPL v3) ; `THIRD-PARTY-NOTICES` et écran « À propos » (crédits Tuhi, mention Cisco).
- [x] Vérification qu'aucune donnée personnelle (notes, captures non anonymisées, jeton Inkspace) n'est dans l'historique git.
- [x] Guide utilisateur FR/EN, README, notes de version.
- [x] Archive de distribution (`ardoise.exe` + licence + guide).

Vérifié sous Windows le 2026-10-29 : `CHECK OK`, `E2E OK` (58 parcours, FR et EN, dont le serveur MCP), `SELFTEST OK`, `PERF OK` (PC au repos : démarrage 0,95 s, 1 000 notes listées en 1,8 s, défilement et mode direct à 60 images/s, MP4 d'une relecture de 2 min en 15 s, exécutable de 21 Mo), `DIST OK` (archive de 8,2 Mo). Deux relectures complètes (Rust, interface et documentation) dont les remarques importantes sont corrigées, ainsi que les retours d'essai du 29 octobre (rotation de plusieurs notes, sélection au stylet, export Markdown fidèle, Échap dans la fenêtre d'export). Ajouts demandés par le cahier des charges et trouvés à la relecture : import d'Inkspace depuis l'interface, étiquettes de couleur, classeurs renommés et supprimés, brouillons automatiques. Restent à essayer à la main : une vraie déconnexion de Windows avec une note modifiée, l'import d'Inkspace pendant qu'Inkspace est ouvert, les couleurs d'étiquettes avec Nextcloud sur deux PC, le déplacement d'une sélection au stylet sur la Surface, et l'essai réel de M9 depuis Claude Code (connexion du Claude Code en ligne de commande à refaire).

### Point de contrôle H3 [matériel]

- [ ] [matériel] Test complet au travail et en classe (procédure H3).
- [ ] Publication sur GitHub **après accord explicite de l'utilisateur**.

## M11 — Partage de la feuille sur le réseau local

Ajouté à la demande de l'utilisateur le 2026-11-02 (SPEC §4.10, `docs/questions.md` Q4). Conception technique : `docs/design/lan-share.md`.

- [x] Serveur de partage dans l'exécutable : HTTP et flux d'évènements vers le navigateur ; démarré et arrêté à la main ; port réglable ; code de 6 caractères avec tentatives limitées ; lecture seule. Aucune adresse ne donne accès à autre chose que la feuille partagée.
- [x] Page de visionnage pour le navigateur (rien à installer) : même rendu des traits que l'application (WebAssembly), plein écran, reconnexion automatique, écran d'attente.
- [x] Contenu partagé, choisi par le PC d'Ardoise :
  - la feuille du mode direct, avec la vue de la présentation (zoom, déplacement, tableau noir, pointeur laser) et les annotations écrites par-dessus la présentation ;
  - ou la page de la note ouverte, mise à jour à chaque modification ;
  - et une pause qui fige l'image.
- [x] Interface :
  - un bouton « Partager sur le réseau » dans la fenêtre de commande du mode direct et dans l'éditeur ;
  - l'adresse, un QR code, le code et le nombre d'écrans connectés ;
  - l'arrêt du partage ;
  - les traductions FR et EN.
- [x] Guide FR et EN (pare-feu, réseau privé, adresse à taper) ; procédure H4 dans `docs/hardware-tests.md`.
- [ ] Port déjà pris (`docs/questions.md` Q7) : Ardoise prend le premier port libre qui suit, le dit et le retient pour ce PC ; réglage « Port du partage » dans les réglages.

**Critères de fin** :
- `CHECK OK` et `E2E OK` ;
- un test démarre le partage et se connecte comme un navigateur. Il vérifie le code, reçoit les traits du mode direct et les modifications d'une note, et vérifie qu'aucune autre adresse ne donne accès à la bibliothèque ;
- le partage s'arrête bien (port fermé, écrans déconnectés) ;
- l'exécutable reste sous 30 Mo et les licences sont vérifiées.

Vérifié sous Windows le 2026-11-02 : `CHECK OK`, `E2E OK` (60 parcours, FR et EN), `SELFTEST OK` (exécutable de 22,0 Mo, page de visionnage embarquée), licences vérifiées. Le parcours du partage compte 24 étapes :
- le code, et toute autre adresse refusée ;
- le mode direct avec les annotations de la présentation, la pause, puis la note de l'éditeur ;
- la page de visionnage ouverte dans un vrai Edge ;
- rien de personnel dans le flux ;
- l'arrêt, avec le port fermé.

Une relecture indépendante a été faite, et ses remarques sont corrigées : calques masqués jamais envoyés, liste blanche du flux, places de connexion réservées et partage équitable, contrôle de l'en-tête `Host`, port exclusif sous Windows, rythme des reconnexions. Les annotations écrites par-dessus la présentation sont partagées aussi, à la demande de l'utilisateur (`docs/questions.md`, Q6). Restent à essayer sur un vrai réseau (H4) : l'alerte du pare-feu, l'adresse tapée et le QR code, le délai ressenti, la reconnexion après une coupure, un téléphone et d'autres navigateurs.

### Point de contrôle H4 [matériel]

- [ ] [matériel] Un second ordinateur du réseau affiche la feuille en direct, puis une note ouverte. À noter : le délai perçu, l'autorisation du pare-feu, l'adresse tapée ou le QR code.

---

## Lancer une étape avec `/goal`

Exemple pour M1 (remplacer `M1` par l'étape voulue) :

```
/goal Le jalon M1 de ROADMAP.md est terminé : toutes ses cases sont cochées sauf celles marquées [matériel] (affiche la section pour le prouver), `cargo xtask check` se termine par CHECK OK (sortie affichée), et chaque critère de fin du jalon est prouvé par une sortie de commande affichée. Ne modifie pas SPEC.md. Ou arrête-toi après 60 tours.
```

Dans une session cloud (Linux, voir la section « Sessions cloud » de `CLAUDE.md`), les cases ne sont cochées qu'après la vérification sur le PC. Exemple pour M5 :

```
Le jalon M5 de ROADMAP.md est réalisé sur la branche de cette session : le code et les tests de toutes ses cases sont écrits, `cargo xtask check` se termine par CHECK OK sous Linux (sortie affichée), et chaque critère de fin qui ne demande pas Windows est prouvé par une sortie de commande affichée. Suis la section « Sessions cloud » de CLAUDE.md : ne coche aucune case, pousse seulement la branche de la session, ne modifie pas SPEC.md. Termine par un compte rendu : ce qui est fait, ce qui reste à vérifier sous Windows et les commandes à lancer sur le PC. Ou arrête-toi après 60 tours.
```
