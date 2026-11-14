# Notes de version — Ardoise 0.1.0

<!-- Texte de la page de publication GitHub : copier ce qui est entre « DÉBUT RELEASE-FR » et « FIN RELEASE-EN », puis remplacer chaque lien relatif par l'adresse complète du fichier dans le dépôt, à l'étiquette v0.1.0. -->

<!-- DÉBUT RELEASE-FR -->
**Ardoise** est une application portable pour Windows qui remplace Wacom Inkspace pour la tablette **Bamboo Slate** et lit aussi la tablette **Xiaomi Mijia « petit tableau noir »** (13,5 pouces, version à mémoire). Elle récupère les pages écrites sur la tablette, laisse écrire en direct à l'écran et au vidéoprojecteur, range les notes dans le dossier de votre choix (par exemple synchronisé par Nextcloud) et permet de les modifier, rejouer, exporter et imprimer, hors connexion, sans compte, avec un seul fichier : `ardoise.exe`.

**Ce que fait cette première version**

- **Les deux tablettes** : enregistrement et synchronisation de la Bamboo Slate, ajout et import de la Xiaomi, quatre positions pour chacune.
- **Le mode direct, pensé pour la classe** : tracé en temps réel, projection plein écran, tableau noir, pointeur laser, correction au stylet de l'ordinateur sur un calque à part, papier et fiche PDF changés pendant que la tablette écrit, partage de la feuille dans un navigateur du réseau local.
- **La bibliothèque** : classeurs à ordre libre, étiquettes en couleur, favoris, recherche dans l'écriture manuscrite (reconnaissance de Windows, hors connexion), import des notes d'Inkspace.
- **L'éditeur** : sept encres sensibles à la pression, calques, formes, texte tapé, équations LaTeX (chimie comprise), signets, image ou PDF de fond, relecture animée avec narration au micro.
- **Les exports** : PNG, JPG, WebP, SVG, PDF, texte, Markdown, Word, vidéos MP4, WebM, GIF et WebP ; un classeur entier en un seul PDF avec plan et page de sommaire ; impression. Un assistant (Claude Code, Claude Desktop…) peut lire, mettre au propre et ranger vos notes.

**Ce qui a été essayé, et comment**

- **À la main** : la Bamboo Slate A4 (synchronisation, mode direct, vidéoprojecteur, partage sur le réseau, correction au stylet d'une Surface, son pendant l'écriture) ; la Xiaomi n'a eu que de premiers essais (13 novembre 2026), sa procédure d'essai n'est pas terminée.
- **Par des tests automatiques sous Windows**, sur le vrai exécutable, en français et en anglais : tout le reste, avec des tablettes simulées.
- **Jamais essayés à la main** : l'impression sur une vraie imprimante, l'export Word ouvert dans Word, le plan et les liens d'un PDF dans un lecteur PDF, l'import avec Inkspace ouvert, la fermeture de session de Windows avec un travail non enregistré. Windows 10 n'a pas été essayé.

**Pour commencer**

