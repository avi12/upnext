#!/bin/sh
# Upload the depots build-depots.sh made to Steam, as one build set live on
# a beta branch (docs/dev/RELEASE.md, "The Steam copy").
#
#   upload.sh --depots DIR [--branch beta] [--desc TEXT] [--dry-run]
#
# The ids come from Steamworks, in STEAM_APP_ID, STEAM_DEPOT_LINUX and
# STEAM_DEPOT_WINDOWS; STEAM_USERNAME is the builder account, whose sign-in
# steamcmd has saved already (RELEASE.md says how). STEAMCMD is the
# steamcmd to run (`steamcmd` by default). With --dry-run it fills in the
# build files, checks them and both depots, and says what it would run,
# without steamcmd or any network.
#
# Run it on Linux: SteamPipe keeps the files' executable bits from the
# system steamcmd runs on, and the Linux depot's launcher and programs need
# theirs.
set -eu

here=$(dirname "$(readlink -f "$0")")
depots='' branch=beta desc='' dry=''
while [ $# -gt 0 ]; do
  case $1 in
    --depots) depots=$2; shift 2 ;;
    --branch) branch=$2; shift 2 ;;
    --desc) desc=$2; shift 2 ;;
    --dry-run) dry=1; shift ;;
    *) echo "usage: upload.sh --depots DIR [--branch NAME] [--desc TEXT] [--dry-run]" >&2; exit 2 ;;
  esac
done
[ -n "$depots" ] || { echo "usage: upload.sh --depots DIR [--branch NAME] [--desc TEXT] [--dry-run]" >&2; exit 2; }
depots=$(readlink -f "$depots")

fail() { echo "$*" >&2; exit 1; }
for v in STEAM_APP_ID STEAM_DEPOT_LINUX STEAM_DEPOT_WINDOWS; do
  case $(printenv "$v" || true) in
    '' | *[!0-9]*) fail "$v must be the number Steamworks gave (docs/dev/RELEASE.md, \"The Steam copy\")" ;;
  esac
done
# A build goes live on the default ("public") branch only by hand, once it
# has been checked on a beta branch.
case $branch in
  '' | default | public | *[!a-z0-9_-]*) fail "--branch must name a beta branch made in Steamworks, not \"$branch\"" ;;
esac

# Each depot must be one build-depots.sh made: the Linux one with its
# launcher and programs executable, both with the Steam copy's marker.
if [ ! -x "$depots/linux/upnext.sh" ] || [ ! -x "$depots/linux/bin/upnext-app" ] || [ ! -f "$depots/linux/bin/steam-copy.txt" ]; then
  fail "$depots/linux isn't a Linux depot from build-depots.sh"
fi
if [ ! -f "$depots/windows/upnext-app.exe" ] || [ ! -f "$depots/windows/steam-copy.txt" ]; then
  fail "$depots/windows isn't a Windows depot from build-depots.sh"
fi
version=$("$depots/linux/bin/upnext" --version 2>/dev/null || echo "UpNext")
[ -n "$desc" ] || desc=$version

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/scripts" "$work/output"
# The values go in through awk's environment and are put in as plain text,
# not through gsub, which would read an & in them as the match. VDF strings
# end at a double quote and treat a backslash as an escape, so neither may
# be in them.
case "$desc$depots$work" in *'"'* | *\\*) fail "a double quote or a backslash can't go in a build's description or paths" ;; esac
for f in app_build.vdf depot_build_linux.vdf depot_build_windows.vdf; do
  APP_ID=$STEAM_APP_ID LINUX=$STEAM_DEPOT_LINUX WINDOWS=$STEAM_DEPOT_WINDOWS BRANCH=$branch DESC=$desc \
    ROOT=$depots OUTPUT=$work/output awk '
      function put(s, key, value,   out, i) {
        out = ""
        while ((i = index(s, key)) > 0) { out = out substr(s, 1, i - 1) value; s = substr(s, i + length(key)) }
        return out s
      }
      {
        s = put($0, "@APP_ID@", ENVIRON["APP_ID"]); s = put(s, "@LINUX_DEPOT_ID@", ENVIRON["LINUX"])
        s = put(s, "@WINDOWS_DEPOT_ID@", ENVIRON["WINDOWS"]); s = put(s, "@BRANCH@", ENVIRON["BRANCH"])
        s = put(s, "@DESC@", ENVIRON["DESC"]); s = put(s, "@CONTENT_ROOT@", ENVIRON["ROOT"]); s = put(s, "@BUILD_OUTPUT@", ENVIRON["OUTPUT"])
        print s
      }' "$here/build/$f" >"$work/scripts/$f"
done
if grep -n '@[A-Z_]*@' "$work/scripts"/*.vdf; then
  fail "a value above wasn't filled in"
fi

echo "App $STEAM_APP_ID, build \"$desc\", set live on the $branch branch:"
echo "  Linux depot $STEAM_DEPOT_LINUX: $(find "$depots/linux" -type f | wc -l) files, $(du -sh "$depots/linux" | cut -f 1)"
echo "  Windows depot $STEAM_DEPOT_WINDOWS: $(find "$depots/windows" -type f | wc -l) files, $(du -sh "$depots/windows" | cut -f 1)"
if [ -n "$dry" ]; then
  for f in "$work/scripts"/*.vdf; do
    echo "== $(basename "$f")"
    grep -v '^//' "$f"
  done
  echo "Dry run: would run ${STEAMCMD:-steamcmd} +login ${STEAM_USERNAME:-<builder account>} +run_app_build $work/scripts/app_build.vdf +quit"
  exit 0
fi

[ -n "${STEAM_USERNAME:-}" ] || fail "STEAM_USERNAME must name the builder account"
"${STEAMCMD:-steamcmd}" +login "$STEAM_USERNAME" +run_app_build "$work/scripts/app_build.vdf" +quit
echo "Uploaded. Check the build on the $branch branch in Steamworks, then set it live on the default branch there."
