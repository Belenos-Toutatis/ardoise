# Journal des modifications

## Non publié

### Ajouté (jalon M5 — découper, assembler, exporter)

- Exports PNG (fond transparent possible), JPG, SVG (vectoriel, calques Inkscape) et PDF (vectoriel, une page par note, taille réelle ou A4), résolution au choix ; depuis l'éditeur (note en cours, modifications non enregistrées comprises) ou depuis la sélection de la bibliothèque (un seul PDF pour plusieurs notes, ou un fichier par note dans un dossier, sans jamais écraser un fichier existant).
- Copie de l'image d'une note dans le presse-papiers de Windows (formats « PNG » et image classique), pour la coller dans Word ou OneNote.
- Tous les formats dessinent les traits avec le même contour (`ardoise-ink`) et le même aspect de chaque outil que l'écran et les miniatures.
- Découper une note à un instant de la ligne du temps (tout ce qui a été écrit ensuite passe dans une nouvelle note) ou selon une zone tracée, avec aperçu des traits qui partent.
- Assembler plusieurs notes sélectionnées : superposées sur une seule page ou bout à bout (page longue), sans modifier les notes d'origine.
- Export automatique PDF ou PNG de chaque page synchronisée dans un sous-dossier de la bibliothèque (réglage, dossier `Exports` par défaut, masqué de la liste des classeurs), pour lire les notes sur le téléphone avec l'appli Nextcloud ; branché à l'endroit où chaque page synchronisée devient une note (vérifiée avant effacement de la tablette).

### Corrigé

- Deux tests de `ardoise-core` passent aussi sous Linux (sessions cloud) : noms de fichiers longs, chemins de corbeille avec lettre de lecteur (un nom contenant `:` est refusé sur tous les systèmes).

### Ajouté (jalon M3 — interface : bibliothèque et éditeur)

