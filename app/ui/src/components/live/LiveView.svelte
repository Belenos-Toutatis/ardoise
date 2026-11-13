<script lang="ts">
  /**
   * Live mode and the classroom's control window (SPEC §4.2; docs/design/tablet-live-classroom.md,
   * "Interface"). Starts live mode when it opens (`live_start`; `live_resume` on the note `resume`
   * when one is given, M12: the sheet is never turned, and a notice says how to hold the tablet
   * for it and, if it changed elsewhere meanwhile, that the writing went on in a new note) and follows the session
   * (`live_subscribe`): toolbar (tool, colour, width), pages, layer, clear, blackboard, laser
   * pointer, the preview of the whole page with the projected area (drag to move it), zoom,
   * the presentation window (screen, project / stop), the measured latency, the narration (SPEC
   * §4.5: record the microphone during the session; it is cut per page and saved with the notes),
   * and "Finish" (the narration stops first, then `live_stop`, then `onclose`; what `live_stop` says
   * of the last write, a sheet that went on in a new note or a narration that could not join its
   * pages, keeps the view open until it is dismissed: the view is the only place that says it).
   *
   * Touching up (M12, this window only): the Select tool (V) lets the mouse and the pen choose strokes of
   * the preview with a lasso or a click; a bar then deletes them (Delete, Backspace), the colour swatches
   * recolour them (and only them: with a selection a swatch does not change the pen's colour), Esc
   * deselects first. Dragging inside the selection's frame moves it and dragging a corner resizes it, with the pen, a finger or the mouse
   * (`transformSelected`, one edit and one step of the history; the selection stays). Undo and Redo (Ctrl+Z, Ctrl+Y) go through the page's history: a delete, a change
   * of colour, « Clear the page », never what the tablet wrote. The selection is dropped when the page
   * or its revision changes, and with the Select tool when the session is over.
   *
   * Correcting during live (M14, SPEC §4.2 « Corriger pendant le direct »): the toolbar's style group is the
   * tablet's; the computer's group has Select (V) and Pen (P), exclusive or neither (then the preview moves
   * the projected area), its ink (`ComputerStyle`, remembered on this PC), Undo and Redo. With Pen the
   * computer's pen or mouse writes on the chosen layer; its eraser end erases strokes of that layer whatever
   * the tool (`LiveComputer`: one queue for every command acting on the page, so that they reach Rust in
   * order). The side panel opens on the layers zone (`LayersZone`, `live_layers`): a click chooses the layer
   * the computer acts on, the tablet icon the one the tablet writes on. Choosing another layer drops the
   * selection, hiding a layer drops its strokes from it; Ctrl+Z waits for a gesture under way to end (and goes
   * once the stroke just written is recorded); Esc drops it first. A merge, which Ctrl+Z cannot undo, is asked
   * first. « Undo the clearing » goes through the same queue, after the strokes and erasings asked for before.
   *
   * The side panel (M16; docs/design/side-panel.md, §8) is the shared tabbed rail (`SidePanel`): Calques (the layers
   * zone, open by default), Page and Fond (M17, below), Son (the narration), Projection (the projector, the projected
   * area with the blackboard and the laser pointer, the share on the network) and Aide (the keys, from `LIVE_SHORTCUTS`).
   * Above the open tab, whatever it is: the selection's bar (a selection reopens the pane) and the hints of the Select
   * and Pen tools. Its badges follow the layers (count, a hidden layer in use), the recording and the projection or share.
   *
   * Paper and image of the current page (M17, docs/design/paper-projection-narration.md §1.6): Page (`PaperBlock`: the
   * paper's colour, template and texture, `live_page_paper`; under the blackboard the swatches wait, the board's slate
   * keeps the page) and Fond (`BackgroundImageSection`: a file or a page of a PDF under the writing, its fit, quarter turn
   * and opacity, `live_page_image`; no offer to turn the page, a page of live mode follows the tablet). Both wait while
   * the session does not run, act on the page on screen (the page index is read when the click is made, or when the file
   * is chosen for an image: a page that changed meanwhile is refused, `liveEditStale`) and are not undone by Ctrl+Z.
   * The class's remote keeps working in them: Page Up and Page Down turn the page even with a select or the slider
   * focused (`liveKeyAction`), and a choice made with the mouse gives the focus back.
   */
  import { onMount, untrack } from 'svelte';
  import { api, errorMessage, type AppState } from '../../lib/api';
  import { bridgeScreens, drivePresentation, until } from '../../lib/app-bridge.svelte';
  import { refit, turnImageAlone, type ImageFit } from '../../lib/background';
  import { INKSPACE_COLOR_NAMES, sameColor } from '../../lib/editor/colors';
  import { currentLocale, t } from '../../lib/i18n.svelte';
  import { TOOLS, loadInk, type Ink, type Tool } from '../../lib/ink';
  import { LatencyRecorder, exposeLatency } from '../../lib/latency';
  import type {
    LiveCommandKind,
    LiveLayerOp,
    LivePageImageOp,
    LiveStopped,
    LiveStyle,
    LiveViewState,
    MonitorInfo,
    SheetFork,
  } from '../../lib/live';
  import { layerWarnings } from '../../lib/layers-zone';
  import { LiveComputer } from '../../lib/live-computer';
  import { loadComputerStyle, saveComputerStyle } from '../../lib/live-computer-style';
  import { computerErrorText, pageErrorText } from '../../lib/live-errors';
  import {
    changeView,
    runKeyAction,
    sendCommand,
    showPageWidth,
    showWholePage,
    toggleBoard,
    toggleLaser,
    zoomStep,
    type LiveActions,
  } from '../../lib/live-actions';
  import { isSpaceKey, isTypingTarget, liveEditKeyAction, liveKeyAction, type LiveEditKeyAction } from '../../lib/live-keys';
  import { LiveSelection } from '../../lib/live-select.svelte';
  import { LIVE_SHORTCUTS } from '../../lib/live-shortcuts';
  import { LiveState, subscribeLive } from '../../lib/live-state.svelte';
  import { deleteSelected, recolourSelected, stepHistory, transformSelected, type TouchUp } from '../../lib/live-touch-up';
  import { MAX_ZOOM, MIN_ZOOM, aspectOf, defaultMonitor } from '../../lib/live-view';
  import { INKSPACE_COLORS, cssColor, type Background, type BackgroundImage, type Rgba } from '../../lib/note';
  import { ORIENTATIONS, type Orientation } from '../../lib/orientation';
  import { share } from '../../lib/share-state.svelte';
  import { SidePanelState, type SideBadges } from '../../lib/side-panel.svelte';
  import { tablet } from '../../lib/tablet-state.svelte';
  import type { NarrationSaved } from '../../lib/video';
  import { askBeforeQuitting, requestWindowClose } from '../../lib/editor/window-close';
  import Modal from '../Modal.svelte';
  import RecordPanel from '../narration/RecordPanel.svelte';
  import ProjectorBlock from '../projection/ProjectorBlock.svelte';
  import ZoneBlock from '../projection/ZoneBlock.svelte';
  import ShareBlock from '../share/ShareBlock.svelte';
  import ShortcutsHelp from '../side/ShortcutsHelp.svelte';
  import SidePanel from '../side/SidePanel.svelte';
  import OrientationIcon from '../tablet/OrientationIcon.svelte';
  import { NarrationRecorder } from '../narration/recorder.svelte';
  import LayersZone from '../layers/LayersZone.svelte';
  import BackgroundImageSection from '../editor/BackgroundImageSection.svelte';
  import PaperBlock from '../editor/PaperBlock.svelte';
  import ComputerStyle from './ComputerStyle.svelte';
  import LiveCanvas from './LiveCanvas.svelte';

  let {
    app,
    resume = null,
    onclose,
    onchange,
  }: {
    app: AppState;
    /** The note of the library to resume live mode on (M12); null: a new session on a blank page. */
    resume?: string | null;
    /** The view is done; `notice` is something the library says on coming back (a recording that was lost). */
    onclose: (notice?: string) => void;
    /** The app state changed (the microphone chosen here): the screen holding it takes the new one. */
    onchange?: (next: AppState) => void;
  } = $props();

  /** Nominal width per ink tool (device units), as in the editor. */
  const DEFAULT_WIDTHS: Record<Tool, number> = {
    ballpoint: 60,
    fountainPen: 80,
    feltTip: 120,
    pencil: 50,
    brush: 150,
    highlighter: 500,
    chalk: 150,
  };
  /** The Inkspace palette, then white chalk and yellow for the blackboard. */
  const COLORS: { color: Rgba; label: string }[] = [
    ...INKSPACE_COLORS.map((color, i) => ({ color, label: `editor.colors.${INKSPACE_COLOR_NAMES[i]}` })),
    { color: { r: 255, g: 255, b: 255, a: 255 }, label: 'live.colors.white' },
    { color: { r: 255, g: 214, b: 0, a: 255 }, label: 'live.colors.yellow' },
  ];
  /** Why stopping the narration may fail without anything being lost: there was no sound to keep. */
  const NOTHING_TO_KEEP = ['emptyRecording', 'microphoneSilent'];
  /** How long « Undo the clearing » stays offered after « Clear the page ». */
  const UNDO_CLEAR_MS = 8000;
  /** How often the latency shown is refreshed. */
  const LATENCY_REFRESH_MS = 500;

  const live = new LiveState();
  const selection = new LiveSelection();
  const latency = new LatencyRecorder();
  const recorder = new NarrationRecorder();
  /** The side panel's tab and fold, remembered on this PC (`ardoise.sidePanel.live`). */
  const side = new SidePanelState('live');
  /** The computer's ink (M14): this PC's, remembered in the browser's storage. */
  let computerStyle = $state.raw<LiveStyle>(loadComputerStyle());
  /** The computer's ink panel shows. */
  let computerStyleOpen = $state(false);
  /** The Pen tool is on (exclusive with Select). */
  let penOn = $state(false);
  /** Width last used per tool by the computer, in this session. */
  const computerWidths: Partial<Record<Tool, number>> = {};
  let canvas = $state<ReturnType<typeof LiveCanvas>>();
  /** The computer's queue, pen and eraser: every command acting on the page goes through its queue. */
  const computer = new LiveComputer({
    live,
    commands: api,
    style: () => computerStyle,
    onerror: (error, from) => (notice = computerErrorText(error, from)),
  });
  /** A merge of the layers zone waiting for its answer (Ctrl+Z cannot undo it). */
  let mergeAsk = $state.raw<{ layer: number; upper: string; lower: string; reveals: boolean } | null>(null);

  let ink = $state.raw<Ink | null>(null);
  let starting = $state(true);
  let startError = $state<string | null>(null);
  let stopping = $state(false);
  let stopError = $state<string | null>(null);
  let notice = $state<string | null>(null);
  /** Title of the sheet resumed ('' when it has none, or on a new session). */
  let sheetTitle = $state('');
  /** `live_stop` answered and had something to say: the view stays until it is all dismissed. */
  let stopped = $state(false);
  /** Why the recording still running at the stop could not join its pages, as a sentence. */
  let stoppedNarration = $state<string | null>(null);
  let monitors = $state.raw<MonitorInfo[]>([]);
  let monitorKey = $state('');
  let monitorsLoaded = $state(false);
  let projecting = $state(false);
  let projectionError = $state<string | null>(null);
  let latencyMedian = $state<number | null>(null);
  let latencyP95 = $state<number | null>(null);
  let latencyUi = $state<number | null>(null);
  let latencyCount = $state(0);
  let narrationOpen = $state(false);
  /** Esc asked to end live mode: waiting for the answer. */
  let confirmLeave = $state(false);
  /** Ctrl + Q asked to quit Ardoise: waiting for the answer. */
  let confirmQuit = $state(false);
  onMount(() =>
    askBeforeQuitting(() => {
      if (stopping || stopped) void requestWindowClose();
      else confirmQuit = true;
    }),
  );
  /** « Undo the clearing » is offered. */
  let undoClear = $state(false);
  let undoTimer: ReturnType<typeof setTimeout> | undefined;
  /** The view was closed: a `live_start` still pending then stops the session it starts. */
  let destroyed = false;
  /** What the last narration recording saved; null before one, or after a cancelled one. */
  let narrated = $state.raw<NarrationSaved | null>(null);
  /** The microphone chosen here, until the app state says so. */
  let chosenMicrophone = $state<string | null | undefined>(undefined);
  /** Width last used per tool in this session. */
  const widths: Partial<Record<Tool, number>> = {};

  const sessionState = $derived(live.ready ? live.state : 'idle');
  /** The computer's tool on the preview. */
  const tool = $derived(selection.active ? 'select' : penOn ? 'pen' : 'none');
  const selectedMonitor = $derived(monitors.find((m) => m.key === monitorKey) ?? null);
  const aspect = $derived(aspectOf(live.presentation ?? selectedMonitor));
  const microphone = $derived(chosenMicrophone === undefined ? app.microphone : chosenMicrophone);
  const narratedText = $derived(
    narrated === null
      ? null
      : narrated.pages === 0
        ? t('live.narration.savedNone')
        : narrated.pages === 1
          ? t('live.narration.savedOne')
          : t('live.narration.savedMany', { count: narrated.pages }),
  );
  /**
   * The rail's badges: the layers' count and an orange dot when the tablet's or the chosen layer is hidden (what the
   * zone's own notice says), red while recording, blue while the projector is on or a share runs. They read the layers,
   * the recorder and the projection, never the strokes: the panel does not redraw the preview.
   */
  const badges = $derived<SideBadges>({
    layers: {
      ...(live.layers.length > 0 ? { count: live.layers.length } : {}),
      ...(layerWarnings(live.layers, live.computerLayer, live.layer).length > 0 ? { dot: 'warn' as const } : {}),
    },
    sound: recorder.phase === 'recording' ? { dot: 'rec' } : {},
    projection: live.presentation !== null || share.status.active ? { dot: 'live' } : {},
  });
  /** The sheet is resumed and cannot be turned: the quick position buttons wait for the next page. */
  const locked = $derived(!live.canTurnPage);
  /**
   * The Xiaomi blackboard writes the live page: it is held one way only (its portrait page is never turned), so the
   * position switch and the hint about the Slate's button do not apply. The session says which tablet it writes with
   * (kept while it reconnects, when no tablet is connected); until it does, the tablet that is connected.
   */
  const onBoard = $derived((live.tablet ?? tablet.active) === 'blackboard');
  /** What the waiting screen asks for: depends on which tablets are known. */
  const waitingKey = $derived(tablet.board ? (tablet.slate ? 'titleBoth' : 'titleBoard') : 'title');
  /** How to hold the tablet for the resumed sheet, when that is not how the setting says. */
  const hold = $derived(
    !onBoard &&
      locked &&
      sessionState !== 'idle' &&
      tablet.status &&
      tablet.status.options.orientation !== live.orientation
      ? live.orientation
      : null,
  );
  /** The page's paper as the snapshot gives it: its own colour (the blackboard's slate is not a choice), its template, its texture. */
  const shownBackground = $derived<Background>({ ...live.background, paper: live.pagePaper });
  /**
   * The paper last asked for in the Page tab, until a snapshot gives it back (or it is refused, or the page changes):
   * a second choice made before then builds on the first instead of on the snapshot before it.
   */
  let askedPaper = $state.raw<{ pageIndex: number; background: Background } | null>(null);
  /** The page's paper as the Page tab shows it. */
  const pageBackground = $derived<Background>(
    askedPaper !== null && askedPaper.pageIndex === live.pageIndex ? askedPaper.background : shownBackground,
  );
  $effect(() => {
    const asked = askedPaper;
    if (asked === null) return;
    if (asked.pageIndex !== live.pageIndex || sameBackground(asked.background, shownBackground)) askedPaper = null;
  });
  const savedCount = $derived(live.saved.length);
  const savedText = $derived(
    savedCount === 0 ? t('live.saved.none') : savedCount === 1 ? t('live.saved.one') : t('live.saved.many', { count: savedCount }),
  );
  const pageLabel = $derived(t('live.pages.position', { n: live.pageIndex + 1, count: live.pageCount }));
  const decimal = $derived(new Intl.NumberFormat(currentLocale(), { minimumFractionDigits: 1, maximumFractionDigits: 1 }));
  /** p95 and the interface's share, in the latency's tooltip. */
  const latencyDetail = $derived(
    latencyMedian === null
      ? ''
      : t('live.latency.detail', {
          count: latencyCount,
          p95: latencyP95 === null ? t('live.latency.none') : decimal.format(latencyP95),
          ui: latencyUi === null ? t('live.latency.none') : decimal.format(latencyUi),
        }),
  );

  const actions: LiveActions = {
    live,
    commands: api,
    aspect: () => aspect,
    onerror: (error) => (notice = t('live.commandFailed', { message: errorMessage(error) })),
    onCleared: offerUndo,
  };

  const touchUp: TouchUp = {
    live,
    selection,
    commands: api,
    queue: computer.queue,
    onerror: (error) => (notice = errorMessage(error)),
    onhistory: dropUndo,
  };

  // A selection belongs to a page and to its revision: another page, a delete or an undo drops it.
  $effect(() => {
    selection.follow(live);
  });

  // The Select and Pen tools belong to a running session: the edits are refused (`liveNotRunning`) at idle.
  $effect(() => {
    if (sessionState === 'idle') {
      untrack(() => {
        selection.toggle(false);
        penOn = false;
      });
    }
  });

  // A selection that appears reopens the side panel, on the tab shown, where its bar is (not at every change of it).
  let hadSelection = false;
  $effect(() => {
    const any = selection.any;
    if (any && !hadSelection) untrack(() => side.open());
    hadSelection = any;
  });

  // The selection belongs to the chosen layer: choosing another one drops it (M14).
  $effect(() => {
    void live.computerLayer;
    untrack(() => selection.clear());
  });

  // The offer concerns the page that was cleared: it goes when the page changes.
  $effect(() => {
    void live.pageIndex;
    dropUndo();
  });

  // Nothing left to undo (a merge dropped the page's history): the clearing cannot come back either.
  $effect(() => {
    if (!live.canUndo) untrack(dropUndo);
  });

  onMount(() => {
    let alive = true;
    // The share on the network shows the live sheet while this view is open (the library says
    // « waiting » again when it is back, `Home.svelte`).
    share.showLive();
    exposeLatency(latency);
    // The position set for the tablet is compared with how the resumed sheet is held.
    tablet.start();
    loadInk()
      .then((module) => {
        if (alive) ink = module;
      })
      .catch((error: unknown) => {
        if (alive) notice = errorMessage(error);
      });
    void (async () => {
      try {
        await subscribeLive(live, api.liveSubscribe);
      } catch {
        // `live_start` gives the session too; batches then follow it.
      }
      if (alive) await start();
    })();
    void refreshMonitors();
    const timer = setInterval(refreshLatency, LATENCY_REFRESH_MS);
    // The assistant's bridge drives the projection and the pages as these buttons and keys do.
    const unregister = bridgeScreens.register('live', {
      presentation: (action, page, abandoned) =>
        drivePresentation(
          {
            get pageIndex() {
              return live.pageIndex;
            },
            get pageCount() {
              return live.pageCount;
            },
            get presentation() {
              return live.presentation;
            },
            // The assistant's choice of screen is not saved: no bridge request changes a setting.
            project: () => openProjection(false),
            stopProjecting: closeProjection,
            command: (kind) => api.liveCommand({ kind }),
            until,
          },
          action,
          page,
          abandoned,
        ),
    });
    return () => {
      unregister();
      alive = false;
      destroyed = true;
      clearInterval(timer);
      clearTimeout(undoTimer);
      computer.dispose();
      live.close();
      recorder.dispose();
      // A recording is never thrown away silently: the pages get the sound recorded so far. (The
      // ways out of the view stop it first and say what went wrong: `releaseRecording`.)
      if (recorder.phase === 'recording') void api.narrationStop(true, null, null, null).catch(() => {});
    };
  });

  // A resume accepted while the tablet was not there yet, then refused when it connected (it cannot write on this sheet):
  // the same banner as a refused `live_resume`, with the same ways out (try again, back to the library).
  $effect(() => {
    if (live.refused) startError = errorMessage(live.refused);
  });

  async function start() {
    starting = true;
    startError = null;
    live.clearRefused();
    try {
      const snapshot = resume === null ? await api.liveStart() : await api.liveResume(resume);
      if (destroyed) {
        // The view was closed while the session started: nobody would stop it.
        void api.liveStop().catch(() => {});
        return;
      }
      if (resume !== null) sheetTitle = snapshot.page.title.trim();
      live.adoptStart(snapshot);
    } catch (error) {
      if (!destroyed) startError = errorMessage(error);
    } finally {
      starting = false;
    }
  }

  function refreshLatency() {
    latencyMedian = latency.median();
    latencyP95 = latency.p95();
    latencyUi = latency.uiMedian();
    latencyCount = latency.size;
  }

  async function chooseMicrophone(device: string | null) {
    chosenMicrophone = device;
    try {
      const next = await api.setMicrophone(device);
      chosenMicrophone = next.microphone;
      onchange?.(next);
    } catch (error) {
      chosenMicrophone = undefined;
      notice = errorMessage(error);
    }
  }

  async function startNarration() {
    narrated = null;
    await recorder.start(microphone, { kind: 'live' });
  }

  /** Stops the recording; saved, the sound is cut per page and written with the pages. */
  async function stopNarration(save: boolean) {
    // Closing the panel while it is still busy keeps its microphone picker from listening again.
    if (save) narrationOpen = false;
    const result = await recorder.stop(save, null);
    if (result.ok) narrated = result.saved;
  }

  async function finish() {
    if (stopping) return;
    stopping = true;
    stopError = null;
    // The narration is handed to the session, which writes it with the pages: it stops first.
    if (recorder.phase === 'recording') {
      narrationOpen = false;
      const result = await recorder.stop(true, null);
      if (!result.ok && !NOTHING_TO_KEEP.includes(result.code ?? '')) {
        stopError = recorder.error;
        stopping = false;
        return;
      }
      // A recording with nothing in it (silent or blocked microphone) is nothing to keep: leaving goes on.
      narrated = result.saved;
    }
    let answer: LiveStopped | null = null;
    try {
      if (live.presentation) {
        await api.closePresentation().catch(() => {
          // Rust closes it with the session anyway.
        });
      }
      answer = await api.liveStop();
    } catch (error) {
      const code = (error as { code?: unknown } | null)?.code;
      if (code !== 'liveNotRunning') {
        stopError = errorMessage(error);
        stopping = false;
        return;
      }
    }
    leaveAfterStop(answer);
  }

  /** The view goes: the library is shown again, with `message` if there is one to say. */
  function leave(message?: string) {
    live.close();
    onclose(message);
  }

  /**
   * The session is over. What its last write did is only told here (the view closes on this answer):
   * a sheet that went on in a new note that this window was not told of, or a narration that could
   * not join its pages. Then the view stays, with the notices, until they are all dismissed.
   */
  function leaveAfterStop(answer: LiveStopped | null, lost: string | null = null) {
    const unseen = answer ? live.adoptForks(answer.forks) : [];
    const problem = answer?.narration ? errorMessage(answer.narration) : lost;
    if (unseen.length === 0 && problem === null) {
      leave();
      return;
    }
    stoppedNarration = problem;
    stopError = null;
    stopped = true;
    stopping = false;
  }

  /** Once the session is over and every notice is dismissed, the view closes. */
  function leaveWhenDismissed() {
    if (stopped && live.forks.length === 0 && stoppedNarration === null) leave();
  }

  function dismissFork(fork: SheetFork) {
    live.dismissFork(fork);
    leaveWhenDismissed();
  }

  function dismissStoppedNarration() {
    stoppedNarration = null;
    leaveWhenDismissed();
  }

  /**
   * A recording still running as the view goes is stopped, its sound kept (nothing is thrown away
   * silently). Null when it went well or there was nothing to keep, otherwise a sentence for the library.
   */
  async function releaseRecording(): Promise<string | null> {
    if (recorder.phase !== 'recording') return null;
    const result = await recorder.stop(true, null);
    if (result.ok || NOTHING_TO_KEEP.includes(result.code ?? '')) return null;
    return t('live.recordingLost', { message: recorder.error ?? '' });
  }

  /**
   * "Close anyway" after a failure: the projection and the Rust session are stopped as well as
   * can be (a failed narration or a failed start must not leave the projector full screen and
   * the session running with nobody to see it), then the view closes whatever happens.
   */
  async function closeAnyway() {
    if (stopping) return;
    stopping = true;
    const lost = await releaseRecording();
    await api.closePresentation().catch(() => {});
    const answer = await api.liveStop().catch(() => null);
    leaveAfterStop(answer, lost);
  }

  /**
   * "Back to the library" after a refused resume: nothing was started on this side (a session
   * that was running goes on), so only the view closes.
   */
  function leaveRefused() {
    if (recorder.phase !== 'recording') leave();
    else void releaseRecording().then((lost) => leave(lost ?? undefined));
  }

  function offerUndo() {
    clearTimeout(undoTimer);
    undoClear = true;
    undoTimer = setTimeout(() => (undoClear = false), UNDO_CLEAR_MS);
  }

  function dropUndo() {
    clearTimeout(undoTimer);
    undoClear = false;
  }

  /** « Undo the clearing »: through the queue, after the strokes and erasings asked for before it. */
  async function undoClearing() {
    dropUndo();
    computer.historyQueued();
    try {
      if (!(await computer.queue.run(() => api.liveRestoreCleared()))) notice = t('live.clearUndoFailed');
    } catch (error) {
      notice = errorMessage(error);
    }
  }

  /**
   * Esc: closes the projection when it is open (as in a slideshow), otherwise asks before
   * ending live mode, which is not undone by a second press.
   */
  function escape() {
    if (live.presentation) void stopProjecting();
    else if (!stopping && !stopped) confirmLeave = true;
  }

  function command(kind: LiveCommandKind) {
    notice = null;
    sendCommand(actions, kind);
  }

  /** The quick switch of the tablet's position: applies at once, the snapshot that follows shows it. */
  function setOrientation(orientation: Orientation) {
    if (locked || orientation === live.orientation) return;
    notice = null;
    api.tabletSetOrientation(orientation).catch((error: unknown) => actions.onerror(error));
  }

  function setStyle(style: LiveStyle) {
    live.changeStyle(style, api.liveSetStyle).catch((error: unknown) => actions.onerror(error));
  }

  function setTool(tool: Tool) {
    const style = live.style;
    widths[style.tool] = style.width;
    setStyle({ ...style, tool, width: widths[tool] ?? DEFAULT_WIDTHS[tool] });
  }

  /** A swatch: recolours the selection if there is one, otherwise it is the pen's colour. */
  function pickColor(color: Rgba) {
    const next = { ...color };
    // With a selection the colour is for it only: what the tablet writes next keeps its colour, so
    // that a correction made in front of the class does not change the ink under the pen.
    if (selection.any) void recolourSelected(touchUp, next);
    else setStyle({ ...live.style, color: next });
  }

  function undoEdit(again = false) {
    // A gesture under way commits after the step it would undo: Ctrl+Z waits for it to end.
    if (computer.busy || canvas?.gripping()) return;
    // The stroke just written is a step once its `end` is recorded: the undo goes after it in the queue.
    if (again ? !live.canRedo : !live.canUndo && !computer.strokeEnding) return;
    computer.historyQueued();
    void stepHistory(touchUp, again);
  }

  /** The Select tool on or off; on, the Pen tool goes off. */
  function toggleSelect(on = !selection.active) {
    selection.toggle(on);
    if (on) penOn = false;
  }

  /** The Pen tool on or off; on, the Select tool goes off, and with it the selection. */
  function togglePen(on = !penOn) {
    penOn = on;
    if (on) selection.toggle(false);
  }

  /** The computer's ink changed in its panel: kept, and remembered on this PC. A new tool takes its last width. */
  function setComputerStyle(next: LiveStyle) {
    let style = next;
    if (next.tool !== computerStyle.tool) {
      computerWidths[computerStyle.tool] = computerStyle.width;
      style = { ...next, width: computerWidths[next.tool] ?? DEFAULT_WIDTHS[next.tool] };
    }
    computerStyle = style;
    saveComputerStyle(style);
  }

  /** An act in the layers zone: through the computer's queue, after its strokes and edits; a refusal is told. */
  function layerOp(op: LiveLayerOp) {
    notice = null;
    const pageIndex = live.pageIndex;
    computer.queue.run(() => api.liveLayers(pageIndex, op)).catch((error: unknown) => (notice = computerErrorText(error, 'layers')));
  }

  /**
   * « ⤓ »: asks first. Ctrl+Z cannot undo a merge during live (it drops the page's history), and merging a shown
   * layer into a hidden one shows the hidden ink at once, on the projector and on the share.
   */
  function askMerge(layer: number) {
    const upper = live.layers[layer];
    const lower = live.layers[layer - 1];
    if (!upper || !lower) return;
    mergeAsk = { layer, upper: upper.name, lower: lower.name, reveals: !lower.visible && upper.visible };
  }

  function confirmMerge() {
    const ask = mergeAsk;
    mergeAsk = null;
    if (ask) layerOp({ kind: 'merge', layer: ask.layer });
  }

  /** The pen came down on a hidden layer: nothing is written, and the window says why. */
  function penBlocked() {
    const name = live.layers[live.computerLayer]?.name ?? '';
    notice = t('layers.warn.chosenHidden', { name });
  }

  function sameBackground(a: Background, b: Background): boolean {
    return JSON.stringify(a.paper) === JSON.stringify(b.paper) && a.template === b.template && a.texture === b.texture;
  }

  /** The Page tab: the paper of the page on screen (`pageIndex` is read now, at the click). */
  function setPaper(background: Background) {
    notice = null;
    const pageIndex = live.pageIndex;
    const asked = { pageIndex, background };
    askedPaper = asked;
    api.livePagePaper(pageIndex, background).catch((error: unknown) => {
      if (askedPaper === asked) askedPaper = null;
      notice = pageErrorText(error, 'paper');
    });
  }

  /** The page on screen when the last click in the Fond tab was made (the choice of a file, which takes a while). */
  let imageFrom: number | null = null;
  /** The last thing done in the side tabs was a pointer's (a choice made with a key keeps its focus). */
  let pointerChoice = false;

  /** An act in the Fond tab: refused as a whole, told in a notice; resolved once Rust has answered. */
  async function pageImage(op: LivePageImageOp, pageIndex = live.pageIndex): Promise<void> {
    notice = null;
    try {
      await api.livePageImage(pageIndex, op);
    } catch (error) {
      notice = pageErrorText(error, 'image');
    }
  }

  /** The image's description changed (fit, quarter turn, opacity): the same pixels, placed otherwise. */
  function placeImage(change: (image: BackgroundImage, pixels: { width: number; height: number }) => BackgroundImage) {
    const image = live.pageImage;
    const pixels = live.imagePixels;
    if (image && pixels) void pageImage({ kind: 'place', image: change(image, pixels) });
  }

  /**
   * A choice made in a select or with the slider of the Page and Fond tabs by the mouse gives the focus back, so that
   * the class's remote (Page Up, Page Down: they turn the page whoever has the focus, but the arrows are the control's)
   * and the keys of live mode go on at once.
   */
  function releaseFocus(e: Event) {
    const el = e.target;
    if (pointerChoice && (el instanceof HTMLSelectElement || (el instanceof HTMLInputElement && el.type === 'range'))) el.blur();
  }

  function runEditKey(action: LiveEditKeyAction) {
    switch (action.kind) {
      case 'select':
        toggleSelect();
        break;
      case 'pen':
        togglePen();
        break;
      case 'deselect':
        selection.clear();
        break;
      case 'deleteSelection':
        void deleteSelected(touchUp);
        break;
      case 'undo':
        undoEdit();
        break;
      case 'redo':
        undoEdit(true);
        break;
    }
  }

  function setView(view: LiveViewState) {
    changeView(actions, view);
  }

  async function refreshMonitors() {
    try {
      monitors = await api.listMonitors();
      if (!monitors.some((m) => m.key === monitorKey)) monitorKey = defaultMonitor(monitors)?.key ?? '';
    } catch (error) {
      projectionError = errorMessage(error);
    } finally {
      monitorsLoaded = true;
    }
  }

  /**
   * "Project": the presentation window on the chosen screen, saved as this PC's choice unless
   * `remember` is false; a failure is shown, and thrown.
   */
  async function openProjection(remember = true): Promise<MonitorInfo> {
    projecting = true;
    projectionError = null;
    try {
      const shown = await api.openPresentation(monitorKey || null, remember);
      monitorKey = shown.key;
      return shown;
    } catch (error) {
      projectionError = errorMessage(error);
      throw error;
    } finally {
      projecting = false;
    }
  }

  async function project() {
    await openProjection().catch(() => {});
  }

  /** "Stop projecting"; a failure is shown, and thrown. */
  async function closeProjection(): Promise<void> {
    projectionError = null;
    try {
      await api.closePresentation();
    } catch (error) {
      projectionError = errorMessage(error);
      throw error;
    }
  }

  async function stopProjecting() {
    await closeProjection().catch(() => {});
  }

  function onkeydown(e: KeyboardEvent) {
    // The question about ending live mode has its own keys.
    if (e.defaultPrevented || confirmLeave || confirmQuit || mergeAsk || share.panelOpen) return;
    if (e.key === 'Escape') {
      // Esc drops a gesture of the computer's pen or eraser under way before anything else, then closes
      // the computer's ink panel.
      if (canvas?.cancelGesture() || computer.cancel()) {
        e.preventDefault();
        return;
      }
      if (computerStyleOpen) {
        e.preventDefault();
        computerStyleOpen = false;
        return;
      }
    }
    // Touching up first: V, undo and redo, and Delete and Esc while something is selected. Only while
    // a session runs, like the Select button (the edits are refused once it is over).
    const edit = live.ready && sessionState !== 'idle' ? liveEditKeyAction(e, selection.any) : null;
    if (edit) {
      e.preventDefault();
      runEditKey(edit);
      return;
    }
    const action = liveKeyAction(e);
    if (!action) return;
    e.preventDefault();
    runKeyAction(actions, action, escape);
  }

  /** Space is "next page" already on key down: a focused button must not also be pressed on key up. */
  function onkeyup(e: KeyboardEvent) {
    if (isSpaceKey(e.key) && !confirmLeave && !confirmQuit && !mergeAsk && !share.panelOpen && !isTypingTarget(e.target)) e.preventDefault();
  }

  const mm = (units: number) => (units / 100).toFixed(units < 100 ? 2 : 1);
  const ms = (value: number | null) => (value === null ? t('live.latency.none') : t('live.latency.value', { ms: decimal.format(value) }));
