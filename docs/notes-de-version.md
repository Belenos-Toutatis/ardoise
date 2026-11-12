# Notes de version — Ardoise 0.1.0

Première version d'Ardoise, en préparation (rien n'est encore publié). Ces notes disent, pour l'utilisateur, ce que la
version fait et surtout **ce qui n'a pas encore été essayé**. Le détail technique est dans le
[journal des modifications](../CHANGELOG.md) ; l'usage est expliqué dans le [guide](guide/guide.fr.md).

*English version below.*

## Ce que fait cette version

**Récupérer et écrire**

- Enregistrement de la tablette Bamboo Slate en Bluetooth LE (une seule fois, valable pour tous vos PC), connexion
  automatique, synchronisation des pages (chaque page devient une note), effacement de la tablette seulement après
  écriture vérifiée, quatre positions de la tablette.
- Mode direct : tracé en temps réel, pages et calques, tableau noir, pointeur laser, zoom ; présentation en plein écran
  sur le vidéoprojecteur (bureau étendu) avec annotations par-dessus qui ne sont pas enregistrées. Pensé pour le cours :
  `Espace` veut toujours dire « page suivante » ; `Échap` ferme la projection quand elle est affichée, sinon demande
  confirmation avant de terminer le mode direct ; **Effacer la page** se rattrape pendant 8 secondes avec **Annuler
  l'effacement**.
- **Reprendre le mode direct sur une feuille** déjà enregistrée (menu de la note ou éditeur) : on continue d'écrire dans la même note, avec son image de fond, son original et sa narration, sans rien écraser si elle change ailleurs ; *pas encore essayé avec la vraie tablette (essai H5)*.
- **Retoucher pendant le direct**, sans quitter la fenêtre de commande ni arrêter la tablette : outil **Sélection** (lasso, clic, `V`), **Supprimer**, couleur d'un groupe de traits (la couleur du stylo ne bouge pas), `Ctrl + Z` / `Ctrl + Y` (suppression, couleur, **Effacer la page** ; jamais ce que la tablette écrit). Le vidéoprojecteur et l'autre écran du réseau ne voient que le résultat. Vérifié par des parcours automatiques avec la tablette simulée (fenêtre de commande, présentation et un vrai navigateur comparés) ; *la fluidité réelle et le confort du lasso à la souris et au stylet ne sont pas encore essayés (essai H5, partie H)*.
- **Corriger pendant le direct**, sur un calque à soi : pendant qu'un élève écrit sur la tablette, vous écrivez en même temps avec le stylet de l'ordinateur (outil **Stylo**, touche `P`), en rouge par défaut, sur un calque à part (par exemple « Correction ») ; le bout gomme du stylet efface des traits entiers de ce calque ; `Ctrl + Z` annule vos actions, jamais l'écriture de la tablette. La zone **Calques** (la même dans l'éditeur) marque le calque choisi et celui de la tablette, qu'on peut envoyer ailleurs, et avertit quand un calque masqué ne serait ni projeté ni partagé. Le trait apparaît aussitôt au vidéoprojecteur et sur l'autre écran du réseau ; dans l'éditeur, la sélection et la gomme ne prennent que le calque choisi. À l'export, le **SVG** et le **PDF** peuvent garder des calques séparés (à afficher ou masquer dans Inkscape, Acrobat Reader ou Firefox), un calque masqué n'est jamais exporté ; *pas encore essayé avec la vraie tablette ni avec le stylet de la Surface (essai H6)*.
- **Le papier et une fiche sous la page, pendant le direct** : les onglets **Page** et **Fond** de la fenêtre de commande changent le papier de la page en cours (couleur, modèle, texture) et mettent une image ou une page de PDF sous l'écriture, sans arrêter la tablette ; le changement apparaît aussitôt au vidéoprojecteur et sur l'autre écran du réseau, et il est enregistré avec la page. Une nouvelle page garde le papier, pas la fiche. Ces changements ne s'annulent pas avec `Ctrl + Z` (on rechoisit l'ancien réglage) ; une page qui porte une fiche et de l'écriture ne tourne plus avec la tablette, comme une feuille reprise. Vérifié par des parcours automatiques avec la tablette simulée (présentation, partage et fichier de la note) ; *pas encore essayé avec la vraie tablette, le vidéoprojecteur ni la télécommande de la salle (essai H7)*.

