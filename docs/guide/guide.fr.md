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
4. Enregistrer la tablette (et ajouter la tablette Xiaomi)
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

Le panneau **Tablette** est étroit : trois onglets, des icônes sur un rail à droite (les flèches haut et bas passent de
l'un à l'autre, `Entrée` ouvre celui qui a le focus), et rien à faire défiler.

- **Utiliser** : la tablette connectée (son dessin, son nom, son état, la batterie et le nombre de pages en mémoire),
  puis **Mode direct** et **Synchroniser**, qui dit combien de pages attendent (« 3 pages »). Une pastille sur l'icône
  donne ce même nombre, même quand un autre onglet est ouvert. Dessous : le résultat de la dernière synchronisation et
  ce que dit Ardoise sur l'état (recherche de la tablette, pages qui attendent, échec…).
- **Réglages** : ceux d'*une* tablette à la fois. Quand vous en connaissez deux, une liste choisit laquelle (la tablette
  connectée au départ). Voir « Les options », plus bas.
- **Tablettes** : les tablettes connues, avec ce qu'elles ont indiqué à leur dernière connexion (modèle, batterie, version
  du firmware ou mémoire libre, nombre de pages en mémoire, zone de dessin, date de lecture), **Oublier la tablette**
  pour chacune, **Ajouter une tablette** et **Tester la tablette…**. Ces informations sont lues à la première connexion.

La tablette est ensuite reconnue sur *tous* vos PC qui utilisent la même bibliothèque. Pour la défaire : **Réglages**,
section **Tablette** (ou l'onglet **Tablettes** du panneau), **Oublier la tablette** (elle devra être enregistrée de
nouveau, sur tous vos ordinateurs). Si
vous avez aussi une tablette Xiaomi (ci-dessous), chacune a son propre bouton.

Dans l'onglet **Réglages** du panneau **Tablette**, pour la Bamboo Slate, vous pouvez **Renommer** la tablette (1 à 15
caractères, sans accent) et la **Mettre à l'heure** (l'horloge de la tablette est réglée sur celle du PC).

### Ajouter la tablette Xiaomi

*Demande la tablette Xiaomi.* Ardoise sait aussi lire la **tablette Xiaomi Mijia « petit tableau noir », version
mémoire** (13,5 pouces, Bluetooth ; modèle `jiqid.sxb.v1` dans Mi Home). Elle s'ajoute à côté de la Bamboo Slate, sans
rien enregistrer.

> *À savoir avant de commencer* : la tablette Xiaomi ne parle qu'à **une application à la fois**. Fermez Mi Home, ou
> coupez le Bluetooth de votre téléphone, tant que vous l'utilisez avec Ardoise. Il n'y a ni bouton à maintenir ni
> enregistrement : la tablette **reste liée à Mi Home**, et Ardoise n'utilise ni votre compte Xiaomi ni son service en
> ligne.

1. Allumez la tablette Xiaomi et gardez-la près de l'ordinateur ; le Bluetooth doit être activé sur le PC.
2. Ouvrez le panneau **Tablette** (le bloc **Tablette** de la barre latérale) et cliquez sur **Ajouter la tablette
   Xiaomi** (onglet **Utiliser** tant qu'aucune tablette n'est connue, sinon onglet **Tablettes**, sous **Ajouter une
   tablette**).
3. Lisez ce qu'il faut vérifier et cliquez sur **Chercher la tablette**. La recherche dure une minute au plus ;
   **Annuler** l'arrête à tout moment. Pendant la recherche, la liaison avec la Bamboo Slate, si elle est connectée,
   est fermée : appuyez brièvement sur son bouton ensuite pour la reconnecter.
4. Ardoise liste ce qu'elle entend. Une seule tablette est le cas habituel : cliquez sur **Ajouter cette tablette**.
   S'il y en a plusieurs (un voisin de classe, par exemple), le **Signal** de chacune est affiché : plus il est fort,
   plus la tablette est proche.
5. **Tablette Xiaomi ajoutée** : Ardoise s'y connecte dès qu'elle est allumée et que rien d'autre ne l'occupe.

Si rien n'est entendu, Ardoise le dit (**Aucune tablette Xiaomi trouvée**) : vérifiez que la tablette est allumée et
proche, que Mi Home est fermé (ou le Bluetooth du téléphone coupé) et que le Bluetooth du PC est activé, puis cliquez
sur **Chercher de nouveau**.

Ardoise retient les **deux** tablettes et se connecte à celle qui est allumée. L'onglet **Tablettes** du panneau et
**Réglages** listent chacune, avec son état, et un **Oublier la tablette** par tablette (la Xiaomi s'oublie sans toucher à la
Bamboo Slate, et inversement). On peut ne connaître que la Xiaomi : rien, dans Ardoise, ne suppose une Bamboo Slate.
La tablette Xiaomi est reconnue sur *tous* vos PC qui utilisent la même bibliothèque.

Pour la tablette Xiaomi, le panneau donne : la **batterie**, la **mémoire libre** (par exemple « 126,4 Mo sur 126,9 Mo »), le
nombre de **pages en mémoire** et le **modèle**. Elle n'a ni nom à changer ni version de firmware, et Ardoise la met à
l'heure à chaque connexion : ces réglages n'apparaissent pas pour elle. En revanche, elle se tient, elle aussi, dans
quatre positions (voir « Les options », plus bas).

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

Les pages sont reçues automatiquement à la connexion, ou à la demande avec **Synchroniser** (onglet **Utiliser** du
panneau **Tablette**). Une barre indique **Page 1 sur 3**… et **Annuler** arrête la synchronisation (les pages déjà reçues
restent des notes, les autres restent sur la tablette). Le panneau résume ensuite : **3 notes enregistrées.**,
**3 pages effacées de la tablette.** ou **Aucune nouvelle page.**

Chaque page devient une *note* à la racine de la bibliothèque, sans titre (la carte affiche la date), à la taille de
la tablette. Vous pouvez la titrer, la ranger, l'étiqueter (chapitre 7).

### Les options

Dans l'onglet **Réglages** du panneau **Tablette** (et dans **Réglages**, section **Tablette**), on règle une tablette à
la fois : s'il y en a deux, choisissez-la dans la liste **Tablette à régler**. Les deux premières options valent pour
toutes vos tablettes, et l'écran le dit quand vous en connaissez deux.

- **Synchroniser automatiquement à la connexion** : les pages sont reçues dès que la tablette est à portée.
- **Effacer de la tablette après import** (activé par défaut) : une page n'est effacée de la tablette qu'*après* que
  son fichier a été écrit dans la bibliothèque et vérifié ; si quelque chose échoue, elle reste sur la tablette.
  **Attention** : la Bamboo Slate ne livre que sa page la plus ancienne. Sans cette option, les autres pages restent
  illisibles tant que la première n'est pas effacée, et le panneau le dit (« N pages attendent sur la tablette… »).
  Gardez donc l'option cochée sauf raison particulière.
- Pour la tablette Xiaomi, cette option change ce que fait **Synchroniser** (voir « Les pages de la tablette
  Xiaomi », plus bas) : désactivée, ses pages ne sont importées qu'à la demande, après confirmation, et sont effacées de
  la tablette quand même.
- **Bouton de la tablette en mode direct** (Bamboo Slate) : **Nouvelle page** ou **Nouveau calque** (chapitre 6).
- **Position de la tablette** (Bamboo Slate) : comment vous tenez la tablette. Ardoise redresse ce que vous dessinez pour qu'il
  apparaisse à l'endroit. Quatre positions : **Paysage, bouton en bas**, **Paysage à l'envers, bouton en haut** (qui
  convient aux gauchers), **Portrait, bouton à gauche**, **Portrait, bouton à droite**. Les positions en portrait
  donnent des notes en portrait. Le bouton est celui qui est à côté du voyant de la tablette.
- **Position de la tablette** (tablette Xiaomi) : on peut la tenir dans n'importe quel sens, et Ardoise redresse ce que
  vous écrivez, pour les pages importées comme pour le direct. Quatre positions, dessinées d'après la tablette (la
  corbeille efface l'écran, la disquette enregistre la page) : **En hauteur, boutons en bas** (au départ), **En
  hauteur, boutons en haut**, **En largeur, boutons à droite**, **En largeur, boutons à gauche**. Les positions en
  largeur donnent des notes en largeur (environ 27 × 20 cm). La position en vigueur au début d'une synchronisation vaut
  pour toutes ses pages. Pendant le direct, les quatre boutons de position de la fenêtre de commande changent ce même
  réglage, et le direct en cours le suit aussitôt (voir « Le mode direct avec la tablette Xiaomi »). Une note importée
  garde la position avec laquelle elle a été faite : changer le réglage après coup ne la retourne pas.

### Les pages de la tablette Xiaomi

Écrivez sur la tablette Xiaomi, puis appuyez sur son bouton d'enregistrement : la page est gardée dans sa mémoire.
Ardoise les reçoit comme celles de la Bamboo Slate : à la connexion, ou avec **Synchroniser**. Chaque page
devient une note, **datée du moment où elle a été enregistrée sur la tablette**, en hauteur (environ 20 × 27 cm), ou en
largeur si la tablette est tenue boutons à droite ou à gauche (voir « Les options »).

Cette tablette ne donne que sa page la plus ancienne : la suivante ne se lit qu'une fois celle-ci effacée. Ardoise
**efface donc chaque page de la tablette une fois sa note écrite et vérifiée** (c'est aussi ce que fait Mi Home). Si
la note ne peut pas être écrite, ou si le transfert s'interrompt, la page reste sur la tablette (sauf la page d'un
écran non effacé, voir « La page d'un écran non effacé », plus bas). Une page déjà importée est reconnue et n'est pas
importée de nouveau.

- Avec **Effacer de la tablette après import** activé (le réglage de départ), tout se passe comme pour la Bamboo
  Slate : synchronisation automatique, sans question.
- Avec cette option désactivée, Ardoise n'importe les pages de la tablette Xiaomi **qu'à votre demande**. Le panneau dit
  « N pages attendent sur la tablette Xiaomi » ; **Synchroniser** demande d'abord de confirmer que les pages
  seront effacées de la tablette une fois importées (**Synchroniser et effacer**).
- La progression compte les pages une à une, et le nombre de **pages en mémoire** diminue au fur et à mesure.
- Après un transfert qui ne s'est pas terminé proprement (interrompu ou annulé en cours de page), Ardoise ferme la
  liaison Bluetooth et la rétablit avant de s'en servir de nouveau pour une page ou pour le direct (tout de suite pendant
  une synchronisation, sinon au moment voulu) : c'est normal, et cela peut prendre un moment.

#### La page d'un écran non effacé

