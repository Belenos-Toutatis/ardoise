# Questions ouvertes

Points à trancher avec l'utilisateur. Chaque question indique ce qui est fait en attendant.

## Q1 — Installer la cible WebAssembly de Rust ? (jalon M2, pour M3) — **tranchée**

SPEC §6.2 prévoit que le calcul du contour des traits (`ardoise-ink`) soit compilé en WebAssembly pour l'affichage à l'écran, ce qui demande la cible `wasm32-unknown-unknown` sur le PC de développement (rien chez l'utilisateur final).

**Décision de l'utilisateur (2026-10-24)** : installation autorisée pour M3. La cible est installée et `cargo build -p ardoise-ink --target wasm32-unknown-unknown` réussit sans modification du code. La commande `note_outlines` (contours calculés par l'application) reste disponible ; M3 choisira l'usage à l'écran.

## Q2 — Le dossier caché `.ardoise/` est-il synchronisé par Nextcloud ? (relecture de M2) — **tranchée**

Le client Nextcloud ne synchronise pas les dossiers cachés (nom commençant par un point) avec ses réglages par défaut ; or ce dossier contient l'enregistrement de la tablette partagé par les PC, les journaux de diagnostic et la corbeille.

**Décision de l'utilisateur (2026-10-24)** : le dossier s'appelle désormais `_ardoise/` (non caché, synchronisé sans réglage). SPEC §3.2 et §4.8 sont mis à jour. Une bibliothèque qui a encore `.ardoise/` est migrée automatiquement au démarrage d'Ardoise et à l'ouverture de la bibliothèque (renommage, ou fusion sans rien écraser si `_ardoise/` existe déjà).

## Q3 — Multiplexeur MP4 du secours OpenH264 : `shiguredo_mp4` au lieu de `muxide` ? (jalon M6) — **tranchée**

SPEC §6.2 (ligne « Vidéo MP4 ») prévoit que le MP4 du secours OpenH264 soit « multiplexé par `muxide` ». À l'essai, `muxide` 0.5.0 ne convient pas :
- le délai de démarrage du son Opus y est figé à 312 échantillons ;
- il n'écrit pas la liste d'édition qui fait finir le son exactement avec la narration ;
- la dernière image reprend la durée de l'image précédente ;
- il garde toutes les données en mémoire jusqu'à la fin.

`shiguredo_mp4` 2026.5.0 (Apache-2.0, sans dépendance) fait tout cela et écrit les données au fil de l'eau.

**En attendant** : le secours utilise `shiguredo_mp4`. Le test de fin de M6 relit le fichier produit avec un lecteur indépendant (`mp4parse`, celui de Firefox), et Media Foundation le lit aussi.

**Décision de l'utilisateur (2026-10-24)** : `shiguredo_mp4` est retenu ; SPEC §6.2 est mis à jour (ligne « Vidéo MP4 » et risque des bibliothèques vidéo jeunes).

## Q4 — Afficher la feuille sur un autre ordinateur du réseau local ? (2026-11-02) — **tranchée**

L'utilisateur demande qu'un autre ordinateur du même réseau puisse afficher la feuille en direct, ou la feuille consultée. SPEC §6.2 interdisait tout port en écoute, parce que l'alerte du pare-feu de Windows demande des droits d'administrateur.

**Décision de l'utilisateur (2026-11-02)** : c'est autorisé.
- Le partage tourne sur un ordinateur personnel dont l'utilisateur est administrateur.
- Le réseau de son établissement laisse déjà un appareil afficher son écran sur le PC du bureau.
- L'autre ordinateur n'affiche jamais la bibliothèque, seulement la feuille du mode direct ou la feuille consultée ou modifiée ; c'est toujours le PC d'Ardoise qui choisit.
- Les protections proposées sont retenues : partage lancé à la main, lecture seule, code d'accès, arrêt à la fermeture.

SPEC §4.10 est ajouté, la ligne « Réseau » du §6.2 est mise à jour, et le jalon M11 est ajouté à ROADMAP.md.

## Q5 — Ajouter `ardoise-share` à la liste des crates de SPEC §6.1 ? (jalon M11) — **tranchée**

Le serveur du partage sur le réseau local (§4.10) est une nouvelle bibliothèque, `crates/ardoise-share`, indépendante de Tauri pour être testée sous Linux avec un vrai client TCP (`docs/design/lan-share.md`). SPEC §6.1 ne la cite pas, et SPEC.md ne se modifie pas sans l'accord de l'utilisateur.

**En attendant** : la crate existe, son rôle est écrit en tête de son `lib.rs` et dans `docs/design/lan-share.md`. Ligne proposée pour §6.1 : `ardoise-share     partage de la feuille sur le réseau local : petit serveur HTTP en lecture seule (code, flux d'évènements)`.

**Décision de l'utilisateur (2026-11-02)** : accord ; la ligne proposée est ajoutée à SPEC §6.1.

