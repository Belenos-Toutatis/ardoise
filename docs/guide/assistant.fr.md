# Ardoise et votre assistant (Claude Code, Claude Desktop)

Ardoise ne contient aucune intelligence artificielle et fonctionne hors ligne. Elle peut en revanche être reliée à
**votre propre assistant** — Claude Code, Claude Desktop ou tout autre logiciel compatible avec le protocole **MCP**
(Model Context Protocol). L'assistant lit alors vos notes (l'écriture à la main, mais aussi les **textes tapés**, les
**équations** et les **signets**), les **voit comme des images** (schémas, formules, mise en page compris), et peut les
mettre au propre, les titrer, les ranger ou piloter Ardoise pendant le cours.

C'est l'assistant qui apporte l'intelligence ; Ardoise lui ouvre seulement l'accès à votre bibliothèque, quand il le
demande.

> **Retrouver ce guide dans Ardoise.** Dans **Réglages > Assistant (MCP)**, le bouton **Ouvrir le guide « Assistant »**
> l'ouvre dans votre navigateur, hors connexion ; s'il manque à côté d'Ardoise (dossier `guide`), c'est le guide sur
> GitHub qui s'ouvre à la place. La touche `F1` (ou le bouton **Guide**) ouvre, elle, le guide d'utilisation.

## Ce que l'assistant peut faire pour vous

Quelques exemples de demandes, en français, telles que vous les écririez :

- « Mets au propre ma note *Cours d'optique* en **Markdown** » — ou en **Word**, ou en **LaTeX** (avec `circuitikz`
  pour les schémas électriques, `mhchem` pour les équations de réaction, `siunitx` pour les unités) ; l'assistant
  liste ce dont il n'est pas sûr.
- « Donne un titre et des étiquettes à mes notes sans titre de cette semaine. »
- « Résume la séance de mardi avec les 4e B pour le cahier de textes. »
- « Retrouve la note où j'ai écrit la loi d'Ohm. » (l'assistant cherche aussi dans vos textes tapés, vos équations et
  vos signets)
- « Recopie en LaTeX l'équation de ma note *Dilution* » : il lit la source LaTeX que vous avez tapée, exacte, sans
  la deviner sur l'image.
- « Fais-moi la liste des signets de la note *Chapitre 3*, avec leur place sur la page. »
- « Range les notes de chimie dans le classeur *Chimie/3e*. »
- « Assemble mes trois notes sur les circuits en une seule » ; « découpe cette note en deux à l'endroit où j'ai changé
  de sujet » ; « mets à la corbeille la note de test d'hier ».
- « Crée une note sur la page 3 de ce PDF, pour que je l'annote » (avec le chemin du fichier).
- « Exporte la fiche méthode sur la dilution en PDF dans mon dossier Documents. »
- Pendant le cours, Ardoise ouverte : « Ouvre la note de correction », « Projette la page », « Page suivante ».
- À la fin du cours : « Fais une version propre de la note que je viens d'écrire en mode direct. »
- « Range l'écriture de ma note et corrige mes fautes d'orthographe » : l'assistant retouche une **copie**, jamais
  l'originale (voir « Retoucher une note sur une copie » plus bas).

## Ce que l'assistant lit dans une note

Une page d'Ardoise peut contenir de l'écriture à la main, des textes tapés, des équations et des signets. L'assistant
reçoit chacun à sa façon :

- **L'écriture à la main** : l'image de la page (`render_note`) et le texte reconnu par Windows, ligne par ligne, avec sa
  position en millimètres (`get_note_text`). La reconnaissance se trompe, surtout sur les formules et les schémas :
  l'assistant doit comparer avec l'image. Si l'écriture ne peut pas être reconnue sur ce PC (pas de module d'écriture
  pour la langue), `get_note_text` répond quand même pour une page qui porte du texte tapé : `lines` est vide, la
  structure ne contient que le texte tapé et `recognition` vaut `{"available": false, …}` avec le code et la raison ;
  pour une page sans texte tapé, c'est une erreur.
