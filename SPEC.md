# Ardoise — cahier des charges

> « Ardoise » est un nom de travail (*slate* en anglais). Il peut changer avant la publication.
> Ce document décrit **quoi** construire. L'ordre de construction et les critères de fin de chaque étape sont dans [ROADMAP.md](ROADMAP.md).
> Les documents de recherche qui justifient les choix techniques sont dans [docs/research/](docs/research/).

## 1. Objectif

Remplacer complètement Wacom Inkspace par une application **portable** pour Windows, qui :

- récupère en Bluetooth les pages écrites sur une **Wacom Bamboo Slate** (modèle testé : Slate Large, A4) ;
- permet d'écrire **en direct** à l'écran avec la tablette, y compris au vidéoprojecteur en classe ;
- range les notes dans un **dossier choisi par l'utilisateur** (en pratique un dossier synchronisé par Nextcloud), pour les retrouver sur tous ses ordinateurs ;
- permet de modifier, découper, assembler, tourner, rejouer, exporter les notes (images, PDF, SVG, vidéos) ;
- reprend toutes les fonctions utiles d'Inkspace et en ajoute d'autres (voir §4).

## 2. Contraintes

| Sujet | Contrainte |
|---|---|
| Système | Windows 11 x64 (Windows 10 si ça ne coûte rien de plus). |
| Droits | **Aucun droit administrateur** : pas d'installateur, pas de pilote, pas de service, rien dans `Program Files` ni dans `HKLM`. |
| Portabilité | Un seul `.exe` à copier. Il doit pouvoir être lancé depuis n'importe quel dossier, y compris le dossier Nextcloud. |
| Bluetooth | Bluetooth LE intégré ou dongle USB (Bluetooth 4.0 ou plus). Aucun appairage Windows préalable ne doit être requis (à confirmer par les tests). |
| Code non signé | L'exécutable n'est pas signé : un avertissement SmartScreen au premier lancement est acceptable, il doit être documenté. |
| Langues | Interface en **français et en anglais**, langue choisie automatiquement d'après Windows, modifiable dans les réglages. |
| Technologie | Cœur en **Rust**, interface **Tauri 2** (page web affichée par WebView2, présent dans Windows 11). |
| Tablettes | Bamboo Slate (A4 et A5) visées et testées sur la Slate Large. Spark, Folio et Intuos Pro Paper : prises en charge si le protocole le permet sans effort particulier, marquées « non testées ». |
| Licence | Compatible avec les sources utilisées (Tuhi est sous GPL) : voir §9. |
| Publication | Sur GitHub, seulement quand l'application est testée et finalisée, et avec l'accord de l'utilisateur. |

## 3. Données et synchronisation

### 3.1 Réglages portables

- Fichier `ardoise.toml` **à côté de l'exécutable**. Si ce dossier n'est pas accessible en écriture, repli sur `%APPDATA%\Ardoise\ardoise.toml`.
- Les réglages qui dépendent de l'ordinateur sont rangés par nom de machine, car le chemin du dossier Nextcloud diffère d'un PC à l'autre :
  ```toml
  [machines."PC-MAISON"]
  library = 'C:\Users\prof\Nextcloud\Ardoise'
  [machines."PC-BOULOT"]
  library = 'D:\Nextcloud\Ardoise'
  ```
- Au premier lancement sur un PC inconnu : assistant qui demande le dossier de bibliothèque (ou en détecte une existante).
- Les caches (miniatures, index de recherche, données WebView2) restent **locaux à chaque PC** et ne sont jamais écrits dans la bibliothèque.

### 3.2 Bibliothèque

- Un dossier ordinaire, lisible sans l'application. **Une note = un fichier `.ardoise`**, rangé dans des sous-dossiers qui servent de classeurs.
- Nom de fichier lisible : `2026-10-24 Titre de la note.ardoise`. Renommer une note renomme le fichier.
- Dossier `_ardoise/` à la racine de la bibliothèque, volontairement **non caché** : avec ses réglages par défaut, le client Nextcloud ne synchronise pas les dossiers cachés (nom commençant par un point). Une bibliothèque qui a encore l'ancien dossier `.ardoise/` est migrée automatiquement à l'ouverture. Contenu :
  - `device.json` : identifiant d'enregistrement de la tablette, **partagé par tous les PC** (enregistrer la tablette une seule fois suffit) ;
  - `diagnostics/` : journaux Bluetooth détaillés (§4.8) ;
  - `trash/` : corbeille (notes supprimées, vidée après 30 jours ou à la demande).
