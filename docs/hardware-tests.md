# Tests sur le matériel réel

Ces tests se font **au travail**, là où se trouvent la tablette et le dongle Bluetooth. Chaque test produit un compte rendu qui revient tout seul à la maison par Nextcloud : il n'y a rien à recopier à la main, sauf indication contraire.

---

## H0 — Ardoise démarre-t-il sur le PC du travail ?

**Durée** : 5 minutes. **Tablette** : pas nécessaire. **Dongle Bluetooth** : branché.

### À la maison, avant

Copier `ardoise.exe` (dossier `target\release\` du projet) dans un dossier synchronisé par Nextcloud, par exemple `Nextcloud\Ardoise\Programme\`.

### Au travail

1. Brancher le dongle Bluetooth et attendre que Windows l'ait reconnu (environ 30 secondes la première fois).
2. Ouvrir dans l'Explorateur le dossier Nextcloud qui contient `ardoise.exe`. Attendre la coche verte de synchronisation sur le fichier.
3. **Double-cliquer sur `ardoise.exe`.** Trois cas possibles :
   - **« Windows a protégé votre ordinateur »** (écran bleu SmartScreen) : cliquer sur *Informations complémentaires*, puis *Exécuter quand même*. C'est normal pour un programme non signé ; le noter.
   - **Message de blocage sans bouton pour continuer** (« bloqué par votre administrateur », « Smart App Control », « Cette application a été bloquée ») : **prendre une photo du message** et s'arrêter là. Il faudra une autre solution pour le PC du travail.
   - **La fenêtre Ardoise s'ouvre** : l'assistant demande un dossier pour la bibliothèque. Choisir un dossier Nextcloud, par exemple `Nextcloud\Ardoise\Notes`, puis *Continuer*. Fermer Ardoise.
4. **Lancer l'autotest** :
   1. dans la barre d'adresse de l'Explorateur (là où le chemin du dossier est affiché), taper `powershell` puis Entrée : une fenêtre bleue s'ouvre dans ce dossier ;
   2. y taper exactement la ligne suivante, puis Entrée :
      ```
      .\ardoise.exe --self-test | Out-String
      ```
   3. attendre une dizaine de secondes : un compte rendu s'affiche ; sa dernière ligne est `SELF-TEST OK` ou `SELF-TEST FAILED`.
5. C'est fini. Le compte rendu est aussi enregistré dans le même dossier sous le nom `ardoise-selftest-<NOM-DU-PC>.txt` et reviendra à la maison par Nextcloud.

### Ce que l'on apprend

| Ligne du compte rendu | Signification |
|---|---|
| `settings` | Le fichier de réglages `ardoise.toml` est lisible (il est à côté de l'exe, donc partagé entre tes PC). |
| `webview` | Le moteur d'affichage de Windows (WebView2) est présent. |
| `interface` | L'interface s'est chargée, en français et en anglais. |
| `bluetooth` | `on (…)` = le dongle est utilisable. `off` = Bluetooth désactivé dans Windows. `no adapter found` = dongle non reconnu. |

**À me signaler** : seulement ce qui n'arrive pas par Nextcloud — un message de blocage (photo), ou le fait que la fenêtre ne s'est pas ouverte.
