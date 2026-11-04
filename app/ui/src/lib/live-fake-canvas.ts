/**
 * A stand-in for `CanvasRenderingContext2D` in the tests (jsdom has no canvas): it records the
 * calls the live drawing makes, so that a test can count fills, see transforms and colours, and
 * time the drawing code without a real rasteriser.
 */
import type { Ctx2D, Surface } from './live-render';

export interface FakeCall {
  op: string;
  args: unknown[];
  /** `fillStyle` / `strokeStyle` / `globalAlpha` / composite at the time of the call. */
  fillStyle?: string;
  strokeStyle?: string;
  alpha?: number;
  composite?: string;
}

export class FakeContext implements Ctx2D {
  fillStyle: string | CanvasGradient | CanvasPattern = '#000000';
  strokeStyle: string | CanvasGradient | CanvasPattern = '#000000';
  lineWidth = 1;
  globalAlpha = 1;
  globalCompositeOperation: GlobalCompositeOperation = 'source-over';
  /** Every call, in order (path building is only counted, see `pathPoints`). */
  calls: FakeCall[] = [];
  /** moveTo + lineTo calls since the last `reset`. */
  pathPoints = 0;
  transform: [number, number, number, number, number, number] = [1, 0, 0, 1, 0, 0];
  private saved: (typeof this.transform)[] = [];

  /** Forgets the recorded calls. */
  reset(): void {
    this.calls = [];
    this.pathPoints = 0;
  }

  private record(op: string, args: unknown[]): void {
    this.calls.push({
      op,
      args,
      fillStyle: String(this.fillStyle),
      strokeStyle: String(this.strokeStyle),
      alpha: this.globalAlpha,
      composite: this.globalCompositeOperation,
    });
  }

  count(op: string): number {
    return this.calls.filter((c) => c.op === op).length;
  }

  ops(op: string): FakeCall[] {
    return this.calls.filter((c) => c.op === op);
  }

  save(): void {
    this.saved.push(this.transform);
    this.record('save', []);
  }
  restore(): void {
    this.transform = this.saved.pop() ?? this.transform;
    this.record('restore', []);
  }
  rect(x: number, y: number, width: number, height: number): void {
    this.record('rect', [x, y, width, height]);
  }
  clip(): void {
    this.record('clip', []);
  }
  setTransform(a: number, b: number, c: number, d: number, e: number, f: number): void {
    this.transform = [a, b, c, d, e, f];
    this.record('setTransform', [a, b, c, d, e, f]);
  }
  clearRect(x: number, y: number, width: number, height: number): void {
    this.record('clearRect', [x, y, width, height]);
  }
  fillRect(x: number, y: number, width: number, height: number): void {
    this.record('fillRect', [x, y, width, height]);
  }
  strokeRect(x: number, y: number, width: number, height: number): void {
    this.record('strokeRect', [x, y, width, height]);
  }
  drawImage(image: CanvasImageSource, dx: number, dy: number, dw?: number, dh?: number): void {
    this.record('drawImage', [image, dx, dy, dw, dh]);
  }
  beginPath(): void {
    this.record('beginPath', []);
  }
  moveTo(): void {
    this.pathPoints += 1;
  }
  lineTo(): void {
    this.pathPoints += 1;
  }
  closePath(): void {}
  arc(x: number, y: number, radius: number): void {
    this.record('arc', [x, y, radius]);
  }
  fill(rule?: CanvasFillRule): void {
    this.record('fill', [rule]);
  }
  stroke(): void {
    this.record('stroke', []);
  }
}

/** An offscreen surface backed by a `FakeContext`. */
export class FakeSurface implements Surface {
  readonly ctx = new FakeContext();
  readonly image = { fake: 'surface' } as unknown as CanvasImageSource;
  width = 0;
  height = 0;
  resizes = 0;

  resize(width: number, height: number): void {
    this.width = width;
    this.height = height;
    this.resizes += 1;
  }
}
