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

## Q7 — Que faire quand le port du partage est déjà pris ? (jalon M11, 2026-11-02) — **tranchée**

Sur le PC de l'utilisateur, le port 8765 était pris par un autre programme (l'aide Python d'un serveur MCP pour
LibreOffice, qui démarre avec Claude Desktop) : le partage refusait de démarrer et renvoyait vers `ardoise.toml`.

**Décision de l'utilisateur (2026-11-02)** : les deux propositions.
- Si le port est pris, Ardoise essaie les ports suivants et prend le premier libre ; le panneau du partage le dit, et le
  port trouvé est retenu pour ce PC (section de la machine dans `ardoise.toml`), pour que l'adresse reste la même.
- Un réglage « Port du partage » dans les réglages d'Ardoise, pour choisir un port sans modifier `ardoise.toml` à la main.

SPEC §4.10 (ligne « Réseau ») est mis à jour.

## Q8 — Laisser l'autre écran régler son affichage ? (jalon M11, 2026-11-03) — **tranchée**

Premier essai du partage par l'utilisateur, sur deux ordinateurs de son réseau à la maison (2026-11-03) : il fonctionne.
Mais sur l'ordinateur qui reçoit l'image, on ne peut rien faire, alors qu'il serait utile de mieux voir la page.

**Décision de l'utilisateur (2026-11-03)** : l'autre écran a des options d'affichage pour lui seul :
- page entière et pleine largeur ;
- zoom sur un endroit, et déplacement dans la page quand elle n'est pas affichée en entier.

Ces réglages ne changent rien sur le PC d'Ardoise, ni le contenu partagé : c'est toujours le PC qui choisit ce qui est
montré. Pendant le mode direct, l'autre écran suit la vue de la présentation tant qu'on n'y touche pas, et un bouton l'y
ramène. SPEC §4.10 est mis à jour.

## Q9 — Reprendre le direct sur une feuille, et retoucher pendant le direct (jalon M12, 2026-11-04) — **tranchée**

L'utilisateur demande de pouvoir reprendre le mode direct sur une feuille existante après une interruption, et de sélectionner quelque chose pendant le direct pour le supprimer ou changer sa couleur, sans passer par l'éditeur.

**Décisions de l'utilisateur (2026-11-04)** :
- On sélectionne dans la **fenêtre de commande seulement** : rien de la sélection n'apparaît au vidéoprojecteur, seul le résultat.
- La reprise écrit dans la **même note**, dont l'état d'origine reste récupérable.
- On reprend **une feuille** (depuis le menu d'une note ou l'éditeur), pas un cours entier.
- **Ctrl+Z** annule la suppression, le changement de couleur et l'effacement de la page, pas l'écriture de la tablette.

**Choix par défaut, à contester si besoin** (ils ne changent pas la demande) :
- La page suivante, après une feuille reprise, crée une note neuve.
- Si la feuille n'est pas tenue comme la tablette (portrait ou paysage), le direct suit la feuille pour cette session : la note n'est jamais tournée, et le bouton pour tourner est désactivé sur cette page.
- Si la feuille n'a pas le format de la tablette, l'écriture est mise à l'échelle pour tenir entière dans la feuille, centrée et sans déformation.
- Si la feuille a changé ailleurs pendant le direct, une nouvelle note est créée dans le même classeur, et Ardoise le dit.
- Le vidéoprojecteur et les écrans du partage montrent le fond de la feuille reprise (modèle, texture, image ou page de PDF), et jamais ses calques masqués.
- Déplacer une sélection reste réservé à l'éditeur.

SPEC §4.2 est mis à jour et le jalon M12 est ajouté à ROADMAP.md. Conception : `docs/design/tablet-live-classroom.md`, « M12 ».
