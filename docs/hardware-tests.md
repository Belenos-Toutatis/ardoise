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
2. Lancer `ardoise.exe` (double-clic). Cliquer sur **Tester la tablette** (en bas de la barre latérale).
3. Laisser cochée la case **Effacer les pages de la tablette une fois ajoutées à la bibliothèque comme notes** (les pages sont d'abord enregistrées, vérifiées et ajoutées à la bibliothèque comme notes, ensuite seulement effacées). Ne pas cocher **Simulation**. Cliquer sur **Démarrer le test**.
   > Les pages **déjà présentes** sur la tablette sont aussi téléchargées puis effacées. Elles ne sont pas perdues : chacune devient une **note** de la bibliothèque (comme à la synchronisation normale, sans doublon si elle y est déjà), et une copie brute (`page-N.bin`) et un aperçu (`page-N.svg`) restent dans le dossier des diagnostics. Si une page ne peut pas devenir une note, elle n'est pas effacée de la tablette.
4. Suivre les consignes à l'écran. Après chaque action, cliquer sur **C'est fait, continuer**. En cas de blocage sur une étape, **Passer cette étape** permet d'aller à la suivante ; **Arrêter le test** termine tout de suite (le journal est quand même enregistré).

| Étape à l'écran | Ce qu'il faut faire |
|---|---|
| Adaptateur Bluetooth | Rien : Ardoise vérifie que le dongle est vu et allumé. |
| Pages de référence | Sur une feuille posée sur la tablette, dessiner les **3 pages de référence** ci-dessous, en appuyant brièvement sur le bouton de la tablette **après chaque page** pour l'enregistrer. |
| Recherche de la tablette | La première fois : **maintenir le bouton enfoncé environ 6 secondes**, jusqu'à ce que le voyant **clignote en bleu**. Les fois suivantes : un appui bref suffit. |
| Connexion | Quand Ardoise le demande, **appuyer brièvement sur le bouton** pour confirmer l'enregistrement (quelques secondes pour le faire). |
| Informations | Rien : nom, version, batterie, taille et nombre de pages sont lus. |
| Téléchargement des pages | Rien : les pages sont téléchargées, vérifiées, ajoutées à la bibliothèque comme notes, puis effacées de la tablette. |
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

   Si Ardoise reconnaît le bouton, il fait ce que l'option indique. Sinon, les notifications reçues de la tablette sont notées dans le journal de diagnostic avec la mention `unclassified during live mode`, ce qui donnera la vraie signature du bouton ; mais ce journal n'est pas réservé aux erreurs : une session de mode direct qui a reçu une notification inconnue est écrite dans `_ardoise\diagnostics\` **à l'arrêt du mode direct** (étape 6, bouton **Terminer**), ou à la perte de la liaison. Il faut donc terminer proprement le mode direct après cet essai. Noter quand même **à la main** ce qui a été observé.
6. Fermer le mode direct (bouton **Terminer**). Vérifier que la session est devenue une **note** de la bibliothèque (une par page dessinée), dont le contenu est celui qu'on a vu à l'écran.
7. **Reprise après coupure** : relancer le mode direct et dessiner, puis éloigner la tablette de l'ordinateur (autre pièce) pendant 10 secondes, puis la rapprocher. L'état « reconnexion » apparaît-il, puis le tracé reprend-il sans qu'il faille tout relancer ? Noter ce qui se passe.

#### E. Projecteur (bureau étendu)

1. Brancher le projecteur (ou le second écran). Appuyer sur **Win + P** et choisir **Étendre**. Vérifier dans Windows que les deux écrans ont chacun leur bureau.
2. Dans Ardoise, ouvrir le **Mode direct**. Dans la fenêtre de commande (sur l'écran du portable), **choisir l'écran** du projecteur dans la liste, puis cliquer sur **Projeter**.
3. Vérifier :
   - la fenêtre de présentation s'ouvre **en plein écran sur l'écran choisi**, **sans aucun élément d'interface** (ni barre de titre, ni boutons, ni curseur de souris après 2 secondes d'immobilité) ;
   - la fenêtre de commande reste **sur le portable** avec les outils, les couleurs, les pages et l'aperçu de ce qui est projeté ;
   - le tracé fait sur la tablette apparaît sur les deux écrans ; l'image projetée est nette et bien cadrée (noter la résolution du projecteur et les bandes noires éventuelles).
4. **Zoom et déplacement** : zoomer (boutons **+** / **−**, ou touches `+` et `-`), déplacer la zone projetée (glisser dans l'aperçu, ou `Maj` + flèches), revenir à la page entière (touche `0` ou bouton **Page entière**). Le projecteur suit-il ? L'aperçu montre-t-il le rectangle projeté ?
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


## H1 bis — Après la première session réelle (2 novembre 2026)

**Durée** : 20 minutes. **Tablette** : déjà enregistrée. **Téléphone** : Bluetooth **coupé**.

La première session avec la vraie tablette a montré que la synchronisation était refusée presque à chaque fois : la tablette refuse la commande qui prépare le transfert des pages (`ec`) quand elle est déjà en mode papier. Ardoise l'envoie maintenant avant le mode papier, cherche la tablette enregistrée seulement à sa propre adresse, lit la batterie que la tablette annonce d'elle-même, et une synchronisation qui échoue s'affiche en rouge (« a échoué ») au lieu de passer inaperçue. Ces essais vérifient tout cela.

1. Fermer Ardoise, remplacer `ardoise.exe` par la nouvelle version, puis lancer `.\ardoise.exe --self-test | Out-String` : dernière ligne `SELF-TEST OK`.
2. **Test guidé** : dessiner les 3 pages de référence (bouton après chacune), puis **Tester la tablette**, effacement coché, Simulation décochée. Un appui bref suffit à l'étape « Recherche ». Attendu : « Téléchargement des pages » réussit (3 pages, ou 4 si la page restée sur la tablette le 2 novembre y est encore). En **Mode direct (30 s)**, appuyer **une fois** sur le bouton de la tablette et noter vers quelle seconde.
3. **Synchronisation normale** : fermer le test, dessiner une page, appuyer sur le bouton, attendre. Le panneau doit afficher « Dernière synchronisation : … 1 note enregistrée. 1 page effacée… ».
4. **Deuxième synchronisation sans déconnexion** : juste après, tablette toujours connectée, dessiner une autre page et appuyer sur le bouton. Elle doit arriver dans les secondes qui suivent.
5. **Après le mode direct** : ouvrir le mode direct, dessiner, le terminer, puis enregistrer une page avec le bouton. Elle doit se synchroniser.
6. **Batterie** : rester connecté (ou en mode direct) plusieurs minutes : le pourcentage du panneau doit baisser d'environ 1 par minute.
7. Fermer Ardoise pendant que la tablette est connectée.

**À me signaler** : tout message « a échoué » (texte exact), une page qui n'arrive pas, le pourcentage de batterie qui ne bouge pas. Les journaux de `_ardoise\diagnostics\` reviennent seuls ; ils ne sont écrits qu'en cas de problème ou de message inconnu.

---

## H3 — Stylet et tactile (PC tactile)

**Durée** : 15 à 20 minutes. **Matériel** : un PC avec écran tactile et stylet (la Surface de l'essai précédent) ; un vidéoprojecteur ou un second écran pour le dernier point. Pas de tablette Bamboo Slate ici. Ouvrir une note de la bibliothèque de démonstration (ou en créer une avec quelques traits) dans l'éditeur.

### A. Éditeur : stylet

1. Écrire quelques mots avec la pointe du stylet en appuyant plus ou moins fort : **le trait s'épaissit avec la pression** (pas d'épaisseur constante).
2. Retourner le stylet et passer le **dos** sur un trait, l'outil étant le **stylo** : le trait s'efface. Même chose avec l'outil Sélection, puis Main. Un petit cercle de gomme doit apparaître **dès que le dos du stylet survole la page**, avant même de toucher. Essayer la gomme « partielle » et « entière » : le dos du stylet suit le réglage de la gomme.
   - **Si rien ne s'efface** ou si le cercle n'apparaît qu'au contact : noter ce qui se passe (c'est le point le moins sûr : la valeur des boutons rapportée par WebView2 pour un stylet retourné).
3. Maintenir le **bouton du stylet** et entourer des traits : un **lasso de sélection** apparaît et les traits entourés sont sélectionnés, quel que soit l'outil. Aucun menu contextuel ne doit s'ouvrir sur la page. Avec la souris, le clic droit ne fait rien de plus qu'avant.
4. Poser la **paume** de la main sur l'écran en écrivant avec le stylet, et la poser aussi **avant** d'approcher le stylet : la paume ne doit ni déplacer la page, ni zoomer, ni dessiner tant que le stylet est sur l'écran ou près de lui (environ une seconde après son départ). Noter si la paume posée **avant** le stylet déclenche quelque chose.

### B. Éditeur : doigts

1. Un doigt posé sur la page et glissé : la page **défile** (aucun trait). Deux doigts qui s'écartent ou se rapprochent : **zoom** autour des doigts, la page suit en même temps le déplacement des deux doigts. Un **double toucher** rapide : la page entière est ajustée.
2. Les boutons **Ajuster** et **Largeur** (barre de zoom, en bas à droite) : « Largeur » met la page sur toute la largeur et montre son haut. Ctrl + 2 fait pareil.
3. Cocher **Doigt** dans la barre de zoom (ou Réglages > Stylet et tactile > Dessiner au doigt) : un doigt **dessine** comme la souris ; le stylet dessine toujours ; poser un deuxième doigt pendant qu'un trait est en cours **annule ce trait** et passe au zoom. Décocher : le doigt ne dessine plus.
4. **Relecture** (touche R) : un doigt fait défiler la page, deux doigts zooment, le double toucher ajuste ; le dos du stylet n'efface rien.

### C. Mode direct et présentation

1. En mode direct, aperçu de la page dans la fenêtre de commande : un doigt déplace le rectangle bleu, deux doigts le zooment, un double toucher montre la page entière ; **Largeur** (et la touche W) affiche la page sur toute la largeur de la zone projetée.
2. Projeter sur le second écran. Sur la **fenêtre de présentation** (écran tactile ou stylet), écrire avec le stylet, avec un doigt et avec la souris : des annotations **rouges** (jaunes sur le tableau noir) apparaissent par-dessus la page ; le dos du stylet les efface une à une ; le bouton **Effacer les annotations** (il apparaît avec le curseur) ou la touche **Suppr** les efface toutes ; elles disparaissent d'elles-mêmes au changement de page ; **rien n'est enregistré** dans la note (la rouvrir pour vérifier). Deux doigts zooment la zone projetée (la fenêtre de commande suit).
   - Si la présentation est sur un écran non tactile, vérifier au moins la souris et le stylet de l'ordinateur, et le comportement de la touche W.

### À me signaler

| Ce que je veux savoir | Test |
|---|---|
| Si le dos du stylet efface, et si le cercle de gomme apparaît au survol | A2 |
| Si le bouton du stylet fait bien un lasso (ou ce que fait Windows à la place) | A3 |
| Si la paume est ignorée, y compris posée avant le stylet | A4 |
| Si les gestes (défilement, zoom, double toucher) sont fluides et au bon endroit | B1 |
| Si le doigt dessine quand l'option est cochée | B3 |
| Si les annotations de la présentation s'effacent et ne s'enregistrent pas | C2 |

---

## H4 — Montrer la feuille sur un autre ordinateur du réseau

**Durée** : 30 à 40 minutes. **Matériel** : le PC d'Ardoise avec la tablette (chargée, dongle branché, téléphone Bluetooth **coupé**), un **second ordinateur sur le même réseau que lui** (l'idéal : le PC du bureau relié au vidéoprojecteur ; sinon n'importe quel PC) et un **téléphone ou une tablette** sur ce réseau, avec un appareil photo capable de filmer au ralenti (120 ou 240 images par seconde). Le réseau est celui de l'école, Wi-Fi ou prise réseau : noter lequel. Les parties A à E demandent la tablette (mode direct) ; la partie F n'en demande pas.

Ce test vérifie le partage de la feuille (SPEC §4.10) : l'autorisation du pare-feu de Windows, la connexion avec l'adresse, le QR code et le code d'accès, le délai perçu, la pause, la reconnexion après une coupure, le partage d'une note ouverte, l'arrêt, et le fait que le port se referme. Les libellés des boutons du panneau de partage sont ceux de la fenêtre ; ceux donnés ici sont indicatifs.

### À la maison, avant

1. Fermer Ardoise sur tous les PC, puis remplacer `ardoise.exe` dans le dossier Nextcloud par la nouvelle version.
2. Ne **pas** utiliser `--simulated-tablet`. Ne **pas** ajouter la variable `ARDOISE_SHARE_LOOPBACK` (elle sert aux tests automatiques et enferme le partage sur le PC lui-même).
3. Noter le port affiché dans **Réglages** > **Partage sur le réseau** > **Port du partage** (8765 par défaut). Si le panneau du partage annonce plus tard qu'il utilise un autre port (port pris par un autre programme), noter les deux.

### Au travail

#### A. Premier partage et pare-feu

1. Couper le Bluetooth du téléphone, brancher le dongle, lancer `ardoise.exe`. Ouvrir le **Mode direct** (panneau **Tablette**) et dessiner quelques traits.
2. Dans la fenêtre de commande, cliquer sur **Partager sur le réseau**. Le panneau doit montrer une **adresse** (`http://192.168.x.x:8765`), un **code QR**, un **code à 6 caractères** (par exemple `K7M Q2X`) et « 0 écran ».
   - Si le PC a plusieurs cartes réseau (Wi-Fi et Ethernet, VPN), **noter toutes les adresses listées** et laquelle est la première.
3. **Fenêtre du pare-feu Windows** : la première fois, Windows affiche « Alerte de sécurité Windows » (*le Pare-feu Windows Defender a bloqué certaines fonctionnalités de cette application*). **Prendre une photo ou une capture de cette fenêtre telle quelle**, puis noter exactement :
   - le nom et l'éditeur affichés, le chemin d'`ardoise.exe` ;
   - les cases proposées (**Réseaux privés**, **Réseaux publics**) et lesquelles sont cochées par défaut ;
   - si Windows demande un **mot de passe ou une confirmation d'administrateur** après **Autoriser l'accès**.
   - Cocher **Réseaux privés** seulement, puis cliquer sur **Autoriser l'accès**.
   - *Si aucune fenêtre n'apparaît* : le noter, c'est que le pare-feu est géré autrement (ou déjà ouvert). Passer à l'étape A4.
4. Noter le **profil du réseau** du PC d'Ardoise : **Paramètres** > **Réseau et Internet** > Wi-Fi (ou Ethernet) > propriétés du réseau > **Profil réseau** : *Public* ou *Privé*. S'il est *Public* et que la connexion de l'étape B échoue, passer le réseau en *Privé* (ou cocher aussi **Réseaux publics**) et le noter.

#### B. Se connecter depuis l'autre ordinateur

1. Sur le second ordinateur, ouvrir un navigateur (noter lequel et sa version) et taper l'adresse **en entier**, avec `http://` et le port. La page de saisie du code doit s'afficher en quelques secondes. Noter le temps qu'il a fallu.
   - *Si la page ne s'ouvre pas* : sur le second ordinateur, ouvrir PowerShell et lancer (en remplaçant l'adresse)
     ```
     Test-NetConnection 192.168.x.x -Port 8765
     ```
     `TcpTestSucceeded : True` veut dire que le réseau et le pare-feu laissent passer (le problème est alors dans le navigateur) ; `False` veut dire que le pare-feu du PC d'Ardoise ou le réseau bloque. Noter le résultat, puis revoir l'étape A3 / A4. Essayer ensuite les autres adresses du panneau.
