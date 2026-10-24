# Questions ouvertes

Points à trancher avec l'utilisateur. Chaque question indique ce qui est fait en attendant.

## Q1 — Installer la cible WebAssembly de Rust ? (jalon M2, pour M3) — **tranchée**

SPEC §6.2 prévoit que le calcul du contour des traits (`ardoise-ink`) soit compilé en WebAssembly pour l'affichage à l'écran, ce qui demande la cible `wasm32-unknown-unknown` sur le PC de développement (rien chez l'utilisateur final).

**Décision de l'utilisateur (2026-10-24)** : installation autorisée pour M3. La cible est installée et `cargo build -p ardoise-ink --target wasm32-unknown-unknown` réussit sans modification du code. La commande `note_outlines` (contours calculés par l'application) reste disponible ; M3 choisira l'usage à l'écran.

## Q2 — Le dossier caché `.ardoise/` est-il synchronisé par Nextcloud ? (relecture de M2) — **tranchée**

Le client Nextcloud ne synchronise pas les dossiers cachés (nom commençant par un point) avec ses réglages par défaut ; or ce dossier contient l'enregistrement de la tablette partagé par les PC, les journaux de diagnostic et la corbeille.

**Décision de l'utilisateur (2026-10-24)** : le dossier s'appelle désormais `_ardoise/` (non caché, synchronisé sans réglage). SPEC §3.2 et §4.8 sont mis à jour. Une bibliothèque qui a encore `.ardoise/` est migrée automatiquement au démarrage d'Ardoise et à l'ouverture de la bibliothèque (renommage, ou fusion sans rien écraser si `_ardoise/` existe déjà).
