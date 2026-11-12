// M17 journey, the sound while writing in the editor (docs/design/paper-projection-narration.md §7): « son pendant qu'on
// écrit dans l'éditeur », run with `--simulated-microphone` (the PC's own microphone is never switched on: Rust records
// a synthetic voice, `narration/simulated.rs`). Helpers in `m17-helpers.mjs`.
//
// On a demo note with ink: Son opens on « Pendant que j'écris »; the picker lists the simulated microphone; a recording
// with two pen strokes 1.5 s apart; « Arrêter » saves note and sound at once (no unsaved marker). In the file: an
// `audio.opus` in note time (`ARDOISE_BASE=note`) starting about a second after the old ink, the new strokes after the
// old ink and on the sound's clock (a stroke's note time less the sound's start is its pen-down wall time less the
// sound's first sample). The replay with the narration lasts to the sound's end and shows the first new stroke at its
// instant; the video estimate counts the narration; a new recording asks before replacing: « Garder l'ancienne »
// records nothing, « Remplacer » records a sound that starts after the newest stroke.
//
// Written without a run (packages D and F were not merged when this was written): the selectors are the doc's and
// vitest's (`Editor.sound.test.ts`); the lines that rely on package F's simulated microphone carry `M17-F: check`.
import { noteOnDisk, opusTags, penStroke } from './m17-helpers.mjs';
import { EDITOR_STROKES, text } from './journeys-m12-resume.mjs';
import { openTab, tabPanel } from './side-panel.mjs';

const j = JSON.stringify;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const SOUND = tabPanel('sound');
const strokeCount = `document.querySelectorAll(${j(EDITOR_STROKES)}).length`;
const canvasBox = `(() => { const r = document.querySelector('.editor svg.canvas').getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height }; })()`;
const phase = `(document.querySelector(${j(`${SOUND} .record`)})?.dataset.phase ?? null)`;
const invoke = (command, args = {}) => `window.__TAURI_INTERNALS__.invoke(${j(command)}, ${j(args)})`;

/** A horizontal line of `n` points across the canvas box at height `fy` (fractions of the box). */
const across = (box, fy, fx0 = 0.3, fx1 = 0.7, n = 20) =>
  Array.from({ length: n + 1 }, (_, i) => ({ x: box.x + box.w * (fx0 + ((fx1 - fx0) * i) / n), y: box.y + box.h * fy }));

/** The latest sample time of `strokes` (decoded from `strokes.bin`), -1 without any. */
const lastTime = (strokes) => strokes.reduce((last, s) => s.points.reduce((m, p) => Math.max(m, p.t), last), -1);

/** The part of a catalog text before its first `{placeholder}`: what a message with a variable starts with. */
const lead = (value) => value.split('{')[0];

/** Opens the recording panel of Son (unless it is open) and presses « Démarrer ». */
async function pressStart(app) {
  await openTab(app, 'sound');
  const start = `${SOUND} [data-action="narration-start"]`;
  if (!(await app.evaluate(`document.querySelector(${j(start)}) !== null`))) {
    await app.click(`${SOUND} [data-action="narration-open"]`);
    await app.waitFor(`document.querySelector(${j(start)}) !== null`, 'the start button of the recording');
  }
  await app.click(start);
}

/**
 * The wall clock (Unix ms, this process's clock) at which the recording under way began, from its elapsed time:
 * read in the middle of the round trip.
 */
async function recordingStartWall(app) {
  const before = Date.now();
  const status = await app.evaluate(invoke('narration_status'));
  const after = Date.now();
  if (status?.state !== 'recording') throw new Error(`the narration status is ${j(status)} while recording`);
  return (before + after) / 2 - status.elapsedMs;
}

