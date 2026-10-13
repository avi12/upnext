#!/usr/bin/env bash
# The image's smoke test: .github/workflows/docker.yml runs it on
# each build before pushing, and it runs by hand the same way:
#
#   contrib/docker/smoke-test.sh upnext:dev
#
# It starts the image as compose.yaml does (read-only, no capabilities, no
# new privileges) with an empty volume, waits for the health
# check, checks the licences and third-party notices are in the image and
# served, pairs with the code from the log the way the host's browser does
# (a request from another address, naming localhost), checks a token made
# with `docker exec`, stops it, and checks the database. Then it starts
# another with UPNEXT_INSTALL_CLAUDE=1 against a stand-in of Anthropic's
# release site (a python container serving a fake `claude`), so the install,
# its checksum, the sign-in token and a test call through the CLI are checked
# without reaching Anthropic. Requests come from a curl container on the same
# network (CURL_IMAGE), so nothing needs a published port. Needs docker;
# sqlite3 or python3 here, or it uses a python image for the database check.
# An image for another architecture (arm64 on an amd64 machine) runs under
# QEMU, which docker uses when binfmt has it.
set -euo pipefail

image=${1:?usage: smoke-test.sh IMAGE}
curl_image=${CURL_IMAGE:-curlimages/curl:8.11.1}
python_image=${PYTHON_IMAGE:-python:3-slim}
run=upnext-smoke-$$
tmp=$(mktemp -d)
# compose.yaml's read_only, tmpfs, security_opt and cap_drop.
hardened="--read-only --tmpfs /tmp --security-opt no-new-privileges --cap-drop ALL"

cleanup() {
  docker rm -f "$run" "$run-claude" "$run-releases" >/dev/null 2>&1 || true
  docker network rm "$run" >/dev/null 2>&1 || true
  docker volume rm "$run" "$run-claude" >/dev/null 2>&1 || true
  rm -rf "$tmp"
}
trap cleanup EXIT

step() { echo "== $*"; }
fail() {
  echo "FAIL: $*" >&2
  # Before the first container starts (the size step) there's no log.
  if docker container inspect "${failing:-$run}" >/dev/null 2>&1; then
    echo "--- container log:" >&2
    docker logs "${failing:-$run}" >&2 2>&1 || true
  fi
  exit 1
}

# A request as the host's browser makes it: from another address, to localhost.
http() { docker run --rm --network "$run" "$curl_image" -sS -H 'Host: localhost:8787' "$@" "http://${host:-upnext}:8787${path}"; }
status() { path=$2 http -o /dev/null -w '%{http_code}' -X "$1" "${@:3}"; }
body() { path=$1 http "${@:2}"; }

wait_healthy() {
  for _ in $(seq 1 60); do
    case $(docker inspect -f '{{.State.Health.Status}} {{.State.Running}}' "${failing:-$run}") in
      "healthy true") return 0 ;;
      *false) fail "the container stopped" ;;
    esac
    sleep 1
  done
  fail "not healthy after 60 s"
}

arch=$(docker image inspect -f '{{.Architecture}}' "$image")
# The size is what the image unpacks to, counted by du inside it (as root, so
# it can read every folder), the same way for both architectures. Summing
# `docker history` gave 0 for the arm64 image on the self-hosted runner,
# whose Docker reported no layer sizes for an image of the other
# architecture, so arm64's budget checked nothing.
#
# Counted that way, Debian's slim image is 79 MB on amd64 and 101 MB on
# arm64, and the whole image was 111 MB and 128 MB on 2026-10-10 (CI's
# readings at 3f4462a); UpNext adds the rest. The server grew from about
# 23 MB as features came in (a stripped amd64 release build was 31.5 MB that
# day), with no single cause, and the amd64 image reached the old 120 MB
# budget as `docker history` counted it. Each budget is about 12 to 14 MB
# over those readings. Fat LTO with one codegen unit would save about 3 MB,
# but it triples the server's build time, for both architectures, on every
# image build.
case $arch in arm64) max_mb=${MAX_IMAGE_MB:-140} ;; *) max_mb=${MAX_IMAGE_MB:-125} ;; esac
step "size ($arch)"
bytes=$(docker run --rm --user 0 --network none --entrypoint du "$image" -sxb / 2>"$tmp/du.err" | cut -f1 || true)
[ "${bytes:-0}" -gt 0 ] 2>/dev/null || { cat "$tmp/du.err" >&2; fail "couldn't measure the image (du gave '${bytes}')"; }
mb=$((bytes / 1000000))
echo "$mb MB (budget $max_mb MB)"
[ "$mb" -lt "$max_mb" ] || fail "the image is $mb MB, over $max_mb MB"

