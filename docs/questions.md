# Questions ouvertes

Points à trancher avec l'utilisateur. Chaque question indique ce qui est fait en attendant.

## Q1 — Installer la cible WebAssembly de Rust ? (jalon M2, pour M3) — **tranchée**

SPEC §6.2 prévoit que le calcul du contour des traits (`ardoise-ink`) soit compilé en WebAssembly pour l'affichage à l'écran, ce qui demande la cible `wasm32-unknown-unknown` sur le PC de développement (rien chez l'utilisateur final).

**Décision de l'utilisateur (2026-10-24)** : installation autorisée pour M3. La cible est installée et `cargo build -p ardoise-ink --target wasm32-unknown-unknown` réussit sans modification du code. La commande `note_outlines` (contours calculés par l'application) reste disponible ; M3 choisira l'usage à l'écran.

## Q2 — Le dossier caché `.ardoise/` est-il synchronisé par Nextcloud ? (relecture de M2) — **tranchée**

Le client Nextcloud ne synchronise pas les dossiers cachés (nom commençant par un point) avec ses réglages par défaut ; or ce dossier contient l'enregistrement de la tablette partagé par les PC, les journaux de diagnostic et la corbeille.

**Décision de l'utilisateur (2026-10-24)** : le dossier s'appelle désormais `_ardoise/` (non caché, synchronisé sans réglage). SPEC §3.2 et §4.8 sont mis à jour. Une bibliothèque qui a encore `.ardoise/` est migrée automatiquement au démarrage d'Ardoise et à l'ouverture de la bibliothèque (renommage, ou fusion sans rien écraser si `_ardoise/` existe déjà).

## Q3 — Multiplexeur MP4 du secours OpenH264 : `shiguredo_mp4` au lieu de `muxide` ? (jalon M6) — **ouverte**

SPEC §6.2 (ligne « Vidéo MP4 ») prévoit que le MP4 du secours OpenH264 soit « multiplexé par `muxide` ». À l'essai, `muxide` 0.5.0 ne convient pas :
- le délai de démarrage du son Opus y est figé à 312 échantillons ;
- il n'écrit pas la liste d'édition qui fait finir le son exactement avec la narration ;
- la dernière image reprend la durée de l'image précédente ;
- il garde toutes les données en mémoire jusqu'à la fin.

`shiguredo_mp4` 2026.5.0 (Apache-2.0, sans dépendance) fait tout cela et écrit les données au fil de l'eau.

**En attendant** : le secours utilise `shiguredo_mp4`. Le test de fin de M6 relit le fichier produit avec un lecteur indépendant (`mp4parse`, celui de Firefox), et Media Foundation le lit aussi.

**À trancher** : remplacer `muxide` par `shiguredo_mp4` dans SPEC §6.2.