La tablette traite à part la page d'un écran qui **n'a pas été effacé depuis son enregistrement** (c'est le cas courant :
on écrit, on enregistre, on synchronise sans effacer l'écran). Elle **retire cette page d'elle-même dès qu'Ardoise l'a
téléchargée** ; la page d'un écran effacé, elle, reste jusqu'à ce qu'on l'efface. Pour qu'aucune page ne soit perdue,
Ardoise procède ainsi :

- Avant d'écrire la note, elle garde une **copie de secours** des octets reçus, dans le dossier `_ardoise\rescued-pages\`
  de la bibliothèque (fichier `page-<heure>.bin`, avec un `page-<heure>.json` quand la page remplace un enregistrement
  précédent du même écran) ; si ce dossier ne peut pas être écrit, dans `%LOCALAPPDATA%\Ardoise\blackboard-rescue\` sur
  ce PC (un sous-dossier `library-…` par bibliothèque : une copie n'est importée que dans sa bibliothèque). Une fois la
  note écrite et relue, la copie est supprimée : en temps normal, ce dossier est vide ou absent.
- Si la note ne peut pas être écrite (bibliothèque inaccessible, disque plein), la copie reste, Ardoise le dit (« Une page
  de la tablette Xiaomi n'a pas pu être écrite dans la bibliothèque… ») et **importe la page toute seule à la prochaine
  synchronisation** de la tablette Xiaomi, avant les pages de la tablette. Vérifiez que la bibliothèque est accessible et
  qu'il reste de la place.
- Si ni la bibliothèque ni le PC ne peuvent recevoir la copie, Ardoise garde la page en mémoire et réessaie à la
  synchronisation suivante (« Une page de la tablette Xiaomi n'a pu être écrite ni dans la bibliothèque ni sur ce
  PC… ») : **ne fermez pas Ardoise** d'ici là.
- Si le transfert d'une telle page ne s'est pas terminé proprement (interrompu, plus d'octets que prévu, illisible comme
  enregistrements du stylet, ou octets arrivés pendant l'écriture de la note), ce qui est arrivé est gardé **à part**,
  dans `_ardoise\rescued-pages\doubtful\`, et n'est jamais transformé en note tout seul ; la fin de la synchronisation
  le dit (« Une page de la tablette Xiaomi n'a pas été reçue proprement… »). Si une note a été faite, vérifiez-la. Cette
  copie douteuse est supprimée toute seule quand la même page arrive ensuite proprement. Vous pouvez en examiner les
  fichiers, ou les donner à `ardoise.exe --import-blackboard-pages DOSSIER` (il signale ceux qui ne sont pas des
  enregistrements du stylet et importe les autres) ; gardez le journal de diagnostic.

**Enregistrer de nouveau un écran non effacé** donne une page qui contient tout l'écran. Ardoise **met alors à jour la note
de l'enregistrement précédent** au lieu d'en ajouter une seconde, quand cette note n'a pas été modifiée depuis son import
et que la nouvelle page la contient en entier. Son titre, ses mots-clés, son favori et son classeur ne comptent pas
comme des modifications : ils sont gardés, comme son fichier et sa date de création. Dans les autres cas (l'encre, les
calques, le papier ou le fond de la note ont été modifiés, elle est à la corbeille ou a été supprimée, ou la nouvelle
page ne la contient pas), une nouvelle note est ajoutée, comme pour n'importe quelle page.

#### Les limites de la tablette Xiaomi

- Elle ne parle qu'à **une application à la fois** : fermez Mi Home ou coupez le Bluetooth du téléphone.
- Les pages vont à **l'application qui les importe la première**, et Mi Home les efface aussi après son import : une page
  importée par Mi Home n'est plus sur la tablette quand Ardoise s'y connecte.
- La tablette **ne donne pas l'heure de chaque point** : la relecture et la vidéo d'une page récupérée rejouent les
  traits à vitesse régulière. (En mode direct, au contraire, Ardoise horodate ce qu'elle reçoit.)
- La page fait environ **20 × 27 cm, en hauteur** (en largeur quand la tablette est tenue sur le côté).
- Ardoise n'utilise ni compte Xiaomi ni service en ligne : la tablette reste liée à Mi Home et à ce compte.

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

Ouvrez le panneau **Tablette** (clic sur le bloc **Tablette** de la barre latérale) et, dans l'onglet **Utiliser**,
cliquez sur **Mode direct**.
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

### Le mode direct avec la tablette Xiaomi

Le mode direct fonctionne de la même façon avec la tablette Xiaomi : outil, couleur, texture, pression, pointeur laser
au survol (la tablette envoie aussi la position du stylo quand il est près de l'écran), autre écran, partage et
retouches. La page du direct est **en hauteur** (environ 20 × 27 cm) quand la tablette est tenue boutons en bas ou en
haut, **en largeur** quand elle est tenue boutons à droite ou à gauche : elle suit la **Position de la tablette** Xiaomi
choisie dans les réglages (voir « Les options »).

Comme avec la Bamboo Slate, la barre du haut de la fenêtre de commande a **quatre boutons de position**, avec les
dessins de la Xiaomi (la corbeille et la disquette montrent où sont ses boutons) : si vous avez oublié de régler la
position avant de commencer, ou si vous vous êtes trompé, cliquez sur la bonne. La page en cours tourne aussitôt avec
ce qui y est déjà écrit, et ce que vous écrivez ensuite arrive à l'endroit. Passer d'une position en hauteur à une
position en largeur (ou l'inverse) change la forme de la page. Le choix est gardé dans les réglages de la tablette
Xiaomi, comme pour la Bamboo : le direct suivant et la synchronisation suivante le prennent. Seule la page en cours
tourne aussitôt : une page déjà terminée garde son sens tant que vous n'y revenez pas, et tourne quand vous y revenez.
Le réglage **Bouton de la tablette en mode direct** ne s'applique qu'à la Bamboo Slate.

- **Enregistrer la page avec le bouton de la tablette** n'interrompt pas le direct. La page est importée ensuite, comme
  une synchronisation : sa note est un doublon de celle du direct, sans danger.
- **Effacer l'écran de la tablette avec son bouton** fait passer le direct à une **nouvelle page** : ce que vous
  écrivez ensuite n'est pas tracé par-dessus la page précédente. Une page encore vide reste la page en cours.
  **Page suivante**, sur l'ordinateur, fonctionne aussi. La tablette peut aussi envoyer toute la page à l'effacement :
  Ardoise la met de côté et ne la dessine jamais (pas de trait parasite) ; le journal de diagnostic dit combien
  d'octets ont été mis de côté.
- **Avant le direct**, Ardoise récupère les pages enregistrées dans la tablette : effacer l'écran pendant le direct peut
  faire perdre la page de l'écran en cours si elle n'a pas été récupérée. Avec **Synchroniser automatiquement à la
  connexion** et **Effacer de la tablette après import** activés, le panneau montre la synchronisation, puis le direct
  commence ; si la synchronisation laisse des pages sur la tablette (une page n'a pas pu être importée), le direct ne
  démarre pas, comme ci-dessous, et le résultat de la synchronisation dit pourquoi. Sinon,
  si la tablette contient des pages, le direct ne démarre pas : une fenêtre dit que la tablette contient encore des
  pages enregistrées (« La tablette Xiaomi contient encore 2 pages enregistrées… ») et propose **Importer et effacer**
  ou **Annuler** (Échap aussi). **Importer et effacer** importe les pages, qui sont effacées de la tablette une fois
  importées, en montrant la progression, puis le direct démarre tout seul ; si des pages restent ou si l'import échoue,
  la fenêtre le dit et vous laisse décider. **Annuler** ramène à la bibliothèque, sans rien importer ni démarrer. Une
  tablette sans page enregistrée démarre comme avant, et un direct qui se reconnecte après une coupure repart aussitôt.
- Une séance reste sur **la tablette avec laquelle elle a commencé**. Si la liaison est perdue, Ardoise attend cette
  même tablette (**Reconnexion…**) et ignore l'autre jusqu'à la fin du direct. Pour continuer avec l'autre tablette,
  terminez le direct et relancez-le.

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
- **L'orientation** (Bamboo Slate) : la feuille n'est jamais tournée. Si elle est en paysage et que la tablette est
  réglée en portrait (ou l'inverse), un avis dit comment tenir la tablette : « Pour écrire sur cette feuille, tenez la tablette ainsi : … ».
  Les quatre boutons de position sont grisés pour cette feuille.
- **Quelle tablette** : la tablette Xiaomi n'écrit que sur une page de la tablette Xiaomi (environ 20 × 27 cm : ses pages
  récupérées et ses notes du direct), et la Bamboo Slate sur toutes les autres feuilles. Une feuille de la Bamboo Slate
  se continue donc avec la Bamboo Slate, une page de la tablette Xiaomi avec la tablette Xiaomi. Si la tablette
  connectée ne convient pas, Ardoise refuse, dit pourquoi (**Le mode direct n'a pas pu reprendre cette feuille.**) et
  n'écrit rien sur la note ; si la tablette ne se connecte qu'ensuite, le message arrive à ce moment-là. **Retour à la
  bibliothèque**, ou **Réessayer** une fois l'autre tablette allumée.
- **Le sens de la page** (tablette Xiaomi) : une page de la Xiaomi en hauteur se continue avec la tablette tenue en
  hauteur (boutons en bas ou en haut), une page en largeur avec la tablette tenue en largeur (boutons à droite ou à
  gauche). Si la page n'est pas dans le sens de la **Position de la tablette** réglée, Ardoise refuse, sans rien écrire
  sur la note, et dit quoi faire (**Cette page de la tablette Xiaomi n'est pas dans le même sens que la tablette telle
  que vous la tenez…**) : choisissez dans les réglages une position du même sens que la page, tenez la tablette ainsi
  puis **Réessayer**, ou reprenez le direct sur une autre feuille. La Xiaomi n'a pas d'avis « tenez la tablette ainsi » :
  elle suit toujours sa position réglée, et ce que vous écrivez est redressé pour cette position (et non pour celle
  avec laquelle la page avait été faite, qui n'est pas retenue pour le direct). Pendant ce direct, la feuille garde sa
  forme : des quatre boutons de position, seuls les deux du même sens qu'elle restent actifs (feuille en largeur :
  boutons à droite ou à gauche ; feuille en hauteur : boutons en bas ou en haut), les deux autres sont grisés et une
  mention à côté dit pourquoi. Changer pour l'autre position du même sens (un demi-tour) ne retourne pas la feuille :
  ce que vous écrivez ensuite arrive à l'endroit, ce qui était déjà écrit reste tel quel. Il en va de même tant que le
  direct garde une page écrite sur une image.
- **La page suivante** crée une note neuve, à la racine de la bibliothèque, avec le papier de la feuille (couleur,
  modèle, texture, pas son image), et suit de nouveau la position réglée pour la tablette : les boutons de position
  fonctionnent alors (avec la tablette Xiaomi, seulement les deux du sens de la feuille reprise, jusqu'à la fin du
  direct). Les onglets **Page** et **Fond** changent le papier et l'image de la feuille en cours (voir
  **Le papier et l'image de la page en cours**).
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

- **Tablette** : **Outil** (stylo à bille, plume, feutre, crayon, pinceau, surligneur, craie), **Couleur** (dont
  **Blanc craie** et **Jaune**) et **Épaisseur** sont le style de la tablette : le trait suivant en tient compte, pas
  les précédents. Le groupe **Ordinateur** (**Sélection**, **Stylo**, l'encre de l'ordinateur, **Annuler**,
  **Rétablir**) sert à retoucher et à corriger : voir plus bas.
- **Pages** : **Page précédente**, **Page suivante** (la position s'affiche : « Page 2 / 5 »), **Nouvelle page**.
  Les calques de la page se gèrent dans l'onglet **Calques** du panneau de côté (voir **Le panneau de côté de la fenêtre
  de commande** plus bas et **Corriger pendant le direct**). **Effacer la page** (ou `Ctrl + Suppr`) efface la page en cours
  seulement ; un message « Page effacée. » propose pendant 8 secondes **Annuler l'effacement**, qui rend l'encre (`Ctrl + Z` la rend aussi, sans limite de temps : voir **Retoucher pendant le direct**). Si la
  page a changé entre-temps, Ardoise dit que l'effacement ne peut plus être annulé.
- **Tableau noir** : fond ardoise et craie blanche. **Pointeur laser** : le stylet en survol, sans toucher la tablette,
  montre un point rouge à l'écran *sans rien écrire* ; en touchant la tablette on écrit de nouveau. Les deux se
  règlent aussi par des interrupteurs, dans l'onglet **Projection** (bloc **Zone projetée**) ; les boutons de la barre
  d'outils restent, pour les avoir sous la main.
- **Position de la tablette** : quatre boutons qui changent aussitôt la position ; la page en cours tourne avec ce
  qui y est déjà dessiné. Avec la tablette Xiaomi, ce sont ses quatre positions, avec ses dessins (voir « Le mode
  direct avec la tablette Xiaomi »).
- L'aperçu montre la page entière ; un *rectangle bleu* indique la zone projetée. Faites-le glisser pour déplacer la
  zone, la molette zoome. **Agrandir**, **Réduire**, **Page entière** et **Largeur** (la page sur toute la largeur de la
  zone projetée) sont aussi proposés, dans l'onglet **Projection**, bloc **Zone projetée**.
- **Latence** : le temps entre la réception du point par l'ordinateur et son dessin, mesuré et affiché (objectif :
  moins de 30 ms).
- **Enregistrer la narration** (onglet **Son**) : raconter pendant la séance ; le son est découpé page par page et
  enregistré avec chaque note, la relecture de la note le rejoue (chapitre 9).

Le *bouton de la tablette* fait, pendant le direct, ce que l'option **Bouton de la tablette en mode direct** indique
(nouvelle page ou nouveau calque). Un nouveau calque est créé en haut et reçoit aussitôt l'écriture de la tablette ;
l'ordinateur reste sur son calque. *Ce que la Bamboo Slate envoie à l'appui n'est pas encore vérifié sur la vraie
tablette.* Les touches de la page suivante et précédente marchent dans tous les cas.

### Le panneau de côté de la fenêtre de commande

À droite de l'aperçu, une colonne d'icônes, le *rail*, range ce qui se règle autour de la page. Un clic sur une icône
ouvre son onglet à gauche du rail. Un clic sur l'onglet déjà ouvert, ou sur **›** en haut du panneau, le *replie* :
l'aperçu prend toute la largeur et le rail reste. Les onglets, de haut en bas :

- **Calques** : la zone **Calques** (voir **Corriger pendant le direct**). C'est l'onglet ouvert au départ.
- **Page** : le **Papier** de la page en cours (couleur, modèle, texture) ; voir **Le papier et l'image de la page en
  cours** plus bas.
- **Fond** : l'**Image ou PDF de fond** de la page en cours ; voir le même paragraphe.
- **Son** : **Enregistrer la narration** pendant la séance.
- **Projection** : le **Vidéoprojecteur** (l'écran de projection, **Projeter**, **Arrêter la projection**), la **Zone
  projetée** (zoom, **Page entière**, **Largeur**, interrupteurs **Tableau noir** et **Pointeur laser**) et le
  **Partage sur le réseau** : le bouton **Partager sur le réseau** n'est plus dans la barre d'outils.
- **Aide** : les raccourcis du mode direct (voir le chapitre 17).

Ardoise retient l'onglet ouvert et l'état replié de cette fenêtre, sur ce PC, et les retrouve au mode direct suivant.

**Les pastilles** se voient aussi panneau replié : le *nombre de calques* sur **Calques** ; un *point orange* quand le
calque de la tablette, ou le calque choisi, est masqué (l'avertissement de la zone **Calques** dit lequel) ; un *point
rouge* sur **Son** pendant un enregistrement ; un *point bleu* sur **Projection** tant qu'une projection ou un partage
est en cours.

**Au-dessus de l'onglet ouvert**, quel qu'il soit, s'affichent la barre de la sélection (« Traits sélectionnés : 3 »,
**Supprimer**, **Désélectionner**) et, tant qu'un outil **Sélection** ou **Stylo** est actif, son aide en une phrase.
Quand des traits sont choisis, le panneau se rouvre s'il était replié.

**Au clavier.** `↓` et `↑` passent d'une icône à l'autre, `Début` et `Fin` vont à la première et à la dernière,
sans rien ouvrir ; `Entrée` ouvre l'onglet ou le replie. `Espace` ne fait pas cela ici : en mode direct il veut toujours
dire « page suivante », même quand une icône a le focus. Il en va de même de `Pg suiv` et `Pg préc` quand une liste
(**Modèle**, **Placement**) ou un curseur (**Opacité**) des onglets **Page** et **Fond** a le focus : ils tournent
toujours la page, pour que la télécommande de la salle marche après un choix à la souris ; ces listes et ce curseur
rendent d'ailleurs le focus après chaque choix.

### Retoucher pendant le direct

*Demande la tablette.* Une faute à enlever, un mot à mettre en rouge ? Vous n'avez pas besoin de quitter le mode direct
ni d'ouvrir l'éditeur. Vous retouchez dans la fenêtre de commande, et **la tablette continue d'écrire pendant ce temps**.

**Choisir des traits**

- Cliquez sur **Sélection** (groupe **Ordinateur**) dans la barre d'outils, ou appuyez sur `V`. Un second appui quitte
  l'outil. **Sélection** et **Stylo** (voir **Corriger pendant le direct**) s'excluent : choisir l'un quitte l'autre.
- **Lasso** : dans l'aperçu, appuyez avec la souris et entourez les traits, puis relâchez. Un trait est choisi quand la
  moitié au moins de ses points est dans le lasso.
- **Clic** : cliquez sur un trait pour le choisir seul. Cliquez sur l'encre elle-même, pas à côté.
- `Maj` + lasso ou clic ajoute des traits à ceux déjà choisis. `Ctrl` + lasso ou clic en retire.
- Un geste dans le vide, ou `Échap`, enlève la sélection. `Échap` ne ferme rien d'autre tant que des traits sont choisis.
- Un doigt hors du cadre de la sélection et la molette continuent de déplacer et de zoomer l'aperçu.
- Les traits choisis sont entourés de bleu dans l'aperçu. La barre de la sélection, au-dessus de l'onglet ouvert du
  panneau de côté, dit « Traits sélectionnés : 3 » (voir **Le panneau de côté de la fenêtre de commande**).
- **Seuls les traits du calque choisi sont pris** (zone **Calques**, voir plus bas) ; choisir un autre calque enlève la
  sélection. On ne peut pas choisir le trait que la tablette est en train d'écrire, ni les traits d'un calque masqué ;
  masquer un calque enlève ses traits de la sélection.
  Sur une feuille reprise, on peut choisir les anciens traits de la feuille, en choisissant leur calque.

**Déplacer et redimensionner** : les traits choisis sont entourés d'un cadre en pointillés, avec un petit carré à chacun
de ses quatre coins.

- **Déplacer** : posez le stylet, le doigt ou la souris **dans le cadre** (ou tout près de son bord : le stylet et le
  doigt n'ont pas besoin de viser pile) et glissez. Les traits suivent, et se posent là où vous relâchez.
- **Redimensionner** : saisissez un **coin** : près du carré, un peu en dehors du cadre fonctionne aussi, surtout au stylet
  et au doigt. Glissez le long de la diagonale. Le coin opposé ne bouge pas ; l'épaisseur du trait
  suit la taille. On ne peut pas descendre sous un vingtième de la taille, ni dépasser vingt fois.
- Un simple appui dans le cadre, sans glisser, ne change rien : il ne refait pas de sélection. Avec `Maj` ou `Ctrl`,
  un geste est toujours un lasso, même sur le cadre. Hors du cadre, la souris et le stylet entourent comme avant et un doigt
  déplace l'aperçu.
- Pendant le geste, les traits se voient soulevés, à la place où vous les menez, et l'endroit qu'ils quittent est vide. `Échap`
  (ou un deuxième doigt, qui prend le geste pour un zoom) abandonne le geste : les traits restent où ils étaient et rien n'est
  envoyé.
- **Le vidéoprojecteur et l'autre écran du réseau ne bougent qu'au relâchement** : ils montrent le résultat, d'un coup, jamais
  le cadre ni les carrés. La sélection reste, pour déplacer encore ou redimensionner. `Ctrl + Z` remet les traits à leur
  place (un pas par déplacement ou par redimensionnement).
- Dans l'éditeur, le cadre et ses coins se prennent de la même façon.

**Supprimer** : cliquez sur **Supprimer** dans la barre de la sélection, ou appuyez sur `Suppr` ou `Retour arrière`.

**Changer la couleur** : cliquez sur une pastille de **Couleur**. Seuls les traits choisis changent, et la nouvelle
couleur se voit aussitôt à l'intérieur du contour bleu. Ils restent choisis, pour essayer une autre couleur. **La couleur du stylo ne change pas** : ce que la tablette écrit ensuite garde
sa couleur. Sans sélection, une pastille règle la couleur du stylo, comme avant.

**Annuler et rétablir**

- `Ctrl + Z` (ou **Annuler**) annule la dernière action de l'ordinateur : un trait ou un coup de gomme (voir
  **Corriger pendant le direct**), une suppression, un changement de couleur, un déplacement ou un redimensionnement, ou **Effacer la page**. Les traits
  supprimés reviennent à leur place.
- `Ctrl + Y` ou `Ctrl + Maj + Z` (ou **Rétablir**) refait ce qui vient d'être annulé. Une nouvelle retouche vide ce qui
  pouvait être rétabli.
- **Ce qui n'est jamais annulé** : ce que la tablette a écrit. Un trait écrit entre-temps reste, dans quelque ordre que
  vous annuliez ou rétablissiez. Les changements d'outil, de couleur du stylo ou d'épaisseur, les changements de
  page et les opérations sur les calques ne sont pas non plus des retouches.
- Chaque page a sa propre liste, qui garde les 100 dernières retouches. Si vous quittez une page puis y revenez,
  `Ctrl + Z` annule encore ses retouches. Les listes disparaissent quand vous terminez le mode direct.
- Les boutons **Annuler** et **Rétablir** sont grisés quand il n'y a rien à faire.

**Le vidéoprojecteur et l'autre écran du réseau ne voient que le résultat.** Jamais le contour bleu ni le lasso : un
trait supprimé disparaît, un trait recoloré change de couleur, un trait déplacé ou redimensionné apparaît à sa nouvelle place. Les annotations tracées sur la présentation ne sont pas
des traits de la page : on ne les choisit pas.

**Dans la note.** Quand vous terminez, la note contient ce qu'il y avait à l'écran : traits supprimés absents, couleurs
changées. Sur une feuille reprise, la retouche d'un ancien trait est enregistrée dans la note elle-même ;
« Revenir à l'original » (chapitre 8) rend toujours la première version.

### Corriger pendant le direct

*Demande la tablette.* En classe, un élève écrit sur la tablette, projetée en direct, et vous corrigez en même temps
avec le stylet de votre ordinateur (une Surface, par exemple), dans une autre couleur et **sur un calque à vous** : ce
que vous écrivez est enregistré dans la note, sans se mêler à l'écriture de l'élève. La tablette continue d'écrire
pendant ce temps, sans jamais être gênée.

**Deux calques, deux repères.** La zone **Calques**, dans l'onglet **Calques** du panneau de côté de la fenêtre de commande, liste les
calques de la page, le plus haut en premier.

- Le *cadre* entoure le **calque choisi** : c'est là que l'ordinateur agit (stylo, bout gomme du stylet, sélection).
  Un clic sur une ligne choisit son calque.
- L'*icône de tablette* marque le calque où **écrit la tablette**. Elle reste sur son calque tant que vous ne
  l'envoyez pas ailleurs.

Au début du mode direct, l'ordinateur est sur le même calque que la tablette : vous ajoutez votre calque quand vous en
avez besoin.

**L'outil Stylo.** Cliquez sur **Stylo** (groupe **Ordinateur**) ou appuyez sur `P` ; un second appui quitte l'outil.
Dans l'aperçu :

- le *stylet* et la *souris* écrivent sur le calque choisi ;
- le *bout gomme du stylet* efface des traits entiers du calque choisi, quel que soit l'outil (un petit cercle suit la
  gomme) ; les traits des autres calques ne sont jamais touchés ;
- le *doigt* ne fait jamais d'encre : il déplace et zoome la zone projetée, comme avant.

**L'encre de l'ordinateur.** Elle est séparée de celle de la tablette : **rouge** par défaut, en feutre. Le bouton
placé après **Stylo** (une pastille de couleur et le nom de l'outil) ouvre **Encre de l'ordinateur** : **Outil**,
**Couleur** et **Épaisseur**. Ces réglages ne touchent ni le style de la tablette (groupe **Tablette**) ni une
sélection ; Ardoise les retient d'une séance à l'autre, sur ce PC. `Échap` ou un clic ailleurs ferme le panneau.

**Un exemple en classe.** L'élève écrit son exercice sur la tablette ; vous voulez le corriger en rouge, sans rien
mélanger à son écriture.

1. Dans la zone **Calques**, cliquez sur **Ajouter un calque**. Le nouveau calque se met en haut et devient le calque
   choisi ; la tablette reste sur le calque de l'élève.
2. Cliquez sur le crayon à côté de son nom, tapez « Correction », puis `Entrée`.
3. Cliquez sur **Stylo** (ou `P`). L'encre est déjà rouge.
4. Écrivez sur l'aperçu avec le stylet de l'ordinateur *pendant que l'élève écrit*. Votre trait apparaît aussitôt au
   vidéoprojecteur et sur l'écran du partage, comme celui de la tablette.
5. Une erreur de votre part : passez le bout gomme du stylet dessus, ou appuyez sur `Ctrl + Z`.
6. Pour retoucher ce que l'élève a écrit, cliquez sur son calque dans la zone, puis utilisez la gomme du stylet ou
   **Sélection**.

**Ce que montrent le vidéoprojecteur et le partage.** Les deux calques, mêlés, comme à l'écran, sauf les calques
masqués, qu'ils ne montrent jamais. Ni le cadre, ni le lasso, ni le cercle de la gomme ne sont projetés.

**Annuler avec `Ctrl + Z`.** Il annule les actions de l'*ordinateur* : un trait, un coup de gomme, une retouche,
**Effacer la page** (voir **Retoucher pendant le direct**). Jamais l'écriture de la tablette. Les opérations sur les
calques (ajouter, masquer, renommer, déplacer, fusionner) ne s'annulent pas avec `Ctrl + Z` : pour défaire un masquage
ou un déplacement, refaites le même clic.

**Envoyer la tablette sur un autre calque.** Passez la souris sur la ligne d'un autre calque : une icône de tablette
pâle apparaît ; cliquez dessus. (Sur un écran tactile, ces icônes sont toujours visibles.) Ce que l'élève écrira
ensuite ira sur ce calque ; ce qu'il a déjà écrit ne bouge pas. Si le bouton de la tablette est réglé sur **Nouveau
calque** (**Bouton de la tablette en mode direct**, chapitre 5), un appui crée un calque en haut et y envoie la
tablette ; l'ordinateur reste où il est.

**Les autres gestes de la zone Calques**

- L'*œil* masque ou affiche un calque ; il est *barré* quand le calque est masqué, et le nom passe en gris italique.
- Le *crayon* renomme : `Entrée` garde le nouveau nom, `Échap` rend l'ancien.
- Sur la ligne choisie seulement, **↑** et **↓** montent et descendent le calque, et **⤓** le fusionne avec le calque
  du dessous. **Fusionner pendant le direct ne s'annule pas** : `Ctrl + Z` ne le défait pas, et la liste des
  annulations de la page est vidée (celles des autres pages ne sont pas touchées ; **Annuler l'effacement** disparaît
  aussi). Ardoise demande donc d'abord : « Fusionner « … » avec « … » ? Ctrl+Z ne pourra pas l'annuler. », avec
  **Fusionner** et **Annuler** (`Échap` répond **Annuler**). Si le calque du dessous est masqué, la question le dit
  aussi : une fois fusionné, son contenu sera projeté et partagé (le calque fusionné est affiché).
- **Ajouter un calque** en crée un nouveau, en haut.
- Les boutons de la zone sont grisés quand le mode direct n'est pas en cours.

**Les avertissements.** Ils apparaissent sous la liste.

- *« La tablette écrit sur « … », qui est masqué : ce que l'élève écrit n'est ni projeté ni partagé. »* L'écriture de
  l'élève est bien enregistrée dans la note, mais personne ne la voit : réaffichez le calque, ou envoyez la tablette
  sur un autre.
- *« Le calque choisi, « … », est masqué : affichez-le pour y écrire. »* Le stylo n'écrit pas tant que le calque choisi
  est masqué (le même message s'affiche si vous essayez), et la gomme et la sélection n'y trouvent rien.

**Dans la note et à la relecture.** Les calques, leurs noms et leur état (affiché ou masqué) sont enregistrés dans la
note, avec chaque trait de l'ordinateur à côté de ceux de l'élève. À la relecture (chapitre 9), les traits de
l'ordinateur apparaissent à l'heure où vous les avez écrits, mêlés à ceux de l'élève. Les annotations tracées sur la
présentation (voir plus bas) ne changent pas : elles ne sont jamais enregistrées.

### Le papier et l'image de la page en cours

*Demande la tablette. Vérifié avec la tablette simulée, pas encore avec la vraie (essai H7).* Pendant le direct, vous
pouvez changer le papier de la page qui est à l'écran et mettre une fiche ou un document *sous* l'écriture, sans arrêter
la tablette : un élève écrit, vous passez du blanc au Seyès, ou vous posez l'énoncé d'un exercice pour que la classe le
complète. Le changement apparaît aussitôt au vidéoprojecteur et sur l'écran du partage, et il est enregistré avec la page.
Les onglets **Page** et **Fond** sont grisés tant que le direct n'est pas lancé.

**Le papier** (onglet **Page**, bloc **Papier**) : les mêmes réglages que dans l'éditeur, la couleur en pastilles
(**Blanc**, **Crème**, **Kraft**, **Ardoise noire**), le **Modèle** (**Uni**, **Ligné**, **Seyès**, **Quadrillé 5 mm**,
**Millimétré**, **Pointillé**, **Portées**) et l'interrupteur **Texture du papier**.

- **Une nouvelle page garde le papier** (couleur, modèle, texture) de la page qui était à l'écran quand vous l'avez créée,
  mais *jamais son image*. Une page déjà créée garde le sien : changer le papier de la page 3 ne touche pas la page 2. Une
  nouvelle séance repart sur du blanc uni.
- **Le tableau noir** : tant qu'il est allumé (`B`), les pastilles de couleur sont grisées et une phrase le dit : la
  couleur choisie *reviendra quand le tableau noir sera éteint*. Le modèle et la texture, eux, s'appliquent tout de suite.
  Choisir **Ardoise noire** avec le tableau noir éteint donne un papier noir où l'écriture de la tablette reste noire : pour
  écrire à la craie, allumez plutôt le tableau noir.
- **On ne revient pas en arrière avec `Ctrl + Z`**, qui n'annule que ce que l'ordinateur écrit : rechoisissez l'ancien
  réglage.
- Une page qui n'a ni écriture ni image n'est pas enregistrée, même si vous avez changé son papier.

**L'image** (onglet **Fond**, bloc **Image ou PDF de fond**) : **Choisir un PDF ou une image…**, la page du PDF s'il y en
a plusieurs (une vignette par page, comme dans l'éditeur : voir plus bas), puis **Mettre en fond**. L'image (PNG ou JPEG) ou la page de PDF se place *sous* l'écriture de la page en
cours, **Entière** (toute visible, sans déformation) ; **Placement** (**Entière**, **Pleine page**, **Étirée**),
**Opacité**, **Tourner l'image seule**, **Remplacer…** et **Retirer l'image** la règlent ensuite, comme dans l'éditeur
(chapitre 8). L'import d'une page de PDF peut prendre quelques secondes : la tablette continue d'écrire pendant ce temps.

- **Une nouvelle page n'a pas d'image.** Revenir à la page d'avant retrouve la sienne : la fiche peut être sur la page 2 et
  une photo sur la page 3.
- **L'image est enregistrée avec la page**, comme image de fond de la note. Une page qui n'a que l'image, sans rien
  d'écrit, devient quand même une note, pour qu'une fiche montrée en classe soit gardée. Si vous retirez ensuite l'image,
  la note reste dans la bibliothèque, vide : mettez-la à la corbeille si elle ne sert plus.
- **Ardoise ne propose pas de passer la feuille en portrait ou en paysage** (une page du direct suit la tablette) : la
  fiche est placée telle qu'elle est, et une fiche en portrait sur une page en paysage est rétrécie. Tenez la tablette en
  portrait *avant* d'écrire pour qu'elle remplisse la page : tant que la page n'a pas d'écriture, elle suit la tablette et
  l'image est replacée avec le même placement.
- **Une page qui a une image et de l'écriture ne tourne plus avec la tablette**, comme une feuille reprise : les boutons de
  position sont grisés, Ardoise dit comment tenir la tablette, et la page suivante suit de nouveau la position réglée. Si
  vous retirez l'image, la page se remet à suivre la tablette : elle se tourne aussitôt, écriture comprise.
- Sur une **feuille reprise** (voir plus haut), changer le papier ou l'image est un changement voulu : il est enregistré
  dans la note même sans écriture nouvelle. **Revenir à l'original** (chapitre 8) rend l'écriture d'avant, pas le papier ni
  l'image.
- **Sur l'écran du partage**, le papier et l'image sont envoyés une fois sous forme d'image ; l'autre ordinateur montre la
  couleur du papier en attendant de l'avoir reçue.

### Projeter sur un vidéoprojecteur

1. Branchez le projecteur et, dans Windows, appuyez sur `Win + P` puis choisissez *Étendre* : chaque écran a son bureau.
2. Dans l'onglet **Projection** du panneau de côté de la fenêtre de commande, bloc **Vidéoprojecteur**, choisissez l'**Écran de projection** dans la liste (le
   bouton **Actualiser la liste des écrans** la remet à jour) et cliquez sur **Projeter**.
3. Une fenêtre de *présentation* s'ouvre *en plein écran sur l'écran choisi*, sans aucun élément d'interface (le
   curseur de la souris disparaît après 2 secondes d'immobilité). La fenêtre de commande reste sur l'écran de
   l'ordinateur. Ce que vous écrivez s'affiche sur les deux.
4. **Arrêter la projection** ferme la présentation. Ardoise retient l'écran choisi sur ce PC.

Avec un seul écran, la présentation recouvre la fenêtre de commande : appuyez sur `Échap` dans la présentation pour la
fermer. Il n'y a jamais deux fenêtres projetées à la fois : projeter le direct ferme la projection d'une note de
l'éditeur (chapitre 8), et inversement.

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
envoie Page suivante et Page précédente) : voir le chapitre 17, ou l'onglet **Aide** du panneau de côté.

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

**Côté Ardoise.** Ouvrez l'onglet **Projection** du panneau de côté, dans la fenêtre de commande du mode direct ou dans
l'éditeur, et cliquez sur **Partager sur le réseau** (bloc **Partage sur le réseau**). Le panneau qui s'ouvre donne :

- l'*adresse* à taper dans le navigateur de l'autre ordinateur, de la forme `http://192.168.1.23:8765` (si le PC en a
  plusieurs, elles sont toutes listées) ;
