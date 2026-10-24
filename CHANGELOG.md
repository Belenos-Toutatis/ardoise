# Journal des modifications

## Non publié

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