/** Presses « Arrêter », then waits for the note and its sound to be saved (the notice, no unsaved marker). */
async function stopAndSave(app, l, path) {
  const before = noteOnDisk(path).audio;
  await app.click(`${SOUND} [data-action="narration-stop"]`);
  // The file itself first: the notice of an earlier recording may still be on screen, and the header says « Enregistré »
  // before the save that follows the stop has begun.
  const deadline = Date.now() + 20_000;
  for (;;) {
    let audio = null;
    try {
      audio = noteOnDisk(path).audio;
    } catch {
      // Being replaced (an atomic rename): read again.
    }
    if (audio && !(before && audio.equals(before))) break;
    if (Date.now() > deadline) throw new Error('timed out waiting for the new sound in the file');
    await sleep(100);
  }
  await app.waitFor(
    `(document.querySelector('.editor-notice')?.textContent ?? '').startsWith(${j(lead(l('editor.sound.saved')))})`,
    'the notice that the sound is saved with the strokes',
    20_000,
  );
  await app.waitFor(`document.querySelector('.editor')?.dataset.dirty === 'no'`, 'the note saved (no unsaved marker)');
  await app.waitFor(`document.querySelector(${j(`${SOUND} [data-slot="narration-stored"]`)}) !== null`, 'the stored narration in Son');
}