- un *code QR* de cette adresse, à scanner avec un téléphone ou une tablette ;
- le *code d'accès* à 6 caractères, en deux groupes de trois (par exemple `K7M Q2X`) ;
- le *nombre d'écrans connectés*, et ce qui est montré ;
- **Pause** (l'image se fige sur les écrans ; vous continuez d'écrire, puis **Reprendre** montre l'état actuel) et
  **Arrêter le partage**.

Tant qu'un partage est en cours, un indicateur avec le nombre d'écrans reste en haut de la bibliothèque, le bouton **Partager
sur le réseau** de l'onglet **Projection** le montre aussi (un clic rouvre le panneau) et l'icône **Projection** du rail porte
un point bleu, même panneau replié.

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
**Date : la plus récente d'abord**, **Date : la plus ancienne d'abord**, **Titre : A → Z**, **Titre : Z → A**, et, quand
un classeur est affiché, **Ordre du classeur**, voir plus bas), les
boutons **Nouvelle note**, **Depuis un PDF ou une image…** et **Réglages**. À gauche : la barre latérale avec
**Toutes les notes**, **Favoris**, les **Classeurs**, les **Étiquettes**, la **Corbeille**, l'entrée
**Importer depuis Inkspace…** (chapitre 11), et en bas le bloc **Tablette** et le bouton **Tester la tablette**.

