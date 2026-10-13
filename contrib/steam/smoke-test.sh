#!/bin/sh
# Run the Linux depot of UpNext's Steam copy on a system with no GTK, no
# WebKitGTK and no GLib, to show it carries everything but what
# linux-exclude.txt leaves to the computer.
#
#   smoke-test.sh DEPOT [IMAGE]
#
# IMAGE (ubuntu:24.04 by default) must have the same glibc as the machine
# the depot was made on or a newer one: ubuntu:22.04 for a release built on
# Ubuntu 22.04. Inside it, only what a computer always gives is installed:
# Mesa (the graphics stack, with X11's and Wayland's client libraries) and
# fontconfig with one font. Needs Docker, and Xvfb for the window; with
# ImageMagick's `import` too, it checks the window drew something.
#
# In a fresh home folder it runs `upnext --version`, a first `upnext serve`
# (which makes the data folder), then the launcher with `--big-screen` as
# Steam would, and checks that the window's server answers, that
# WebKitGTK's web and network processes start from the depot, and which
# libraries outside the depot each process loaded.
set -eu

depot=$(readlink -f "${1:?usage: smoke-test.sh DEPOT [IMAGE]}")
image=${2:-ubuntu:24.04}
[ -x "$depot/upnext.sh" ] || { echo "$depot has no upnext.sh" >&2; exit 1; }
# The notices of what is built into the programs, beside the bundled
# libraries' licences.
for f in THIRD_PARTY_NOTICES.txt LICENSE-MIT LICENSE-APACHE share/licenses/PACKAGES.txt; do
  [ -s "$depot/$f" ] || { echo "$depot has no $f" >&2; exit 1; }
done

display=:97
xvfb_pid=
cleanup() {
  if [ -n "$xvfb_pid" ]; then kill "$xvfb_pid" 2>/dev/null || true; fi
}
trap cleanup EXIT
Xvfb "$display" -screen 0 1280x800x24 -nolisten tcp >/dev/null 2>&1 &
xvfb_pid=$!
i=0
until [ -S "/tmp/.X11-unix/X${display#:}" ]; do
  i=$((i + 1)); [ "$i" -lt 50 ] || { echo "Xvfb didn't start" >&2; exit 1; }
  sleep 0.1
done

shots=$(mktemp -d)
# The proxy variables pass through for a machine that reaches the package
# archive only through one; nothing else from this machine goes in.
docker run --rm --network host \
  -e http_proxy -e https_proxy -e HTTP_PROXY -e HTTPS_PROXY -e no_proxy -e NO_PROXY \
  -e DISPLAY="$display" -e UPNEXT_CLAUDE_BIN=/nonexistent -e UPNEXT_CODEX_BIN=/nonexistent \
  -v /tmp/.X11-unix:/tmp/.X11-unix:ro -v "$depot:/opt/upnext:ro" -v "$shots:/shots" \
  "$image" bash -euc '
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -qq >/dev/null
    apt-get install -y -qq --no-install-recommends libegl1 libgl1 libgles2 libgbm1 libegl-mesa0 libgl1-mesa-dri fontconfig fonts-dejavu-core ca-certificates curl >/dev/null
    echo "== Not on this system (from the linker cache):"
    for lib in libgtk-3 libwebkit2gtk libjavascriptcore libsoup libglib-2.0 libgio-2.0 libgstreamer libmanette libgdk_pixbuf libpango libcairo; do
      if ldconfig -p | grep -q "$lib"; then echo "  $lib IS INSTALLED, so this test proves less"; exit 1; fi
      echo "  $lib"
    done

    useradd -m player
    run() { su player -s /bin/bash -c "cd /home/player && export DISPLAY=$DISPLAY UPNEXT_CLAUDE_BIN=/nonexistent UPNEXT_CODEX_BIN=/nonexistent UPNEXT_PORT=8797 && $*"; }
    health() { curl -sf http://127.0.0.1:8797/api/health; }
    wait_health() {
      for _ in $(seq 1 100); do health 2>/dev/null | grep -q "\"app\":\"upnext\"" && return 0; sleep 0.2; done
      return 1
    }

    echo "== upnext --version"
    run /opt/upnext/bin/upnext --version

    echo "== First run of upnext serve, in a home folder with nothing in it"
    run "setsid /opt/upnext/bin/upnext serve >/tmp/serve.log 2>&1 &"
    wait_health || { cat /tmp/serve.log; exit 1; }
    health; echo
    test -f /home/player/.local/share/upnext/upnext.db && echo "made /home/player/.local/share/upnext/upnext.db"
    pkill -f "upnext serve"; sleep 1

    echo "== The launcher, as Steam starts it"
    run "setsid /opt/upnext/upnext.sh --big-screen >/tmp/app.log 2>&1 &"
    wait_health || { cat /tmp/app.log; exit 1; }
    health; echo
    # Each process counts by its program, not its command line, which su'"'"'s
    # shell shares; only their own user may read where they came from.
    cat >/tmp/inspect.sh <<"EOS"
webkit=/opt/upnext/lib/x86_64-linux-gnu/webkit2gtk-4.1
status=0
for program in /opt/upnext/bin/upnext-app $webkit/WebKitWebProcess $webkit/WebKitNetworkProcess; do
  pid=
  for p in /proc/[0-9]*; do
    [ "$(readlink "$p/exe" 2>/dev/null)" = "$program" ] && { pid=${p#/proc/}; break; }
  done
  if [ -z "$pid" ]; then echo "$program is not running"; status=1; continue; fi
  echo "== $program loaded, from outside the depot:"
  grep -o "/[^ ]*\.so[^ ]*" /proc/$pid/maps | sort -u | grep -v "^/opt/upnext/" | sed "s/^/  /"
done
exit $status
EOS
    status=0
    echo "== Add to Steam, which the Steam copy leaves to Steam"
    shortcut=$(curl -sf http://127.0.0.1:8797/api/steam/shortcut)
    echo "$shortcut"
    case $shortcut in *"Steam copy"*) ;; *) echo "The window doesn'"'"'t know it is the Steam copy"; status=1 ;; esac
    sleep 8
    su player -s /bin/sh -c "sh /tmp/inspect.sh" || status=1
    # The window is up: let the screen be taken now, while it still shows.
    touch /shots/ready
    for _ in $(seq 1 100); do [ -f /shots/taken ] && break; sleep 0.2; done
    [ -f /home/player/.cache/upnext/steam-copy/loaders.cache ] && echo "== gdk-pixbuf loaders: $(grep -c "^\"/opt/upnext" /home/player/.cache/upnext/steam-copy/loaders.cache) from the depot"
    echo "== The window'"'"'s log"
    cat /tmp/app.log
    pkill -f upnext-app || true
    exit $status
  ' &
runner=$!

# The window draws while the container waits, so look at the screen then.
while kill -0 "$runner" 2>/dev/null && [ ! -f "$shots/ready" ]; do sleep 0.2; done
if [ -f "$shots/ready" ] && command -v import >/dev/null; then
  DISPLAY=$display import -window root "$shots/window.png" 2>/dev/null || true
fi
touch "$shots/taken"
wait "$runner"
if [ -f "$shots/window.png" ]; then
  spread=$(convert "$shots/window.png" -format '%[fx:standard_deviation]' info:)
  echo "== The screen with the window up: $shots/window.png (spread of its pixels $spread; 0 is blank)"
  awk -v s="$spread" 'BEGIN { exit !(s > 0.02) }' || { echo "The window drew nothing" >&2; exit 1; }
fi
echo "The depot ran on $image."