step "start with an empty volume, read-only, without capabilities"
docker network create "$run" >/dev/null
docker volume create "$run" >/dev/null
# shellcheck disable=SC2086 # hardened is a list of arguments.
docker run -d --name "$run" --network "$run" --network-alias upnext \
  $hardened -v "$run:/data" "$image" >/dev/null
wait_healthy
docker exec "$run" upnext healthcheck

step "the log has a pairing code"
code=$(docker logs "$run" 2>&1 | sed -n 's/.*Open http:\/\/localhost:8787 and enter \([0-9]\{3\} [0-9]\{3\}\).*/\1/p' | head -n 1)
[ -n "$code" ] || fail "no pairing code in the log"
echo "code: $code"

step "health and settings say container; nothing works without pairing"
health=$(body /api/v1/health)
echo "$health"
grep -q '"container":true' <<<"$health" || fail "health doesn't say container"
[ "$(status GET /api/v1/stats)" = 401 ] || fail "the API answered without a token"

step "the licences and third-party notices"
for f in LICENSE-MIT LICENSE-APACHE THIRD_PARTY_NOTICES.txt; do
  docker run --rm --network none --entrypoint test "$image" -s "/usr/share/doc/upnext/$f" || fail "the image has no /usr/share/doc/upnext/$f"
done
# Read the whole file before grep: with pipefail, grep -q leaving at the first
# match would break the pipe and fail the check, since the notices are far
# bigger than a pipe's buffer and tini is near the top. The script reads every
# output it searches this way.
notices=$(body /third-party-notices.txt) || fail "couldn't fetch /third-party-notices.txt"
grep -q '^tini 0.19.0 ' <<<"$notices" || fail "/third-party-notices.txt doesn't list tini"

step "pair with the logged code"
headers=$(path=/api/v1/pair http -o /dev/null -D - -X POST -H 'Origin: http://localhost:8787' \
  -H 'Content-Type: application/json' -d "{\"code\":\"$code\"}")
grep -qi '^HTTP/1.1 201' <<<"$headers" || fail "pairing: $headers"
# The cookie is named with the port the server listens on (src/server/pairing.rs),
# which in the container is 8787 whatever port the host publishes.
cookie=$(sed -n 's/^[Ss]et-[Cc]ookie: \(upnext_device_8787=[^;]*\);.*/\1/p' <<<"$headers")
[ -n "$cookie" ] || fail "no cookie: $headers"
[ "$(status GET /api/v1/stats -H "Cookie: $cookie")" = 200 ] || fail "the paired browser was refused"
settings=$(body /api/v1/settings -H "Cookie: $cookie")
for want in '"container":true' '"this_computer":false' '"heroic_dir":null' '"steam_local_dir":null'; do
  grep -q "$want" <<<"$settings" || fail "settings lack $want"
done

step "docker exec: a new code, and a token"
paired=$(docker exec "$run" upnext pair)
echo "$paired"
grep -qE 'enter [0-9]{3} [0-9]{3}' <<<"$paired" || fail "upnext pair"
token=$(docker exec "$run" upnext token create --name smoke --scope read 2>/dev/null)
[ "$(status GET /api/v1/stats -H "Authorization: Bearer $token")" = 200 ] || fail "the token was refused"
[ "$(status POST /api/v1/sync -H "Authorization: Bearer $token" -H 'Origin: http://localhost:8787')" = 403 ] ||
  fail "a read token could write"

step "docker stop: clean, within 10 s"
start=$(date +%s)
docker stop -t 10 "$run" >/dev/null
took=$(($(date +%s) - start))
exit_code=$(docker inspect -f '{{.State.ExitCode}}' "$run")
echo "stopped in ${took} s, exit code $exit_code"
[ "$exit_code" = 0 ] || fail "exit code $exit_code"
[ "$took" -lt 10 ] || fail "took $took s to stop"
log=$(docker logs "$run" 2>&1)
grep -q "UpNext stopped" <<<"$log" || fail "no graceful stop in the log"

step "the database is intact"
docker cp -q "$run:/data/." "$tmp/data"
check="import sqlite3; print(sqlite3.connect('/data/upnext.db').execute('PRAGMA integrity_check').fetchone()[0])"
if command -v sqlite3 >/dev/null; then
  result=$(sqlite3 "$tmp/data/upnext.db" 'PRAGMA integrity_check')
elif command -v python3 >/dev/null; then
  result=$(python3 -c "${check//\/data/$tmp/data}")
else
  result=$(docker run --rm -v "$tmp/data:/data:ro" "$python_image" python3 -c "$check")
fi
echo "integrity_check: $result"
[ "$result" = ok ] || fail "integrity_check: $result"

