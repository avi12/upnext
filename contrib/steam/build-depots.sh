#!/bin/sh
# Lay out both depots of UpNext's Steam copy from a release's archives
# (contrib/steam/README.md).
#
#   build-depots.sh --tag v0.1.0 --out DIR     fetch the tag's release files
#   build-depots.sh --from DIR --out DIR       use archives already in DIR
#
# Either way it uses dist's x86_64 Linux archive and Windows zip, checks
# each against its .sha256 file, and makes DIR/linux (bundle-linux.py:
# the programs, WebKitGTK and the libraries they need, the launcher) and
# DIR/windows (the two programs, the licences and the third-party notices),
# each with the marker file that tells UpNext it is the Steam copy. Nothing
# is uploaded; that is upload.sh. The Linux depot must be made on the system
# the programs were built on (Ubuntu 22.04 for a release), so the libraries
# it copies are the ones they were built against.
set -eu

here=$(dirname "$(readlink -f "$0")")
repo=zytocc/upnext
linux=upnext-x86_64-unknown-linux-gnu.tar.xz
windows=upnext-x86_64-pc-windows-msvc.zip
tag='' from='' out=''
while [ $# -gt 0 ]; do
  case $1 in
    --tag) tag=$2; shift 2 ;;
    --from) from=$2; shift 2 ;;
    --out) out=$2; shift 2 ;;
    *) echo "usage: build-depots.sh (--tag vX.Y.Z | --from DIR) --out DIR" >&2; exit 2 ;;
  esac
done
if [ -z "$out" ] || { [ -z "$tag" ] && [ -z "$from" ]; }; then
  echo "usage: build-depots.sh (--tag vX.Y.Z | --from DIR) --out DIR" >&2
  exit 2
fi
[ -e "$out" ] && { echo "$out exists already; give a new folder" >&2; exit 1; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

if [ -n "$tag" ]; then
  from=$work/release
  mkdir -p "$from"
  for f in $linux $linux.sha256 $windows $windows.sha256; do
    curl --proto '=https' --tlsv1.2 -fsSL -o "$from/$f" "https://github.com/$repo/releases/download/$tag/$f"
  done
fi

# dist writes each checksum as "<sha256>  <file name>"; only the hash counts,
# checked against the file whatever it is called here.
for f in $linux $windows; do
  if [ ! -f "$from/$f" ] || [ ! -f "$from/$f.sha256" ]; then
    echo "$from has no $f with its .sha256" >&2
    exit 1
  fi
  want=$(cut -d ' ' -f 1 "$from/$f.sha256")
  have=$(sha256sum "$from/$f" | cut -d ' ' -f 1)
  [ "$want" = "$have" ] || { echo "$f doesn't match its .sha256" >&2; exit 1; }
  echo "$f: SHA-256 $have, as its .sha256 says"
done

mkdir -p "$work/linux" "$work/windows" "$out"
tar -xJf "$from/$linux" -C "$work/linux"
bin_dir=$(dirname "$(find "$work/linux" -type f -name upnext-app | head -n 1)")
python3 "$here/bundle-linux.py" --bin-dir "$bin_dir" --out "$out/linux"

python3 -m zipfile -e "$from/$windows" "$work/windows"
exe=$(find "$work/windows" -type f -name upnext.exe | head -n 1)
[ -n "$exe" ] || { echo "$windows has no upnext.exe" >&2; exit 1; }
src=$(dirname "$exe")
mkdir -p "$out/windows"
for f in "$src"/*; do
  case $(basename "$f") in
    upnext.exe | upnext-app.exe | LICENSE* | THIRD_PARTY_NOTICES* | README* | CHANGELOG*) cp "$f" "$out/windows/" ;;
  esac
done
[ -f "$out/windows/upnext-app.exe" ] || { echo "$windows has no upnext-app.exe" >&2; exit 1; }
[ -f "$out/windows/THIRD_PARTY_NOTICES.txt" ] || { echo "$windows has no THIRD_PARTY_NOTICES.txt" >&2; exit 1; }
# The same marker as the Linux depot's (bundle-linux.py), beside the programs.
cp "$out/linux/bin/steam-copy.txt" "$out/windows/steam-copy.txt"

echo "Depots in $out: linux ($(du -sh "$out/linux" | cut -f 1)), windows ($(du -sh "$out/windows" | cut -f 1))"