- **Les textes tapés** (outil **Texte**) : leur texte est **exact**, rien n'est deviné ; l'assistant en a aussi la
  police, le gras, la taille, la couleur et la place sur la page, dans l'ordre de lecture (`get_note` et
  `get_note_text`). `get_note_text` donne tous les cadres, en disant pour chacun si son calque est visible ; le « texte
  tapé » d'ensemble qu'il rend ne reprend que ceux des calques visibles.
- **Les équations** : leur source **LaTeX**, exacte (la chimie en `\ce{…}`), avec taille, couleur et place ;
  `render_note` en montre l'aspect. Elles se lisent aussi dans le texte tapé d'ensemble, sous forme de source.
- **Les signets** : les mots que vous avez posés à des endroits de la page, avec leur position (`get_note`). Ils ne
  servent qu'à l'éditeur : l'image de `render_note` ne les dessine pas.
- Un objet d'un genre que cette version d'Ardoise ne connaît pas est signalé à l'assistant (`other_objects`), mais
  `render_note` ne le montre pas.

**La recherche** (`search_notes`) cherche comme l'application : dans les titres, les étiquettes, les classeurs, les
textes tapés (équations comprises), les signets et l'écriture reconnue, sans tenir compte des accents ni des
majuscules. Chaque résultat dit **où** le mot a été trouvé (titre, étiquettes, classeur, texte tapé, signets, écriture)
et montre un court extrait. Les notes dont l'écriture n'a pas encore été lue par Windows ne sont pas cherchées par leur
écriture : le résultat en donne le nombre, et l'assistant peut en faire lire une (`get_note_text`) pour qu'elle soit
trouvée ensuite.

## Avant de commencer

- Ardoise a déjà été ouverte au moins une fois sur ce PC, et **une bibliothèque y est choisie** (le réglage est
  propre à chaque PC).
- Pour que l'assistant lise **votre écriture**, il faut la reconnaissance d'écriture manuscrite de Windows en
  français : Paramètres Windows > Heure et langue > Langue et région > Français > Options de langue > Écriture
  manuscrite. Sans elle, l'assistant voit quand même vos notes en image et lit leurs textes tapés et leurs équations.
- Dans Ardoise, **Réglages > Assistant (MCP)** affiche le texte exact à copier, avec le chemin de *votre*
  `ardoise.exe`. Copiez-le de là plutôt que de le retaper : c'est le plus sûr. Cette page a aussi la case
  **Lecture seule**, la ligne **Liaison avec Ardoise ouverte** et le bouton qui ouvre ce guide.

> Ardoise est portable : si vous déplacez son dossier, refaites la déclaration avec le nouveau chemin. Si vous avez
> plusieurs copies d'Ardoise, l'assistant utilise celle dont le chemin est déclaré.

## Déclarer Ardoise dans Claude Code

Une seule fois, dans **PowerShell** ou l'**invite de commandes** (le chemin est un exemple : prenez celui que donne
Réglages > Assistant) :

```
claude mcp add --scope user ardoise "--" "C:\Outils\Ardoise\ardoise.exe" --mcp
```

- `--scope user` : Ardoise est disponible dans **tous vos projets** Claude Code (la déclaration est enregistrée
  dans votre profil, pas dans un projet).
- Gardez les **guillemets** autour du chemin (il peut contenir des espaces).
- Le `"--"` sépare les options de Claude Code de la commande d'Ardoise : tout ce qui suit est donné tel quel à
  Ardoise. **Gardez aussi ses guillemets** : si Claude Code a été installé avec npm, la commande `claude` est un
  script que PowerShell lance en supprimant un `--` sans guillemets, et `--mcp` serait alors pris pour une option de
  Claude Code. Avec les guillemets, la même commande marche dans PowerShell comme dans l'invite de commandes, quelle
  que soit l'installation de Claude Code.
- Vérifier : `claude mcp list` (Ardoise doit être « Connected »), ou `/mcp` dans Claude Code.
- Retirer : `claude mcp remove ardoise`.

## Déclarer Ardoise dans Claude Desktop

