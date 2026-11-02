# Questions ouvertes

Points à trancher avec l'utilisateur. Chaque question indique ce qui est fait en attendant.

## Q1 — Installer la cible WebAssembly de Rust ? (jalon M2, pour M3) — **tranchée**

SPEC §6.2 prévoit que le calcul du contour des traits (`ardoise-ink`) soit compilé en WebAssembly pour l'affichage à l'écran, ce qui demande la cible `wasm32-unknown-unknown` sur le PC de développement (rien chez l'utilisateur final).

**Décision de l'utilisateur (2026-10-24)** : installation autorisée pour M3. La cible est installée et `cargo build -p ardoise-ink --target wasm32-unknown-unknown` réussit sans modification du code. La commande `note_outlines` (contours calculés par l'application) reste disponible ; M3 choisira l'usage à l'écran.

## Q2 — Le dossier caché `.ardoise/` est-il synchronisé par Nextcloud ? (relecture de M2) — **tranchée**

Le client Nextcloud ne synchronise pas les dossiers cachés (nom commençant par un point) avec ses réglages par défaut ; or ce dossier contient l'enregistrement de la tablette partagé par les PC, les journaux de diagnostic et la corbeille.

**Décision de l'utilisateur (2026-10-24)** : le dossier s'appelle désormais `_ardoise/` (non caché, synchronisé sans réglage). SPEC §3.2 et §4.8 sont mis à jour. Une bibliothèque qui a encore `.ardoise/` est migrée automatiquement au démarrage d'Ardoise et à l'ouverture de la bibliothèque (renommage, ou fusion sans rien écraser si `_ardoise/` existe déjà).

## Q3 — Multiplexeur MP4 du secours OpenH264 : `shiguredo_mp4` au lieu de `muxide` ? (jalon M6) — **tranchée**

SPEC §6.2 (ligne « Vidéo MP4 ») prévoit que le MP4 du secours OpenH264 soit « multiplexé par `muxide` ». À l'essai, `muxide` 0.5.0 ne convient pas :
- le délai de démarrage du son Opus y est figé à 312 échantillons ;
- il n'écrit pas la liste d'édition qui fait finir le son exactement avec la narration ;
- la dernière image reprend la durée de l'image précédente ;
- il garde toutes les données en mémoire jusqu'à la fin.

`shiguredo_mp4` 2026.5.0 (Apache-2.0, sans dépendance) fait tout cela et écrit les données au fil de l'eau.

**En attendant** : le secours utilise `shiguredo_mp4`. Le test de fin de M6 relit le fichier produit avec un lecteur indépendant (`mp4parse`, celui de Firefox), et Media Foundation le lit aussi.

**Décision de l'utilisateur (2026-10-24)** : `shiguredo_mp4` est retenu ; SPEC §6.2 est mis à jour (ligne « Vidéo MP4 » et risque des bibliothèques vidéo jeunes).

## Q4 — Afficher la feuille sur un autre ordinateur du réseau local ? (2026-11-02) — **tranchée**

L'utilisateur demande qu'un autre ordinateur du même réseau puisse afficher la feuille en direct, ou la feuille consultée. SPEC §6.2 interdisait tout port en écoute, parce que l'alerte du pare-feu de Windows demande des droits d'administrateur.

**Décision de l'utilisateur (2026-11-02)** : c'est autorisé.
- Le partage tourne sur un ordinateur personnel dont l'utilisateur est administrateur.
- Le réseau de son établissement laisse déjà un appareil afficher son écran sur le PC du bureau.
- L'autre ordinateur n'affiche jamais la bibliothèque, seulement la feuille du mode direct ou la feuille consultée ou modifiée ; c'est toujours le PC d'Ardoise qui choisit.
- Les protections proposées sont retenues : partage lancé à la main, lecture seule, code d'accès, arrêt à la fermeture.

SPEC §4.10 est ajouté, la ligne « Réseau » du §6.2 est mise à jour, et le jalon M11 est ajouté à ROADMAP.md.

## Q5 — Ajouter `ardoise-share` à la liste des crates de SPEC §6.1 ? (jalon M11) — **tranchée**

Le serveur du partage sur le réseau local (§4.10) est une nouvelle bibliothèque, `crates/ardoise-share`, indépendante de Tauri pour être testée sous Linux avec un vrai client TCP (`docs/design/lan-share.md`). SPEC §6.1 ne la cite pas, et SPEC.md ne se modifie pas sans l'accord de l'utilisateur.

**En attendant** : la crate existe, son rôle est écrit en tête de son `lib.rs` et dans `docs/design/lan-share.md`. Ligne proposée pour §6.1 : `ardoise-share     partage de la feuille sur le réseau local : petit serveur HTTP en lecture seule (code, flux d'évènements)`.

**Décision de l'utilisateur (2026-11-02)** : accord ; la ligne proposée est ajoutée à SPEC §6.1.

## Q6 — Montrer sur l'autre écran les annotations écrites par-dessus la présentation ? (jalon M11, 2026-11-02) — **tranchée**

La relecture de M11 a relevé que les annotations écrites au stylet, au doigt ou à la souris sur la fenêtre de présentation (rouges, jaunes sur le tableau noir, temporaires, jamais enregistrées dans la note) restaient sur le PC : le partage ne montrait que l'écriture de la tablette et la vue (zoom, déplacement, tableau noir, pointeur laser).

**Décision de l'utilisateur (2026-11-02)** : les montrer aussi sur l'autre écran, pour qu'il affiche exactement ce que montre le vidéoprojecteur. SPEC §4.10 et ROADMAP M11 sont mis à jour.
