# Guide d'utilisation d'Ardoise

Ardoise est une application pour Windows qui remplace Wacom Inkspace pour la tablette *Bamboo Slate*. Elle récupère
par Bluetooth les pages que vous écrivez sur la tablette, vous laisse écrire en direct à l'écran (et au
vidéoprojecteur, en classe), range vos notes dans un dossier que vous choisissez, et permet de les retoucher, de les
rejouer et de les exporter.

Ce guide suit l'ordre dans lequel on rencontre les choses. Les noms des boutons et des menus sont écrits en gras,
tels qu'ils apparaissent à l'écran. English version: [guide.en.md](guide.en.md).

> *Où en est cette première version ?* Tout ce qui ne demande pas la tablette (bibliothèque, édition, relecture,
> exports, import des notes d'Inkspace) a été vérifié sous Windows par des tests automatiques ; quelques essais à la
> main restent à faire (impression, stylet et écran tactile, export Word ouvert dans Word, import avec Inkspace
> ouvert, fermeture de session de Windows avec une note non enregistrée, couleurs des étiquettes sur deux PC avec
> Nextcloud…). Tout ce qui parle à la
> vraie tablette (enregistrement, synchronisation, mode direct, projection du direct) a été vérifié avec une tablette
> simulée, mais *pas encore avec une Bamboo Slate réelle* : ces essais sont prévus au travail. Les passages concernés
> le signalent par la mention « *Demande la tablette* ». Voir aussi les notes de version.

## Sommaire

1. Ce qu'est Ardoise
2. Installer Ardoise
3. Premier démarrage
4. Enregistrer la tablette
5. Récupérer ses pages : la synchronisation
6. Le mode direct et la présentation en classe
7. La bibliothèque
8. Modifier une note
9. Relecture, narration et vidéo
10. Exporter, imprimer, partager
11. Importer les notes d'Inkspace
12. Stylet et écran tactile
13. Diagnostics et essais avec la tablette
14. L'assistant (Claude Code, Claude Desktop)
15. Vie privée
16. En cas de problème
17. Raccourcis clavier

---

## 1. Ce qu'est Ardoise

- *Portable* : un seul fichier, `ardoise.exe`, à copier où vous voulez, y compris dans un dossier Nextcloud. Pas
  d'installateur, pas de droits d'administrateur, rien dans `Program Files`.
- *Sans compte et sans nuage* : vos notes sont des fichiers dans un dossier de votre choix. Ardoise ne se connecte
  à aucun serveur (sa seule connexion possible est le téléchargement facultatif d'un encodeur vidéo de secours, voir le
  chapitre 9). Elle ne contient pas d'intelligence artificielle et fonctionne hors ligne.
- *Un dossier, plusieurs ordinateurs* : si ce dossier est synchronisé par Nextcloud, OneDrive ou autre, vous
  retrouvez vos notes, et même l'enregistrement de la tablette, sur tous vos PC.
- *Une note = un fichier* `.ardoise`, rangé dans des sous-dossiers qui sont vos classeurs.
- Interface en *français et en anglais*.

*Ce qui demande la tablette* : enregistrer la tablette, la synchroniser, le mode direct (et donc la projection du
direct), le test de la tablette. *Tout le reste marche sans elle*, y compris créer des notes à la souris, au stylet
ou au doigt, annoter un PDF, rejouer, exporter.

*Ce qu'Ardoise ne fait pas* : le nuage Wacom et la collaboration en temps réel d'Inkspace, macOS et Linux, les
applications pour téléphone. Les notes ne peuvent pas contenir de zone de texte tapée au clavier.

## 2. Installer Ardoise

### Ce qu'il faut

- Windows 11 (Windows 10 n'a pas été essayé). Le moteur d'affichage *WebView2* de Microsoft doit être présent : il
  l'est dans Windows 11.
- Pour la tablette : le Bluetooth à basse consommation (Bluetooth 4.0 ou plus), intégré au PC ou par une petite clé
  USB. Aucun réglage préalable dans les paramètres Bluetooth de Windows n'est prévu : l'enregistrement de la tablette se
  fait dans Ardoise (*à confirmer avec la vraie tablette*).

### Installer = décompresser

Ardoise est livrée dans une archive `ardoise-<version>-windows-x64.zip` (par exemple `ardoise-0.1.0-windows-x64.zip`).

1. Si l'archive vient d'un téléchargement ou d'un courriel, faites d'abord un clic droit dessus, *Propriétés*, et
   cochez *Débloquer* s'il est proposé.
2. Faites un clic droit sur l'archive, *Extraire tout…*, et choisissez le dossier où ranger le programme, par exemple
   `C:\Users\Prénom\Nextcloud\Ardoise\Programme` (Windows propose un dossier portant le nom de l'archive : vous
   pouvez le raccourcir). Le contenu arrive dans un dossier `ardoise-<version>`.
3. Dans ce dossier, double-cliquez sur `ardoise.exe`. Créez un raccourci sur le bureau si vous voulez.

Le dossier contient :

- `ardoise.exe` : le programme. C'est le seul fichier indispensable ; il n'y a rien à installer ;
- `LISEZMOI.txt` : l'essentiel en français et en anglais ;
- `LICENSE` et `THIRD-PARTY-NOTICES` : la licence d'Ardoise (GNU GPL version 3) et celles des composants libres
  qu'elle utilise ;
- `guide\` : ce guide et le guide de l'assistant, en pages HTML à ouvrir dans un navigateur (par un double-clic) :
  `guide.fr.html`, `guide.en.html`, `assistant.fr.html` et `assistant.en.html`. Les liens d'une page à l'autre
  fonctionnent.

Pour la désinstallation, supprimez ce dossier (et, si vous le souhaitez, `%LOCALAPPDATA%\Ardoise`, voir plus bas).
Vos notes, elles, ne sont pas touchées : elles sont dans votre bibliothèque.

### L'avertissement de Windows au premier lancement

L'exécutable n'est pas signé numériquement. Windows peut donc afficher un écran bleu « *Windows a protégé votre
ordinateur* » : cliquez sur *Informations complémentaires*, puis sur *Exécuter quand même*. C'est normal pour un
programme non signé. Si le fichier vient d'un téléchargement ou d'un courriel, faites aussi un clic droit sur
`ardoise.exe`, *Propriétés*, et cochez *Débloquer* s'il est proposé.

Sur un PC géré par un service informatique, Windows (Smart App Control, AppLocker…) peut refuser sans proposer de
continuer. Ardoise n'y peut rien : il faut demander une autorisation au service informatique.

### Le fichier de réglages

Vos réglages sont dans `ardoise.toml`, *à côté de `ardoise.exe`* : le fichier voyage avec le programme. Si ce
dossier n'est pas accessible en écriture, Ardoise utilise `%APPDATA%\Ardoise\ardoise.toml`. Le chemin exact est
rappelé en bas de **Réglages**. Le fichier est du texte simple, mais il n'est pas nécessaire de le modifier à la
main.

### Lancer Ardoise deux fois

Une seule fenêtre d'Ardoise est ouverte par exemplaire du programme : si vous double-cliquez de nouveau sur le même
`ardoise.exe` (un double-clic de trop pendant un démarrage lent, par exemple), Ardoise ramène la fenêtre déjà ouverte
au premier plan au lieu d'en ouvrir une seconde, qui se disputerait la tablette. Deux exemplaires placés dans *deux
dossiers différents* (donc avec deux fichiers `ardoise.toml`) peuvent en revanche tourner côte à côte. Les commandes
écrites dans ce guide (`--self-test`, `--import-inkspace`, `--create-demo-library`…) fonctionnent même quand la
fenêtre d'Ardoise est ouverte.

### Plusieurs ordinateurs

- Mettez `ardoise.exe` (et donc `ardoise.toml`) dans un dossier synchronisé, par exemple Nextcloud. Le chemin du
  dossier de bibliothèque n'étant pas le même sur chaque PC, Ardoise range ce réglage *par nom d'ordinateur* dans
  `ardoise.toml`, par exemple :

  ```toml
  [machines."PC-MAISON"]
  library = 'C:\Users\Prénom\Nextcloud\Ardoise\Notes'

  [machines."PC-TRAVAIL"]
  library = 'D:\Nextcloud\Ardoise\Notes'
  ```

- Au premier lancement sur un nouveau PC, Ardoise demande où se trouve la bibliothèque (chapitre 3) ; vous n'avez
  pas à toucher au fichier.
- L'enregistrement de la tablette est dans la bibliothèque (`_ardoise\device.json`) : *enregistrer la tablette une
  seule fois suffit* pour tous vos PC.
- *Ne modifiez pas la même note sur deux PC en même temps.* Si cela arrive, Ardoise refuse d'écraser l'autre version
  et vous propose d'enregistrer la vôtre comme copie (chapitre 8). Les « copies en conflit » créées par Nextcloud sont
  repérées dans la bibliothèque (chapitre 7).
- *Mettre à jour* : fermez Ardoise sur tous les PC, puis remplacez `ardoise.exe`. Un exécutable lancé depuis Nextcloud
  est verrouillé tant qu'il tourne, la synchronisation de la nouvelle version échouerait. Une version récente lit
  toutes les notes anciennes ; si une note a été créée par une version plus récente que la vôtre, Ardoise vous
  demande de la mettre à jour.

### Ce qu'Ardoise garde sur chaque PC

Ces dossiers sont propres à chaque ordinateur et ne sont jamais dans votre bibliothèque ni dans Nextcloud, sous
`%LOCALAPPDATA%\Ardoise` : le dossier de WebView2 (`webview`), les miniatures (`thumbnails`), le texte reconnu pour la
recherche (`handwriting`), l'encodeur OpenH264 si vous l'avez téléchargé (`codecs`), et trois dossiers de *copies de
secours* (`drafts` pour les modifications d'une note pas encore enregistrées, voir le chapitre 8, `live-backup` pour
des pages du direct qui n'ont pas pu être écrites, `narration-backup` pour une narration qui n'a pas pu être ajoutée à
sa note). Le dossier de WebView2, les miniatures et le texte reconnu se reconstruisent tout seuls ; ne supprimez les
copies de secours qu'après avoir vérifié que vous n'en avez plus besoin.

## 3. Premier démarrage

Au premier lancement sur un PC, la fenêtre **Bienvenue dans Ardoise** demande le dossier où ranger vos notes.

1. Cliquez sur **Choisir un dossier…** et choisissez de préférence un dossier synchronisé (Nextcloud…), par exemple
   `C:\Users\Prénom\Nextcloud\Ardoise\Notes`.
2. Ardoise indique s'il contient déjà une bibliothèque (« elle sera utilisée telle quelle ») ou si elle en crée une
   nouvelle. Ce choix ne concerne que cet ordinateur.
3. Si Ardoise trouve des notes de Wacom Inkspace sur ce PC, la fenêtre le dit (« Ardoise a trouvé des notes de Wacom
   Inkspace sur cet ordinateur (24 notes). ») et propose une case, cochée d'office : **Les importer dans la
   bibliothèque (classeur « Inkspace »)**. Décochez-la si vous ne voulez pas les importer maintenant ; vous pourrez le
   faire plus tard (chapitre 11).
4. Cliquez sur **Continuer**. Si la case d'Inkspace est cochée, la fenêtre d'import s'ouvre aussitôt dans la
   bibliothèque : vous confirmez l'import avant qu'il ne commence (chapitre 11).

Vous arrivez dans la *bibliothèque*. Tant qu'elle est vide, elle dit que les pages de la tablette apparaîtront après
la première synchronisation ; vous pouvez déjà créer une note avec **Nouvelle note**. Ardoise crée dans la
bibliothèque un dossier `_ardoise` (enregistrement de la tablette, journaux de diagnostic, corbeille, couleurs des
étiquettes). Il n'est pas caché, pour que Nextcloud le synchronise : n'y touchez pas.

Pour changer de bibliothèque plus tard : **Réglages**, puis **Changer** à côté de **Dossier de la bibliothèque**, et
choisissez le dossier. Ardoise ne change pas aussitôt : elle montre d'abord ce que contient ce dossier (« Ce dossier
contient déjà une bibliothèque Ardoise : elle sera utilisée telle quelle » ou « Une nouvelle bibliothèque sera créée
dans ce dossier », et le nombre de notes), rappelle que le changement ne vaut que pour ce PC, et attend votre accord :
**Utiliser ce dossier** (ou **Annuler**). Les notes de l'ancienne bibliothèque restent dans leur dossier.

Les réglages généraux sont aussi dans **Réglages** : **Langue** (**Automatique (langue de Windows)**, **Français** ou
**English**) et **Thème** (**Comme Windows**, **Clair**, **Sombre**).

*Essayer sans tablette.* Pour découvrir Ardoise sans rien enregistrer de vos notes, vous pouvez créer une
bibliothèque de démonstration (des notes inventées). Dans PowerShell, dans le dossier d'`ardoise.exe` :

```
.\ardoise.exe --create-demo-library C:\Users\Prénom\Démo-Ardoise
```

puis choisissez ce dossier comme bibliothèque. Et, pour essayer la synchronisation et le mode direct sans tablette,
lancez `.\ardoise.exe --simulated-tablet` : une *tablette simulée* (trois pages en mémoire) remplace le Bluetooth ;
elle n'est jamais mêlée à l'enregistrement de la vraie tablette. Une note de la bibliothèque de démonstration, « Cours
d'optique », est écrite à la main (glyphes de synthèse) et sert à essayer la recherche dans l'écriture.

## 4. Enregistrer la tablette

*Demande la tablette.* L'enregistrement se fait une seule fois.

> *À savoir avant de commencer* : une Bamboo Slate n'est enregistrée qu'auprès d'une seule application à la fois.
> Une fois enregistrée dans Ardoise, elle ne fonctionne plus avec l'application Wacom du téléphone ni avec Inkspace,
> jusqu'à ce que vous l'y enregistriez de nouveau. Coupez le Bluetooth de votre téléphone pendant l'enregistrement.

1. Chargez la tablette et gardez-la près de l'ordinateur ; le Bluetooth doit être activé sur le PC.
2. Dans la barre latérale, en bas, cliquez sur le bloc **Tablette** (il affiche **Aucune tablette enregistrée**), puis
   sur **Enregistrer la tablette**.
3. Lisez l'avertissement (**Avant de commencer**) et cliquez sur **Continuer**.
4. Suivez l'assistant, qui montre un dessin de la tablette :
   - **Maintenez le bouton de la tablette** enfoncé environ 6 secondes, jusqu'à ce que le voyant clignote en bleu ;
   - **Recherche de la tablette**, puis **Connexion à la tablette** : ne bougez pas, cela peut prendre quelques
     secondes ;
   - **Appuyez brièvement sur le bouton** pour confirmer (vous avez environ 30 secondes) ;
   - **Tablette enregistrée** : le nom de la tablette s'affiche. Cliquez sur **Terminer**.
5. Si l'enregistrement échoue, Ardoise écrit ce qui n'a pas marché ; vérifiez le Bluetooth, la charge et la distance,
   puis cliquez sur **Réessayer**. **Annuler** est possible à chaque étape.

Le panneau **Tablette** donne ensuite le nom, la batterie (et si elle est **en charge**), la version du firmware, le
nombre de pages en mémoire, le modèle et la zone de dessin. Ces informations sont lues à la première connexion.

La tablette est ensuite reconnue sur *tous* vos PC qui utilisent la même bibliothèque. Pour la défaire : **Réglages**,
section **Tablette**, **Oublier la tablette** (elle devra être enregistrée de nouveau, sur tous vos ordinateurs).

Dans le panneau **Tablette**, la section **Réglages de la tablette** permet de **Renommer** la tablette (1 à 15
caractères, sans accent) et de **Mettre à l'heure** (l'horloge de la tablette est réglée sur celle du PC).

## 5. Récupérer ses pages : la synchronisation

*Demande la tablette.*

### Écrire sur la tablette

Posez une feuille sur la Bamboo Slate et écrivez avec son stylo. À la fin d'une page, *un appui bref* sur le bouton
l'enregistre dans la mémoire de la tablette et passe à la suivante. (Ne maintenez pas le bouton : environ 6 secondes,
il remettrait la tablette en mode enregistrement.)

### Recevoir les pages

Quand la tablette enregistrée est à portée, Ardoise s'y connecte toute seule ; si besoin, un appui bref sur son bouton
la réveille. L'état s'affiche dans le bloc **Tablette** de la barre latérale : **Recherche de la tablette…**,
**Connexion…**, **Connectée**, **Synchronisation…**, avec la batterie.

Les pages sont reçues automatiquement à la connexion, ou à la demande avec **Synchroniser maintenant** dans le panneau
**Tablette**. Une barre indique **Page 1 sur 3**… et **Annuler** arrête la synchronisation (les pages déjà reçues
restent des notes, les autres restent sur la tablette). Le panneau résume ensuite : **3 notes enregistrées.**,
**3 pages effacées de la tablette.** ou **Aucune nouvelle page.**

Chaque page devient une *note* à la racine de la bibliothèque, sans titre (la carte affiche la date), à la taille de
la tablette. Vous pouvez la titrer, la ranger, l'étiqueter (chapitre 7).

### Les options

Dans le panneau **Tablette**, section **Options** (et dans **Réglages**, section **Tablette**) :

- **Synchroniser automatiquement à la connexion** : les pages sont reçues dès que la tablette est à portée.
- **Effacer de la tablette après import** (activé par défaut) : une page n'est effacée de la tablette qu'*après* que
  son fichier a été écrit dans la bibliothèque et vérifié ; si quelque chose échoue, elle reste sur la tablette.
  **Attention** : la Bamboo Slate ne livre que sa page la plus ancienne. Sans cette option, les autres pages restent
  illisibles tant que la première n'est pas effacée, et le panneau le dit (« N pages attendent sur la tablette… »).
  Gardez donc l'option cochée sauf raison particulière.
- **Bouton de la tablette en mode direct** : **Nouvelle page** ou **Nouveau calque** (chapitre 6).
- **Position de la tablette** : comment vous tenez la tablette. Ardoise redresse ce que vous dessinez pour qu'il
  apparaisse à l'endroit. Quatre positions : **Paysage, bouton en bas**, **Paysage à l'envers, bouton en haut** (qui
  convient aux gauchers), **Portrait, bouton à gauche**, **Portrait, bouton à droite**. Les positions en portrait
  donnent des notes en portrait. Le bouton est celui qui est à côté du voyant de la tablette.

### Export automatique pour le téléphone

Dans **Réglages**, **Export automatique après synchronisation** : choisissez un **Format** (**Désactivé**, **PDF** ou
**PNG (image)**) et un **Dossier dans la bibliothèque** (par exemple `Exports`), puis **Enregistrer**. Chaque page
reçue est alors aussi écrite en PDF ou en PNG dans ce dossier : vous pouvez lire vos notes sur votre téléphone avec
l'application Nextcloud. Ce dossier n'est pas affiché dans la liste des classeurs.

## 6. Le mode direct et la présentation en classe

*Demande la tablette.* En mode direct, ce que vous écrivez sur la tablette apparaît aussitôt à l'écran, avec l'outil,
la couleur et l'épaisseur que vous avez choisis. Chaque page écrite devient une note de la bibliothèque, enregistrée au
fil de l'eau.

### Lancer le mode direct

Ouvrez le panneau **Tablette** (clic sur le bloc **Tablette** de la barre latérale) et cliquez sur **Mode direct**.
L'état passe de **En attente de la tablette** à **En direct** dès que la tablette envoie des points : si besoin,
appuyez brièvement sur son bouton pour la connecter. En cas de coupure, l'état devient **Reconnexion…** et le tracé
reprend tout seul.

Pour terminer, cliquez sur **Terminer** : chaque page est enregistrée comme une note (le compteur de la fenêtre dit
« N notes enregistrées »). La touche `Échap` fait la même demande, mais Ardoise vous interroge d'abord (**Terminer le
mode direct ?**, avec **Continuer** ou **Terminer**), car terminer ne s'annule pas (voir plus bas pour `Échap` et la
projection). Si une page ne peut pas être écrite dans la bibliothèque (disque plein, dossier
inaccessible…), un message l'explique, la page reste en mémoire et une copie de secours est écrite sur ce PC à la
fermeture d'Ardoise. Si le mode direct ne s'arrête pas correctement, **Fermer quand même** arrête aussi la
projection et la séance, et ferme la fenêtre.

Si une note de la séance est renommée ou étiquetée pendant le cours (par l'assistant, par exemple), les pages suivantes
continuent d'y être écrites au lieu d'être dupliquées.

### Reprendre le mode direct sur une feuille

*Demande la tablette.* Le cours a été interrompu, ou vous voulez ajouter de l'écriture à une feuille déjà
enregistrée ? Vous pouvez reprendre le mode direct sur cette feuille.

- **Depuis la bibliothèque** : ouvrez le menu **⋯** de la carte de la note et choisissez **Reprendre en mode direct**.
- **Depuis l'éditeur** : cliquez sur **Reprendre en mode direct** dans la barre d'outils. S'il y a des modifications non
  enregistrées, Ardoise le demande : **Enregistrer et reprendre le direct**, **Reprendre sans enregistrer** ou
  **Rester**.

Le bouton est grisé tant que la tablette n'est pas enregistrée ou qu'elle est occupée (synchronisation en cours). Il
faut aussi ne choisir qu'une seule note.

**Ce qui se passe**

- La fenêtre de commande, la présentation et l'autre écran du réseau montrent la feuille avec son fond (modèle,
  texture, image ou page de PDF) et ses traits. Ce que vous écrivez s'ajoute, sur le dernier calque visible. Les
  calques masqués ne sont jamais montrés à la présentation ni sur l'autre écran.
- **C'est la note elle-même qui est écrite** : même fichier, même titre, pas de copie. Son image de fond, son
  original, sa narration, son texte reconnu et tout ce qu'elle contient sont gardés. Si vous ne tracez rien, le fichier
  n'est pas touché.
- **Le tableau noir** ne fait pas perdre le fond de la feuille : en le quittant, vous retrouvez son papier et son image.
  Si vous terminez sans l'avoir quitté, la feuille garde son papier tant que vous n'avez rien écrit ; si vous y avez
  écrit à la craie, elle est enregistrée en ardoise pour que la craie reste lisible.
- **Le format** : si la feuille n'a pas le format de la tablette, ce que vous écrivez est réduit ou agrandi pour tenir
  en entier dans la feuille, au milieu, sans déformation. L'épaisseur du trait ne change pas.
- **L'orientation** : la feuille n'est jamais tournée. Si elle est en paysage et que la tablette est réglée en portrait
  (ou l'inverse), un avis dit comment tenir la tablette : « Pour écrire sur cette feuille, tenez la tablette ainsi : … ».
  Les quatre boutons de position sont grisés pour cette feuille.
- **La page suivante** crée une note neuve, à la racine de la bibliothèque, et suit de nouveau la position réglée pour
  la tablette : les boutons de position fonctionnent alors.
- **La narration** : si vous enregistrez, le son s'ajoute après l'ancien sans le recouvrir, et la relecture reste juste
  malgré le temps passé entre les deux séances. Si le son ne peut pas s'y ajouter (plus de 4 heures en tout, ou ancienne
  narration illisible), il part dans une note neuve à côté de la feuille, et Ardoise vous le dit en terminant.
- **Terminer** enregistre la note comme d'habitude (compteur « N notes enregistrées »).

**Revenir à l'original, après une reprise.** La note garde sa *première version* (voir le chapitre 8, « Revenir à
l'original »). Une reprise n'en crée pas une nouvelle à chaque fois :

- une note qui n'a pas encore d'original (écrite en direct, importée, faite dans Ardoise, ou fiche encore vierge) retrouve
  la feuille *telle qu'elle était avant la reprise* ;
- une note reçue de la tablette retrouve la page *telle qu'elle a été synchronisée* ;
- une note déjà modifiée retrouve *sa première version*, pas l'état juste avant la reprise.

Seuls le format de la page, les calques, les traits et la place de l'image de fond reviennent ; le titre, les
étiquettes, le favori et la narration restent comme ils sont. Le papier aussi, sauf si l'ardoise du tableau noir l'a
remplacé : le papier de la première version revient alors. `Ctrl + Z` annule le tout.

**Si la feuille change ailleurs pendant le direct** (une copie arrive par Nextcloud, ou la note est modifiée ou
supprimée sur un autre PC), Ardoise n'écrase rien. Un avis s'affiche : « Cette feuille a été modifiée ailleurs pendant
le direct : pour ne rien écraser, l'écriture continue dans une nouvelle note, à côté d'elle. » (ou « supprimée
ailleurs » si le fichier a disparu). La nouvelle note est dans le même classeur, avec l'image, l'original et la
narration de la feuille ; la version écrite ailleurs reste telle qu'elle est. Fermez l'avis avec **Fermer le message**.
Un simple changement de titre, d'étiquette ou de favori fait ailleurs ne crée pas de nouvelle note.

### La fenêtre de commande

- **Outil** (stylo à bille, plume, feutre, crayon, pinceau, surligneur, craie), **Couleur** (dont **Blanc craie** et
  **Jaune**) et **Épaisseur** : le trait suivant en tient compte, pas les précédents.
- **Pages** : **Page précédente**, **Page suivante** (la position s'affiche : « Page 2 / 5 »), **Nouvelle page**.
  **Nouveau calque** ajoute un calque à la page en cours. **Effacer la page** (ou `Ctrl + Suppr`) efface la page en cours
  seulement ; un message « Page effacée. » propose pendant 8 secondes **Annuler l'effacement**, qui rend l'encre (`Ctrl + Z` la rend aussi, sans limite de temps : voir **Retoucher pendant le direct**). Si la
  page a changé entre-temps, Ardoise dit que l'effacement ne peut plus être annulé.
- **Tableau noir** : fond ardoise et craie blanche. **Pointeur laser** : le stylet en survol, sans toucher la tablette,
  montre un point rouge à l'écran *sans rien écrire* ; en touchant la tablette on écrit de nouveau.
- **Position de la tablette** : quatre boutons qui changent aussitôt la position ; la page en cours tourne avec ce
  qui y est déjà dessiné.
- L'aperçu montre la page entière ; un *rectangle bleu* indique la zone projetée. Faites-le glisser pour déplacer la
  zone, la molette zoome. **Agrandir**, **Réduire**, **Page entière** et **Largeur** (la page sur toute la largeur de la
  zone projetée) sont aussi proposés.
- **Latence** : le temps entre la réception du point par l'ordinateur et son dessin, mesuré et affiché (objectif :
  moins de 30 ms).
- **Enregistrer la narration** : raconter pendant la séance ; le son est découpé page par page et enregistré avec
  chaque note, la relecture de la note le rejoue (chapitre 9).

Le *bouton de la tablette* fait, pendant le direct, ce que l'option **Bouton de la tablette en mode direct** indique
(nouvelle page ou nouveau calque). *Ce que la Bamboo Slate envoie à l'appui n'est pas encore vérifié sur la vraie
tablette.* Les touches de la page suivante et précédente marchent dans tous les cas.

### Retoucher pendant le direct

*Demande la tablette.* Une faute à enlever, un mot à mettre en rouge ? Vous n'avez pas besoin de quitter le mode direct
ni d'ouvrir l'éditeur. Vous retouchez dans la fenêtre de commande, et **la tablette continue d'écrire pendant ce temps**.

**Choisir des traits**

- Cliquez sur **Sélection** dans la barre d'outils, ou appuyez sur `V`. Un second appui quitte
  l'outil.
- **Lasso** : dans l'aperçu, appuyez avec la souris et entourez les traits, puis relâchez. Un trait est choisi quand la
  moitié au moins de ses points est dans le lasso.
- **Clic** : cliquez sur un trait pour le choisir seul. Cliquez sur l'encre elle-même, pas à côté.
- `Maj` + lasso ou clic ajoute des traits à ceux déjà choisis. `Ctrl` + lasso ou clic en retire.
- Un geste dans le vide, ou `Échap`, enlève la sélection. `Échap` ne ferme rien d'autre tant que des traits sont choisis.
- Un doigt et la molette continuent de déplacer et de zoomer l'aperçu.
- Les traits choisis sont entourés de bleu dans l'aperçu. Une barre dit « Traits sélectionnés : 3 ».
- On ne peut pas choisir le trait que la tablette est en train d'écrire, ni les traits d'un calque masqué. Sur une
  feuille reprise, on peut choisir les anciens traits de la feuille.

**Supprimer** : cliquez sur **Supprimer** dans la barre, ou appuyez sur `Suppr` ou `Retour arrière`.

**Changer la couleur** : cliquez sur une pastille de **Couleur**. Seuls les traits choisis changent. Ils restent
choisis, pour essayer une autre couleur. **La couleur du stylo ne change pas** : ce que la tablette écrit ensuite garde
sa couleur. Sans sélection, une pastille règle la couleur du stylo, comme avant.

**Annuler et rétablir**

- `Ctrl + Z` (ou **Annuler**) annule la dernière retouche : une suppression, un changement de couleur, ou **Effacer la
  page**. Les traits supprimés reviennent à leur place.
- `Ctrl + Y` ou `Ctrl + Maj + Z` (ou **Rétablir**) refait ce qui vient d'être annulé. Une nouvelle retouche vide ce qui
  pouvait être rétabli.
- **Ce qui n'est jamais annulé** : ce que la tablette a écrit. Un trait écrit entre-temps reste, dans quelque ordre que
  vous annuliez ou rétablissiez. Les changements d'outil, de couleur du stylo ou d'épaisseur, et les changements de
  page, ne sont pas non plus des retouches.
- Chaque page a sa propre liste, qui garde les 100 dernières retouches. Si vous quittez une page puis y revenez,
  `Ctrl + Z` annule encore ses retouches. Les listes disparaissent quand vous terminez le mode direct.
- Les boutons **Annuler** et **Rétablir** sont grisés quand il n'y a rien à faire.

**Le vidéoprojecteur et l'autre écran du réseau ne voient que le résultat.** Jamais le contour bleu ni le lasso : un
trait supprimé disparaît, un trait recoloré change de couleur. Les annotations tracées sur la présentation ne sont pas
des traits de la page : on ne les choisit pas.

**Dans la note.** Quand vous terminez, la note contient ce qu'il y avait à l'écran : traits supprimés absents, couleurs
changées. Sur une feuille reprise, la retouche d'un ancien trait est enregistrée dans la note elle-même ;
« Revenir à l'original » (chapitre 8) rend toujours la première version.

### Projeter sur un vidéoprojecteur

1. Branchez le projecteur et, dans Windows, appuyez sur `Win + P` puis choisissez *Étendre* : chaque écran a son bureau.
2. Dans la section **Projection** de la fenêtre de commande, choisissez l'**Écran de projection** dans la liste (le
   bouton **Actualiser la liste des écrans** la remet à jour) et cliquez sur **Projeter**.
3. Une fenêtre de *présentation* s'ouvre *en plein écran sur l'écran choisi*, sans aucun élément d'interface (le
   curseur de la souris disparaît après 2 secondes d'immobilité). La fenêtre de commande reste sur l'écran de
   l'ordinateur. Ce que vous écrivez s'affiche sur les deux.
4. **Arrêter la projection** ferme la présentation. Ardoise retient l'écran choisi sur ce PC.

Avec un seul écran, la présentation recouvre la fenêtre de commande : appuyez sur `Échap` dans la présentation pour la
fermer.

*À quoi sert `Échap`.* Dans la fenêtre de commande comme dans la présentation, quand la projection est affichée,
`Échap` la ferme (comme dans un diaporama) ; quand rien n'est projeté, `Échap` demande s'il faut terminer le mode
direct. *`Espace` veut toujours dire « page suivante »*, même si un bouton de la fenêtre a le focus : il n'appuie
jamais sur ce bouton (c'est ce qui évite d'effacer la page suivante par mégarde).

### Écrire par-dessus la présentation

Sur la fenêtre projetée, vous pouvez annoter la page au stylet, au doigt ou à la souris : les annotations sont
rouges (jaunes sur le tableau noir). Le dos du stylet les efface une à une ; le bouton **Effacer les annotations**
(qui apparaît avec le curseur) ou la touche `Suppr` les efface toutes ; elles disparaissent aussi au changement de page.
*Elles ne sont jamais enregistrées dans la note.* Pendant un partage sur le réseau (voir plus bas), l'autre ordinateur
les montre aussi. Deux doigts zooment la zone projetée.

### Au clavier

Les mêmes touches marchent dans la fenêtre de commande et dans la présentation (une télécommande de présentation
envoie Page suivante et Page précédente) : voir le chapitre 17.

### Montrer la feuille sur un autre ordinateur du réseau

*Le partage du mode direct demande la tablette ; celui d'une note ouverte dans l'éditeur n'en demande pas. Ce partage a
été vérifié par des tests automatiques (serveur, page de l'autre écran, tablette simulée), mais pas encore sur deux
vrais ordinateurs ni avec le pare-feu de l'école : l'essai H4 est prévu au travail.*

Un autre ordinateur du même réseau (par exemple le PC de la salle, relié au vidéoprojecteur) peut afficher la feuille
dans un *simple navigateur* : rien à installer dessus. Il montre :

- pendant le mode direct, la feuille en direct, comme la fenêtre de présentation (zone zoomée, tableau noir, pointeur
  laser, annotations écrites par-dessus la présentation) ;
- quand une note est ouverte dans l'éditeur, la page de cette note, y compris vos modifications non enregistrées ;
- sinon (bibliothèque, réglages), un écran d'attente.

*Rien d'autre n'est accessible* : ni la bibliothèque, ni les titres des notes, ni les autres notes, ni un fichier du PC.
L'autre ordinateur ne peut pas changer ce qui est montré, mais il peut régler son propre affichage (voir plus bas).

**Côté Ardoise.** Cliquez sur **Partager sur le réseau**, dans la fenêtre de commande du mode direct ou dans la barre
d'outils de l'éditeur. Le panneau qui s'ouvre donne :

- l'*adresse* à taper dans le navigateur de l'autre ordinateur, de la forme `http://192.168.1.23:8765` (si le PC en a
  plusieurs, elles sont toutes listées) ;
- un *code QR* de cette adresse, à scanner avec un téléphone ou une tablette ;
- le *code d'accès* à 6 caractères, en deux groupes de trois (par exemple `K7M Q2X`) ;
- le *nombre d'écrans connectés*, et ce qui est montré ;
- **Pause** (l'image se fige sur les écrans ; vous continuez d'écrire, puis **Reprendre** montre l'état actuel) et
  **Arrêter le partage**.

Tant qu'un partage est en cours, un indicateur avec le nombre d'écrans reste en haut de la bibliothèque et des barres
d'outils : un clic rouvre le panneau.

**Côté autre ordinateur.**

1. Ouvrez un navigateur (Edge, Chrome, Firefox…) et tapez l'adresse *en entier, avec `http://` et le numéro de port* :
   `http://192.168.1.23:8765`.
2. Tapez le code d'accès (majuscules ou minuscules, avec ou sans espace).
3. La feuille s'affiche. Appuyez sur `F11` ou sur le bouton plein écran de la page (ou double-cliquez) pour la montrer
   sur tout l'écran ; le bouton disparaît avec le curseur.

**Régler l'affichage sur l'autre ordinateur.** Bougez la souris (ou touchez l'écran) : une barre de boutons apparaît
en bas de la page, puis disparaît au bout de quelques secondes. Ces réglages ne concernent que cet écran : ils ne
changent rien sur le PC d'Ardoise ni sur les autres écrans.

- **Page entière** : toute la page est visible (touche `0` ou `Début`).
- **Pleine largeur** : la page occupe toute la largeur de l'écran ; on voit le haut de la page, et on descend en la
  faisant glisser (touche `W`).
- **Agrandir** et **Réduire** (touches `+` et `-`) : pour regarder un endroit de plus près. La molette de la souris
  zoome à l'endroit du curseur ; avec un écran tactile, on écarte ou on rapproche deux doigts.
- Pour déplacer la page quand elle n'est pas affichée en entier : glissez avec la souris ou avec un doigt, ou
  utilisez `Maj` + les flèches.
- **Suivre le PC** (touche `Échap`) : pendant le mode direct, l'écran suit d'abord la vue choisie sur le PC (zoom,
  déplacement). Dès que vous réglez l'affichage ici, il garde votre réglage ; ce bouton, qui apparaît alors, revient à la
  vue du PC. Le tableau noir et le pointeur laser restent toujours ceux du PC.
- `F` : plein écran (comme `F11`).

Si le PC passe à une autre page, « Page entière » et « Pleine largeur » restent actifs ; un zoom libre revient à la
page entière.

Si la connexion se coupe un instant, la page se reconnecte toute seule et se remet à jour. Quand vous arrêtez le
partage ou fermez Ardoise, elle indique que le partage est terminé.

**Le pare-feu de Windows.** La première fois, Windows affiche une fenêtre « Alerte de sécurité Windows » (*le Pare-feu
Windows Defender a bloqué certaines fonctionnalités de cette application*). Cochez **Réseaux privés** puis cliquez sur
**Autoriser l'accès**. Il faut des droits d'administrateur pour cela : sur votre PC personnel, c'est vous ; sinon,
demandez au service informatique d'autoriser `ardoise.exe` sur le réseau privé. Ardoise ne modifie jamais le pare-feu
lui-même.

- *Réseau « public » pour Windows.* Beaucoup de réseaux d'école sont classés « Public ». Soit vous passez ce réseau en
  « Privé » (**Paramètres** > **Réseau et Internet** > propriétés du réseau Wi-Fi ou Ethernet > **Profil réseau** :
  **Privé**), soit vous cochez aussi **Réseaux publics** dans la fenêtre du pare-feu, en sachant que le code d'accès est
  alors la seule protection.
- *La fenêtre a été fermée sans choisir.* Ouvrez **Pare-feu Windows Defender** > **Autoriser une application ou une
  fonctionnalité via le Pare-feu Windows Defender** > **Modifier les paramètres** > **Autoriser une autre
  application…** > **Parcourir…** > `ardoise.exe`, puis cochez **Privé** (et **Public** si besoin) pour Ardoise.
- Windows attache cette autorisation au *programme*, à son emplacement : `ardoise.exe` dans ce dossier, quel que soit
  le port. Si vous déplacez ou renommez `ardoise.exe`, ou lancez une autre copie, Windows repose la question :
  répondez de la même façon. Changer de port ne la repose pas.
- Le *port* est 8765 d'office. Il ne change pas d'un partage à l'autre, donc l'adresse reste la même, et le service
  informatique peut ouvrir ce port. Aucun autre programme ne peut prendre ce port pendant un partage.
- Si un autre programme utilise déjà ce port, Ardoise prend de lui-même le premier port libre qui suit (8766, 8767…)
  et le dit en haut du panneau : « Le port 8765 est utilisé par un autre programme : le partage utilise le port 8766,
  retenu pour ce PC. » Ce port est mémorisé pour ce PC : aux partages suivants, l'adresse reste la même. Tapez
  simplement l'adresse que le panneau affiche. Si aucun des 20 ports qui suivent n'est libre, Ardoise le dit et ne
  démarre pas le partage.
- Pour choisir vous-même le port, ouvrez **Réglages** > **Partage sur le réseau** > **Port du partage**, tapez un
  nombre de 1024 à 65535 et cliquez sur **Enregistrer**. Ce choix vaut pour ce PC seulement. Un partage déjà lancé
  garde son port ; le nouveau sert au partage suivant. Changer de port ne redemande pas l'autorisation du pare-feu,
  mais l'autre ordinateur doit taper la nouvelle adresse.

**Confidentialité.** Le partage ne démarre que si vous cliquez, et s'arrête avec **Arrêter le partage** ou à la
fermeture d'Ardoise (le port est alors refermé). Le code change à chaque partage. Après 5 codes faux en 5 minutes,
l'ordinateur concerné doit attendre (la page dit combien de temps), sans gêner les autres. Les échanges ne sont *pas
chiffrés* (c'est du `http` sur le réseau local) : n'utilisez ce partage que sur un réseau de confiance, et pas pour des
notes confidentielles sur un réseau ouvert. L'écriture et l'image des notes partagées sont visibles par quiconque a
l'adresse *et* le code.

**Si ça ne se connecte pas.**

- Les deux ordinateurs sont-ils sur le *même réseau* (même Wi-Fi ou même prise réseau, pas un réseau « invités »
  isolé) ?
- L'adresse est-elle tapée avec `http://` et le port (`:8765`) ? Prenez celle que le panneau affiche, pas celle d'un
  autre jour : le port peut avoir changé si un autre programme tenait le 8765.
- Le pare-feu a-t-il été autorisé pour ce type de réseau (voir plus haut) ? C'est la cause la plus fréquente.
- La page dit que le code est incorrect : relisez le code du panneau (il n'y a ni `0`, ni `O`, ni `1`, ni `I`, ni `L`).
- La page dit qu'il y a trop d'écrans : le partage accepte 32 écrans à la fois (4 par ordinateur). Elle réessaie
  toute seule toutes les 5 secondes.
- Le navigateur affiche seulement « Misdirected Request » : tapez l'adresse en chiffres que montre le panneau. Par
  sécurité, le partage refuse les noms avec un point (comme `pc.ecole.lan`) ; il accepte le nom court de
  l'ordinateur, si le réseau le connaît.
- Un logiciel de sécurité ou le réseau de l'école peut aussi bloquer les connexions entre ordinateurs : demandez au
  service informatique d'ouvrir vers votre PC le port que le panneau affiche (8765 d'habitude).

## 7. La bibliothèque

### Parcourir

La bibliothèque est la page d'accueil. En haut : le champ **Rechercher une note**, le tri (**Trier les notes** :
**Date : la plus récente d'abord**, **Date : la plus ancienne d'abord**, **Titre : A → Z**, **Titre : Z → A**), les
boutons **Nouvelle note**, **Depuis un PDF ou une image…** et **Réglages**. À gauche : la barre latérale avec
**Toutes les notes**, **Favoris**, les **Classeurs**, les **Étiquettes**, la **Corbeille**, l'entrée
**Importer depuis Inkspace…** (chapitre 11), et en bas le bloc **Tablette** et le bouton **Tester la tablette**.

Une carte montre la miniature, le titre (ou la date si la note n'a pas de titre), la date et les étiquettes. Un clic
ouvre la note dans l'éditeur. Sur la carte : la case pour *sélectionner*, l'étoile des favoris, le menu
**Actions pour…** (**Renommer**, favoris, **Mettre à la corbeille**) et une poignée, qui apparaît sur la carte, pour
glisser la note hors d'Ardoise (chapitre 10). Renommer une note renomme aussi son fichier.

Les miniatures sont gardées dans un cache propre à chaque PC. Quand une note change sur un autre ordinateur
(synchronisation Nextcloud), la bibliothèque se met à jour toute seule.

Des messages peuvent s'afficher au-dessus des notes : **Fichiers illisibles : N** (voir plus bas) et, après un arrêt
brutal d'Ardoise, la liste des modifications non enregistrées que l'on peut récupérer (chapitre 8).

### Classeurs

Un classeur est un simple *sous-dossier* de la bibliothèque. Cliquez sur l'icône **Nouveau classeur** (ou sur
**Nouveau sous-classeur dans…** à côté d'un classeur), tapez le **Nom du classeur** et cliquez sur **Créer**. Pour
ranger des notes, glissez-les sur un classeur de la barre latérale, ou sélectionnez-les puis **Déplacer vers…**
(**Bibliothèque (racine)** ramène à la racine).

Pour renommer ou supprimer un classeur, ouvrez le menu **⋯** (« Actions sur le classeur… ») à côté de son nom dans la
barre latérale :

- **Renommer…** : tapez le nouveau nom puis **Renommer** (**Annuler** ou `Échap` abandonne). Le dossier est renommé et
  ses notes le suivent. Ardoise ne remplace jamais un classeur ou un fichier du même nom : elle prévient et vous
  laisse en choisir un autre.
- **Supprimer…** : Ardoise demande confirmation en disant combien de notes (et de sous-classeurs) le classeur contient.
  **Supprimer le classeur** envoie ses notes à la **Corbeille**, puis supprime le dossier. Rien n'est détruit : chaque
  note restaurée depuis la corbeille retrouve son classeur. Un dossier qui contient encore des fichiers qui ne sont pas
  des notes est gardé, et le message le dit. Un message **Annuler** reste affiché quelques secondes (voir « La
  corbeille » plus bas) : il rétablit le classeur avec ses notes.

### Étiquettes et favoris

Sélectionnez une ou plusieurs notes, cliquez sur **Étiquettes…**, tapez une étiquette (plusieurs, séparées par une
virgule) et cliquez sur **Ajouter** ; une étiquette existante se retire avec sa croix. Un clic sur une étiquette de la
barre latérale, ou sur celle d'une carte, n'affiche que les notes qui la portent. L'étoile d'une carte (ou
**Mettre en favoris**) ajoute aux **Favoris**.

*Couleurs des étiquettes.* Le petit rond à côté d'une étiquette, dans la barre latérale et dans le panneau
**Étiquettes…** (sous **Étiquettes existantes**), ouvre une palette de huit couleurs, lisibles en thème clair comme en
thème sombre : **Rouge**, **Orange**, **Jaune**, **Vert**, **Turquoise**, **Bleu**, **Violet**, **Rose** ; **Aucune
couleur** l'enlève. Au clavier, les flèches passent d'une couleur à l'autre, `Entrée` choisit et `Échap` ferme. La
couleur change sur toutes les notes qui portent l'étiquette : on la retrouve sur les cartes, dans la barre latérale et
dans le panneau. Elle appartient à la *bibliothèque* (fichier `_ardoise\tags.json`), pas aux notes : tous vos PC la
voient, par Nextcloud. Si deux PC changent les couleurs en même temps, Ardoise fusionne étiquette par étiquette, le
changement le plus récent l'emportant.

### Sélection multiple

Cochez les cartes, ou : `Ctrl + clic` ajoute ou retire une note, `Maj + clic` sélectionne une plage, `Ctrl + A` tout,
`Échap` désélectionne. Une barre d'actions apparaît : **Déplacer vers…**, **Étiquettes…**, **Exporter…**,
**Imprimer…**, **Assembler…**, **Tourner**, **Mettre en favoris** (ou **Retirer des favoris**),
**Mettre à la corbeille**, **Désélectionner**.

**Tourner** fait pivoter *toutes* les notes sélectionnées d'un coup : choisissez **Un quart de tour à gauche** ou
**Un quart de tour à droite**. La feuille, l'écriture et l'image de fond tournent ensemble, exactement comme
**Tourner la feuille avec l'écriture** dans l'éditeur (chapitre 8). Une barre indique l'avancement (« Rotation des notes :
2 sur 5 »), **Arrêter** interrompt l'opération entre deux notes, et les notes qui n'ont pas pu être tournées sont
listées avec la raison. Le message **Annuler** remet les notes dans leur sens (voir « La corbeille »).

**Assembler** (au moins deux notes) crée une nouvelle note, sans modifier les originales : **Superposer sur une page**
(toutes les notes sur une seule page, chacune sur ses propres calques) ou **Mettre bout à bout (page longue)** (les
notes l'une sous l'autre). Donnez un **Titre de la nouvelle note** et cliquez sur **Assembler**. Seule l'image de fond
de la première note est gardée.

### Créer une note

- **Nouvelle note** : une page blanche A4 en paysage, dans le classeur affiché.
- **Depuis un PDF ou une image…** : une note par page du PDF choisie (**Toutes les pages** ou **Certaines pages**,
  par exemple `1-3, 5`), à la taille et dans l'orientation de la page, avec la page en fond pour écrire par-dessus. Une
  image donne une note de la taille d'une feuille A4, dans l'orientation de l'image. Les PDF protégés par un mot de
  passe ne sont pas lus : enregistrez-en une copie sans mot de passe. Pour un long PDF, une barre indique
  l'avancement (« Création des notes : 3 sur 12… ») et **Annuler** arrête la création : aucune note n'est alors gardée
  (« Création annulée : aucune note n'a été gardée »).

### Chercher une note

Tapez dans **Rechercher une note** : Ardoise cherche dans les *titres*, les *étiquettes* et l'*écriture
manuscrite*, sans tenir compte des accents ni des majuscules. Les notes trouvées grâce à l'écriture sont regroupées
sous **Trouvé dans l'écriture manuscrite**. `Ctrl + F` place le curseur dans le champ, `Échap` l'efface.

La recherche dans l'écriture utilise la *reconnaissance d'écriture manuscrite de Windows*, hors connexion, sur ce PC.
Il faut que le module de la langue soit installé dans Windows (le français l'est en général ; pour l'anglais :
*Paramètres de Windows > Heure et langue > Langue et région*, la langue, puis *Options de langue > Écriture manuscrite
> Télécharger*, puis redémarrer Ardoise). Les réglages sont dans **Réglages**, **Reconnaissance d'écriture** : **Langue de
l'écriture** (**Automatique**, **Français**, **Anglais**), état de la lecture, **Arrêter la lecture** et **Reprendre la
lecture**. Les notes sont lues en arrière-plan ; la lecture se met en pause pendant le mode direct et la présentation,
et un message dit combien de notes sont lues (« notes lues : 12 sur 40 »). Le texte reconnu reste dans un cache sur ce
PC : il n'est jamais écrit dans vos notes ni synchronisé. La reconnaissance se trompe parfois sur une écriture
difficile.

### La corbeille

**Mettre à la corbeille** ne détruit rien tout de suite. Dans **Corbeille**, chaque note a un bouton **Restaurer**
(elle revient où elle était) ou **Supprimer définitivement**. **Vider la corbeille** supprime tout, après confirmation.
Les notes de la corbeille sont supprimées définitivement après 30 jours.

*Se reprendre tout de suite.* Après **Mettre à la corbeille** (le bouton ou la touche `Suppr`), un message s'affiche en
bas de la bibliothèque (« 3 notes mises à la corbeille. ») avec un bouton **Annuler** qui restaure les notes. Il reste
quelques secondes (8), et tant que la souris ou le clavier est dessus. `Ctrl + Z` fait la même chose depuis la
bibliothèque (hors d'un champ de texte, et pas dans l'éditeur, qui a son propre **Annuler**). Les messages **Annuler**
de **Tourner** et de la suppression d'un classeur marchent de la même façon, et `Ctrl + Z` annule la plus récente de
ces actions. Si une note ne peut pas aller à la corbeille (elle a été supprimée entre-temps sur un autre ordinateur,
par exemple), les autres y vont quand même ; celle-là est signalée et reste sélectionnée.

### Notes en conflit et fichiers illisibles

Quand Nextcloud crée une copie « en conflit » d'une note modifiée sur deux PC, la carte porte l'étiquette
**Copie en conflit** : ouvrez les deux versions, gardez celle que vous voulez et mettez l'autre à la corbeille.

Si la bibliothèque contient des fichiers que l'application ne sait pas lire, un bandeau **Fichiers illisibles : N**
s'affiche au-dessus des notes. Il liste chaque fichier avec la raison, dans la langue de l'application : par exemple
une note créée par une version plus récente d'Ardoise demande de *mettre Ardoise à jour* (chapitre 2). Ces fichiers ne
sont pas modifiés. Les fichiers temporaires `.part` laissés par un arrêt brutal sont ignorés, puis nettoyés.

## 8. Modifier une note

Cliquez sur une carte pour ouvrir la note. En haut : **Bibliothèque** (retour), le **Titre de la note**, l'état
(**Enregistrée** ou **Modifications non enregistrées**), **Annuler (Ctrl+Z)**, **Rétablir (Ctrl+Y)** et
**Enregistrer** (`Ctrl + S`). *Rien n'est enregistré tant que vous ne cliquez pas sur Enregistrer* ; en quittant avec des
modifications, Ardoise demande : **Enregistrer et revenir**, **Revenir sans enregistrer** ou **Rester**. Annuler et
rétablir n'ont pas de limite pendant que la note est ouverte.

Si la note a été modifiée entre-temps sur un autre PC, Ardoise *refuse d'écraser l'autre version* : vos modifications
restent à l'écran et **Enregistrer comme copie** les sauve dans une nouvelle note.

### La copie de secours automatique

Tant qu'une note a des modifications non enregistrées, Ardoise en garde une *copie de secours* : environ 2 secondes
après votre dernière modification (au plus 10 secondes après la première, si vous écrivez sans pause). Cette copie est
rangée sur ce PC seulement, dans `%LOCALAPPDATA%\Ardoise\drafts`, jamais dans la bibliothèque ni dans Nextcloud. Elle ne
remplace pas **Enregistrer** : la note du disque n'est pas touchée, et la copie disparaît dès que vous enregistrez (ou
que vous choisissez **Revenir sans enregistrer**).

- *Après un plantage, un redémarrage forcé, une batterie vide ou une mise à jour de Windows*, Ardoise propose de
  récupérer les modifications. Quand vous rouvrez la note, la fenêtre **Modifications récupérées** dit quand la
  dernière copie a été faite. Si la note n'a pas changé depuis, **Récupérer les modifications** les remet dans la note
  (à vous d'enregistrer ensuite). Si la note a changé entre-temps, par exemple sur un autre PC, la fenêtre ne propose
  que **Récupérer dans une nouvelle note** : les modifications vont dans une nouvelle note intitulée « … (récupérée) »,
  sans toucher à la note qui a changé ; Ardoise n'écrase jamais une version plus récente. **Les abandonner** les
  supprime, après confirmation (**Abandonner** ou **Garder**) ; **Plus tard** les laisse en attente.
- Sur l'écran de la bibliothèque, un bandeau « Ardoise s'est fermé sans enregistrer des modifications. Notes
  concernées : N » les liste aussi, avec la date de la dernière copie. Chaque ligne propose **Ouvrir** (la note, où la
  fenêtre ci-dessus s'affiche), ou **Récupérer dans une nouvelle note** quand la note n'existe plus, et **Abandonner**.
- *À la fermeture de session ou à l'arrêt de Windows*, si une note a des modifications non enregistrées, Ardoise écrit
  aussitôt sa copie de secours, et Windows affiche qu'Ardoise empêche l'arrêt, avec la raison : « Une note a des
  modifications non enregistrées. Revenez dans Ardoise pour l'enregistrer. » Vous pouvez revenir dans Ardoise pour
  enregistrer, ou arrêter quand même : la copie de secours est alors déjà sur le disque et Ardoise la proposera au
  prochain lancement. (Quand Windows impose l'arrêt sans attendre, la copie de secours est écrite quand même.)

### Les outils

La barre d'outils propose quatre outils (**Outil**) :

- **Stylo** : choisissez l'**Encre** (**Stylo à bille**, **Stylo plume**, **Feutre**, **Crayon**, **Pinceau**,
  **Surligneur**, **Craie**), la **Couleur** (**Noir**, **Bleu**, **Vert**, **Violet**, **Rouge**, **Orange**, **Gris**,
  ou **Choisir une autre couleur** ; les couleurs personnalisées sont mémorisées) et l'**Épaisseur**. L'épaisseur du trait
  suit la pression du stylet. **Appliquer à toute la note :** **Couleur**, **Encre** ou **Épaisseur** change tous les
  traits d'un coup.
- **Gomme** : **Traits entiers** ou **Partielle**, avec sa **Taille**.
- **Sélection** (le lasso) : entourez des traits pour les sélectionner ; `Maj` + lasso ajoute, `Ctrl` + lasso retire. Le
  panneau **Sélection** propose **Copier**, **Couper**, **Coller**, **Dupliquer**, **Redresser les formes**, **Supprimer**.
  Les changements de couleur, d'encre et d'épaisseur s'appliquent alors à la sélection ; on la déplace (en la saisissant
  à l'intérieur de son cadre) et on la redimensionne (par les poignées du cadre) à la souris, ou au stylet (voir ci-dessous
  et le chapitre 12).
- **Déplacer la page** : faire glisser la page (la barre d'espace maintenue, ou le bouton du milieu de la souris,
  font la même chose avec n'importe quel outil).

**Zoom** : la molette zoome autour du curseur ; en bas à droite, **Agrandir**, **Réduire**, **Ajuster** (page
entière, `Ctrl + 0`) et **Largeur** (`Ctrl + 2`, la page sur toute la largeur de la fenêtre). Ces deux raccourcis
marchent aussi sur un clavier AZERTY, où les chiffres s'obtiennent avec `Maj` : `Ctrl + à` et `Ctrl + é` suffisent,
sans `Maj`, de même que `Ctrl + Maj + à`.

*Avec le stylet.* Quand vous avez fait la sélection avec le *bouton du stylet* (le lasso, quel que soit l'outil), la
pointe du stylet, comme un doigt, saisit ensuite la sélection au lieu de dessiner : posée dans le cadre de la
sélection, elle la déplace ; posée sur une poignée du cadre, elle la redimensionne. Cela vaut avec les outils **Stylo**
et **Gomme** : pas besoin de repasser par l'outil **Sélection**. Hors du cadre, le stylet dessine comme d'habitude, et
`Échap` désélectionne.

### Redresser les formes

Sélectionnez des traits, puis **Redresser les formes** (ou la touche `F`) : les droites, cercles, ellipses, rectangles
(même tournés) et flèches tracés à main levée deviennent des formes nettes, avec le même outil, la même couleur et la
même épaisseur. L'écriture et tout ce qui n'est pas une forme nette ne changent pas. Ardoise redresse les droites et
flèches d'au moins 1 cm, les cercles, ellipses et rectangles d'environ 1,5 cm ou plus ; un petit ovale debout (moins de
2,5 cm) ressemble à un « O » ou à un « 0 » et reste tel quel. Sélectionnez plutôt les dessins que le texte. Une seule
action annule tout (`Ctrl + Z`).

### Copier et coller

`Ctrl + C`, `Ctrl + X` et `Ctrl + V` (ou les boutons du panneau **Sélection**) copient, coupent et collent des traits
*d'une note à l'autre*, d'une fenêtre à l'autre, par le presse-papiers de Windows. Les traits collés gardent leur
couleur, leur outil et leur épaisseur ; ils sont rejoués en dernier. Ce qui ne vient pas d'Ardoise est ignoré.

### La feuille : papier, modèle, format, orientation

Le panneau **Page** (à droite) règle la feuille :

- **Papier** : **Blanc**, **Crème**, **Kraft**, **Ardoise noire** (ou **Personnalisé**) ; **Modèle** : **Uni**,
  **Ligné**, **Seyès**, **Quadrillé 5 mm**, **Millimétré**, **Pointillé**, **Portées** ; **Texture du papier**.
- **Orientation de la feuille** : **Portrait** ou **Paysage**. **L'écriture ne tourne pas** : seule la feuille change de
  sens, et l'écriture garde sa place (si elle dépasse, elle est replacée dans la feuille quand elle y tient ; sinon la
  partie hors cadre est conservée mais pas affichée, et un message le dit).
- **Format de la feuille** : **A4**, **A5**, **Letter**, **Tablette** (216 × 297 mm, la page de la Bamboo Slate) ou
  **Personnalisé** (largeur et hauteur en millimètres, puis **Appliquer**). **Adapter l'écriture à la feuille** réduit ou
  agrandit l'écriture et l'image de fond avec la feuille, sans déformation.
- **Tourner la feuille avec l'écriture** (**Gauche** ou **Droite**) : un quart de tour de tout ensemble, comme tourner
  une feuille de papier.
- **Recadrer autour du contenu** (automatique) et **Recadrer à la main** (tracez le rectangle à garder).

### Image ou PDF de fond

Dans le panneau **Page**, la section **Image ou PDF de fond** place une image (PNG ou JPEG) ou une page de PDF *sous*
l'écriture, pour annoter une fiche, un document ou une photo : **Choisir un PDF ou une image…**, la page du PDF si
besoin, puis **Mettre en fond**. Réglez le **Placement** (**Entière**, **Pleine page**, **Étirée**) et l'**Opacité** ; **Remplacer…**
change le fichier, **Retirer l'image** l'enlève, **Tourner l'image seule** tourne l'image sans toucher à la feuille ni
à l'écriture. Si l'image n'a pas l'orientation de la note, Ardoise propose de passer la note dans l'orientation de
l'image. Tout est annulable. L'image suit les rotations, découpes et assemblages et figure dans les miniatures, les
exports et les vidéos. Les PDF sont rendus par Windows.

### Calques

Le panneau **Calques** liste les calques. **Ajouter un calque** en crée un ; on peut choisir celui sur lequel dessiner,
l'afficher ou le masquer, le renommer, le monter ou le descendre, et le fusionner avec celui du dessous. Un calque
masqué n'accepte pas l'écriture : affichez-le d'abord.

### Découper une note

**Découper** détache une partie de la note dans une nouvelle note, au choix :

- **À un instant** : faites glisser l'**Instant de la découpe** sur la ligne du temps ; tout ce qui a été écrit à partir
  de là passe dans la nouvelle note (titre proposé : « … (suite) »). Les traits concernés sont en couleur.
- **Selon une zone** : tracez sur la page, comme avec le lasso, la zone à détacher (titre proposé : « … (extrait) »).

Ardoise indique combien de traits restent et combien partent, et vous laisse changer le **Titre de la nouvelle note**.
Chaque note doit garder au moins un trait.

### Revenir à l'original

Pour une note reçue de la tablette, **Revenir à l'original** (panneau **Original**) rétablit l'encre telle qu'elle est
arrivée, sans vos modifications. Ardoise précise ce qui est perdu (traits ajoutés, effacés, déplacés, calques, rotation,
format) et ce qui reste (titre, étiquettes, favori, papier, narration, image de fond). L'action est annulable
(`Ctrl + Z`) ; enregistrez ensuite pour la garder. Pour une note qui n'a jamais été modifiée, il n'y a pas encore
d'original : Ardoise garde la version précédente la première fois que vous enregistrez une modification de l'encre, ou
la fois où le mode direct reprend la feuille et y écrit (voir le chapitre 6). L'original est toujours la *première*
version de la note, jamais l'état juste avant la dernière modification.

## 9. Relecture, narration et vidéo

### Relecture

Le bouton **Relecture** (ou la touche `R`) rejoue le tracé *tel qu'il a été écrit*. La page n'est pas modifiée.

- **Lecture** / **Pause** (`Espace`), et la ligne du temps **Position dans la relecture** (les flèches du clavier avancent
  ou reculent d'une seconde quand elle a le focus).
- **Vitesse** : de 0,25× à 16× (0,25 ; 0,5 ; 1 ; 2 ; 4 ; 8 ; 16).
- **Supprimer les temps morts** : les pauses de plus d'une demi-seconde entre deux traits sont ramenées à une
  demi-seconde.
- `R`, `Échap` ou le bouton quittent la relecture.

Les écrans tactiles marchent comme dans l'éditeur : un doigt fait défiler, deux doigts zooment.

### Narration

Pendant une relecture, **Enregistrer une narration** (dans le panneau **Narration**) vous laisse raconter pendant que le
tracé défile ; le son est rangé *dans la note*. La narration suit la vitesse et les temps morts de la relecture
au moment où vous la démarrez. Choisissez le **Micro** (la liste mémorise votre choix sur ce PC ; **Actualiser la liste
des micros** si vous venez de le brancher) et regardez le **Niveau du micro** avant de lancer : **Démarrer**, puis
**Arrêter et enregistrer**. Un nouvel enregistrement remplace le précédent. **Lire avec la narration** rejoue tracé et
son ensemble ; **Supprimer la narration** l'enlève. *Enregistrez la note (`Ctrl + S`) avant* : le son est ajouté à la
note telle qu'elle est sur le disque.

Si Windows bloque le micro, vous obtenez du silence et Ardoise le détecte : ouvrez *Paramètres > Confidentialité et
sécurité > Microphone* et activez *Autoriser les applications de bureau à accéder à votre microphone* (un interrupteur
de coupure du micro, sur certains PC, donne le même résultat).

En mode direct, **Enregistrer la narration** enregistre le son pendant la séance, découpé page par page.

### Exporter en vidéo

Le bouton **Vidéo** ouvre **Exporter en vidéo**. Quatre formats :

| | **MP4 (vidéo)** | **GIF animé** | **WebP animé** | **WebM (vidéo)** |
|---|---|---|---|---|
| Se lit avec | Tout : Windows, téléphone, PowerPoint, ENT | Navigateur, courriel, diapositive : sans lecteur | Navigateurs et logiciels récents | Navigateurs, Moodle, VLC |
| Son (narration) | Oui | Non | Non | Oui |
| Fond transparent | Non | Oui | Oui | Non |
| Taille du fichier | Petite | Grosse | Moyenne | Petite |
| Durée de l'export | Rapide | Très rapide | Longue | La plus longue |

En cas de doute : le MP4, qui se lit partout et porte le son ; le GIF pour une courte animation qui doit s'afficher sans
lecteur.

Réglages : **Taille de l'image** (**720p**, **1080p**, **4K** ou **Taille de la page**), **Images par seconde** (10, 15,
24, 30 ou 60), **Durée de la vidéo** (une **Vitesse** ou une **Durée cible** en secondes), **Supprimer les temps morts**,
**Fond** (**Avec le papier** ou **Transparent**), **Image finale figée** (de 0 à 60 secondes), **Inclure la narration**.
La taille, la durée et le *temps d'export estimé* s'affichent avant de lancer ; **Exporter…** demande ensuite le fichier
à créer, une barre montre l'avancement et **Annuler l'export** l'arrête. Le fichier n'apparaît qu'une fois complet. Le
WebM (compression AV1) est nettement plus lent : réduisez la taille ou le nombre d'images par seconde si l'estimation
est longue.

*MP4 sur une édition « N » de Windows.* Ardoise écrit le MP4 avec l'encodeur intégré à Windows. Il manque sur les
éditions « N » installées sans le *Media Feature Pack*. Dans **Réglages**, la section **Encodeur vidéo de secours
(OpenH264)** vous laisse alors **Télécharger depuis Cisco** l'encodeur OpenH264 (le fichier vient du serveur de Cisco et son
empreinte est vérifiée ; la licence est affichée ; **Annuler le téléchargement**, **Supprimer** et la case
**Autoriser Ardoise à utiliser OpenH264 pour les MP4** vous gardent la main). Ce téléchargement n'a lieu que si vous le
demandez ; OpenH264 n'est jamais fourni avec Ardoise. Sans lui, choisissez le WebM. Avec ce secours, le son du MP4 est en
Opus : lisible dans les navigateurs et VLC, mais pas dans le Lecteur multimédia de Windows.

## 10. Exporter, imprimer, partager

### Exporter une note ou une sélection

**Exporter…** (barre d'outils de l'éditeur, ou barre de la sélection dans la bibliothèque pour plusieurs notes) ouvre
la fenêtre **Exporter** avec la question **Quel format choisir ?** :

| | **PNG (image)** | **JPG (image)** | **WebP (image)** | **SVG (vectoriel)** | **PDF** |
|---|---|---|---|---|---|
| Net quand on agrandit | Non | Non | Non | Oui | Oui |
| Fond transparent | Oui | Non (fond blanc) | Oui | Oui | Non |
| Plusieurs notes dans un fichier | Non (un fichier par note) | Non | Non | Non | Oui (une page par note) |
| S'ouvre avec | Tous les logiciels | Tous les logiciels | Navigateurs, Windows 11, logiciels récents | Navigateur, Inkscape, Word récent | Tout lecteur PDF |
| Idéal pour | Word, OneNote, diapositive | Site ou formulaire qui exige un JPG | Page web, messagerie | Retoucher le dessin | Imprimer, envoyer, archiver |

En cas de doute : le **PDF** pour imprimer ou envoyer une note, le **PNG** pour l'insérer dans un document.

Réglages : **Résolution** (**96 ppp (écran)**, **150 ppp (usuel)**, **300 ppp (impression)**), **Qualité JPG**, **Page du
PDF** (**Taille réelle de la page** ou **A4 (note ajustée et centrée)**), **Inclure le fond** (décoché, seul le tracé est
exporté). Ardoise vous demande ensuite le fichier à créer (ou le dossier, pour un export à un fichier par note : aucun
fichier existant n'est alors écrasé). Les résolutions qui donneraient une image de plus de 16 384 pixels de côté ne
sont pas proposées ; utilisez alors le PDF ou le SVG. Le SVG garde l'épaisseur du trait selon la pression ; les
calques apparaissent comme calques dans Inkscape. Tous les formats dessinent les traits exactement comme l'écran.

### Le texte de l'écriture

Au bas de la même fenêtre, **Texte reconnu (écriture manuscrite)** donne l'écriture lue par Windows : **Enregistrer en
.txt…**, **Enregistrer en Markdown…**, **Enregistrer en Word…** ou **Copier le texte**. Le Markdown et le Word gardent la
structure : titre, date et étiquettes en tête, puis paragraphes, listes et dessins (images). Un aperçu est disponible.
Si Windows n'a pas pu analyser la mise en page, Ardoise le dit : la structure est alors déduite du texte et peut être
moins juste. *Relisez toujours le texte* : la reconnaissance peut se tromper.

- *Les mots sont les mêmes* dans le .txt, le Markdown, le Word et le texte copié : ce sont ceux de la reconnaissance de
  la note entière. L'analyse de mise en page de Windows ne fournit que la structure (paragraphes, listes, dessins).
- *Les corrections l'emportent.* Si un texte corrigé a été enregistré pour la note (par l'assistant, chapitre 14), c'est
  lui qui est exporté, partout, au lieu du texte reconnu.
- *Un trait sous des mots* les souligne : il devient du **gras** en Markdown (qui n'a pas de souligné) et un vrai
  soulignement dans Word. Un trait qui barre des mots les barre (`~~mot~~` en Markdown, texte barré dans Word). Ces
  traits ne sont pas exportés comme des images.
- **Inclure les dessins (Markdown, Word)** : les schémas et figures sont placés comme des images entre les paragraphes
  (dans Word, dans le document ; en Markdown, en fichiers PNG dans un dossier à côté du fichier `.md`, sans jamais
  remplacer un fichier existant). Un dessin posé à côté d'un paragraphe vient après lui. La case n'est cochée que si
  les notes ont des dessins ; sinon Ardoise écrit « Aucun dessin trouvé : le texte seul est exporté ». Décochez-la pour
  n'avoir que le texte.

### Copier, imprimer, glisser

- **Copier l'image** : met la note dans le presse-papiers de Windows ; collez-la dans Word ou OneNote.
- **Imprimer…** (éditeur, ou barre de la sélection) : la fenêtre d'impression de Windows s'ouvre pour choisir
  l'imprimante. Chaque note est imprimée sur une page, dans son orientation. Vous réglez **Imprimer le fond** et la
  **Taille sur la feuille** (**Taille réelle** ou **Ajustée à la feuille**).
- *Glisser hors d'Ardoise* : la poignée d'une carte de la bibliothèque dépose une *image PNG* de la note dans
  l'Explorateur, sur le bureau, dans Word, OneNote ou un courriel (une image par note si plusieurs sont sélectionnées).
  Gardez le bouton enfoncé pendant la préparation de l'image.

### Montrer la feuille en direct

Pour que la feuille (ou la note ouverte) s'affiche dans le navigateur d'un autre ordinateur du réseau, sans fichier à
envoyer : **Partager sur le réseau**, voir la fin du chapitre 6.

## 11. Importer les notes d'Inkspace

Si vous avez utilisé Wacom Inkspace sur ce PC, Ardoise peut reprendre ses notes. Ardoise ne modifie ni ne supprime rien
chez Inkspace : elle ne fait que lire ses données.

### Depuis l'application

1. Fermez Inkspace s'il est ouvert (si Ardoise ne peut pas lire ses fichiers, elle vous le dit : « fermez Inkspace puis
   réessayez »).
2. Dans la barre latérale de la bibliothèque, cliquez sur **Importer depuis Inkspace…**. (Au premier lancement, la fenêtre
   s'ouvre toute seule si vous avez laissé cochée la case d'Inkspace, chapitre 3.)
3. La fenêtre **Importer depuis Wacom Inkspace** cherche Inkspace sur ce PC (« Recherche d'Inkspace sur cet
   ordinateur… »). Quand elle le trouve, elle affiche le **Dossier d'Inkspace** et le nombre de notes (« 24 notes
   trouvées »), en précisant combien sont *déjà importées* (dans la bibliothèque ou sa corbeille : elles seront
   ignorées) et combien sont illisibles.
4. Si Inkspace n'est pas trouvé (« Ardoise n'a pas trouvé Wacom Inkspace sur cet ordinateur… »), ou si vous voulez
   importer depuis une copie (sur une clé USB, par exemple), cliquez sur **Choisir un dossier…** (ou **Choisir un autre
   dossier…**) et indiquez le dossier `LocalState` d'Inkspace, ou une copie de ce dossier.
5. **Classeur de destination** : « Inkspace » par défaut. Tapez un autre nom, ou choisissez un classeur existant dans la
   liste proposée ; laissé vide, le champ range les notes à la racine de la bibliothèque.
6. Cliquez sur **Importer N notes**. Une barre d'avancement indique « Import des notes : 3 sur 24… ». **Arrêter
   l'import** s'arrête après la note en cours : les notes déjà importées sont complètes, et relancer l'import ajoute les
   autres.
7. Un compte rendu s'affiche (**Import terminé**, ou **Import arrêté**) : combien de notes sont importées et dans quel
   classeur, combien étaient déjà importées auparavant (ignorées), et les notes qui n'ont pas pu l'être, avec leur date, le
   début de leur identifiant et la raison. **Voir les notes importées** ouvre le classeur.

Importer plusieurs fois ne crée pas de doublon : une note déjà importée est ignorée, même si vous l'avez renommée,
déplacée ou mise à la corbeille.

### En ligne de commande

La même importation existe sans fenêtre : dans PowerShell, dans le dossier d'`ardoise.exe`,

```
.\ardoise.exe --import-inkspace --library "C:\Users\Prénom\Nextcloud\Ardoise\Notes" | Out-String
```

(`| Out-String` fait attendre PowerShell et affiche le compte rendu à la fin ; ajoutez en premier argument le dossier
`LocalState` d'une copie si Ardoise ne trouve pas Inkspace.) Sans `--library`, elle affiche seulement le tableau des
notes trouvées. Le compte rendu, en français, se termine par « N/N notes importées ».

### À savoir

- Inkspace ne garde pas la *pression* mais l'épaisseur du trait : Ardoise estime la pression à partir de l'épaisseur.
  L'aspect est très proche, mais pas identique.
- Inkspace ne garde pas toujours le *minutage* de l'écriture : quand il manque, Ardoise l'invente (colonne
  « synthétisé » du compte rendu de la ligne de commande), et la relecture de ces notes n'est qu'indicative.
- Les notes importées sont écrites à l'*outil stylo à bille*, sur fond blanc, sans titre ni papier ; vous pouvez les
  retoucher comme les autres.
- Le nuage de Wacom et la collaboration d'Inkspace n'existent pas dans Ardoise : seules les notes présentes sur le PC
  sont reprises.

## 12. Stylet et écran tactile

Sur un PC à écran tactile et stylet (une Surface, par exemple), l'éditeur, la relecture, l'aperçu du mode direct et la
présentation s'utilisent au stylet ou au doigt.

*Le stylet*

- Le trait *s'épaissit avec la pression*.
- Le *dos du stylet* (la gomme) *efface*, avec les réglages de la gomme (**Traits entiers** ou **Partielle**), quel que
  soit l'outil choisi. Un cercle de gomme apparaît quand il survole la page.
- Le *bouton du stylet* trace un *lasso de sélection*, quel que soit l'outil. Ensuite, la *pointe* du stylet (comme un
  doigt) saisit la sélection au lieu de dessiner : posée dans le cadre, elle la déplace ; posée sur une poignée, elle la
  redimensionne (avec les outils **Stylo** et **Gomme**). Hors du cadre, elle dessine comme d'habitude.
- La *paume* posée sur l'écran est ignorée tant que le stylet est utilisé ou proche de l'écran.

*Le doigt*

- Par défaut, *un doigt ne dessine pas* : il fait *défiler* la page, *deux doigts zooment* autour d'eux, un *double
  toucher* ajuste la page entière.
- Pour un écran tactile sans stylet, cochez **Dessiner au doigt** (dans **Réglages**, section **Stylet et tactile**, ou la case
  **Doigt** de la barre de zoom) : le doigt dessine alors comme la souris, et le zoom à deux doigts marche toujours. Ce
  réglage est propre à chaque PC. Avec l'outil **Déplacer la page**, ou en relecture, un doigt fait toujours défiler.

*Le dos du stylet et le bouton ne peuvent pas être simulés par les tests automatiques ; ils sont à essayer sur le PC
tactile.*

## 13. Diagnostics et essais avec la tablette

*Demande la tablette.* Si quelque chose ne marche pas avec la tablette, ou pour vérifier une installation, Ardoise sait
se tester et garder la trace de ce qu'elle échange avec elle.

### Tester la tablette

Cliquez sur **Tester la tablette** (en bas de la barre latérale, ou dans **Réglages**). Le **Test de la tablette** déroule
huit étapes : **Adaptateur Bluetooth**, **Pages de référence**, **Recherche de la tablette**, **Connexion**,
**Informations**, **Téléchargement des pages**, **Mode direct**, **Reconnexion**. Il dure environ 5 minutes ; suivez les
consignes à l'écran, cliquez sur **C'est fait, continuer** après chaque action, **Passer cette étape** si une étape bloque,
**Arrêter le test** pour terminer. Pour les pages de référence, dessinez : un carré et un cercle aussi grands que possible ;
« Ardoise 2026 » en écriture normale ; trois traits horizontaux, très léger, normal, très appuyé.

Deux cases à connaître : **Effacer les pages de la tablette une fois ajoutées à la bibliothèque comme notes** (cochée
par défaut) et **Simulation (sans tablette)**. Le test peut enregistrer la tablette si elle ne l'est pas encore, avec les mêmes
conséquences qu'au chapitre 4.

> *Attention aux pages qui sont déjà sur la tablette.* Pendant le test, les pages en mémoire sont téléchargées, puis
> effacées de la tablette si la case est cochée. Chaque page est d'abord enregistrée et vérifiée : une copie brute est
> gardée dans le dossier des diagnostics (voir plus bas) et la page est ajoutée à la bibliothèque comme une note, comme
> dans la synchronisation normale (une page déjà présente n'est pas ajoutée deux fois). Ce n'est qu'ensuite que la
> tablette l'efface ; si une page ne peut pas être enregistrée, elle reste sur la tablette. Si la tablette contient des
> pages importantes, vous pouvez aussi les récupérer d'abord par la synchronisation normale (chapitre 5).

À la fin, le résumé indique « N réussie(s), N échec(s), N ignorée(s) » et l'emplacement du journal.

### Où sont les journaux

Chaque échange Bluetooth est noté (heure, sens, octets, interprétation) dans la bibliothèque, dans
`_ardoise\diagnostics\` : `AAAA-MM-JJ_HHMMSS.jsonl` (le journal complet), `.txt` (le rapport lisible) et, pour chaque
page reçue, un fichier brut et un aperçu SVG. Un journal est aussi écrit automatiquement quand une connexion échoue.
*Comme ce dossier est dans la bibliothèque, Nextcloud le ramène tout seul sur l'autre PC* : vous n'avez rien à
envoyer à la main. Ces journaux contiennent les échanges avec la tablette, donc les pages qu'elle a envoyées pendant le
test ; ils ne contiennent pas le reste de votre bibliothèque.

### Les essais sur le matériel

Le dépôt du projet contient `docs/hardware-tests.md`, la liste détaillée des essais à faire avec le vrai matériel
(l'exécutable sur le PC du travail, la tablette, la synchronisation, le mode direct et le projecteur, le stylet et
le tactile). L'*autotest* vérifie les réglages, WebView2, l'interface, les traductions et la présence du Bluetooth :

```
.\ardoise.exe --self-test | Out-String
```

Sa dernière ligne est `SELF-TEST OK` ou `SELF-TEST FAILED`, et le compte rendu est aussi écrit à côté des réglages.

## 14. L'assistant (Claude Code, Claude Desktop)

Ardoise peut être pilotée par *votre propre assistant* (Claude Code, Claude Desktop ou tout logiciel compatible avec
le protocole MCP) : il lit vos notes, les voit comme des images, les met au propre, les titre, les range. Ardoise elle-même
ne contient aucune IA et reste utilisable hors ligne ; c'est le logiciel de l'assistant qui fait le travail.

**Réglages**, section **Assistant (MCP)**, donne le texte exact à copier dans l'assistant, avec une case **Lecture seule** pour
qu'il ne puisse que lire. Tout est expliqué pas à pas, avec des exemples de demandes, dans le guide
[L'assistant](assistant.fr.md) (English: [the assistant guide](assistant.en.md)). Dans l'archive d'Ardoise, c'est le
fichier `guide\assistant.fr.html` (et `guide\assistant.en.html` en anglais), à ouvrir dans un navigateur ; **Réglages**
le rappelle sous le texte à copier.

## 15. Vie privée

- Vos notes restent dans *votre* dossier, sur vos ordinateurs. Ardoise n'a pas de compte et n'envoie rien nulle part.
- Seule connexion sortante : le téléchargement d'OpenH264 chez Cisco, *seulement si vous le demandez*. Ardoise
  n'écoute sur aucun port réseau, sauf pendant un partage sur le réseau que *vous* avez lancé (chapitre 6), qui ne
  montre que la feuille partagée et s'arrête avec Ardoise.
- La reconnaissance d'écriture est celle de Windows, hors connexion, et le texte reconnu reste dans un cache local à
  chaque PC.
- Les copies de secours et caches sont sous `%LOCALAPPDATA%\Ardoise`, jamais dans la bibliothèque.
- Les *journaux de diagnostic* sont dans la bibliothèque (donc dans Nextcloud) : n'y mettez rien de personnel.
- Si vous utilisez *un assistant* (chapitre 14), ce qu'il lit part chez son fournisseur selon les réglages de votre
  compte. Le texte et l'image de vos notes peuvent contenir des noms d'élèves : réfléchissez avant de lui confier des
  notes d'élèves, et préférez la **Lecture seule** quand il n'a besoin que de lire. Rien n'est jamais supprimé
  définitivement par l'assistant (une note supprimée va à la corbeille).

### Licence et crédits

**Réglages** > **À propos d'Ardoise…** donne la version, la licence (logiciel libre, GPL-3.0 ou ultérieure, avec son
texte complet), le crédit au projet Tuhi, la mention d'OpenH264 de Cisco (et s'il est installé sur ce PC) et la liste
de tous les composants utilisés avec leur licence.

## 16. En cas de problème

| Ce que vous voyez | Ce qu'il faut faire |
|---|---|
| Écran bleu « Windows a protégé votre ordinateur » | Programme non signé : *Informations complémentaires*, puis *Exécuter quand même* (chapitre 2). |
| Windows bloque `ardoise.exe` sans bouton pour continuer | Règle de votre PC (Smart App Control, AppLocker…) : demandez une autorisation au service informatique. |
| Fenêtre vide ou qui ne s'ouvre pas | WebView2 est peut-être absent : installez-le depuis le site de Microsoft. `.\ardoise.exe --self-test \| Out-String` dit si le moteur est présent. |
| « Impossible de lire le fichier de réglages… » | `ardoise.toml` est corrompu ou modifié à la main avec une erreur. Ardoise ne l'écrase pas : corrigez-le ou supprimez-le, puis relancez. |
| « Le Bluetooth est désactivé » | Activez-le dans les paramètres de Windows. |
| « Aucun adaptateur Bluetooth trouvé » | Branchez la clé Bluetooth (4.0 ou plus), attendez que Windows la reconnaisse, réessayez. |
| « La tablette ne répond pas » / « n'est pas connectée » | Rapprochez-la de l'ordinateur et appuyez brièvement sur son bouton ; vérifiez la charge. |
| « La tablette refuse la commande » | L'application du téléphone ou Inkspace l'a peut-être enregistrée depuis : enregistrez-la de nouveau dans Ardoise (chapitre 4). Coupez le Bluetooth du téléphone. |
| « La tablette est occupée » | Une synchronisation ou le mode direct est en cours : réessayez à la fin. |
| Une seule page est reçue, les suivantes « attendent » | Cochez **Effacer de la tablette après import** (chapitre 5). |
| La note s'affiche à l'envers ou couchée | Réglez la **Position de la tablette** pour qu'elle corresponde à la façon dont vous la tenez. |
| « Vos modifications n'ont pas été enregistrées » | La note a changé sur un autre PC. **Enregistrer comme copie**, ou revenez à la bibliothèque et rouvrez la note. |
| « Cette note a été créée par une version plus récente d'Ardoise » | Remplacez `ardoise.exe` par la version récente (chapitre 2). |
| La bibliothèque n'affiche pas une note arrivée d'un autre PC | Attendez la fin de la synchronisation Nextcloud ; les fichiers temporaires `.part` sont ignorés. |
| « Windows ne peut pas écrire de MP4 sur ce PC » | Installez l'encodeur de secours OpenH264 dans **Réglages** (chapitre 9), ou choisissez le WebM. |
| La narration est muette, ou le micro est introuvable | Voir le chapitre 9 (confidentialité du micro) ; **Actualiser la liste des micros**. |
| La recherche ne trouve pas l'écriture | Le module d'écriture de Windows manque pour la langue, ou la lecture est en pause (mode direct) ou arrêtée : voir **Réglages**, **Reconnaissance d'écriture**. |
| « Ce PDF est protégé par un mot de passe » | Enregistrez-en une copie sans mot de passe. |
| « L'impression a échoué » | Vérifiez l'imprimante, ou exportez la note en PDF. |
| Des pages du direct n'ont pas pu être enregistrées | Elles restent en mémoire. Vérifiez le dossier de la bibliothèque (place, droits, synchronisation) ; une copie de secours est écrite dans `%LOCALAPPDATA%\Ardoise\live-backup`. |
| « Ardoise s'est fermé sans enregistrer des modifications » (bandeau de la bibliothèque), ou fenêtre « Modifications récupérées » | Ardoise s'est arrêté avant que vous enregistriez. Rouvrez la note et récupérez les modifications (chapitre 8). |
| « Fichiers illisibles : N » | Ardoise ne sait pas lire ces fichiers et ne les a pas modifiés ; la raison est écrite à côté de chacun (chapitres 2 et 7). |
| Windows dit qu'« Ardoise empêche l'arrêt » | Le mode direct est en cours : ce qui est écrit est enregistré, et vous pouvez arrêter ; ou bien une note a des modifications non enregistrées : revenez dans Ardoise et enregistrez, ou arrêtez quand même (la copie de secours est déjà écrite, chapitre 8). |
| « Ardoise n'a pas trouvé Wacom Inkspace sur cet ordinateur » | Cliquez sur **Choisir un dossier…** et indiquez le dossier `LocalState` d'Inkspace ou une copie (chapitre 11). Si Ardoise dit de fermer Inkspace, fermez-le et réessayez. |
| L'autre ordinateur n'arrive pas à se connecter au partage | Même réseau ? Adresse tapée avec `http://` et le port ? Pare-feu de Windows autorisé pour ce réseau (**Réseaux privés**, ou réseau passé en **Privé**) ? Voir « Montrer la feuille sur un autre ordinateur du réseau », chapitre 6. |
| « Ce site ne peut pas fournir de connexion sécurisée » sur l'autre ordinateur ou le téléphone | Le navigateur a ouvert l'adresse en `https://`, souvent en la complétant tout seul. Le partage d'Ardoise est en `http://` (il reste sur le réseau local, protégé par le code) : corrigez l'adresse en `http://…`. |
| Un message dit, en partageant, que le port est déjà utilisé, ou qu'aucun port n'est libre | Ardoise prend normalement de lui-même le port libre suivant et le dit dans le panneau. Si aucun des 20 ports qui suivent n'est libre, choisissez-en un autre dans **Réglages** > **Partage sur le réseau** > **Port du partage** (chapitre 6). |
| Un second double-clic sur `ardoise.exe` ne fait rien de nouveau | C'est voulu : la fenêtre déjà ouverte revient au premier plan (chapitre 2). |

Si le problème tient à la tablette, lancez **Tester la tablette** (chapitre 13) : le journal, ramené par Nextcloud,
permet de comprendre ce qui s'est passé.

## 17. Raccourcis clavier

Ces raccourcis n'agissent pas quand vous tapez dans un champ de texte.

### Bibliothèque

| Touches | Action |
|---|---|
| `Ctrl + F` | Aller dans le champ de recherche (`Échap` l'efface) |
| `Ctrl + N` | Nouvelle note |
| Flèches, `Début`, `Fin` | Passer d'une carte à l'autre (avec `Maj` : étendre la sélection) |
| `Entrée` | Ouvrir la note |
| `Espace` | Sélectionner la carte (avec `Maj` : une plage) |
| `Ctrl + clic`, `Maj + clic` | Ajouter ou retirer une note, sélectionner une plage |
| `Ctrl + A` | Sélectionner toutes les notes |
| `Échap` | Désélectionner |
| `F2` | Renommer la note |
| `Suppr` | Mettre la note (ou la sélection) à la corbeille |
| `Ctrl + Z` | Annuler la dernière action dont le message **Annuler** est affiché (mise à la corbeille, **Tourner**, suppression d'un classeur) |

### Éditeur

| Touches | Action |
|---|---|
| `Ctrl + S` | Enregistrer |
| `Ctrl + Z` | Annuler |
| `Ctrl + Y`, `Ctrl + Maj + Z` | Rétablir |
| `Ctrl + A` | Tout sélectionner |
| `Ctrl + C`, `Ctrl + X`, `Ctrl + V` | Copier, couper, coller des traits |
| `Suppr` ou `Retour arrière` | Supprimer la sélection |
| `Échap` | Désélectionner |
| `V`, `P`, `E`, `H` | Outils **Sélection**, **Stylo**, **Gomme**, **Déplacer la page** |
| `Espace` (maintenu) | Déplacer la page (sauf si un bouton a le focus : `Espace` agit alors sur ce bouton) |
| `F` | **Redresser les formes** |
| `R` | **Relecture** |
| `Ctrl + 0` | **Ajuster** (page entière). Au clavier AZERTY : `Ctrl + à` ou `Ctrl + Maj + à` |
| `Ctrl + 2` | **Largeur**. Au clavier AZERTY : `Ctrl + é` ou `Ctrl + Maj + é` |
| Molette | Zoom autour du curseur |
| Bouton du milieu (glisser) | Déplacer la page |

### Relecture

| Touches | Action |
|---|---|
| `Espace` | Lecture / pause |
| `R` ou `Échap` | Quitter la relecture |
| Flèches (sur la ligne du temps) | Reculer ou avancer d'une seconde (`←` et `↓` reculent, `→` et `↑` avancent) |
| `Pg préc`, `Pg suiv` (sur la ligne du temps) | Avancer (`Pg préc`) ou reculer (`Pg suiv`) de 10 secondes, comme sur un curseur |
| `Ctrl + 0`, `Ctrl + 2` | **Ajuster**, **Largeur** |

### Mode direct (fenêtre de commande et présentation)

| Touches | Action |
|---|---|
| `→`, `Pg suiv`, `Espace` | Page suivante (`Espace` aussi quand un bouton a le focus) |
| `←`, `Pg préc` | Page précédente |
| `Ctrl + Suppr` (ou `Ctrl + Retour arrière`) | Effacer la page (**Annuler l'effacement** pendant 8 secondes) |
| `B` | **Tableau noir** |
| `L` | **Pointeur laser** |
| `+` / `−` | Agrandir / réduire la zone projetée |
| `0` (au clavier AZERTY, la touche `à`) | **Page entière** |
| `W` | **Largeur** |
| `Maj + flèches` | Déplacer la zone projetée |
| `Suppr` ou `Retour arrière` | Effacer les annotations écrites sur la présentation ; **avec des traits choisis** (fenêtre de commande), les supprimer |
| `V` | Outil **Sélection** (fenêtre de commande) : lasso ou clic sur un trait |
| `Ctrl + Z` | Annuler une suppression, un changement de couleur ou **Effacer la page** (fenêtre de commande) |
| `Ctrl + Y` ou `Ctrl + Maj + Z` | Rétablir (fenêtre de commande) |
| `Échap` | Enlever la sélection s'il y en a une ; sinon fermer la projection si elle est affichée ; sinon, demander s'il faut **Terminer** le mode direct |

### Dans les fenêtres

| Touches | Action |
|---|---|
| `Échap` | Fermer la fenêtre ouverte (export, impression, assemblage, import d'Inkspace…), où que soit le focus ; pas pendant une opération en cours, qu'il faut d'abord arrêter ou laisser finir |