step "the next start pairs no code by itself"
docker start "$run" >/dev/null
wait_healthy
log=$(docker logs "$run" 2>&1 | tail -n 3)
grep -q "To pair another browser" <<<"$log" || fail "no greeting after a restart"
docker stop -t 10 "$run" >/dev/null

step "UPNEXT_INSTALL_CLAUDE=1: Claude Code from a stand-in release site"
# Anthropic's layout: the channel names a version, whose manifest has each
# platform's SHA-256, beside the binary. The fake installs itself the way
# `claude install` does and answers a call only with the token.
case $arch in amd64) platform=linux-x64 ;; arm64) platform=linux-arm64 ;; *) fail "no Claude Code build for $arch" ;; esac
releases=$tmp/releases
mkdir -p "$releases/0.0.1/$platform"
cat >"$releases/0.0.1/$platform/claude" <<'FAKE'
#!/bin/sh
case "$1" in
  install) mkdir -p "$HOME/.local/bin" && cp "$0" "$HOME/.local/bin/claude" && echo "installed $2" ;;
  --version) echo '0.0.1 (Claude Code)' ;;
  -p)
    cat >/dev/null
    if [ "$CLAUDE_CODE_OAUTH_TOKEN" != smoke-token ]; then
      echo '{"type":"result","subtype":"success","is_error":true,"result":"Not logged in"}'
      exit 1
    fi
    echo '{"type":"result","subtype":"success","is_error":false,"result":"","structured_output":{"ok":true},"usage":{"input_tokens":1,"output_tokens":1},"modelUsage":{"claude-sonnet-5-5":{"outputTokens":1}},"total_cost_usd":0}' ;;
esac
FAKE
echo 0.0.1 >"$releases/stable"
sum=$(sha256sum "$releases/0.0.1/$platform/claude" | cut -d' ' -f1)
echo "{\"version\":\"0.0.1\",\"platforms\":{\"$platform\":{\"checksum\":\"$sum\"}}}" >"$releases/0.0.1/manifest.json"
chmod -R a+rX "$releases"
docker run -d --name "$run-releases" --network "$run" --network-alias releases -v "$releases:/srv:ro" -w /srv \
  "$python_image" python3 -m http.server 8000 >/dev/null
for _ in $(seq 1 30); do
  docker run --rm --network "$run" "$curl_image" -sf -o /dev/null http://releases:8000/stable && break
  sleep 1
done
failing=$run-claude
docker volume create "$run-claude" >/dev/null
# shellcheck disable=SC2086 # hardened is a list of arguments.
docker run -d --name "$run-claude" --network "$run" --network-alias upnext-claude $hardened \
  -v "$run-claude:/data" -e UPNEXT_INSTALL_CLAUDE=1 -e UPNEXT_CLAUDE_RELEASES=http://releases:8000 \
  -e CLAUDE_CODE_OAUTH_TOKEN=smoke-token "$image" >/dev/null
wait_healthy
for _ in $(seq 1 60); do
  log=$(docker logs "$run-claude" 2>&1)
  grep -q "Claude Code 0.0.1 is installed at /data/.local/bin/claude." <<<"$log" && break
  grep -q "Claude Code wasn't installed" <<<"$log" && fail "the install failed"
  sleep 1
done
log=$(docker logs "$run-claude" 2>&1)
grep -q "is installed at" <<<"$log" || fail "not installed after 60 s"
[ "$(docker exec "$run-claude" claude --version)" = '0.0.1 (Claude Code)' ] || fail "claude isn't on PATH"
token=$(docker exec "$run-claude" upnext token create --name smoke --scope full 2>/dev/null)
ai=$(host=upnext-claude path=/api/v1/setup http -H "Authorization: Bearer $token")
for want in '"backend":"claude-cli"' '"claude":"/data/.local/bin/claude"' '"claude_install":null' '"claude_token":true'; do
  grep -q "$want" <<<"$ai" || fail "setup lacks $want: $ai"
done
called=$(host=upnext-claude path=/api/v1/llm/test http -X POST -H "Authorization: Bearer $token" -H 'Origin: http://localhost:8787')
echo "$called"
grep -q '"ok":true' <<<"$called" || fail "a call through the installed CLI: $called"
docker restart -t 10 "$run-claude" >/dev/null
wait_healthy
log=$(docker logs "$run-claude" 2>&1)
grep -q "Claude Code is at /data/.local/bin/claude" <<<"$log" || fail "no Claude Code after a restart"
[ "$(docker logs "$run-claude" 2>&1 | grep -c 'Installing Claude Code')" = 1 ] || fail "installed again on restart"
docker rm -f "$run-claude" "$run-releases" >/dev/null

echo "== passed"