**Ranger et retrouver**

- Bibliothèque dans un dossier ordinaire (compatible Nextcloud : écritures atomiques, détection des copies en conflit,
  mise à jour quand une note change sur un autre PC) ; classeurs (créés, renommés, supprimés depuis leur menu ⋯ : les
  notes d'un classeur supprimé vont à la corbeille et reviennent dans leur classeur), étiquettes avec huit couleurs au
  choix (partagées entre vos PC), favoris, corbeille (30 jours), sélection multiple, assemblage de notes, **Tourner**
  plusieurs notes d'un coup ; recherche par titre, étiquette et écriture manuscrite (reconnaissance d'écriture de
  Windows, hors connexion).
- Un message **Annuler** après **Mettre à la corbeille**, **Tourner** ou la suppression d'un classeur (et `Ctrl + Z`
  dans la bibliothèque). Changer de bibliothèque dans les Réglages montre d'abord ce que contient le dossier et demande
  confirmation. Un bandeau signale les fichiers illisibles, avec la raison.
- Notes créées à partir d'un PDF ou d'une image, à annoter (avec une barre d'avancement et un bouton **Annuler** pour
  un long PDF).
- Import des notes de Wacom Inkspace depuis l'application (**Importer depuis Inkspace…** dans la barre latérale, proposé
  aussi par l'assistant du premier lancement) : détection automatique ou dossier au choix, classeur de destination
  (« Inkspace » par défaut), barre d'avancement et **Arrêter l'import**, compte rendu ; une note déjà importée est
  ignorée. La ligne de commande `ardoise.exe --import-inkspace` fait le même import.

**Modifier**

- Sept outils d'encre sensibles à la pression, gomme, lasso, couleurs, calques, papiers, modèles de page (ligné, Seyès,
  quadrillé, millimétré, pointillé, portées), format et orientation de la feuille, redressement des formes, découpe
  par instant ou par zone, retour à l'original, copier-coller entre notes, annuler et rétablir sans limite.
- Stylet et écran tactile (dos du stylet qui gomme, bouton du stylet qui fait un lasso puis pointe qui déplace ou
  redimensionne la sélection, doigts, rejet de la paume).
- **Panneau de côté à onglets**, dans l'éditeur et dans la fenêtre de commande du direct : une colonne d'icônes (Calques, Page, Fond, Son, Projection, Aide) et le panneau de l'onglet ouvert, qu'un clic sur l'onglet ou sur **›** replie pour donner toute la largeur à la page ; l'onglet et l'état replié sont retenus pour chaque fenêtre. Des pastilles sur le rail (nombre de calques, calque masqué, enregistrement, projection ou partage), la sélection au-dessus de l'onglet ouvert, le partage sur le réseau dans **Projection**, l'**Aide** avec la liste des raccourcis de la fenêtre (une seule liste, que les tests confrontent aux touches traitées). Vérifié par des parcours automatiques dans chaque fenêtre ; *l'aspect avec le stylet et le doigt sur la Surface, le thème sombre et la fenêtre étroite ne sont pas encore regardés sur le PC*.
- **Projeter la note de l'éditeur** : l'onglet **Projection** de l'éditeur envoie la page de la note ouverte, en plein écran, au vidéoprojecteur, mise à jour à chaque trait (le trait en cours aussi), avec le zoom, **Page entière** et **Largeur** ; la zone projetée suit ce que montre l'éditeur et un cadre en pointillés la montre ; la relecture est projetée aussi ; elle s'arrête par son bouton, `Échap` ou en quittant l'éditeur. Pas de tableau noir, de laser ni d'annotations sur cette projection. Vérifié par des tests automatiques ; *le retard du trait avec le stylet de la Surface et la fluidité sur un vrai vidéoprojecteur ne sont pas encore essayés (essai H7)*.
- Copie de secours automatique des modifications non enregistrées, sur le PC seulement : après un plantage ou un
  redémarrage forcé, Ardoise propose de les récupérer (sur la note si elle n'a pas changé, sinon dans une copie).
  À une fermeture de session ou un arrêt de Windows avec un travail non enregistré, Windows affiche qu'Ardoise
  empêche l'arrêt, avec la raison.

**Relire et exporter**

- Relecture animée du tracé (0,25× à 16×, temps morts supprimables), narration au micro, enregistrée pendant une relecture ou, au choix, **pendant qu'on écrit dans l'éditeur** : les traits écrits prennent l'heure du son et se rejouent avec lui, après ceux qui existent déjà, à l'écran et dans la vidéo ; une seule narration par note, et enregistrer à nouveau demande d'abord s'il faut **Remplacer** ou **Garder l'ancienne**. *La synchronisation de la voix et de l'écriture n'est pas encore mesurée avec le vrai micro de la Surface (essai H7).*
- Exports : PNG, JPG, WebP, SVG, PDF, texte, Markdown et Word (avec la structure tirée de l'écriture ; mêmes mots que le
  texte copié, les corrections de l'assistant l'emportent, un trait sous des mots les met en gras en Markdown et les
  souligne dans Word, option **Inclure les dessins**), vidéos MP4, GIF, WebP et WebM ; copie dans le presse-papiers,
  impression, glisser une note vers l'Explorateur ou Word.
- Assistant (MCP) : Claude Code, Claude Desktop ou un autre assistant peut lire les notes, les mettre au propre, les titrer
  et les ranger, et piloter Ardoise ouverte ; mode lecture seule possible.

**Autour**

- Interface en français et en anglais, thèmes clair et sombre, application portable (un exécutable, réglages à côté), aucun
  droit d'administrateur, aucun compte, aucune connexion sauf le téléchargement facultatif d'OpenH264 et le partage sur
  le réseau local lancé à la main.
- **Partager sur le réseau** (mode direct et éditeur) : un autre ordinateur du même réseau affiche dans un navigateur la
  feuille du mode direct (avec les annotations écrites par-dessus la présentation) ou la page de la note ouverte, et
  rien d'autre ; adresse, QR code, code de 6 caractères, pause, arrêt. La première fois, Windows demande
  l'autorisation du pare-feu (droits d'administrateur) : répondre « Réseaux privés ».
