# Journal des modifications

## Non publié

### Ajouté (jalon M1 — protocole, simulateur, outil de diagnostic)

- `ardoise-proto` : codage et décodage de tous les messages de la Bamboo Slate, des pages stockées (format Slate, et Intuos Pro pour les données d'exemple de Tuhi) et des paquets du mode direct, sans aucune entrée/sortie.
- Interface commune « transport » pour la vraie tablette, le simulateur et le rejeu de journaux ; horloge virtuelle pour des tests instantanés.
- `ardoise-ble` : transport Bluetooth LE Windows (btleplug) : recherche, connexion, écriture, notifications, déconnexion et reconnexion.
- `ardoise-sim` : tablette simulée (enregistrement, connexion, informations, pages, mode direct, bouton) et rejeu des journaux `.jsonl`.
- Gestionnaire de tablette : enregistrement unique partagé par tous les PC (`.ardoise/device.json`), connexion, synchronisation avec effacement seulement après enregistrement vérifié, mode direct avec reconnexion automatique.
- Journal de diagnostic `.ardoise/diagnostics/AAAA-MM-JJ_HHMMSS.jsonl` (chaque octet échangé, avec son interprétation), écrit de façon atomique.
- Test guidé de la tablette (bouton « Tester la tablette », ou `ardoise.exe --diagnostic`) : 8 étapes, rapport en français ou en anglais, pages brutes et aperçus SVG. `ardoise.exe --diagnostic --simulated` le déroule sans fenêtre contre la tablette simulée.
- Procédure H1 dans `docs/hardware-tests.md`.

### Ajouté (jalon M0 — fondations)

- Espace de travail Cargo : crates `ardoise-*` (vides sauf `ardoise-core` et `ardoise-ble`), application Tauri 2 dans `app/`, automatisation dans `xtask/`.
- `cargo xtask check` (vérification complète, ligne finale `CHECK OK`) et `cargo xtask selftest` (taille de l'exe + autotest).
- Réglages portables `ardoise.toml` à côté de l'exe (repli sur `%APPDATA%\Ardoise`), une section par ordinateur, écriture atomique qui préserve les sections des autres PC.
- Interface Svelte 5 en français et en anglais (langue de Windows par défaut), thèmes clair/sombre/système.
- Assistant de premier lancement : choix du dossier de bibliothèque, détection d'une bibliothèque existante (`.ardoise/`).
- `ardoise.exe --self-test` : vérifie réglages, WebView2, interface, traductions et Bluetooth LE ; compte rendu affiché et enregistré à côté des réglages.
- Exécutable autonome : C runtime lié statiquement, interface embarquée (option `custom-protocol`, imposée par une garde pour toute compilation release), données WebView2 dans `%LOCALAPPDATA%\Ardoise\webview`.
- Écriture atomique robuste dans un dossier synchronisé : fichiers temporaires `.part` (ignorés par Nextcloud), nouvelles tentatives si un autre programme verrouille brièvement le fichier.
- Backend simulé dans le navigateur (`npm run dev`) pour développer et tester l'interface sans l'application.
- Licence GPL-3.0-or-later ; procédure de test H0 dans `docs/hardware-tests.md`.
