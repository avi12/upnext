#!/usr/bin/env bash
# The packages' smoke test: the release workflow's
# build-linux-packages job runs it on the amd64 .deb in debian:12 and
# ubuntu:24.04 and the amd64 .rpm in fedora:44, its build-arch-package job
# on the Arch package in archlinux:latest, and it runs by hand the same way:
#
#   contrib/linux/smoke-test.sh target/linux-packages/upnext_0.1.0-1_amd64.deb debian:12 0.1.0
#   contrib/linux/smoke-test.sh target/arch-package/upnext-bin-0.1.0-1-x86_64.pkg.tar.zst archlinux:latest 0.1.0
#
# The .rpm is meant for openSUSE too; nothing runs it there yet, but it can
# be checked by hand the same way, in opensuse/tumbleweed.
#
# In a fresh container of the image it installs the package with apt, dnf,
# zypper or pacman, which brings WebKitGTK and GTK from the distribution's own
# repositories, so the declared dependencies are checked too: neither
# program may miss a library. Then `upnext --version` must print the
# version, `upnext doctor` must run, and `upnext serve` must answer
# /api/health and serve the built-in web UI and the third-party notices,
# which the package must carry as a file too. Last, it removes the package
# and checks the data folder is still there. Needs docker and the network
# for the distribution's packages. SMOKE_DOCKER_ARGS adds arguments to
# `docker run` (`--network host` behind a proxy on this machine, say).
set -euo pipefail

# Whether LINE is what `upnext healthcheck` says for VERSION: "ok: UpNext
# 0.1.0", or, for a build that knows its commit, "ok: UpNext 0.1.0
# (abc1234)". Every Actions build knows it (build.rs reads GITHUB_SHA), so
# the release jobs always see the second form. The commit is seven hex
# digits, or more when git needs them to tell commits apart. The function
# goes into the container with the script below, and `smoke-test.sh
# --health-line VERSION LINE` runs it alone, for the test in
# src/cli/running.rs.
health_line_ok() {
  local commit
  [ "$2" = "ok: UpNext $1" ] && return 0
  case $2 in
    "ok: UpNext $1 ("*")") ;;
    *) return 1 ;;
  esac
  commit=${2#"ok: UpNext $1 ("}
  commit=${commit%")"}
  [[ $commit =~ ^[0-9a-f]{7,40}$ ]]
}
if [ "${1:-}" = --health-line ]; then
  health_line_ok "${2:?usage: smoke-test.sh --health-line VERSION LINE}" "${3-}"
  exit
fi

package=${1:?usage: smoke-test.sh PACKAGE IMAGE VERSION}
image=${2:?usage: smoke-test.sh PACKAGE IMAGE VERSION}
version=${3:?usage: smoke-test.sh PACKAGE IMAGE VERSION}
[ -f "$package" ] || { echo "smoke-test.sh: $package isn't a file" >&2; exit 1; }
dir=$(cd "$(dirname "$package")" && pwd)
file=$(basename "$package")

echo "== $file in $image"
# shellcheck disable=SC2086 # SMOKE_DOCKER_ARGS is a list of arguments.
{ declare -f health_line_ok; cat <<'EOF'; } | docker run --rm -i ${SMOKE_DOCKER_ARGS:-} -v "$dir:/pkg:ro" -e "PKG=/pkg/$file" -e "VERSION=$version" \
  "$image" bash -s
set -euo pipefail
step() { echo "== $*"; }
fail() { echo "FAIL: $*" >&2; exit 1; }

step "install"
case $PKG in
  *.deb)
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -qq
    apt-get install -y -qq "$PKG" > /tmp/install.log 2>&1 || { cat /tmp/install.log; fail "apt-get install"; }
    remove() { apt-get remove -y -qq upnext; }
    ;;
  *.rpm)
    if command -v dnf > /dev/null; then
      dnf install -y -q "$PKG" > /tmp/install.log 2>&1 || { cat /tmp/install.log; fail "dnf install"; }
      remove() { dnf remove -y -q upnext; }
    else
      # openSUSE: the package isn't signed, which zypper refuses unless told.
      zypper --non-interactive install --allow-unsigned-rpm "$PKG" > /tmp/install.log 2>&1 ||
        { cat /tmp/install.log; fail "zypper install"; }
      remove() { zypper --non-interactive remove upnext; }
    fi
    ;;
  *.pkg.tar.zst)
    # The image has no sync databases, so pacman couldn't resolve the
    # dependencies before -Syu fetches them.
    { pacman -Syu --noconfirm && pacman -U --noconfirm "$PKG"; } > /tmp/install.log 2>&1 ||
      { cat /tmp/install.log; fail "pacman -U"; }
    remove() { pacman -R --noconfirm upnext-bin; }
    ;;
  *) fail "$PKG isn't a .deb, an .rpm or a .pkg.tar.zst" ;;
