#!/usr/bin/env bash
# Writes the Flatpak's offline sources into contrib/flatpak/generated/
#, which the manifest reads:
#
#   cargo-sources.json         every crate in Cargo.lock, from crates.io
#   node-sources.json          every npm package the web build installs
#   web/package.json, web/package-lock.json
#                              web/'s, without Playwright: only the end-to-end
#                              tests use it, and the node generator would
#                              otherwise fetch its browsers into the build
#   commit                     the commit this checkout is at, for build.rs:
#                              the build copies the source without .git, and
#                              `upnext --version` and /api/health name it
#
# Flathub builds with no network, so each crate and package is listed with
# its checksum, from the lock files. The generators are flatpak-builder-tools'
# at a pinned commit, run in a throwaway virtualenv; they need git, python3
# with venv, and npm. Run it again whenever Cargo.lock or web/package-lock.json
# changes. Usage: contrib/flatpak/generate-sources.sh [--out DIR]
set -euo pipefail

# flatpak-builder-tools at a commit of 2026-09-30, and the generators' own
# Python packages at the versions it was pinned with.
TOOLS_REPO=https://github.com/flatpak/flatpak-builder-tools.git
TOOLS_COMMIT=74697c75b630d7330e77250fc13cb5ea688d9479
PY_PACKAGES=(aiohttp==3.14.4 tomlkit==0.15.1 PyYAML==6.0.3)

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
out="$here/generated"
while [ $# -gt 0 ]; do
  case "$1" in
    --out) out="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

for tool in git python3 npm; do
  command -v "$tool" >/dev/null || { echo "$tool is needed" >&2; exit 1; }
done

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

git clone -q --filter=blob:none "$TOOLS_REPO" "$work/tools"
git -C "$work/tools" -c advice.detachedHead=false checkout -q "$TOOLS_COMMIT"
# Flathub's build image sets PYTHONPATH to its own site-packages. pip would
# take the packages it finds there as installed and leave them out of the
# virtualenv, where the generators, run with their own PYTHONPATH, can't
# see them.
unset PYTHONPATH
python3 -m venv "$work/venv"
"$work/venv/bin/pip" install -q --disable-pip-version-check --only-binary=:all: "${PY_PACKAGES[@]}"

mkdir -p "$out/web"
"$work/venv/bin/python" "$work/tools/cargo/flatpak-cargo-generator.py" "$root/Cargo.lock" -o "$out/cargo-sources.json"

# The lock without Playwright: npm takes it out of the lock file alone,
# offline, leaving every other package as it was.
cp "$root/web/package.json" "$root/web/package-lock.json" "$work/"
(cd "$work" && npm_config_cache="$work/npm-cache" npm uninstall --offline --package-lock-only --ignore-scripts \
  --no-audit --no-fund --no-update-notifier --loglevel=error @playwright/test)
cp "$work/package.json" "$work/package-lock.json" "$out/web/"
PYTHONPATH="$work/tools/node" "$work/venv/bin/python" -m flatpak_node_generator npm "$out/web/package-lock.json" \
  -o "$out/node-sources.json" >/dev/null

# The commit, as build.rs would find it outside the build: UPNEXT_COMMIT,
# GitHub's GITHUB_SHA, or git when this is a checkout of UpNext itself.
# Empty otherwise (a release archive), and the version then has none.
commit=${UPNEXT_COMMIT:-${GITHUB_SHA:-}}
if [ -z "$commit" ] && [ "$(git -C "$root" rev-parse --show-toplevel 2>/dev/null)" = "$root" ]; then
  commit=$(git -C "$root" rev-parse HEAD)
fi
printf '%s\n' "${commit:0:7}" > "$out/commit"

echo "Wrote $out: $(grep -c '"dest": "cargo/vendor' "$out/cargo-sources.json") crates and the npm packages for web/."
