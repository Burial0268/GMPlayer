#!/usr/bin/env bash
# Real device notifications and CPAL opens, without touching the user's audio server.
set -euo pipefail

for tool in pulseaudio pactl cargo; do
  command -v "$tool" >/dev/null || { echo "Missing required tool: $tool" >&2; exit 1; }
done

repo_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
test_runtime=$(mktemp -d "${TMPDIR:-/tmp}/gmplayer-device-watch.XXXXXX")
daemon_pid=
cleanup() {
  if [[ -n "$daemon_pid" ]]; then
    kill "$daemon_pid" 2>/dev/null || true
    wait "$daemon_pid" 2>/dev/null || true
  fi
  rm -rf -- "$test_runtime"
}
trap cleanup EXIT

export PULSE_SERVER="unix:$test_runtime/native"
export PULSE_COOKIE="$test_runtime/cookie"
export PULSE_STATE_PATH="$test_runtime/state"
export PULSE_RUNTIME_PATH="$test_runtime/runtime"
export GMPLAYER_DEVICE_WATCH_TEST=1
mkdir -p "$PULSE_STATE_PATH" "$PULSE_RUNTIME_PATH"

pulseaudio --daemonize=no --use-pid-file=no --exit-idle-time=-1 --disable-shm=yes \
  --disallow-exit=yes --log-target=stderr -n \
  -L "module-native-protocol-unix socket=$test_runtime/native auth-anonymous=1" \
  -L "module-null-sink sink_name=gmplayer_watch_a" \
  -L "module-null-sink sink_name=gmplayer_watch_b" \
  >"$test_runtime/daemon.log" 2>&1 &
daemon_pid=$!

ready=false
for _ in {1..100}; do
  if pactl info >/dev/null 2>&1; then
    ready=true
    break
  fi
  if ! kill -0 "$daemon_pid" 2>/dev/null; then
    break
  fi
  sleep 0.05
done
if [[ "$ready" != true ]]; then
  echo "Isolated PulseAudio server did not start:" >&2
  tail -n 30 "$test_runtime/daemon.log" >&2
  exit 1
fi

cd "$repo_dir/src-tauri"
cargo test -p gmplayer-audio-backend --lib \
  "${1:-live_default_switch_is_notified_and_resolves_to_the_new_output}" -- --ignored --nocapture