1. Dans Claude Desktop : **Paramètres > Développeur > Modifier la configuration** (*Settings > Developer > Edit
   Config*). Cela ouvre le fichier `claude_desktop_config.json`, quel que soit le mode d'installation :
   - installation classique : `%APPDATA%\Claude\claude_desktop_config.json` ;
   - installation par le Microsoft Store (ou le paquet MSIX) : le dossier de l'application est alors sous
     `%LOCALAPPDATA%\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\Claude\` ; le bouton *Modifier la
     configuration* ouvre toujours le bon fichier.
2. Ajoutez l'entrée `ardoise` dans `mcpServers` (copiez-la depuis Réglages > Assistant ; dans ce fichier, chaque
   `\` du chemin est doublé) :

   ```json
   {
     "mcpServers": {
       "ardoise": {
         "command": "C:\\Outils\\Ardoise\\ardoise.exe",
         "args": ["--mcp"]
       }
     }
   }
   ```

   S'il y a déjà d'autres serveurs dans `mcpServers`, ajoutez seulement le bloc `"ardoise": { … }` à côté d'eux,
   séparé par une virgule.
3. Enregistrez, puis **quittez complètement** Claude Desktop (aussi depuis la zone de notification) et relancez-le.

En cas de souci, le journal de Claude Desktop pour Ardoise est `%APPDATA%\Claude\logs\mcp-server-ardoise.log`.

## L'option « lecture seule » (`--read-only`)

Avec `--read-only` (case **Lecture seule** dans Réglages > Assistant, qui l'ajoute aux deux textes), l'assistant
peut **seulement lire** : lister et chercher les notes, lire leur contenu (écriture reconnue, textes tapés, équations,
signets), les voir en image et regarder de près leurs traits (`get_strokes`, `ink_lines`). Ardoise ne lui donne alors
**aucun autre outil** : il ne peut ni créer, ni renommer, ni ranger, ni mettre à la corbeille, ni assembler ou découper,
ni exporter, ni enregistrer une transcription corrigée, ni essayer ou faire une retouche, ni lire l'état d'Ardoise ou
la dernière séance du mode direct, ni piloter Ardoise. L'invite `titrer_et_etiqueter` donne alors une liste de titres
et d'étiquettes à appliquer vous-même dans Ardoise ; `mettre_au_propre` marche sans `export_note` (les dessins sont
alors à exporter vous-même depuis l'application) ; `ameliorer_note` n'existe pas.

**Conseillé chaque fois que l'assistant n'a besoin que de lire** (résumer, retrouver une note, préparer un document
à partir de vos notes).

## Les invites prêtes à l'emploi

Ardoise propose des invites toutes prêtes. Dans Claude Code, tapez `/` : elles apparaissent sous la forme
`/ardoise:nom (MCP)` ; on peut aussi taper `/mcp__ardoise__nom`. Les arguments se donnent à la suite, séparés par des
espaces. Leur nom suit la langue d'Ardoise :

| En français | En anglais | Rôle |
|---|---|---|
| `mettre_au_propre` | `clean_up` | une note mise au propre en Markdown, Word ou LaTeX (arguments : la note, le format) |
| `titrer_et_etiqueter` | `title_and_tag` | titre et étiquettes pour les notes sans titre (argument : un classeur) |
| `resume_seance` | `lesson_summary` | résumé d'une séance pour le cahier de textes (arguments : le jour, le classeur de la classe) |
| `ameliorer_note` | `improve_note` | une note améliorée sur une copie (écriture rangée, schéma redressé, fautes corrigées) ; absente en lecture seule (arguments : la note, ce que vous voulez améliorer) |

Sans note donnée, `mettre_au_propre` et `ameliorer_note` prennent la dernière note du mode direct (voir « Piloter
Ardoise ouverte » plus bas).

Vous pouvez aussi simplement demander en français, sans invite : l'assistant choisit lui-même les outils d'Ardoise.

## Les outils d'Ardoise, selon ce que vous demandez

Vous n'avez pas à les connaître : l'assistant choisit. Cette liste dit ce qui est possible, et vous aide à comprendre
ce qu'il affiche quand il demande l'autorisation d'utiliser un outil. Une note se désigne par son identifiant ou par
son chemin dans la bibliothèque ; les positions sont en millimètres depuis le coin haut gauche de la page.

**Lire, chercher, voir** (les seuls outils qui restent en lecture seule) :

| Vous voulez… | Une demande | Outils |
|---|---|---|
| parcourir la bibliothèque | « Quelles notes ai-je écrites cette semaine dans le classeur *4e B* ? » | `list_notes` (par classeur, étiquette, favori, dates ; tri), `list_binders`, `list_tags` |
| retrouver une note | « Retrouve la note où j'ai écrit la loi d'Ohm. » | `search_notes` |
| lire une note | « Que dit ma note *Optique* ? » | `get_note` (papier, calques, cadres de texte, équations, signets ; les traits sur demande), `get_note_text` (écriture reconnue et texte tapé) |
| qu'il regarde une note | « Regarde le schéma en bas de *Circuit série*. » | `render_note` (page entière ou zone, avec ou sans le numéro des traits) |
| un travail fin sur l'encre | « Dis-moi où sont les lignes écrites de cette page. » | `get_strokes` (les points d'un trait), `ink_lines` (les lignes écrites, leur pente, leur hauteur) |

**Créer, titrer, ranger** (l'encre n'est jamais modifiée dans la note elle-même) :

| Vous voulez… | Une demande | Outils |
|---|---|---|
| une nouvelle note | « Crée une note quadrillée dans le classeur *Chimie/3e*. » ; « Crée une note sur la page 3 de ce PDF. » | `create_note` (vierge, ou sur une image ou une page de PDF) |
| titrer, étiqueter, mettre en favori | « Donne un titre à mes notes sans titre. » | `update_note` (le titre change le nom du fichier ; l'ancien titre et les anciennes étiquettes ne sont pas gardés) |
| ranger | « Range les notes de chimie dans *Chimie/3e*. » | `move_note` (le classeur est créé s'il manque ; une note du même nom n'est jamais écrasée) |
| supprimer | « Mets à la corbeille la note de test. » | `trash_note` (restaurable depuis la corbeille d'Ardoise, 30 jours) |
| assembler | « Assemble ces trois notes l'une après l'autre. » | `merge_notes` (sur une seule page ou bout à bout ; les notes d'origine ne changent pas) |
| découper | « Découpe cette note en deux à cet endroit. » | `split_note` (à un instant ou autour d'une zone ; voir « Retoucher une note sur une copie ») |

**Exporter** :

| Vous voulez… | Une demande | Outil |
|---|---|---|
| des fichiers | « Exporte la fiche sur la dilution en PDF dans mon dossier Documents. » | `export_note` |

`export_note` fait les mêmes exports que l'application : PNG, JPG, WebP, SVG, PDF (une page par note), `.txt`,
Markdown et Word, et le rejeu de l'écriture d'**une** note en GIF, WebM ou MP4 (de quelques secondes à plusieurs
minutes). Il n'écrase jamais un fichier : un nom déjà pris reçoit « (2) ». Quelques précisions :

- Le **PDF d'une seule note** a les signets de la note pour plan (le panneau de signets de votre lecteur PDF). Le PDF de
  plusieurs notes n'en a pas : pour le PDF d'un classeur, avec son plan et sa page de sommaire, utilisez **Exporter le
  classeur en PDF** dans Ardoise.
- Les **images** n'ont pas les signets (ils sont à l'usage de l'éditeur).
- Les fichiers `.txt`, `.md` et `.docx` produits par `export_note` contiennent l'écriture manuscrite reconnue (ou sa
  correction) et le texte tapé sur la page, dans l'ordre de lecture : cadres de texte tels qu'ils ont été saisis,
  équations sous forme de source LaTeX (entre `$…$` en Markdown, en simple paragraphe dans Word). Les dessins s'y
  ajoutent en `.md` et `.docx`, sauf pour les notes dont l'écriture ne peut pas être reconnue sur ce PC : elles ne
  donnent alors que le texte tapé.

**Corriger la transcription** :

| Vous voulez… | Une demande | Outil |
|---|---|---|
| que la recherche et les exports de texte utilisent un texte juste | « Windows a lu *ohn* au lieu de *Ohm* : corrige la transcription de ma note. » | `set_note_text` |

La correction reste dans la mémoire d'écriture de **ce PC**, jamais dans la note, et ne vaut que tant que l'encre de la
note ne change pas ; un texte vide l'enlève. Elle ne concerne que l'écriture, pas les textes tapés.

**Retoucher l'encre, sur une copie** (chapitre suivant) :

| Vous voulez… | Une demande | Outils |
|---|---|---|
| ranger l'écriture, redessiner un schéma, corriger des fautes | « Range l'écriture de ma note *Compte rendu*. » | `revision_orders` (la liste des retouches possibles), `preview_revision` (essayer sans rien écrire), `create_revision` (écrire la copie), `edit_revision` (retoucher encore une copie) |
| des formes nettes | « Redresse les formes de ma note de physique. » | `straighten_shapes` |

**Piloter Ardoise ouverte** (voir plus bas ; aucun de ces outils n'existe en lecture seule) :

| Vous voulez… | Une demande | Outil |
|---|---|---|
| savoir ce qu'Ardoise affiche | « Quelle note est ouverte ? » | `app_state` |
| ouvrir une note | « Ouvre la note de correction. » | `open_note` |
| projeter en classe | « Projette la page. », « Page suivante. » | `presentation` |
| retrouver ce qui vient d'être écrit | « Fais une version propre de la note que je viens d'écrire. » | `last_live_note` |

## Retoucher une note sur une copie

L'assistant peut **améliorer une note sans toucher à l'encre de l'originale** : il en fait une *copie*, à côté, et
c'est sur la copie qu'il travaille. Il faut bien distinguer ce qu'**Ardoise garantit** (le programme l'impose, quel
que soit l'assistant et ce que vous lui dites) de ce qui est **seulement demandé à l'assistant** (il peut y manquer).

**Ce qu'Ardoise garantit :**

- **L'encre de l'original n'est jamais écrite par l'assistant** : les outils qui écrivent de l'encre ne savent modifier
  qu'une copie de ce genre, jamais une note ordinaire.
- **Pendant un essai, rien n'est écrit** nulle part.
- **La copie est une nouvelle note**, à côté de l'originale. Découper une note, quand c'est l'assistant qui le fait,
  donne de même deux notes neuves, marquées comme des copies : la note de départ reste entière.
- **Si la note a changé depuis que l'assistant l'a regardée** (vous l'avez retouchée à la main, par exemple), Ardoise
  refuse d'écrire la copie ou la retouche : l'assistant doit regarder de nouveau.
- **Une retouche ne peut désigner que des traits de la note qu'elle retouche**, jamais ceux d'une autre note. Les
  traits d'un calque masqué ne sont touchés que si l'assistant les désigne expressément.
- **Les retouches ne portent que sur les traits** : les textes tapés, les équations et les signets de la page ne sont
  jamais changés ; la copie les garde tels quels, et un découpage les donne à la partie à laquelle ils
  appartiennent selon l'instant ou la zone choisis (un texte tapé va à la seconde partie s'il a été écrit à partir de
  l'instant, ou si son centre est dans la zone).
- **Rien n'est supprimé définitivement** : la version d'avant une retouche va à la corbeille (voir « Vie privée »).

**Ce qui est seulement demandé à l'assistant**, par l'invite `ameliorer_note` (`improve_note` en anglais), et
qu'Ardoise ne peut pas vérifier : lister d'abord les fautes d'orthographe et ne corriger que celles que vous
validez ; ne prendre les lettres que dans la même écriture, dans la même note ; marquer en couleur ce qu'il ne sait pas
écrire ; vous dire tout ce qu'il a changé ; garder la relecture juste. **Sans cette invite, rien de cela n'est
demandé** : c'est à vous de le dire (« donne-moi d'abord la liste des fautes », « n'utilise que mon écriture de cette
note »). Dans tous les cas, **regardez toujours la copie** (et **Comparer**) avant de vous en servir.

**Ce qu'on peut lui demander**, avec vos mots :

- « Range l'écriture de ma note *Compte rendu d'incident* : lignes droites, même taille de lettres, écriture posée sur
  les lignes du papier. Ne change aucun mot. »
- « Redessine proprement le schéma de la note *Forces sur un bateau* : flèches droites, forces égales de la même
  longueur, axes alignés. »
- « Mets en couleur les mots à retenir et encadre le résultat dans la note de cours sur la dilution. »
- « Corrige les fautes d'orthographe de ma note avec ma propre écriture. »
- « Redresse les formes de ma note de physique » (cela aussi se fait maintenant sur une copie : l'originale n'est plus
  modifiée). L'assistant redresse avec **les mêmes valeurs que le réglage** à côté du bouton « Redresser les formes » de
  l'éditeur (tolérance, taille minimale, alignement automatique) et vous dit celles qu'il a utilisées, aussi dans le
  compte rendu de la copie quand elles ne sont pas les valeurs d'origine. Vous pouvez lui en demander d'autres pour une
  fois (« avec une tolérance plus stricte », « même les traits de 6 mm ») sans toucher à votre réglage ; en dessous de
  10 mm, des lettres peuvent être redressées.
- « Découpe ma note en deux à cet endroit » (les deux morceaux sont des notes neuves, la note de départ reste telle
  quelle).
- Dans Claude Code, l'invite `ameliorer_note` (ou `improve_note` en anglais) déroule tout cela pas à pas, avec les
  demandes de prudence ci-dessus ; elle n'existe pas en lecture seule.

**Ce qui se passe**, dans l'ordre :

1. L'assistant regarde la page (l'image, les lignes écrites, le texte reconnu) et vous dit ce qu'il compte faire.
2. Il **essaie** : il prépare une liste de retouches et voit l'image du résultat **sans rien écrire** nulle part. Il
   recommence autant qu'il faut, en regardant l'image à chaque fois.
3. Quand l'image est bonne, il **crée la copie** (titre : « `titre` (propre) », ou « `titre` (clean) » en anglais) et,
   comme l'invite le lui demande, vous dit tout ce qu'il a changé. Ce compte rendu, écrit par lui, reste avec la copie.

**Les fautes d'orthographe d'abord en liste** (demandé par l'invite). L'assistant ne doit pas corriger en silence : il
vous donne la liste des fautes qu'il voit, *vous* dites lesquelles corriger, et seules celles-là doivent l'être. Il ne
doit pas changer le sens de ce que vous avez écrit. Pour un document dont chaque mot compte (un compte rendu, une
fiche d'incident), il doit rester sobre : pas de couleur, sauf si vous en demandez. Ardoise ne contrôle pas le texte
corrigé : relisez.

**D'où viennent les lettres d'un mot corrigé** (demandé par l'invite). Pour remplacer une lettre, l'assistant doit
prendre, dans l'ordre :

1. la même lettre *dans votre écriture, dans cette même note* (il compare les lettres avant de choisir : une note peut
   contenir deux écritures, la vôtre et celle d'un élève ou d'un collègue) ;
2. sinon des morceaux de cette écriture (le rond d'un « q » et la queue d'un « j » font un « g », un « w » agrandi fait
   un « W ») ;
3. sinon il ne fabrique rien : il **marque le mot en couleur** et vous le signale.

**Pas de lettre prise dans une autre note.** Vous confiez parfois votre calepin à un élève : une lettre venue d'une
autre note pourrait être la sienne, ou celle d'un autre jour. Ardoise fait ceci : une retouche ne peut pas désigner un
trait d'une autre note. Elle ne peut pas empêcher cela : un assistant qui recopierait à la main les points d'un trait
vu ailleurs ; seule l'invite le lui interdit. Une lettre fabriquée se lit bien, mais reste un peu serrée à côté de la
vraie écriture : l'assistant doit les nommer toutes dans son compte rendu.

**La relecture : ce qu'on demande à l'assistant.** Pour chaque lettre ajoutée, ou mot déplacé dans sa phrase, il doit
dire où elle prend place dans le temps de l'écriture (celle du rejeu et de la vidéo) ; Ardoise décale alors les traits
suivants si nécessaire. Si la note a une *narration*, rien ne se décale (la voix et l'écriture ne seraient plus
d'accord) : la lettre est tracée plus vite. Si l'assistant ne l'a pas fait, le rejeu peut être faux : regardez-le.

**Dans l'éditeur**, ouvrez la copie : une ligne sous la barre du haut dit « Copie retouchée de « Cours d'optique » »,
avec trois boutons — **Ouvrir l'original**, **Ce qui a changé** (le compte rendu de l'assistant, une ligne datée par
retouche) et **Comparer** (l'original et la copie côte à côte). Les deux boutons « original » montrent la note
d'origine *telle qu'elle est maintenant* ; si elle n'est plus dans la bibliothèque, ils sont grisés. Voir le
chapitre 8 du guide d'utilisation.

**Retoucher encore.** Vous pouvez demander une nouvelle retouche de la copie (« mets aussi le titre en rouge ») :
l'assistant la modifie *sur place*, en gardant ce que vous avez changé à la main, et ajoute une ligne au compte rendu.
La version d'avant est mise à la corbeille d'Ardoise (voir « Vie privée »). Si vous avez modifié la copie entre-temps,
l'assistant doit d'abord la regarder de nouveau : Ardoise refuse sinon. Demander une retouche de la *note originale*
fait une nouvelle copie.

**Ce que ça ne remplace pas.** L'assistant peut se tromper, et Ardoise ne vérifie pas ce qui est seulement « demandé »
plus haut (fautes validées, lettres de la bonne écriture, compte rendu complet, couleur) : regardez toujours la copie
(et **Comparer**) avant de vous en servir. Et le rappel du chapitre « Vie privée » vaut aussi ici : pour retoucher une note, l'assistant la voit, donc elle part
chez son fournisseur.

Les outils d'Ardoise derrière tout cela (vous n'avez pas à les connaître, l'assistant les choisit) : **lire** (`get_note`
avec ses lignes de papier, `get_strokes` pour les points d'un trait, `ink_lines` pour les lignes écrites,
`render_note` avec les numéros des traits écrits à côté d'eux) ; **écrire** (`revision_orders` pour connaître les
ordres, `preview_revision` pour essayer, `create_revision` pour créer la copie, `edit_revision` pour retoucher une
copie, `straighten_shapes`).

## Piloter Ardoise ouverte

Quatre outils agissent sur Ardoise **ouverte** (ils ne font rien si elle est fermée, et n'existent pas en lecture
seule). Aucun ne supprime quoi que ce soit, ne change un réglage ni ne touche à la tablette :

- **état** (`app_state`) : ce qu'Ardoise affiche — la bibliothèque, l'éditeur (quelle note, modifications non
  enregistrées ou non), le mode direct (page en cours, présentation ouverte ou non) ;
- **ouvrir une note** (`open_note`) dans l'éditeur, comme un clic sur sa vignette. Ardoise refuse si la note ouverte
  a des modifications non enregistrées, si le mode direct est à l'écran ou si une fenêtre de dialogue est ouverte :
  rien n'est jamais perdu ;
- **présentation** (`presentation`) : projeter la page du mode direct sur l'écran choisi dans la fenêtre du mode
  direct, arrêter la projection, page suivante, précédente ou numéro de page. **C'est vous qui lancez le mode
  direct** (panneau de la tablette > Mode direct) : l'assistant ne touche jamais à la tablette. « Page suivante » depuis
  la dernière page en ajoute une vierge, comme la touche `→` ;
- **dernière note du mode direct** (`last_live_note`) : les notes écrites pendant le mode direct en cours ou le
  dernier (une note par page), pour en proposer une version propre à la fin du cours. Ardoise s'en souvient sur ce
  PC même après avoir été fermée.

L'assistant doit utiliser **la même copie** d'Ardoise que celle que vous ouvrez (même dossier).

## En cas de problème

| L'assistant dit… | Que faire |
|---|---|
| `noLibrary` : aucune bibliothèque | Ouvrez Ardoise sur ce PC et choisissez la bibliothèque. |
| `handwritingNoRecognizer` / `handwritingUnsupported` | Installez l'écriture manuscrite française de Windows (voir plus haut). Les images restent lisibles, et `get_note_text` lit encore le texte tapé d'une page qui en porte (`recognition.available` vaut alors `false`) ; sur une page sans texte tapé, il rend cette erreur. |
| `appNotRunning` : « Ardoise is not open » | Ouvrez Ardoise — la copie dont le chemin est déclaré. Si elle est déjà ouverte, regardez la ligne « Liaison avec Ardoise ouverte » dans Réglages > Assistant : elle dit si cette fenêtre peut être pilotée. |
| `appNotResponding` | Ardoise est occupée (export, fenêtre de dialogue) ou démarre encore : réessayez. |
| `appBusy` | Terminez d'abord dans Ardoise ce qui est en cours (enregistrer la note, fermer la fenêtre de dialogue, finir le mode direct). |
| `liveNotRunning` | Lancez le mode direct dans Ardoise avant de demander la présentation. |
| `noteConflict` | La note a changé depuis que l'assistant l'a regardée (vous l'avez retouchée, ou un autre ordinateur l'a fait) : demandez-lui de la regarder de nouveau, puis de recommencer. |
| `notARevision` | L'assistant a voulu retoucher « sur place » une note ordinaire : Ardoise refuse, car l'encre d'une note ordinaire ne change jamais. Demandez-lui de faire d'abord une copie. |
| L'assistant dit qu'il ne peut ni créer, ni ranger, ni exporter, ni piloter Ardoise | Ardoise est déclarée en **lecture seule** : décochez **Lecture seule** dans Réglages > Assistant, recopiez la déclaration et relancez l'assistant. |
| Ardoise n'apparaît pas dans l'assistant | Vérifiez le chemin (dossier déplacé ?), `claude mcp list`, le journal de Claude Desktop ; relancez l'assistant. |

## Vie privée : à lire avant de commencer

- **Ce que l'assistant lit part chez son fournisseur.** Le texte reconnu et l'image de vos notes — qui peuvent
  contenir des **noms d'élèves**, des notes ou des appréciations — sont envoyés au fournisseur de l'assistant
  (Anthropic pour Claude) et traités selon **les réglages de votre compte** (conservation des conversations,
  utilisation pour améliorer les modèles…). Vérifiez ces réglages, et respectez les règles de votre établissement
  (RGPD) avant de faire lire des notes qui contiennent des informations sur des élèves.
- Ardoise elle-même n'envoie rien sur Internet : c'est l'assistant qui lit, quand vous le lui demandez.
- **Lecture seule conseillée** quand l'assistant n'a besoin que de lire (`--read-only`).
- **En général, l'assistant vous demande votre accord avant de modifier quelque chose** : Claude Code et Claude
  Desktop affichent une demande d'autorisation avant d'utiliser un outil qui écrit, et vous pouvez refuser. Mais cela
  dépend de leurs réglages : un outil que vous avez « toujours autorisé », ou un mode qui accepte tout, ne demande
  plus rien. Ardoise signale à l'assistant les outils qui modifient ou retirent des notes (mettre à la corbeille,
  modifier le titre, les étiquettes ou le favori, déplacer, découper, redresser les formes, retoucher une copie déjà
  faite) comme **destructifs**, ce qui l'incite à demander. Pour être sûr que rien ne change, déclarez Ardoise en **lecture seule** (`--read-only`).
- **Rien n'est jamais supprimé définitivement** : une note que l'assistant supprime va dans la corbeille d'Ardoise
  (gardée 30 jours), d'où vous pouvez la restaurer. Quand il retouche une copie déjà faite (ou y redresse les formes, ou
  la découpe), la version d'avant y est mise aussi, sous le nom « `titre` (avant retouche) », « `titre` (avant
  redressement) » ou « `titre` (avant découpe) ».
  L'assistant n'écrit jamais l'encre d'une note ordinaire : il ne retouche qu'une copie, à côté. Assembler des notes
  en fait une nouvelle sans changer les notes d'origine ; les exports n'écrasent jamais un fichier existant ; une
  transcription corrigée reste dans la mémoire d'écriture de ce PC, jamais dans la note. En revanche, **changer le
  titre ou les étiquettes d'une note** (`update_note`) ne garde pas les anciens : c'est un des changements à regarder
  avant d'autoriser.
