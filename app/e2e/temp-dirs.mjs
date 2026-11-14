// Temporary folders of the journeys: every one is made here, so that none is left behind.
//
// A journey's folder holds a copy of the app and its WebView2 profile; right after the window closes, WebView2's own
// processes still hold files in it for a moment, and a removal tried at once often fails. So a folder that could not be
// removed is tried again at the end of the run (`removeTemps`), when those processes are gone. A failed journey keeps
// its folder and its capture (`keepTemp`); such folders, and those of a run that was killed, are removed by a later run
// once they are a day old (`removeStaleTemps`).
import { mkdtempSync, readdirSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const made = new Set();
/** The process ids of the apps the run started: each leaves its simulated tablets in `ardoise-simulation-<pid>-…`. */
const apps = new Set();
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** A new temporary folder whose name starts with `prefix` (`ardoise-e2e-…`, `ardoise-mcp-…`), removed at the end of the run. */
export function makeTemp(prefix) {
  const dir = mkdtempSync(join(tmpdir(), prefix));
  made.add(dir);
  return dir;
}

/** An app the run started: the folders of its simulated tablets go at the end of the run. */
export function trackApp(pid) {
  if (pid) apps.add(pid);
}

function removeSimulations() {
  if (apps.size === 0) return;
  let names = [];
  try {
    names = readdirSync(tmpdir());
  } catch {
    return;
  }
  for (const name of names) {
    const pid = /^ardoise-simulation-(\d+)-/.exec(name)?.[1];
    if (pid && apps.has(Number(pid))) remove(join(tmpdir(), name));
  }
}

/** `dir` is kept after the run: a failed journey's folder, with its capture. */
export function keepTemp(dir) {
  made.delete(dir);
}

function remove(dir) {
  try {
    rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
    return true;
  } catch {
    return false;
  }
}

/** Removes `dir` now if nothing holds it; otherwise it goes at the end of the run. */
export function removeTemp(dir) {
  if (remove(dir)) made.delete(dir);
}

/** The end of the run: every folder made and not kept is removed, waiting a little for the processes that hold them. */
export async function removeTemps() {
  for (let attempt = 0; attempt < 10 && made.size > 0; attempt++) {
    for (const dir of [...made]) if (remove(dir)) made.delete(dir);
    if (made.size > 0) await sleep(1000);
  }
  removeSimulations();
  if (made.size > 0) console.log(`  (${made.size} dossier(s) temporaire(s) encore retenu(s) par Windows : ${[...made].join(', ')})`);
}

/** The start of a run: the folders an earlier run left (failed journeys, a killed run) go once they are a day old. */
export function removeStaleTemps(maxAgeMs = 24 * 3600 * 1000) {
  let names = [];
  try {
    names = readdirSync(tmpdir());
  } catch {
    return;
  }
  for (const name of names) {
    if (!/^ardoise-(e2e|mcp|perf)-/.test(name)) continue;
    const dir = join(tmpdir(), name);
    try {
      if (Date.now() - statSync(dir).mtimeMs > maxAgeMs) remove(dir);
    } catch {
      // Gone meanwhile, or not ours to read.
    }
  }
}
