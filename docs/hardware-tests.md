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

---

## H1 — La tablette parle-t-elle à Ardoise ?

**Durée** : 10 minutes. **Tablette** : chargée. **Dongle Bluetooth** : branché. **Téléphone** : Bluetooth **coupé** pendant tout le test (sinon l'application du téléphone peut se connecter à la tablette à la place d'Ardoise et récupérer les pages).

Ce test enregistre la tablette auprès d'Ardoise, lit ses informations, télécharge trois pages de référence, essaie le mode direct pendant 30 secondes puis vérifie la reconnexion. Chaque octet échangé est enregistré dans la bibliothèque, dans le dossier caché `.ardoise\diagnostics\` : tout revient à la maison par Nextcloud.

> **Important** : une tablette ne reste enregistrée qu'auprès d'une seule application. Après ce test, l'application Wacom du téléphone (et Inkspace) ne verront plus la tablette. Pour revenir en arrière, il suffira de la réenregistrer dans l'application du téléphone (bouton maintenu 6 secondes).

### À la maison, avant

Remplacer `ardoise.exe` dans le dossier Nextcloud par la nouvelle version (Ardoise doit être fermé sur tous les PC pendant la copie).

### Au travail

1. Couper le Bluetooth du téléphone. Brancher le dongle. Vérifier que la synchronisation Nextcloud est active.
2. Lancer `ardoise.exe` (double-clic). Cliquer sur **Tester la tablette** (en haut à droite).
3. Laisser cochée la case **Effacer les pages de la tablette une fois enregistrées** (les pages sont d'abord enregistrées et vérifiées, ensuite seulement effacées). Ne pas cocher **Simulation**. Cliquer sur **Démarrer le test**.
   > Les pages **déjà présentes** sur la tablette sont aussi téléchargées puis effacées. Elles ne sont pas perdues (fichiers `page-N.bin` dans le dossier des diagnostics, qu'Ardoise saura importer plus tard), mais elles n'apparaîtront pas encore comme des notes. Si la tablette contient des pages importantes, les récupérer d'abord avec l'application du téléphone.
4. Suivre les consignes à l'écran. Après chaque action, cliquer sur **C'est fait, continuer**. En cas de blocage sur une étape, **Passer cette étape** permet d'aller à la suivante ; **Arrêter le test** termine tout de suite (le journal est quand même enregistré).

| Étape à l'écran | Ce qu'il faut faire |
|---|---|
| Adaptateur Bluetooth | Rien : Ardoise vérifie que le dongle est vu et allumé. |
| Pages de référence | Sur une feuille posée sur la tablette, dessiner les **3 pages de référence** ci-dessous, en appuyant brièvement sur le bouton de la tablette **après chaque page** pour l'enregistrer. |
| Recherche de la tablette | La première fois : **maintenir le bouton enfoncé environ 6 secondes**, jusqu'à ce que le voyant **clignote en bleu**. Les fois suivantes : un appui bref suffit. |
| Connexion | Quand Ardoise le demande, **appuyer brièvement sur le bouton** pour confirmer l'enregistrement (quelques secondes pour le faire). |
| Informations | Rien : nom, version, batterie, taille et nombre de pages sont lus. |
| Téléchargement des pages | Rien : les pages sont téléchargées, vérifiées, enregistrées, puis effacées de la tablette. |
| Mode direct (30 s) | **Dessiner sans s'arrêter pendant 30 secondes** : traits, cercles, quelques mots, en appuyant plus ou moins fort. |
| Reconnexion | Si Ardoise le demande, appuyer brièvement sur le bouton. |

**Les 3 pages de référence** (une par feuille, ou sur la même feuille en appuyant sur le bouton entre deux) :
1. un carré et un cercle, aussi grands que possible ;
2. écrire `Ardoise 2026` en écriture normale ;
3. trois traits horizontaux, le premier très léger, le deuxième normal, le troisième très appuyé.

5. À la fin, Ardoise affiche le résumé (« N réussie(s), N échec(s), N ignorée(s) ») et l'emplacement du journal. Fermer Ardoise.

### Ce qui revient par Nextcloud

Dans `<bibliothèque>\.ardoise\diagnostics\` :
- `AAAA-MM-JJ_HHMMSS.jsonl` : le journal complet des échanges Bluetooth ;
- `AAAA-MM-JJ_HHMMSS.txt` : le rapport lisible ;
- `AAAA-MM-JJ_HHMMSS\page-1.bin`, `page-1.svg`… : chaque page brute reçue de la tablette et son aperçu (ouvrable dans un navigateur).

Et dans `<bibliothèque>\.ardoise\device.json` : l'enregistrement de la tablette, partagé par tous les PC (pas besoin de réenregistrer la tablette sur un autre ordinateur).

**À me signaler** : seulement ce que le journal ne peut pas contenir — la couleur du voyant à un moment surprenant, un message d'erreur de Windows (photo), une étape où la tablette n'a visiblement pas réagi.
