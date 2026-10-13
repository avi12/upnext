#!/usr/bin/env bash
# Builds the Arch Linux package from dist's Linux archives, as the
# release workflow's build-arch-package job does: it renders the release's
# PKGBUILD (contrib/arch/render-pkgbuild.sh), puts the x86_64 archive beside
# it under the name the PKGBUILD's source gives it, so makepkg checks it
# against its SHA-256 instead of downloading it, builds the package with
# makepkg and checks the PKGBUILD and the package with namcap. By hand, after
# `dist build` has written the archive to target/distrib:
#
#   contrib/arch/build-package.sh --tag v0.1.0 --dist-dir target/distrib
#
# It writes PKGBUILD and upnext-bin-<pkgver>-1-x86_64.pkg.tar.zst, with a
# .sha256 beside the package, to --out-dir. The package is then smoke-tested
# with contrib/linux/smoke-test.sh in archlinux:latest.
#
# Options:
#   --tag TAG         the release's tag, vX.Y.Z or vX.Y.Z-pre (required)
#   --dist-dir DIR    the folder with dist's Linux archives (required)
#   --out-dir DIR     where the files go; target/arch-package by default
#   --image IMAGE     the container to build in; by default archlinux:base-devel,
#                     pinned to the digest below (docs/dev/RELEASE.md, "The
#                     Arch images", says how to move it on)
#   --host            build with this machine's makepkg instead of a container,
#                     as a user other than root (makepkg refuses root); namcap
#                     runs if it's installed.
#   --allow-missing   passed to render-pkgbuild.sh, for an archive of one
#                     architecture; the PKGBUILD it writes isn't publishable.
#
# In a container (the default) it needs docker and Arch's mirrors: it updates
# the image, installs namcap and the package's dependencies (so namcap can
# tell which package each linked library belongs to), and runs makepkg as an
# unprivileged user. Any line namcap marks as an error fails the build; its
# warnings are printed. ARCH_DOCKER_ARGS adds arguments to `docker run`, and
# MAKEPKG_FLAGS to makepkg, either way: --nodeps where pacman doesn't know
# the dependencies, such as on a system other than Arch.
set -euo pipefail

die() { echo "build-package.sh: $*" >&2; exit 1; }

root=$(cd "$(dirname "$0")/../.." && pwd)
tag='' dist_dir='' out_dir='' image=archlinux:base-devel@sha256:996c3a1d6b0d87b01242f6fcd8cfa3ad3eece1a67ab5c8f108e20af1d7b97bdd host=false render_args=()
while [ $# -gt 0 ]; do
  case $1 in
    --tag) tag=${2:?}; shift 2 ;;
    --dist-dir) dist_dir=${2:?}; shift 2 ;;
    --out-dir) out_dir=${2:?}; shift 2 ;;
    --image) image=${2:?}; shift 2 ;;
    --host) host=true; shift ;;
    --allow-missing) render_args+=(--allow-missing); shift ;;
    *) die "unknown option $1 (see the comment at the top)" ;;
  esac
done
[ -n "$tag" ] || die "--tag is required"
[ -n "$dist_dir" ] || die "--dist-dir is required"
archive="$dist_dir/upnext-x86_64-unknown-linux-gnu.tar.xz"
[ -f "$archive" ] || die "$archive isn't there"

out_dir=${out_dir:-$root/target/arch-package}
mkdir -p "$out_dir"
out_dir=$(cd "$out_dir" && pwd)
build=$out_dir/build
rm -rf "$build" "$out_dir"/upnext-bin-*.pkg.tar.zst*
mkdir -p "$build"

"$root/contrib/arch/render-pkgbuild.sh" --tag "$tag" --dist-dir "$dist_dir" --out "$out_dir/PKGBUILD" "${render_args[@]}"
cp "$out_dir/PKGBUILD" "$build/PKGBUILD"
cp "$archive" "$build/upnext-$tag-x86_64.tar.xz"

if $host; then
  [ "$(id -u)" != 0 ] || die "makepkg refuses to run as root; run --host as another user"
  # shellcheck disable=SC2086 # MAKEPKG_FLAGS is a list of arguments.
  (cd "$build" && PKGEXT=.pkg.tar.zst makepkg -f --noconfirm ${MAKEPKG_FLAGS:-})
  if command -v namcap > /dev/null; then
    (cd "$build" && namcap PKGBUILD ./*.pkg.tar.zst) | tee "$build/namcap.log"
    ! grep -q ' E: ' "$build/namcap.log" || die "namcap found errors"
  else
    echo "build-package.sh: namcap isn't installed, so the package wasn't checked with it" >&2
  fi
else
  # shellcheck disable=SC2086 # ARCH_DOCKER_ARGS is a list of arguments.
  docker run --rm -i ${ARCH_DOCKER_ARGS:-} -v "$build:/build" \
    -e "HOST_UID=$(id -u)" -e "HOST_GID=$(id -g)" -e "MAKEPKG_FLAGS=${MAKEPKG_FLAGS:-}" "$image" bash -s <<'IN_CONTAINER'
set -euo pipefail
step() { echo "== $*"; }
cd /build
# Whatever happens, the files go back to the user who ran the script.
trap 'chown -R "$HOST_UID:$HOST_GID" /build' EXIT

step "pacman -Syu, namcap and the package's dependencies"
mapfile -t deps < <(bash -c 'source ./PKGBUILD && printf "%s\n" "${depends[@]}"')
pacman -Syu --noconfirm --needed namcap "${deps[@]}" > /tmp/pacman.log 2>&1 || { cat /tmp/pacman.log; exit 1; }

step "makepkg"
useradd -m builder
chown -R builder: /build
# shellcheck disable=SC2086 # MAKEPKG_FLAGS is a list of arguments.
runuser -u builder -- env PKGEXT=.pkg.tar.zst makepkg -f --noconfirm $MAKEPKG_FLAGS

step "namcap"
namcap PKGBUILD ./*.pkg.tar.zst | tee namcap.log
if grep -q ' E: ' namcap.log; then
  echo "FAIL: namcap found errors" >&2
  exit 1
fi
IN_CONTAINER
fi

shopt -s nullglob
packages=("$build"/upnext-bin-*.pkg.tar.zst)
[ ${#packages[@]} = 1 ] || die "expected one package in $build, found ${#packages[@]}"
file=$(basename "${packages[0]}")
cp "${packages[0]}" "$out_dir/"
(cd "$out_dir" && sha256sum "$file" > "$file.sha256")
echo "$out_dir/$file"