</script>

<svelte:window {onkeydown} {onkeyup} />

<section class="live-view" data-state={sessionState} aria-label={t('live.title')}>
  <header class="live-top">
    <h2>{t('live.title')}</h2>
    <span class="live-state" data-state={sessionState} role="status">
      <span class="dot" aria-hidden="true"></span>
      {starting && !live.ready ? t('live.starting') : t(`live.state.${sessionState}`)}
    </span>
    <span class="latency" title={latencyDetail ? `${t('live.latency.hint')} ${latencyDetail}` : t('live.latency.hint')}>
      <span class="muted">{t('live.latency.label')}</span>
      <strong data-latency-median={latencyMedian === null ? '' : latencyMedian.toFixed(1)}>{ms(latencyMedian)}</strong>
      {#if latencyDetail}
        <span class="visually-hidden">{latencyDetail}</span>
      {/if}
    </span>
    <span class="saved muted" data-saved={savedCount}>{savedText}</span>
    {#if stopped}
      <!-- The session is over and a notice is waiting: this closes the view and the notices with it. -->
      <button type="button" class="button primary" data-action="stopped-back" onclick={() => leave()}>
        {t('live.resume.back')}
      </button>
    {:else}
      <button
        type="button"
        class="button primary"
        data-action="stop-live"
        disabled={stopping || starting}
        title={t('live.stopHint')}
        onclick={finish}
      >
        {stopping ? t('live.stopping') : t('live.stop')}
      </button>
    {/if}
  </header>

  {#if startError}
    <div class="banner error live-banner" role="alert" data-slot={resume === null ? 'start-failed' : 'resume-failed'}>
      <strong>{resume === null ? t('live.startFailed') : t('live.resume.failed')}</strong>
      <p>{startError}</p>
      <button type="button" class="button small-button primary" onclick={start}>{t('live.retry')}</button>
      {#if resume === null}
        <button type="button" class="button small-button" disabled={stopping} onclick={closeAnyway}>{t('common.close')}</button>
      {:else}
        <button type="button" class="button small-button" data-action="resume-back" onclick={leaveRefused}>
          {t('live.resume.back')}
        </button>
      {/if}
    </div>
  {/if}
  <!-- A sheet that went on in a new note: the notice stays until it is dismissed. -->
  {#each live.forks as fork (fork.path)}
    <div class="banner notice live-banner" role="status" data-slot="sheet-forked" data-reason={fork.reason}>
      {#if sheetTitle}
        <strong>{t('live.resume.forked.sheet', { title: sheetTitle })}</strong>
      {/if}
      <p>{t(`live.resume.forked.${fork.reason}`)}</p>
      <button type="button" class="button small-button" data-action="dismiss-fork" onclick={() => dismissFork(fork)}>
        {t('live.dismiss')}
      </button>
    </div>
  {/each}
  {#if hold}
    <div class="banner notice live-banner" role="status" data-slot="resume-hold" data-orientation={hold}>
      <p>{t('live.resume.hold', { position: t(`tablet.orientation.${hold}`) })}</p>
    </div>
  {/if}
  {#if stoppedNarration}
    <div class="banner error live-banner" role="alert" data-slot="narration-not-added">
      <strong>{t('live.stopped.narration')}</strong>
      <p>{stoppedNarration}</p>
      <button type="button" class="button small-button" data-action="dismiss-narration-problem" onclick={dismissStoppedNarration}>
        {t('live.dismiss')}
      </button>
    </div>
  {/if}
  {#if stopError}
    <div class="banner error live-banner" role="alert">
      <strong>{t('live.stopFailed')}</strong>
      <p>{stopError}</p>
      <button type="button" class="button small-button" disabled={stopping} onclick={closeAnyway}>{t('live.closeAnyway')}</button>
    </div>
  {/if}
  {#if live.saveErrors.length > 0}
    <div class="banner error live-banner" role="alert" data-slot="save-failed">
      <strong>
        {live.saveErrors.length === 1 ? t('live.saveFailed.one') : t('live.saveFailed.many', { count: live.saveErrors.length })}
      </strong>
      <ul>
        {#each live.saveErrors as error (error.pageIndex)}
          <li>{t('live.saveFailed.item', { page: error.pageIndex + 1, message: errorMessage(error) })}</li>
        {/each}
      </ul>
      <p>{t('live.saveFailed.advice')}</p>
    </div>
  {/if}
  {#if undoClear}
    <div class="banner notice live-banner" role="status" data-slot="undo-clear">
      <p>{t('live.clearDone')}</p>
      <button type="button" class="button small-button primary" data-action="undo-clear" onclick={undoClearing}>
        {t('live.clearUndo')}
      </button>
    </div>
  {/if}
  {#if notice}
    <div class="banner error live-banner" role="alert">
      <p>{notice}</p>
      <button type="button" class="button small-button" onclick={() => (notice = null)}>{t('live.dismiss')}</button>
    </div>
  {/if}

  <div class="live-toolbar" role="toolbar" aria-label={t('live.toolbar')}>
    <div class="group" role="group" aria-labelledby="live-tablet-style">
      <span class="group-label" id="live-tablet-style">{t('live.tabletStyle')}</span>
      <label class="group">
        <span class="label">{t('live.tool')}</span>
        <select value={live.style.tool} onchange={(e) => setTool(e.currentTarget.value as Tool)}>
          {#each TOOLS as tool (tool)}
            <option value={tool}>{t(`editor.ink.tools.${tool}`)}</option>
          {/each}
        </select>
      </label>
      <div class="group swatches" role="group" aria-label={t('live.color')}>
        {#each COLORS as { color, label } (label)}
          <button
            type="button"
            class="swatch"
            class:active={sameColor(color, live.style.color)}
            style:background={cssColor(color)}
            aria-pressed={sameColor(color, live.style.color)}
            aria-label={t(label)}
            title={t(label)}
            onclick={() => pickColor(color)}
          ></button>
        {/each}
      </div>
      <label class="group slider">
        <span class="label">{t('live.width')}</span>
        <input
          type="range"
          min="10"
          max="1000"
          step="5"
          value={live.style.width}
          oninput={(e) => live.previewStyle({ ...live.style, width: Number(e.currentTarget.value) })}
          onchange={(e) => setStyle({ ...live.style, width: Number(e.currentTarget.value) })}
        />
        <span class="value">{t('editor.width.value', { mm: mm(live.style.width) })}</span>
      </label>
    </div>

    <div class="group" role="group" aria-label={t('live.pages.label')}>
      <button
        type="button"
        class="button small-button"
        aria-label={t('live.pages.previous')}
        title={t('live.pages.previous')}
        disabled={live.pageIndex === 0}
        onclick={() => command('previousPage')}><span aria-hidden="true">◀</span></button
      >
      <span class="page-position" aria-live="polite" data-page-position="{live.pageIndex + 1}/{live.pageCount}">{pageLabel}</span>
      <button
        type="button"
        class="button small-button"
        aria-label={t('live.pages.next')}
        title={t('live.pages.next')}
        onclick={() => command('nextPage')}><span aria-hidden="true">▶</span></button
      >
      <button type="button" class="button small-button" data-action="new-page" onclick={() => command('newPage')}>
        {t('live.pages.new')}
      </button>
    </div>

    <div class="group" role="group">
      <button type="button" class="button small-button" data-action="clear-page" onclick={() => command('clearPage')}>
        {t('live.clear')}
      </button>
      <button
        type="button"
        class="button small-button toggle"
        data-action="board"
        class:active={live.view.board}
        aria-pressed={live.view.board}
        onclick={() => toggleBoard(actions)}
      >
        {t('live.board')}
      </button>
      <button
        type="button"
        class="button small-button toggle"
        data-action="laser"
        class:active={live.view.laser}
        aria-pressed={live.view.laser}
        onclick={() => toggleLaser(actions)}
      >
        <span class="laser-dot" aria-hidden="true"></span>
        {t('live.laser')}
      </button>
    </div>

    <div class="group" role="group" aria-labelledby="live-computer-tools" data-slot="touch-up">
      <span class="group-label" id="live-computer-tools">{t('live.computer.label')}</span>
      <button
        type="button"
        class="button small-button toggle"
        data-action="select"
        class:active={selection.active}
        aria-pressed={selection.active}
        title={t('live.select.hint')}
        disabled={sessionState === 'idle'}
        onclick={() => toggleSelect()}
      >
        {t('live.select.toggle')}
      </button>
      <button
        type="button"
        class="button small-button toggle"
        data-action="pen"
        class:active={penOn}
        aria-pressed={penOn}
        title={t('live.pen.hint')}
        disabled={sessionState === 'idle'}
        onclick={() => togglePen()}
      >
        {t('live.pen.toggle')}
      </button>
      <ComputerStyle
        style={computerStyle}
        colors={COLORS}
        disabled={sessionState === 'idle'}
        bind:open={computerStyleOpen}
        onchange={setComputerStyle}
      />
      <button
        type="button"
        class="button small-button"
        data-action="undo-edit"
        title={t('live.undoHint')}
        disabled={!live.canUndo}
        onclick={() => undoEdit()}
      >
        {t('live.undo')}
      </button>
      <button
        type="button"
        class="button small-button"
        data-action="redo-edit"
        title={t('live.redoHint')}
        disabled={!live.canRedo}
        onclick={() => undoEdit(true)}
      >
        {t('live.redo')}
      </button>
    </div>

    {#if onBoard}
      <span class="group-label muted" data-slot="xiaomi-note" title={t('live.xiaomi.note')}>{t('live.xiaomi.label')}</span>
    {:else}
      <div
        class="group"
        role="group"
        aria-label={t('live.orientation.label')}
        title={locked ? t('live.resume.cannotTurn') : t('live.orientation.hint')}
        data-slot="orientation"
      >
        {#each ORIENTATIONS as orientation (orientation)}
          <button
            type="button"
            class="button small-button toggle orientation"
            class:active={live.orientation === orientation}
            data-orientation={orientation}
            aria-pressed={live.orientation === orientation}
            aria-label={t(`tablet.orientation.${orientation}`)}
            title={locked ? t('live.resume.cannotTurn') : t(`tablet.orientation.${orientation}`)}
            disabled={locked}
            onclick={() => setOrientation(orientation)}
          >
            <OrientationIcon {orientation} size={26} />
          </button>
        {/each}
      </div>
    {/if}
  </div>

  <div class="live-main">
    <div class="live-preview">
      <LiveCanvas
        bind:this={canvas}
        {live}
        {ink}
        mode="preview"
        {aspect}
        {latency}
        label={t('live.preview.label', { page: `${live.pageIndex + 1} / ${live.pageCount}` })}
        onview={setView}
        {tool}
        selection={selection.indices}
        onselect={(picked, combine) => selection.choose(live, picked, combine)}
        ontransform={(transform) => transformSelected(touchUp, transform)}
        {computer}
        onblocked={penBlocked}
      />
      {#if sessionState === 'waiting'}
        <div class="live-overlay" role="status">
          <span class="pulse" aria-hidden="true"></span>
          <strong>{t(`live.waiting.${waitingKey}`)}</strong>
          <p>{t(waitingKey === 'title' ? 'live.waiting.hint' : 'live.waiting.hintBoard')}</p>
        </div>
      {:else if sessionState === 'reconnecting'}
        <p class="live-reconnecting" role="status">{t('live.reconnecting')}</p>
      {/if}
    </div>

    <aside class="live-side" aria-label={t('side.label')}>
      <SidePanel where="live" panel={side} {badges} above={selection.any || selection.active || penOn ? aboveBlock : undefined}>
        {#snippet layers()}
          <LayersZone
            where="live"
            layers={live.layers}
            chosen={live.computerLayer}
            tablet={live.layer}
            disabled={sessionState === 'idle'}
            onchoose={(layer) => layerOp({ kind: 'computer', layer })}
            ontablet={(layer) => layerOp({ kind: 'tablet', layer })}
            onvisible={(layer, visible) => layerOp({ kind: 'visible', layer, visible })}
            onrename={(layer, name) => layerOp({ kind: 'rename', layer, name })}
            onmove={(from, to) => layerOp({ kind: 'move', from, to })}
            onmerge={askMerge}
            onadd={() => layerOp({ kind: 'add' })}
          />
        {/snippet}
        {#snippet page()}
          <section
            class="pane-block"
            aria-labelledby="live-paper-heading"
            data-section="live-paper"
            onpointerdowncapture={() => (pointerChoice = true)}
            onkeydowncapture={() => (pointerChoice = false)}
            onchange={releaseFocus}
          >
            <h3 class="pane-title" id="live-paper-heading">{t('editor.page.paperTitle')}</h3>
            <p class="pane-help">{t('live.page.help')}</p>
            <PaperBlock
              background={pageBackground}
              disabled={sessionState === 'idle'}
              colourDisabled={live.view.board}
              onbackground={setPaper}
            />
            {#if live.view.board}
              <p class="pane-help" data-slot="paper-board">{t('live.page.board')}</p>
            {:else if live.pagePaper === 'slate'}
              <p class="pane-help" data-slot="paper-slate">{t('live.page.slate')}</p>
            {/if}
            <p class="pane-help">{t('live.page.back')}</p>
          </section>
        {/snippet}
        {#snippet background()}
          <!-- Clicks and changes are only listened to (the page the click was made on, the focus given back). -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="pane-through"
            onpointerdowncapture={() => (pointerChoice = true)}
            onkeydowncapture={() => (pointerChoice = false)}
            onclickcapture={() => (imageFrom = live.pageIndex)}
            onchange={releaseFocus}
          >
            <BackgroundImageSection
              page={live.pageSize}
              image={live.pageImage ?? undefined}
              pixels={live.imagePixels ?? undefined}
              disabled={sessionState === 'idle'}
              offerOrientation={false}
              hint={t('live.background.help')}
              onset={(image, pixels) => pageImage({ kind: 'set', key: pixels.key, image }, imageFrom ?? live.pageIndex)}
              onturn={(direction) => placeImage((image, pixels) => turnImageAlone(image, live.pageSize, pixels, direction))}
              onfit={(fit: ImageFit) => placeImage((image, pixels) => refit(image, live.pageSize, pixels, fit))}
              onopacity={(opacity) => placeImage((image) => ({ ...image, opacity }))}
              onremove={() => void pageImage({ kind: 'remove' })}
            />
          </div>
        {/snippet}
        {#snippet sound()}
          <section class="pane-block" aria-labelledby="live-narration-title" data-section="narration">
            <h3 class="pane-title" id="live-narration-title">{t('live.narration.title')}</h3>
            <RecordPanel
              bind:open={narrationOpen}
              {recorder}
              device={microphone}
              ondevice={chooseMicrophone}
              onstart={startNarration}
              onstop={() => void stopNarration(true)}
              oncancel={() => void stopNarration(false)}
              openLabel={t('live.narration.open')}
              openHint={t('live.narration.openHint')}
              info={t('live.narration.hint')}
            />
            {#if narratedText}
              <p class="pane-help" role="status" data-slot="narration-saved" data-pages={narrated?.pages ?? 0}>{narratedText}</p>
            {/if}
          </section>
        {/snippet}
        {#snippet projection()}
          <ProjectorBlock
            {monitors}
            bind:monitorKey
            loaded={monitorsLoaded}
            presentation={live.presentation}
            busy={projecting}
            error={projectionError}
            onproject={project}
            onstop={stopProjecting}
            onrefresh={refreshMonitors}
          />
          <ZoneBlock
            zoom={live.view.zoom}
            min={MIN_ZOOM}
            max={MAX_ZOOM}
            onzoom={(direction) => zoomStep(actions, direction)}
            onwhole={() => showWholePage(actions)}
            onwidth={() => showPageWidth(actions)}
            hint={t('live.preview.hint')}
          >
            <!-- The same states as the toolbar's buttons, which stay beside the pages (O2). -->
            <label class="pane-row">
              <span>{t('live.board')}</span>
              <input
                type="checkbox"
                role="switch"
                class="pane-switch"
                data-field="board"
                checked={live.view.board}
                onchange={() => toggleBoard(actions)}
              />
            </label>
            <label class="pane-row">
              <span>{t('live.laser')}</span>
              <input
                type="checkbox"
                role="switch"
                class="pane-switch"
                data-field="laser"
                checked={live.view.laser}
                onchange={() => toggleLaser(actions)}
              />
            </label>
          </ZoneBlock>
          <ShareBlock />
        {/snippet}
        {#snippet help()}
          <ShortcutsHelp where="live" shortcuts={LIVE_SHORTCUTS} />
        {/snippet}
      </SidePanel>
    </aside>
  </div>

  {#snippet aboveBlock()}
    <div class="pane-block">
      {#if selection.any}
        <div class="select-bar" role="toolbar" aria-label={t('live.select.bar')} data-slot="select-bar">
          <span class="select-count" role="status" data-count={selection.count}>{t('live.select.count', { count: selection.count })}</span>
          <div class="pane-actions">
            <button type="button" class="button small-button danger" data-action="delete-selection" onclick={() => void deleteSelected(touchUp)}>
              {t('live.select.delete')}
            </button>
            <button type="button" class="button small-button" data-action="deselect" onclick={() => selection.clear()}>
              {t('live.select.clear')}
            </button>
          </div>
          <p class="pane-help">{t('live.select.colors')}</p>
        </div>
      {/if}
      {#if selection.active}
        <p class="pane-help" data-slot="select-hint">{t('live.select.help')}</p>
      {/if}
      {#if penOn}
        <p class="pane-help" data-slot="pen-hint">{t('live.pen.help')}</p>
      {/if}
    </div>
  {/snippet}

  {#if confirmLeave}
    <Modal title={t('live.leave.title')} onclose={() => (confirmLeave = false)}>
      <p>{t('live.leave.message')}</p>
      <div class="actions">
        <button type="button" class="button primary" data-autofocus data-action="leave-cancel" onclick={() => (confirmLeave = false)}>
          {t('live.leave.continue')}
        </button>
        <button
          type="button"
          class="button danger"
          data-action="leave-confirm"
          onclick={() => {
            confirmLeave = false;
            void finish();
          }}
        >
          {t('live.leave.confirm')}
        </button>
      </div>
    </Modal>
  {/if}

  {#if mergeAsk}
    <Modal title={t('live.merge.title', { upper: mergeAsk.upper, lower: mergeAsk.lower })} role="alertdialog" onclose={() => (mergeAsk = null)}>
      <div data-slot="merge-question">
        <p>{t('live.merge.noUndo')}</p>
        {#if mergeAsk.reveals}
          <p data-slot="merge-reveals">{t('live.merge.hidden', { lower: mergeAsk.lower })}</p>
        {/if}
      </div>
      <div class="actions">
        <button type="button" class="button primary" data-autofocus data-action="merge-cancel" onclick={() => (mergeAsk = null)}>
          {t('live.merge.cancel')}
        </button>
        <button type="button" class="button danger" data-action="merge-confirm" onclick={confirmMerge}>
          {t('live.merge.confirm')}
        </button>
      </div>
    </Modal>
  {/if}

  {#if confirmQuit}
    <Modal title={t('live.quit.title')} onclose={() => (confirmQuit = false)}>
      <p>{t('live.quit.message')}</p>
      <div class="actions">
        <button type="button" class="button primary" data-autofocus data-action="quit-cancel" onclick={() => (confirmQuit = false)}>
          {t('live.leave.continue')}
        </button>
        <button
          type="button"
          class="button danger"
          data-action="quit-confirm"
          onclick={() => {
            confirmQuit = false;
            void requestWindowClose();
          }}
        >
          {t('live.quit.confirm')}
        </button>
      </div>
    </Modal>
  {/if}
</section>

<style>
  .live-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background: var(--bg);
  }

  .live-top {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem 1rem;
    padding: 0.5rem 1rem;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  .live-top h2 {
    margin-right: auto;
  }

  .live-state {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.875rem;
    font-weight: 600;
  }

  .dot {
    width: 0.6rem;
    height: 0.6rem;
    border-radius: 50%;
    background: var(--muted);
  }

  .live-state[data-state='live'] .dot {
    background: #1f9d55;
  }

  .live-state[data-state='waiting'] .dot,
  .live-state[data-state='reconnecting'] .dot {
    background: #e0a100;
  }

  .latency {
    display: inline-flex;
    align-items: baseline;
    gap: 0.35rem;
    font-size: 0.875rem;
    font-variant-numeric: tabular-nums;
  }

  .saved {
    font-size: 0.875rem;
  }

  .live-banner {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.25rem 1rem;
    margin: 0.5rem 1rem;
  }

  .live-banner p {
    flex-basis: 100%;
    margin: 0;
  }

  .live-banner ul {
    flex-basis: 100%;
    margin: 0;
    padding-left: 1.2rem;
  }

  .live-toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem 1.25rem;
    padding: 0.4rem 1rem;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  .group {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.35rem;
    min-height: 2.25rem;
  }

  .label {
    color: var(--muted);
    font-size: 0.875rem;
  }

  .group-label {
    color: var(--muted);
    font-size: 0.875rem;
    font-weight: 600;
  }

  .small-button {
    padding: 0.3rem 0.65rem;
    font-size: 0.875rem;
  }

  .button:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .button.orientation {
    padding: 0.1rem 0.25rem;
  }

  .button.toggle.active {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--accent-text);
  }

  .laser-dot {
    width: 0.6rem;
    height: 0.6rem;
    border-radius: 50%;
    background: #ff1f1f;
    box-shadow: 0 0 0 3px rgb(255 31 31 / 25%);
  }

  .swatch {
    width: 1.6rem;
    height: 1.6rem;
    padding: 0;
    border: 2px solid var(--border);
    border-radius: 50%;
    cursor: pointer;
  }

  .swatch.active {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .swatch:focus-visible,
  .slider input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .slider input {
    width: 8rem;
  }

  .value,
  .page-position {
    min-width: 4.5rem;
    color: var(--muted);
    font-size: 0.875rem;
    font-variant-numeric: tabular-nums;
    text-align: center;
  }

  .page-position {
    color: var(--text);
    font-weight: 600;
  }

  .live-main {
    display: flex;
    flex: 1;
    min-height: 0;
  }

  .live-preview {
    position: relative;
    display: flex;
    flex: 1;
    min-width: 0;
    background: var(--surface-2);
  }

  /* In the side panel's above block: the rail's `side-above` gives the look, this lays out the count and the buttons. */
  .select-bar {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .select-count {
    font-weight: 600;
    font-size: 0.875rem;
  }

  .live-overlay {
    position: absolute;
    top: 50%;
    left: 50%;
    max-width: min(32rem, 90%);
    padding: 1.25rem 1.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    box-shadow: var(--shadow);
    text-align: center;
    transform: translate(-50%, -50%);
  }

  .live-overlay p {
    margin-bottom: 0;
    color: var(--muted);
  }

  .pulse {
    display: block;
    width: 0.9rem;
    height: 0.9rem;
    margin: 0 auto 0.75rem;
    border-radius: 50%;
    background: #2f6fdf;
    animation: pulse 1.4s ease-in-out infinite;
  }

  @keyframes pulse {
    0%,
    100% {
      opacity: 0.35;
      transform: scale(0.85);
    }
    50% {
      opacity: 1;
      transform: scale(1.15);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .pulse {
      animation: none;
    }
  }

  .live-reconnecting {
    position: absolute;
    top: 0.75rem;
    left: 50%;
    margin: 0;
    padding: 0.4rem 0.9rem;
    border-radius: 999px;
    background: #e0a100;
    color: #1d232b;
    font-size: 0.875rem;
    font-weight: 600;
    transform: translateX(-50%);
  }

  /* The rail and the pane (`SidePanel`) bring their own width, border and scrolling. */
  .live-side {
    display: flex;
    flex: none;
    min-height: 0;
  }

  /* Listens for the Fond tab's clicks and changes without being a box of its own. */
  .pane-through {
    display: contents;
  }
</style>