## Q6 — Montrer sur l'autre écran les annotations écrites par-dessus la présentation ? (jalon M11, 2026-11-02) — **tranchée**

La relecture de M11 a relevé que les annotations écrites au stylet, au doigt ou à la souris sur la fenêtre de présentation (rouges, jaunes sur le tableau noir, temporaires, jamais enregistrées dans la note) restaient sur le PC : le partage ne montrait que l'écriture de la tablette et la vue (zoom, déplacement, tableau noir, pointeur laser).

**Décision de l'utilisateur (2026-11-02)** : les montrer aussi sur l'autre écran, pour qu'il affiche exactement ce que montre le vidéoprojecteur. SPEC §4.10 et ROADMAP M11 sont mis à jour.

## Q7 — Que faire quand le port du partage est déjà pris ? (jalon M11, 2026-11-02) — **tranchée**

Sur le PC de l'utilisateur, le port 8765 était pris par un autre programme (l'aide Python d'un serveur MCP pour
LibreOffice, qui démarre avec Claude Desktop) : le partage refusait de démarrer et renvoyait vers `ardoise.toml`.

**Décision de l'utilisateur (2026-11-02)** : les deux propositions.
- Si le port est pris, Ardoise essaie les ports suivants et prend le premier libre ; le panneau du partage le dit, et le
  port trouvé est retenu pour ce PC (section de la machine dans `ardoise.toml`), pour que l'adresse reste la même.
- Un réglage « Port du partage » dans les réglages d'Ardoise, pour choisir un port sans modifier `ardoise.toml` à la main.

SPEC §4.10 (ligne « Réseau ») est mis à jour.

## Q8 — Laisser l'autre écran régler son affichage ? (jalon M11, 2026-11-03) — **tranchée**

Premier essai du partage par l'utilisateur, sur deux ordinateurs de son réseau à la maison (2026-11-03) : il fonctionne.
Mais sur l'ordinateur qui reçoit l'image, on ne peut rien faire, alors qu'il serait utile de mieux voir la page.

**Décision de l'utilisateur (2026-11-03)** : l'autre écran a des options d'affichage pour lui seul :
- page entière et pleine largeur ;
- zoom sur un endroit, et déplacement dans la page quand elle n'est pas affichée en entier.

Ces réglages ne changent rien sur le PC d'Ardoise, ni le contenu partagé : c'est toujours le PC qui choisit ce qui est
montré. Pendant le mode direct, l'autre écran suit la vue de la présentation tant qu'on n'y touche pas, et un bouton l'y
ramène. SPEC §4.10 est mis à jour.

## Q9 — Reprendre le direct sur une feuille, et retoucher pendant le direct (jalon M12, 2026-11-04) — **tranchée**

L'utilisateur demande de pouvoir reprendre le mode direct sur une feuille existante après une interruption, et de sélectionner quelque chose pendant le direct pour le supprimer ou changer sa couleur, sans passer par l'éditeur.

**Décisions de l'utilisateur (2026-11-04)** :
- On sélectionne dans la **fenêtre de commande seulement** : rien de la sélection n'apparaît au vidéoprojecteur, seul le résultat.
- La reprise écrit dans la **même note**, dont l'état d'origine reste récupérable.
- On reprend **une feuille** (depuis le menu d'une note ou l'éditeur), pas un cours entier.
- **Ctrl+Z** annule la suppression, le changement de couleur et l'effacement de la page, pas l'écriture de la tablette.

