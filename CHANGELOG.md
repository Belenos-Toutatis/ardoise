# Journal des modifications

## Non publié

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