1 bis. *Facultatif* : dans un autre onglet, taper `http://<nom du PC d'Ardoise>:8765` (le nom court, celui de **Paramètres** > **Système** > **Informations** > *Nom de l'appareil*). La page de saisie du code s'ouvre-t-elle ? (Ardoise accepte ce nom ; c'est le réseau qui doit le connaître.) Puis taper le même nom suivi d'un point et d'un mot (par exemple `http://<nom>.ecole:8765`) : si le navigateur trouve le PC, la page doit afficher seulement « Misdirected Request » (refus voulu). Noter les deux résultats.
2. **Mauvais code d'abord** : taper un code faux (par exemple `ZZZZZZ`). Un message doit dire que le code est incorrect et combien d'essais restent. Le noter.
3. **Bon code** : taper le code du panneau, en minuscules et sans espace. La feuille en direct s'affiche. Le panneau d'Ardoise passe à **1 écran**.
4. **Plein écran** : appuyer sur `F11`, puis essayer le bouton plein écran de la page (et le double-clic). Le bouton et le curseur disparaissent-ils après 2 secondes d'immobilité ?
5. **Téléphone ou tablette** : scanner le **code QR** du panneau. L'adresse s'ouvre dans le navigateur ; taper le code. Le panneau doit afficher **2 écrans**. Le noter (appareil, système, navigateur).
6. *Facultatif, une seule fois (5 minutes d'attente ensuite)* : sur le second ordinateur, dans une fenêtre de navigation privée, taper **5 codes faux** de suite, puis un sixième (même juste) : la page doit demander d'attendre et dire combien de temps. Pendant ce temps, le téléphone (autre appareil, navigation privée aussi) doit encore pouvoir se connecter avec le bon code.

#### C. La feuille en direct et le délai perçu

1. Écrire sur la tablette : le tracé apparaît à l'écran d'Ardoise **et** sur le second ordinateur.
2. **Délai perçu** : placer les deux écrans **côte à côte** (ou le second ordinateur à côté du portable) et écrire **une longue ligne, vite, d'un bout à l'autre de la page**. Noter, pour l'autre écran : *instantané* / *léger retard* / *retard gênant*. Objectif : moins de 150 ms de plus que l'écran d'Ardoise.
3. **Mesure avec le téléphone** : filmer **ensemble** l'écran d'Ardoise et l'autre écran au ralenti (240 images par seconde si possible, sinon 120), pendant qu'on trace un trait rapide. En relisant la vidéo image par image, compter les images entre le moment où le trait apparaît sur l'écran d'Ardoise et celui où il apparaît sur l'autre écran. À 240 images par seconde, 36 images font 150 ms ; à 120, 18 images. Noter l'écart en images et la cadence. Garder la vidéo si possible (sans élèves ni noms à l'écran).
4. **Tableau noir** : l'activer dans la fenêtre de commande : l'autre écran passe au fond ardoise et à la craie. Le désactiver.
5. **Pointeur laser** : l'activer, approcher le stylet de la tablette sans la toucher : le point rouge suit sur l'autre écran, sans rien écrire.
6. **Zoom et déplacement** : zoomer, déplacer la zone, revenir à la page entière : l'autre écran suit-il, au même cadrage que la présentation ?
7. **Pages** : page suivante, page précédente, nouvelle page, effacer la page : l'autre écran change-t-il tout de suite ?
8. Écrire sans interruption pendant 2 minutes : le tracé reste-t-il fluide sur l'autre écran, sans saccade ni retard qui grandit ?

#### D. Pause

1. Cliquer sur **Pause**. L'image de l'autre écran se **fige** (un repère discret le dit). Écrire encore sur la tablette : rien ne bouge sur l'autre écran, mais le tracé continue sur l'écran d'Ardoise.
2. Pendant la pause, **ouvrir la page de l'autre écran dans un nouvel onglet** (ou sur le téléphone) et se connecter : elle doit montrer l'image figée, pas la page en cours.
3. Cliquer sur **Reprendre** : l'autre écran montre aussitôt l'état actuel (y compris ce qui a été écrit pendant la pause) puis suit de nouveau.

#### E. Coupure du réseau

1. Sur le second ordinateur, **couper le réseau quelques secondes** (désactiver le Wi-Fi ou débrancher la prise, 5 à 10 secondes). La page doit afficher qu'elle se reconnecte (la feuille reste visible ou se grise : le noter).
2. Rétablir le réseau, sans toucher à la page : elle doit **se reconnecter toute seule**, sans redemander le code, et montrer l'état actuel. Noter le temps de reprise.
3. Recommencer en coupant le réseau **du PC d'Ardoise** (mettre le portable hors Wi-Fi). Écrire pendant la coupure, rétablir : les deux écrans sont-ils de nouveau d'accord, et le mode direct d'Ardoise n'a-t-il rien perdu ?

#### F. Une note ouverte dans l'éditeur (sans tablette)

1. Terminer le mode direct (**Terminer**), puis revenir à la bibliothèque : l'autre écran montre un **écran d'attente**.
2. Ouvrir une note de la bibliothèque : l'autre écran montre **la page de cette note**, avec son papier (lignes, quadrillage…) ou son image de fond. Comparer avec l'éditeur : mêmes traits, mêmes couleurs, même fond.
3. Dans l'éditeur, **tracer un trait** : il apparaît sur l'autre écran en moins d'une seconde. Changer la couleur de tous les traits, effacer un trait, annuler (`Ctrl + Z`) : l'autre écran suit-il, **sans enregistrer** la note ?
4. Changer de note : l'autre écran change. Revenir à la bibliothèque : écran d'attente. Vérifier que **le titre de la note et le nom du dossier n'apparaissent nulle part** sur l'autre écran.
4 bis. Rouvrir **la même note, sans la modifier** : l'autre écran doit la remontrer (et pas rester sur l'écran d'attente). Avec une note qui a une image ou un PDF de fond : effacer un trait, puis annuler ; noter si l'autre écran suit vite (l'image de fond n'est renvoyée que quand elle change).
5. Rouvrir le **Mode direct** (tablette connectée) : l'autre écran repasse à la feuille en direct.

#### G. Arrêt, fermeture et deuxième partage

1. Cliquer sur **Arrêter le partage** : l'autre écran indique que le partage est **terminé**. Le panneau d'Ardoise n'affiche plus d'adresse ni de code. Sur le second ordinateur, rafraîchir la page : elle doit ne plus répondre, ou demander un code ; la commande `Test-NetConnection` de l'étape B1 doit donner `False`.
2. Relancer un partage : un **nouveau code** s'affiche (différent du précédent), la même adresse et le même port. Le second ordinateur, resté sur la page, passe tout seul à la saisie du code (l'ancien code ne marche plus) : taper le nouveau.
3. **Fermer Ardoise pendant le partage** (croix de la fenêtre, après avoir terminé le mode direct). L'autre écran doit indiquer que le partage est terminé ou qu'il se reconnecte, et `Test-NetConnection` doit donner `False` : le port est refermé.
4. **Deuxième lancement** : relancer `ardoise.exe` (même emplacement) et partager de nouveau. **Windows ne doit pas redemander** l'autorisation du pare-feu : elle est attachée au programme à cet emplacement (pas au port) et reste valable. Le noter. (Si la fenêtre revient, noter si l'exe a changé de dossier ou de nom entre-temps.)
5. *Facultatif* : changer le **Port du partage** dans les Réglages (par exemple 8766), partager : Windows redemande-t-il l'autorisation ? (Attendu : non, l'autorisation suit le programme, quel que soit le port.) Le second ordinateur se connecte-t-il sur `:8766` ? Remettre ensuite 8765.
6. *Facultatif* : lancer sur le même PC un programme qui occupe déjà le port 8765 (ou lancer deux copies d'Ardoise placées dans deux dossiers différents et partager dans les deux) : la seconde doit prendre d'elle-même le port suivant et le dire dans le panneau (« Le port 8765 est utilisé par un autre programme… »), sans planter. (Ardoise prend le port pour lui seul pendant un partage : c'est aussi la seconde copie qui doit être refusée si on inverse l'ordre.)
7. *Facultatif* : fermer Ardoise, **copier `ardoise.exe` dans un autre dossier** (ou le renommer, par exemple `ardoise2.exe`), le lancer et partager : Windows **doit** reposer la question du pare-feu (attendu : oui, l'autorisation suit l'emplacement du programme). Le noter, puis supprimer la copie et revenir à l'exe habituel.
8. *Facultatif* : arrêter le partage puis le relancer **aussitôt** (dans les 5 secondes) : il doit redémarrer sur le même port, sans message « port utilisé ».

### Ce qui revient par Nextcloud

- Les notes écrites en mode direct pendant le test (bibliothèque).
- Dans `<bibliothèque>\_ardoise\diagnostics\` : un journal seulement en cas d'erreur de la tablette ou d'une session de mode direct anormale. Le partage lui-même n'écrit rien dans la bibliothèque.
- Dans `ardoise.toml` : la section `[share]` si le port a été changé.

### À me signaler

Ce qui ne revient pas tout seul :

| Quoi | Où |
|---|---|
| La **fenêtre du pare-feu** telle que Windows l'a montrée (photo) : cases proposées et cochées, droits d'administrateur demandés ou non ; si aucune fenêtre n'est apparue | A3 |
| Le **profil du réseau** (Public ou Privé) et le **type de réseau** (Wi-Fi ou prise réseau), le système et le navigateur du second ordinateur, ceux du téléphone ou de la tablette | A4, B1, B5 |
| Les adresses listées par le panneau, laquelle a marché, et le résultat de `Test-NetConnection` en cas d'échec ; si le nom court du PC a ouvert la page, et ce qu'a affiché le nom suivi d'un point | A2, B1, B1 bis |
| Le texte exact de toute erreur (dans Ardoise ou dans la page), et celui du message de code faux | partout, B2 |
| Le **délai perçu** (*instantané*, *léger retard*, *retard gênant*) et la mesure à la caméra (nombre d'images et cadence) ; si le tracé reste fluide pendant 2 minutes | C2, C3, C8 |
| Si le tableau noir, le pointeur laser, le zoom, le déplacement et les pages suivent sur l'autre écran | C4 à C7 |
| Si la pause fige bien l'image, y compris pour une page ouverte pendant la pause, et si la reprise remet à jour | D |
| Si plusieurs écrans passent par **une même adresse** (point d'accès Wi-Fi en mode routeur, proxy) : le partage accepte au plus 4 écrans et 8 connexions par adresse, et 8 ouvertures d'écran de suite par adresse puis une toutes les 2 secondes ; noter si un écran a été refusé (« trop d'écrans ») ou a redemandé le code | B, E |
| Le comportement après la coupure du réseau (page grisée ou non, reconnexion seule, temps de reprise, code redemandé ou non) | E |
| Si la note ouverte s'affiche comme dans l'éditeur (fond compris), le délai d'un nouveau trait, et si un titre ou un chemin apparaît quelque part | F |
| Ce que dit l'autre écran à l'arrêt du partage et à la fermeture d'Ardoise ; si le port est refermé | G1, G3 |
| Si Windows a redemandé l'autorisation au deuxième lancement, après un changement de port, après une copie ou un renommage de l'exe ; si le nouveau code est bien différent | G2, G4, G5, G7 |
| Si un partage relancé aussitôt a retrouvé son port | G8 |
| Le port utilisé s'il a été changé dans `ardoise.toml` | A, G5 |


---

## H5 — Reprendre une feuille en mode direct

**Durée** : 40 à 50 minutes. **Matériel** : le PC d'Ardoise avec la vraie tablette (chargée, dongle branché, téléphone Bluetooth **coupé**), le **vidéoprojecteur** de la salle en bureau étendu, un **second ordinateur sur le même réseau** (celui du partage, voir H4) et une **tablette ou un téléphone** pour filmer ou regarder la page partagée. Ce test suppose que H1 à H4 ont réussi.

Ce test vérifie la reprise d'une feuille (SPEC §4.2, `docs/questions.md` Q9) avec la vraie tablette : continuer à écrire dans une note existante après une interruption, ce que la tablette envoie quand la feuille n'a pas son format ni sa position, le fond de la feuille au vidéoprojecteur et sur l'autre écran, la page suivante, l'original, et une feuille qui change ailleurs. Les libellés ci-dessous sont ceux de la fenêtre ; les parties A à H demandent la tablette.

### À la maison, avant

1. Fermer Ardoise sur tous les PC, puis remplacer `ardoise.exe` dans le dossier Nextcloud par la nouvelle version.
2. Ne **pas** utiliser `--simulated-tablet`.
3. Préparer **trois notes** dans un classeur « Essai H5 » (**Créer une note**, **Depuis un PDF ou une image**) :
   - **« Fiche image »** : une note faite à partir d'une **image** (une photo de fiche, un schéma), en paysage, avec quelques traits écrits dans l'éditeur, puis **enregistrée**. Si possible, enregistrer aussi une **narration** de quelques secondes (**Relecture**, **Narration**) et laisser Ardoise reconnaître l'écriture.
   - **« Fiche portrait »** : une note **en portrait** (A4 portrait, papier ligné), avec quelques traits.
   - **« Fiche tablette »** : une note venue de la tablette par la synchronisation (n'y toucher ni dans l'éditeur ni ailleurs).
4. Noter pour chacune le **nombre de traits** (ouvrir la note dans l'éditeur) et copier le fichier `.ardoise` de « Fiche image » ailleurs (hors de la bibliothèque), pour comparer ensuite.

### Au travail

#### A. Interrompre, puis reprendre depuis la bibliothèque

1. Lancer `ardoise.exe`, ouvrir le **Mode direct**, écrire quelques mots, puis **Terminer** : les notes sont enregistrées (le compteur le dit). Noter le nom de la dernière.
2. Dans la bibliothèque, ouvrir le menu **⋯** de la carte de « Fiche image » et choisir **Reprendre en mode direct**. La fenêtre du mode direct doit s'ouvrir **sur cette feuille** : ses traits et son **image de fond** sont visibles dans l'aperçu, à leur place. Noter si l'état passe à **En direct** tout seul, ou s'il faut appuyer sur le bouton de la tablette.
3. Écrire sur la tablette un mot et un trait qui **traverse** la page. Noter si le tracé apparaît **au bon endroit** de la feuille (le coin en haut à gauche de la tablette est-il en haut à gauche de la feuille ?), s'il est de la bonne taille et sans déformation, et la latence affichée.
4. **Terminer**. Dans la bibliothèque, vérifier qu'il n'y a **toujours qu'une** carte « Fiche image » (pas de copie) et qu'elle montre les nouveaux traits. L'ouvrir dans l'éditeur : les traits d'avant sont **inchangés**, l'**image de fond** est là, la **narration** se rejoue, **Revenir à l'original** est proposé. Comparer le fichier avec la copie de l'étape « À la maison » (même nom, même image).
5. Reprendre « Fiche image » **sans rien écrire** (ne pas toucher la tablette, **Terminer** aussitôt) : le fichier ne doit pas avoir changé (même date de modification dans l'Explorateur).

#### B. Reprendre depuis l'éditeur

1. Ouvrir « Fiche portrait » dans l'éditeur, tracer un trait (sans enregistrer), cliquer sur **Reprendre en mode direct** dans la barre d'outils. Une boîte propose **Enregistrer et reprendre le direct**, **Reprendre sans enregistrer**, **Rester** : choisir la première. La feuille s'ouvre en direct **avec le trait tracé**.
2. Écrire un mot sur la tablette. **Terminer**, rouvrir la note : le trait tracé dans l'éditeur et le mot de la tablette y sont.

#### C. Format et position de la tablette

La tablette est tenue **en paysage** (réglage « Paysage, bouton en bas »).

1. Reprendre « Fiche portrait » (feuille en portrait). Un **avis** doit dire comment tenir la tablette (par exemple « Pour écrire sur cette feuille, tenez la tablette ainsi : Portrait, bouton à gauche »). Les **quatre boutons de position** de la fenêtre de commande sont **grisés**. Noter le texte exact de l'avis.
2. Tenir la tablette **comme l'avis le dit** et écrire une ligne de haut en bas puis de gauche à droite : le tracé suit-il le sens de la main ? La feuille reste-t-elle **dans le même sens** (elle n'est pas tournée) ? Noter si l'écriture est de la bonne taille et centrée sur la feuille, avec ses bords (la tablette n'a pas le format de la feuille : une marge peut rester vide).
3. **Tenir la tablette autrement** que l'avis (l'autre sens) et écrire : noter ce qui apparaît (le tracé est tourné, mais la feuille ne l'est pas ; est-ce clair pour l'utilisateur ?).
4. Cliquer sur **Page suivante** : une nouvelle page s'ouvre, **les boutons de position fonctionnent de nouveau** et suivent le réglage de la tablette. Écrire un mot, **Terminer** : la nouvelle page est une **note neuve** à la racine de la bibliothèque, « Fiche portrait » n'a pas changé de nom ni d'emplacement.
5. Recommencer avec « Fiche image » (paysage) et la tablette réglée en **portrait** (panneau **Tablette** > **Position**) : l'avis demande de tenir la tablette en paysage. Remettre ensuite le réglage habituel.

#### D. Le fond au vidéoprojecteur et sur l'autre écran

1. Reprendre « Fiche image ». **Projeter** sur le vidéoprojecteur (section **Projection**) : l'**image de fond** doit être visible à l'écran projeté, **au même endroit** que dans la fenêtre de commande, avec les traits. Noter si elle est nette à la taille du projecteur et si le zoom (molette, **Largeur**) la garde alignée avec l'écriture.
2. **Partager sur le réseau** (voir H4) et se connecter depuis le second ordinateur : la page montre l'image de fond et les traits. Écrire : les nouveaux traits arrivent sur les deux écrans. Noter le délai perçu.
3. Activer le **Tableau noir** puis le désactiver : le fond de la feuille doit **revenir** (papier et image). Noter ce qu'on voit pendant le tableau noir.
4. Si « Fiche image » a un calque masqué : il ne doit apparaître **ni au projecteur ni sur l'autre écran**.
5. Arrêter le partage et la projection, **Terminer**.

#### E. L'original après la reprise

1. Ouvrir « Fiche tablette » dans l'éditeur, noter ses traits, puis la reprendre en direct, écrire un mot, **Terminer**.
2. La rouvrir et cliquer sur **Revenir à l'original** : l'encre doit redevenir **la page telle qu'elle a été synchronisée** (sans le mot écrit en direct). Ne pas enregistrer (**Ctrl + Z**).
3. Faire de même avec une note **écrite dans Ardoise puis déjà modifiée** (par exemple « Fiche image » après A) : **Revenir à l'original** rend la **première version** de la note, pas l'état d'avant la dernière reprise. Noter ce qui revient.

#### F. La feuille change ailleurs pendant le direct

1. Sur le PC d'Ardoise, reprendre « Fiche portrait » ; garder le mode direct ouvert.
2. Depuis **un second PC**, ouvrir la même note (même bibliothèque Nextcloud), tracer un trait et enregistrer. Attendre que Nextcloud ait synchronisé le fichier sur le PC d'Ardoise.
3. Écrire quelques traits sur la tablette et attendre une dizaine de secondes. Un **avis** doit apparaître (« Cette feuille a été modifiée ailleurs pendant le direct… »). **Terminer**.
4. Dans la bibliothèque, il y a **deux notes** dans le même classeur : celle du second PC, **intacte** (avec son trait, pas celui de la tablette), et une nouvelle avec l'image, l'original et l'écriture de la tablette. Noter le titre et le nom du fichier de la nouvelle note.
5. *Facultatif* : même chose en **supprimant** la note depuis le second PC : l'avis dit qu'elle a été supprimée, l'écriture continue dans une nouvelle note.

#### G. Coupure et reprise

1. Reprendre une feuille, écrire, puis **éteindre la tablette** (ou la sortir de la portée) quelques secondes : l'état passe à **Reconnexion…**, puis le tracé reprend sur la même feuille. Noter la durée.
2. Reprendre une feuille avec une **narration** ancienne, enregistrer une narration pendant le direct, **Terminer**, rouvrir et lancer la **Relecture** : l'ancienne et la nouvelle narration sont audibles, au bon moment du tracé (la nouvelle après l'ancienne).

#### H. Retouches pendant le direct

La tablette est tenue comme d'habitude, le **vidéoprojecteur** et le **second ordinateur** (partage, voir H4) montrent la page. Ouvrir « Fiche image » avec **Reprendre en mode direct**, puis écrire sur la tablette quelques mots bien séparés (une dizaine de traits) pour avoir de quoi retoucher.

1. **Choisir** : cliquer sur **Sélection** (ou touche `V`). Avec la souris, **entourer** deux ou trois traits dans l'aperçu : ils sont cernés de bleu, l'encre gardant sa couleur, et la barre dit « Traits sélectionnés : 2 » (ou 3). **Écrire sur la tablette pendant ce temps** : le nouveau trait apparaît, sans être choisi. **Regarder le vidéoprojecteur et l'autre écran** : on n'y voit **ni contour bleu, ni lasso, ni cadre**, seulement la page. Noter si le halo bleu se voit bien à la distance de la souris, et s'il gêne la lecture de l'aperçu.
2. **Un clic** sur un autre trait le choisit seul ; `Maj` + clic en ajoute un, `Ctrl` + clic le retire ; `Échap` enlève la sélection. Avec le **stylet de l'ordinateur** (ou le doigt, si l'écran est tactile) : le stylet entoure comme la souris ; un doigt déplace l'aperçu sans choisir. Noter si cliquer pile sur un trait fin est facile.
3. **Recolorer** : choisir deux traits, cliquer sur une pastille (Rouge). Les deux traits changent de couleur **au vidéoprojecteur et sur l'autre écran** en moins d'une seconde, et dans l'aperçu aussi, à l'intérieur du contour bleu, sans appuyer sur `Échap`. **La couleur du stylo n'a pas changé** : écrire un mot sur la tablette, il est noir (la pastille noire reste marquée). Noter le délai ressenti sur les deux écrans.
4. **Supprimer** : choisir deux traits, cliquer sur **Supprimer** ; recommencer avec la touche `Suppr`. Ils disparaissent des **trois écrans**, et la tablette peut écrire à tout moment sans que rien ne se perde. Compter les traits de l'aperçu avec ceux de l'autre écran (ils doivent être les mêmes).
5. **Annuler** : `Ctrl + Z` rend les traits supprimés **à leur place**, `Ctrl + Y` les enlève de nouveau. **Écrire un mot sur la tablette entre un `Ctrl + Z` et le suivant** : le mot reste, il n'est jamais annulé. Appuyer sur `Ctrl + Z` jusqu'au bout : les boutons **Annuler** se grisent, et les mots écrits sont toujours là.
6. **Effacer la page** puis `Ctrl + Z` : la page revient avec ce qui a été écrit après l'effacement.
7. **Sur la feuille reprise**, retoucher un trait **d'avant la reprise** (supprimer l'un, recolorer l'autre), puis **Terminer** : dans l'éditeur, les deux retouches y sont, l'**image de fond** aussi, et **Revenir à l'original** rend la première version de la feuille (sans les retouches).
8. **Pendant que la tablette écrit vite** (un mot long, sans lever le stylet) : retoucher à la souris au même moment. Noter si l'écriture reste fluide au vidéoprojecteur, sans à-coup ni trait qui saute, et si la latence affichée reste proche de ce qu'elle était.
9. **Quitter la page puis y revenir** (Page suivante, Page précédente), puis `Ctrl + Z` : la retouche faite sur cette page est annulée (une page garde son historique) ; sur la page suivante, rien à annuler.
10. **Déplacer et redimensionner, au stylet et au doigt** (sur la **Surface, clavier détaché** : ni souris ni clavier, c'est le cas de l'enseignant en classe). Avec le stylet, **entourer** deux ou trois traits : le cadre en pointillés porte **quatre petits carrés** aux coins.
    - **Stylet, déplacer** : poser la pointe **dans le cadre**, puis une autre fois **juste à côté de son bord, à deux ou trois millimètres** (le stylet « accroche » le cadre à cette distance ; un doigt, à quatre ou cinq millimètres), et glisser : les traits suivent la pointe, **soulevés**, avec leur halo, et l'endroit qu'ils quittent se vide. **Le vidéoprojecteur et l'autre écran ne bougent pas pendant le glissement**, puis montrent les traits à leur nouvelle place **d'un coup au relâchement**, sans cadre ni carrés. La sélection reste.
    - **Stylet, redimensionner** : prendre un **coin** sans viser pile (sur le carré, un peu dedans, ou un peu **dehors, à deux ou trois millimètres** en diagonale ; pour un doigt, jusqu'à quatre ou cinq millimètres), glisser vers le coin opposé puis en s'éloignant : les traits rétrécissent puis grandissent autour du coin opposé, **l'épaisseur du trait avec eux**. Noter s'ils se voient flous pendant le glissement (c'est une image agrandie : le trait redevient net au relâchement).
    - **Doigt** : poser un doigt **dans le cadre** et glisser : les traits se déplacent et **l'aperçu ne bouge pas** ; un doigt **hors du cadre** déplace toujours l'aperçu ; la même chose avec un coin. Poser un **deuxième doigt** pendant le glissement : le geste est abandonné (les traits restent où ils étaient) et les deux doigts zooment. Un simple appui dans le cadre ne change rien.
    - `Échap` (clavier tactile ou clavier branché) pendant un glissement : les traits **reviennent à leur place** et rien n'est envoyé au vidéoprojecteur.
    - `Ctrl + Z` (clavier tactile ou bouton **Annuler**) remet les traits **à leur place en un pas par déplacement ou redimensionnement**, `Ctrl + Y` les y remet ; vérifier le **vidéoprojecteur**, l'**autre écran**, puis **Terminer** : la note contient les traits tels qu'ils étaient à l'écran.
    - Noter si la prise est **facile** (le stylet « accroche » le cadre sans viser, un doigt n'attrape pas le cadre quand on veut déplacer l'aperçu), si l'aperçu suit **sans à-coup** (aucun trait qui disparaît puis réapparaît au relâchement) et si la tablette peut écrire pendant tout cela sans rien perdre.

### Ce qui revient par Nextcloud

- Les notes de l'essai (« Essai H5 ») et les notes écrites en direct pendant le test.
- Dans `<bibliothèque>\_ardoise\diagnostics\` : un journal seulement en cas d'erreur de la tablette ou d'une session de mode direct anormale.

### À me signaler

Ce qui ne revient pas tout seul :

| Quoi | Où |
|---|---|
| Si le tracé arrive **au bon endroit** de la feuille reprise, à la bonne taille et sans déformation, quand la feuille n'a pas le format de la tablette | A3, C2 |
| Le **texte exact de l'avis** sur la position de la tablette, si la position conseillée est la bonne pour la main, et ce qui se passe quand on tient la tablette dans l'autre sens | C1, C3, C5 |
| Si une reprise sans écriture a laissé le fichier intact (date de modification) | A5 |
| Si les traits d'avant, l'image de fond, la narration, l'original sont intacts après la reprise | A4 |
| Si l'image de fond est nette et bien alignée avec les traits au vidéoprojecteur et sur l'autre écran, aussi en zoom, et ce que fait le tableau noir | D1 à D3 |
| Ce que rend **Revenir à l'original** pour une note de la tablette, et pour une note déjà modifiée | E |
| Le texte de l'avis « modifiée ailleurs » ou « supprimée ailleurs », les deux notes obtenues (titres, noms de fichiers) et si Nextcloud a créé une copie en conflit en plus | F |
| La reprise après une coupure de la tablette, et la relecture avec deux narrations | G |
| Si le contour bleu de la sélection **n'apparaît jamais** au vidéoprojecteur ni sur l'autre écran, et le délai d'une recoloration ou d'une suppression sur les deux | H1, H3, H4 |
| Si l'écriture reste **fluide** pendant qu'on retouche, la latence affichée, et si choisir un trait fin à la souris ou au stylet de l'ordinateur est facile | H2, H8 |
| Si la couleur du stylo est restée la même après une recoloration, et si les traits écrits entre deux `Ctrl + Z` sont tous restés | H3, H5 |
| Si le cadre et les coins se prennent **facilement** au stylet et au doigt sur la Surface, sans souris, si l'aperçu suit sans à-coup, et si le vidéoprojecteur ne montre le résultat qu'au relâchement | H10 |
| Ce que contient la feuille reprise après **Terminer** (retouches, image, original) | H7 |

---

## H6 — Corriger pendant le direct, sur son propre calque

**Durée** : 50 à 60 minutes. **Matériel** : le PC d'Ardoise, **la Surface avec son stylet** (c'est elle qui sert à corriger), la vraie tablette (chargée, dongle branché, téléphone Bluetooth **coupé**), le **vidéoprojecteur** de la salle en bureau étendu, le **PC du vidéoprojecteur sur le même réseau** (celui du partage, voir H4), **un élève ou un collègue** qui écrit sur la tablette pendant que vous corrigez, et un **lecteur PDF à calques** (Acrobat Reader ou Firefox, voire les deux) sur un PC. Ce test suppose que H1 à H5 ont réussi.

Ce test vérifie la correction en classe (SPEC §4.2 « Corriger pendant le direct », §4.4 « Calques », §4.6 « Calques à l'export », `docs/questions.md` Q11) avec la vraie tablette et le vrai stylet : écrire en rouge sur un calque à soi pendant que l'élève écrit, gommer, annuler, envoyer la tablette sur un autre calque, les avertissements d'un calque masqué, puis l'export PDF à calques séparés. Les libellés ci-dessous sont ceux de la fenêtre ; les parties A à F demandent la tablette.

### À la maison, avant

1. Fermer Ardoise sur tous les PC, puis remplacer `ardoise.exe` dans le dossier Nextcloud par la nouvelle version.
2. Ne **pas** utiliser `--simulated-tablet`.
3. Préparer le matériel : le stylet de la Surface chargé (ou avec une pile neuve), le PC du vidéoprojecteur prêt à recevoir le partage (H4).
4. Noter la version d'Ardoise et celle de Windows. Dans le panneau **Tablette**, noter le réglage de **Bouton de la tablette en mode direct** (il sert en partie D).

### Au travail

#### A. Installer la correction

La tablette est tenue comme d'habitude, l'élève (ou le collègue) a une consigne à écrire (un schéma de circuit, trois lignes d'exercice). **Projeter** sur le vidéoprojecteur et **Partager sur le réseau** vers le PC du vidéoprojecteur (voir H4).

1. Ouvrir le **Mode direct**. L'élève écrit les premières lignes. Dans la zone **Calques** (en haut du panneau de côté), vérifier qu'il n'y a qu'**un calque**, **encadré** (choisi), avec l'**icône de tablette** : l'ordinateur et la tablette sont sur le même.
2. Cliquer sur **Ajouter un calque** : un calque apparaît **en haut** et il est **encadré** ; l'icône de tablette reste sur le calque de l'élève. L'élève continue d'écrire : son écriture n'a pas changé de calque.
3. Cliquer sur le **crayon** à côté du nouveau calque, taper « Correction », `Entrée`. Essayer aussi `Échap` dans le champ, sur un autre renommage : l'ancien nom revient.
4. Cliquer sur **Stylo** (groupe **Ordinateur**) ou appuyer sur `P`. Le bouton de l'encre de l'ordinateur doit montrer une **pastille rouge** et « Feutre ». L'ouvrir (**Encre de l'ordinateur**) : **Outil**, **Couleur** et **Épaisseur** sont réglables ; `Échap` le ferme. L'encre de la tablette (groupe **Tablette**) n'a pas bougé.

#### B. Écrire en rouge pendant que l'élève écrit

1. Avec le **stylet de la Surface**, écrire sur l'aperçu un mot et souligner une erreur de l'élève **pendant que l'élève écrit** sur la tablette. Noter :
   - si le trait rouge apparaît **sous la pointe** du stylet, sans retard ressenti, et la **latence** affichée ;
   - si l'écriture de l'élève **continue sans à-coup** au vidéoprojecteur pendant que l'on écrit (à regarder : un mot long de l'élève, sans lever la tablette, pendant que l'on écrit un mot) ;
   - si les deux écritures s'affichent **ensemble**, sans que l'une efface l'autre, au vidéoprojecteur et sur l'écran du partage, et le délai ressenti sur les deux (filmer l'écran du partage au ralenti si besoin).
2. Poser la **paume** sur l'écran en écrivant au stylet, comme on le fait vraiment : noter si elle laisse des traces ou fait bouger l'aperçu. Noter aussi si **Windows Ink** montre quelque chose par-dessus la page (un tracé de stylet, un cercle d'appui long, une barre, un menu du stylet) : prendre une capture ou une photo.
3. Avec la **souris** (l'outil **Stylo** toujours actif) : écrire un mot, il apparaît en rouge aussi. Avec un **doigt** : l'aperçu se déplace, sans encre.
4. Changer l'encre de l'ordinateur (une autre couleur, plus épais) : le trait suivant en tient compte, pas les précédents ; le style de la tablette ne bouge pas. Remettre le rouge.

#### C. Gommer et annuler

1. Avec le **bout gomme du stylet**, passer sur un mot rouge de « Correction » : le **trait entier** disparaît, et un **petit cercle** suit la gomme dans l'aperçu (il n'est pas projeté). Noter si le bout gomme est reconnu du premier coup, et si le cercle a la bonne taille.
2. Passer le bout gomme sur un mot de l'**élève** : **rien** ne doit disparaître (il est sur un autre calque).
3. `Ctrl + Z` : le trait gommé revient. `Ctrl + Z` encore : le trait de l'ordinateur précédent disparaît. `Ctrl + Y` le rend. **L'élève écrit entre deux `Ctrl + Z`** : son écriture reste, elle n'est jamais annulée. Noter si le trait de l'élève et ceux de la correction restent à leur place après chaque appui.
4. Écrire un trait pendant que l'élève écrit, puis `Ctrl + Z` **au moment même** où l'on lève le stylet : le trait doit disparaître en entier (pas de demi-trait).
5. **Effacer la page** puis `Ctrl + Z` : tout revient (les deux écritures).

#### D. Choisir un autre calque, envoyer la tablette ailleurs

1. Cliquer sur la ligne du calque de l'**élève** : le cadre y passe. Avec le **bout gomme**, gommer un de ses traits : seuls les traits de ce calque partent (ceux de « Correction » restent). `Ctrl + Z` le rend.
2. Cliquer sur **Sélection** (ou `V`) et entourer des traits de l'élève et des traits rouges : **seuls les traits du calque choisi** sont pris. Choisir un autre calque : la sélection s'en va. **Stylo** et **Sélection** s'excluent.
3. **Envoyer la tablette sur un autre calque** : amener le stylet (puis la souris) sur la ligne de « Correction » **sans cliquer** : une **icône de tablette pâle** doit apparaître. Noter si elle apparaît **au survol du stylet** (le stylet de la Surface survole-t-il ?) ou seulement à la souris, et si elle est toujours visible au doigt. Cliquer dessus : l'icône de tablette passe sur cette ligne. L'élève écrit un mot : il se retrouve **sur ce calque** (le vérifier en masquant puis en affichant le calque).
4. Remettre la tablette sur le calque de l'élève de la même manière.
5. Si le **bouton de la tablette** est réglé sur **Nouveau calque** (panneau **Tablette**, **Bouton de la tablette en mode direct**) : appuyer dessus. Un calque apparaît **en haut**, l'icône de tablette s'y place, et **l'ordinateur reste où il est** (le cadre n'a pas bougé). Noter ce que la Bamboo Slate envoie, et si l'élève écrit bien sur le nouveau calque. Remettre ensuite le réglage de départ.

#### E. Masquer, avertissements, fusion

1. Cliquer sur l'**œil** du calque de l'**élève** : il devient **barré**, le nom passe en gris italique, et l'écriture de l'élève **disparaît du vidéoprojecteur et de l'écran du partage**. Comme la tablette y écrit encore, l'avertissement « La tablette écrit sur « … », qui est masqué : ce que l'élève écrit n'est ni projeté ni partagé. » doit s'afficher sous la liste. L'élève écrit un mot : personne ne le voit. Réafficher le calque : tout revient, **mot compris**.
2. Choisir un calque **masqué** (œil barré) puis, avec **Stylo**, essayer d'écrire : rien n'est écrit, et le message « Le calque choisi, « … », est masqué : affichez-le pour y écrire. » s'affiche (sous la liste et dans la fenêtre). Réafficher le calque.
3. Monter puis descendre « Correction » (**↑**, **↓**) : l'ordre change au vidéoprojecteur. Ces boutons ne sont que sur la ligne **choisie**.
4. **Fusionner** : ajouter deux calques d'essai (pas « Correction », pour ne pas la perdre), écrire quelques traits sur chacun, puis **⤓** sur le plus haut. La question « Fusionner « … » avec « … » ? » s'affiche, avec « Ctrl+Z ne pourra pas l'annuler. » : répondre **Annuler** (ou `Échap`) : rien ne change ; recommencer et répondre **Fusionner**. Appuyer sur `Ctrl + Z` : **rien** ne revient. Les traits sont tous là, sur le calque du dessous. Refaire l'essai avec le calque du dessous **masqué** : la question dit en plus « « … » est masqué : une fois fusionné, son contenu sera projeté et partagé. », et après **Fusionner** son encre apparaît au vidéoprojecteur.
5. Ajouter un calque **pendant que l'élève écrit un mot long** : l'écriture n'est pas coupée en deux traits.

#### F. L'élève écrit sur plusieurs pages

1. **Page suivante** : la page neuve a **un calque**, l'ordinateur et la tablette dessus. Revenir à la page précédente : ses deux calques (celui de la tablette et celui de l'ordinateur) sont **là où on les avait laissés**.
2. Sur la page neuve, écrire en rouge sur un calque de correction, puis `Ctrl + Z` : seule la page courante est concernée.

#### G. Après le direct : la note, la relecture, l'export

1. **Terminer**. Ouvrir la note dans l'éditeur : la zone **Calques** (en bas du panneau de droite) liste les calques créés (« Correction »…), dans l'ordre ; **cliquer sur une ligne** la choisit (cadre). Avec le **lasso** et `Ctrl + A`, **seuls les traits du calque choisi** sont pris ; la **gomme** n'efface que sur lui. Masquer le calque choisi : un avertissement s'affiche, et le calque reste choisi.
2. Lancer la **Relecture** : les traits rouges de la correction apparaissent **à l'heure où ils ont été écrits**, mêlés à ceux de l'élève. Noter si l'ordre est juste.
3. **Exporter…** en **PDF** : le choix **Calques** (**Séparés** / **Fusionnés**) est proposé, **Fusionnés** coché au départ. Choisir **Séparés** et enregistrer. Faire de même en **SVG** (**Séparés** doit être coché au départ). Rouvrir la fenêtre : le choix est **retenu**, pour chaque format. Pour **PNG**, la phrase « Une image n'a pas de calques : ils sont toujours fusionnés. » apparaît ; avec un calque masqué qui porte de l'encre, la phrase sur les calques masqués apparaît aussi.
4. Ouvrir le **PDF séparé** dans **Acrobat Reader** (panneau **Calques**) puis dans **Firefox** (le panneau des calques de la visionneuse) : on doit pouvoir **afficher et masquer** « Correction » et le calque de l'élève, un interrupteur par nom. Noter pour chaque lecteur : si le panneau s'ouvre tout seul, si les calques sont dans l'ordre (le plus haut en premier), si la page se redessine bien, et ce que montre un autre lecteur (Edge, Aperçu de l'Explorateur, un lecteur de téléphone) : tout, sans choix ?
5. Faire un **PDF de deux notes** qui ont chacune un calque « Correction » : le panneau ne doit montrer **qu'un** « Correction » (un interrupteur pour les deux pages). Le **PDF fusionné** n'a aucun panneau de calques.
6. Si **Inkscape** est installé : ouvrir le **SVG séparé** (les calques dans le panneau **Calques**) et le **SVG fusionné** (un seul calque d'encre).
7. **Un calque masqué n'est jamais exporté** : masquer le calque de l'élève dans l'éditeur, exporter le PDF **séparé** puis **fusionné** : ni son encre ni son nom (à chercher dans le panneau des calques du lecteur) n'y sont.
8. Quitter Ardoise et le relancer : l'encre de l'ordinateur (couleur, outil, épaisseur choisis en B4) est **retenue** au prochain direct.

### Ce qui revient par Nextcloud

- Les notes écrites pendant le test, et les exports que l'on a faits dans la bibliothèque.
- Dans `<bibliothèque>\_ardoise\diagnostics\` : un journal seulement en cas d'erreur de la tablette ou d'une session de mode direct anormale.
- Les captures ou photos de Windows Ink et de l'icône de tablette sont à déposer à la main avec le compte rendu.

### À me signaler

Ce qui ne revient pas tout seul :

| Quoi | Où |
|---|---|
| Si le trait rouge du stylet apparaît sous la pointe sans retard, la latence affichée, et si l'écriture de l'élève reste **fluide** au vidéoprojecteur pendant que l'on écrit | B1 |
| Si les deux écritures s'affichent ensemble au vidéoprojecteur et sur l'écran du partage, et le délai ressenti | B1 |
| Le comportement du **stylet** : bout gomme reconnu du premier coup, taille du cercle de la gomme, boutons du stylet qui font quelque chose d'inattendu | B, C1 |
| Si la **paume** laisse des traces ou fait bouger l'aperçu | B2 |
| Si **Windows Ink** montre quelque chose par-dessus la page (tracé, cercle d'appui long, barre, menu) | B2 |
| Si l'**icône de tablette** pâle apparaît au survol du stylet, ou seulement à la souris, et si elle est visible au doigt | D3 |
| Ce que la **Bamboo Slate** envoie quand son bouton est réglé sur **Nouveau calque**, et si l'élève écrit bien sur le nouveau calque | D5 |
| Si `Ctrl + Z` n'a **jamais** annulé l'écriture de l'élève ni coupé un trait en deux | C3, C4 |
| Si l'écriture masquée de l'élève revient entière à l'affichage, et le texte exact des deux avertissements | E1, E2 |
| Si fusionner ou ajouter un calque pendant le direct a gêné l'élève (écriture coupée, retard) | E4, E5 |
| Si les calques des pages sont retrouvés là où on les avait laissés | F |
| Si les traits de la correction sont à leur heure à la relecture | G2 |
| Si le PDF séparé s'ouvre avec des calques à afficher et masquer dans **Acrobat Reader** et **Firefox** (panneau ouvert tout seul, ordre, un interrupteur par nom), et ce que montrent les autres lecteurs | G4, G5 |
| Si le SVG séparé et le SVG fusionné s'ouvrent comme prévu dans **Inkscape** | G6 |
| Si un calque masqué est bien absent des fichiers exportés | G7 |
| Si l'encre de l'ordinateur est retenue d'une séance à l'autre | G8 |

---

## H7 — Papier et fond en direct, projection de l'éditeur, son pendant qu'on écrit

**Durée** : 50 à 60 minutes. **Matériel** : le PC d'Ardoise (**la Surface et son stylet**), la vraie tablette (chargée, dongle branché, téléphone Bluetooth **coupé**), le **vidéoprojecteur** de la salle en bureau étendu, le **PC du vidéoprojecteur sur le même réseau** (celui du partage, voir H4), **un élève ou un collègue** qui écrit sur la tablette, le **micro de la Surface**, et sur la Surface une **fiche en PDF de deux pages** et une **photo**. Ce test suppose que H1 à H6 ont réussi.

Ce test vérifie SPEC §4.2 « Papier et fond pendant le direct », §4.4 « Projeter la note de l'éditeur » et §4.5 « Narration au micro » (« pendant qu'on écrit dans l'éditeur ») (`docs/questions.md` Q13) avec la vraie tablette, le vrai vidéoprojecteur et le vrai micro : changer le papier et poser une fiche sous la page pendant que l'élève écrit, projeter une note de l'éditeur et y écrire, enregistrer le son en écrivant puis relire et exporter la vidéo. Les libellés ci-dessous sont ceux de la fenêtre ; les parties A à C demandent la tablette, D et E non.

### À la maison, avant

1. Fermer Ardoise sur tous les PC, puis remplacer `ardoise.exe` dans le dossier Nextcloud par la nouvelle version.
2. Ne **pas** utiliser `--simulated-tablet` ni `--simulated-microphone`.
3. Copier la fiche PDF (deux pages) et la photo dans un dossier de la Surface (pas dans la bibliothèque).
4. Noter la version d'Ardoise et celle de Windows. Vérifier que Windows laisse Ardoise utiliser le micro (*Paramètres > Confidentialité et sécurité > Microphone*).

### Au travail

#### A. Le papier en direct

1. Ouvrir le **Mode direct**, **Projeter** (onglet **Projection**), **Partager sur le réseau** vers le PC du vidéoprojecteur. L'élève écrit.
2. Onglet **Page** : **Crème**, modèle **Seyès**, **Texture du papier**. Noter si le vidéoprojecteur et l'écran du partage changent, en combien de temps, et si l'écriture de l'élève continue **sans à-coup** pendant le changement (regarder un mot long, sans lever la tablette).
3. Appuyer sur `B` (tableau noir) : les pastilles de couleur sont **grisées** et une phrase le dit ; choisir le modèle **Ligné** : il apparaît sur l'ardoise. `B` à nouveau : le crème revient, lignée. Noter le texte exact de la phrase.
4. Choisir **Ardoise noire** tableau éteint : le papier devient noir, une phrase propose le tableau noir ; l'écriture de l'élève est noire sur noir (c'est voulu). Remettre **Crème**. Noter le texte exact de la phrase.
5. Faire passer la page avec le **bouton de la tablette** (réglé sur « nouvelle page ») ou **Page suivante** : la nouvelle page est crème lignée, avec la texture. Revenir en arrière : la page d'avant a gardé le sien. Essayer `Ctrl + Z` : le papier ne change pas (c'est voulu).

#### B. Une fiche sous la page

1. Onglet **Fond** : **Choisir un PDF ou une image…**, la fiche, **page 2**, **Mettre en fond**. Noter le temps avant qu'elle s'affiche au vidéoprojecteur et sur le partage, et si l'élève peut écrire pendant ce temps.
2. L'élève complète la fiche : l'écriture tombe-t-elle **au bon endroit** sur la fiche, au vidéoprojecteur et sur le partage, aussi en agrandissant la zone projetée ? Essayer **Opacité** 50 %, **Pleine page**, **Entière**, **Tourner l'image seule**.
3. Cliquer dans la liste **Modèle** (onglet **Page**), puis appuyer sur la **télécommande** (Page suivante) : la page doit tourner, le modèle ne doit pas changer. Même essai après un choix de **Placement** et un réglage de l'**Opacité** à la souris.
4. Page suivante : pas de fiche, le papier est gardé. Page précédente : la fiche est là. Mettre la photo sur une autre page.
5. Tenir la tablette dans l'autre sens **avant** d'écrire sur une page qui a la fiche : la page suit-elle la tablette, la fiche est-elle replacée ? Écrire un mot, puis essayer de tourner la tablette : la page ne doit plus tourner, les boutons de position sont grisés, une phrase dit comment tenir la tablette. **Retirer l'image** : la page se tourne-t-elle avec la tablette, écriture comprise ? Noter le texte exact de la phrase.
6. **Terminer.** Dans la bibliothèque, ouvrir les notes : la fiche sous la première, la photo sous la sienne, le papier sur toutes. Noter le temps d'import d'une page de PDF (à l'œil) et si l'ordinateur a ralenti.

#### C. Une feuille reprise

1. Reprendre en direct une note qui a déjà une image de fond. Remplacer l'image par la fiche page 1, changer le papier, écrire, **Terminer**.
2. Ouvrir la note : la nouvelle image, le nouveau papier. **Revenir à l'original** : l'encre d'avant revient, l'image et le papier restent (c'est voulu).

#### D. Projeter une note de l'éditeur

1. Ouvrir une note, onglet **Projection**, **Projeter**. Écrire au stylet de la Surface : le trait apparaît-il au vidéoprojecteur **pendant qu'on écrit** ? Noter le retard ressenti.
2. `Ctrl + Z`, `Ctrl + Y`, masquer un calque, changer le papier : le vidéoprojecteur suit-il à chaque fois ? Vérifier que la sélection et le lasso n'y apparaissent **jamais**.
3. **Agrandir** deux fois, puis faire défiler et zoomer l'éditeur : la zone projetée suit-elle ? Le cadre en pointillés montre-t-il ce que voit la classe ? **Page entière**, **Largeur**.
4. **Relecture** (`R`) : la relecture passe-t-elle au vidéoprojecteur sans saccade ?
5. Note suivante (`Alt + →`) : le vidéoprojecteur montre la nouvelle note. **Partager sur le réseau** en même temps : les deux marchent, et le point bleu de **Projection** reste.
6. `Échap` sur le vidéoprojecteur : il se ferme, l'onglet le dit. Projeter à nouveau, puis revenir à la bibliothèque : la projection se ferme.
7. Avec **un seul écran** (débrancher le projecteur, ou choisir l'écran de la Surface) : la projection recouvre l'éditeur ; que fait Windows de l'éditeur dessous (le trait suit-il toujours ?), et `Échap` la ferme-t-il ?
8. Projeter la note, puis revenir à la bibliothèque et ouvrir le **Mode direct** et **Projeter** : une seule fenêtre projetée à la fois.

#### E. Le son pendant qu'on écrit

1. Ouvrir une note déjà écrite en classe. Onglet **Son**, **Pendant que j'écris**, **Enregistrer en écrivant**, choisir le micro de la Surface (regarder le **Niveau du micro**), **Démarrer**. Parler une minute en écrivant au stylet, avec des moments où l'on parle sans écrire et où l'on écrit sans parler. **Arrêter et enregistrer** : « Son enregistré … avec vos traits ». Noter combien de temps passe avant que la note soit enregistrée.
2. **Relecture**, puis **Lire avec la narration** : les traits apparaissent-ils avec la voix ? Noter un décalage, en secondes, **au début et à la fin** de l'enregistrement. Combien de temps passe avant la voix (l'encre déjà là défile-t-elle en temps réel : c'est une limite connue) ?
3. **Vidéo**, exporter en MP4 avec **Inclure la narration**, la lire dans le lecteur de Windows (et dans VLC si possible) : même vérification.
4. **Enregistrer en écrivant** à nouveau, **Démarrer** : Ardoise demande s'il faut **remplacer** ; essayer **Garder l'ancienne** (rien ne s'enregistre), puis **Remplacer**.
5. Pendant un enregistrement, écrire un mot puis `Ctrl + Z`, et coller des traits (`Ctrl + V`) : le trait annulé revient à son heure avec `Ctrl + Y`, le trait collé paraît à l'instant où on l'a collé.
6. Avec **Dessiner au doigt** activé dans les **Réglages**, écrire au doigt pendant un enregistrement : le trait au doigt suit le son comme celui du stylet.
7. Pendant un enregistrement, essayer de quitter l'éditeur, de lancer la **Relecture**, de **Revenir à l'original** : refusés avec « Arrêtez ou annulez d'abord l'enregistrement de la narration. »

### Ce qui revient par Nextcloud

- Les notes de la séance (A à C) et celles de D et E, dans la bibliothèque (le son est dans la note).
- Dans `<bibliothèque>\_ardoise\diagnostics\` : les journaux du partage, et un journal seulement en cas d'erreur de la tablette ou d'une session de mode direct anormale.
- La vidéo exportée en E3 et les mesures de décalage sont à déposer à la main avec le compte rendu.

### À me signaler

Ce qui ne revient pas tout seul :

| Quoi | Où |
|---|---|
| Tout **à-coup** de l'écriture de l'élève pendant un changement de papier ou de fond, et le **temps d'apparition** de la fiche au vidéoprojecteur et sur le partage | A2, B1 |
| Le texte exact des phrases sur le tableau noir (couleur grisée) et sur l'ardoise noire | A3, A4 |
| Un **décalage de l'écriture sur la fiche**, au vidéoprojecteur ou sur le partage, aussi en zoom | B2 |
| Si la télécommande tourne bien la page après un choix dans une liste ou sur un curseur | B3 |
| Ce que fait la page avec une fiche quand on tourne la tablette, avant puis après avoir écrit, et le texte de la phrase | B5 |
| Le temps d'import d'une page de PDF et si l'ordinateur a ralenti | B6 |
| Ce que contiennent les notes après **Terminer** (papier, fiche, photo), et ce que rend **Revenir à l'original** | B6, C |
| Le **retard du trait de l'éditeur** au vidéoprojecteur, et si la sélection n'y apparaît jamais | D1, D2 |
| La **zone projetée** et le cadre en pointillés : suivent-ils l'éditeur, montrent-ils ce que voit la classe | D3 |
| La relecture projetée (saccades) | D4 |
| Avec un seul écran : ce que fait Windows de l'éditeur sous la projection | D7 |
| Le **décalage entre la voix et l'écriture**, en secondes, au début et à la fin, dans Ardoise et dans la vidéo (lecteur de Windows et VLC) | E2, E3 |
| Le temps avant la voix quand la note contenait déjà de l'écriture | E2 |
| Si la question **Remplacer** / **Garder l'ancienne** apparaît et si chaque réponse fait ce qu'elle dit | E4 |
| Si le trait collé, le trait annulé puis rétabli et le trait au doigt suivent le son | E5, E6 |
| Toute phrase incompréhensible, et ce que **Windows Ink** affiche par-dessus la page | tout le test |

---

## H8 — La tablette Xiaomi

**Durée** : 45 à 60 minutes. **Matériel** : le PC d'Ardoise, la **tablette Xiaomi Mijia « petit tableau noir »** (chargée, avec son stylet), le **dongle Bluetooth** branché, **le téléphone avec Mi Home** (Bluetooth du téléphone **coupé** pendant le test), **la Bamboo Slate** (parties G et H seulement), une **règle**. Ce test suppose que H0 à H2 ont réussi.

Ce test vérifie SPEC §4.1 « Tablette Xiaomi » et §4.2 « Avec la tablette Xiaomi » (`docs/questions.md` Q14, `docs/design/blackboard.md`) avec la vraie tablette : rien n'a été essayé dessus par Ardoise avant (seules des captures de Mi Home ont été étudiées). Les libellés ci-dessous sont ceux de la fenêtre.

### À la maison, avant

1. Fermer Ardoise sur tous les PC, puis remplacer `ardoise.exe` dans le dossier Nextcloud par la nouvelle version.
2. Ne **pas** utiliser `--simulated-tablet` ni `--simulated-blackboard`.
3. Noter la version d'Ardoise et celle de Windows.

### Au travail

#### A. Préparer et ajouter la tablette

1. **Allumer la tablette Xiaomi.** **Fermer Mi Home** sur le téléphone (le fermer vraiment, pas seulement le mettre en arrière-plan) et **couper le Bluetooth du téléphone**.
2. Lancer Ardoise, ouvrir le panneau **Tablette**, cliquer sur **Ajouter la tablette Xiaomi**, lire la fenêtre, cliquer sur **Chercher la tablette**. Noter le temps avant que la tablette soit trouvée, son **nom** annoncé (attendu : `xiaomi.dev.ble`) et son **Signal**.
3. Cliquer sur **Ajouter cette tablette**. Noter si Ardoise se connecte seule (**Connexion…**, puis **Connectée**) et en combien de temps.
4. *Variante, si le temps le permet* : refaire la recherche avec Mi Home **ouvert** : la tablette doit rester introuvable, avec le message « Aucune tablette Xiaomi trouvée ». Noter le texte exact et si la recherche s'arrête bien au bout d'une minute.

#### B. Ses informations

1. Dans le panneau **Tablette**, noter la **batterie** (comparer avec Mi Home si possible), la **mémoire libre** et son total, le nombre de **pages en mémoire**, le **modèle**, la **zone de dessin** affichée.
2. Vérifier qu'il n'y a **ni nom à changer, ni version de firmware, ni « en charge », ni bouton Mettre à l'heure**. Noter si la tablette est bien à l'heure (la date d'une page écrite à l'étape C le dira).

#### C. Écrire une page, la récupérer

1. Sur la tablette, **écrire une page d'essai** : quelques mots, un cercle, un trait appuyé et un trait léger, et **une ligne droite de 10 cm** repérée à la règle. Appuyer sur le **bouton d'enregistrement** de la tablette (celui qui range la page).
2. Dans Ardoise, **Synchroniser maintenant** (avec **Effacer de la tablette après import** coché, la synchronisation part toute seule). Noter le temps du transfert et si la progression (**Page 1 sur …**) est juste.
3. Ouvrir la note : l'écriture est-elle fidèle (forme, épaisseur, pression) ? La **date** est-elle celle de l'enregistrement sur la tablette ? **Mesurer la ligne de 10 cm** dans la note (l'unité : exporter en PDF et mesurer sur papier ou à l'écran, ou comparer avec une règle posée sur l'écran) : noter la longueur obtenue.
4. Le nombre de **pages en mémoire** est-il retombé à 0 ? **Ouvrir Mi Home** (après avoir remis le Bluetooth du téléphone) : la page a-t-elle bien **disparu de la tablette** ?
5. *Option désactivée* : décocher **Effacer de la tablette après import**, écrire et enregistrer deux autres pages, reconnecter la tablette (l'éteindre et la rallumer). Le panneau dit-il « 2 pages attendent sur la tablette Xiaomi » et **rien** n'est-il importé tout seul ? **Synchroniser maintenant** demande-t-il de confirmer (**Synchroniser et effacer**) ? Les deux pages sont-elles importées puis effacées ? Recocher l'option.

#### D. Le mode direct

1. **Mode direct** depuis le panneau. Écrire pendant **trente secondes**. Noter le retard du trait, la fidélité, et si le **pointeur laser** suit le stylet quand il survole sans toucher.
2. Vérifier que la page du direct est **en hauteur**, sans choix de position, et que le rappel « Tablette Xiaomi : page en hauteur » est là.
3. **Enregistrer une page avec le bouton** de la tablette pendant le direct : le direct continue-t-il sans coupure ni trait parasite ? Terminer le direct : la page enregistrée est-elle importée ensuite (doublon de la note du direct)?
4. Relancer le direct, puis **appuyer sur le bouton d'effacement de l'écran** de la tablette : noter **ce qui se passe** dans Ardoise (rien ? nouvelle page ? trait parasite ? coupure ?), à chaque appui. (SPEC §4.2 : le direct passe à une nouvelle page si la tablette le signale ; sinon on change de page avec **Page suivante**.)
5. **Couper puis rétablir le Bluetooth du PC** (ou s'éloigner de la tablette) pendant le direct : l'état dit-il **Reconnexion…**, et le direct reprend-il tout seul, sur la même tablette ? Noter le temps.
6. **Annuler une synchronisation au milieu d'une grande page** (écrire une page très chargée, enregistrer, lancer **Synchroniser maintenant**, puis **Annuler** pendant le transfert), puis lancer le mode direct : **aucun trait parasite** ne doit apparaître. Noter le temps que prend la liaison à se rétablir (Ardoise ferme la liaison et la rouvre : c'est voulu).

#### D bis. Continuer une feuille

1. Dans la bibliothèque, **Reprendre en mode direct** sur la note récupérée de la Xiaomi à l'étape C, **avec la Xiaomi connectée**. Écrire un mot et la même ligne de 10 cm : le **nouveau trait tombe-t-il là où est le stylet**, et **à la même taille** que l'ancien ? (Comparer la longueur des deux lignes dans la note après **Terminer**.) Noter tout décalage ou réduction.
2. Avec la Xiaomi connectée, essayer de **reprendre une note de la Bamboo Slate** : Ardoise doit refuser tout de suite avec « Cette feuille ne peut pas être continuée avec cette tablette… » (**Le mode direct n'a pas pu reprendre cette feuille.**), sans rien écrire sur la note (la date de modification de la note ne change pas). Noter le texte exact. Puis, la Xiaomi éteinte, reprendre cette même note et allumer la Xiaomi : le message doit arriver à la connexion, le direct passer à **Arrêté**, et **Retour à la bibliothèque** fonctionner.

#### E. Deux tablettes (avec la Bamboo Slate)

1. Enregistrer la Bamboo Slate (chapitre 4 du guide) si elle ne l'est pas : les **deux** tablettes doivent être listées, chacune avec son **Oublier la tablette**.
2. Laisser la Xiaomi **connectée** (état **Connectée**), puis **allumer la Bamboo Slate** (appui bref sur son bouton). **Ardoise passe-t-elle à la Bamboo Slate**, et en combien de temps ? Sinon, que montre le panneau ? Noter si la Xiaomi reste connectée.
3. Lancer le **mode direct** sur la Xiaomi, puis allumer la Bamboo Slate : le direct doit **rester sur la Xiaomi**. Terminer.
4. **Oublier la Xiaomi** (confirmer) : la Bamboo Slate reste connue et se connecte. Ajouter de nouveau la Xiaomi.

### Ce qui revient par Nextcloud

- Les notes importées (C, D), dans la bibliothèque.
- Dans `<bibliothèque>\_ardoise\diagnostics\` : les journaux du service (`AAAA-MM-JJ_HHMMSS.jsonl` et `.txt`), écrits quand quelque chose s'est mal passé ou qu'une trame inconnue est arrivée (la Xiaomi y a ses trames décrites, `BOARD …`). Si rien n'y apparaît alors qu'un essai a échoué, le noter.

### À me signaler

Ce qui ne revient pas tout seul :

| Quoi | Où |
|---|---|
| Le temps de la recherche, le **nom** annoncé et le **Signal** ; le texte exact quand rien n'est trouvé (Mi Home ouvert) | A2, A4 |
| La **batterie** (comparée à Mi Home), la **mémoire**, le nombre de pages, la zone de dessin | B1 |
| Si la **date** d'une page est juste (l'heure de la tablette) | B2, C3 |
| La fidélité de la page importée, et la **longueur obtenue pour la ligne de 10 cm** (c'est l'unité à corriger : `BOARD_WIDTH` et `BOARD_HEIGHT`) | C3 |
| Si la page a bien **disparu de la tablette** après l'import, et si Mi Home la voit encore | C4 |
| Le comportement avec l'option **Effacer de la tablette après import** désactivée, et le texte de la confirmation | C5 |
| Le retard et la fidélité du direct, le pointeur laser | D1 |
| Ce qui se passe pendant le direct quand on **enregistre une page** avec le bouton, et quand on **appuie sur le bouton d'effacement** de l'écran | D3, D4 |
| La reprise du direct après une coupure du Bluetooth, et son délai | D5 |
| Un **trait parasite** après l'annulation d'un transfert, et le délai de reconnexion | D6 |
| Où tombe le **trait ajouté** à une note de la Xiaomi, et sa taille comparée à l'ancien | D bis 1 |
| Le texte exact du refus quand la tablette ne convient pas à la feuille, et s'il arrive tout de suite ou à la connexion | D bis 2 |
| Ce que fait Ardoise quand la **Bamboo Slate** s'allume alors que la Xiaomi est connectée (hors direct, puis pendant un direct) | E2, E3 |
| Toute phrase incompréhensible, tout message d'erreur nouveau | tout le test |
