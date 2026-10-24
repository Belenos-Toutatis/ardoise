# Questions ouvertes

Points à trancher avec l'utilisateur. Chaque question indique ce qui est fait en attendant.

## Q1 — Installer la cible WebAssembly de Rust ? (jalon M2, pour M3)

SPEC §6.2 prévoit que le calcul du contour des traits (`ardoise-ink`) soit compilé en WebAssembly pour l'affichage à l'écran. Cela demande d'installer la cible `wasm32-unknown-unknown` sur le PC de développement (`rustup target add wasm32-unknown-unknown`, environ 30 Mo téléchargés chez rust-lang.org, rien chez l'utilisateur final).

**En attendant** : `ardoise-ink` est du Rust pur, sans dépendance au système, donc prêt pour WebAssembly ; l'écran obtient les contours par une commande de l'application (`stroke_outlines`), qui appelle exactement le même code que les exports. Le choix final (WebAssembly ou commande) se fera en M3, quand l'éditeur affichera les notes.