export const writingSoundJourneys = [
  {
    name: 'son pendant qu’on écrit dans l’éditeur',
    args: ['--simulated-microphone'],
    async run(app, check) {
      const l = (key) => text(app.language, key);
      let path;
      let before;
      let oldLast;
      let box;
      const downs = [];
      let soundWall;
      let first;

      await check('ouvrir une note de démonstration écrite : le dernier instant de son encre, lu dans le fichier', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length > 0`, 'the demo notes');
        await app.click('.note-card .card-open');
        await app.waitFor(`${strokeCount} > 0`, 'the strokes of the note');
        // The page is fitted once the editor has measured its stage.
        await sleep(400);
        path = await app.evaluate(`document.querySelector('.editor')?.dataset.path ?? null`);
        before = noteOnDisk(path);
        if (before.audio) throw new Error('the demo note already has a narration');
        oldLast = lastTime(before.strokes);
        if (!(oldLast > 0)) throw new Error(`the demo note's ink ends at ${oldLast}`);
        box = await app.evaluate(canvasBox);
      });

      await check('Son : « Pendant que j’écris » est le mode par défaut ; le choix des micros donne « Simulated microphone »', async () => {
        await openTab(app, 'sound');
        const mode = await app.evaluate(`(() => {
          const group = document.querySelector(${j(`${SOUND} [data-field="narration-mode"]`)});
          return group ? { role: group.getAttribute('role'), checked: group.querySelector('input:checked')?.value ?? null, labels: [...group.querySelectorAll('label')].map((x) => x.textContent.trim()) } : null;
        })()`);
        if (mode?.role !== 'radiogroup') throw new Error(`the mode switch is ${j(mode)}`);
        if (mode.checked !== 'writing') throw new Error(`the mode shown by default is ${j(mode.checked)}`);
        if (j(mode.labels) !== j([l('editor.sound.modeWriting'), l('editor.sound.modeReplay')])) throw new Error(`the modes are ${j(mode.labels)}`);
        const open = `${SOUND} [data-action="narration-open"]`;
        if (!(await app.evaluate(`(document.querySelector(${j(open)})?.textContent ?? '').includes(${j(l('editor.sound.recordWriting'))})`))) {
          throw new Error('the Son tab has no « Enregistrer en écrivant »');
        }
        await app.click(open);
        // M17-F: check (the simulated microphone is the one device listed, and the default).
        await app.waitFor(
          `[...document.querySelectorAll(${j(`${SOUND} select[data-field="microphone"] option`)})].some((o) => o.textContent.includes('Simulated microphone'))`,
          'the simulated microphone in the list',
          15_000,
        );
      });

      await check('Démarrer, deux traits au stylet à 1,5 s d’écart, 1 s de plus, Arrêter : note et son enregistrés ensemble', async () => {
        await app.click(`.toolbar button[title="${l('editor.modes.pen')}"]`).catch(() => {
          // The pen is the editor's tool by default.
        });
        await pressStart(app);
        await app.waitFor(`${phase} === 'recording'`, 'the recording', 15_000);
        await app.waitFor(`document.querySelector('[role="tab"][data-tab="sound"]')?.dataset.dot === 'rec'`, 'the red dot on Son');
        soundWall = await recordingStartWall(app);
        await sleep(300);
        downs.push(Date.now());
        await penStroke(app, across(box, 0.8));
        await sleep(1500);
        downs.push(Date.now());
        await penStroke(app, across(box, 0.88));
        await app.waitFor(`${strokeCount} === ${before.strokes.length + 2}`, 'the two strokes in the editor');
        await sleep(1000);
        await stopAndSave(app, l, path);
      });

      await check('le fichier : audio.opus en temps de la note, après l’encre ; les traits sur l’horloge du son, après l’ancienne encre', async () => {
        const saved = noteOnDisk(path);
        if (!saved.audio) throw new Error(`the note has no audio.opus (members ${j(saved.names)})`);
        const tags = opusTags(saved.audio);
        if (tags.BASE !== 'note') throw new Error(`the narration's base is ${j(tags.BASE)} (tags ${j(tags)})`);
        const start = Number(tags.START_MS);
        if (!(start >= oldLast + 500 && start <= oldLast + 1300)) {
          throw new Error(`the sound starts at ${start} ms, the old ink ends at ${oldLast} ms: expected within [${oldLast + 500}, ${oldLast + 1300}]`);
        }
        if (saved.strokes.length !== before.strokes.length + 2) throw new Error(`the file has ${saved.strokes.length} strokes, expected ${before.strokes.length + 2}`);
        const added = saved.strokes.slice(before.strokes.length);
        first = added[0].points[0].t;
        const second = added[1].points[0].t;
        if (!(first > oldLast)) throw new Error(`the first new stroke starts at ${first}, inside the old ink (ends ${oldLast})`);
        if (!(second > first)) throw new Error(`the new strokes are not in order: ${first}, ${second}`);
        // A stroke's note time less the sound's start = its pen-down wall time less the sound's first sample (M17-F: check).
        const offset = first - start;
        const wall = downs[0] - soundWall;
        if (Math.abs(offset - wall) > 150) {
          throw new Error(`the first stroke is ${offset} ms into the sound, drawn ${Math.round(wall)} ms after it began: more than 150 ms apart`);
        }
        const gap = second - first;
        const drawn = downs[1] - downs[0];
        if (Math.abs(gap - drawn) > 150) throw new Error(`the strokes are ${gap} ms apart in the note, ${drawn} ms apart on the wall clock`);
        // The old ink is untouched.
        if (j(saved.strokes.slice(0, before.strokes.length)) !== j(before.strokes)) throw new Error('the old ink changed');
      });

      await check('Relecture avec la narration : elle dure jusqu’à la fin du son, et montre le premier nouveau trait à son instant', async () => {
        const info = await app.evaluate(`${invoke('narration_info', { path })}`);
        if (info?.base !== 'note' || !(info.durationMs > 2000)) throw new Error(`the narration is ${j(info)}`);
        await app.key('r');
        await app.waitFor(`document.querySelector('.replay-bar') !== null`, 'the replay');
        await app.waitFor(`document.querySelector('[data-action="replay-narration"]') !== null`, 'the narration switch');
        await app.click('[data-action="replay-narration"]');
        await app.waitFor(`document.querySelector('.replay-bar audio') !== null || document.querySelector('audio') !== null`, 'the narration’s sound', 15_000);
        const max = Number(await app.evaluate(`document.querySelector('[data-action="replay-seek"]')?.max ?? 0`));
        if (!(max >= info.startMs + info.durationMs - 50)) throw new Error(`the replay lasts ${max} ms, the sound ends at ${info.startMs + info.durationMs} ms`);
        const seek = (ms) =>
          app.evaluate(`(() => {
            const el = document.querySelector('[data-action="replay-seek"]');
            el.value = String(${ms});
            el.dispatchEvent(new Event('input', { bubbles: true }));
            return true;
          })()`);
        // Speed 1, nothing cut: the replay instant of a sample is its note time.
        await seek(first - 150);
        await app.waitFor(`${strokeCount} === ${before.strokes.length}`, 'the page just before the first new stroke (without it)');
        await seek(first + 150);
        await app.waitFor(`${strokeCount} === ${before.strokes.length + 1}`, 'the page just after (with it)');
        await app.key('r');
        await app.waitFor(`document.querySelector('.replay-bar') === null`, 'the end of the replay');
      });

      await check('estimation de la vidéo MP4 avec la narration : la narration comptée, la vidéo jusqu’à la fin du son', async () => {
        const info = await app.evaluate(`${invoke('narration_info', { path })}`);
        const options = {
          format: 'mp4',
          resolution: '720p',
          frameRate: 30,
          timing: { kind: 'speed', speed: 1 },
          trimPauses: false,
          background: true,
          holdEndSeconds: 0,
          narration: true,
        };
        const estimate = await app.evaluate(`${invoke('estimate_video_export', { source: { path }, options })}`);
        if (!(estimate?.narrationMs > 0)) throw new Error(`the estimate has no narration: ${j(estimate)}`);
        if (Math.abs(estimate.narrationMs - info.durationMs) > 100) throw new Error(`the estimate counts ${estimate.narrationMs} ms of narration, the note has ${info.durationMs}`);
        if (!(estimate.durationMs >= info.startMs + info.durationMs - 100)) {
          throw new Error(`the video lasts ${estimate.durationMs} ms, the sound ends at ${info.startMs + info.durationMs} ms`);
        }
      });

      await check('enregistrer à nouveau : la question d’abord ; « Garder l’ancienne » n’enregistre rien', async () => {
        const sound = noteOnDisk(path).audio;
        await pressStart(app);
        const question = `${SOUND} [data-slot="replace-question"]`;
        await app.waitFor(`document.querySelector(${j(question)}) !== null`, 'the replace question');
        const said = await app.evaluate(`document.querySelector(${j(question)}).textContent`);
        if (!said.includes(lead(l('narration.record.replaceQuestion')))) throw new Error(`the question says ${j(said)}`);
        await app.click(`${SOUND} [data-action="narration-replace-no"]`);
        await app.waitFor(`document.querySelector(${j(question)}) === null`, 'the question to go');
        await sleep(500);
        if ((await app.evaluate(phase)) !== 'idle') throw new Error('a recording started after « Garder l’ancienne »');
        const status = await app.evaluate(invoke('narration_status'));
        if (status?.state === 'recording') throw new Error('Rust records after « Garder l’ancienne »');
        if (!noteOnDisk(path).audio.equals(sound)) throw new Error('the narration changed');
      });

      await check('« Remplacer » : un nouveau son, qui commence après le trait le plus récent', async () => {
        const newest = lastTime(noteOnDisk(path).strokes);
        await app.click(`${SOUND} [data-action="narration-start"]`);
        await app.waitFor(`document.querySelector(${j(`${SOUND} [data-slot="replace-question"]`)}) !== null`, 'the replace question');
        await app.click(`${SOUND} [data-action="narration-replace-yes"]`);
        await app.waitFor(`${phase} === 'recording'`, 'the new recording', 15_000);
        await sleep(1500);
        await stopAndSave(app, l, path);
        const saved = noteOnDisk(path);
        const tags = opusTags(saved.audio);
        const start = Number(tags.START_MS);
        if (tags.BASE !== 'note') throw new Error(`the new narration's base is ${j(tags.BASE)}`);
        if (!(start > newest)) throw new Error(`the new sound starts at ${start} ms, before the newest stroke (${newest} ms)`);
        if (!(start <= newest + 1300)) throw new Error(`the new sound starts at ${start} ms, long after the newest stroke (${newest} ms)`);
        if (saved.strokes.length !== before.strokes.length + 2) throw new Error('the strokes changed with the new sound');
      });
    },
  },
];