- Exports automatiques optionnels (PDF ou PNG) dans un sous-dossier configurable, pour consulter les notes depuis le téléphone via l'appli Nextcloud.

### 3.3 Format de fichier `.ardoise`

- Archive ZIP contenant :
  - `note.json` : version du format, identifiant unique, titre, dates, taille et orientation de la page, fond (couleur, texture, modèle), étiquettes, liste des calques ;
  - `strokes.bin` : traits en binaire compact (x, y, pression, horodatage par point ; outil, couleur, épaisseur, calque par trait) — format décrit dans `docs/file-format.md` ;
  - `original.bin` : copie intacte des traits reçus de la tablette, pour « revenir à l'original » ;
  - `thumb.png` : miniature ;
  - optionnel : `audio.opus` (narration), `background.*` (image ou PDF de fond), `text.json` (texte reconnu).
- Le format est **versionné** ; une version plus récente de l'application sait lire toutes les versions précédentes.

### 3.4 Sûreté face à Nextcloud

- Écriture **atomique** : fichier temporaire dans le même dossier, puis renommage. Jamais de fichier à moitié écrit.
- Aucun fichier verrou ni base de données partagée dans la bibliothèque (source classique de conflits de synchro).
- Surveillance du dossier : une note modifiée sur un autre PC se met à jour à l'écran.
- Les copies en conflit créées par Nextcloud (`… (conflicted copy …)`) sont repérées et proposées à la comparaison.
- Une note ouverte en modification sur deux PC à la fois : la dernière sauvegarde ne doit pas écraser l'autre sans prévenir (comparaison de la date de modification avant écriture).

## 4. Fonctionnalités

Priorités : **P1** = indispensable pour remplacer Inkspace, **P2** = proposé et accepté, **P3** = si le temps le permet.

### 4.1 Tablette (P1)

