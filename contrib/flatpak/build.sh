#!/usr/bin/env bash
# Builds UpNext's Flatpak from this checkout into a local
# repository and a single-file bundle:
#
#   OUT/repo                     an OSTree repository holding the app
#   OUT/upnext-VERSION-ARCH.flatpak, and its .sha256
#
# It writes the offline sources first (generate-sources.sh), then runs
# flatpak-builder, installing the GNOME runtime, its SDK and the Rust and
# Node SDK extensions from Flathub for the current user when they're missing.
# The bundle names Flathub as the place to get the runtime, so
# `flatpak install --user upnext-VERSION-ARCH.flatpak` works on a computer
# with Flathub set up. With --lint it also runs Flathub's linter
# (flatpak-builder-lint, from the org.flatpak.Builder app) on the manifest
# and the repository. The release workflow's build-flatpak job runs this;
# contrib/flatpak/README.md says how to install what it needs.
#
# Usage: contrib/flatpak/build.sh [--out DIR] [--version V] [--lint] [--no-deps]
set -euo pipefail

APP_ID=io.github.zytocc.UpNext
FLATHUB=https://dl.flathub.org/repo/flathub.flatpakrepo

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
out="$root/target/flatpak"
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n1)
lint=false
deps=true
while [ $# -gt 0 ]; do
  case "$1" in
    --out) out="$2"; shift 2 ;;
    --version) version="$2"; shift 2 ;;
    --lint) lint=true; shift ;;
    # The runtime, the SDK and the extensions are installed already, as in
    # a container image that carries them.
    --no-deps) deps=false; shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
arch=$(flatpak --default-arch)
mkdir -p "$out"
out=$(cd "$out" && pwd)

"$here/generate-sources.sh"

if $deps; then
  flatpak remote-add --user --if-not-exists flathub "$FLATHUB"
  deps_args=(--install-deps-from=flathub)
else
  deps_args=()
fi
flatpak-builder --user "${deps_args[@]}" --force-clean --disable-rofiles-fuse --state-dir="$out/state" \
  --repo="$out/repo" --default-branch=stable "$out/build" "$here/$APP_ID.yml"

bundle="upnext-$version-$arch.flatpak"
flatpak build-bundle --runtime-repo="$FLATHUB" "$out/repo" "$out/$bundle" "$APP_ID" stable
(cd "$out" && sha256sum -b "$bundle" > "$bundle.sha256")

if $lint; then
  # Flathub's build image has the linter on its PATH; elsewhere it comes
  # with the org.flatpak.Builder app.
  if command -v flatpak-builder-lint >/dev/null; then
    linter=(flatpak-builder-lint)
  else
    flatpak install --user -y --noninteractive flathub org.flatpak.Builder
    linter=(flatpak run --command=flatpak-builder-lint org.flatpak.Builder)
  fi
  # The findings lint-exceptions.json lists are the permissions Flathub's
  # reviewers have to grant (README.md, "Permissions"); any other fails.
  exceptions=(--exceptions --user-exceptions "$here/lint-exceptions.json")
  # From OUT, where the linter's flatpak-builder leaves its .flatpak-builder
  # folder, rather than in the checkout.
  (cd "$out" && "${linter[@]}" "${exceptions[@]}" manifest "$here/$APP_ID.yml")
  (cd "$out" && "${linter[@]}" "${exceptions[@]}" repo "$out/repo")
fi
echo "Built $out/$bundle"
