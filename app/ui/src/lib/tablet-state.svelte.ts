/**
 * The tablet as the whole window sees it: one subscription to the service (`api.tabletSubscribe`)
 * shared by the sidebar block, the tablet panel, the registration wizard and the settings, plus the
 * actions they offer. Components read `tablet.status` and call its methods; errors come back as
 * translated sentences (`errorMessage`). Contract: docs/design/tablet-live-classroom.md.
 */
import { api, errorMessage } from './api';
import type { Connection, TabletOptions, TabletStatus } from './tablet';

/** Longest tablet name the tablet accepts. */
export const TABLET_NAME_MAX = 15;

/** A name the tablet accepts: 1 to 15 printable ASCII characters once trimmed (`invalidName` otherwise). */
export function validTabletName(name: string): boolean {
  const trimmed = name.trim();
  return trimmed.length >= 1 && trimmed.length <= TABLET_NAME_MAX && /^[\x20-\x7e]+$/.test(trimmed);
}

/** Drawing area in millimetres from device units (10 µm): `297 × 216 mm`. */
export function sizeInMillimetres(width: number, height: number): string {
  return `${Math.round(width / 100)} × ${Math.round(height / 100)} mm`;
}

/** Connections in which the tablet is doing something that a new request must not interrupt. */
export function isBusy(connection: Connection): boolean {
  return connection === 'registering' || connection === 'syncing' || connection === 'live';
}

export class TabletState {
  /** Last status received; null until the first answer of the service. */
  status = $state<TabletStatus | null>(null);
  /** Translated message of the last action that failed. */
  actionError = $state<string | null>(null);
  /** The subscription itself failed (no service). */
  unavailable = $state(false);
  /** Drift left in seconds after the last "set the clock"; null when not done since the status changed. */
  drift = $state<number | null>(null);
  /** The name was changed by the last rename (for the confirmation). */
  renamed = $state(false);
  /** An action is waiting for the tablet. */
  working = $state(false);

  private started = false;
  /** An event arrived: the answer to the subscription call is then older than the status. */
  private received = false;

  /** The service's own failure, translated. */
  readonly statusError = $derived(this.status?.error ? errorMessage(this.status.error) : null);
  /** What to show as the failure: the last action's, else the service's. */
  readonly error = $derived(this.actionError ?? this.statusError);
  readonly registered = $derived(this.status?.registered != null);

  /** Starts following the service (once for the whole window). */
  start(): void {
    if (this.started) return;
    this.started = true;
    api
      .tabletSubscribe((event) => {
        this.received = true;
        this.apply(event.status);
      })
      .then(
        (status) => {
          if (!this.received) this.apply(status);
        },
        () => {
          this.started = false;
          this.unavailable = true;
        },
      );
  }

  /** Forgets everything and subscribes again at the next `start()` (tests). */
  reset(): void {
    this.started = false;
    this.received = false;
    this.status = null;
    this.actionError = null;
    this.unavailable = false;
    this.drift = null;
    this.renamed = false;
    this.working = false;
  }

  private apply(status: TabletStatus): void {
    if (this.status?.registered?.name !== status.registered?.name) this.renamed = false;
    if (status.registered === null) this.drift = null;
    this.unavailable = false;
    this.status = status;
  }

  dismissError(): void {
    this.actionError = null;
  }

  /** Runs a request, keeping the failure as a message; resolves `undefined` when it failed. */
  private async run<T>(action: () => Promise<T>): Promise<{ value: T } | undefined> {
    this.actionError = null;
    try {
      return { value: await action() };
    } catch (error) {
      this.actionError = errorMessage(error);
      return undefined;
    }
  }

  /** Reads the status again (after a change the service does not announce by itself). */
  async refresh(): Promise<void> {
    const done = await this.run(() => api.tabletStatus());
    if (done) this.apply(done.value);
  }

  /** Starts the registration (progress arrives in `status.registerStep`). */
  async register(): Promise<boolean> {
    return (await this.run(() => api.tabletRegister())) !== undefined;
  }

  /** Stops the registration or the sync under way. */
  async cancel(): Promise<boolean> {
    return (await this.run(() => api.tabletCancel())) !== undefined;
  }

  async sync(): Promise<boolean> {
    return (await this.run(() => api.tabletSync())) !== undefined;
  }

  /** Renames the tablet; false when the name is not acceptable or the tablet refused. */
  async rename(name: string): Promise<boolean> {
    if (!validTabletName(name)) return false;
    this.renamed = false;
    this.working = true;
    const done = await this.run(() => api.tabletRename(name.trim()));
    this.working = false;
    if (!done) return false;
    this.apply(done.value);
    this.renamed = true;
    return true;
  }

  /** Sets the tablet's clock; `drift` then holds the seconds left. */
  async setTime(): Promise<boolean> {
    this.drift = null;
    this.working = true;
    const done = await this.run(() => api.tabletSetTime());
    this.working = false;
    if (!done) return false;
    this.drift = done.value;
    return true;
  }

  /** Changes some options; they show at once and go back if the service refuses. */
  async setOptions(change: Partial<TabletOptions>): Promise<boolean> {
    const before = this.status;
    if (!before) return false;
    const next = { ...before.options, ...change };
    this.status = { ...before, options: next };
    const done = await this.run(() => api.tabletSetOptions(next));
    if (!done) {
      // Unless a newer status came meanwhile.
      if (this.status && JSON.stringify(this.status.options) === JSON.stringify(next)) {
        this.status = { ...this.status, options: before.options };
      }
      return false;
    }
    this.apply(done.value);
    return true;
  }

  /** Forgets the registered tablet, then reads the new status. */
  async forget(): Promise<boolean> {
    const done = await this.run(() => api.forgetTablet());
    if (!done) return false;
    await this.refresh();
    return true;
  }

  /** Presses the simulated tablet's button (`--simulated-tablet`). */
  async pressButton(): Promise<boolean> {
    return (await this.run(() => api.simPressButton())) !== undefined;
  }
}

/** The window's tablet. */
export const tablet = new TabletState();