- Recherche des tablettes à proximité et **enregistrement** guidé (appui long sur le bouton, animation explicative). L'enregistrement rend la tablette inutilisable par l'appli du téléphone et par Inkspace : l'assistant le dit avant.
- Connexion automatique quand la tablette enregistrée est à portée.
- **Synchronisation** des pages stockées : à la connexion (automatique, désactivable) ou à la demande ; chaque page devient une note ; option « effacer de la tablette après import réussi » (activée par défaut, l'effacement n'a lieu qu'après écriture vérifiée du fichier).
- Informations : nom, batterie et charge, version du firmware, nombre de pages en mémoire, modèle et taille.
- Réglages : renommer la tablette, mettre à l'heure, orientation (droitier / gaucher), oublier la tablette.

### 4.2 Mode direct (P1)

- La tablette envoie le tracé en temps réel ; il s'affiche avec l'outil, la couleur et la texture choisis.
- Latence cible : affichage moins de 30 ms après réception du point.
- Chaque session directe est enregistrée comme une note. Le bouton de la tablette crée une nouvelle page (ou un nouveau calque, au choix).
- **Classe et vidéoprojecteur** (bureau étendu) :
  - fenêtre de présentation **plein écran sur l'écran choisi** (le projecteur), sans aucun élément d'interface ;
  - fenêtre de commande sur l'écran de l'ordinateur : outils, couleurs, pages, aperçu de ce qui est projeté ;
  - zoom et déplacement de la zone projetée ; « tableau noir » (fond ardoise, craie) ;
  - pointeur laser (le stylet en survol montre un point lumineux sans écrire) ;
  - effacement rapide de la page, page suivante / précédente au clavier ou par le bouton de la tablette.
- **Reprendre le direct sur une feuille** (ajout demandé par l'utilisateur le 2026-11-04, `docs/questions.md` Q9) : depuis le menu d'une note ou l'éditeur, le mode direct repart sur cette note. La nouvelle écriture s'ajoute à la note elle-même, qui garde son fond, ses calques et sa narration, et son état d'origine reste récupérable (« Revenir à l'original »). Si elle a changé ailleurs entre-temps, rien n'est écrasé : une nouvelle note est créée à côté, et Ardoise le dit. La page suivante crée une note neuve.
- **Retoucher pendant le direct** (même ajout) : dans la fenêtre de commande, un outil Sélection (lasso ou clic) permet de supprimer des traits ou de changer leur couleur, sans passer par l'éditeur et pendant que la tablette continue d'écrire. Le vidéoprojecteur et les écrans du partage montrent aussitôt le résultat, jamais la sélection. Ctrl+Z annule ces actions et l'effacement de la page, pas l'écriture de la tablette.

### 4.3 Bibliothèque (P1)

- Grille de miniatures, tri par date ou titre, classeurs (sous-dossiers), déplacement par glisser-déposer.
- Étiquettes de couleur, favoris, recherche par titre et étiquettes (P1), par texte manuscrit reconnu (P2, §4.7).
- Sélection multiple : exporter, déplacer, supprimer, fusionner.
- Corbeille avec restauration.
- **Import des notes Inkspace** existantes sur l'ordinateur (P1, cet ordinateur en a 22) : détection automatique de l'installation d'Inkspace, import sans rien modifier côté Inkspace (détails et limites en §6.3).

### 4.4 Édition d'une note (P1 sauf mention)

- **Outils d'encre** sensibles à la pression : stylo à bille, stylo plume, feutre, crayon (texture granuleuse), pinceau, surligneur (P2), craie (P2).
- **Couleurs** : palette (dont les couleurs d'Inkspace) + couleurs personnalisées mémorisées ; **épaisseur** réglable.
- **Fonds** : blanc, crème, kraft, ardoise noire ; modèles ligné, Seyès, quadrillé 5 mm, millimétré, pointillé, partition (P3) ; texture de papier.
- Le style peut s'appliquer à toute la note ou à une sélection de traits.
- **Calques** : la tablette peut en créer (bouton), l'utilisateur peut les afficher/masquer, fusionner, réordonner.
- **Sélection au lasso** : déplacer, redimensionner, recolorer, changer d'outil, supprimer, copier.
- **Gomme** : trait entier ou partielle.
- **Annuler / rétablir** sans limite pendant la session.
- **Rotation** de la page par quart de tour ; **recadrage** (manuel ou automatique autour du contenu).
- **Découper une page** : à un instant choisi sur la ligne du temps (tout ce qui a été écrit après passe dans une nouvelle note), ou selon une zone tracée.
- **Assembler des pages** : superposer plusieurs notes sur une seule page, ou les mettre bout à bout (page longue).
- **Redresser les formes** : transformer un tracé à main levée en droite, cercle, rectangle, flèche (P2).
- **Fond image ou PDF** importé, à annoter (P2).
- Zone de texte tapé au clavier (P3).
- **Revenir à l'original** de la tablette (P2).

### 4.5 Relecture et vidéo

- Lecture animée du tracé avec ligne du temps, pause, vitesse de 0,25× à 16× (P1).
- Option « supprimer les temps morts » (raccourcit les pauses longues) (P1).
- **Exports animés** (P1) : MP4 (H.264), GIF animé, WebP animé, WebM. Réglages : résolution (720p, 1080p, 4K, ou taille de la page), images par seconde, durée cible ou vitesse, fond inclus ou transparent (GIF/WebP), image finale figée pendant N secondes.
- Le MP4 utilise l'encodeur intégré à Windows (Media Foundation : vidéo H.264, son AAC). S'il manque (éditions « N » de Windows sans le Media Feature Pack), l'application propose de télécharger OpenH264 chez Cisco, en affichant sa licence, et le range dans son dossier local.
- **Narration au micro** (P1) : choix du micro parmi ceux connectés, niveau d'entrée visible, enregistrement pendant une relecture ou pendant le mode direct ; le son est intégré aux exports MP4 et WebM.

### 4.6 Export, import, partage

- **Images** : PNG (fond transparent possible), JPG ; résolution au choix (P1).
- **SVG** vectoriel, épaisseur variable fidèle à la pression, ouvrable dans Inkscape (P1).
- **PDF** vectoriel, une ou plusieurs notes, format A4 ou taille réelle de la page (P1).
- **Copier** dans le presse-papiers pour coller directement dans Word ou OneNote (P1).
- PSD avec un calque par calque de la note (P3, Inkspace le proposait).
- Formats d'encre Wacom WILL / Universal Ink Model en lecture et écriture (P3).
- Glisser une note de l'application vers l'Explorateur ou un autre logiciel (P2).

### 4.7 Fonctions supplémentaires (P2)

- **Reconnaissance d'écriture** hors ligne en français et en anglais : recherche dans les notes manuscrites, export en texte brut.
- Thème clair / sombre / système, raccourcis clavier.

La tablette comme stylet Windows (écrire dans n'importe quel logiciel en mode direct) a été abandonnée à la demande de l'utilisateur le 2026-10-25.

### 4.8 Diagnostics et tests à distance (P1)

La tablette est au travail alors que le développement se fait à la maison. L'application doit donc pouvoir produire elle-même tout ce qu'il faut pour diagnostiquer un problème :

- journal détaillé de chaque échange Bluetooth (horodatage, sens, octets bruts, interprétation) dans `<bibliothèque>/_ardoise/diagnostics/AAAA-MM-JJ_HHMMSS.jsonl` — donc récupéré automatiquement à la maison par Nextcloud ;
- **outil de test guidé** : une suite d'étapes (rechercher, enregistrer, lire les infos, télécharger les pages, mode direct pendant 30 s, dessins de référence à reproduire) qui produit un rapport ;
- ces captures servent ensuite de **données de test** rejouées par le simulateur de tablette.

### 4.9 Pilotage par un assistant (MCP)

Ajouté à la demande de l'utilisateur le 2026-10-28. Ardoise peut être commandée par un assistant comme Claude Code ou Claude Desktop, grâce au protocole MCP (Model Context Protocol). Ardoise ne contient aucune IA et reste utilisable hors ligne : c'est l'assistant de l'utilisateur qui analyse les notes, par exemple pour reconnaître les schémas et mettre une note au propre.

- `ardoise.exe --mcp` : serveur MCP local (entrée et sortie standard, aucun port réseau), déclaré une fois dans l'assistant. Il lit et écrit la bibliothèque avec le même code que l'application (écritures atomiques, contrôle des modifications concurrentes), même quand l'application est fermée.
- **Outils de lecture** : lister et chercher les notes (titre, étiquettes, classeur, dates, texte reconnu) ; lire une note (informations, traits) ; son texte reconnu avec la position des lignes ; son rendu en image, page entière ou zone, pour que l'assistant voie les schémas.
- **Outils d'écriture** : créer une note, renommer, étiqueter, ranger, assembler, découper, redresser les formes, exporter dans tous les formats, enregistrer un texte corrigé qui améliore la recherche d'Ardoise. Aucune suppression définitive : une note supprimée va à la corbeille.
- **Invites prêtes à l'emploi** : mettre une note au propre (Markdown, Word ou LaTeX), titrer et étiqueter les notes sans titre, résumer une séance pour le cahier de textes.
- **Passerelle vers l'application ouverte** : ouvrir une note, lancer la présentation, changer de page, proposer une version propre à la fin du mode direct.
- **Vie privée** : ce que l'assistant lit part chez son fournisseur, selon les réglages du compte de l'utilisateur ; le guide le rappelle (noms d'élèves). Les outils d'écriture sont séparés des outils de lecture, pour qu'on puisse limiter l'assistant à la lecture.

### 4.10 Partage de la feuille sur le réseau local (P1)

Ajouté à la demande de l'utilisateur le 2026-11-02. Un autre ordinateur du même réseau, par exemple le PC du bureau relié au vidéoprojecteur, affiche dans un simple navigateur, sans rien installer, ce que le PC d'Ardoise choisit de montrer. Ce PC est un ordinateur personnel dont l'utilisateur est administrateur.

- **C'est toujours le PC d'Ardoise qui choisit.** Le partage montre la feuille du mode direct pendant le mode direct, avec la vue de la présentation (zoom, déplacement, tableau noir, pointeur laser) et les annotations écrites par-dessus la présentation (ajout demandé par l'utilisateur le 2026-11-02). Sinon, il montre la page de la note ouverte, pendant qu'on la consulte ou la modifie, mise à jour à chaque modification. Sans l'un ni l'autre, il montre un écran d'attente. Une pause fige l'image partagée.
- **Rien d'autre n'est accessible** : ni la bibliothèque, ni la liste des notes, ni une autre note ou page. L'autre ordinateur ne peut ni modifier ni choisir le contenu. Il peut passer en plein écran et régler son propre affichage, sans effet sur le PC d'Ardoise : page entière, pleine largeur, zoom et déplacement dans la page ; un bouton le ramène à la vue choisie par le PC (ajout demandé par l'utilisateur le 2026-11-03, `docs/questions.md` Q8).
- **Lancé à la main** : un bouton « Partager sur le réseau » dans la fenêtre de commande du mode direct et dans l'éditeur. Ardoise affiche l'adresse à taper (`http://<adresse>:<port>`), un QR code, un code de 6 caractères demandé par la page (nouveau code à chaque partage, tentatives limitées) et le nombre d'écrans connectés. « Arrêter le partage » déconnecte tout le monde ; fermer Ardoise aussi.
- **Réseau** : le port n'est ouvert que pendant un partage. Il est réglable dans les réglages d'Ardoise (8765 par défaut). S'il est déjà pris par un autre programme, Ardoise prend de lui-même le premier port libre qui suit, le dit dans le panneau du partage et le retient pour ce PC, pour que l'adresse reste la même d'un partage à l'autre (décision de l'utilisateur du 2026-11-02, `docs/questions.md` Q7). L'autorisation du pare-feu suit le programme, pas le port : changer de port ne la redemande pas. Il n'y a rien sur internet ni aucun relais. La première fois, Windows demande l'autorisation du pare-feu, et le guide explique quoi répondre (« réseau privé »). Plusieurs écrans peuvent se connecter à la fois.
- **Fluidité** : sur le réseau local, un trait apparaît sur l'autre écran moins de 150 ms après avoir été tracé, et le mode direct y reste fluide. La page se reconnecte d'elle-même après une coupure.

### 4.11 Hors périmètre de la version 1

- Le cloud Wacom et la collaboration en temps réel d'Inkspace.
- macOS et Linux (l'architecture ne doit pas les empêcher plus tard).
- Applications mobiles.

## 5. Interface

- Fenêtre principale : barre latérale (classeurs, étiquettes, tablette avec état de connexion et batterie), grille de notes, éditeur plein espace à l'ouverture d'une note.
- Barre d'outils de l'éditeur : outils, couleurs, épaisseur, fond, calques, relecture, export.
- Tout est faisable à la souris ; les actions fréquentes ont un raccourci clavier.
- Retour visuel pour toute opération longue (synchro, export vidéo) avec annulation possible.
- Messages d'erreur compréhensibles et actionnables (« Bluetooth désactivé : l'activer dans les paramètres Windows »), jamais un code brut seul.

## 6. Architecture technique

Sources : [protocol.md](docs/research/protocol.md), [windows-platform.md](docs/research/windows-platform.md), [media-stack.md](docs/research/media-stack.md), [inkspace-format.md](docs/research/inkspace-format.md), [inkspace-features.md](docs/research/inkspace-features.md).

### 6.1 Découpage du code

```
crates/
  ardoise-proto     protocole des tablettes Wacom, sans aucune entrée/sortie : messages, enregistrement,
                    décodage des pages stockées et des paquets du mode direct
  ardoise-device    gestion de la tablette : connexion, synchro, mode direct, journal de diagnostic ;
                    dépend d'un « transport » abstrait
  ardoise-ble       transport Bluetooth LE Windows
  ardoise-sim       tablette simulée + rejeu de captures (même interface que ardoise-ble)
  ardoise-ink       modèle de trait et géométrie (contour à épaisseur variable selon la pression) ;
                    compilé aussi en WebAssembly pour l'interface
  ardoise-core      notes, format .ardoise, bibliothèque, réglages, opérations (découpe, assemblage,
                    rotation, annuler/rétablir)
  ardoise-render    rendu image (tiny-skia), fonds, modèles, textures ; export PNG/JPG/SVG/PDF
  ardoise-media     relecture image par image, MP4/GIF/WebP/WebM, capture micro, encodage audio
  ardoise-inkspace  import des notes Inkspace
  ardoise-hwr       reconnaissance d'écriture (P2)
  ardoise-mcp       serveur MCP : outils et invites pour un assistant (§4.9)
  ardoise-share     partage de la feuille sur le réseau local : petit serveur HTTP en lecture seule
                    (code, flux d'évènements) (§4.10)
app/
  src-tauri/        ardoise.exe : commandes, fenêtres, options en ligne de commande
                    (--self-test, --diagnostic, --import-inkspace, --mcp…)
  ui/               interface Svelte 5 + TypeScript + Vite
xtask/              cargo xtask check | release | golden
testdata/           données de test publiables (synthétiques ou anonymisées)
testdata/private/   copies de notes personnelles pour les tests locaux — ignoré par git, jamais publié
```

### 6.2 Choix techniques

| Sujet | Choix | Raison |
|---|---|---|
| Protocole | Réécriture « en salle blanche » à partir de `protocol.md` et de nos propres captures, pas une traduction ligne à ligne du code de Tuhi. | Code propre en Rust et licence maîtrisée. Tuhi reste cité comme source. |
| Bluetooth | `btleplug`, caché derrière notre propre interface de transport. Plan B : appels WinRT directs (crate `windows`, `GattSession.MaintainConnection`) si la connexion ne tient pas pendant l'appui sur le bouton. | Pas d'appairage Windows ni de droits admin nécessaires ; btleplug ne règle pas `MaintainConnection`. |
| Unités internes | Unité de la tablette : 10 µm. Slate Large : 29 700 × 21 600 ; Slate A5 : 21 600 × 14 800. Pression normalisée de 0 à 1 (tablette : 0 à 2047). Temps en ms par point. | Aucune perte de précision ; conversions seulement à l'affichage et à l'export. |
| Un seul dessin des traits | Le contour de chaque trait est calculé par `ardoise-ink`, en natif pour les exports et en WebAssembly pour l'écran. | L'écran, le PNG, le SVG, le PDF et la vidéo montrent exactement la même chose. |
| Rust → interface | `Channel` Tauri avec envoi groupé à chaque image (pas un évènement par point). | Débit du mode direct (un point toutes les 5 ms). |
| Fenêtres | Écrans listés par `available_monitors` (coordonnées physiques) ; fenêtres créées depuis des commandes asynchrones. | Projecteur en bureau étendu ; évite un blocage connu de Tauri sous Windows. |
| Exécutable | CRT lié statiquement ; interface embarquée dans l'exe ; dossier de données WebView2 fixé explicitement dans `%LOCALAPPDATA%\Ardoise\webview`. | Fonctionne sur un PC vierge ; aucun cache dans Nextcloud. |
| Réseau | Aucun port en écoute, sauf pendant un partage sur le réseau local lancé à la main (§4.10) : serveur HTTP en lecture seule, arrêté avec le partage. Seule connexion sortante : téléchargement d'OpenH264 sur demande. | Une alerte du pare-feu demande des droits admin : seul le partage en a besoin, sur un PC personnel dont l'utilisateur est administrateur (décision de l'utilisateur, 2026-11-02). |
| Vidéo MP4 | Par défaut : Media Foundation de Windows (`IMFSinkWriter` via la crate `windows`) qui produit à la fois le H.264, l'AAC et le fichier MP4, avec accélération matérielle si disponible. Secours si Media Foundation manque : H.264 par OpenH264 2.6.0 chargé à l'exécution (`openh264` + `libloading`), téléchargé chez Cisco (empreinte SHA-256 vérifiée), multiplexé par `shiguredo_mp4`, son en Opus (lisible dans les navigateurs et VLC, pas dans le Lecteur multimédia de Windows). | Rien à télécharger dans le cas courant. Pour le secours, conditions de Cisco : téléchargement par l'utilisateur, désactivable et supprimable, mention « OpenH264 Video Codec provided by Cisco Systems, Inc. », licence affichée. |
| Vidéo WebM | AV1 par `rav1e` + `webm` (son Opus). | libvpx impossible à compiler ici. AV1 est lent sans assembleur : `nasm` portable sera nécessaire sur la machine de compilation (pas chez l'utilisateur). |
| GIF / WebP | `gif` + quantification des couleurs ; `webp-animation`. | `gifski` exclu (licence AGPL). |
| Micro | `cpal` (WASAPI, liste et choix du micro) + `rubato` (rééchantillonnage) ; Opus par `ropus` (Rust pur). | Pas de droits admin. Si Windows bloque le micro pour les applications de bureau, on reçoit du silence : le détecter et expliquer le réglage à changer. |
| Écriture (P2) | `InkRecognizerContainer` de Windows (fonctionne hors application du Store). | Français présent sur ce PC ; anglais seulement si le module d'écriture anglais est installé dans Windows. |

### 6.3 Import Inkspace

- Les notes sont dans la base LevelDB `LocalState\WacomCloudDb\_` ; les traits sont en protobuf de style WILL 2.0 ; 1 unité de page Inkspace = 25 unités tablette (0,25 mm).
- **Ne jamais lire la clé `profile` de `LocalState\db`** : elle contient un jeton d'accès et l'adresse e-mail. Aucune donnée de `db\` n'est nécessaire.
- Inkspace ne garde pas la pression mais l'épaisseur tracée : la pression est **estimée** à partir de l'épaisseur. Le minutage est reconstruit (début du trait + fréquence d'échantillonnage) ; pour les notes qui n'en ont pas, il est **synthétisé**, et la note est marquée comme telle.
- L'import ne modifie rien chez Inkspace ; il travaille sur une copie du dossier.

### 6.4 Risques identifiés

| Risque | Parade |
|---|---|
| Aucune capture réelle de Slate dans Tuhi : le décodage des pages n'est vérifié que sur des données Intuos Pro. | Point de contrôle H1 le plus tôt possible ; outil de diagnostic qui enregistre tout. |
| Le PC du travail peut bloquer un exe non signé (Smart App Control, AppLocker) sans possibilité de passer outre. | À vérifier dès H1 avec un exe minimal. |
| Stratégie d'entreprise bloquant le Bluetooth ou le dongle. | Idem. |
| Exe lancé depuis Nextcloud : verrouillé pendant l'exécution, la mise à jour par synchro échoue. | Documenter : mettre à jour l'exe quand il est fermé sur tous les PC. |
| Bibliothèques vidéo jeunes (`shiguredo_mp4`, `webm`, `ropus`). | Isolées dans `ardoise-media` ; solutions de repli listées dans media-stack.md. |

## 7. Qualité et tests

- Chaque module a des tests unitaires ; `cargo test --workspace` et `cargo clippy --workspace -- -D warnings` doivent passer à la fin de chaque étape.
- **Simulateur de tablette** : implémente le protocole côté tablette, rejoue des captures réelles, permet de tester synchro et mode direct sans le matériel.
- **Images de référence** : le rendu de notes de test est comparé à des images de référence (tolérance chiffrée).
- Import Inkspace vérifié en comparant le rendu de chaque note importée à l'aperçu PNG enregistré par Inkspace.
- Tests de bout en bout de l'interface pour les parcours principaux.
- Tests sur matériel réel : listés dans `docs/hardware-tests.md`, exécutés par l'utilisateur avec l'outil de test guidé (§4.8).

## 8. Performances

- Démarrage en moins de 2 s ; bibliothèque de 1 000 notes fluide (miniatures en cache local).
- Mode direct sans saccade à 60 images/s ou plus.
- Export MP4 1080p d'une relecture de 2 min en moins de 2 min sur un portable récent. Le WebM (AV1) peut être nettement plus lent : la durée estimée est affichée avant de lancer l'export.
- Exécutable de moins de 30 Mo (hors encodeur OpenH264 téléchargé).

## 9. Licence et vie privée

- Licence du projet : **GPL-3.0-or-later**. Elle est compatible avec Tuhi (GPL-2.0-or-later), avec `cpal` (Apache-2.0, incompatible avec la GPL v2 seule), avec `webm` (MPL-2.0) et avec `imagequant` (GPL-3.0+).
- Interdit : toute dépendance AGPL ou incompatible avec la GPL v3 (ex. `gifski`, `fdk-aac`). Vérification automatique des licences avant la publication.
- OpenH264 n'est jamais distribué avec l'application : chaque utilisateur le télécharge chez Cisco.
- Fichier `THIRD-PARTY-NOTICES` et écran « À propos » listant les composants et leurs licences ; Tuhi y est crédité.
- **Vie privée** : les notes personnelles (y compris celles d'Inkspace) ne sont jamais ajoutées au dépôt git ni publiées ; les tests qui en ont besoin les lisent dans `testdata/private/`, ignoré par git.