esac
# apt shows the post-install script's message; dnf may keep it to its log,
# so for an rpm the script itself is checked. The Arch package has no
# install script: docs/running.md says how to start the service, and the unit's
# header says it too.
case $PKG in
  *.deb) grep -q 'systemctl --user enable --now upnext' /tmp/install.log || fail "no post-install message" ;;
  *.rpm) scripts=$(rpm -q --scripts upnext) && grep -q 'systemctl --user enable --now upnext' <<<"$scripts" || fail "no post-install script" ;;
  *.pkg.tar.zst) grep -q 'systemctl --user enable --now upnext' /usr/lib/systemd/user/upnext.service || fail "the unit's header" ;;
esac
# Not the docs: the container images install packages without them.
for f in /usr/bin/upnext /usr/bin/upnext-app /usr/share/applications/upnext.desktop \
  /usr/share/icons/hicolor/scalable/apps/upnext.svg /usr/share/icons/hicolor/256x256/apps/upnext.png \
  /usr/lib/systemd/user/upnext.service; do
  [ -e "$f" ] || fail "$f isn't installed"
done
grep -qx 'ExecStart=/usr/bin/upnext serve' /usr/lib/systemd/user/upnext.service || fail "the unit's ExecStart"
# The third-party notices, from the package's own list of files: a container
# image may leave the docs out when it installs (Debian's drop /usr/share/doc).
# Each list is read whole before grep -q, which with pipefail would otherwise
# fail the check by breaking the pipe when it finds the line early.
case $PKG in
  *.deb) files=$(dpkg-deb -c "$PKG") && grep -q ' \./usr/share/doc/upnext/THIRD_PARTY_NOTICES\.txt$' <<<"$files" ;;
  *.rpm) files=$(rpm -qlp "$PKG") && grep -qx '/usr/share/licenses/upnext/THIRD_PARTY_NOTICES.txt' <<<"$files" ;;
  *.pkg.tar.zst) files=$(pacman -Qlpq "$PKG") && grep -qx '/usr/share/licenses/upnext-bin/THIRD_PARTY_NOTICES.txt' <<<"$files" ;;
esac || fail "the package has no THIRD_PARTY_NOTICES.txt"

step "libraries"
missing=$(ldd /usr/bin/upnext /usr/bin/upnext-app | grep 'not found' || true)
[ -z "$missing" ] || fail "missing libraries: $missing"

step "upnext --version"
out=$(upnext --version)
echo "$out"
case " $out " in *" $VERSION "*) ;; *) fail "the version isn't $VERSION" ;; esac

export UPNEXT_DATA_DIR=/tmp/upnext-data
step "upnext doctor"
upnext doctor

step "upnext serve"
upnext serve > /tmp/serve.log 2>&1 &
for _ in $(seq 1 30); do
  if upnext healthcheck > /tmp/health.log 2>&1; then break; fi
  sleep 1
done
cat /tmp/health.log
answered=no
while IFS= read -r line; do
  if health_line_ok "$VERSION" "$line"; then answered=yes; fi
done < /tmp/health.log
[ "$answered" = yes ] || { cat /tmp/serve.log; fail "no answer on /api/health"; }
# The built-in web UI, not the "no web UI" placeholder; bash's /dev/tcp
# needs nothing installed.
exec 3<>/dev/tcp/127.0.0.1/8787
printf 'GET / HTTP/1.0\r\nHost: localhost\r\n\r\n' >&3
page=$(cat <&3)
exec 3<&-
grep -q '<script' <<<"$page" || fail "/ isn't the web UI"
# Settings › About's link to the notices built into the program.
exec 3<>/dev/tcp/127.0.0.1/8787
printf 'GET /third-party-notices.txt HTTP/1.0\r\nHost: localhost\r\n\r\n' >&3
page=$(cat <&3) || fail "couldn't fetch /third-party-notices.txt"
exec 3<&-
grep -q 'UpNext: third-party notices' <<<"$page" || fail "/third-party-notices.txt isn't the notices"
kill %1
wait %1 || true
[ -f "$UPNEXT_DATA_DIR/upnext.db" ] || fail "no database in $UPNEXT_DATA_DIR"

step "remove"
remove
[ ! -e /usr/bin/upnext ] || fail "/usr/bin/upnext is still there"
[ -f "$UPNEXT_DATA_DIR/upnext.db" ] || fail "removing the package took the database"
step "ok"
EOF
