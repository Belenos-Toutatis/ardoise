# Journal des modifications

## Non publié

### Ajouté (jalon M8 — retours des premiers essais)

- Stylet et écran tactile (Surface) : le dos du stylet efface avec les réglages de la gomme, quel que soit l'outil, et montre le cercle de gomme au survol ; le bouton du stylet trace un lasso de sélection ; la largeur du trait suit la pression ; les doigts sont ignorés tant que le stylet est utilisé ou proche de l'écran. Un doigt fait défiler la page, deux doigts zooment, un double toucher ajuste la page (éditeur, relecture, aperçu du mode direct) ; l'option « Dessiner au doigt » (Réglages et barre de zoom) laisse le doigt dessiner.
- Zoom « Largeur » à côté de « Ajuster » / « Page entière » (éditeur et relecture : Ctrl + 2 ; mode direct et présentation : touche W).
- Écrire par-dessus la présentation : au stylet, au doigt ou à la souris sur la fenêtre projetée, annotations temporaires (rouges, jaunes sur le tableau noir) effacées avec le dos du stylet, la touche Suppr ou le bouton « Effacer les annotations », jamais enregistrées dans la note.

### Ajouté (jalon M7 — fonctions en plus)

- Glisser une note vers l'Explorateur, bureau, Word, OneNote ou un e-mail : la poignée qui apparaît sur chaque carte de la bibliothèque dépose une image PNG de la note, nommée comme elle (une image par note quand plusieurs sont sélectionnées), rendue comme l'export PNG dans un dossier temporaire d'Ardoise nettoyé ensuite. Le glisser de cartes vers un classeur est inchangé.
- « Redresser les formes » sur la sélection de l'éditeur (bouton de l'outil Sélection, panneau de la sélection ou touche F) : les droites, cercles, ellipses, rectangles (même tournés) et flèches (en un ou plusieurs traits) tracés à main levée deviennent des formes nettes, en une seule étape annulable, avec le même outil, la même couleur et la même épaisseur ; l'écriture et les traits qui ne sont pas clairement une forme ne changent pas, et l'éditeur le dit quand rien n'est reconnu.
- « Revenir à l'original » dans l'éditeur : l'encre telle qu'elle est arrivée de la tablette, ou telle qu'elle était avant la première modification enregistrée (gardée une fois pour toutes dans la note, `original.ardoise`) ; confirmation qui précise ce qui est perdu (l'encre modifiée) et ce qui reste (titre, étiquettes, fond, narration), une seule étape annulable.
- Reconnaissance de l'écriture manuscrite hors ligne avec la reconnaissance d'écriture de Windows (français ; anglais si le module d'écriture anglais de Windows est installé, les réglages expliquent où l'ajouter) : la recherche de la bibliothèque trouve aussi les notes par les mots écrits à la main (accents et majuscules ignorés), avec un extrait qui montre pourquoi la note correspond ; lecture des notes en arrière-plan, en pause pendant le mode direct et la présentation ; texte reconnu enregistré en `.txt` ou copié depuis la fenêtre d'export (éditeur et sélection) ; le texte reconnu reste dans un cache sur chaque PC, jamais dans les notes. Note de démonstration « Cours d'optique » écrite à la main (glyphes de synthèse).
- Image ou PDF de fond sous l'écriture, pour annoter une fiche, un document ou une photo : dans l'éditeur (section « Image ou PDF de fond » du panneau Page : choix du fichier et de la page du PDF, placement entière / remplir / étirer, opacité, retirer, le tout annulable), et depuis la bibliothèque (« Depuis un PDF ou une image » : une note par page choisie du PDF, à la taille de la page, ou une note pour une image). Les pages des PDF sont rendues par Windows à 300 dpi ; l'image suit les rotations, recadrages, découpes et assemblages, et figure dans les miniatures, les exports PNG, JPG, SVG et PDF, le presse-papiers, l'export automatique et les vidéos.

### Ajouté (jalon M6 — relecture, vidéo et narration)

