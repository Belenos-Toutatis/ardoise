/**
 * The work under way in dialogs of the library (an export, an import, a tablet test) that closing the
 * window would cut short. A dialog says so while it runs:
 * `$effect(() => { if (running) return beginWork(); })`.
 */
let running = 0;

/** Counts one piece of work until the function it returns is called. */
export function beginWork(): () => void {
  running += 1;
  let ended = false;
  return () => {
    if (ended) return;
    ended = true;
    running -= 1;
  };
}

export function workInProgress(): boolean {
  return running > 0;
}
