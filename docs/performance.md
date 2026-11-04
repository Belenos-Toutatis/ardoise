# Performances

Les objectifs viennent de SPEC §8. `cargo xtask perf` les mesure sur l'exécutable de production (`target\release\ardoise.exe`) et affiche un tableau : objectif, valeur mesurée, verdict.

## Relancer la mesure

1. `cargo xtask check` (compile l'exécutable de production).
2. Fermer les autres programmes, surtout les compilations : les chiffres dépendent de la charge du PC. Brancher le portable sur secteur.
3. `cargo xtask perf` : environ 5 à 10 minutes (la bibliothèque de 1 000 notes se génère en une minute, le MP4 de 2 minutes est un vrai export). `cargo xtask perf --write` met aussi à jour le tableau de ce document. `--only start,library,live,touchup,export,size` ne mesure que certaines parties (sans `--write`).

La commande se termine par `PERF OK` si tous les objectifs sont atteints, sinon par `PERF MISSED` (code de sortie 1) ; les lignes « à titre indicatif » ne sont pas jugées. Les seules données relevées sur le PC sont le modèle du processeur, le nombre de cœurs, la mémoire, la carte graphique et la version de Windows.

## Ce que chaque mesure veut dire

Le script `app/e2e/perf.mjs` copie `ardoise.exe` dans un dossier temporaire (comme un utilisateur qui le pose où il veut), crée les bibliothèques avec `ardoise.exe --create-demo-library DOSSIER [--demo-notes N]` et pilote la vraie fenêtre par le protocole de débogage de WebView2, comme `cargo xtask e2e`. Chaque exécution a son propre `LOCALAPPDATA` : profil WebView2 et cache des miniatures qui n'appartiennent qu'à elle.

| Mesure | Comment elle est prise |
|---|---|
| **Démarrage** (< 2 s) | Temps réel entre le lancement du processus et le repère `ardoise:library-listed`, posé par l'interface deux images après l'affichage de la grille des notes (bibliothèque de démonstration, 8 notes). *À froid* : profil WebView2 neuf. *À chaud* : le même profil, relancé trois fois ; on juge la médiane. |
| **Bibliothèque de 1 000 notes affichée** (< 2 s) | Même mesure avec 1 000 notes de 24 classeurs (copies des notes de démonstration, avec leur identifiant, leur titre, leur date et leurs étiquettes). *Hors cache* : première ouverture, cache des miniatures de ce PC vide. *En cache* : deuxième ouverture, cache rempli par la première. On juge l'ouverture en cache. |
| **Miniatures visibles dessinées** (< 3 s) | Du lancement jusqu'au moment où toutes les cartes visibles à l'écran ont leur image (`data-drawn-at`, posé à l'événement `load` de l'image). |
| **Défilement** (p95 ≤ 33 ms, ≤ 1 % d'images au-delà de 50 ms) | Défilement scripté de la bibliothèque à 3 000 pixels par seconde pendant 6 s (un coup de molette rapide, environ 70 rangées de cartes), en notant l'intervalle entre deux images (`requestAnimationFrame`). On juge le défilement avec miniatures en cache ; celui hors cache est donné à titre indicatif. |
| **Mode direct** (≥ 60 images/s, ≤ 1 % d'images en retard) | La tablette simulée (`--simulated-tablet`, 200 échantillons par seconde comme la vraie) est enregistrée par l'assistant, synchronisée, puis le mode direct est ouvert ; après 2 s, 8 s d'intervalles entre images pendant que le tracé arrive. Une image est « en retard » au-delà de 25 ms (elle a manqué un rafraîchissement à 60 Hz). La mesure est invalide si aucun trait n'est arrivé pendant ce temps. Le seuil de 57 images/s tolère la gigue du minuteur autour d'un écran à 60 Hz. |
| **Mode direct, page chargée et retouchée** (≥ 60 images/s, ≤ 1 % d'images en retard ; `--only touchup`) | Même mesure sur une feuille de 4 000 traits (50 rangées de 80 petits traits, une note posée dans la bibliothèque avant le démarrage) reprise en mode direct depuis le menu de sa carte, pendant que la tablette simulée écrit. Pendant 10 s d'intervalles entre images, le script joue un enseignant par l'interface réelle (souris et touches du protocole de débogage) : outil Sélection, lasso autour d'une centaine de traits, pastille Rouge, `Suppr`, `Ctrl + Z` deux fois, `Ctrl + Y`, `Ctrl + Z`, puis un autre endroit de la page, sans interruption. La mesure est invalide si la tablette n'a rien écrit, si aucune retouche n'a été faite ou si une suppression n'a pas fait baisser le nombre de traits du canevas. Les images de la page pendant les gestes de la souris du script comptent : c'est ce que voit l'enseignant. Avant les retouches, 4 s de la même page sans retouche (la tablette écrit) sont mesurées à part et données à titre indicatif : elles disent ce que coûte la page seule. |
| **Export MP4 1080p d'une relecture de 2 min** (< 2 min) | Le banc d'essai de `ardoise-media` (`export_speed_mp4_2min`, compilé en release) exporte 2 minutes d'écriture à 1920 × 1080 et 30 images/s par Media Foundation : c'est le code que l'application exécute, sans l'interface. L'encodeur choisi par Windows (matériel ou logiciel) est indiqué dans le résultat. |
| **Taille de l'exécutable** (< 30 Mo) | La taille du fichier (la même limite que `cargo xtask selftest`). |

Le WebM (AV1) est plus lent par nature : la durée estimée est affichée avant l'export, il n'a pas d'objectif chiffré.

## Dernière mesure

<!-- perf:start -->
Dernière mesure : 2026-10-29, sur 12th Gen Intel(R) Core(TM) i5-1235U, 10 cœurs / 12 threads, 16 Go de RAM, GPU : Intel(R) Iris(R) Xe Graphics, Windows 10.0.26300.0.

| Objectif | Mesuré | Résultat |
|---|---|---|
| Démarrage à chaud (< 2 s) | 0,95 s (médiane de 3 ; à froid : 1,03 s) | OK |
| 1000 notes affichées (< 2 s) | en cache : 1,77 s ; hors cache : 2,46 s | OK |
| 1000 notes, miniatures visibles dessinées (< 3 s) | en cache : 1,82 s ; hors cache : 2,76 s (12 cartes) | OK |
| 1000 notes, défilement, miniatures en cache (p95 <= 33 ms, <= 1 % au-delà de 50 ms) | 60,0 fps, p95 16,8 ms, 0 en retard (> 25 ms), 0 au-delà de 50 ms (360 images) | OK |
| 1000 notes, défilement, miniatures hors cache (p95 <= 33 ms, <= 1 % au-delà de 50 ms) | 60,0 fps, p95 16,8 ms, 0 en retard (> 25 ms), 0 au-delà de 50 ms (360 images) | à titre indicatif |
| Mode direct (>= 60 images/s, <= 1 % d'images en retard) | 60,0 fps, p95 16,9 ms, 0 en retard (> 25 ms), 0 au-delà de 50 ms (481 images) | OK |
| MP4 1080p d'une relecture de 2 min (< 2 min) | 14,8 s (3599 images, 120 s de relecture ; Media Foundation: Intel® Quick Sync Video H.264 Encoder MFT (hardware)) | OK |
| Taille de l'exécutable (< 30 Mo) | 21,4 Mo | OK |
<!-- perf:end -->

## Mesure de référence avant les corrections de la bibliothèque (M10)

Première mesure, prise sur le code d'avant les corrections de l'affichage de la bibliothèque (premier balayage sur le fil principal, relecture complète de la bibliothèque à chaque changement, écriture du cache des miniatures en O(N²), un `IntersectionObserver` par carte, sélection en O(n²), cache de 400 miniatures), **pendant que d'autres compilations occupaient le PC** : les temps de démarrage et d’affichage (12 s à chaud ; 7 s puis 38 s pour 1 000 notes selon la charge du moment) et le défilement hors cache ne sont donc pas ceux d’un PC au repos. À refaire sur un PC au repos après la fusion ; ce qui reste fiable malgré la charge : le mode direct (60,0 images/s, aucune image en retard), l’export MP4 (62 à 88 s pour 2 min, encodeur matériel Intel Quick Sync) et la taille (19,7 Mo).