Une carte montre la miniature, le titre (ou la date si la note n'a pas de titre), la date, le classeur et les
étiquettes. Un clic ouvre la note dans l'éditeur. Sur la carte : la case pour *sélectionner*, l'étoile des favoris, le menu
**Actions pour…** (**Renommer**, favoris, **Dupliquer**, **Changer la date…**, **Mettre à la corbeille**) et une poignée,
qui apparaît sur la carte, pour glisser la note hors d'Ardoise (chapitre 10). Le **clic droit** sur une carte ouvre ce
même menu. Renommer une note renomme aussi son fichier.

**Le clic droit** n'affiche jamais le menu d'une page web (« Enregistrer l'image sous… », « Outils supplémentaires »…) :
Ardoise montre son propre menu, ou rien. Le menu s'ouvre **sous le pointeur** (la touche `Menu`, ou `Maj + F10`, sur une carte ou
un classeur l'ouvre sous son bouton **⋯**). Il existe sur : une carte et un classeur (ci-dessus) ; **Toutes les notes** et
**Favoris** (**Afficher**, **Tout sélectionner**) ; une étiquette (**Afficher**, **Couleur…**, qui ouvre la palette de la
couleur) ; la **Corbeille** (**Ouvrir la corbeille**, **Vider la corbeille…**, avec la même question de confirmation que le
bouton) ; une note de la corbeille (**Restaurer**, **Supprimer définitivement…**, qui demande confirmation) ; un calque et la
page de l'éditeur (chapitre 8). Les flèches, `Début`, `Fin` et `Entrée` conduisent le menu ; `Échap`, un clic ailleurs, le
défilement ou un changement de fenêtre le ferment. Dans un champ de texte, le clic droit garde le menu de Windows (couper,
copier, coller). Ailleurs, il ne se passe rien.

Les miniatures sont gardées dans un cache propre à chaque PC. Quand une note change sur un autre ordinateur
(synchronisation Nextcloud), la bibliothèque se met à jour toute seule.

Des messages peuvent s'afficher au-dessus des notes : **Fichiers illisibles : N** (voir plus bas) et, après un arrêt
brutal d'Ardoise, la liste des modifications non enregistrées que l'on peut récupérer (chapitre 8).

### Classeurs

Un classeur est un simple *sous-dossier* de la bibliothèque. Cliquez sur l'icône **Nouveau classeur** (ou sur
**Nouveau sous-classeur dans…** à côté d'un classeur), tapez le **Nom du classeur** et cliquez sur **Créer**. Pour
ranger des notes, glissez-les sur un classeur de la barre latérale, ou sélectionnez-les puis **Déplacer vers…**
(**Bibliothèque (racine)** ramène à la racine).

Sous la date, chaque carte indique le classeur de la note, avec son chemin complet s'il est dans un autre classeur
(par exemple `Classe/3e B`). Une note qui n'est dans aucun classeur, donc à la racine de la bibliothèque, porte la
mention **Non classée** : vous voyez d'un coup d'œil ce qu'il reste à ranger. Une note étant un fichier, elle est dans un
seul classeur à la fois.

Pour renommer, exporter, imprimer ou supprimer un classeur, ouvrez le menu **⋯** (« Actions sur le classeur… ») à côté de son
nom dans la barre latérale, ou faites un **clic droit** sur son nom :

- **Renommer…** : tapez le nouveau nom puis **Renommer** (**Annuler** ou `Échap` abandonne). Le dossier est renommé et
  ses notes le suivent. Ardoise ne remplace jamais un classeur ou un fichier du même nom : elle prévient et vous
  laisse en choisir un autre.
- **Exporter le classeur en PDF…** et **Imprimer le classeur…** : voir le chapitre 10.
- **Supprimer…** : Ardoise demande confirmation en disant combien de notes (et de sous-classeurs) le classeur contient.
  **Supprimer le classeur** envoie ses notes à la **Corbeille**, puis supprime le dossier. Rien n'est détruit : chaque
  note restaurée depuis la corbeille retrouve son classeur. Un dossier qui contient encore des fichiers qui ne sont pas
  des notes est gardé, et le message le dit. Un message **Annuler** reste affiché quelques secondes (voir « La
  corbeille » plus bas) : il rétablit le classeur avec ses notes.

### Ranger les notes d'un classeur dans l'ordre voulu

Quand un classeur est affiché, le tri **Ordre du classeur** range ses notes comme *vous* le décidez, au lieu de la date. Dans
les autres vues (**Toutes les notes**, **Favoris**, une étiquette), ce choix n'est pas proposé et les notes sont triées par date,
la plus récente d'abord ; votre choix revient dès que vous rouvrez un classeur.

- **Au glisser-déposer** : faites glisser une carte (ou toute la sélection, si la carte en fait partie) *entre deux cartes* : une
  barre verticale montre où elle va se poser (sur la moitié gauche d'une carte, avant elle ; sur la moitié droite, après). Les
  notes sélectionnées se déplacent ensemble en gardant leur ordre. Déposée sur un classeur de la barre latérale, la note change
  de classeur, quel que soit le tri.
- **Au clavier** : `Alt + flèche gauche` ou `Alt + flèche droite` déplace la carte qui a le focus (ou la sélection) d'une place ;
  `Alt + Début` ou `Alt + Fin` la met au début ou à la fin. Le lecteur d'écran annonce la nouvelle place (« Beta déplacée en
  position 1 sur 3 »).
- **Le classeur parent** montre ses propres notes d'abord, puis celles de chaque sous-classeur, dans l'ordre de ce
  sous-classeur (les sous-classeurs dans l'ordre de la barre latérale). Une carte ne se déplace que parmi les notes de son
  classeur : on range un sous-classeur en l'ouvrant ou en glissant sa note à côté d'une note du même sous-classeur.
- **Une note qui arrive** (tablette, nouvelle note, note déplacée dans le classeur, note restaurée) se place à la fin ; une
  copie (voir plus bas) se place juste après son original. Pendant une **recherche**, les notes gardent l'ordre du classeur
  mais ne se déplacent pas : videz la recherche pour les ranger.
- **L'ordre est enregistré dans le classeur lui-même**, dans le petit fichier `_ardoise-order.json` (à côté des notes), et suit donc la
  synchronisation. Chaque classeur a son ordre. Les boutons **Page précédente** et **Page suivante** de l'éditeur, et l'export
  de plusieurs notes, suivent l'ordre affiché. Si le fichier est absent ou abîmé, les notes sont simplement triées par date, sans
  rien perdre. Si deux PC rangent le même classeur en même temps, Nextcloud garde une « copie en conflit » du fichier : c'est le
  rangement le plus récent qui est retenu, et aucune note n'est perdue (une note que ce rangement ne cite pas passe à la fin).

### Le sommaire d'un classeur

Pour retrouver un passage dans tout un classeur, posez des **signets** dans vos notes (voir **Poser des signets**, au chapitre 8) :
un mot ou une courte phrase à l'endroit voulu de la page (« Chapitre 3 », « Corrigé de l'exercice 2 »). Quand un classeur est
affiché, le bouton **Sommaire** de la barre du haut (ou **Sommaire du classeur…**, dans le menu **⋯** du classeur) ouvre la liste de
ses signets, **note par note, dans l'ordre du classeur** (**Ordre du classeur**, ci-dessus : celui que vous avez rangé, sinon par
date, la plus ancienne d'abord), les notes de ses sous-classeurs après les siennes. Seules les notes qui ont des signets y figurent.
La case **Afficher les dates** (décochée au départ) décide du nom d'une note sans titre : sa date, ou **Note sans titre** (suivi
d'un numéro s'il y en a plusieurs : « Note sans titre (2) »). Décochée, le sommaire se lit comme celui d'un livre, sans horodatage ;
ce choix est gardé, et c'est le même pour les signets du PDF du classeur (voir **Exporter ou imprimer un classeur**).
Un clic sur un signet (ou `Entrée`, après avoir parcouru la liste avec `Tab` ou les flèches) ouvre la note **à cet endroit** : le
signet est sélectionné et au milieu de l'éditeur. Le champ **Filtrer les signets** ne garde que ceux dont le texte (ou le titre de la
note) contient les mots tapés. Ce sommaire n'est pas affiché quand on revient de la note ; rouvrez-le avec le même bouton.

### Dupliquer une note, changer sa date