- Archive de distribution (`cargo xtask dist`) : un dossier `ardoise-<version>` avec `ardoise.exe`, `LICENSE`,
  `THIRD-PARTY-NOTICES`, `LISEZMOI.txt` et les guides en pages HTML (`guide\`). Écran **À propos** (version, licence,
  crédits, composants).
- Une seule fenêtre par exemplaire d'Ardoise : un second lancement ramène la fenêtre ouverte au premier plan ; deux
  exemplaires dans deux dossiers fonctionnent côte à côte.
- Test guidé de la tablette et journaux de diagnostic rangés dans la bibliothèque ; les pages téléchargées pendant le
  test sont aussi ajoutées à la bibliothèque comme notes avant d'être effacées de la tablette.
- Format de fichier des notes : version 1.2 (les versions précédentes sont lues).

## À essayer avant de compter dessus

La tablette n'est pas sur le poste de développement : **tout ce qui parle à la vraie Bamboo Slate a été vérifié avec une
tablette simulée uniquement**. Les essais suivants sont encore à faire par l'utilisateur, avec les procédures de
`docs/hardware-tests.md` ; les journaux reviennent par Nextcloud.

| Essai | Ce qu'il doit établir |
|---|---|
| **H0** — `ardoise.exe` sur le PC du travail | Windows (SmartScreen, Smart App Control, AppLocker) accepte l'exécutable non signé, depuis le dossier Nextcloud ; la clé Bluetooth est vue. |
| **H1** — la tablette parle à Ardoise | L'enregistrement, la lecture des informations, le téléchargement des pages et le mode direct fonctionnent avec une vraie Slate. **Le décodage des pages n'a jamais été vérifié sur une capture réelle de Slate** (seulement sur des données d'une autre tablette et des données synthétiques). |
| **H2** — synchronisation, mode direct, projecteur | Synchro réelle, latence ressentie, ce que la tablette envoie à l'appui sur son bouton en mode direct (inconnu aujourd'hui), reprise après coupure, projection sur un vrai vidéoprojecteur (et son débranchement), les quatre positions de la tablette. |
| **H3** — stylet et tactile ; essai complet en classe | Dos du stylet, bouton, paume, gestes des doigts sur un PC tactile ; puis un cours entier avec la tablette. |
| **H4** — partage sur le réseau | Un second ordinateur du réseau affiche la feuille en direct puis une note ouverte : autorisation du pare-feu, adresse tapée ou QR code, délai ressenti (moins de 150 ms attendus), reconnexion après une coupure. |
| **H5** — reprendre une feuille en direct | Avec la vraie tablette, en classe : interrompre un direct, reprendre la feuille (menu de la note ou éditeur), écrire dessus ; format et position de la tablette différents de la feuille ; fond visible au vidéoprojecteur et sur un autre écran du réseau ; puis supprimer, recolorer et annuler des traits pendant que la tablette écrit, sans que rien de la sélection n'apparaisse au vidéoprojecteur ni sur l'autre écran. |
| **H6** — corriger pendant le direct | Avec la vraie tablette et le stylet de la Surface, en classe : un élève écrit pendant que l'enseignant ajoute un calque « Correction » et y écrit en rouge, gomme avec le bout gomme et annule avec `Ctrl + Z` ; envoyer la tablette sur un autre calque, masquer un calque (avertissement) ; fluidité, paume, marques de Windows Ink, icône de tablette au survol du stylet ; puis un export PDF à calques séparés ouvert dans Acrobat Reader ou Firefox. |
| **H7** — papier et fond en direct, projection de l'éditeur, son pendant qu'on écrit | Avec la vraie tablette, le vidéoprojecteur, la télécommande et le micro de la Surface : changer le papier et poser une fiche PDF sous la page pendant qu'un élève écrit (à-coups, délai, écriture bien placée sur la fiche, télécommande qui tourne la page après un choix dans une liste, page qui ne tourne plus quand on tourne la tablette) ; projeter une note de l'éditeur et y écrire au stylet (retard du trait, zone projetée et cadre en pointillés, relecture, un seul écran) ; enregistrer le son en écrivant, relire avec la narration et exporter la vidéo MP4 (décalage entre la voix et l'écriture au début et à la fin, lecteur de Windows et VLC), question **Remplacer** / **Garder l'ancienne**. |

D'autres essais à la main restent à faire, car les tests automatiques ne peuvent pas les remplacer :

- le panneau de côté sur la Surface : le rail et ses icônes de 38 px au stylet, au doigt et à la souris (info-bulle au survol), le panneau replié puis rouvert, le thème sombre, la fenêtre de 800 × 560, le focus visible sur les icônes, la lecture des onglets et de leurs pastilles par le Narrateur, `Espace` sur une icône en mode direct (il tourne la page) ; la pastille rouge pendant un vrai enregistrement (les parcours n'ont pas de micro) ;
- la reconnaissance d'écriture **en anglais** (le module d'écriture anglais de Windows n'est pas installé sur le PC de
  développement) et la pause de la lecture de l'écriture pendant la présentation ;
- un vrai dépôt d'une note glissée dans l'Explorateur, Word ou OneNote ;
- l'impression sur une vraie imprimante, l'export Word ouvert dans Word, la mise en forme Markdown et Word sur une vraie
  écriture ;
- l'assistant : l'essai réel depuis Claude Code (mettre au propre une note), la commande sous PowerShell 7, la
  présentation sur un second écran pilotée par l'assistant, la note du dernier mode direct retrouvée après un vrai
  redémarrage ;
- l'import d'Inkspace depuis la fenêtre d'Ardoise, avec Inkspace ouvert puis fermé (il peut verrouiller ses fichiers) ;
- une vraie fermeture de session de Windows avec une note non enregistrée : l'écran « Ardoise empêche l'arrêt », la
  copie de secours écrite, sa proposition au lancement suivant ;
- les couleurs des étiquettes avec Nextcloud sur deux PC (changement sur l'un, vu sur l'autre ; deux changements
  simultanés fusionnés).

Performances mesurées le 29 octobre 2026 sur un portable au repos (Intel Core i5-1235U, 16 Go) : tous les objectifs
sont atteints (démarrage en 0,95 s, bibliothèque de 1 000 notes affichée en 1,8 s et défilée à 60 images/s, mode
direct à 60 images/s, export MP4 1080p d'une relecture de 2 min en 15 s, exécutable de 21 Mo). Détails :
[performances](performance.md).

## Limites connues

- **Windows seulement** : Windows 11 est la seule version essayée. Pas de macOS, pas de Linux, pas d'application pour
  téléphone.
- **Exécutable non signé** : un avertissement SmartScreen au premier lancement est normal ; un PC géré peut refuser de le
  lancer.
- **Tablettes** : Bamboo Slate (A4, modèle visé) ; Slate A5, Spark, Folio et Intuos Pro Paper non testés.
- La Bamboo Slate ne livre que sa **page la plus ancienne** : sans l'option « Effacer de la tablette après import »
  (activée par défaut), une seule page est lisible à la fois.
- Une tablette n'est enregistrée qu'auprès d'une application : une fois enregistrée dans Ardoise, elle ne fonctionne plus
  avec l'application du téléphone ni avec Inkspace avant d'y être réenregistrée.
- **Import d'Inkspace** : la pression est estimée à partir de l'épaisseur du trait, et le minutage de l'écriture est
  parfois inventé (la relecture de ces notes n'est qu'indicative) ; le cloud Wacom et la collaboration d'Inkspace
  n'existent pas dans Ardoise ; seules les notes présentes sur le PC sont reprises. L'import ne modifie rien chez
  Inkspace.
- **Copies de secours** : elles ne couvrent que les modifications d'une note ouverte dans l'éditeur, quelques secondes
  après le dernier changement ; elles restent sur le PC (jamais dans Nextcloud). Un arrêt que Windows impose sans
  attendre n'est pas retenu.
- **Reconnaissance d'écriture** : elle dépend du module d'écriture manuscrite installé dans Windows pour la langue, et
  peut se tromper ; les textes exportés doivent être relus.
- **MP4** : il utilise l'encodeur de Windows ; sur une édition « N » sans le Media Feature Pack, il faut télécharger
  OpenH264 chez Cisco (à la demande) ou choisir le WebM, qui est nettement plus lent.
- **Narration enregistrée en écrivant** : la relecture avec cette narration se lit à la vitesse normale, sans couper les
  temps morts ; une note déjà écrite en classe, commentée ensuite en écrivant, rejoue donc ses pauses réelles avant que la
  voix commence (dans l'application et dans la vidéo). Un son enregistré en écrivant qui n'a pas pu être écrit dans la note
  (elle a changé ailleurs) n'est pas rattaché de nouveau après un plantage ; un enregistrement en cours à la fermeture
  d'Ardoise est perdu.
- **Papier et fond du direct** : pas de retour en arrière par `Ctrl + Z` (on rechoisit l'ancien réglage) ; pas de
  proposition de passer la feuille en portrait ou en paysage pour une fiche (une page du direct suit la tablette).
- Pas de zone de texte tapée au clavier dans une note, pas d'export PSD ni de formats d'encre WILL / Universal Ink Model.
- Les notes d'une même bibliothèque ne doivent pas être modifiées sur deux PC en même temps (Ardoise refuse d'écraser
  l'autre version et propose une copie). Pour mettre à jour `ardoise.exe` dans un dossier synchronisé, fermer Ardoise
  sur tous les PC.
- Le nom « Ardoise » est un nom de travail ; il peut changer avant la publication.

---

# Release notes — Ardoise 0.1.0

First version of Ardoise, in preparation (nothing is published yet). These notes tell the user what the version does and,
above all, **what has not been tried yet**. Technical detail is in the [changelog](../CHANGELOG.md) (in French); usage is
explained in the [guide](guide/guide.en.md).

## What this version does

**Receiving and writing**

- Registering the Bamboo Slate over Bluetooth LE (once, valid for all your PCs), automatic connection, syncing the pages
  (each page becomes a note), deleting from the tablet only after the file has been written and checked, four tablet
  positions.
- Live mode: real-time stroke, pages and layers, blackboard, laser pointer, zoom; full-screen presentation on a
  projector (extended desktop) with annotations over it that are not saved. Built for the classroom: `Space` always
  means "next page"; `Esc` closes the projection when it is shown, otherwise asks before ending live mode; **Clear the
  page** can be taken back for 8 seconds with **Undo clearing**.
- **Resuming live mode on a sheet** you already saved (note menu or editor): you keep writing in the same note, with its background image, original and narration, and nothing is overwritten if it changes elsewhere; *not yet tried with the real tablet (test H5)*.
- **Touching up during live mode**, without leaving the control window or stopping the tablet: **Select** tool (lasso, click, `V`), **Delete**, a colour for a group of strokes (the pen's colour does not move), `Ctrl + Z` / `Ctrl + Y` (deletion, colour, **Clear the page**; never what the tablet writes). The projector and the other screen on the network only see the result. Checked by automated journeys with the simulated tablet (control window, presentation and a real browser compared); *the real smoothness and the comfort of the lasso with the mouse and the pen have not been tried yet (test H5, part H)*.
- **Correcting during live mode**, on a layer of your own: while a pupil writes on the tablet, you write at the same time with the computer's pen (**Pen** tool, `P` key), in red by default, on a separate layer (for example "Correction"); the eraser end of the pen erases whole strokes of that layer; `Ctrl + Z` undoes your actions, never what the tablet wrote. The **Layers** zone (the same in the editor) marks the chosen layer and the tablet's, which you can send elsewhere, and warns when a hidden layer would be neither projected nor shared. The stroke appears at once on the projector and on the other screen of the network; in the editor, the selection and the eraser take only the chosen layer. On export, the **SVG** and the **PDF** can keep separate layers (to show or hide in Inkscape, Acrobat Reader or Firefox), and a hidden layer is never exported; *not yet tried with the real tablet nor with the Surface's pen (test H6)*.
- **The paper and a worksheet under the page, during live mode**: the **Page** and **Background** tabs of the control window change the paper of the current page (colour, template, texture) and put an image or a PDF page under the writing, without stopping the tablet; the change appears at once on the projector and on the other screen of the network, and it is saved with the page. A new page keeps the paper, not the worksheet. These changes are not undone by `Ctrl + Z` (you choose the previous setting again); a page that carries a worksheet and writing no longer turns with the tablet, like a resumed sheet. Checked by automated journeys with the simulated tablet (presentation, share and the note's file); *not yet tried with the real tablet, the projector nor the room's remote (test H7)*.

**Filing and finding**

- A library in an ordinary folder (Nextcloud-friendly: atomic writes, conflict-copy detection, refresh when a note
  changes on another PC); binders (created, renamed, deleted from their ⋯ menu: the notes of a deleted binder go to the
  trash and come back to their binder), tags with a choice of eight colours (shared between your PCs), favourites, trash
  (30 days), multiple selection, assembling notes, **Turn** several notes at once; search by title, tag and handwriting
  (Windows handwriting recognition, offline).
- An **Undo** message after **Move to trash**, **Turn** or deleting a binder (and `Ctrl + Z` in the library). Changing
  library in Settings first shows what the folder holds and asks for confirmation. A banner flags unreadable files, with
  the reason.
- Notes created from a PDF or an image, to annotate (with a progress bar and a **Cancel** button for a long PDF).
- Import of Wacom Inkspace notes from the application (**Import from Inkspace…** in the sidebar, also offered by the
  first-run wizard): automatic detection or a folder of your choice, destination binder ("Inkspace" by default),
  progress bar and **Stop the import**, a report; a note already imported is skipped. The command line
  `ardoise.exe --import-inkspace` does the same import.

**Editing**

- Seven pressure-sensitive ink tools, eraser, lasso, colours, layers, papers, page templates (lined, Seyès, squared,
  graph paper, dotted, music staves), sheet format and orientation, straightening shapes, splitting by instant or by
  zone, back to the original, copy and paste between notes, unlimited undo and redo.
- Pen and touch screen (pen eraser end, pen button as a lasso and then the tip moving or resizing the selection,
  fingers, palm rejection).
- **Tabbed side panel**, in the editor and in the live control window: a column of icons (Layers, Page, Background, Sound, Projection, Help) and the panel of the open tab, which a click on the tab or on **›** collapses to give the page the whole width; the tab and the collapsed state are remembered for each window. Badges on the rail (number of layers, hidden layer, recording, projection or share), the selection above the open tab, network sharing in **Projection**, and **Help** with the window's list of shortcuts (a single list, which the tests compare with the keys handled). Checked by automated journeys in each window; *how it looks with the pen and a finger on the Surface, in the dark theme and in a narrow window has not been looked at on the PC yet*.
- **Projecting the editor's note**: the editor's **Projection** tab sends the page of the open note, full screen, to the projector, updated at every stroke (the stroke under way too), with the zoom, **Whole page** and **Width**; the projected area follows what the editor shows and a dashed frame shows it; the replay is projected too; it stops with its button, `Esc` or on leaving the editor. No blackboard, laser or marks on this projection. Checked by automated tests; *the stroke's delay with the Surface's pen and the smoothness on a real projector have not been tried yet (test H7)*.
- Automatic backup copy of unsaved changes, on the PC only: after a crash or a forced restart, Ardoise offers to
  recover them (onto the note if it has not changed, otherwise into a copy). At a Windows logoff or shutdown with
  unsaved work, Windows shows that Ardoise is preventing the shutdown, with the reason.

**Replaying and exporting**

- Animated replay (0.25× to 16×, dead time removable), narration with a microphone, recorded during a replay or, as you choose, **while you write in the editor**: the strokes you write take the time of the sound and are replayed with it, after the ones that already exist, on screen and in the video; a single narration per note, and recording again first asks whether to **Replace** or **Keep the old one**. *The synchronisation of voice and writing has not been measured with the Surface's real microphone yet (test H7).*
- Exports: PNG, JPG, WebP, SVG, PDF, text, Markdown and Word (with the structure drawn from the handwriting; the same
  words as the copied text, the assistant's corrections win, a line under words makes them bold in Markdown and
  underlines them in Word, **Include the drawings** option), MP4, GIF, WebP and WebM videos; clipboard copy, printing,
  dragging a note out to Explorer or Word.
- Assistant (MCP): Claude Code, Claude Desktop or another assistant can read notes, tidy them up, title and file them,
  and drive the open Ardoise; read-only mode available.

**Around it**

- French and English interface, light and dark themes, portable application (one executable, settings next to it), no
  administrator rights, no account, no connection except the optional OpenH264 download and the local network share
  started by hand.
- **Share on the network** (live mode and editor): another computer of the same network shows in a browser the live
  sheet or the page of the open note, and nothing else; address, QR code, 6-character code, pause, stop. The first
  time, Windows asks for the firewall permission (administrator rights): answer "Private networks".
- Distribution archive (`cargo xtask dist`): an `ardoise-<version>` folder with `ardoise.exe`, `LICENSE`,
  `THIRD-PARTY-NOTICES`, `LISEZMOI.txt` and the guides as HTML pages (`guide\`). An **About** screen (version, licence,
  credits, components).
- One window per copy of Ardoise: a second launch brings the open window to the front; two copies in two folders run
  side by side.
- Guided tablet test and diagnostic logs kept in the library; the pages downloaded during the test are also added to
  the library as notes before being deleted from the tablet.
- Note file format: version 1.2 (earlier versions are read).

## To try before relying on it

The tablet is not on the development PC: **everything that talks to the real Bamboo Slate has been checked with a
simulated tablet only**. The following trials are still to be made by the user, following the procedures in
`docs/hardware-tests.md` (in French); the logs come back through Nextcloud.

| Trial | What it must establish |
|---|---|
| **H0**: `ardoise.exe` on the work PC | Windows (SmartScreen, Smart App Control, AppLocker) accepts the unsigned executable, from the Nextcloud folder; the Bluetooth dongle is seen. |
| **H1**: the tablet talks to Ardoise | Registration, reading the information, downloading the pages and live mode work with a real Slate. **Page decoding has never been checked against a real Slate capture** (only against another tablet's data and synthetic data). |
| **H2**: sync, live mode, projector | Real sync, perceived latency, what the tablet sends when its button is pressed in live mode (unknown today), recovery after a drop, projecting on a real projector (and unplugging it), the four tablet positions. |
| **H3**: pen and touch; full trial in class | Pen eraser end, button, palm, finger gestures on a touch PC; then a whole lesson with the tablet. |
| **H4**: sharing on the network | A second computer of the network shows the live sheet, then an open note: firewall permission, address typed or QR code, perceived delay (under 150 ms expected), reconnection after a cut. |
| **H5**: resuming a sheet in live mode | With the real tablet, in class: interrupt a live session, resume the sheet (note menu or editor), write on it; a tablet whose size and position differ from the sheet's; background shown on the projector and on another screen of the network; then delete, recolour and undo strokes while the tablet writes, with nothing of the selection showing on the projector or on the other screen. |
| **H6**: correcting during live mode | With the real tablet and the Surface's pen, in class: a pupil writes while the teacher adds a "Correction" layer and writes on it in red, erases with the eraser end and undoes with `Ctrl + Z`; sending the tablet to another layer, hiding a layer (warning); smoothness, palm, Windows Ink marks, the tablet icon on pen hover; then a PDF export with separate layers opened in Acrobat Reader or Firefox. |
| **H7**: paper and background in live mode, projecting the editor, sound while writing | With the real tablet, the projector, the remote and the Surface's microphone: change the paper and put a PDF worksheet under the page while a pupil writes (jerks, delay, writing well placed on the worksheet, the remote turning the page after a choice in a list, the page no longer turning when the tablet is turned); project an editor note and write on it with the pen (stroke delay, projected area and dashed frame, replay, a single screen); record the sound while writing, replay with the narration and export the MP4 video (offset between voice and writing at the start and at the end, Windows' player and VLC), the **Replace** / **Keep the old one** question. |

Other hands-on checks remain, because automated tests cannot replace them:

- the side panel on the Surface: the rail and its 38 px icons with the pen, a finger and the mouse (tooltip on hover), the panel collapsed then opened again, the dark theme, the 800 × 560 window, the visible focus on the icons, Narrator reading the tabs and their badges, `Space` on an icon in live mode (it turns the page); the red dot during a real recording (the journeys have no microphone);
- handwriting recognition **in English** (the English handwriting module of Windows is not installed on the development
  PC) and the pause of handwriting reading during the presentation;
- a real drop of a note dragged into Explorer, Word or OneNote;
- printing on a real printer, a Word export opened in Word, Markdown and Word formatting on real handwriting;
- the assistant: the real trial from Claude Code (tidy up a note), the command under PowerShell 7, the presentation on
  a second screen driven by the assistant, the last live-mode note found again after a real restart;
- the Inkspace import from Ardoise's window, with Inkspace open and then closed (it may lock its files);
- a real Windows logoff with an unsaved note: the "Ardoise is preventing shutdown" screen, the backup copy written, its
  offer at the next start;
- tag colours with Nextcloud on two PCs (a change on one seen on the other; two simultaneous changes merged).

Performance measured on 29 October 2026 on an idle laptop (Intel Core i5-1235U, 16 GB): every target is met (start in
0.95 s, a 1,000-note library shown in 1.8 s and scrolled at 60 fps, live mode at 60 fps, MP4 1080p export of a 2-min
replay in 15 s, a 21 MB executable). Details: [performance](performance.md) (in French).

## Known limits

- **Windows only**: Windows 11 is the only version tried. No macOS, no Linux, no phone app.
- **Unsigned executable**: a SmartScreen warning at the first start is normal; a managed PC may refuse to run it.
- **Tablets**: Bamboo Slate (A4, the target model); Slate A5, Spark, Folio and Intuos Pro Paper are untested.
- The Bamboo Slate hands over only its **oldest page**: without the "Delete from the tablet after import" option (on by
  default), only one page is readable at a time.
- A tablet is registered with one application only: once registered in Ardoise, it no longer works with the phone app or
  with Inkspace until it is registered there again.
- **Inkspace import**: the pressure is estimated from the stroke width, and the timing of the writing is sometimes made
  up (the replay of those notes is only indicative); Wacom's cloud and Inkspace's collaboration do not exist in Ardoise;
  only the notes present on the PC are taken over. The import changes nothing in Inkspace.
- **Backup copies**: they only cover the changes of a note open in the editor, a few seconds after the last change; they
  stay on the PC (never in Nextcloud). A shutdown that Windows forces without waiting is not held back.
- **Handwriting recognition**: it depends on the handwriting module installed in Windows for the language, and can make
  mistakes; exported texts must be read over.
- **MP4**: it uses Windows' encoder; on a Windows "N" edition without the Media Feature Pack, you must download OpenH264
  from Cisco (on request) or choose WebM, which is much slower.
- **Narration recorded while writing**: the replay with this narration plays at normal speed, without cutting dead time; a
  note already written in class, then commented while writing, therefore replays its real pauses before the voice starts
  (in the application and in the video). A sound recorded while writing that could not be written into the note (it changed
  elsewhere) is not attached again after a crash; a recording under way when Ardoise closes is lost.
- **Live paper and background**: no going back with `Ctrl + Z` (you choose the previous setting again); no offer to switch
  the sheet to portrait or landscape for a worksheet (a live page follows the tablet).
- No text box typed on the keyboard in a note, no PSD export, no WILL / Universal Ink Model ink formats.
- Notes of a library must not be edited on two PCs at the same time (Ardoise refuses to overwrite the other version and
  offers a copy). To update `ardoise.exe` in a synced folder, close Ardoise on all PCs.
- The name "Ardoise" is a working name; it may change before publication.
