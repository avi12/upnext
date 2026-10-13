#!/usr/bin/env bash
# The server for the smoke test: the mock library in a fresh data directory,
# nothing read from this machine (its own HOME, no Steam or Heroic folders,
# and no other users' homes: its HOME is in a folder of homes of its own, so
# setup looks for other users there and not in the machine's /home), and the
# model and the verdicts service at the fakes in fake-llm.mjs.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
bin="${UPNEXT_BIN:-$here/../../target/debug/upnext}"
# A long folder name with no hyphens, so a path the UI shows (Settings,
# Backups) can't wrap and is longer than a phone's screen wherever the
# checkout is: the phone-width test must not pass only on a short path.
# With UPNEXT_E2E_ONE_PLAYER=1, the mock library's first player alone, in a
# folder of its own, since both servers start side by side.
root="$here/.data"
mock=(mock-library)
if [ "${UPNEXT_E2E_ONE_PLAYER:-}" = 1 ]; then
  root="$here/.data-one-player"
  mock=(mock-library --one-player)
fi
data="$root/upnextendtoendtestdatafolderwithalongnamethatcannotwrapanywhere"
rm -rf "$root"
mkdir -p "$data/home/you"
export HOME="$data/home/you"
export UPNEXT_CONFIG_DIR="$data"
export UPNEXT_DATA_DIR="$data"
# The user name the wizard offers as the first player's name, in place of
# whoever runs the tests.
export USER=sam LOGNAME=sam
unset USERNAME
export UPNEXT_STEAM_DIR="$data/no-steam"
export UPNEXT_HEROIC_DIR="$data/no-heroic"
export UPNEXT_STATIC_DIR="$here/../dist"
export UPNEXT_LLM=openai
export UPNEXT_OPENAI_BASE_URL="http://127.0.0.1:${FAKE_LLM_PORT:-8799}/v1"
export UPNEXT_OPENAI_MODEL=fake-model
# The backlog verdicts service is the fake in fake-verdicts.mjs, never a real one.
export UPNEXT_VERDICTS_URL="http://127.0.0.1:${FAKE_LLM_PORT:-8799}/verdicts"
"$bin" "${mock[@]}"
exec "$bin" serve --port "${UPNEXT_PORT:-8798}"