- Relecture animée dans l'éditeur (bouton « Relecture » ou touche R) : ligne du temps, lecture et pause (Espace), vitesses de 0,25× à 16×, « Supprimer les temps morts » (pauses ramenées à 0,5 s) ; la page reste intacte pendant la relecture. Le calcul du temps est le même que celui des exports vidéo.
- Export vidéo depuis l'éditeur (bouton « Vidéo ») en MP4, GIF, WebP ou WebM, avec une carte par format (atouts et limites) et un tableau « Quel format choisir ? » ; résolution 720p, 1080p, 4K ou taille de la page, images par seconde, vitesse ou durée cible, temps morts supprimés ou non, fond ou transparence (GIF et WebP), image finale figée ; taille, durée et temps d'export estimé affichés avant l'export ; progression et annulation ; le fichier n'apparaît qu'une fois complet.
- Moteur commun aux quatre formats : images dessinées au fil de la relecture (même rendu que l'écran et les exports PNG/PDF), son de la narration placé à l'échantillon près dans les MP4 et WebM ; estimation du temps d'export à partir des vitesses mesurées.
- Narration au micro : choix du micro (mémorisé sur chaque PC), niveau d'entrée, explication quand Windows bloque le micro pour les applications de bureau ; enregistrement pendant une relecture (rangé dans la note, à la vitesse de cette relecture) ou en mode direct (découpé page par page) ; écoute de la narration pendant la relecture ; suppression. La narration est intégrée aux exports MP4 et WebM.
- La narration est stockée dans la note elle-même (`audio.opus`, flux Ogg Opus lisible par VLC), écrite de façon atomique avec la vérification habituelle contre l'écrasement d'une version plus récente ; les versions précédentes de l'application conservent ce fichier tel quel (format décrit dans `docs/file-format.md`).
- Réglages : section « Micro » et section « Encodeur vidéo de secours (OpenH264) » : état, téléchargement chez Cisco avec progression, autoriser ou non, supprimer, mention « OpenH264 Video Codec provided by Cisco Systems, Inc. » et licence de Cisco affichées.
- MP4 fabriqué par Windows (Media Foundation : image H.264 et son AAC, avec l'encodeur de la carte graphique quand il y en a un), sans rien télécharger. Si Windows ne sait pas le faire (éditions « N »), secours par OpenH264 2.6.0 téléchargé chez Cisco à la demande (empreinte vérifiée, puis revérifiée avant chaque utilisation), avec le son en Opus. Les bibliothèques vidéo de Windows ne sont chargées qu'au premier export : Ardoise démarre aussi sur les éditions « N ».
- Parcours de bout en bout « relecture d'une note puis export vidéo en GIF ».

### Ajouté (jalon M4 — synchro, mode direct et classe)

- Tablette dans l'interface : état de connexion et batterie dans la barre latérale (avec la progression de la synchro), panneau de la tablette (nom, batterie et charge, firmware, pages en mémoire, modèle et taille), assistant d'enregistrement qui prévient d'abord que la tablette ne marchera plus avec l'appli du téléphone ni avec Inkspace, puis explique l'appui long sur le bouton par une animation.
- Connexion automatique à la tablette enregistrée dès qu'elle est à portée ; synchronisation à la connexion (désactivable) ou à la demande, avec progression et annulation ; « effacer de la tablette après import » activé par défaut, l'effacement n'ayant lieu qu'après l'écriture vérifiée de la note ; sans effacement, une page déjà importée ne l'est pas deux fois ; export automatique PDF/PNG branché sur la synchro.
- Réglages de la tablette : renommer, mettre à l'heure, droitier ou gaucher (tablette retournée), action du bouton en mode direct, oublier la tablette.
- Mode direct : le tracé s'affiche en temps réel avec l'outil, la couleur et l'épaisseur choisis ; chaque page devient une note (enregistrée au fil de l'eau) ; le bouton de la tablette crée une nouvelle page ou un nouveau calque, au choix ; latence mesurée et affichée.
- Classe et vidéoprojecteur : fenêtre de présentation plein écran sur l'écran choisi, sans aucun élément d'interface ; fenêtre de commande avec outils, couleurs, pages, aperçu de la zone projetée ; zoom et déplacement, tableau noir (fond ardoise et craie), pointeur laser (le stylet en survol montre un point sans écrire), effacement rapide, page suivante ou précédente au clavier ou par le bouton de la tablette.
- `ardoise.exe --simulated-tablet` : la tablette simulée remplace le Bluetooth (en temps réel, 3 pages en mémoire), sans jamais toucher à l'enregistrement de la vraie tablette ; parcours de bout en bout « synchro de 3 pages → 3 notes » et mode direct.
- Journaux de diagnostic écrits automatiquement dans `_ardoise/diagnostics/` quand une connexion échoue, ou quand la tablette envoie en mode direct une notification encore inconnue (pour identifier le bouton au point de contrôle H2).
- Procédure H2 dans `docs/hardware-tests.md`.

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