**Choix par défaut, à contester si besoin** (ils ne changent pas la demande) :
- La page suivante, après une feuille reprise, crée une note neuve.
- Si la feuille n'est pas tenue comme la tablette (portrait ou paysage), le direct suit la feuille pour cette session : la note n'est jamais tournée, et le bouton pour tourner est désactivé sur cette page.
- Si la feuille n'a pas le format de la tablette, l'écriture est mise à l'échelle pour tenir entière dans la feuille, centrée et sans déformation.
- Si la feuille a changé ailleurs pendant le direct, une nouvelle note est créée dans le même classeur, et Ardoise le dit.
- Le vidéoprojecteur et les écrans du partage montrent le fond de la feuille reprise (modèle, texture, image ou page de PDF), et jamais ses calques masqués.
- Déplacer une sélection reste réservé à l'éditeur.
- Une pastille de couleur, quand une sélection existe, ne change que la sélection : la couleur du stylet ne change pas (dans l'éditeur, elle change aussi).
- Une note reprise qui n'a pas encore d'original garde son état d'avant la reprise, même vierge.

SPEC §4.2 est mis à jour et le jalon M12 est ajouté à ROADMAP.md. Conception : `docs/design/tablet-live-classroom.md`, « M12 ».

## Q10 — L'assistant retouche une copie d'une note (jalon M13, 2026-11-06) — **tranchée**

L'utilisateur demande que l'IA puisse récupérer les documents et les améliorer sur une copie, sans perdre l'original : améliorer les tracés, mettre des parties en couleur, améliorer l'écriture, corriger les fautes. Le pilotage du mode direct ne l'intéresse pas davantage. Un essai sur quatre de ses notes (`p.95 n°21`, `p.99 n°32`, `p.114 n°14`, `fiche incident`) l'a convaincu.

**Décisions de l'utilisateur :**
- Écriture : niveau 1 (l'écriture de la note, mieux rangée). Pas de police de remplacement, pas d'écriture recomposée.
- Une lettre recopiée prend sa place dans le temps, ou les traits suivants se décalent.
- S'il n'y a pas de lettre à recopier, la fabriquer à partir de formes semblables dans les lettres existantes.
- Jamais de lettre prise dans une autre note : il confie parfois son calepin à un élève.

**Choix par défaut, à contester si besoin :**
- Avec une narration, rien ne se décale dans le temps (l'écriture et la voix se décaleraient) : la lettre est tracée plus vite.
- Les fautes sont listées et validées avant d'être corrigées ; ce qui ne peut pas être corrigé proprement est marqué en couleur.
- La copie s'appelle « Titre (propre) », dans le même classeur, avec la même date de création : elle se range à côté de l'original.
- Un aperçu ne demande aucune écriture ; `preview_revision` n'est pourtant proposé qu'avec les outils d'écriture (absent en `--read-only`).
- `straighten_shapes` crée désormais une copie au lieu de modifier la note et d'en mettre l'ancienne version à la corbeille.
- La copie garde la narration de l'original.
- `split_note` ne réécrit plus la note d'origine : découper une note ordinaire donne deux notes neuves, et l'original reste entier. Sinon, une IA pouvait retirer un mot de l'original en le découpant, puis en jetant le morceau. Découper dans l'éditeur, à la main, ne change pas.
- Les ordres de retouche portent la version de la note qu'ils visent : si la note a changé depuis l'aperçu, ils sont refusés.

SPEC §4.9 et §6.1 sont mis à jour et le jalon M13 est ajouté à ROADMAP.md. Conception : `docs/design/revision.md`.

## Q11 — Corriger pendant le direct, sur son propre calque (jalon M14, 2026-11-11) — **tranchée**

En classe, l'utilisateur confie la tablette à un élève qui écrit son exercice, projeté en direct par le partage sur le réseau. Il demande de pouvoir corriger en même temps sur son ordinateur (Surface Pro 9), avec son stylet et dans une autre couleur. Aujourd'hui la fenêtre de commande sait seulement sélectionner, supprimer et recolorer, et la sélection prend les traits de tous les calques.

**Décisions de l'utilisateur (2026-11-11)**, à partir d'une maquette qu'il a validée :
- Ce que l'enseignant écrit est enregistré dans la note, sur un calque à part (par exemple « Correction »), pas mêlé à l'écriture de l'élève ni temporaire.
- L'ordinateur agit sur le **calque choisi** : écrire, modifier, gommer, sélectionner. La tablette écrit toujours sur **son** calque. Pour modifier ce qu'a fait l'élève, on choisit son calque.
- Zone Calques : un clic sur la ligne choisit le calque (plus de bouton rond) ; un œil, barré quand le calque est masqué, remplace la case « Afficher » ; une icône crayon après le nom le renomme ; une icône de tablette marque le calque où écrit la tablette.
- Export : pour les formats qui le permettent, choisir entre calques séparés et fusionnés.
- Confirmé ensuite (2026-11-11), sur trois propositions : la sélection ne prend que le calque choisi, dans l'éditeur aussi, pour que la règle soit la même partout ; au début du mode direct, l'ordinateur est sur le calque de la tablette, et l'enseignant ajoute son calque quand il en a besoin ; à l'export, le SVG garde les calques séparés par défaut et le PDF fusionnés, comme aujourd'hui.

**Choix par défaut, à contester si besoin** (proposés avec la maquette, sans objection) :
- « Ajouter un calque » crée un calque en haut et le choisit pour l'ordinateur ; la tablette reste sur le sien. On envoie la tablette sur un autre calque avec l'icône de tablette pâle qui apparaît au survol de sa ligne. Le bouton de la tablette, réglé sur « nouveau calque », crée un calque et y envoie la tablette, l'ordinateur restant où il est. Le bouton « Nouveau calque » de la barre d'outils du direct est remplacé par la zone.
- Monter, descendre et fusionner avec le calque du dessous restent, sur la ligne choisie seulement.
- Un avertissement apparaît si la tablette écrit sur un calque masqué (rien ne serait projeté ni partagé), ou si le calque choisi est masqué.
- Le style de l'ordinateur est séparé de celui de la tablette : rouge par défaut, encre et épaisseur au choix, retenus d'une séance à l'autre.
- Le bout gomme du stylet efface des traits entiers du calque choisi. Ctrl+Z annule les actions de l'ordinateur (traits, gomme, retouches, effacement de la page), jamais l'écriture de la tablette.
- Avec l'outil Stylo, le stylet et la souris écrivent, le doigt déplace et zoome la zone projetée, comme aujourd'hui.
- À la relecture, les traits de l'ordinateur apparaissent à l'heure où ils ont été écrits, mêlés à ceux de l'élève.
- Les annotations temporaires de la fenêtre de présentation ne changent pas.
- Export : le dernier choix est retenu. Les calques PDF de même nom ne font qu'un seul interrupteur pour toutes les pages d'un export de plusieurs notes. Un calque masqué n'est jamais exporté, même en séparé : il peut contenir les réponses. L'export par l'assistant (`export_note`) ne change pas.

SPEC §4.2, §4.4 et §4.6 sont mis à jour et le jalon M14 est ajouté à ROADMAP.md.

## Q12 — Régler « Redresser les formes » (jalon M15, 2026-11-11) — **tranchée**

L'utilisateur demande un réglage de la sensibilité de la détection des formes. Aujourd'hui les seuils sont fixes (`crates/ardoise-ink/src/shapes.rs`) : une droite ou une flèche d'au moins 1 cm, une forme fermée d'au moins 1,3 cm, un écart limité à la forme idéale, et un alignement automatique à 4° de l'horizontale ou de la verticale, et à 10 % d'un cercle ou d'un carré.

**Décisions de l'utilisateur (2026-11-11)**, à partir d'une maquette qu'il a validée (`target/maquettes/maquette-redresser.html`, hors de git) :
- Trois réglages : la tolérance, la taille minimale et l'alignement automatique.
- Le réglage est à côté du bouton « Redresser les formes » : un petit bouton à droite du bouton (icône de réglage) ouvre un panneau.
- Le panneau doit pouvoir se voir en entier : s'il manque de place, il défile à la molette ou au doigt.

**Choix par défaut, proposés avec le texte du SPEC et acceptés** (« Oui, lance-le ») :
- Tolérance sur cinq crans, de « stricte » à « souple » ; le cran du milieu, « normale », est le réglage d'aujourd'hui. Taille minimale de 5 à 25 mm (10 mm aujourd'hui) pour une droite ou une flèche, une forme fermée demandant 3 mm de plus ; un avertissement sous 10 mm, car des lettres peuvent alors être redressées. Alignement : une case pour rendre horizontal ou vertical ce qui l'est presque, jusqu'à 1 à 10° (4°), une case pour faire un cercle ou un carré de ce qui est presque rond ou carré, à moins de 2 à 20 % (10 %).
- « Valeurs par défaut » remet exactement les réglages d'aujourd'hui, et le redressement donne alors les mêmes résultats qu'avant.
- Pendant que le panneau est ouvert et qu'il y a une sélection, la page montre en pointillés ce qui serait redressé, et le panneau dit combien de formes sont reconnues. « Redresser » applique ; Ctrl+Z annule comme aujourd'hui. Échap ferme le panneau sans redresser ; les valeurs choisies restent.
- Le panneau ne dépasse jamais le bas de la fenêtre : s'il manque de place, il défile, et ses boutons restent visibles en bas.
- Les valeurs sont retenues dans les réglages d'Ardoise (`ardoise.toml`, section `[shapes]`) : les mêmes pour toutes les notes, et sur chaque PC qui utilise cette copie. Une valeur hors des limites est ramenée dans les limites.
- Le bouton « Redresser les formes » du bloc Sélection et la touche F redressent avec les mêmes valeurs.
- L'assistant (`straighten_shapes`) redresse avec les mêmes valeurs ; on peut lui en donner d'autres pour un appel, et sa réponse dit les valeurs utilisées.
- Les protections de l'écriture restent, quel que soit le réglage : un petit ovale debout (un O, un 0) de moins de 2,5 cm n'est jamais redressé.

SPEC §4.4 est mis à jour et le jalon M15 est ajouté à ROADMAP.md.

## Q13 — Le panneau de côté à onglets, et trois fonctions qu'il fait apparaître (jalons M16 et M17, 2026-11-11) — **tranchée**

L'utilisateur trouve que la barre latérale droite pourrait être modernisée, « avec des petits onglets comme dans Photoshop ». Une première maquette (`target/maquettes/maquette-barre-laterale.html`, hors de git) proposait un rail d'icônes différent pour l'éditeur et pour la fenêtre de commande du direct.

**Décisions de l'utilisateur (2026-11-11)** :
- Des changements à la première maquette : les deux fenêtres doivent partager les mêmes onglets. Aide, Son et Projection aussi dans l'éditeur, Page aussi en mode direct ; « je ne vois pas ce qui empêche les boutons d'être dans les deux modes ».
- Enregistrer le son aussi pendant qu'il dessine sur l'écran tactile de sa Surface, pas seulement avec la tablette Wacom.
- La maquette mise à jour (le même rail dans les deux fenêtres : Calques, Page, Fond, Son, Projection, Aide) lui convient.

**Découpage et choix par défaut, proposés avec le texte du SPEC et acceptés** (« Oui, lance-le ») :
- Deux jalons : M16 range dans le rail tout ce qui existe déjà ; M17 ajoute les trois fonctions nouvelles (papier et fond pendant le direct, projection de l'éditeur, son pendant qu'on écrit dans l'éditeur). Jusqu'à M17, ce qui n'existe pas encore n'apparaît pas.
- L'onglet ouvert et l'état replié sont retenus pour chaque fenêtre, sur chaque PC. Le direct s'ouvre sur Calques.
- Le partage sur le réseau passe dans l'onglet Projection, dans les deux fenêtres ; le bouton « Partager sur le réseau » de la barre d'outils de l'éditeur disparaît, un point bleu sur l'icône Projection dit qu'un partage (ou une projection) est en cours.
- « Revenir à l'original » passe dans l'onglet Page de l'éditeur.
- L'onglet Aide montre les vrais raccourcis de la fenêtre, tirés de l'application elle-même (une seule liste pour l'aide et pour les touches), en français et en anglais.
- Papier et fond en direct : le changement vaut pour la page en cours ; une nouvelle page garde le papier de la précédente, pas l'image de fond ; tout est enregistré dans la note ; on revient en arrière en rechoisissant l'ancien réglage, pas avec Ctrl+Z (réservé à l'écriture de l'ordinateur).
- Projection de l'éditeur : la page telle qu'elle est dans l'éditeur (papier, fond, calques visibles), mise à jour à chaque trait, avec le zoom et « Page entière » / « Largeur » ; le tableau noir et le pointeur laser restent propres au direct ; elle s'arrête avec son bouton ou quand on quitte l'éditeur.
- Son pendant qu'on écrit dans l'éditeur : pendant l'enregistrement, les traits écrits dans l'éditeur (stylet, souris ou doigt) prennent l'heure du son et se rejouent avec lui, après les traits déjà présents. Une note n'a qu'un seul son, comme aujourd'hui : en enregistrer un autre demande d'abord si l'on remplace le premier.

SPEC §4.2, §4.4, §4.5 et §5 sont mis à jour ; les jalons M16 et M17 sont ajoutés à ROADMAP.md.

## Q14 — Une seconde tablette : la Xiaomi Mijia « petit tableau noir, version mémoire » (2026-11-12) — **tranchée**

L'utilisateur possède aussi une tablette Xiaomi 13,5 pouces Bluetooth (米家液晶小黑板 存储版, modèle Mi Home `jiqid.sxb.v1`) et voudrait qu'Ardoise récupère ses pages et fasse du direct avec elle. SPEC.md ne prévoit que la Bamboo Slate, et le protocole de cette tablette n'est documenté nulle part.

**En attendant** : une étude de faisabilité, acceptée par l'utilisateur le 2026-11-12 (« Oui, lance-la »), sans rien changer à l'application ni à SPEC.md. Ses résultats sont dans `docs/research/xiaomi-blackboard.md` ; l'outil de relevé est `cargo run -p ardoise-ble --example survey`. Premiers relevés sur la vraie tablette : elle se laisse connecter depuis le PC sans identification Mi Home et envoie des messages en clair sur un canal à part ; ses commandes restent à apprendre, par une capture de ce que Mi Home lui envoie depuis le téléphone Android de l'utilisateur. Aucune commande devinée n'est écrite sur la tablette.

**Résultat de l'étude (2026-11-12)** : c'est faisable, pour les deux demandes.
- La tablette obéit à ses propres commandes, en clair, sans identification auprès de Xiaomi : elle reste liée à Mi Home, et Ardoise n'a besoin ni du compte Xiaomi ni d'un enregistrement.
- Le mode direct a été démarré depuis le PC : 3 176 points reçus pendant que l'utilisateur écrivait, et le tracé reconstitué correspond à ce qu'il a écrit (confirmé par lui).
- Les pages enregistrées se téléchargent une par une, de la plus ancienne à la plus récente, puis s'effacent une par une ; une page a le même format que le direct. Vu dans une capture de Mi Home ; pas encore rejoué depuis le PC.
- Les neuf pages que la tablette contenait ont été importées par Mi Home pendant la capture (qui les a effacées de la tablette, comme il le fait toujours) ; leurs données brutes sont gardées dans `testdata/private/xiaomi/pages/`.

**Proposition, acceptée par l'utilisateur le 2026-11-12** (« Oui, ajoute-la ») ; SPEC §1, §2, §3.2, §4.1, §4.2, §4.8, §4.11 et §6 sont mis à jour et le jalon M18 est ajouté à ROADMAP.md :
- *Ajouter la tablette* : « Ajouter une tablette » trouve aussi la Xiaomi quand elle est allumée et que Mi Home est fermé. Pas d'appui long ni d'enregistrement. Ardoise retient les deux tablettes et se connecte à celle qui est allumée.
- *Récupérer les pages* : comme pour la Bamboo, à la connexion ou à la demande ; chaque page devient une note datée du moment où elle a été enregistrée sur la tablette ; même option « effacer de la tablette après import réussi ».
- *Mode direct* : comme pour la Bamboo (outil, couleur, texture, pression, autre écran, partage, retouches).
- *Informations* : batterie, mémoire libre, nombre de pages ; mise à l'heure à chaque connexion.
- *Les neuf pages déjà récupérées* deviennent des notes de la bibliothèque, par un import fait une fois.
- *Limites à écrire dans le guide* : la tablette ne parle qu'à une application à la fois (fermer Mi Home, ou couper le Bluetooth du téléphone) ; les pages vont à l'application qui les importe la première ; la tablette ne donne pas l'heure de chaque point, donc la relecture et la vidéo d'une page rejouent les traits à vitesse régulière ; la page fait environ 20 × 27 cm, en hauteur.
- *Tests* : une tablette Xiaomi simulée dans `ardoise-sim` et les captures rejouées, comme pour la Bamboo ; essais réels dans `docs/hardware-tests.md`.

**Précision ajoutée en écrivant le SPEC, confirmée par l'utilisateur le 2026-11-12** (« Oui, d'accord ») : la tablette ne donne que sa page la plus ancienne, donc lire la suivante oblige à effacer la première. Ardoise efface chaque page après l'écriture vérifiée de sa note, comme Mi Home ; si l'option « effacer de la tablette après import réussi » est désactivée, les pages de cette tablette ne sont importées qu'à la demande, après l'avoir dit.

**Reste à mesurer avant ou pendant M18** : le téléchargement et l'effacement d'une page d'essai depuis le PC ; ce qu'envoie le bouton d'effacement de l'écran (pour savoir si le direct doit passer à une nouvelle page) ; l'unité exacte des coordonnées, à la règle.

## Q15 — Trois choix faits en construisant la tablette Xiaomi (jalon M18, 2026-11-13) — **tranchée le 2026-11-13**

En construisant M18, trois comportements ont dû être fixés que le SPEC ne dit pas. Ils sont dans le code, les guides et les tests ; le SPEC n'est pas modifié tant que l'utilisateur ne les a pas confirmés.

1. **Reprendre une feuille en direct** (SPEC §4.2) : une feuille de la Bamboo Slate se reprend avec la Bamboo Slate, une page de la Xiaomi avec la Xiaomi. Dans l'autre sens Ardoise refuse (« Cette feuille ne peut pas être continuée avec cette tablette… ») et n'écrit rien sur la note. Raison : les deux tablettes n'ont pas la même surface (29,7 × 21,6 cm et 20,5 × 27,2 cm) ; avant ce choix, l'écriture de l'autre tablette était rétrécie (à environ 80 % ou 92 %) et parfois tournée d'un quart de tour pour tenir dans la feuille. Autre possibilité, non retenue : placer l'écriture à sa taille réelle depuis le coin de la feuille et refuser seulement quand elle déborderait.
2. **Un direct reste sur sa tablette** : si la liaison se perd, Ardoise attend la tablette avec laquelle le direct a commencé et ignore l'autre ; pour continuer avec l'autre, on termine le direct et on le relance. Pendant un direct, on ne peut pas ajouter la Xiaomi ; pendant un direct qui attend la Xiaomi, on ne peut pas enregistrer la Bamboo Slate. Raison : une page du direct a les dimensions de sa tablette.
3. **Synchroniser avec « effacer de la tablette après import réussi » désactivé** (SPEC §4.1 : « Ardoise n'importe les pages de cette tablette qu'à la demande, après avoir dit qu'elles seront effacées ») : la demande ne vaut que si elle est confirmée pendant que la Xiaomi est la tablette connectée. « Synchroniser maintenant » cliqué alors qu'elle n'est pas connectée n'importe donc pas ses pages plus tard, quand elle se connecte : le panneau dit alors combien de pages attendent, et le bouton passe par la confirmation. Raison : sans cela, un clic destiné à la Bamboo Slate pouvait effacer les pages de la Xiaomi sans avertissement.

**Décision (2026-11-13)** : l'utilisateur confirme les trois choix, après une seconde explication avec des exemples (la première n'était pas claire). SPEC §4.2 (points 1 et 2) et §4.1 (point 3) mis à jour avec son accord.

## Q16 — Xiaomi : ce que les premiers essais sur la vraie tablette ont montré (2026-11-13) — **ouverte**

Premiers essais de l'utilisateur avec la vraie tablette, puis mesures faites avec lui devant la tablette (`docs/research/xiaomi-blackboard.md` §6). Le téléchargement et l'effacement depuis le PC fonctionnent. Trois comportements de la tablette, absents des captures de Mi Home, changent ce qu'Ardoise doit faire.

**Mesuré.**

1. La page d'un écran **pas encore effacé** est décrite en 12 octets au lieu de 8 (Ardoise la refusait : « échec d'importation » à chaque fois ; corrigé). Le troisième nombre est l'heure du précédent enregistrement du même écran.
2. **La tablette retire d'elle-même une telle page dès qu'elle est téléchargée**, sans attendre l'ordre d'effacement. C'est le cas courant : on écrit, on enregistre, on synchronise sans avoir effacé l'écran.
3. Enregistrer de nouveau un écran non effacé remplace sa page par une page qui contient tout l'écran.
4. Le bouton d'effacement, pendant le direct, envoie `11 01 0c` (Ardoise passe à une nouvelle page ; corrigé). Il peut aussi faire **envoyer toute la page** par la tablette sur le canal du tracé, ou la faire **disparaître** : une page enregistrée avant le direct et pas encore récupérée a été perdue quand l'écran a été effacé pendant le direct (deux fois).
5. La tablette ne reste pas en mode direct après une coupure de la liaison.

**À faire dans Ardoise** (dans le sens du SPEC, sans le modifier).

- a. Ne jamais laisser tomber les octets d'une page que la tablette a retirée : si la note ne peut pas être écrite ou vérifiée, ou si le transfert est douteux, garder une copie de secours et la reprendre à la synchronisation suivante. Aujourd'hui ces cas supposent que la page est restée dans la tablette, ce qui est faux pour ces pages.
- b. Pendant le direct, ne pas tracer une page que la tablette renvoie à l'effacement (elle arrive par gros paquets, que le stylet ne produit jamais).
- c. Avant de lancer un direct, récupérer les pages de la tablette quand la synchronisation automatique et l'effacement après import sont activés.

**À décider par l'utilisateur.**

- d. **Tranché le 2026-11-13 : demander d'abord de les importer.** Quand « Synchroniser automatiquement » ou « Effacer de la tablette après import » est désactivé et que la tablette contient des pages, Ardoise ne lance pas le direct : elle demande d'abord de les importer (elles seront effacées de la tablette), puis lance le direct. SPEC §4.2 mis à jour avec l'accord de l'utilisateur.
- e. **Tranché le 2026-11-13 : mettre à jour la première note.** Quand un écran est enregistré une seconde fois sans avoir été effacé, la nouvelle page contient l'ancienne, et la tablette donne l'heure de la version précédente. Ardoise met à jour la note de cette version si elle n'a pas été modifiée depuis son import (titre, mots-clés et classeur mis à part) et si la nouvelle page la contient en entier ; sinon elle ajoute une note. SPEC §4.1 mis à jour avec l'accord de l'utilisateur.

**Pas encore mesuré** : une page dont le téléchargement est coupé en route est-elle retirée quand même ; un transfert s'arrête-t-il quand la liaison se ferme ; un appui sur le bouton d'enregistrement avec un écran vide ; ce que devient la page d'un écran non effacé quand on éteint la tablette ; l'unité (une règle).

**Fait le 2026-11-13, à vérifier sur la vraie tablette (H8, « C quater »)** : a (copies de secours dans `_ardoise\rescued-pages`, ou sur le PC ; transferts douteux gardés à part, jamais transformés en note), b (ce que la tablette pousse pendant le direct est mis de côté), c et d (le direct récupère les pages d'abord, ou ne démarre pas : code `blackboardPagesFirst` ; la fenêtre qui pose la question reste à faire dans l'interface, `docs/design/blackboard.md` §9.5 dit quoi appeler), e (mise à jour de la note). Pour ce qui n'est pas mesuré, Ardoise prend le parti qui ne peut pas perdre de page : une page au transfert coupé est copiée à part, et le simulateur suppose le cas défavorable (une page enregistrée avant le direct et pas récupérée est perdue à l'effacement). Conception : `docs/design/blackboard.md` §9.

## Q17 — Panneau « Tablette » à onglets et positions de la Xiaomi (2026-11-13) — **tranchée le 2026-11-13**

Retour de l'utilisateur après ses premiers essais : le panneau de la tablette est trop long, il faut le faire défiler pour trouver une fonction ou une information ; et, la Xiaomi connectée, les dessins de « Position de la tablette » sont ceux de la Bamboo.

**Décision** (maquette `target/maquettes/maquette-panneau-tablette.html`, validée par « Oui, construis-la ») :

1. Le panneau devient étroit, avec trois onglets en icônes sur un rail, comme le panneau de côté de l'éditeur : **Utiliser** (la tablette connectée, son état, « Mode direct », « Synchroniser », les messages importants), **Réglages** (ceux de la tablette choisie dans une liste, la tablette connectée par défaut), **Tablettes** (les tablettes connues, leurs informations, « Oublier », « Ajouter une tablette »). Une pastille sur l'icône « Utiliser » dit combien de pages attendent.
2. Les réglages sont ceux de la tablette choisie : pour la Bamboo, ses quatre positions avec ses dessins, le rôle de son bouton en direct, son nom, sa mise à l'heure ; pour la Xiaomi, ses quatre positions avec ses propres dessins (corbeille et disquette).
3. **La Xiaomi se tient dans quatre positions** (boutons en bas, en haut, à droite, à gauche), l'utilisateur pouvant la tenir dans n'importe quel sens. Ardoise redresse l'écriture, pour les pages importées comme pour le direct. SPEC §4.1 mis à jour (il disait que cette tablette n'avait pas d'orientation).

## Q18 — SPEC §4.2 : une page de la Xiaomi peut être en largeur (2026-11-13) — **tranchée le 2026-11-13**

Avec les quatre positions (Q17), une page de la Xiaomi est en largeur quand la tablette est tenue boutons à droite ou à gauche. SPEC §4.2 (« Reprendre le direct sur une feuille ») dit encore « une page de la Xiaomi (environ 20 × 27 cm, en hauteur) » : il fallait « en hauteur ou en largeur ». **Décision** : l'utilisateur accepte (« Oui, écris-le ») ; SPEC §4.2 dit maintenant « en hauteur ou en largeur » et que la page doit être dans le même sens que la position réglée, sinon Ardoise refuse.

**Fait le 2026-11-13** (`docs/design/blackboard.md`, « Positions, the Rust side », resuming a sheet) : une page de la Xiaomi, en hauteur ou en largeur, se reprend avec la Xiaomi seulement (jamais avec la Bamboo Slate). L'écriture ajoutée est redressée pour la position choisie au début du direct, comme une page neuve, et non pour celle avec laquelle la page a été faite (sinon, la tablette tenue dans l'autre sens, l'écriture ajoutée serait à l'envers). Si la page n'est pas dans le sens où la tablette est tenue (page en largeur, tablette tenue en hauteur, ou l'inverse), Ardoise refuse, sans rien écrire sur la note, et dit de choisir une position dans le sens de la page (code `sheetBoardPosition`).

## Q20 — Fonctions d'un logiciel de prise de notes, décidées une par une (2026-11-13) — **tranchée le 2026-11-13**

L'utilisateur a demandé ce qu'on pourrait ajouter à Ardoise, puis a tranché chaque proposition. SPEC §4.3, §4.4, §4.6 et §8 mis à jour avec son accord.

**Retenu**

1. **Dupliquer une note** : clic droit sur une carte et barre de sélection ; même classeur, **même date** que l'originale (son choix), titre suivi de « (copie) », pas en favori ; le son (Opus, environ 0,5 Mo par minute) et l'original de la tablette sont copiés. **« Changer la date… »** au clic droit : aujourd'hui, ou une date réglée.
2. **Échap dans l'éditeur**, par étapes : action en cours, puis panneau ou outil ouvert, puis sélection, puis enregistrer et revenir à la bibliothèque ; pas pendant un enregistrement sonore.
3. **Classeur à ordre libre** (à la place des notes de plusieurs pages, son idée) : tri « Ordre du classeur », glisser-déposer entre les cartes ; ordre rangé dans un fichier du classeur ; note qui arrive à la fin ; déposée sur un classeur elle change de classeur, entre deux cartes elle change de place ; chaque classeur a son ordre.
4. **Outils de tracé** : ligne, rectangle, ellipse ; trois boutons à bascule (forme régulière, depuis le centre, aimanter), Maj et Alt au clavier ; **pas de redressement automatique** (« des fois on n'a pas envie que ce soit dressé ») ; la ligne régulière se cale sur l'horizontale, la verticale ou une diagonale ; aimantation selon le modèle de la page ; dans l'éditeur seulement.
5. **Signets**, sous la forme d'un **sommaire du classeur** (« Ce serait utile si ça permettait de chercher dans un classeur entier ») : posés sur la page, trouvés par la recherche, signets du PDF du classeur, absents des exports image et vidéo.
6. **Outil Texte** : deux polices fournies, **Latin Modern** (la police de LaTeX) et **Andika** (sa demande) ; accord donné pour télécharger leurs fichiers sur leurs sites officiels (GUST, SIL) et les inclure.
7. **Outil Équation** en code LaTeX (son idée), rendu en dessin vectoriel gardé dans la note avec son code ; accord donné pour ajouter **MathJax** (Apache 2.0) au projet.
8. **Exporter un classeur en PDF** au clic droit, et **impression** en 1, 2 ou 4 pages par feuille ; les sous-classeurs sont **une option** dans les deux cas (sa précision).
9. **Limite de taille de l'exécutable : 40 Mo** au lieu de 30 (« On passe la limite à 40 mégaoctets »).

**Écarté** : modèles de page (« Les modèles me conviennent très bien comme c'était maintenant »), liens entre notes, recherche dans le texte des PDF de fond, historique des versions, notes de plusieurs pages.

**Corrigé au passage** (défauts qu'il a signalés) : Ctrl+A et Échap dans la bibliothèque où que soit le focus, Maj+clic sur les cases, et le glisser-déposer d'une note vers un classeur, refusé par la fenêtre.
