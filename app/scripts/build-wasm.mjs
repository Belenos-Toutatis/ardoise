// Builds `crates/ardoise-ink-wasm` for WebAssembly and copies the module where the interface
// imports it (`ui/src/assets/ardoise_ink.wasm`, git-ignored). Needs the Rust target
// `wasm32-unknown-unknown` (installed with `rustup target add wasm32-unknown-unknown`).
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const app = join(dirname(fileURLToPath(import.meta.url)), '..');
const root = join(app, '..');
execFileSync(
  'cargo',
  ['build', '--quiet', '-p', 'ardoise-ink-wasm', '--target', 'wasm32-unknown-unknown', '--release'],
  { cwd: root, stdio: 'inherit' },
);
const target = join(app, 'ui', 'src', 'assets');
mkdirSync(target, { recursive: true });
// Cargo builds into CARGO_TARGET_DIR when it is set (several worktrees can share one).
const cargoTarget = process.env.CARGO_TARGET_DIR ? resolve(root, process.env.CARGO_TARGET_DIR) : join(root, 'target');
copyFileSync(
  join(cargoTarget, 'wasm32-unknown-unknown', 'release', 'ardoise_ink_wasm.wasm'),
  join(target, 'ardoise_ink.wasm'),
);