- Bibliothèque : grille de miniatures (cache local à chaque PC), tri par date ou titre, classeurs en arborescence, glisser-déposer, étiquettes, favoris, recherche sans accents ni majuscules, sélection multiple (déplacer, étiqueter, favori, corbeille), corbeille avec restauration, rafraîchissement automatique quand une note change sur un autre PC.
- Éditeur : zoom et déplacement, stylo (outils d'encre, couleurs d'Inkspace et couleurs personnalisées, épaisseur), gomme (trait entier ou partielle), sélection au lasso (déplacer, redimensionner, recolorer, copier, supprimer), papiers, modèles et texture, calques, rotation par quart de tour, recadrage automatique ou à la main, annuler/rétablir sans limite, renommage, enregistrement protégé contre l'écrasement d'une version venue d'un autre PC (la note modifiée peut alors être enregistrée comme copie), avertissement quand la note ouverte change ou disparaît sur un autre PC, confirmation avant de fermer la fenêtre avec des modifications non enregistrées.
- Les traits sont dessinés à l'écran par le même code que les exports, compilé en WebAssembly.
- Réglages : section Tablette (tablette enregistrée, l'oublier, la tester).
- Favoris enregistrés dans la note (format de fichier 1.1).
- `ardoise.exe --create-demo-library` et `cargo xtask e2e` (parcours de bout en bout en français et en anglais).

### Modifié

- Le dossier de données de la bibliothèque `.ardoise/` s'appelle désormais `_ardoise/` : n'étant plus caché, Nextcloud le synchronise sans réglage particulier (enregistrement de la tablette partagé, journaux de diagnostic, corbeille). Les bibliothèques existantes sont migrées automatiquement, sans rien écraser.

### Ajouté (jalon M2 — notes, bibliothèque, rendu, import Inkspace)

- Modèle de note et format de fichier `.ardoise` (archive ZIP : `note.json`, `strokes.bin`, original de la tablette, miniature…), versionné et décrit dans `docs/file-format.md`.
- Bibliothèque : notes rangées en classeurs (sous-dossiers), noms de fichiers lisibles, étiquettes, corbeille avec restauration, surveillance du dossier, détection des copies en conflit de Nextcloud, protection contre l'écrasement d'une note modifiée sur un autre PC.
- Géométrie des traits (contour à épaisseur variable selon la pression), commune à l'écran et aux exports.
- Rendu des notes : stylo à bille, stylo plume, feutre, crayon, pinceau, surligneur, craie ; papiers blanc, crème, kraft, ardoise ; modèles ligné, Seyès, quadrillé 5 mm, millimétré, pointillé, portée ; texture de papier. Images de référence vérifiées par les tests.
- Import des notes Wacom Inkspace (`ardoise.exe --import-inkspace`), sans jamais lire le jeton de connexion d'Inkspace, avec comparaison chiffrée de chaque note à l'aperçu d'Inkspace.

### Ajouté (jalon M1 — protocole, simulateur, outil de diagnostic)

- `ardoise-proto` : codage et décodage de tous les messages de la Bamboo Slate, des pages stockées (format Slate, et Intuos Pro pour les données d'exemple de Tuhi) et des paquets du mode direct, sans aucune entrée/sortie.
- Interface commune « transport » pour la vraie tablette, le simulateur et le rejeu de journaux ; horloge virtuelle pour des tests instantanés.
- `ardoise-ble` : transport Bluetooth LE Windows (btleplug) : recherche, connexion, écriture, notifications, déconnexion et reconnexion.
- `ardoise-sim` : tablette simulée (enregistrement, connexion, informations, pages, mode direct, bouton) et rejeu des journaux `.jsonl`.
- Gestionnaire de tablette : enregistrement unique partagé par tous les PC (`_ardoise/device.json`), connexion, synchronisation avec effacement seulement après enregistrement vérifié, mode direct avec reconnexion automatique.
- Journal de diagnostic `_ardoise/diagnostics/AAAA-MM-JJ_HHMMSS.jsonl` (chaque octet échangé, avec son interprétation), écrit de façon atomique.
- Test guidé de la tablette (bouton « Tester la tablette », ou `ardoise.exe --diagnostic`) : 8 étapes, rapport en français ou en anglais, pages brutes et aperçus SVG. `ardoise.exe --diagnostic --simulated` le déroule sans fenêtre contre la tablette simulée.
- Procédure H1 dans `docs/hardware-tests.md`.

### Ajouté (jalon M0 — fondations)

- Espace de travail Cargo : crates `ardoise-*` (vides sauf `ardoise-core` et `ardoise-ble`), application Tauri 2 dans `app/`, automatisation dans `xtask/`.
- `cargo xtask check` (vérification complète, ligne finale `CHECK OK`) et `cargo xtask selftest` (taille de l'exe + autotest).
- Réglages portables `ardoise.toml` à côté de l'exe (repli sur `%APPDATA%\Ardoise`), une section par ordinateur, écriture atomique qui préserve les sections des autres PC.
- Interface Svelte 5 en français et en anglais (langue de Windows par défaut), thèmes clair/sombre/système.
- Assistant de premier lancement : choix du dossier de bibliothèque, détection d'une bibliothèque existante (`_ardoise/`).
- `ardoise.exe --self-test` : vérifie réglages, WebView2, interface, traductions et Bluetooth LE ; compte rendu affiché et enregistré à côté des réglages.
- Exécutable autonome : C runtime lié statiquement, interface embarquée (option `custom-protocol`, imposée par une garde pour toute compilation release), données WebView2 dans `%LOCALAPPDATA%\Ardoise\webview`.
- Écriture atomique robuste dans un dossier synchronisé : fichiers temporaires `.part` (ignorés par Nextcloud), nouvelles tentatives si un autre programme verrouille brièvement le fichier.
- Backend simulé dans le navigateur (`npm run dev`) pour développer et tester l'interface sans l'application.
- Licence GPL-3.0-or-later ; procédure de test H0 dans `docs/hardware-tests.md`.
