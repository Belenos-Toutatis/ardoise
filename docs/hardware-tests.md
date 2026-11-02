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
3. Vérifier que `ardoise.toml` ne contient pas de section `[share]` : le port par défaut est 8765. Si une section `[share]` existe, noter son port.

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
5. *Facultatif* : changer le port dans `ardoise.toml` (section `[share]`, `port = 8766`), relancer, partager : Windows redemande-t-il l'autorisation ? (Attendu : non, l'autorisation suit le programme, quel que soit le port.) Le second ordinateur se connecte-t-il sur `:8766` ? Remettre ensuite 8765 (ou supprimer la section).
6. *Facultatif* : lancer sur le même PC un programme qui occupe déjà le port 8765 (ou lancer deux copies d'Ardoise placées dans deux dossiers différents et partager dans les deux) : la seconde doit dire que le port est utilisé et comment le changer, sans planter. (Ardoise prend le port pour lui seul pendant un partage : c'est aussi la seconde copie qui doit être refusée si on inverse l'ordre.)
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