- Windows 11, sans droits d'administrateur. Décompressez `ardoise-0.1.0-windows-x64.zip` où vous voulez et lancez `ardoise.exe`. **L'exécutable n'est pas signé** : Windows peut afficher « Windows a protégé votre ordinateur » ; cliquez sur *Informations complémentaires*, puis *Exécuter quand même*. Un PC géré peut refuser de le lancer.
- Le guide d'utilisation (illustré, avec une section « Premiers pas ») est dans l'archive, dossier `guide\`, à ouvrir dans un navigateur ; le dépôt le contient aussi : [français](guide/guide.fr.md), [English](guide/guide.en.md).
- Le bouton **Guide** (touche `F1`) ouvre ce guide depuis Ardoise.
- Logiciel libre, licence GPL-3.0 ou ultérieure. [Les notes complètes](#ce-que-fait-cette-version) disent ce qui est vérifié, ce qui reste à essayer et les limites connues.
<!-- FIN RELEASE-FR -->

<!-- DÉBUT RELEASE-EN -->
**Ardoise** is a portable Windows application that replaces Wacom Inkspace for the **Bamboo Slate** tablet and also reads the **Xiaomi Mijia LCD blackboard** (13.5", the model with memory). It receives the pages written on the tablet, lets you write live on screen and on a projector, keeps your notes in a folder of your choice (for example synced by Nextcloud), and lets you edit, replay, export and print them, offline, with no account, from a single file: `ardoise.exe`.

- **Highlights**: live mode for the classroom (projection, blackboard, laser pointer, a layer of your own to correct on, paper and a PDF worksheet changed under the page, sharing the sheet with a browser on the local network); a library with binders in a free order, coloured tags and handwriting search; an editor with pressure-sensitive inks, shapes, typed text, LaTeX equations (chemistry included), bookmarks, replay and narration; exports to PNG, JPG, WebP, SVG, PDF, text, Markdown, Word and video, a whole binder in one PDF, printing; Inkspace import; an assistant (Claude Code, Claude Desktop…) can read and tidy your notes.
- **Tested how**: by hand with a real Bamboo Slate A4 (sync, live mode, projector, network sharing, correcting with a Surface pen, sound while writing); only first trials with the real Xiaomi tablet (13 November 2026), its test procedure is not finished; everything else by automated tests on Windows, on the real executable, in French and English, with simulated tablets. Never tried by hand: printing on a real printer, a Word export opened in Word, a PDF's outline and links in a PDF reader, an Inkspace import with Inkspace open, a Windows logoff with unsaved work. Windows 10 not tried.
- **Start**: Windows 11, no administrator rights. Unzip `ardoise-0.1.0-windows-x64.zip` anywhere and run `ardoise.exe`. **It is not signed**: Windows may show "Windows protected your PC"; click *More info*, then *Run anyway*. A managed PC may refuse to run it.
- **Guide**: the illustrated user guide (with a "Getting started" section) is in the archive, `guide\` folder, to open in a browser; it is also in the repository: [French](guide/guide.fr.md), [English](guide/guide.en.md).
- The **Guide** button (`F1` key) opens this guide from Ardoise.
- Free software, GPL-3.0-or-later licence. [The full notes](#what-this-version-does) (French, with an English section) say what is checked, what is still to be tried and the known limits.
<!-- FIN RELEASE-EN -->

Version 0.1.0, première version d'Ardoise. Date de publication : à publier. Ces notes disent, pour l'utilisateur, ce que la version fait et surtout **ce qui n'a pas été essayé**, et comment chaque chose a été vérifiée. Le détail technique est dans le [journal des modifications](../CHANGELOG.md) ; l'usage est expliqué dans le [guide](guide/guide.fr.md).

*English version below.*

## Ce que fait cette version

**Les tablettes**

- **Bamboo Slate** : enregistrement en Bluetooth LE, une seule fois, valable pour tous vos PC ; connexion automatique ; synchronisation (chaque page devient une note, effacée de la tablette seulement après écriture vérifiée de la note) ; informations de la tablette (batterie, nom, mise à l'heure) ; quatre positions de la tablette, que quatre boutons de la fenêtre du direct changent aussi pendant le cours.
- **Tablette Xiaomi Mijia « petit tableau noir »** (13,5 pouces, version à mémoire) : elle s'ajoute sans enregistrement (elle reste liée à Mi Home, qui doit être fermé) ; ses pages sont importées en notes datées de leur enregistrement sur la tablette, puis effacées d'elle (elle ne donne que sa page la plus ancienne) ; avec **Effacer de la tablette après import** désactivé, elles ne sont importées qu'après votre confirmation. Ses quatre positions (en hauteur ou en largeur, boutons d'un côté ou de l'autre) se règlent comme celles de la Slate, même pendant le direct.
- **Les pages de la Xiaomi sont protégées** : la tablette retire d'elle-même la page d'un écran non effacé dès qu'elle est téléchargée, donc Ardoise en garde d'abord une copie de secours et la réimporte si la note n'a pas pu être écrite ; enregistrer de nouveau le même écran met à jour sa note au lieu d'en ajouter une. Avec la Xiaomi, le mode direct commence par importer les pages qu'elle contient (ou propose de le faire).
- **Deux tablettes** peuvent être connues en même temps : Ardoise se connecte à celle qui est allumée, et un direct reste sur la tablette avec laquelle il a commencé. Le panneau **Tablette** a trois onglets (**Utiliser**, **Réglages**, **Tablettes**).
- **Test guidé de la tablette** et journaux de diagnostic rangés dans la bibliothèque ; les pages téléchargées pendant le test sont aussi ajoutées à la bibliothèque comme notes avant d'être effacées de la tablette.

**Le mode direct et la projection**

- Tracé en temps réel, pages et calques, tableau noir, pointeur laser, zoom ; présentation en plein écran sur le vidéoprojecteur (bureau étendu) avec des annotations par-dessus qui ne sont pas enregistrées. Pensé pour le cours : `Espace` veut toujours dire « page suivante » ; `Échap` ferme la projection quand elle est affichée, sinon demande confirmation avant de terminer le mode direct ; **Effacer la page** se rattrape pendant 8 secondes avec **Annuler l'effacement**. Le bouton de la Slate crée une nouvelle page (ou un nouveau calque, au choix).
- **Reprendre le mode direct sur une feuille** déjà enregistrée (menu de la note ou éditeur) : on continue d'écrire dans la même note, avec son image de fond, son original et sa narration, sans rien écraser si elle change ailleurs. Une feuille se reprend avec la tablette qui l'a écrite.
- **Retoucher pendant le direct**, sans quitter la fenêtre de commande ni arrêter la tablette : outil **Sélection** (lasso, clic, `V`), **Supprimer**, couleur d'un groupe de traits (la couleur du stylo ne bouge pas), déplacement et redimensionnement de la sélection (stylet, doigt ou souris), `Ctrl + Z` / `Ctrl + Y` (jamais ce que la tablette écrit). Le vidéoprojecteur et l'autre écran du réseau ne voient que le résultat.
- **Corriger pendant le direct**, sur un calque à soi : pendant qu'un élève écrit sur la tablette, vous écrivez en même temps avec le stylet de l'ordinateur (outil **Stylo**, touche `P`), en rouge par défaut, sur un calque à part (par exemple « Correction ») ; le bout gomme du stylet efface des traits entiers de ce calque ; `Ctrl + Z` annule vos actions, jamais l'écriture de la tablette. La zone **Calques** (la même dans l'éditeur) marque le calque choisi et celui de la tablette, et avertit quand un calque masqué ne serait ni projeté ni partagé. Un calque masqué n'est jamais exporté.
- **Le papier et une fiche sous la page**, pendant le direct : les onglets **Page** et **Fond** de la fenêtre de commande changent le papier de la page en cours (couleur, modèle, texture) et mettent une image ou une page de PDF sous l'écriture, sans arrêter la tablette ; le changement apparaît aussitôt au vidéoprojecteur et sur l'autre écran du réseau, et il est enregistré avec la page. Une nouvelle page garde le papier, pas la fiche.
- **Partager sur le réseau** (direct et éditeur) : un autre ordinateur du même réseau affiche dans un navigateur la feuille du direct (avec les annotations écrites par-dessus la présentation) ou la page de la note ouverte, et rien d'autre ; adresse, QR code, code de 6 caractères, pause, arrêt. L'autre écran règle lui-même son zoom. La première fois, Windows demande l'autorisation du pare-feu (droits d'administrateur) : répondre « Réseaux privés ».

**Ranger et retrouver**

- Bibliothèque dans un dossier ordinaire (compatible Nextcloud : écritures atomiques, détection des copies en conflit, mise à jour quand une note change sur un autre PC).
- **Classeurs** (créés, renommés, supprimés depuis leur menu ⋯ ; les notes d'un classeur supprimé vont à la corbeille et reviennent dans leur classeur) et sous-classeurs ; **ordre libre** des notes d'un classeur (glisser-déposer entre deux cartes, ou `Alt` + flèches) ; **étiquettes** en huit couleurs, partagées entre vos PC ; favoris ; corbeille (30 jours) ; on peut aussi **glisser des notes sur Favoris ou sur une étiquette** de la barre latérale.
- **Sélection multiple** : déplacer, étiqueter, assembler, **Tourner** plusieurs notes d'un coup, **Dupliquer** (copie « (copie) » dans le même classeur, à la même date) ; **Changer la date** d'une note.
- **Recherche** par titre, étiquette, signet, texte tapé, code d'une équation et écriture manuscrite (reconnaissance d'écriture de Windows, hors connexion).
- **Sommaire d'un classeur** : bouton **Sommaire**, qui liste au choix les sous-classeurs, les notes et leurs signets ; un clic ouvre la note à l'endroit du signet.
- Un message **Annuler** après **Mettre à la corbeille**, **Tourner**, **Dupliquer**, **Changer la date** ou la suppression d'un classeur (et `Ctrl + Z` dans la bibliothèque). Changer de bibliothèque dans les Réglages montre d'abord ce que contient le dossier et demande confirmation. Un bandeau signale les fichiers illisibles, avec la raison.
- **Nouvelle note depuis un PDF ou une image**, à annoter : pour un PDF de plusieurs pages, une miniature par page à cocher, une barre d'avancement et un bouton **Annuler** pour un long PDF.
- **Import des notes de Wacom Inkspace** depuis l'application (**Importer depuis Inkspace…** dans la barre latérale, proposé aussi par l'assistant du premier lancement) : détection automatique ou dossier au choix, classeur de destination (« Inkspace » par défaut), barre d'avancement et **Arrêter l'import**, compte rendu ; une note déjà importée est ignorée. La ligne de commande `ardoise.exe --import-inkspace` fait le même import.

**Modifier**

- Sept encres sensibles à la pression, gomme, lasso, couleurs et épaisseur (réglable par pas de 0,05 mm : curseur, boutons **−** et **+**, valeur tapée), calques, papiers, modèles de page (ligné, Seyès, quadrillé, millimétré, pointillé, portées), format et orientation de la feuille, redressement des formes (réglable), découpe par instant ou par zone, retour à l'original, copier-coller entre notes, annuler et rétablir sans limite. `Échap` agit par étapes : il abandonne le geste en cours, ferme un panneau, désélectionne, puis enregistre la note et revient à la bibliothèque.
- **Lignes, rectangles et ellipses** (aimantables au quadrillage du papier) ; rien n'est redressé sans qu'on le demande.
- **Texte tapé au clavier** (outil **Texte**, touche `T`), en Latin Modern ou en Andika, au milieu de l'écriture à la main.
- **Équations en LaTeX** (outil **Équation**, touche `M`), chimie comprise (`\ce{2H2 + O2 -> 2H2O}`) : un aperçu se dessine pendant la frappe ; l'équation est un dessin vectoriel qui garde son code, retrouvé par la recherche. MathJax est fourni avec Ardoise : aucun accès au réseau.
- **Signets** (outil **Signet**, touche `B`) : un mot ou une courte phrase posés sur la page, pour le sommaire du classeur, la recherche et le plan du PDF ; on ne les voit que dans l'éditeur.
- **Image ou PDF de fond** à annoter, la page du PDF se choisissant sur des miniatures.
- Stylet et écran tactile (dos du stylet qui gomme, bouton du stylet qui fait un lasso puis pointe qui déplace ou redimensionne la sélection, doigts, rejet de la paume).
- **Panneau de côté à onglets**, dans l'éditeur et dans la fenêtre de commande du direct : une colonne d'icônes (Calques, Page, Fond, Son, Projection, Aide) et le panneau de l'onglet ouvert, qu'un clic replie pour donner toute la largeur à la page ; l'**Aide** donne la liste des raccourcis de la fenêtre.
- **Projeter la note de l'éditeur** : l'onglet **Projection** envoie la page de la note ouverte, en plein écran, au vidéoprojecteur, mise à jour à chaque trait, avec le zoom, **Page entière** et **Largeur** ; la zone projetée suit ce que montre l'éditeur et un cadre en pointillés la montre ; la relecture est projetée aussi. Pas de tableau noir, de laser ni d'annotations sur cette projection.
- Copie de secours automatique des modifications non enregistrées, sur le PC seulement : après un plantage ou un redémarrage forcé, Ardoise propose de les récupérer (sur la note si elle n'a pas changé, sinon dans une copie). À une fermeture de session ou un arrêt de Windows avec un travail non enregistré, Windows affiche qu'Ardoise empêche l'arrêt, avec la raison.

**Relire et enregistrer le son**

- Relecture animée du tracé (0,25× à 16×, temps morts supprimables) ; narration au micro, enregistrée pendant une relecture ou, au choix, **pendant qu'on écrit dans l'éditeur** : les traits écrits prennent l'heure du son et se rejouent avec lui, à l'écran et dans la vidéo. Une seule narration par note : enregistrer à nouveau demande d'abord s'il faut **Remplacer** ou **Garder l'ancienne**.

**Exporter et imprimer**

- Images PNG, JPG et WebP ; SVG et PDF (les calques peuvent rester séparés, à afficher ou masquer dans Inkscape, Acrobat Reader ou Firefox) ; texte, Markdown et Word, avec la structure tirée de l'écriture (mêmes mots que le texte copié, les corrections de l'assistant l'emportent, un trait sous des mots les met en gras en Markdown et les souligne dans Word, option **Inclure les dessins**) ; vidéos MP4, GIF, WebP et WebM ; copie dans le presse-papiers ; glisser une note vers l'Explorateur ou Word.
- **Un classeur en un seul PDF** (clic droit sur le classeur), dans l'ordre du classeur, avec un plan (une entrée par note, par sous-classeur et par signet) et, au choix, une **page de sommaire** au début, dont chaque ligne est un lien. Les trois choix de **Dans le sommaire** (sous-classeurs, notes, signets) règlent le sommaire du classeur, le plan du PDF et cette page. Le PDF d'une seule note prend ses signets pour plan.
- **Impression** (`Ctrl + P`, ou **Imprimer…**) d'une note, d'une sélection ou d'un classeur (**Imprimer le classeur…**), à 1, 2 ou 4 notes par feuille, avec ou sans le fond du papier, en taille réelle ou ajustée à la feuille. Le texte tapé et les équations s'impriment ; les signets ne s'impriment jamais.
- **Assistant (MCP)** : Claude Code, Claude Desktop ou un autre assistant peut lire les notes (écriture, textes tapés, équations, signets), les mettre au propre sur une copie, les titrer et les ranger, et piloter Ardoise ouverte ; mode lecture seule possible. Ardoise elle-même ne contient aucune intelligence artificielle.

**Autour**

- Interface en français et en anglais, thèmes clair et sombre, application portable (un exécutable, réglages à côté), aucun droit d'administrateur, aucun compte, aucune connexion sauf le téléchargement facultatif d'OpenH264 et le partage sur le réseau local lancé à la main.
- **Clic droit** : des menus d'Ardoise là où on les attend (cartes, classeurs, étiquettes, corbeille, calques, page de l'éditeur), jamais le menu d'une page web ; les raccourcis d'un navigateur (`F5`, `Ctrl + R`, `F12`…) sont désactivés.
- **Archive de distribution** : un dossier `ardoise-<version>` avec `ardoise.exe`, `LICENSE`, `THIRD-PARTY-NOTICES`, `LISEZMOI.txt` et les guides en pages HTML (`guide\`), en français et en anglais, illustrés de captures et avec une section « Premiers pas ». Écran **À propos** (version, licence, crédits, composants).
- Le bouton **Guide** (touche `F1`) ouvre le guide d'utilisation depuis Ardoise.
- Une seule fenêtre par exemplaire d'Ardoise : un second lancement ramène la fenêtre ouverte au premier plan ; deux exemplaires dans deux dossiers fonctionnent côte à côte.
- Logiciel libre, licence GPL-3.0 ou ultérieure. Crédits : le projet Tuhi (documentation du protocole Wacom), MathJax (Apache 2.0), les polices Latin Modern et Andika, OpenH264 de Cisco (jamais fourni : téléchargé par vous, si besoin).
- Format de fichier des notes : version 1.6 (les versions précédentes sont lues ; une version plus ancienne d'Ardoise garde les textes, équations et signets d'une note sans les dessiner).

## À essayer avant de compter dessus

### Comment cette version a été vérifiée

- **À la main, avec le vrai matériel** : par l'auteur, avec une Bamboo Slate A4, une tablette Xiaomi, un vidéoprojecteur, une Surface et son stylet, un micro, en suivant les procédures de la [liste des essais sur le matériel](hardware-tests.md) (en français).
- **Par des tests automatiques sous Windows, sur l'exécutable de production** : formatage et analyse du code, tests de chaque partie, plus d'une centaine de parcours de bout en bout, en français et en anglais, qui pilotent la vraie fenêtre (avec une tablette Slate et une tablette Xiaomi simulées, un micro simulé et un vrai navigateur pour le partage sur le réseau) ; vérification des licences des composants et de l'absence de données personnelles dans l'historique du projet.
- **Une tablette simulée** remplace la vraie dans ces parcours : ce qui n'a été vu que de cette façon est dit « tablette simulée » ci-dessous.

### Ce qui a été essayé avec le vrai matériel

| Essai | Ce qu'il doit établir | Où il en est |
|---|---|---|
| **H0** — `ardoise.exe` sur le PC du travail | Windows (SmartScreen, Smart App Control, AppLocker) accepte l'exécutable non signé, depuis le dossier Nextcloud ; la clé Bluetooth est vue. | **Compte rendu non fait.** Ardoise a servi sur le PC de l'établissement de l'auteur pour les essais suivants, mais ce que Windows a affiché au premier lancement n'a pas été consigné. Sur un autre PC géré, rien n'est garanti. |
| **H1** — la tablette parle à Ardoise | L'enregistrement, la lecture des informations, le téléchargement des pages et le mode direct fonctionnent avec une vraie Slate. | **Fait le 2 novembre 2026**, avec une vraie Slate A4. Les journaux ont fait corriger le protocole (version du firmware, ordre des commandes de synchronisation, recherche de la tablette) ; les captures réelles sont rejouées par les tests, si bien que le décodage des pages a été vérifié sur des données réelles de Slate. |
| **H2** — synchronisation, mode direct, projecteur | Synchro réelle, latence ressentie, bouton de la tablette en direct, reprise après coupure, projection sur un vrai vidéoprojecteur, quatre positions. | **Fait** (signalé le 12 novembre). Le bouton de la tablette crée une nouvelle page en direct (signal observé avec la vraie tablette le 5 novembre). Retour de l'essai : une sélection ne pouvait ni se déplacer ni se redimensionner en direct ; c'est ajouté. |
| **H3** — stylet et tactile ; essai complet en classe | Dos du stylet, bouton, paume, gestes des doigts sur un PC tactile ; puis un cours entier avec la tablette. | **Fait** (signalé le 12 novembre). |
| **H4** — partage sur le réseau | Un second ordinateur affiche la feuille en direct puis une note ouverte : pare-feu, adresse tapée ou QR code, délai ressenti, reconnexion. | **Fait le 3 novembre** sur deux ordinateurs, puis au travail derrière le filtre web de l'établissement ; ces essais ont fait corriger plusieurs défauts (adresse complétée en `https://`, connexion retenue par le filtre). Pas confirmés depuis : Chrome sur Android après le correctif, et une note ouverte (non la feuille du direct) partagée sur le réseau de l'établissement. |
| **H5** — reprendre une feuille en direct, retoucher | Interrompre un direct, reprendre la feuille, écrire dessus ; supprimer, recolorer, annuler pendant que la tablette écrit. | **Fait** (signalé le 7 novembre, sans remarque). Déplacer et redimensionner une sélection au stylet ou au doigt a été ajouté après : pas encore essayé sur la Surface. |
| **H6** — corriger pendant le direct | Un élève écrit pendant que l'enseignant corrige au stylet de la Surface sur un calque à part ; export PDF à calques séparés. | **Fait** (signalé le 12 novembre). |
| **H7** — papier et fiche en direct, projection de l'éditeur, son pendant qu'on écrit | Papier et fiche PDF changés pendant que la tablette écrit, note de l'éditeur projetée, narration enregistrée en écrivant avec le micro de la Surface. | **Fait** (signalé le 12 novembre). |
| **H8** — la tablette Xiaomi | Ajouter la tablette, récupérer puis effacer ses pages, direct, positions, deux tablettes. | **En cours.** Étude du 12 novembre (le mode direct a été lancé depuis le PC avec la vraie tablette et le tracé reconstitué a été confirmé), puis premiers essais du 13 novembre 2026 : l'import d'une page enregistrée pendant qu'Ardoise est connectée, l'effacement de l'écran pendant le direct et la page qui disparaît de la tablette dès son téléchargement ont été observés, et ont fait corriger plusieurs défauts. **Pas encore essayés sur la vraie tablette** : les copies de secours des pages, la mise à jour de la note d'un écran enregistré de nouveau, le direct précédé de l'import des pages, le sens des quatre positions (déduit de leur nom, à confirmer), les boutons de position pendant le direct, la taille réelle de l'écriture importée (l'unité des coordonnées n'a pas été mesurée à la règle) et deux tablettes allumées ensemble : tout cela n'a été vérifié qu'avec une tablette simulée. |

