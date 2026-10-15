#!/usr/bin/env bash
# Real playback through a private PipeWire with a virtual sink; nothing reaches
# the host audio. Build first: cargo test -p okbs-platform-linux --locked --no-run
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ ${1:-} != --session ]]; then exec dbus-run-session -- bash "$0" --session; fi
export PATH="$HOME/.cargo/bin:$PATH"
linux_sound=tmp/linux-sound
rm -rf "$linux_sound"
mkdir -p "$linux_sound"/{run,config,state,tmp}
export TMPDIR="$PWD/$linux_sound/tmp" XDG_RUNTIME_DIR="$PWD/$linux_sound/run" XDG_CONFIG_HOME="$PWD/$linux_sound/config" XDG_STATE_HOME="$PWD/$linux_sound/state"
chmod 700 "$XDG_RUNTIME_DIR"
unset PULSE_SERVER PIPEWIRE_REMOTE
pipewire >"$linux_sound/pipewire.log" 2>&1 &
linux_pipewire_pid=$!
wireplumber >"$linux_sound/wireplumber.log" 2>&1 &
linux_wireplumber_pid=$!
trap 'kill $linux_wireplumber_pid $linux_pipewire_pid 2>/dev/null || true' EXIT
for _ in {1..50}; do pw-cli info 0 >/dev/null 2>&1 && break; sleep 0.1; done
pw-cli create-node adapter '{ factory.name=support.null-audio-sink node.name=okbs-test-sink node.description=okbs-test-sink media.class=Audio/Sink object.linger=true audio.position=[MONO] audio.rate=48000 }' >/dev/null
for _ in {1..50}; do wpctl status 2>/dev/null | grep -q okbs-test-sink && break; sleep 0.1; done
linux_sink=$(pw-cli ls Node | awk '/^\tid/{id=$2} /node.name = "okbs-test-sink"/{sub(",","",id); print id}')
wpctl set-default "$linux_sink"
export OKBS_TEST_SOUND_SINK=okbs-test-sink
timeout 120s cargo test -p okbs-platform-linux real_sounds_reach_the_default_audio_output --locked -- --ignored --nocapture
