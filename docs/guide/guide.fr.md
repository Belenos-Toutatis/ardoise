# Guide d'utilisation d'Ardoise

Ardoise est une application pour Windows qui remplace Wacom Inkspace pour la tablette *Bamboo Slate*. Elle récupère
par Bluetooth les pages que vous écrivez sur la tablette, vous laisse écrire en direct à l'écran (et au
vidéoprojecteur, en classe), range vos notes dans un dossier que vous choisissez, et permet de les retoucher, de les
rejouer et de les exporter.

Ce guide suit l'ordre dans lequel on rencontre les choses. Les noms des boutons et des menus sont écrits en gras,
tels qu'ils apparaissent à l'écran. English version: [guide.en.md](guide.en.md).

> *Où en est cette première version ?* Tout ce qui ne demande pas la tablette (bibliothèque, édition, relecture,
> exports, import des notes d'Inkspace) a été vérifié sous Windows par des tests automatiques ; quelques essais à la
> main restent à faire (impression, stylet et écran tactile, export Word ouvert dans Word…). Tout ce qui parle à la
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

### Installer = copier

1. Créez un dossier pour le programme, par exemple `C:\Users\Prénom\Nextcloud\Ardoise\Programme`.
2. Copiez-y `ardoise.exe` (et, si vous le souhaitez, les guides et la licence qui l'accompagnent).
3. Double-cliquez sur `ardoise.exe`. Créez un raccourci sur le bureau si vous voulez. <!-- M10: à confirmer -->

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
recherche (`handwriting`), l'encodeur OpenH264 si vous l'avez téléchargé (`codecs`), et deux dossiers de *copies de
secours* (`live-backup` pour des pages du direct qui n'ont pas pu être écrites, `narration-backup` pour une narration
qui n'a pas pu être ajoutée à sa note). Le dossier de WebView2, les miniatures et le texte reconnu se reconstruisent
tout seuls ; ne supprimez les copies de secours qu'après avoir vérifié que vous n'en avez plus besoin.

## 3. Premier démarrage

Au premier lancement sur un PC, la fenêtre **Bienvenue dans Ardoise** demande le dossier où ranger vos notes.

1. Cliquez sur **Choisir un dossier…** et choisissez de préférence un dossier synchronisé (Nextcloud…), par exemple
   `C:\Users\Prénom\Nextcloud\Ardoise\Notes`.
2. Ardoise indique s'il contient déjà une bibliothèque (« elle sera utilisée telle quelle ») ou si elle en crée une
   nouvelle. Ce choix ne concerne que cet ordinateur.
3. Cliquez sur **Continuer**.

Vous arrivez dans la *bibliothèque*. Tant qu'elle est vide, elle dit que les pages de la tablette apparaîtront après
la première synchronisation ; vous pouvez déjà créer une note avec **Nouvelle note**. Pour changer de bibliothèque
plus tard : **Réglages**, puis **Changer** à côté de **Dossier de la bibliothèque**. Ardoise crée dans la
bibliothèque un dossier `_ardoise` (enregistrement de la tablette, journaux de diagnostic, corbeille). Il n'est pas
caché, pour que Nextcloud le synchronise : n'y touchez pas.

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
« N notes enregistrées »). Si une page ne peut pas être écrite dans la bibliothèque (disque plein, dossier
inaccessible…), un message l'explique, la page reste en mémoire et une copie de secours est écrite sur ce PC à la
fermeture d'Ardoise.

### La fenêtre de commande

- **Outil** (stylo à bille, plume, feutre, crayon, pinceau, surligneur, craie), **Couleur** (dont **Blanc craie** et
  **Jaune**) et **Épaisseur** : le trait suivant en tient compte, pas les précédents.
- **Pages** : **Page précédente**, **Page suivante** (la position s'affiche : « Page 2 / 5 »), **Nouvelle page**.
  **Nouveau calque** ajoute un calque à la page en cours. **Effacer la page** efface la page en cours seulement. <!-- M10: à confirmer -->
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

### Projeter sur un vidéoprojecteur

1. Branchez le projecteur et, dans Windows, appuyez sur `Win + P` puis choisissez *Étendre* : chaque écran a son bureau.
2. Dans la section **Projection** de la fenêtre de commande, choisissez l'**Écran de projection** dans la liste (le
   bouton **Actualiser la liste des écrans** la remet à jour) et cliquez sur **Projeter**.
3. Une fenêtre de *présentation* s'ouvre *en plein écran sur l'écran choisi*, sans aucun élément d'interface (le
   curseur de la souris disparaît après 2 secondes d'immobilité). La fenêtre de commande reste sur l'écran de
   l'ordinateur. Ce que vous écrivez s'affiche sur les deux.
4. **Arrêter la projection** ferme la présentation. Ardoise retient l'écran choisi sur ce PC.

<!-- M10: à confirmer -->
Avec un seul écran, la présentation recouvre la fenêtre de commande : appuyez sur `Échap` dans la présentation pour la
fermer.

### Écrire par-dessus la présentation

Sur la fenêtre projetée, vous pouvez annoter la page au stylet, au doigt ou à la souris : les annotations sont
rouges (jaunes sur le tableau noir). Le dos du stylet les efface une à une ; le bouton **Effacer les annotations**
(qui apparaît avec le curseur) ou la touche `Suppr` les efface toutes ; elles disparaissent aussi au changement de page.
*Elles ne sont jamais enregistrées dans la note.* Deux doigts zooment la zone projetée.

### Au clavier

Les mêmes touches marchent dans la fenêtre de commande et dans la présentation (une télécommande de présentation
envoie Page suivante et Page précédente) : voir le chapitre 17.

## 7. La bibliothèque

### Parcourir

La bibliothèque est la page d'accueil. En haut : le champ **Rechercher une note**, le tri (**Trier les notes** :
**Date : la plus récente d'abord**, **Date : la plus ancienne d'abord**, **Titre : A → Z**, **Titre : Z → A**), les
boutons **Nouvelle note**, **Depuis un PDF ou une image…** et **Réglages**. À gauche : la barre latérale avec
**Toutes les notes**, **Favoris**, les **Classeurs**, les **Étiquettes**, la **Corbeille**, et en bas le bloc
**Tablette** et le bouton **Tester la tablette**.

Une carte montre la miniature, le titre (ou la date si la note n'a pas de titre), la date et les étiquettes. Un clic
ouvre la note dans l'éditeur. Sur la carte : la case pour *sélectionner*, l'étoile des favoris, le menu
**Actions pour…** (**Renommer**, favoris, **Mettre à la corbeille**) et une poignée, qui apparaît sur la carte, pour
glisser la note hors d'Ardoise (chapitre 10). Renommer une note renomme aussi son fichier.

Les miniatures sont gardées dans un cache propre à chaque PC. Quand une note change sur un autre ordinateur
(synchronisation Nextcloud), la bibliothèque se met à jour toute seule.

### Classeurs

<!-- M10: à confirmer -->
Un classeur est un simple *sous-dossier* de la bibliothèque. Cliquez sur l'icône **Nouveau classeur** (ou sur
**Nouveau sous-classeur dans…** à côté d'un classeur), tapez le **Nom du classeur** et cliquez sur **Créer**. Pour
ranger des notes, glissez-les sur un classeur de la barre latérale, ou sélectionnez-les puis **Déplacer vers…**
(**Bibliothèque (racine)** ramène à la racine). Ardoise ne propose pas de renommer ou de supprimer un classeur : faites-le
dans l'Explorateur, c'est un dossier ordinaire.

### Étiquettes et favoris

<!-- M10: à confirmer -->
Sélectionnez une ou plusieurs notes, cliquez sur **Étiquettes…**, tapez une étiquette (plusieurs, séparées par une
virgule) et cliquez sur **Ajouter** ; une étiquette existante se retire avec sa croix. Un clic sur une étiquette de la
barre latérale, ou sur celle d'une carte, n'affiche que les notes qui la portent. L'étoile d'une carte (ou
**Mettre en favoris**) ajoute aux **Favoris**.

### Sélection multiple

Cochez les cartes, ou : `Ctrl + clic` ajoute ou retire une note, `Maj + clic` sélectionne une plage, `Ctrl + A` tout,
`Échap` désélectionne. Une barre d'actions apparaît : **Déplacer vers…**, **Étiquettes…**, **Exporter…**,
**Imprimer…**, **Assembler…**, **Mettre en favoris** (ou **Retirer des favoris**), **Mettre à la corbeille**,
**Désélectionner**.

**Assembler** (au moins deux notes) crée une nouvelle note, sans modifier les originales : **Superposer sur une page**
(toutes les notes sur une seule page, chacune sur ses propres calques) ou **Mettre bout à bout (page longue)** (les
notes l'une sous l'autre). Donnez un **Titre de la nouvelle note** et cliquez sur **Assembler**. Seule l'image de fond
de la première note est gardée.

### Créer une note

<!-- M10: à confirmer -->
- **Nouvelle note** : une page blanche A4 en paysage, dans le classeur affiché.
- **Depuis un PDF ou une image…** : une note par page du PDF choisie (**Toutes les pages** ou **Certaines pages**,
  par exemple `1-3, 5`), à la taille et dans l'orientation de la page, avec la page en fond pour écrire par-dessus. Une
  image donne une note de la taille d'une feuille A4, dans l'orientation de l'image. Les PDF protégés par un mot de
  passe ne sont pas lus : enregistrez-en une copie sans mot de passe.

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

### Notes en conflit et fichiers illisibles

Quand Nextcloud crée une copie « en conflit » d'une note modifiée sur deux PC, la carte porte l'étiquette
**Copie en conflit** : ouvrez les deux versions, gardez celle que vous voulez et mettez l'autre à la corbeille. Un
message **Fichiers illisibles : N** liste des fichiers que l'application ne sait pas afficher ; ils ne sont pas modifiés.

## 8. Modifier une note

Cliquez sur une carte pour ouvrir la note. En haut : **Bibliothèque** (retour), le **Titre de la note**, l'état
(**Enregistrée** ou **Modifications non enregistrées**), **Annuler (Ctrl+Z)**, **Rétablir (Ctrl+Y)** et
**Enregistrer** (`Ctrl + S`). *Rien n'est enregistré tant que vous ne cliquez pas sur Enregistrer* ; en quittant avec des
modifications, Ardoise demande : **Enregistrer et revenir**, **Revenir sans enregistrer** ou **Rester**. Annuler et
rétablir n'ont pas de limite pendant que la note est ouverte.

Si la note a été modifiée entre-temps sur un autre PC, Ardoise *refuse d'écraser l'autre version* : vos modifications
restent à l'écran et **Enregistrer comme copie** les sauve dans une nouvelle note.

<!-- M10: à confirmer -->

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
  Les changements de couleur, d'encre et d'épaisseur s'appliquent alors à la sélection ; on la déplace et on la
  redimensionne à la souris.
- **Déplacer la page** : faire glisser la page (la barre d'espace maintenue, ou le bouton du milieu de la souris,
  font la même chose avec n'importe quel outil).

**Zoom** : la molette zoome autour du curseur ; en bas à droite, **Agrandir**, **Réduire**, **Ajuster** (page
entière) et **Largeur** (`Ctrl + 2`, la page sur toute la largeur de la fenêtre).

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
d'original : Ardoise garde la version précédente la première fois que vous enregistrez une modification de l'encre.

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

### Copier, imprimer, glisser

- **Copier l'image** : met la note dans le presse-papiers de Windows ; collez-la dans Word ou OneNote.
- **Imprimer…** (éditeur, ou barre de la sélection) : la fenêtre d'impression de Windows s'ouvre pour choisir
  l'imprimante. Chaque note est imprimée sur une page, dans son orientation. Vous réglez **Imprimer le fond** et la
  **Taille sur la feuille** (**Taille réelle** ou **Ajustée à la feuille**).
- *Glisser hors d'Ardoise* : la poignée d'une carte de la bibliothèque dépose une *image PNG* de la note dans
  l'Explorateur, sur le bureau, dans Word, OneNote ou un courriel (une image par note si plusieurs sont sélectionnées).
  Gardez le bouton enfoncé pendant la préparation de l'image.

## 11. Importer les notes d'Inkspace

<!-- M10: à confirmer -->
Si vous avez des notes dans Wacom Inkspace sur ce PC, Ardoise peut les importer. *L'import se fait pour l'instant en
ligne de commande*, il n'y a pas de bouton dans l'application.

1. Fermez Inkspace (par précaution). Ardoise ne modifie rien chez Inkspace : elle lit ses données en mémoire.
2. Ouvrez PowerShell dans le dossier d'`ardoise.exe` (dans l'Explorateur, tapez `powershell` dans la barre d'adresse
   puis Entrée) et lancez, en indiquant *le dossier de votre bibliothèque* :

   ```
   .\ardoise.exe --import-inkspace --library "C:\Users\Prénom\Nextcloud\Ardoise\Notes" | Out-String
   ```

   (`| Out-String` fait attendre PowerShell et affiche le compte rendu à la fin.) Ardoise trouve seule l'installation
   d'Inkspace ; si elle n'y arrive pas, ajoutez en premier argument le dossier `LocalState` d'une copie. Sans
   `--library`, Ardoise affiche le tableau des notes trouvées mais n'enregistre rien.
3. Le compte rendu liste chaque note (date, orientation, nombre de calques, de traits, de points, minutage) et se termine
   par « N/N notes importées ».

Les notes arrivent dans un classeur *Inkspace*. Importer deux fois ne crée pas de doublon, et une note que vous avez
mise à la corbeille n'est pas rapportée. À savoir :

- Inkspace ne garde pas la *pression* mais l'épaisseur du trait : Ardoise l'estime à partir de l'épaisseur. L'aspect
  est très proche, mais pas identique.
- Inkspace ne garde pas toujours le *minutage* de l'écriture : quand il manque, Ardoise l'invente (colonne
  « synthétisé » du compte rendu), et la relecture de ces notes n'est qu'indicative.
- Les notes importées sont écrites à l'*outil stylo à bille*, sur fond blanc, sans titre ni papier ; vous pouvez les
  retoucher comme les autres.

## 12. Stylet et écran tactile

Sur un PC à écran tactile et stylet (une Surface, par exemple), l'éditeur, la relecture, l'aperçu du mode direct et la
présentation s'utilisent au stylet ou au doigt.

*Le stylet*

- Le trait *s'épaissit avec la pression*.
- Le *dos du stylet* (la gomme) *efface*, avec les réglages de la gomme (**Traits entiers** ou **Partielle**), quel que
  soit l'outil choisi. Un cercle de gomme apparaît quand il survole la page.
- Le *bouton du stylet* trace un *lasso de sélection*, quel que soit l'outil.
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

Deux cases à connaître : **Effacer les pages de la tablette une fois enregistrées** (cochée par défaut) et
**Simulation (sans tablette)**. Le test peut enregistrer la tablette si elle ne l'est pas encore, avec les mêmes
conséquences qu'au chapitre 4.

> *Attention aux pages qui sont déjà sur la tablette.* Pendant le test, les pages en mémoire sont téléchargées, puis
> effacées de la tablette si la case est cochée. Elles sont enregistrées et vérifiées avant d'être effacées : elles sont conservées dans le dossier des diagnostics (voir plus bas) et, avec la case cochée, aussi enregistrées comme notes de la bibliothèque. Si la tablette contient des pages importantes, vous pouvez aussi les récupérer d'abord par la synchronisation normale (chapitre 5). <!-- M10: à confirmer -->

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
[L'assistant](assistant.fr.md) (English: [the assistant guide](assistant.en.md)).

## 15. Vie privée

- Vos notes restent dans *votre* dossier, sur vos ordinateurs. Ardoise n'a pas de compte et n'envoie rien nulle part.
- Seule connexion sortante : le téléchargement d'OpenH264 chez Cisco, *seulement si vous le demandez*. Ardoise
  n'écoute sur aucun port réseau.
- La reconnaissance d'écriture est celle de Windows, hors connexion, et le texte reconnu reste dans un cache local à
  chaque PC.
- Les copies de secours et caches sont sous `%LOCALAPPDATA%\Ardoise`, jamais dans la bibliothèque.
- Les *journaux de diagnostic* sont dans la bibliothèque (donc dans Nextcloud) : n'y mettez rien de personnel.
- Si vous utilisez *un assistant* (chapitre 14), ce qu'il lit part chez son fournisseur selon les réglages de votre
  compte. Le texte et l'image de vos notes peuvent contenir des noms d'élèves : réfléchissez avant de lui confier des
  notes d'élèves, et préférez la **Lecture seule** quand il n'a besoin que de lire. Rien n'est jamais supprimé
  définitivement par l'assistant (une note supprimée va à la corbeille).

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
| `Espace` (maintenu) | Déplacer la page |
| `F` | **Redresser les formes** |
| `R` | **Relecture** |
| `Ctrl + 0` | **Ajuster** (page entière) |
| `Ctrl + 2` | **Largeur** |
| Molette | Zoom autour du curseur |
| Bouton du milieu (glisser) | Déplacer la page |

### Relecture

| Touches | Action |
|---|---|
| `Espace` | Lecture / pause |
| `R` ou `Échap` | Quitter la relecture |
| Flèches (sur la ligne du temps) | Reculer ou avancer d'une seconde |
| `Pg préc`, `Pg suiv` (sur la ligne du temps) | Reculer ou avancer de 10 secondes |
| `Ctrl + 0`, `Ctrl + 2` | **Ajuster**, **Largeur** |

### Mode direct (fenêtre de commande et présentation)

| Touches | Action |
|---|---|
| `→`, `Pg suiv`, `Espace` | Page suivante |
| `←`, `Pg préc` | Page précédente |
| `Ctrl + Suppr` (ou `Ctrl + Retour arrière`) | Effacer la page |
| `B` | **Tableau noir** |
| `L` | **Pointeur laser** |
| `+` / `−` | Agrandir / réduire la zone projetée |
| `0` | **Page entière** |
| `W` | **Largeur** |
| `Maj + flèches` | Déplacer la zone projetée |
| `Suppr` ou `Retour arrière` | Effacer les annotations écrites sur la présentation |
| `Échap` | **Terminer** (dans la présentation : la fermer) |
