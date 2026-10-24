#!/bin/bash
# Prepares a Claude Code cloud session (Ubuntu 24.04) to build and test Ardoise: Rust with
# the WebAssembly target, Node 24 and the WebKitGTK libraries Tauri needs on Linux.
#
# Runs at every session start through the SessionStart hook in .claude/settings.json and
# skips what is already installed. It can also be pasted as the cloud environment's setup
# script, so that the result is cached. Does nothing outside a cloud session.
[ "$CLAUDE_CODE_REMOTE" = "true" ] || exit 0

RUST_VERSION=1.98.1
NODE_VERSION=24.22.1
NODE_DIR=/opt/node24
APT_PACKAGES="build-essential pkg-config file libwebkit2gtk-4.1-dev libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev"
export PATH="$NODE_DIR/bin:$HOME/.cargo/bin:$PATH"

log() { echo "[cloud-setup] $*"; }
as_root() { if [ "$(id -u)" = 0 ]; then "$@"; else sudo "$@"; fi; }

system_libraries() {
  # shellcheck disable=SC2086
  dpkg -s $APT_PACKAGES >/dev/null 2>&1 && return 0
  log "installing the WebKitGTK libraries"
  as_root apt-get update -qq &&
    # shellcheck disable=SC2086
    as_root env DEBIAN_FRONTEND=noninteractive apt-get install -y -qq $APT_PACKAGES >/dev/null
}

rust_toolchain() {
  if ! command -v rustup >/dev/null; then
    log "installing rustup"
    curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none >/dev/null || return 1
  fi
  if ! rustup toolchain list | grep -q "^$RUST_VERSION"; then
    log "installing Rust $RUST_VERSION"
    rustup toolchain install "$RUST_VERSION" --profile minimal \
      --component rustfmt,clippy --target wasm32-unknown-unknown >/dev/null || return 1
  fi
  rustup default "$RUST_VERSION" >/dev/null
}

node_runtime() {
  [ -x "$NODE_DIR/bin/node" ] && return 0
  log "installing Node $NODE_VERSION"
  as_root mkdir -p "$NODE_DIR" &&
    curl -sSfL "https://nodejs.org/dist/v$NODE_VERSION/node-v$NODE_VERSION-linux-x64.tar.xz" |
    as_root tar -xJ -C "$NODE_DIR" --strip-components=1
}

system_libraries & libs=$!
rust_toolchain & rust=$!
node_runtime & node=$!
failed=""
wait $libs || failed="$failed system-libraries"
wait $rust || failed="$failed rust"
wait $node || failed="$failed node"

# Later shell commands of the session see Node 24 and cargo first.
if [ -n "$CLAUDE_ENV_FILE" ]; then
  echo "export PATH=\"$NODE_DIR/bin:\$HOME/.cargo/bin:\$PATH\"" >>"$CLAUDE_ENV_FILE"
fi

if [ -n "$failed" ]; then
  log "FAILED:$failed (see the messages above; fix it before running cargo xtask check)"
else
  log "ready: node $(node --version), $(rustc --version)"
fi
exit 0
