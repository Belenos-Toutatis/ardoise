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

Ce test enregistre la tablette auprès d'Ardoise, lit ses informations, télécharge trois pages de référence, essaie le mode direct pendant 30 secondes puis vérifie la reconnexion. Chaque octet échangé est enregistré dans la bibliothèque, dans le dossier `_ardoise\diagnostics\` : tout revient à la maison par Nextcloud.

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

Dans `<bibliothèque>\_ardoise\diagnostics\` :
- `AAAA-MM-JJ_HHMMSS.jsonl` : le journal complet des échanges Bluetooth ;
- `AAAA-MM-JJ_HHMMSS.txt` : le rapport lisible ;
- `AAAA-MM-JJ_HHMMSS\page-1.bin`, `page-1.svg`… : chaque page brute reçue de la tablette et son aperçu (ouvrable dans un navigateur).

Et dans `<bibliothèque>\_ardoise\device.json` : l'enregistrement de la tablette, partagé par tous les PC (pas besoin de réenregistrer la tablette sur un autre ordinateur).

**À me signaler** : seulement ce que le journal ne peut pas contenir — la couleur du voyant à un moment surprenant, un message d'erreur de Windows (photo), une étape où la tablette n'a visiblement pas réagi.

---

## H2 — Synchronisation, mode direct et projecteur

**Durée** : 45 minutes à 1 heure. **Tablette** : chargée. **Dongle Bluetooth** : branché. **Téléphone** : Bluetooth **coupé** pendant tout le test. **Pour la dernière partie** : un second écran ou un vidéoprojecteur branché au PC (un simple deuxième écran suffit pour tout vérifier, sauf le débranchement du projecteur).

Ce test vérifie le travail courant : la tablette se connecte toute seule, ses pages deviennent des notes, les réglages tiennent, le mode direct suit le stylet, et la présentation s'affiche sur l'écran choisi. Tout se fait avec l'interface d'Ardoise (« Tester la tablette » ne sert plus que si une étape échoue). Les libellés des boutons du mode direct sont ceux de la fenêtre ; ceux donnés ici sont indicatifs.

### À la maison, avant

1. Fermer Ardoise sur tous les PC, puis remplacer `ardoise.exe` dans le dossier Nextcloud par la nouvelle version.
2. Ne **pas** utiliser l'option `--simulated-tablet` (elle sert aux essais sans tablette).

### Au travail

#### A. Préparation et enregistrement

1. Couper le Bluetooth du téléphone. Brancher le dongle. Vérifier que la synchronisation Nextcloud est active.
2. Lancer `ardoise.exe`. Dans la barre latérale (en bas à gauche), le bloc **Tablette** donne le nom de la tablette, un point de couleur et l'état de la connexion.
   - **Si H1 a déjà enregistré la tablette** : le bloc affiche son nom et « Recherche de la tablette… ». Passer directement à la partie B.
   - **Sinon** : le bloc affiche « Aucune tablette enregistrée ». Cliquer dessus pour ouvrir le panneau, puis sur **Enregistrer la tablette**.
3. L'assistant commence par un **avertissement** : la tablette ne fonctionnera plus avec l'application du téléphone ni avec Inkspace. Le lire, puis cliquer sur **Continuer**.
4. Suivre l'animation, qui montre le dessin de la tablette :

| Étape de l'assistant | Ce qu'il faut faire |
|---|---|
| Maintenez le bouton de la tablette | **Maintenir le bouton enfoncé environ 6 secondes**, jusqu'à ce que le voyant **clignote en bleu**. L'anneau du dessin se remplit pendant ce temps, puis le voyant du dessin clignote : comparer avec la vraie tablette. |
| Recherche, puis connexion | Rien : garder la tablette près de l'ordinateur. |
| Appuyez brièvement sur le bouton | **Un appui bref** sur le bouton (environ 30 secondes pour le faire). |
| Tablette enregistrée | Lire le résumé (nom, batterie, firmware, modèle, taille). Cliquer sur **Terminer**. |

5. Vérifier aussi que **Annuler** est présent à chaque étape. Facultatif : cliquer sur **Annuler** pendant la recherche et constater que la tablette reste « Non enregistrée », puis recommencer jusqu'au bout. Si une erreur s'affiche, **noter son texte exact**, puis cliquer sur **Réessayer**.

#### B. Synchronisation réelle

1. Ouvrir le panneau de la tablette (clic sur le bloc **Tablette**). Laisser cochés **Synchroniser automatiquement à la connexion** et **Effacer de la tablette après import** (c'est l'état par défaut).
2. **Sur la tablette, avec une feuille posée dessus, dessiner 3 pages** reconnaissables (par exemple : un carré et un cercle ; le mot `Ardoise` ; trois traits horizontaux de pression différente). **Appuyer brièvement sur le bouton après chaque page** pour l'enregistrer dans la mémoire de la tablette.
   > Un appui bref seulement : maintenu 6 secondes, le bouton remettrait la tablette en mode enregistrement.
3. Appuyer brièvement sur le bouton une dernière fois pour réveiller la tablette. Observer le bloc **Tablette** : « Recherche de la tablette… », puis « Connexion… », puis « Connectée ». **Noter le temps** entre l'appui et « Connectée ».
4. Comme la synchro automatique est activée et que des pages attendent, la synchronisation démarre seule : barre de progression dans la barre latérale et dans le panneau (« Page 1 sur 3 »…). Si elle ne démarre pas au bout d'une minute, cliquer sur **Synchroniser maintenant** et le noter.
5. **Vérifier** à la fin :
   - le panneau indique « 3 notes enregistrées. 3 pages effacées de la tablette. » et **Pages en mémoire : 0** ;
   - la grille de la bibliothèque contient **3 nouvelles notes** (sans titre, avec la date du jour) : les ouvrir et vérifier que les dessins sont reconnaissables, **à l'endroit**, et que la pression se voit (traits de plus en plus épais) ;
   - un nouveau clic sur **Synchroniser maintenant** répond « Aucune nouvelle page. »
6. **Annulation** : dessiner 3 nouvelles pages (bouton après chacune), reconnecter la tablette, et cliquer sur **Annuler** pendant la synchro. Les pages déjà reçues restent des notes, les autres restent sur la tablette (**Pages en mémoire** le montre) ; un clic sur **Synchroniser maintenant** récupère le reste, sans doublon.
7. **Sans effacement** : la tablette ne livre que sa page **la plus ancienne** ; la suivante n'est accessible qu'une fois la première effacée. Sans l'option d'effacement, **une seule page est donc lisible à la fois**. À vérifier :
   - décocher **Effacer de la tablette après import** : l'astuce sous l'option doit le dire (« La tablette ne livre que sa page la plus ancienne… ») ;
   - dessiner **2 pages** (bouton après chacune), reconnecter la tablette, synchroniser : **une seule** note apparaît (« 1 note enregistrée. »), **Pages en mémoire : 2**, et le panneau affiche le message « 2 pages attendent sur la tablette : seule la plus ancienne peut être lue tant qu'elle n'est pas effacée. Activez « Effacer de la tablette après import » pour les récupérer. » ;
   - synchroniser encore : « Aucune nouvelle page. » et aucune note en double (c'est normal : la plus ancienne page est toujours la même) ;
   - recocher l'option et synchroniser : la seconde page est enfin lue (une nouvelle note, sans copie en double de la première) et les deux pages sont effacées de la tablette ; **Pages en mémoire : 0** et le message d'attente disparaît. **Noter les textes exacts** affichés si ce n'est pas ce qui est décrit ici.
8. Fermer Ardoise puis la relancer : la tablette se reconnecte d'elle-même quand on appuie sur son bouton. Noter si c'est plus rapide ou plus lent que la première fois.

#### C. Réglages de la tablette

Dans le panneau, section **Réglages de la tablette** (la tablette doit être « Connectée » pour les deux premiers réglages) :

1. **Renommer** : écrire `Classe 3B`, cliquer sur **Renommer** : le nom change dans le panneau et dans la barre latérale. Essayer `Éléphant` (accent) et un nom de plus de 15 caractères : le message « Le nom doit faire de 1 à 15 caractères… » apparaît et rien n'est envoyé. Fermer et relancer Ardoise, reconnecter la tablette : le nom est conservé. Remettre `Bamboo Slate` à la fin.
2. **Mettre à l'heure** : cliquer, puis **noter l'écart restant** affiché (moins d'une seconde est normal).
3. **Position de la tablette** (quatre positions ; remplace « droitier / gaucher »). Pour chacune des quatre, choisir la position dans **Réglages de la tablette**, tenir la tablette comme le dessin de la case l'indique, dessiner une page avec le mot `Ardoise` écrit normalement et une flèche vers le haut, appuyer brièvement sur le bouton, reconnecter, synchroniser. À chaque fois la note doit apparaître **à l'endroit** (mot lisible, flèche vers le haut, pas de miroir) :
   - **Paysage, bouton en bas** : la tablette posée comme d'habitude, le bouton et le voyant du côté de soi. Note en paysage. **Vérifier que le bouton est bien du côté indiqué** : sinon, noter où il est ;
   - **Paysage à l'envers, bouton en haut** : la tablette tournée d'un demi-tour (l'ancien « gaucher »). Note en paysage ;
   - **Portrait, bouton à gauche** : la tablette tournée d'un quart de tour dans le sens des aiguilles d'une montre. Note en **portrait** (plus haute que large) ;
   - **Portrait, bouton à droite** : la tablette tournée d'un quart de tour en sens inverse. Note en **portrait**.
   Noter les positions où le mot est couché, à l'envers ou en miroir. Reprendre ensuite **Paysage, bouton en bas**. Un ancien réglage « Gaucher » doit être devenu **Paysage à l'envers** (vérifier après avoir ouvert Ardoise avec un `ardoise.toml` contenant `hand = "left"`).
   - En **mode direct** (partie D), vérifier la même chose avec la barre des quatre boutons de position de la fenêtre de commande : écrire un mot, changer de position sans lever la main de la tablette puis la reposer : la page en cours **tourne avec son tracé** (le mot reste lisible à l'écran), et ce qu'on écrit ensuite apparaît à l'endroit dans la nouvelle position. Revenir à une page précédente dessinée dans l'autre position : elle tourne à son tour.
4. **Batterie** : noter le pourcentage affiché. Brancher la tablette en USB : l'indication « en charge » (éclair) apparaît-elle à côté du pourcentage ?
5. **Oublier la tablette** (en dernier) : un message de confirmation demande « Oublier la tablette… ? » ; **Annuler** ne fait rien, « Oui, l'oublier » oublie la tablette, et le panneau propose alors **Enregistrer la tablette**. Réenregistrer par l'assistant (partie A) avant de continuer.

#### D. Mode direct (sans projecteur)

1. Dans le panneau, cliquer sur **Mode direct**. L'état est « en attente » puis « direct » dès que la tablette envoie des points. Si besoin, appuyer brièvement sur le bouton pour la réveiller.
2. **Dessiner** sur la tablette : le tracé doit apparaître à l'écran pendant qu'on dessine.
   - **Impression de latence** : le tracé suit-il le stylet sans retard visible ? Choisir : *instantané* / *léger retard* / *retard gênant*. Noter aussi la **latence médiane** affichée dans la fenêtre de commande (objectif : moins de 30 ms).
   - Essayer des traits rapides, des courbes, une écriture, en appuyant plus ou moins fort : l'épaisseur suit-elle la pression ? Y a-t-il des sauts ou des points manquants ?
3. **Outil, couleur, texture** : changer d'outil (stylo bille, plume, feutre, crayon, pinceau, surligneur, craie), de couleur et d'épaisseur : le prochain trait en tient compte, les traits précédents ne changent pas.
4. **Pages et calques** : créer une page, revenir à la précédente (boutons, ou touches ← →), ajouter un calque, effacer la page (`Ctrl + Suppr`).
5. **Le bouton de la tablette pendant le mode direct** *(hypothèse à vérifier : on ne sait pas ce qu'envoie la Bamboo Slate)*. Le mode direct étant ouvert, **appuyer brièvement sur le bouton de la tablette 3 fois, en laissant passer quelques secondes entre deux appuis**. **Noter exactement ce qui se passe** :
   - une nouvelle page (ou un nouveau calque, selon l'option **Bouton de la tablette en mode direct** du panneau) apparaît-elle ?
   - rien ne se passe ?
   - le tracé s'arrête, la tablette se déconnecte, l'état de la barre latérale change ?
   - le voyant de la tablette change-t-il de couleur ?

   Refaire l'essai en **maintenant** le bouton 2 secondes (sans aller jusqu'à 6 secondes), puis en changeant l'option **Bouton de la tablette en mode direct** (Nouvelle page / Nouveau calque).

   Si Ardoise reconnaît le bouton, il fait ce que l'option indique. Sinon, les notifications reçues de la tablette sont notées dans le journal de diagnostic avec la mention `unclassified during live mode`, ce qui donnera la vraie signature du bouton ; mais ce journal n'est pas réservé aux erreurs : une session de mode direct qui a reçu une notification inconnue est écrite dans `_ardoise\diagnostics\` **à l'arrêt du mode direct** (étape 6, bouton **Arrêter**), ou à la perte de la liaison. Il faut donc terminer proprement le mode direct après cet essai. Noter quand même **à la main** ce qui a été observé.
6. Fermer le mode direct (bouton **Arrêter**). Vérifier que la session est devenue une **note** de la bibliothèque (une par page dessinée), dont le contenu est celui qu'on a vu à l'écran.
7. **Reprise après coupure** : relancer le mode direct et dessiner, puis éloigner la tablette de l'ordinateur (autre pièce) pendant 10 secondes, puis la rapprocher. L'état « reconnexion » apparaît-il, puis le tracé reprend-il sans qu'il faille tout relancer ? Noter ce qui se passe.

#### E. Projecteur (bureau étendu)

1. Brancher le projecteur (ou le second écran). Appuyer sur **Win + P** et choisir **Étendre**. Vérifier dans Windows que les deux écrans ont chacun leur bureau.
2. Dans Ardoise, ouvrir le **Mode direct**. Dans la fenêtre de commande (sur l'écran du portable), **choisir l'écran** du projecteur dans la liste, puis cliquer sur **Projeter**.
3. Vérifier :
   - la fenêtre de présentation s'ouvre **en plein écran sur l'écran choisi**, **sans aucun élément d'interface** (ni barre de titre, ni boutons, ni curseur de souris après 2 secondes d'immobilité) ;
   - la fenêtre de commande reste **sur le portable** avec les outils, les couleurs, les pages et l'aperçu de ce qui est projeté ;
   - le tracé fait sur la tablette apparaît sur les deux écrans ; l'image projetée est nette et bien cadrée (noter la résolution du projecteur et les bandes noires éventuelles).
4. **Zoom et déplacement** : zoomer (boutons **+** / **−**, ou touches `+` et `-`), déplacer la zone projetée (glisser dans l'aperçu, ou `Maj` + flèches), revenir à la page entière (touche `0` ou bouton **1:1**). Le projecteur suit-il ? L'aperçu montre-t-il le rectangle projeté ?
5. **Tableau noir** : activer (touche `B` ou bouton) : fond ardoise, craie blanche par défaut. Désactiver : retour au fond blanc.
6. **Pointeur laser** : activer (touche `L` ou bouton). **Approcher le stylet de la tablette sans la toucher** : un point rouge suit le stylet sur le projecteur, **sans rien écrire**. Éloigner le stylet : le point disparaît. Toucher la tablette : on écrit de nouveau.
7. **Pages au clavier et au bouton** : `→`, `Espace` ou `Page suivante` passent à la page suivante ; `←` ou `Page précédente` reviennent. Faire de même **avec le bouton de la tablette** (selon l'option du panneau et ce qui a été constaté en D5). Le changement de page apparaît-il tout de suite sur le projecteur ?
8. **Effacement rapide** : `Ctrl + Suppr` efface la page en cours, sur les deux écrans ; les autres pages restent intactes.
9. **Changer d'écran** : cliquer sur **Arrêter la projection**, puis reprojeter sur l'écran du portable lui-même : la présentation le recouvre alors, et `Échap` dans cette fenêtre la ferme. Reprojeter ensuite sur le projecteur.
10. **Mémoire du choix** : quitter le mode direct, fermer Ardoise, la relancer et rouvrir le mode direct : l'écran du projecteur est-il proposé par défaut ?
11. **Débrancher le projecteur pendant la présentation** (ou choisir Win + P → *Écran du PC uniquement*) : **noter ce qui arrive** à la fenêtre de présentation (elle disparaît ? elle reste sur le portable ? Ardoise plante ?), à la fenêtre de commande et au tracé en cours. Le mode direct continue-t-il d'enregistrer ? Rebrancher, refaire Win + P → *Étendre* et vérifier qu'on peut projeter de nouveau.
12. Arrêter le mode direct. Vérifier que les pages dessinées pendant la présentation sont des notes de la bibliothèque.

#### F. Tester la tablette pendant que le service tourne

Le service de la tablette (connexion automatique, synchronisation, mode direct) tourne en permanence. Il est **mis en pause** pendant le test réel (**Tester la tablette…**, en bas du panneau) pour ne pas se disputer la liaison Bluetooth, puis **reprend** à la fin du test.

1. Tablette « Connectée » et service actif, cliquer sur **Tester la tablette…** et lancer le test. Pendant le test, ne pas demander de synchronisation ni de mode direct. Noter si le bloc **Tablette** de la barre latérale change d'état.
2. Terminer le test (ou **Arrêter le test**). Sans relancer Ardoise, le service doit **reprendre** : le bloc **Tablette** repasse par « Recherche de la tablette… » puis « Connectée » (au besoin après un appui bref sur le bouton), et **Synchroniser maintenant** fonctionne de nouveau. Noter le temps de reprise, ou si la tablette reste injoignable jusqu'au redémarrage d'Ardoise.

### Ce qui revient par Nextcloud

- Dans la bibliothèque : les notes synchronisées (parties B et C) et celles du mode direct (D et E), avec leur date. Elles permettent de juger la fidélité des tracés, la pression et les quatre positions de la tablette.
- Dans `<bibliothèque>\_ardoise\diagnostics\` : les journaux (`AAAA-MM-JJ_HHMMSS.jsonl` et `.txt`) **écrits à chaque erreur** de la tablette, de la synchronisation ou du mode direct, **et à l'arrêt d'une session de mode direct qui a reçu une notification inconnue** (`unclassified during live mode` : le bouton de la tablette par exemple), même sans erreur. Une session sans erreur ni notification inconnue n'en laisse pas : l'absence de fichier n'est pas un problème, mais après l'étape D5 il y en a un si Ardoise n'a pas reconnu le bouton.
- Dans `<bibliothèque>\_ardoise\device.json` : l'enregistrement de la tablette (nom compris), partagé par tous les PC.
- Dans `ardoise.toml` (à côté de l'exe) : la section `[tablet]` (options) et le dernier écran de projection de ce PC.

### À me signaler

Ce qui ne revient pas tout seul :

| Quoi | Où |
|---|---|
| Le texte exact de toute erreur affichée (enregistrement, synchronisation, mode direct, projection) et ce qu'elle proposait de faire | A, B, D, E |
| Le temps entre l'appui sur le bouton et « Connectée » ; si la synchro automatique a démarré seule | B3, B4, B8 |
| Une page perdue, en double, tronquée ou à l'envers (dans les quatre positions de la tablette) | B, C3 |
| Sans effacement : le message « N pages attendent sur la tablette… » apparaît-il, et la seconde page est-elle bien récupérée une fois l'option recochée | B7 |
| L'écart affiché après « Mettre à l'heure » ; si le nom et la batterie (avec « en charge ») sont corrects | C |
| **Ce qui se passe à l'appui sur le bouton de la tablette pendant le mode direct** (rien / nouvelle page / nouveau calque / déconnexion / couleur du voyant), appui bref et appui de 2 secondes | D5 |
| L'impression de latence (*instantané*, *léger retard*, *retard gênant*), la latence médiane affichée, les sauts ou points manquants | D2 |
| Ce qui arrive à la coupure puis à la reprise de la liaison en mode direct | D7 |
| Si le service de la tablette reprend seul après « Tester la tablette… » ; le temps de reprise | F |
| Si le projecteur affiche la présentation sans interface ; sa résolution ; les bandes noires | E3 |
| Si le pointeur laser, le tableau noir, le zoom et les pages (clavier et bouton) fonctionnent à la projection | E4 à E8 |
| Ce qui arrive quand on débranche le projecteur pendant la présentation | E11 |
| Une photo du message de Windows s'il y en a un (pare-feu, Bluetooth, écran) | partout |
