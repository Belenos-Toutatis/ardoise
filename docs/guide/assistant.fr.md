# Ardoise et votre assistant (Claude Code, Claude Desktop)

Ardoise ne contient aucune intelligence artificielle et fonctionne hors ligne. Elle peut en revanche être reliée à
**votre propre assistant** — Claude Code, Claude Desktop ou tout autre logiciel compatible avec le protocole **MCP**
(Model Context Protocol). L'assistant lit alors vos notes, les **voit comme des images** (schémas, formules, mise en
page compris), et peut les mettre au propre, les titrer, les ranger ou piloter Ardoise pendant le cours.

C'est l'assistant qui apporte l'intelligence ; Ardoise lui ouvre seulement l'accès à votre bibliothèque, quand il le
demande.

## Ce que l'assistant peut faire pour vous

Quelques exemples de demandes, en français, telles que vous les écririez :

- « Mets au propre ma note *Cours d'optique* en **Markdown** » — ou en **Word**, ou en **LaTeX** (avec `circuitikz`
  pour les schémas électriques, `mhchem` pour les équations de réaction, `siunitx` pour les unités) ; l'assistant
  liste ce dont il n'est pas sûr.
- « Donne un titre et des étiquettes à mes notes sans titre de cette semaine. »
- « Résume la séance de mardi avec les 4e B pour le cahier de textes. »
- « Retrouve la note où j'ai écrit la loi d'Ohm. »
- « Range les notes de chimie dans le classeur *Chimie/3e*. »
- « Exporte la fiche méthode sur la dilution en PDF dans mon dossier Documents. »
- Pendant le cours, Ardoise ouverte : « Ouvre la note de correction », « Projette la page », « Page suivante ».
- À la fin du cours : « Fais une version propre de la note que je viens d'écrire en mode direct. »

## Avant de commencer

- Ardoise a déjà été ouverte au moins une fois sur ce PC, et **une bibliothèque y est choisie** (le réglage est
  propre à chaque PC).
- Pour que l'assistant lise **votre écriture**, il faut la reconnaissance d'écriture manuscrite de Windows en
  français : Paramètres Windows > Heure et langue > Langue et région > Français > Options de langue > Écriture
  manuscrite. Sans elle, l'assistant voit quand même vos notes en image.
- Dans Ardoise, **Paramètres > Assistant (MCP)** affiche le texte exact à copier, avec le chemin de *votre*
  `ardoise.exe`. Copiez-le de là plutôt que de le retaper : c'est le plus sûr.

> Ardoise est portable : si vous déplacez son dossier, refaites la déclaration avec le nouveau chemin. Si vous avez
> plusieurs copies d'Ardoise, l'assistant utilise celle dont le chemin est déclaré.

## Déclarer Ardoise dans Claude Code

Une seule fois, dans **PowerShell** ou l'**invite de commandes** (le chemin est un exemple : prenez celui que donne
Paramètres > Assistant) :

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
2. Ajoutez l'entrée `ardoise` dans `mcpServers` (copiez-la depuis Paramètres > Assistant ; dans ce fichier, chaque
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

Avec `--read-only` (case **Lecture seule** dans Paramètres > Assistant, qui l'ajoute aux deux textes), l'assistant
peut **seulement lire** : lister et chercher les notes, lire leur texte reconnu, les voir en image. Il ne peut ni
créer, ni renommer, ni ranger, ni exporter, ni piloter Ardoise. L'invite `titrer_et_etiqueter` donne alors une liste de
titres et d'étiquettes à appliquer vous-même dans Ardoise.

**Conseillé chaque fois que l'assistant n'a besoin que de lire** (résumer, retrouver une note, préparer un document
à partir de vos notes).

## Les invites prêtes à l'emploi

Ardoise propose des invites toutes prêtes. Dans Claude Code, tapez `/` : elles apparaissent sous la forme
`/ardoise:nom (MCP)` ; on peut aussi taper `/mcp__ardoise__nom`. Les arguments se donnent à la suite, séparés par des
espaces. Leur nom suit la langue d'Ardoise :

| En français | En anglais | Rôle |
|---|---|---|
| `mettre_au_propre` | `clean_up` | une note mise au propre en Markdown, Word ou LaTeX |
| `titrer_et_etiqueter` | `title_and_tag` | titre et étiquettes pour les notes sans titre |
| `resume_seance` | `lesson_summary` | résumé d'une séance pour le cahier de textes |

Vous pouvez aussi simplement demander en français, sans invite : l'assistant choisit lui-même les outils d'Ardoise.

## Piloter Ardoise ouverte

Quatre outils agissent sur Ardoise **ouverte** (ils ne font rien si elle est fermée) :

- **état** (`app_state`) : ce qu'Ardoise affiche — la bibliothèque, l'éditeur (quelle note, modifications non
  enregistrées ou non), le mode direct (page en cours, présentation ouverte ou non) ;
- **ouvrir une note** (`open_note`) dans l'éditeur, comme un clic sur sa vignette. Ardoise refuse si la note ouverte
  a des modifications non enregistrées, si le mode direct est à l'écran ou si une fenêtre de dialogue est ouverte :
  rien n'est jamais perdu ;
- **présentation** (`presentation`) : projeter la page du mode direct sur l'écran choisi dans la fenêtre du mode
  direct, arrêter la projection, page suivante, précédente ou numéro de page. **C'est vous qui lancez le mode
  direct** (panneau de la tablette > Mode direct) : l'assistant ne touche jamais à la tablette ;
- **dernière note du mode direct** (`last_live_note`) : les notes écrites pendant le mode direct en cours ou le
  dernier (une note par page), pour en proposer une version propre à la fin du cours. Ardoise s'en souvient sur ce
  PC même après avoir été fermée.

L'assistant doit utiliser **la même copie** d'Ardoise que celle que vous ouvrez (même dossier).

## En cas de problème

| L'assistant dit… | Que faire |
|---|---|
| `noLibrary` : aucune bibliothèque | Ouvrez Ardoise sur ce PC et choisissez la bibliothèque. |
| `handwritingNoRecognizer` / `handwritingUnsupported` | Installez l'écriture manuscrite française de Windows (voir plus haut). Les images restent lisibles. |
| `appNotRunning` : « Ardoise is not open » | Ouvrez Ardoise — la copie dont le chemin est déclaré. Si elle est déjà ouverte, regardez la ligne « Liaison avec Ardoise ouverte » dans Paramètres > Assistant : elle dit si cette fenêtre peut être pilotée. |
| `appNotResponding` | Ardoise est occupée (export, fenêtre de dialogue) ou démarre encore : réessayez. |
| `appBusy` | Terminez d'abord dans Ardoise ce qui est en cours (enregistrer la note, fermer la fenêtre de dialogue, finir le mode direct). |
| `liveNotRunning` | Lancez le mode direct dans Ardoise avant de demander la présentation. |
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
  modifier le titre ou les étiquettes, découper, redresser les formes, déplacer) comme **destructifs**, ce qui
  l'incite à demander. Pour être sûr que rien ne change, déclarez Ardoise en **lecture seule** (`--read-only`).
- **Rien n'est jamais supprimé définitivement** : une note que l'assistant supprime va dans la corbeille d'Ardoise
  (gardée 30 jours), d'où vous pouvez la restaurer. Quand il redresse les formes d'une note, la version d'avant y est
  aussi, sous le nom « <titre> (avant redressement) ». Les exports n'écrasent jamais un fichier existant.