Pour H2, H3, H6 et H7, la feuille de route dit seulement que l'auteur les a faits et signalés, sans détailler les étapes ni les mesures.

### Ce qui reste à essayer à la main

Ces points n'ont été vérifiés que par des tests automatiques, ou pas du tout :

- **Une vraie imprimante** : l'impression est testée vers « Microsoft Print to PDF » ; la fenêtre de Windows n'est pas pilotée par les tests. Cela vaut aussi pour 2 et 4 notes par feuille.
- **Word** : un export Word ouvert dans Word, et la mise en forme Markdown et Word sur une vraie écriture.
- **PDF** : le plan et les liens de la page de sommaire d'un PDF de classeur, ouverts dans un lecteur PDF (Acrobat Reader, Firefox…).
- **La Surface, au stylet et au doigt, depuis les derniers ajouts** : déplacer et redimensionner une sélection du direct ; les outils Texte, Équation, Signet, Ligne, Rectangle et Ellipse ; les menus du clic droit (qui ne doivent jamais s'ouvrir avec le bouton du stylet) ; le clavier à l'écran de Windows dans les champs de texte, d'équation et de signet ; le sélecteur **Choisir une autre couleur**.
- **Le panneau de côté** sur la Surface : le rail et ses icônes de 38 px au stylet, au doigt et à la souris, le thème sombre, la fenêtre de 800 × 560, le focus visible sur les icônes, la lecture des onglets par le Narrateur.
- **Les raccourcis du clavier** (`Ctrl + W`, `Ctrl + Q`, `Ctrl + P`, `Échap`…) : les parcours automatiques envoient les touches par programme, pas depuis un vrai clavier.
- **La reconnaissance d'écriture en anglais** (le module d'écriture anglais de Windows n'est pas installé sur le PC de développement) et la pause de la lecture de l'écriture pendant la présentation.
- **Un vrai dépôt d'une note** glissée dans l'Explorateur, Word ou OneNote.
- **L'assistant** : il a déjà servi pour de vrai à retoucher des notes (ce qui a fait corriger un défaut), mais pas la commande sous PowerShell 7, ni la présentation sur un second écran pilotée par l'assistant, ni la note du dernier mode direct retrouvée après un vrai redémarrage.
- **L'import d'Inkspace** depuis la fenêtre d'Ardoise, avec Inkspace ouvert puis fermé (il peut verrouiller ses fichiers).
- **Une vraie fermeture de session de Windows** avec une note non enregistrée, ou pendant un direct : l'écran « Ardoise empêche l'arrêt », la copie de secours écrite, sa proposition au lancement suivant.
- **Les couleurs des étiquettes avec Nextcloud sur deux PC** (changement sur l'un, vu sur l'autre ; deux changements simultanés fusionnés).
- **Windows 10**.

### Performances

Mesurées le 12 novembre 2026 sur un portable au repos (Intel Core i5-1235U, 16 Go), avant les ajouts des 13 et 14 novembre : démarrage en 0,62 s, bibliothèque de 1 000 notes affichée en 1,63 s (en cache) et défilée à 60 images/s, mode direct à 60 images/s, projection de l'éditeur à 46 ms de retard (50 ms acceptés), export MP4 1080p d'une relecture de 2 min en 15 s. **Un objectif n'est pas atteint** : le trait écrit au stylet de l'ordinateur arrive au vidéoprojecteur avec 40 ms de retard médian, pour 30 ms visés. L'exécutable reste sous la limite de 40 Mo que le projet s'est fixée (25,2 Mo mesurés le 13 novembre, avant l'ajout des polices et de MathJax). Détails : [performances](performance.md).

## Limites connues

- **Windows seulement** : Windows 11 est la seule version essayée (Windows 10 est visé, non essayé). Pas de macOS, pas de Linux, pas d'application pour téléphone.
- **Exécutable non signé** : un avertissement SmartScreen au premier lancement est normal ; un PC géré peut refuser de le lancer.
- **Tablettes** : Bamboo Slate A4 (modèle visé) et Xiaomi Mijia « petit tableau noir » de 13,5 pouces, version à mémoire (premiers essais seulement). Slate A5, Spark, Folio, Intuos Pro Paper et les autres modèles Xiaomi : non testés.
- Ces deux tablettes ne livrent que leur **page la plus ancienne** : la Bamboo Slate, sans l'option « Effacer de la tablette après import » (activée par défaut), n'a qu'une page lisible à la fois.
- La Bamboo Slate n'est enregistrée qu'auprès d'une application : une fois enregistrée dans Ardoise, elle ne fonctionne plus avec l'application du téléphone ni avec Inkspace avant d'y être réenregistrée. La tablette Xiaomi, elle, ne parle qu'à une application à la fois (Mi Home doit être fermé) et ses pages vont à l'application qui les importe la première.
- **Xiaomi** : la tablette ne donne pas l'heure de chaque point, donc la relecture et la vidéo d'une page récupérée rejouent les traits à vitesse régulière (en direct, Ardoise horodate ce qu'elle reçoit) ; une page fait environ 20 × 27 cm ; une feuille ne se reprend qu'avec la tablette qui l'a écrite, et seulement dans le sens où la tablette est tenue.
- **Texte tapé, équations, signets, formes** : ces outils n'existent que dans l'éditeur, pas pendant le direct. Pas de retour à la ligne automatique dans un texte (`Entrée` commence une ligne) ; la gomme n'efface pas un texte. Une équation n'est pas du LaTeX complet : pas de couleurs à l'intérieur (une seule couleur pour toute l'équation), pas de numéros, que les écritures latine et grecque ; dans un PDF, le code d'une équation n'est pas cherchable (le texte tapé l'est). Un signet ne se voit que dans l'éditeur : jamais dans les exports en image, les vidéos, l'impression, la projection ni le partage.
- **PDF d'un classeur** : la page de sommaire n'existe que dans le PDF, pas à l'impression (imprimez le PDF pour l'avoir sur papier) ; l'export d'une sélection de notes qui n'est pas un classeur n'a pas de plan.
- **Import d'Inkspace** : la pression est estimée à partir de l'épaisseur du trait, et le minutage de l'écriture est parfois inventé (la relecture de ces notes n'est qu'indicative) ; le cloud Wacom et la collaboration d'Inkspace n'existent pas dans Ardoise ; seules les notes présentes sur le PC sont reprises. L'import ne modifie rien chez Inkspace.
- **Copies de secours** : elles ne couvrent que les modifications d'une note ouverte dans l'éditeur, quelques secondes après le dernier changement ; elles restent sur le PC (jamais dans Nextcloud). Un arrêt que Windows impose sans attendre n'est pas retenu.
- **Reconnaissance d'écriture** : elle dépend du module d'écriture manuscrite installé dans Windows pour la langue, et peut se tromper ; les textes exportés doivent être relus.
- **MP4** : il utilise l'encodeur de Windows ; sur une édition « N » sans le Media Feature Pack, il faut télécharger OpenH264 chez Cisco (à la demande) ou choisir le WebM, qui est nettement plus lent.
- **Narration enregistrée en écrivant** : la relecture avec cette narration se lit à la vitesse normale, sans couper les temps morts ; une note déjà écrite en classe, commentée ensuite en écrivant, rejoue donc ses pauses réelles avant que la voix commence (dans l'application et dans la vidéo). Un son enregistré en écrivant qui n'a pas pu être écrit dans la note (elle a changé ailleurs) n'est pas rattaché de nouveau après un plantage ; un enregistrement en cours à la fermeture d'Ardoise est perdu.
- **Papier et fond du direct** : pas de retour en arrière par `Ctrl + Z` (on rechoisit l'ancien réglage) ; pas de proposition de passer la feuille en portrait ou en paysage pour une fiche (une page du direct suit la tablette).
- **Deux comportements vus de temps en temps, pas encore expliqués** : en direct, recolorer une sélection pendant que la tablette écrit a, deux fois lors d'un essai automatique, laissé manquer une petite partie d'un trait jusqu'au prochain redessin de la page (impossible à reproduire à volonté) ; dans l'éditeur, un second clic sur **Page précédente** ou **Page suivante**, fait pendant que la note s'ouvre encore, peut être ignoré : cliquez de nouveau.
- Pas d'export PSD ni de formats d'encre WILL / Universal Ink Model.
- Les notes d'une même bibliothèque ne doivent pas être modifiées sur deux PC en même temps (Ardoise refuse d'écraser l'autre version et propose une copie). Pour mettre à jour `ardoise.exe` dans un dossier synchronisé, fermer Ardoise sur tous les PC.

---

# Release notes — Ardoise 0.1.0

Version 0.1.0, the first version of Ardoise. Publication date: to be published. These notes tell the user what the version does and, above all, **what has not been tried**, and how each thing was checked. Technical detail is in the [changelog](../CHANGELOG.md) (in French); usage is explained in the [guide](guide/guide.en.md).

## What this version does

**Tablets**

- **Bamboo Slate**: registration over Bluetooth LE, once, valid for all your PCs; automatic connection; sync (each page becomes a note, deleted from the tablet only after the note has been written and checked); tablet information (battery, name, setting the clock); four tablet positions, which four buttons of the live window also change during the lesson.
- **Xiaomi Mijia LCD blackboard** (13.5", the model with memory): it is added without registration (it stays linked to Mi Home, which must be closed); its pages are imported as notes dated from when they were saved on the tablet, then deleted from it (it only hands over its oldest page); with **Delete from the tablet after import** turned off, they are imported only after you confirm. Its four positions (portrait or landscape, buttons on one side or the other) are set like the Slate's, even during live mode.
- **The Xiaomi's pages are protected**: the tablet removes by itself the page of a screen that was not cleared as soon as it is downloaded, so Ardoise first keeps a backup copy and imports it again if the note could not be written; saving the same screen again updates its note instead of adding a second one. With the Xiaomi, live mode starts by importing the pages the tablet holds (or offers to).
- **Two tablets** can be known at the same time: Ardoise connects to the one that is switched on, and a live session stays on the tablet it started with. The **Tablet** panel has three tabs (**Use**, **Settings**, **Tablets**).
- **Guided tablet test** and diagnostic logs kept in the library; the pages downloaded during the test are also added to the library as notes before being deleted from the tablet.

**Live mode and projection**

- Real-time stroke, pages and layers, blackboard, laser pointer, zoom; full-screen presentation on a projector (extended desktop) with annotations over it that are not saved. Built for the classroom: `Space` always means "next page"; `Esc` closes the projection when it is shown, otherwise asks before ending live mode; **Clear the page** can be taken back for 8 seconds with **Undo clearing**. The Slate's button creates a new page (or a new layer, as you choose).
- **Resuming live mode on a sheet** you already saved (note menu or editor): you keep writing in the same note, with its background image, original and narration, and nothing is overwritten if it changes elsewhere. A sheet is resumed with the tablet that wrote it.
- **Touching up during live mode**, without leaving the control window or stopping the tablet: **Select** tool (lasso, click, `V`), **Delete**, a colour for a group of strokes (the pen's colour does not move), moving and resizing the selection (pen, finger or mouse), `Ctrl + Z` / `Ctrl + Y` (never what the tablet writes). The projector and the other screen on the network only see the result.
- **Correcting during live mode**, on a layer of your own: while a pupil writes on the tablet, you write at the same time with the computer's pen (**Pen** tool, `P` key), in red by default, on a separate layer (for example "Correction"); the eraser end of the pen erases whole strokes of that layer; `Ctrl + Z` undoes your actions, never what the tablet wrote. The **Layers** zone (the same in the editor) marks the chosen layer and the tablet's, and warns when a hidden layer would be neither projected nor shared. A hidden layer is never exported.
- **The paper and a worksheet under the page**, during live mode: the **Page** and **Background** tabs of the control window change the paper of the current page (colour, template, texture) and put an image or a PDF page under the writing, without stopping the tablet; the change appears at once on the projector and on the other screen of the network, and it is saved with the page. A new page keeps the paper, not the worksheet.
- **Share on the network** (live mode and editor): another computer of the same network shows in a browser the live sheet (with the annotations written over the presentation) or the page of the open note, and nothing else; address, QR code, 6-character code, pause, stop. The other screen sets its own zoom. The first time, Windows asks for the firewall permission (administrator rights): answer "Private networks".

**Filing and finding**

- A library in an ordinary folder (Nextcloud-friendly: atomic writes, conflict-copy detection, refresh when a note changes on another PC).
- **Binders** (created, renamed, deleted from their ⋯ menu; the notes of a deleted binder go to the trash and come back to their binder) and sub-binders; a **free order** of the notes in a binder (drag and drop between two cards, or `Alt` + arrows); **tags** in eight colours, shared between your PCs; favourites; trash (30 days); you can also **drag notes onto Favourites or onto a tag** in the sidebar.
- **Multiple selection**: move, tag, assemble, **Turn** several notes at once, **Duplicate** (a "(copy)" in the same binder, with the same date); **Change the date** of a note.
- **Search** by title, tag, bookmark, typed text, an equation's code and handwriting (Windows handwriting recognition, offline).
- **Binder summary**: a **Summary** button listing, as you choose, the sub-binders, the notes and their bookmarks; a click opens the note at the bookmark.
- An **Undo** message after **Move to trash**, **Turn**, **Duplicate**, **Change the date** or deleting a binder (and `Ctrl + Z` in the library). Changing library in Settings first shows what the folder holds and asks for confirmation. A banner flags unreadable files, with the reason.
- **New note from a PDF or an image**, to annotate: for a multi-page PDF, one thumbnail per page to tick, a progress bar and a **Cancel** button for a long PDF.
- **Import of Wacom Inkspace notes** from the application (**Import from Inkspace…** in the sidebar, also offered by the first-run wizard): automatic detection or a folder of your choice, destination binder ("Inkspace" by default), progress bar and **Stop the import**, a report; a note already imported is skipped. The command line `ardoise.exe --import-inkspace` does the same import.

**Editing**

- Seven pressure-sensitive inks, eraser, lasso, colours and width (adjustable in steps of 0.05 mm: slider, **−** and **+** buttons, a typed value), layers, papers, page templates (lined, Seyès, squared, graph paper, dotted, music staves), sheet format and orientation, straightening shapes (adjustable), splitting by instant or by zone, back to the original, copy and paste between notes, unlimited undo and redo. `Esc` acts in steps: it abandons the gesture under way, closes a panel, deselects, then saves the note and goes back to the library.
- **Lines, rectangles and ellipses** (which can snap to the paper's grid); nothing is straightened unless you ask.
- **Text typed on the keyboard** (**Text** tool, `T` key), in Latin Modern or Andika, among the handwriting.
- **LaTeX equations** (**Equation** tool, `M` key), chemistry included (`\ce{2H2 + O2 -> 2H2O}`): a preview is drawn as you type; the equation is a vector drawing that keeps its code, which the search finds. MathJax is shipped with Ardoise: no network access.
- **Bookmarks** (**Bookmark** tool, `B` key): a word or a short phrase put on the page, for the binder summary, the search and the PDF outline; they are only seen in the editor.
- **Image or PDF background** to annotate, the PDF page being chosen on thumbnails.
- Pen and touch screen (pen eraser end, pen button as a lasso and then the tip moving or resizing the selection, fingers, palm rejection).
- **Tabbed side panel**, in the editor and in the live control window: a column of icons (Layers, Page, Background, Sound, Projection, Help) and the panel of the open tab, which a click collapses to give the page the whole width; **Help** gives the window's list of shortcuts.
- **Projecting the editor's note**: the editor's **Projection** tab sends the page of the open note, full screen, to the projector, updated at every stroke, with the zoom, **Whole page** and **Width**; the projected area follows what the editor shows and a dashed frame shows it; the replay is projected too. No blackboard, laser or marks on this projection.
- Automatic backup copy of unsaved changes, on the PC only: after a crash or a forced restart, Ardoise offers to recover them (onto the note if it has not changed, otherwise into a copy). At a Windows logoff or shutdown with unsaved work, Windows shows that Ardoise is preventing the shutdown, with the reason.

**Replaying and recording sound**

- Animated replay (0.25× to 16×, dead time removable); narration with a microphone, recorded during a replay or, as you choose, **while you write in the editor**: the strokes you write take the time of the sound and are replayed with it, on screen and in the video. A single narration per note: recording again first asks whether to **Replace** or **Keep the old one**.

**Exporting and printing**

- PNG, JPG and WebP images; SVG and PDF (layers can be kept apart, to show or hide in Inkscape, Acrobat Reader or Firefox); text, Markdown and Word, with the structure drawn from the handwriting (the same words as the copied text, the assistant's corrections win, a line under words makes them bold in Markdown and underlines them in Word, **Include the drawings** option); MP4, GIF, WebP and WebM videos; clipboard copy; dragging a note out to Explorer or Word.
- **A binder in a single PDF** (right-click the binder), in the binder's order, with an outline (one entry per note, per sub-binder and per bookmark) and, as you choose, a **summary page** at the start, each line of which is a link. The three **In the summary** choices (sub-binders, notes, bookmarks) set the binder summary, the PDF outline and this page. The PDF of a single note takes its bookmarks as its outline.
- **Printing** (`Ctrl + P`, or **Print…**) of a note, a selection or a binder (**Print the binder…**), at 1, 2 or 4 notes per sheet, with or without the paper background, at real size or fitted to the sheet. Typed text and equations print; bookmarks never do.
- **Assistant (MCP)**: Claude Code, Claude Desktop or another assistant can read the notes (handwriting, typed text, equations, bookmarks), tidy them up on a copy, title and file them, and drive the open Ardoise; read-only mode available. Ardoise itself contains no artificial intelligence.

**Around it**

- French and English interface, light and dark themes, portable application (one executable, settings next to it), no administrator rights, no account, no connection except the optional OpenH264 download and the local network share started by hand.
- **Right-click**: Ardoise's own menus where you expect them (cards, binders, tags, trash, layers, the editor's page), never a web page's menu; a browser's shortcuts (`F5`, `Ctrl + R`, `F12`…) are turned off.
- **Distribution archive**: an `ardoise-<version>` folder with `ardoise.exe`, `LICENSE`, `THIRD-PARTY-NOTICES`, `LISEZMOI.txt` and the guides as HTML pages (`guide\`), in French and English, illustrated with screenshots and with a "Getting started" section. An **About** screen (version, licence, credits, components).
- The **Guide** button (`F1` key) opens the user guide from Ardoise.
- One window per copy of Ardoise: a second launch brings the open window to the front; two copies in two folders run side by side.
- Free software, GPL-3.0-or-later licence. Credits: the Tuhi project (documentation of the Wacom protocol), MathJax (Apache 2.0), the Latin Modern and Andika fonts, Cisco's OpenH264 (never shipped: downloaded by you, if needed).
- Note file format: version 1.6 (earlier versions are read; an older version of Ardoise keeps a note's texts, equations and bookmarks without drawing them).

## To try before relying on it

### How this version was checked

- **By hand, with the real hardware**: by the author, with a Bamboo Slate A4, a Xiaomi tablet, a projector, a Surface and its pen, a microphone, following the procedures in the [list of hardware tests](hardware-tests.md) (in French).
- **By automated tests on Windows, on the production executable**: formatting and code analysis, tests of each part, over a hundred end-to-end journeys, in French and English, driving the real window (with a simulated Slate and a simulated Xiaomi tablet, a simulated microphone and a real browser for network sharing); a check of the components' licences and of the absence of personal data in the project's history.
- **A simulated tablet** stands in for the real one in those journeys: what was only seen that way is called "simulated tablet" below.

### What has been tried with the real hardware

| Trial | What it must establish | Where it stands |
|---|---|---|
| **H0**: `ardoise.exe` on the work PC | Windows (SmartScreen, Smart App Control, AppLocker) accepts the unsigned executable, from the Nextcloud folder; the Bluetooth dongle is seen. | **No report made.** Ardoise was used on the author's school PC for the later trials, but what Windows showed at the first launch was not recorded. On another managed PC, nothing is guaranteed. |
| **H1**: the tablet talks to Ardoise | Registration, reading the information, downloading the pages and live mode work with a real Slate. | **Done on 2 November 2026**, with a real Slate A4. The logs led to protocol fixes (firmware version, order of the sync commands, searching for the tablet); the real captures are replayed by the tests, so page decoding has been checked against real Slate data. |
| **H2**: sync, live mode, projector | Real sync, perceived latency, the tablet's button in live mode, recovery after a drop, projecting on a real projector, the four positions. | **Done** (reported on 12 November). The tablet's button creates a new page in live mode (signal seen with the real tablet on 5 November). Feedback from the trial: a selection could be neither moved nor resized in live mode; that is added. |
| **H3**: pen and touch; full trial in class | Pen eraser end, button, palm, finger gestures on a touch PC; then a whole lesson with the tablet. | **Done** (reported on 12 November). |
| **H4**: sharing on the network | A second computer shows the live sheet, then an open note: firewall permission, address typed or QR code, perceived delay, reconnection. | **Done on 3 November** on two computers, then at work behind the school's web filter; those trials led to several fixes (an address completed as `https://`, a connection held back by the filter). Not confirmed since: Chrome on Android after the fix, and an open note (not the live sheet) shared on the school network. |
| **H5**: resuming a sheet in live mode, touching up | Interrupt a live session, resume the sheet, write on it; delete, recolour and undo while the tablet writes. | **Done** (reported on 7 November, no remark). Moving and resizing a selection with the pen or a finger was added afterwards: not yet tried on the Surface. |
| **H6**: correcting during live mode | A pupil writes while the teacher corrects with the Surface's pen on a layer of their own; a PDF export with separate layers. | **Done** (reported on 12 November). |
| **H7**: paper and worksheet in live mode, projecting the editor, sound while writing | Paper and a PDF worksheet changed while the tablet writes, an editor note projected, narration recorded while writing with the Surface's microphone. | **Done** (reported on 12 November). |
| **H8**: the Xiaomi tablet | Adding the tablet, fetching then deleting its pages, live mode, positions, two tablets. | **In progress.** A study on 12 November (live mode was started from the PC with the real tablet and the reconstructed stroke was confirmed), then first trials on 13 November 2026: importing a page saved while Ardoise is connected, clearing the screen during live mode and the page that disappears from the tablet as soon as it is downloaded were observed, and led to several fixes. **Not yet tried on the real tablet**: the backup copies of pages, updating the note of a screen saved again, live mode preceded by the import of the pages, the direction of the four positions (deduced from their names, to be confirmed), the position buttons during live mode, the real size of the imported writing (the unit of the coordinates has not been measured with a ruler) and two tablets switched on together: all of that has been checked with a simulated tablet only. |

For H2, H3, H6 and H7, the roadmap only says that the author did them and reported back, without detailing the steps or the measurements.

### What remains to try by hand

These points were checked by automated tests only, or not at all:

- **A real printer**: printing is tested towards "Microsoft Print to PDF"; Windows' own dialog is not driven by the tests. This also applies to 2 and 4 notes per sheet.
- **Word**: a Word export opened in Word, and Markdown and Word formatting on real handwriting.
- **PDF**: the outline and the links of the summary page of a binder PDF, opened in a PDF reader (Acrobat Reader, Firefox…).
- **The Surface, with the pen and a finger, since the latest additions**: moving and resizing a selection in live mode; the Text, Equation, Bookmark, Line, Rectangle and Ellipse tools; the right-click menus (which must never open with the pen's button); Windows' on-screen keyboard in the text, equation and bookmark fields; the **Choose another colour** picker.
- **The side panel** on the Surface: the rail and its 38 px icons with the pen, a finger and the mouse, the dark theme, the 800 × 560 window, the visible focus on the icons, Narrator reading the tabs.
- **Keyboard shortcuts** (`Ctrl + W`, `Ctrl + Q`, `Ctrl + P`, `Esc`…): the automated journeys send the keys by program, not from a real keyboard.
- **Handwriting recognition in English** (the English handwriting module of Windows is not installed on the development PC) and the pause of handwriting reading during the presentation.
- **A real drop** of a note dragged into Explorer, Word or OneNote.
- **The assistant**: it has already been used for real to retouch notes (which led to a fix), but not the command under PowerShell 7, nor the presentation on a second screen driven by the assistant, nor the last live-mode note found again after a real restart.
- **The Inkspace import** from Ardoise's window, with Inkspace open and then closed (it may lock its files).
- **A real Windows logoff** with an unsaved note, or during a live session: the "Ardoise is preventing shutdown" screen, the backup copy written, its offer at the next start.
- **Tag colours with Nextcloud on two PCs** (a change on one seen on the other; two simultaneous changes merged).
- **Windows 10**.

### Performance

Measured on 12 November 2026 on an idle laptop (Intel Core i5-1235U, 16 GB), before the additions of 13 and 14 November: start in 0.62 s, a 1,000-note library shown in 1.63 s (cached) and scrolled at 60 fps, live mode at 60 fps, the editor projection at 46 ms delay (50 ms accepted), MP4 1080p export of a 2-min replay in 15 s. **One target is not met**: a stroke written with the computer's pen reaches the projector with a median delay of 40 ms, against 30 ms aimed for. The executable stays under the 40 MB limit the project has set (25.2 MB measured on 13 November, before the fonts and MathJax were added). Details: [performance](performance.md) (in French).

## Known limits

- **Windows only**: Windows 11 is the only version tried (Windows 10 is aimed at, not tried). No macOS, no Linux, no phone app.
- **Unsigned executable**: a SmartScreen warning at the first start is normal; a managed PC may refuse to run it.
- **Tablets**: Bamboo Slate A4 (the target model) and Xiaomi Mijia LCD blackboard, 13.5", the model with memory (first trials only). Slate A5, Spark, Folio, Intuos Pro Paper and the other Xiaomi models are untested.
- Both tablets hand over only their **oldest page**: the Bamboo Slate, without the "Delete from the tablet after import" option (on by default), has only one readable page at a time.
- The Bamboo Slate is registered with one application only: once registered in Ardoise, it no longer works with the phone app or with Inkspace until it is registered there again. The Xiaomi tablet talks to one application at a time (Mi Home must be closed) and its pages go to the application that imports them first.
- **Xiaomi**: the tablet does not give the time of each point, so the replay and the video of a fetched page replay the strokes at a regular speed (in live mode, Ardoise timestamps what it receives); a page is about 20 × 27 cm; a sheet is resumed only with the tablet that wrote it, and only in the direction the tablet is held.
- **Typed text, equations, bookmarks, shapes**: these tools exist only in the editor, not during live mode. No automatic line wrap in a text (`Enter` starts a line); the eraser does not erase a text. An equation is not full LaTeX: no colours inside (a single colour for the whole equation), no numbers, only the Latin and Greek scripts; in a PDF, an equation's code is not searchable (typed text is). A bookmark is seen only in the editor: never in image exports, videos, printing, projection or sharing.
- **A binder's PDF**: the summary page exists only in the PDF, not in printing (print the PDF to have it on paper); the export of a selection of notes that is not a binder has no outline.
- **Inkspace import**: the pressure is estimated from the stroke width, and the timing of the writing is sometimes made up (the replay of those notes is only indicative); Wacom's cloud and Inkspace's collaboration do not exist in Ardoise; only the notes present on the PC are taken over. The import changes nothing in Inkspace.
- **Backup copies**: they only cover the changes of a note open in the editor, a few seconds after the last change; they stay on the PC (never in Nextcloud). A shutdown that Windows forces without waiting is not held back.
- **Handwriting recognition**: it depends on the handwriting module installed in Windows for the language, and can make mistakes; exported texts must be read over.
- **MP4**: it uses Windows' encoder; on a Windows "N" edition without the Media Feature Pack, you must download OpenH264 from Cisco (on request) or choose WebM, which is much slower.
- **Narration recorded while writing**: the replay with this narration plays at normal speed, without cutting dead time; a note already written in class, then commented while writing, therefore replays its real pauses before the voice starts (in the application and in the video). A sound recorded while writing that could not be written into the note (it changed elsewhere) is not attached again after a crash; a recording under way when Ardoise closes is lost.
- **Live paper and background**: no going back with `Ctrl + Z` (you choose the previous setting again); no offer to switch the sheet to portrait or landscape for a worksheet (a live page follows the tablet).
- **Two behaviours seen now and then, not yet explained**: in live mode, recolouring a selection while the tablet writes twice, in an automated trial, left a small part of a stroke missing until the next redraw of the page (not reproducible on demand); in the editor, a second click on **Previous page** or **Next page**, made while the note is still opening, can be ignored: click again.
- No PSD export, no WILL / Universal Ink Model ink formats.
- Notes of a library must not be edited on two PCs at the same time (Ardoise refuses to overwrite the other version and offers a copy). To update `ardoise.exe` in a synced folder, close Ardoise on all PCs.