- **Dupliquer** (menu de la carte, clic droit, ou barre de la sélection pour une ou plusieurs notes) crée une copie dans le
  *même classeur*, avec la *même date de création*, le titre suivi de **(copie)** (« Cours (copie) », puis « Cours (copie 2) »
  pour la copie d'une copie ; une note sans titre donne « (copie) »), sans le favori. Le tracé, les calques, le papier, l'image de
  fond, les étiquettes, la **narration** et l'**original de la tablette** sont copiés : **Revenir à l'original** marche sur la copie.
  Une copie d'une *copie retouchée par un assistant* reste une copie retouchée. Un message **Annuler** (ou `Ctrl + Z`) met les
  copies à la corbeille.
- **Changer la date…** (menu de la carte, clic droit) ouvre une petite fenêtre : **Aujourd'hui** remplit le champ avec la date et
  l'heure de maintenant, ou tapez une **Nouvelle date et heure**, puis **Changer la date**. C'est la date que montre la note, selon
  laquelle la bibliothèque la trie, et qui commence le nom du fichier ; l'écriture et le son ne changent pas. **Annuler** remet
  l'ancienne date.
- *Avec la tablette* : une copie n'est pas « la note d'une page » de la tablette, et changer la date d'une note importée ne
  la fait pas revenir à la synchronisation suivante (Ardoise la reconnaît à son identifiant). Un cas particulier : un écran de
  la tablette Xiaomi enregistré de nouveau met à jour sa note ; si vous avez daté cette note *après* l'enregistrement précédent,
  une nouvelle note est ajoutée au lieu de la mettre à jour (rien n'est perdu ni écrasé).

### Étiquettes et favoris

Sélectionnez une ou plusieurs notes, cliquez sur **Étiquettes…**, tapez une étiquette (plusieurs, séparées par une
virgule) et cliquez sur **Ajouter** ; une étiquette existante se retire avec sa croix. Un clic sur une étiquette de la
barre latérale, ou sur celle d'une carte, n'affiche que les notes qui la portent. L'étoile d'une carte (ou
**Mettre en favoris**) ajoute aux **Favoris**. Vous pouvez aussi faire glisser une carte (ou toute la sélection) sur
**Favoris** ou sur une étiquette de la barre latérale : les notes deviennent des favoris, ou reçoivent cette étiquette,
sans changer de classeur.

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
**Imprimer…**, **Assembler…**, **Dupliquer**, **Tourner**, **Mettre en favoris** (ou **Retirer des favoris**),
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
  par exemple `1-3, 5`), à la taille et dans l'orientation de la page, avec la page en fond pour écrire par-dessus. Pour un
  PDF de plusieurs pages, la fenêtre montre **une miniature de chaque page**, dessinée au fur et à mesure qu'on la fait défiler
  (même un PDF de plusieurs centaines de pages s'ouvre vite) : un clic choisit ou retire une page, **Maj + clic** choisit toute
  une suite de pages depuis la dernière cliquée, **Toutes** et **Aucune** les prennent ou les retirent toutes ; le champ des
  numéros reste là et suit les miniatures (et inversement). Au clavier : les flèches déplacent le curseur sur les pages,
  `Espace` choisit la page, `Maj + flèche` étend la suite, `Ctrl + A` les choisit toutes. Une
  image donne une note de la taille d'une feuille A4, dans l'orientation de l'image. Les PDF protégés par un mot de
  passe ne sont pas lus : enregistrez-en une copie sans mot de passe. Pour un long PDF, une barre indique
  l'avancement (« Création des notes : 3 sur 12… ») et **Annuler** arrête la création : aucune note n'est alors gardée
  (« Création annulée : aucune note n'a été gardée »).

### Chercher une note

Tapez dans **Rechercher une note** : Ardoise cherche dans les *titres*, les *étiquettes*, les *signets*, les *textes tapés* (outil
**Texte**, et le code des équations, sur les calques affichés) et l'*écriture manuscrite*, sans tenir compte des accents ni des majuscules. Les notes trouvées grâce à l'écriture sont regroupées
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

**La touche `Échap`, par étapes.** Chaque appui fait la plus petite chose qui s'applique : d'abord il abandonne le geste en
cours (un trait, une forme, un lasso, un déplacement, la question qu'Ardoise pose à l'écran) ou termine la saisie d'un texte, d'un signet ou
d'une équation en gardant ce qui est écrit (une équation au code incorrect reste dans son panneau) ; sinon il ferme le panneau ou
l'outil ouvert (la découpe, la relecture, le recadrage à la main, le réglage de **Redresser les formes**) ; sinon il enlève
la sélection ; sinon il **enregistre la note et revient à la bibliothèque**, comme le bouton **Bibliothèque** mais sans poser
la question des modifications : l'enregistrement est la réponse. Si l'enregistrement échoue (note modifiée ailleurs entre-temps,
disque plein), la note reste ouverte et le message dit pourquoi. Pendant l'enregistrement d'une narration, `Échap` ne
quitte jamais la note : un message demande d'arrêter l'enregistrement d'abord. Dans un champ de saisie (le titre, le nom d'un
calque, une liste de choix) et dans une fenêtre ouverte (export, impression…), `Échap` reste celui du champ ou de la fenêtre ;
une touche maintenue ne compte que pour un appui.

Pour passer à une autre note sans revenir à la bibliothèque, utilisez **Page précédente** et **Page suivante** : les
flèches `‹` et `›` à côté du titre, ou `Alt + ←` et `Alt + →`. Elles suivent la liste que la bibliothèque montre : même
classeur, mêmes étiquettes, même recherche, même ordre. Entre les deux flèches, Ardoise écrit la place de la note, par
exemple **3 / 12**. Les flèches sont grisées à la première et à la dernière note, et cachées quand la note n'est pas
dans la liste ou qu'elle y est seule. Avec des modifications non enregistrées, Ardoise demande : **Enregistrer et
ouvrir l'autre note**, **Ouvrir sans enregistrer** ou **Rester**. Pendant l'enregistrement d'une narration ou un export
vidéo, les flèches ne font rien : arrêtez-les d'abord. En revenant à la bibliothèque, vous retrouvez la dernière note
ouverte. Si la note est montrée sur un autre écran (partage sur le réseau), cet écran suit.

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

### Le panneau de côté de l'éditeur

À droite de la page, une colonne d'icônes, le *rail*, range tout ce qui se règle autour de la page. Un clic sur une icône
ouvre son onglet à gauche du rail. Un clic sur l'onglet déjà ouvert, ou sur **›** en haut du panneau, le *replie* : la page
prend toute la largeur et le rail reste. Les onglets, de haut en bas :

- **Calques** : la zone **Calques** (voir plus bas). C'est l'onglet ouvert au départ.
- **Page** : le **Papier** (couleur en pastilles, modèle, texture), le **Format de la feuille** et son orientation,
  **Tourner et recadrer**, et **Revenir à l'original** (voir plus bas).
- **Fond** : l'**Image ou PDF de fond**.
- **Son** : la narration. Le contrôle **Enregistrer le son** choisit entre **Pendant que j'écris** (au départ) et
  **Pendant une relecture** ; voir le chapitre 9. Pendant une relecture, c'est là qu'on enregistre en relisant ; en dehors
  d'une relecture, dans le mode **Pendant une relecture**, l'onglet propose **Lancer la relecture**.
- **Projection** : le **Vidéoprojecteur** (l'écran de projection, **Projeter**, **Arrêter la projection**), la **Zone
  projetée** (zoom, **Page entière**, **Largeur**) et **Partager sur le réseau** (le bouton n'est plus dans la barre
  d'outils ; voir la fin du chapitre 6). Voir **Projeter la note au vidéoprojecteur** plus bas.
- **Aide** : les raccourcis clavier de l'éditeur (voir le chapitre 17).

Ardoise retient l'onglet ouvert et l'état replié de l'éditeur, sur ce PC, et les retrouve quand vous ouvrez une autre
note ou rouvrez l'éditeur. Les modifications de l'onglet **Page** s'annulent avec `Ctrl + Z`, comme avant.

**Les pastilles** se voient aussi panneau replié : le *nombre de calques* sur **Calques** ; un *point orange* quand le
calque choisi est masqué ; un *point rouge* sur **Son** pendant un enregistrement ; un *point bleu* sur **Projection**
pendant une projection ou un partage.

**Au-dessus de l'onglet ouvert**, quel qu'il soit, s'affichent la découpe (quand elle est ouverte) et la sélection :
« Traits sélectionnés : 3 », **Copier**, **Couper**, **Coller**, **Dupliquer**, **Redresser les formes**,
**Supprimer**, et la croix **Désélectionner**. Dès que des traits sont choisis, ou qu'une découpe commence, le panneau se
rouvre s'il était replié.

**La relecture** ouvre l'onglet **Son** pour qu'on puisse enregistrer la narration, même panneau replié ; en la quittant,
Ardoise remet l'onglet et l'état d'avant.

**Au clavier.** `↓` et `↑` passent d'une icône à l'autre, `Début` et `Fin` vont à la première et à la dernière, sans
rien ouvrir ; `Entrée` ou `Espace` ouvre l'onglet ou le replie.

### Les outils

La barre d'outils propose les outils suivants (**Outil**) :

- **Stylo** : choisissez l'**Encre** (**Stylo à bille**, **Stylo plume**, **Feutre**, **Crayon**, **Pinceau**,
  **Surligneur**, **Craie**), la **Couleur** (**Noir**, **Bleu**, **Vert**, **Violet**, **Rouge**, **Orange**, **Gris**,
  ou **Choisir une autre couleur** ; les couleurs personnalisées sont mémorisées) et l'**Épaisseur**. L'épaisseur se règle de
  trois façons : la glissière pour aller vite, les boutons **−** et **+** pour avancer de 0,05 mm, ou la valeur tapée en
  millimètres dans le champ (de 0,10 à 10 mm, avec une virgule ou un point). L'épaisseur du trait
  suit la pression du stylet. **Appliquer à toute la note :** **Couleur**, **Encre** ou **Épaisseur** change tous les
  traits d'un coup.
- **Ligne**, **Rectangle**, **Ellipse** : des formes nettes, tracées d'un glissé (voir **Tracer une ligne, un rectangle,
  une ellipse**, plus bas).
- **Texte** : un cadre où l'on tape au clavier (voir **Taper du texte**, plus bas).
- **Équation** : une formule ou une équation de chimie écrite en LaTeX (voir **Écrire une équation**, plus bas).
- **Signet** : un mot ou une courte phrase posés à un endroit de la page, pour retrouver un passage (voir **Poser des signets**,
  plus bas).
- **Gomme** : **Traits entiers** ou **Partielle**, avec sa **Taille**. Elle n'efface que sur le calque choisi (voir
  **Calques**).
- **Sélection** (le lasso) : entourez des traits et des textes du calque choisi pour les sélectionner (un texte est pris quand
  son milieu est dans le lasso ; un clic sur un texte le prend seul) ; `Maj` + lasso ajoute, `Ctrl` + lasso retire. Le
  bloc **Sélection**, au-dessus de l'onglet ouvert du panneau de côté, propose **Copier**, **Couper**, **Coller**,
  **Dupliquer**, **Redresser les formes**, **Supprimer**.
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

### Tracer une ligne, un rectangle, une ellipse

Les outils **Ligne**, **Rectangle** et **Ellipse**, à côté du **Stylo**, tracent des formes nettes : appuyez (souris, stylet,
ou doigt si **Dessiner au doigt** est activé), glissez, levez. La forme se voit pendant le glissé ; un simple clic ne trace
rien ; `Échap` pendant le glissé l'abandonne. La forme prend l'**Encre**, la **Couleur** et l'**Épaisseur** choisies (à
l'épaisseur indiquée, quelle que soit la pression) et va sur le calque choisi.

**Rien n'est redressé sans que vous le demandiez** : une ligne va exactement du premier point au dernier, un rectangle ou une
ellipse est inscrit dans la boîte que vous tracez. Trois boutons à bascule, sous la barre d'outils tant qu'un de ces outils est
choisi, s'utilisent au doigt, au stylet ou à la souris, sans clavier :

- **Forme régulière** : un carré, un cercle ; une ligne se cale sur l'horizontale, la verticale ou la diagonale à 45° la plus
  proche. La touche `Maj`, tenue pendant un tracé, inverse ce bouton pour ce tracé.
- **Depuis le centre** : le premier point est le centre de la forme (une ligne grandit des deux côtés). La touche `Alt`, tenue
  pendant un tracé, inverse ce bouton pour ce tracé.
- **Aimanter au quadrillage** : les points que vous posez (le départ et l'arrivée ; pour un rectangle ou une ellipse, les coins de
  leur boîte) se calent sur le fond de la page : les intersections des fonds **Quadrillé 5 mm**, **Millimétré** et **Pointillé** ;
  les lignes du fond **Ligné** (l'horizontale seulement, l'autre sens reste libre) ; les lignes et les grands carreaux du **Seyès**
  (pas les traits fins). Le bouton est grisé, avec la raison, sur un fond sans lignes (uni, partition). Si la forme doit rester
  régulière et que les deux sens n'ont pas le même pas (fond ligné), la forme régulière l'emporte.

Les trois réglages sont gardés tant qu'Ardoise est ouvert, d'une note à l'autre, et reviennent éteints au prochain lancement.
Une forme est un trait ordinaire : un rectangle ou une ellipse est un seul trait fermé (la gomme partielle peut l'ouvrir) ; la
gomme, le lasso, **Annuler**, les calques, la relecture et tous les exports la traitent comme n'importe quel trait. Ces outils ne
sont pas dans le mode direct avec une tablette.

### Taper du texte

L'outil **Texte** (la touche `T`) pose sur la page un cadre où l'on tape au clavier : touchez ou cliquez la page à l'endroit
voulu (au stylet, à la souris ou au doigt ; sur un écran tactile, Windows propose son clavier à l'écran), puis tapez. Le texte
se dessine au fur et à mesure, tel qu'il sera exporté. `Entrée` commence une nouvelle ligne ; une ligne ne passe jamais toute
seule à la suivante : le cadre s'élargit avec ce que vous tapez. `Échap` (ou un clic ailleurs, ou un autre outil) termine la
saisie : le texte reste, et le cadre est sélectionné. Un cadre laissé vide disparaît. Toute la saisie s'annule d'un seul
`Ctrl + Z`. Pendant la saisie, les touches tapées vont au texte, jamais aux raccourcis de l'éditeur.

Pour modifier un texte, touchez-le avec l'outil **Texte**, ou double-cliquez dessus avec l'outil **Sélection** : le curseur se
place à l'endroit touché.

Sous la barre d'outils, avec l'outil **Texte** ou quand des textes sont sélectionnés :

- **Police** : **Latin Modern** (la police de LaTeX) ou **Andika** (une police dessinée pour l'apprentissage de la lecture).
  Les deux sont fournies avec Ardoise : le texte est le même sur tous les ordinateurs, dans tous les exports. Les lettres
  grecques qui manquent à Latin Modern sont prises dans Andika (pas toutes : α, β, γ, δ, θ, λ, μ, π, ρ, σ, φ, χ, ω, Δ, Ω, entre
  autres).
- **Taille** : **Petit** (4 mm), **Moyen** (6 mm) ou **Grand** (9 mm).
- **Gras**.
- La couleur est celle de l'outil : choisissez-la dans les couleurs de la barre.

Ces réglages s'appliquent à tout le cadre (pas de mise en forme à l'intérieur d'un cadre) : au cadre que vous tapez, sinon aux
textes sélectionnés, et aux prochains cadres. Un clic sur ces boutons ne fait pas quitter la saisie.

Un texte vit avec les traits : il se sélectionne au lasso, se déplace, se redimensionne (il grandit ou rapetisse avec les
traits ; choisir une taille lui rend cette taille), se duplique, se copie et se colle, se supprime, tourne avec la page, va sur
le calque choisi et suit son calque. La gomme ne l'efface pas : sélectionnez-le et supprimez-le. Il apparaît d'un coup dans la
relecture et dans les vidéos, au moment où il a été posé, et figure dans tous les exports (images, SVG, PDF, presse-papiers,
impression, projection, partage sur le réseau ; dans le PDF, il peut être cherché et copié). La recherche de la bibliothèque le
trouve. L'outil **Texte** n'est pas dans le mode direct, mais une feuille reprise en direct garde ses textes.

### Écrire une équation

L'outil **Équation** (la touche `M`) pose sur la page une formule écrite en LaTeX, en mode mathématique, comme dans un document
LaTeX entre `$…$` : touchez ou cliquez la page à l'endroit voulu (au stylet, à la souris ou au doigt). Un panneau s'ouvre à côté :
tapez le code dans **Code LaTeX** ; l'**aperçu** montre au fur et à mesure ce qui sera posé. **Poser** (ou `Entrée`, ou `Échap`
comme pour un texte) met l'équation sur la page, à l'endroit touché, et la sélectionne ; `Maj + Entrée` commence une nouvelle
ligne dans le code. Avec un champ vide, `Échap` ferme le panneau sans rien poser ; avec un code incorrect, le panneau reste
ouvert, le code et son erreur dedans, pour que vous le corrigiez : rien n'est perdu. **Annuler** (ou un autre outil) ferme le
panneau sans rien changer. Tant que l'équation n'est pas posée, la page ne change pas ; une fois posée, un seul `Ctrl + Z` la
retire. Pendant que vous tapez, les touches vont au code, jamais aux raccourcis
de l'éditeur. La première fois, l'outil met une seconde à se charger.

Si le code n'est pas correct, l'aperçu le dit avec le message de MathJax (en anglais, par exemple « Missing close brace » pour
une accolade oubliée) et l'équation ne peut pas être posée : rien n'est corrigé ni deviné à votre place. Ardoise refuse aussi un
caractère qu'il ne sait pas dessiner (des écritures autres que la latine et la grecque) et les numéros d'équation (`\tag`).

Quelques exemples :

| Vous tapez | Pour |
|---|---|
| `v = \dfrac{d}{t}` | une vitesse moyenne (`\dfrac` : une fraction en grand, `\frac` : en petit) |
| `E_c = \dfrac{1}{2} m v^2` | une énergie cinétique (`_` : indice, `^` : exposant ; `^{-3}` pour plusieurs caractères) |
| `\vec{F} = m\,\vec{a}` | des vecteurs (`\,` : une petite espace ; `\overrightarrow{AB}` pour une flèche longue) |
| `\rho = \dfrac{m}{V} = 1{,}0\ \text{g}\cdot\text{cm}^{-3}` | une masse volumique avec son unité (`{,}` : la virgule décimale ; `\text{…}` : des mots droits, accents compris) |
| `\Delta t`, `\alpha`, `\lambda`, `\Omega` | des lettres grecques |
| `\ce{2H2 + O2 -> 2H2O}` | une équation de réaction (`\ce{…}` : la chimie, les nombres en indice tout seuls) |
| `\ce{Cu^2+ + 2e- <=> Cu}` | des ions, des électrons, un équilibre |
| `\ce{CaCO3 -> CaO + CO2 ^}` | un gaz qui se dégage (`^`), un précipité (`v`) |
| `\pu{9,81 m/s^2}` | une valeur avec son unité, bien espacée |
| `20\,\celsius` ou `5\,\ohm` | des degrés Celsius, des ohms |
| `\begin{cases} x = 1 \\ y = 2 \end{cases}` | un système (`\\` : ligne suivante) |

Pour modifier une équation, touchez-la avec l'outil **Équation**, ou double-cliquez dessus avec l'outil **Sélection** : son code
revient dans le panneau ; **Mettre à jour** (ou `Entrée`, ou `Échap`) la redessine à la même place (le coin en haut à gauche
ne bouge pas). Tant que son panneau est ouvert, l'équation reste sélectionnée : les couleurs et les tailles s'y appliquent.

Sous la barre d'outils, avec l'outil **Équation** ou quand des équations sont sélectionnées : la **Taille**, **Petit** (4 mm),
**Moyen** (6 mm) ou **Grand** (9 mm), la hauteur d'une lettre comme pour un texte de cette taille ; elle s'applique aux
équations sélectionnées et aux prochaines.

Les pastilles de **Couleur** de la barre d'outils (et **Choisir une autre couleur**) colorent toute l'équation : celle que vous
écrivez, dont l'aperçu prend aussitôt la couleur sans que le curseur quitte le code, celle que vous modifiez, et celles que vous
sélectionnez, seules ou avec des traits et des textes (un `Ctrl + Z` par changement de couleur).

Une équation vit avec les traits, comme un texte : elle se sélectionne, se déplace, se redimensionne, se duplique, se copie et se
colle, se supprime, tourne avec la page et suit son calque ; la gomme ne l'efface pas. Elle est dessinée par Ardoise elle-même
dans tous les exports (images, SVG, PDF vectoriel, vidéos, presse-papiers, impression, projection, partage sur le réseau) et
apparaît d'un coup dans la relecture, au moment où elle a été posée. La note garde son code : la recherche de la bibliothèque le
trouve (« H2O » trouve `\ce{2H2 + O2 -> 2H2O}`), l'assistant le lit. Ce n'est pas un LaTeX complet : seulement le mode
mathématique, la chimie de `\ce` et les commandes courantes (`amsmath`, `\cancel`, `\boxed`, `\celsius`…). L'outil **Équation**
n'est pas dans le mode direct, mais une feuille reprise en direct garde ses équations.

### Poser des signets

L'outil **Signet** (la touche `B`) pose sur la page un petit repère portant un mot ou une courte phrase : touchez ou cliquez la page
à l'endroit voulu (au stylet, à la souris ou au doigt), puis tapez le texte dans le champ qui s'ouvre sous le signet. Le signet se
dessine au fur et à mesure, avec un petit drapeau. `Entrée` ou `Échap` (ou un clic ailleurs, ou un autre outil) termine la saisie :
le texte reste et le signet est sélectionné. Un texte vide supprime le signet. Le texte tient sur une ligne (60 caractères au plus).
Toute la saisie s'annule d'un seul `Ctrl + Z`, et les touches tapées ne déclenchent jamais les raccourcis de l'éditeur.

Pour renommer un signet, touchez-le avec l'outil **Signet**, ou double-cliquez dessus avec l'outil **Sélection**. Pour le
déplacer, sélectionnez-le (un clic, ou le lasso) et faites-le glisser ; **Supprimer** le retire ; il se duplique, se copie et se colle
comme un texte.

Les signets sont **à l'usage de l'éditeur** : ils sont visibles à l'écran, mais **absents des exports en image (PNG, JPG, WebP,
SVG), des vidéos, de la relecture, de l'impression, de la projection et du partage sur le réseau**. Ils servent à trois choses :

- **le sommaire du classeur** (voir **Le sommaire d'un classeur**, chapitre 7) ;
- **la recherche** de la bibliothèque, qui trouve une note par le texte de ses signets ;
- **les signets du PDF** : le PDF d'un classeur (**Exporter le classeur en PDF…**) a, sous le signet de chaque note, ceux de ses
  signets, qui mènent à leur endroit sur la page ; le PDF d'une seule note a ses signets pour plan.

Aucun calque ne masque un signet : il reste visible, sélectionnable et dans le sommaire même si son calque est masqué. « Revenir à
l'original » ne l'enlève pas (un signet n'est pas du tracé). Une note qui n'a que des signets reste une page vide pour l'export.

### Redresser les formes

Sélectionnez des traits, puis **Redresser les formes** (ou la touche `F`) : les droites, cercles, ellipses, rectangles
(même tournés) et flèches tracés à main levée deviennent des formes nettes, avec le même outil, la même couleur et la
même épaisseur. L'écriture et tout ce qui n'est pas une forme nette ne changent pas. Ardoise redresse les droites et
flèches d'au moins 1 cm, les cercles, ellipses et rectangles d'au moins 1,3 cm ; un petit ovale debout (moins de
2,5 cm) ressemble à un « O » ou à un « 0 » et reste tel quel. Sélectionnez plutôt les dessins que le texte. Une seule
action annule tout (`Ctrl + Z`).

**Régler le redressement.** Le petit bouton à droite du bouton **Redresser les formes** (**Réglage du redressement**, une icône de réglage) ouvre un panneau avec
trois réglages :

- **Tolérance** : de **stricte** à **souple**, en cinq crans. Souple redresse aussi les formes tracées moins
  nettement ; stricte, seulement les plus nettes. Le cran du milieu, **normale**, est le réglage d'avant.
- **Taille minimale** : de 5 à 25 mm (10 mm au départ). Une droite ou une flèche doit avoir au moins cette taille, un
  cercle, une ellipse ou un rectangle 3 mm de plus. Sous 1 cm, des lettres peuvent être redressées : sélectionnez alors
  seulement les dessins.
- **Alignement automatique** : deux cases, chacune avec son curseur. La première rend horizontal ou vertical un trait
  qui l'est presque (jusqu'à 4° au départ, de 1° à 10°) ; la seconde fait un cercle ou un carré d'une forme presque
  ronde ou carrée (à moins de 10 % au départ, de 2 % à 20 %). Décochée, la case laisse la forme comme vous l'avez tracée.

Tant que le panneau est ouvert et que des traits sont sélectionnés, la page montre en **pointillés bleus** ce qui serait
redressé (les traits d'origine pâlissent), et le panneau dit combien de formes sont reconnues : bougez les curseurs
pour les voir apparaître ou disparaître. Rien ne change dans la note tant que vous n'avez pas cliqué sur **Redresser**
(ou sur le bouton **Redresser les formes**, ou appuyé sur `F`) ; `Ctrl + Z` annule comme d'habitude. `Échap` ou un clic
ailleurs ferme le panneau sans rien redresser, et les valeurs choisies restent. **Valeurs par défaut** remet celles
d'avant le réglage.

Les valeurs sont retenues pour toutes les notes (et par l'assistant, qui redresse avec les mêmes). Le panneau ne
dépasse jamais le bas de la fenêtre : si elle est basse, il défile à la molette ou au doigt, et ses boutons restent
visibles en bas. Un petit ovale debout (un « O », un « 0 ») n'est jamais redressé, quel que soit le réglage.

### Copier et coller

`Ctrl + C`, `Ctrl + X` et `Ctrl + V` (ou les boutons du bloc **Sélection**) copient, coupent et collent des traits, des
textes et des signets *d'une note à l'autre*, d'une fenêtre à l'autre, par le presse-papiers de Windows. Les traits collés gardent leur
couleur, leur outil et leur épaisseur, les textes leur police, leur taille et leur couleur ; ils sont rejoués en dernier. Ce
qui ne vient pas d'Ardoise est ignoré.

Le **clic droit sur la page** (à la souris : pas avec le bouton du stylet, qui lasse, ni avec le doigt) ouvre un menu des mêmes
commandes. Avec l'outil **Sélection**, le trait ou le texte sous le pointeur est d'abord sélectionné s'il ne l'est pas ; une
sélection qui existe reste telle quelle quand le pointeur est sur une de ses parties. Avec une sélection : **Copier**,
**Couper**, **Coller**, **Dupliquer**, **Modifier** (un seul texte, une seule équation ou un seul signet), **Redresser les
formes** (quand des traits sont sélectionnés) et **Supprimer**. Sans sélection : **Coller**, **Tout sélectionner**,
**Annuler** et **Rétablir**. **Coller** est toujours proposé : si le presse-papiers ne contient rien d'Ardoise, un message le
dit. Rien pendant la relecture ni avec le panneau de découpe ouvert.

### La feuille : papier, modèle, format, orientation

L'onglet **Page** du panneau de côté règle la feuille :

- **Papier** : quatre pastilles de couleur, **Blanc**, **Crème**, **Kraft**, **Ardoise noire** (et **Personnalisé** quand
  la note a un papier d'une autre couleur) ; **Modèle** : **Uni**, **Ligné**, **Seyès**, **Quadrillé 5 mm**,
  **Millimétré**, **Pointillé**, **Portées** ; interrupteur **Texture du papier**.
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

Dans l'onglet **Fond** du panneau de côté, la section **Image ou PDF de fond** place une image (PNG ou JPEG) ou une page de PDF *sous*
l'écriture, pour annoter une fiche, un document ou une photo : **Choisir un PDF ou une image…**, la page du PDF si
besoin, puis **Mettre en fond**. Pour un PDF de plusieurs pages, Ardoise montre une **vignette de chaque page**, son
numéro dessous, pour reconnaître celle qu'il vous faut : cliquez sur la vignette voulue (la première est choisie au
départ, la page choisie est entourée), ou, au clavier, `Tab` pour entrer dans la liste, les flèches pour passer d'une
page à l'autre (`↑` et `↓` d'une ligne, `←` et `→` d'une page), `Début` et `Fin` pour la première et la dernière page.
Les vignettes se dessinent quand elles arrivent à l'écran, si bien qu'un PDF de plusieurs centaines de pages s'ouvre
tout de suite ; à partir de huit pages, un champ **Page** permet d'aller directement à un numéro. Une page que
Windows n'arrive pas à dessiner reste choisissable : sa vignette dit « Aperçu indisponible ». Réglez le **Placement** (**Entière**, **Pleine page**, **Étirée**) et l'**Opacité** ; **Remplacer…**
change le fichier, **Retirer l'image** l'enlève, **Tourner l'image seule** tourne l'image sans toucher à la feuille ni
à l'écriture. Si l'image n'a pas l'orientation de la note, Ardoise propose de passer la note dans l'orientation de
l'image. Tout est annulable. L'image suit les rotations, découpes et assemblages et figure dans les miniatures, les
exports et les vidéos. Les PDF sont rendus par Windows.

### Calques

La zone **Calques**, dans l'onglet **Calques** du panneau de côté (ouvert au départ), liste les calques de la page, le plus haut en premier. Un calque est
une feuille transparente superposée aux autres : l'écriture d'une tablette, vos annotations, une correction.

- **Choisir un calque** : cliquez sur sa ligne, qui s'entoure d'un cadre. **Le stylo, la gomme et la sélection n'agissent
  que sur le calque choisi** : le lasso, le clic, `Ctrl + A` (**Tout sélectionner**) et la gomme ne prennent que ses
  traits, et ce que vous dessinez, collez ou dupliquez y va. Pour toucher à l'encre d'un autre calque, choisissez-le
  d'abord. Changer de calque choisi enlève la sélection. À l'ouverture d'une note, le calque choisi est le plus haut des
  calques affichés.
- **Afficher ou masquer** : l'*œil* au début de la ligne ; il est *barré* quand le calque est masqué, et son nom passe en
  gris italique. Un calque masqué n'est ni dessiné à l'écran ni exporté (chapitre 10).
- **Renommer** : le *crayon* après le nom. Tapez le nom, `Entrée` le garde, `Échap` rend l'ancien (cliquer ailleurs le
  garde aussi) ; un nom vide ne change rien.
- **Monter**, **descendre**, **fusionner avec le calque du dessous** : les boutons **↑**, **↓** et **⤓**, sur la ligne
  choisie seulement.
- **Ajouter un calque** (sous la liste) crée un calque tout en haut et le choisit.
- **Un avertissement** s'affiche quand le calque choisi est masqué : « Le calque choisi, « … », est masqué : affichez-le
  pour y écrire. » Tant qu'il l'est, le stylo n'écrit pas, et la gomme et la sélection n'y trouvent rien : affichez-le,
  ou choisissez un autre calque.

Le **clic droit sur un calque** le choisit et ouvre le menu de ses actions : **Renommer…**, **Masquer** (ou **Afficher**),
**Monter**, **Descendre**, **Fusionner avec le calque du dessous** (et, en mode direct, **Faire écrire la tablette ici**).

Ajouter, masquer, renommer, déplacer et fusionner s'annulent avec `Ctrl + Z`. Pour les calques pendant le mode direct,
voir **Corriger pendant le direct** (chapitre 6).

### Projeter la note au vidéoprojecteur

*Ne demande pas la tablette.* Pour montrer à la classe la note que vous corrigez ou complétez, sans passer par le mode
direct : dans l'onglet **Projection** du panneau de côté, bloc **Vidéoprojecteur**, choisissez l'**Écran de projection**
(**Actualiser la liste des écrans** la remet à jour) et cliquez sur **Projeter**. Comme pour le direct, branchez le
projecteur et choisissez *Étendre* dans Windows (`Win + P`) avant. La page de l'éditeur s'affiche en plein écran sur le
vidéoprojecteur, avec son papier, son image de fond et ses calques affichés, et elle est **mise à jour à chaque trait** :
ce que vous écrivez au stylet, à la souris ou au doigt apparaît pendant que vous l'écrivez.

- **Ce que la classe voit.** Le trait en cours, puis le résultat de chaque action : annuler, rétablir, gommer, déplacer,
  redimensionner, recolorer, coller, redresser, changer le papier, l'image, le format ou l'orientation de la feuille,
  **Revenir à l'original**. Une gomme ou un déplacement en cours de glissement n'apparaît qu'une fois lâché. La classe ne
  voit *jamais* la sélection, le lasso, le cercle de la gomme, ni l'aperçu d'un redressement, d'une découpe ou d'un
  recadrage. Un calque masqué n'est pas projeté.
- **La zone projetée.** Le bloc **Zone projetée** règle le zoom : **Page entière** (au départ), **Largeur** (la page sur
  toute la largeur du vidéoprojecteur), **Agrandir** et **Réduire**. La zone est *centrée sur ce que l'éditeur montre*, et
  un **cadre en pointillés**, dans l'éditeur, la montre sur la page. Vous pouvez donc agrandir l'éditeur pour écrire petit :
  la classe garde son zoom ; faire défiler l'éditeur déplace la zone projetée.
- **Une fenêtre qui n'affiche que la page** : ni tableau noir, ni pointeur laser, ni annotations par-dessus (c'est l'éditeur
  qui sert à écrire).
- **La relecture** (chapitre 9) est projetée aussi, comme l'éditeur la montre.
- **Une autre note** (`Alt + ←` et `Alt + →`) : la projection reste ouverte et montre l'autre note. Elle s'arrête quand
  vous quittez l'éditeur (retour à la bibliothèque, reprise du mode direct, fermeture de la fenêtre).
- **Arrêter** : **Arrêter la projection**, ou `Échap` dans la fenêtre projetée (`Ctrl + W`, `Ctrl + F4` et `Ctrl + Q` la
  ferment aussi). L'onglet et le point bleu de **Projection** le suivent. Avec un seul écran, la fenêtre recouvre l'éditeur :
  `Échap` la ferme.
- **Une seule fenêtre projetée à la fois** : projeter l'éditeur ferme la projection du direct, et inversement.
- Le **partage sur le réseau** (bloc **Partage sur le réseau**, même onglet) peut tourner en même temps : l'autre ordinateur
  montre la note entière, pas la zone agrandie, avec un léger retard.
- L'assistant ne pilote pas cette projection, et aucune touche ne la lance.

### Découper une note

**Découper** détache une partie de la note dans une nouvelle note, au choix :

- **À un instant** : faites glisser l'**Instant de la découpe** sur la ligne du temps ; tout ce qui a été écrit à partir
  de là passe dans la nouvelle note (titre proposé : « … (suite) »). Les traits concernés sont en couleur.
- **Selon une zone** : tracez sur la page, comme avec le lasso, la zone à détacher (titre proposé : « … (extrait) »).

Ardoise indique combien de traits restent et combien partent, et vous laisse changer le **Titre de la nouvelle note**.
Chaque note doit garder au moins un trait.

### Revenir à l'original

Pour une note reçue de la tablette, **Revenir à l'original** (onglet **Page**, bloc **Original**) rétablit l'encre telle qu'elle est
arrivée, sans vos modifications. Ardoise précise ce qui est perdu (traits ajoutés, effacés, déplacés, calques, rotation,
format) et ce qui reste (titre, étiquettes, favori, papier, narration, image de fond). L'action est annulable
(`Ctrl + Z`) ; enregistrez ensuite pour la garder. Pour une note qui n'a jamais été modifiée, il n'y a pas encore
d'original : Ardoise garde la version précédente la première fois que vous enregistrez une modification de l'encre, ou
la fois où le mode direct reprend la feuille et y écrit (voir le chapitre 6). L'original est toujours la *première*
version de la note, jamais l'état juste avant la dernière modification.

### Une copie retouchée par un assistant

Quand votre assistant (chapitre 14) retouche une note, il n'écrit jamais l'encre de l'originale : il en fait une
**copie**, à côté, qu'Ardoise reconnaît. Ouvrez cette copie : sous la barre du haut, une ligne dit **Copie retouchée de
« Titre »** (« Note sans titre » si l'originale n'a pas de titre), avec trois boutons.

- **Ouvrir l'original** ouvre la note d'origine dans l'éditeur, comme les flèches « Page précédente » et « Page suivante »
  (avec des modifications non enregistrées, Ardoise demande d'abord ce qu'il faut en faire). Le bouton est grisé, avec
  « L'original n'est pas dans la bibliothèque », si Ardoise ne la trouve pas : elle est peut-être à la corbeille
  (**restaurez-la depuis la corbeille**), pas encore synchronisée sur ce PC, ou illisible ici.
- **Ce qui a changé** ouvre le compte rendu de l'assistant : une ligne datée par retouche, avec chaque changement et
  chaque lettre fabriquée. C'est un texte écrit par l'assistant, qui peut se tromper : regardez la copie.
- **Comparer** montre l'original et la copie côte à côte (l'un au-dessus de l'autre dans une fenêtre étroite). La copie
  y est montrée telle qu'elle est enregistrée : ses modifications non enregistrées n'y sont pas. Grisé, lui aussi,
  quand l'original n'est pas dans la bibliothèque.

**Deux sens du mot « original ».** **Ouvrir l'original** et **Comparer** montrent la note d'origine *telle qu'elle est
maintenant* : si vous l'avez modifiée après la copie, **Comparer** montre aussi ces modifications. **Revenir à l'original**,
sur la copie, rétablit autre chose : la *première* encre de cette note (pour une note venue de la tablette, la page
telle que la tablette l'a envoyée), qui n'est peut-être ni la note d'origine d'aujourd'hui ni la copie retouchée.

Une copie retouchée est une note comme les autres : vous pouvez la modifier à la main, la renommer, l'exporter, l'imprimer,
la reprendre en mode direct (pour la projeter) ou la supprimer, ce qui veut dire **Mettre à la corbeille** (d'où vous
la restaurez). Mettre l'*original* à la corbeille ne touche pas la copie : elle reste, et ses deux boutons « original »
sont grisés. Ce qu'on peut demander à l'assistant, ce qu'Ardoise garantit et ce qui n'est que demandé, tout cela est
dans le guide [L'assistant](assistant.fr.md).

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

Une note a *une seule narration*, un son rangé *dans la note*. L'onglet **Son** du panneau de côté dit ce que la note
contient (« Narration enregistrée dans la note (…) » ou « Cette note n'a pas de narration. ») et, en dehors d'une
relecture, propose deux façons de l'enregistrer, au choix par le contrôle **Enregistrer le son** (le choix est retenu sur
ce PC) : **Pendant que j'écris**, au départ, et **Pendant une relecture**. Dans les deux cas, choisissez le **Micro** (la
liste mémorise votre choix sur ce PC ; **Actualiser la liste des micros** si vous venez de le brancher) et regardez le
**Niveau du micro** avant de lancer : **Démarrer**, puis **Arrêter et enregistrer**.

**Pendant que j'écris** (*Ne demande pas la tablette.*). Vous parlez *en écrivant dans l'éditeur*, au stylet, à la souris
ou au doigt (si **Dessiner au doigt** est activé dans les **Réglages**). Les traits écrits pendant l'enregistrement
prennent l'heure du son et se rejouent avec lui, au rythme où vous les avez écrits, silences compris, **après** les traits
que la note contient déjà.

1. Dans l'onglet **Son**, choisissez **Pendant que j'écris**, ouvrez **Enregistrer en écrivant**, choisissez le micro.
2. **Démarrer**. Si la note a déjà une narration, Ardoise demande d'abord : « Cette note a déjà une narration (…). La
   remplacer par ce nouvel enregistrement ? » avec **Remplacer** ou **Garder l'ancienne** (rien n'est alors enregistré).
   La même question est posée dans le mode **Pendant une relecture** (qui remplaçait jusqu'ici en le disant seulement).
3. Parlez et écrivez : « Écrivez : vos traits suivent le son. » Un point rouge marque l'onglet **Son**.
4. **Arrêter et enregistrer** : la note est *enregistrée aussitôt*, avec le son et ce que vous avez écrit (« Son enregistré
   (…) avec vos traits. »). **Annuler** jette le son et *garde les traits*. Si l'enregistrement de la note échoue (elle a
   changé sur un autre PC), le son reste en mémoire (« Le son est gardé en mémoire… »), le bandeau habituel propose de
   recharger ou d'**Enregistrer comme copie**, et la copie reçoit le son aussi. Recharger la note, ou choisir de ne pas
   l'enregistrer, abandonne ce son.

Ce qui se passe pendant l'enregistrement :

- `Ctrl + Z` et `Ctrl + Y` font disparaître et revenir un trait avec son heure, sans toucher au son ; la gomme, le
  déplacement, le changement de couleur et la suppression ne changent pas les heures des traits. Un trait *collé* paraît à
  l'instant où vous l'avez collé, avec le rythme qu'il avait.
- Quitter l'éditeur, changer de note, reprendre le mode direct, fermer la fenêtre, lancer une relecture, **Revenir à
  l'original**, découper et exporter la vidéo sont refusés avec « Arrêtez ou annulez d'abord l'enregistrement de la
  narration. » Tout le reste se fait comme d'habitude.
- Si rien n'est écrit, le son est enregistré seul, après l'encre : un commentaire. Si Windows bloque le micro (silence),
  Ardoise le dit et ne garde aucun son ; les traits restent.
- Après l'enregistrement, un trait écrit plus tard est placé *après la fin du son*, pour ne pas paraître pendant la
  narration.

*Un point à connaître.* **Lire avec la narration** rejoue ce son à la vitesse normale, sans couper les temps morts : si la
note contenait déjà de l'écriture faite en classe, ses pauses réelles sont rejouées avant que la voix commence, dans la
relecture et dans la vidéo. Sans la narration, la relecture est inchangée.

**Pendant une relecture.** **Enregistrer une narration** (dans l'onglet **Son**, qui s'ouvre tout seul avec la relecture)
vous laisse raconter pendant que le tracé défile. La narration suit la vitesse et les temps morts de la relecture au moment
où vous la démarrez. **Lancer la relecture** (dans l'onglet **Son**, hors relecture) la démarre. *Enregistrez la note
(`Ctrl + S`) avant* : le son est ajouté à la note telle qu'elle est sur le disque.

Dans les deux modes, **Lire avec la narration** rejoue tracé et son ensemble, et **Supprimer la narration** l'enlève.

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
sont pas proposées ; utilisez alors le PDF ou le SVG. Le SVG garde l'épaisseur du trait selon la pression. Tous les
formats dessinent les traits exactement comme l'écran. Pour les calques de la note, voir juste en dessous.

### Les calques à l'export

Quand une note a au moins deux calques affichés qui portent de l'encre, la fenêtre **Exporter** montre, pour le **SVG**
et le **PDF**, le choix **Calques** :

- **Séparés** : un calque du fichier par calque de la note. Dans un SVG, ce sont des calques Inkscape, à afficher ou
  masquer dans Inkscape. Dans un PDF, ce sont des calques PDF, à afficher ou masquer dans Acrobat Reader ou Firefox (leur
  panneau des calques) ; certains lecteurs les montrent tous, sans choix, et un lecteur qui ne gère pas les calques PDF
  dessine tout. Dans un PDF de plusieurs notes, les calques de même nom ne font qu'un seul interrupteur pour toutes les
  pages.
- **Fusionnés** : un seul dessin, comme à l'écran.

Au départ, le SVG est en **Séparés** et le PDF en **Fusionnés**. Ardoise retient le dernier choix, pour chacun des deux
formats.

- **Une image** (PNG, JPG, WebP) n'a pas de calques : ils sont toujours fusionnés. La fenêtre le dit quand une note a
  plusieurs calques.
- **Un calque masqué n'est jamais exporté**, même en **Séparés** : il peut contenir les réponses. La fenêtre le rappelle
  quand une note a un calque masqué qui porte de l'encre.
- L'export automatique pour le téléphone, **Imprimer…** et **Copier l'image** n'ont pas ce choix : leurs
  calques sont fusionnés, comme avant.

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
- **Imprimer…** (éditeur, ou barre de la sélection ; `Ctrl + P` fait la même chose, voir ci-dessous) : la fenêtre d'impression de Windows s'ouvre pour choisir
  l'imprimante. Chaque note est imprimée sur une page, dans son orientation. Vous réglez **Imprimer le fond**, la
  **Taille sur la feuille** (**Taille réelle** ou **Ajustée à la feuille**) et les **Notes par feuille** : **1**, **2 par
  feuille** ou **4 par feuille**. Avec 2 ou 4, Ardoise range les notes dans l'ordre sur la feuille, chacune dans sa case sans
  être déformée (deux notes en hauteur côte à côte sur une feuille en largeur, deux notes en largeur l'une sous l'autre ;
  quatre notes en deux fois deux) ; ce réglage est celui d'Ardoise, pas celui du pilote de l'imprimante (qui n'agirait que
  sur des feuilles déjà faites). Dans la fenêtre de Windows, les *pages* sont alors les feuilles : cinq notes à deux par feuille
  font trois pages.

### Exporter ou imprimer un classeur

**Exporter le classeur en PDF…** (menu **⋯** ou clic droit sur un classeur de la barre latérale) met toutes ses notes dans un
seul PDF, une page par note, **dans l'ordre du classeur** (**Ordre du classeur**, ci-dessus), quel que soit le tri affiché. La
fenêtre est celle de l'export, réduite au PDF, avec la case **Inclure les sous-classeurs** (décochée au départ : elle n'apparaît
que si le classeur a des sous-classeurs) ; leurs notes viennent après celles du classeur, chaque sous-classeur dans son ordre.
**Un classeur qui n'a pas de note à lui**, seulement dans ses sous-classeurs, a la case cochée dès l'ouverture, et la fenêtre
le dit ; décochée, il n'y a plus rien à exporter : le bouton **Exporter…** attend et une phrase dit quoi cocher. Un classeur
sans aucune note affiche « Ce classeur ne contient aucune note. » et le bouton reste grisé.
Le PDF a des **signets** : un par note (son titre, ou son nom de sommaire : sa date, ou **Note sans titre**, selon la case
**Afficher les dates**, décochée au départ), et, avec les sous-classeurs, un par sous-classeur qui regroupe les
signets de ses notes ; sous celui d'une note viennent les **signets posés dans cette note** (outil **Signet**), qui mènent à leur
endroit de la page ; ouvrez le panneau des signets de votre lecteur PDF pour les voir.
La case **Ajouter une page de sommaire au début** (décochée au départ, et gardée) met en tête du PDF une **page de sommaire** :
le nom du classeur, puis les sous-classeurs, les notes et leurs signets, avec leur **numéro de page** et des points de conduite,
comme dans un livre ; une liste longue prend plusieurs pages, et les numéros les comptent. Chaque ligne est un **lien** vers sa
page, et le PDF a un signet **Sommaire** en premier. La page est en A4 portrait si vous avez choisi la page A4, sinon à la taille
de la première note ; elle suit la case **Afficher les dates**. Pour l'avoir sur papier, imprimez ce PDF (**Imprimer le
classeur…** n'ajoute pas de page de sommaire : les numéros de page n'auraient pas de sens avec plusieurs notes par feuille).
**Imprimer le classeur…** ouvre la fenêtre d'impression avec les mêmes notes, dans le même ordre, la même case
**Inclure les sous-classeurs** (cochée d'emblée pour un classeur sans note à lui) et les mêmes phrases ; la page imprimée ne
porte ni date ni signet, il n'y a donc pas de case pour les dates.
- *Glisser hors d'Ardoise* : la poignée d'une carte de la bibliothèque dépose une *image PNG* de la note dans
  l'Explorateur, sur le bureau, dans Word, OneNote ou un courriel (une image par note si plusieurs sont sélectionnées).
  Gardez le bouton enfoncé pendant la préparation de l'image.

### Montrer la feuille en direct

Pour que la feuille (ou la note ouverte) s'affiche dans le navigateur d'un autre ordinateur du réseau, sans fichier à
envoyer : **Partager sur le réseau** (onglet **Projection** du panneau de côté), voir la fin du chapitre 6. Pour montrer
la note ouverte sur un vidéoprojecteur branché à l'ordinateur, c'est **Projeter** dans le même onglet (chapitre 8,
**Projeter la note au vidéoprojecteur**).

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
- Dans l'aperçu du *mode direct*, l'outil **Stylo** (touche `P`) écrit au stylet sur le calque choisi, et le dos du
  stylet efface des traits entiers de ce calque : voir **Corriger pendant le direct** (chapitre 6). Le doigt y déplace
  seulement la zone projetée.

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
le protocole MCP) : il lit vos notes, les voit comme des images, les met au propre, les titre, les range, et peut en
retoucher une copie (écriture rangée, schéma redessiné, fautes corrigées) sans jamais modifier l'originale. Ardoise elle-même
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
| « Cette feuille ne peut pas être continuée avec cette tablette » | Une feuille de la Bamboo Slate se continue avec la Bamboo Slate, une page de la tablette Xiaomi avec la tablette Xiaomi. Allumez l'autre tablette (la Xiaomi se connecte, la Bamboo Slate demande un appui bref), puis **Réessayer** ; la note n'a pas été touchée (chapitre 6). |
| « Cette page de la tablette Xiaomi n'est pas dans le même sens que la tablette telle que vous la tenez… » | La page est en largeur et la position réglée est « en hauteur » (ou l'inverse). Choisissez une position du même sens que la page dans les réglages de la tablette Xiaomi, tenez-la ainsi, puis **Réessayer** ; ou reprenez le direct sur une autre feuille. La note n'a pas été touchée (chapitre 6). |
| « La tablette Xiaomi a envoyé une page qu'Ardoise ne sait pas lire » | Cette page n'a pas été importée et reste sur la tablette, avec celles enregistrées après elle (la tablette ne donne que sa page la plus ancienne). Importez-les avec Mi Home, et gardez le journal de diagnostic (`_ardoise\diagnostics\` dans la bibliothèque, chapitre 13). |
| « La tablette est occupée » | Une synchronisation ou le mode direct est en cours : réessayez à la fin. |
| « Aucune tablette Xiaomi trouvée » | Allumez-la, rapprochez-la de l'ordinateur, fermez Mi Home (ou coupez le Bluetooth du téléphone : elle ne parle qu'à une application à la fois), puis **Chercher de nouveau** (chapitre 4). |
| La tablette Xiaomi n'a plus ses pages | Mi Home les a importées (et effacées) avant Ardoise : les pages vont à l'application qui les prend la première (chapitre 5). |
| « Les pages de la tablette Xiaomi ne sont importées qu'après votre accord » | **Effacer de la tablette après import** est désactivé : confirmez la synchronisation (**Synchroniser et effacer**) ou activez l'option (chapitre 5). |
| « Une page de la tablette Xiaomi n'a pas été reçue en entier » | La page reste sur la tablette (sauf celle d'un écran non effacé : voir la ligne « n'a pas été reçue proprement » ci-dessous). Rapprochez-la de l'ordinateur et synchronisez de nouveau. |
| « Une page de la tablette Xiaomi n'a pas pu être écrite dans la bibliothèque… » | La tablette ne garde plus cette page, mais Ardoise en a gardé une copie de secours (`_ardoise\rescued-pages\` dans la bibliothèque, ou `%LOCALAPPDATA%\Ardoise\blackboard-rescue\` sur ce PC) et l'importera à la prochaine synchronisation de la tablette Xiaomi. Vérifiez que la bibliothèque est accessible et qu'il reste de la place, puis **Synchroniser** (chapitre 5). |
| « Une page de la tablette Xiaomi n'a pu être écrite ni dans la bibliothèque ni sur ce PC… » | Ardoise garde la page en mémoire et réessaiera à la prochaine synchronisation. **Ne fermez pas Ardoise** d'ici là ; rendez la bibliothèque accessible ou libérez de la place, puis synchronisez de nouveau (chapitre 5). |
| « Une page de la tablette Xiaomi n'a pas été reçue proprement… » | Ce qui est arrivé est gardé à part, sans note, dans `_ardoise\rescued-pages\doubtful\` de la bibliothèque. Si une note de cette page a été faite, vérifiez-la, et gardez le journal de diagnostic (chapitre 13). |
| « La tablette Xiaomi contient encore 2 pages enregistrées… » (fenêtre) | Le mode direct ne démarre pas tant que ces pages ne sont pas importées : **Importer et effacer** les importe puis démarre le direct (chapitre 6). |
| « La tablette Xiaomi garde une page… » | Ardoise lui a demandé d'effacer une page, et elle l'a gardée : la note de cette page est déjà dans la bibliothèque. Synchronisez de nouveau ; si cela se répète, gardez le journal de diagnostic (chapitre 13). |
| Une seule page est reçue, les suivantes « attendent » | Cochez **Effacer de la tablette après import** (chapitre 5). |
| La note s'affiche à l'envers ou couchée | Réglez la **Position de la tablette** pour qu'elle corresponde à la façon dont vous la tenez. En mode direct, cliquez sur la bonne position dans la barre du haut : la page en cours se redresse avec ce qui y est écrit (Bamboo Slate et tablette Xiaomi). |
| « Ce direct écrit sur une page de la tablette Xiaomi qui garde sa forme… » | Le direct a repris une feuille (ou écrit sur une image) : seules les deux positions du même sens que cette page sont possibles jusqu'à la fin du direct. Choisissez l'une d'elles ; pour l'autre sens, terminez le direct (chapitre 6). |
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

Ces raccourcis n'agissent pas quand vous tapez dans un champ de texte, sauf ceux de **Fermer et quitter**. Chaque fenêtre
montre aussi sa liste dans l'onglet **Aide** de son panneau de côté : c'est la même liste, que les tests de l'application
comparent aux touches que la fenêtre traite vraiment.

### Les touches d'un navigateur

Ardoise est une application, pas une page web : les touches d'un navigateur n'y font rien. `F5`, `Ctrl + R` (recharger la page, ce
qui ferait perdre une note non enregistrée), `Ctrl + F` hors de la bibliothèque, `F3`, `Ctrl + G` (barre de recherche du
navigateur), `Ctrl + U`, `Ctrl + J`, `Ctrl + H`, `Ctrl + O`, `F7` et `F12` sont sans effet ; `Ctrl + P` et `Ctrl + Maj + P` ouvrent
l'impression d'Ardoise (jamais l'impression de la fenêtre comme une page web). Les touches d'édition (`Ctrl + A`, `C`, `V`, `X`,
`Z`, `Y`) et toutes celles d'Ardoise restent comme décrit dans ce chapitre.

### Fermer et quitter

| Touches | Action |
|---|---|
| `Ctrl + W`, `Ctrl + F4` | Fermer ce qui est ouvert : une boîte de dialogue, le panneau **Réglages**, **Tablette** ou **Diagnostic**, la découpe, la relecture, l'éditeur (retour à la bibliothèque, avec la question habituelle si la note n'est pas enregistrée), la projection. En mode direct, Ardoise demande s'il faut terminer. Dans la bibliothèque seule, ces touches ne font rien, pour ne pas quitter par accident |
| `Ctrl + Q` | Quitter Ardoise, avec les mêmes questions que la croix de la fenêtre. En mode direct, Ardoise demande d'abord confirmation et enregistre toutes les pages. Dans la fenêtre de présentation, la touche ferme seulement la projection |
| `Alt + F4` | Fermer la fenêtre, comme sa croix. Dans la bibliothèque, Ardoise demande d'abord confirmation si une tâche est en cours : export, impression, import, assemblage, synchronisation, opération sur plusieurs notes, test de la tablette |

### Bibliothèque

| Touches | Action |
|---|---|
| `Ctrl + F` | Aller dans le champ de recherche (`Échap` l'efface) |
| `Ctrl + N` | Nouvelle note |
| `Ctrl + P` (ou `Ctrl + Maj + P`) | **Imprimer…** les notes sélectionnées ; sans sélection, **Imprimer le classeur…** du classeur affiché ; sinon un message vous dit de sélectionner des notes (Ardoise n'imprime pas toute une vue sur une supposition). Rien sous une boîte de dialogue |
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
| `Ctrl + P` (ou `Ctrl + Maj + P`) | **Imprimer…** la note ouverte (comme le bouton) |
| `Ctrl + Z` | Annuler |
| `Ctrl + Y`, `Ctrl + Maj + Z` | Rétablir |
| `Ctrl + A` | Tout sélectionner (les traits et les textes du calque choisi) |
| `Ctrl + C`, `Ctrl + X`, `Ctrl + V` | Copier, couper, coller des traits et des textes |
| `Suppr` ou `Retour arrière` | Supprimer la sélection |
| `Échap` | Par étapes, un appui par étape : abandonner le geste en cours, terminer la saisie d'un texte ou d'une équation (ce qui est écrit reste ; une équation au code incorrect reste dans son panneau) ; fermer la découpe, la relecture, le recadrage ou le réglage de **Redresser les formes** ; désélectionner ; enregistrer la note et revenir à la bibliothèque (jamais pendant l'enregistrement d'une narration) |
| `Maj`, `Alt` (tenues pendant le tracé d'une forme) | Inverser **Forme régulière** (`Maj`) ou **Depuis le centre** (`Alt`) pour ce tracé |
| `Alt + ←`, `Alt + →` | **Page précédente**, **Page suivante** (la note d'avant, la note d'après dans la bibliothèque) |
| `V`, `P`, `E`, `T`, `M`, `B`, `H` | Outils **Sélection**, **Stylo**, **Gomme**, **Texte**, **Équation**, **Signet**, **Déplacer la page** |
| `Espace` (maintenu) | Déplacer la page (sauf si un bouton a le focus : `Espace` agit alors sur ce bouton, y compris une icône du rail) |
| `F` | **Redresser les formes** |
| `R` | **Relecture** |
| `Ctrl + 0` | **Ajuster** (page entière). Au clavier AZERTY : `Ctrl + à` ou `Ctrl + Maj + à` |
| `Ctrl + 2` | **Largeur**. Au clavier AZERTY : `Ctrl + é` ou `Ctrl + Maj + é` |
| Molette | Zoom autour du curseur |
| Bouton du milieu (glisser) | Déplacer la page |

Dans la fenêtre de **projection de l'éditeur** (chapitre 8), `Échap`, `Ctrl + W`, `Ctrl + F4` et `Ctrl + Q` ferment la
projection ; la fenêtre n'a pas d'autre touche.

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
| `→`, `Pg suiv`, `Espace` | Page suivante (`Espace` aussi quand un bouton a le focus ; `Pg suiv` aussi quand une liste ou un curseur des onglets **Page** et **Fond** a le focus) |
| `←`, `Pg préc` | Page précédente (`Pg préc` aussi quand une liste ou un curseur des onglets **Page** et **Fond** a le focus) |
| `Ctrl + Suppr` (ou `Ctrl + Retour arrière`) | Effacer la page (**Annuler l'effacement** pendant 8 secondes) |
| `B` | **Tableau noir** |
| `L` | **Pointeur laser** |
| `+` / `−` | Agrandir / réduire la zone projetée |
| `0` (au clavier AZERTY, la touche `à`) | **Page entière** |
| `W` | **Largeur** |
| `Maj + flèches` | Déplacer la zone projetée |
| `Suppr` ou `Retour arrière` | Fenêtre de commande, **avec des traits choisis** : les supprimer (sans sélection, rien). Fenêtre projetée : effacer les annotations écrites sur la présentation |
| `V` | Outil **Sélection** (fenêtre de commande) : lasso ou clic sur un trait du calque choisi |
| `P` | Outil **Stylo** (fenêtre de commande) : écrire avec le stylet ou la souris de l'ordinateur sur le calque choisi |
| `Ctrl + Z` | Annuler un trait ou un coup de gomme de l'ordinateur, une suppression, un changement de couleur ou **Effacer la page** (fenêtre de commande) |
| `Ctrl + Y` ou `Ctrl + Maj + Z` | Rétablir (fenêtre de commande) |
| `Échap` | Abandonner le geste du stylet en cours ; fermer le panneau **Encre de l'ordinateur** s'il est ouvert ; enlever la sélection s'il y en a une ; sinon fermer la projection si elle est affichée ; sinon, demander s'il faut **Terminer** le mode direct |

### Dans les fenêtres

| Touches | Action |
|---|---|
| `Échap` | Fermer la fenêtre ouverte (export, impression, assemblage, import d'Inkspace…), où que soit le focus ; pas pendant une opération en cours, qu'il faut d'abord arrêter ou laisser finir |
| `Échap` | Fermer le panneau **Réglages**, **Tablette** ou **Diagnostic** ; pas pendant un test de la tablette, qui s'arrête par son bouton |
