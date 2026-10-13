#!/usr/bin/env bash
# Installs a Flatpak bundle of UpNext for the current user and runs it in its
# sandbox: `upnext --version` and the licence files, then
# `upnext serve` on a free port with a made-up library, checking that it
# answers, names the commit generate-sources.sh found and serves the
# third-party notices, that it knows it runs in a
# Flatpak, that its data went to the app's own folder, and that "Add to
# Steam" says to add it in Steam instead. It uninstalls the app
# again and removes its folder, so run it where no real UpNext Flatpak is
# installed (a CI runner or a container). The runtime comes from Flathub,
# which must be set up as the `flathub` remote for the user.
#
# Usage: contrib/flatpak/smoke-test.sh BUNDLE [PORT]
set -euo pipefail

APP_ID=io.github.zytocc.UpNext
here=$(cd "$(dirname "$0")" && pwd)
bundle="$1"
port="${2:-8796}"

# `flatpak run` needs the system bus and a session bus, which a desktop has
# and a build container doesn't, so start them there.
if [ ! -S /run/dbus/system_bus_socket ] && [ "$(id -u)" = 0 ]; then
  mkdir -p /run/dbus
  dbus-daemon --system --fork
fi
if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
  exec dbus-run-session -- "$0" "$@"
fi
# A host's own certificate file, as a proxy sets up, isn't in the sandbox,
# and the runtime brings its own. `flatpak install` below uses neither.
unset SSL_CERT_FILE SSL_CERT_DIR

if flatpak info --user "$APP_ID" >/dev/null 2>&1; then
  echo "$APP_ID is installed for this user already; the smoke test would remove it" >&2
  exit 1
fi

home_dir="$HOME/.var/app/$APP_ID"
cleanup() {
  # Killing `flatpak run` leaves the sandbox running, and Flatpak keeps a
  # running app's files until it ends.
  flatpak kill "$APP_ID" 2>/dev/null || true
  [ -n "${server:-}" ] && wait "$server" 2>/dev/null || true
  flatpak uninstall --user -y --noninteractive "$APP_ID" >/dev/null 2>&1 || true
  rm -rf "$home_dir"
}
trap cleanup EXIT

flatpak install --user -y --noninteractive --bundle "$bundle"
run() { flatpak run --command=upnext "$APP_ID" "$@"; }

run --version
# UpNext's licences and the third-party notices, where Flatpak keeps an app's.
for f in LICENSE-MIT LICENSE-APACHE THIRD_PARTY_NOTICES.txt; do
  flatpak run --command=test "$APP_ID" -s "/app/share/licenses/$APP_ID/$f" || { echo "the Flatpak has no /app/share/licenses/$APP_ID/$f" >&2; exit 1; }
done
# No AI program, and no real library: a made-up one.
export UPNEXT_CLAUDE_BIN=/nonexistent UPNEXT_CODEX_BIN=/nonexistent UPNEXT_PORT="$port"
run mock-library

# UpNext starts again without a certificate file the sandbox can't read
# (`flatpak::restart_without_unreadable_cert_env`): serve is given one that
# exists nowhere, and its log is checked below.
SSL_CERT_FILE=/nonexistent/upnext-smoke-test-ca.pem run serve > "$home_dir/serve.log" 2>&1 &
server=$!
for _ in $(seq 1 60); do
  curl -fsS "http://127.0.0.1:$port/api/health" >/dev/null 2>&1 && break
  sleep 1
done
health=$(curl -fsS "http://127.0.0.1:$port/api/health")
echo "$health"
# The build it names is the commit generate-sources.sh wrote for it (empty
# outside a checkout), so a bundle built without writing the sources again
# after a new commit is caught. A bundle tried without them skips this.
commit_file="$here/generated/commit"
if [ -f "$commit_file" ]; then
  python3 - "$health" "$(cat "$commit_file")" <<'PY' || exit 1
import json, sys
build, commit = json.loads(sys.argv[1]).get("build"), sys.argv[2].strip() or None
assert build == commit, f"the bundle says it was built from {build}, and generated/commit says {commit}"
print("build:", build)
PY
fi
# Read the whole file before grep: with pipefail, grep -q leaving at the first
# line would break curl's pipe and fail the check.
notices=$(curl -fsS "http://127.0.0.1:$port/third-party-notices.txt") \
  || { echo "couldn't fetch /third-party-notices.txt" >&2; exit 1; }
grep -q '^UpNext: third-party notices' <<<"$notices" \
  || { echo "/third-party-notices.txt isn't the notices" >&2; exit 1; }
grep -q "SSL_CERT_FILE=/nonexistent/upnext-smoke-test-ca.pem can't be read inside the Flatpak" "$home_dir/serve.log" \
  || { echo "serve didn't say it dropped the unreadable SSL_CERT_FILE" >&2; cat "$home_dir/serve.log" >&2; exit 1; }

setup=$(curl -fsS "http://127.0.0.1:$port/api/v1/setup")
python3 - "$setup" "$APP_ID" <<'PY'
import json, sys
setup, app_id = json.loads(sys.argv[1]), sys.argv[2]
assert setup["environment"]["flatpak"] == app_id, setup["environment"]
assert setup["library"]["titles"] > 0, setup["library"]
print("setup:", setup["environment"])
PY
shortcut=$(curl -fsS "http://127.0.0.1:$port/api/v1/steam/shortcut")
python3 - "$shortcut" "$APP_ID" <<'PY'
import json, sys
shortcut, app_id = json.loads(sys.argv[1]), sys.argv[2]
assert shortcut["target"]["arguments"] == f"run {app_id} --big-screen", shortcut
assert "Flatpak" in shortcut["unavailable"], shortcut
print("Add to Steam:", shortcut["unavailable"])
PY
test -f "$home_dir/data/upnext/upnext.db" || { echo "no database in $home_dir/data/upnext" >&2; ls -R "$home_dir" >&2; exit 1; }
echo "The Flatpak ran in its sandbox and kept its library in $home_dir."
