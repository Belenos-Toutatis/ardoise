# Questions ouvertes

Points à trancher avec l'utilisateur. Chaque question indique ce qui est fait en attendant.

## Q1 — Installer la cible WebAssembly de Rust ? (jalon M2, pour M3)

SPEC §6.2 prévoit que le calcul du contour des traits (`ardoise-ink`) soit compilé en WebAssembly pour l'affichage à l'écran. Cela demande d'installer la cible `wasm32-unknown-unknown` sur le PC de développement (`rustup target add wasm32-unknown-unknown`, environ 30 Mo téléchargés chez rust-lang.org, rien chez l'utilisateur final).

**En attendant** : `ardoise-ink` est du Rust pur, sans dépendance au système, donc prêt pour WebAssembly ; l'écran obtient les contours par une commande de l'application (`stroke_outlines`), qui appelle exactement le même code que les exports. Le choix final (WebAssembly ou commande) se fera en M3, quand l'éditeur affichera les notes.

## Q2 — Le dossier caché `.ardoise/` est-il synchronisé par Nextcloud ? (relecture de M2)

SPEC §3.2 range dans `<bibliothèque>/.ardoise/` l'enregistrement de la tablette (`device.json`, partagé entre les PC), les journaux de diagnostic (qui doivent revenir du travail par Nextcloud) et la corbeille. Or le client Nextcloud ne synchronise pas les fichiers et dossiers cachés (nom commençant par un point) tant que la case « Synchroniser les fichiers cachés » n'est pas cochée (Paramètres → Général → Modifier les fichiers ignorés). Si elle est décochée sur un PC : la tablette devrait être réenregistrée sur chaque PC, les journaux de H1 ne reviendraient pas, et une note mise à la corbeille sur un PC apparaîtrait comme supprimée sur l'autre.

Deux solutions :
1. **Garder `.ardoise/`** et cocher la case sur les deux PC (ajouté aux procédures H0/H1) ; Ardoise pourrait plus tard lire la configuration du client Nextcloud (`%APPDATA%\Nextcloud\nextcloud.cfg`) pour avertir si la case est décochée.
2. **Renommer le dossier** avec un nom non caché (par exemple `_ardoise`), ce qui marche sans réglage. Cela modifie SPEC §3.2 : il faut ton accord.

**En attendant** : rien n'est renommé ; la procédure H1 demande de vérifier la case.
