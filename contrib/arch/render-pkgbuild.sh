#!/usr/bin/env bash
# Writes a release's PKGBUILD: contrib/arch/upnext-bin/PKGBUILD with
# the tag, the version and the SHA-256 of both Linux archives filled in. The
# release workflow's build-arch-package job runs it through
# contrib/arch/build-package.sh and attaches the result; by hand:
#
#   contrib/arch/render-pkgbuild.sh --tag v0.1.0 --dist-dir target/distrib > PKGBUILD
#
# Options:
#   --tag TAG         the release's tag, vX.Y.Z or vX.Y.Z-pre (required)
#   --dist-dir DIR    the folder with dist's Linux archives and their .sha256
#                     files (required)
#   --out FILE        where the PKGBUILD goes; standard output by default
#   --allow-missing   write SKIP for an architecture whose archive isn't in
#                     --dist-dir, for a local build of one architecture. A
#                     PKGBUILD written this way must never be published.
#
# The checksums come from each archive's .sha256 file, as dist writes it; an
# archive without one is hashed here instead.
set -euo pipefail

die() { echo "render-pkgbuild.sh: $*" >&2; exit 1; }

root=$(cd "$(dirname "$0")/../.." && pwd)
tag='' dist_dir='' out='' allow_missing=false
while [ $# -gt 0 ]; do
  case $1 in
    --tag) tag=${2:?}; shift 2 ;;
    --dist-dir) dist_dir=${2:?}; shift 2 ;;
    --out) out=${2:?}; shift 2 ;;
    --allow-missing) allow_missing=true; shift ;;
    *) die "unknown option $1 (see the comment at the top)" ;;
  esac
done
[ -n "$tag" ] || die "--tag is required"
[ -n "$dist_dir" ] || die "--dist-dir is required"
[ -d "$dist_dir" ] || die "$dist_dir isn't a folder"
[[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] || die "the tag \"$tag\" isn't vX.Y.Z or vX.Y.Z-pre"

# pkgver can't hold a hyphen, so v0.2.0-rc.1 becomes 0.2.0rc.1, which vercmp
# sorts after 0.1.9 and before 0.2.0rc.2 and 0.2.0. Writing 0.2.0.rc.1 or
# 0.2.0_rc.1 instead would sort after 0.2.0.
pkgver=${tag#v}
pkgver=${pkgver//-/}

checksum() {
  local archive="$dist_dir/upnext-$1-unknown-linux-gnu.tar.xz" sum _
  if [ -f "$archive.sha256" ]; then
    read -r sum _ < "$archive.sha256"
  elif [ -f "$archive" ]; then
    read -r sum _ < <(sha256sum "$archive")
  elif $allow_missing; then
    echo "render-pkgbuild.sh: no $archive, so its checksum is SKIP; don't publish this PKGBUILD" >&2
    echo SKIP
    return
  else
    die "neither $archive nor its .sha256 is there"
  fi
  # dist may write the hash as "<sha256> *<file>"; only the hash matters.
  [[ $sum =~ ^[0-9a-f]{64}$ ]] || die "$archive.sha256 doesn't start with a SHA-256"
  echo "$sum"
}
sha_x86_64=$(checksum x86_64)
sha_aarch64=$(checksum aarch64)

rendered=$(sed -e "s|@TAG@|$tag|" -e "s|@PKGVER@|$pkgver|" \
  -e "s|@SHA256_X86_64@|$sha_x86_64|" -e "s|@SHA256_AARCH64@|$sha_aarch64|" \
  "$root/contrib/arch/upnext-bin/PKGBUILD")
! grep -q '@[A-Z0-9_]*@' <<<"$rendered" || die "a placeholder in the template wasn't filled in"

if [ -n "$out" ]; then
  printf '%s\n' "$rendered" > "$out"
else
  printf '%s\n' "$rendered"
fi
