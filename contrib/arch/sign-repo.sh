#!/usr/bin/env bash
# Signs the Arch package and builds the signed pacman repository for a
# release, as the release workflow's sign-pacman-repo job
# does. By hand, after contrib/arch/build-package.sh:
#
#   PACMAN_SIGNING_KEY="$(cat signing-subkey.asc)" contrib/arch/sign-repo.sh \
#     --package target/arch-package/upnext-bin-0.1.0-1-x86_64.pkg.tar.zst \
#     --fingerprint <the key's fingerprint>
#
# It writes to --out-dir: the package's detached signature (<package>.sig),
# the repository database upnext.db, listing that one package with its
# signature inside, the file list upnext.files that `pacman -F` reads, a
# signature for each (upnext.db.sig, upnext.files.sig), and
# upnext-signing-key.asc, a copy of the public key. The release attaches
# them beside the package, and pacman finds them through the
# releases/latest/download link in the [upnext] stanza added to pacman.conf. repo-add
# makes upnext.db and upnext.files symlinks to .tar.gz files, and a release
# file can't be a symlink, so they're copied under the names pacman asks for.
#
# The key comes from the environment, never from an argument:
#
#   PACMAN_SIGNING_KEY             the signing subkey, armoured, as
#                                  `gpg --export-secret-subkeys --armor` writes it
#   PACMAN_SIGNING_KEY_PASSPHRASE  its passphrase, if it has one
#
# It is imported into a throwaway GnuPG home, removed when the script ends.
# The script refuses to sign unless the imported key's fingerprint is
# --fingerprint, the public key file is that same key with the same signing
# subkey, and the primary key's secret half is absent: the primary key stays
# offline and only the subkey is ever handed over (docs/dev/RELEASE.md, "The
# pacman repository"). It also checks every signature it made before it
# finishes, since repo-add only warns when signing the database fails.
#
# Options:
#   --package FILE      the package build-package.sh wrote (required)
#   --fingerprint FPR   the primary key's fingerprint (required); the
#                       workflow passes the PACMAN_KEY_FINGERPRINT variable
#   --public-key FILE   the public key to check the key against and publish;
#                       contrib/arch/upnext-signing-key.asc by default
#   --out-dir DIR       where the files go; target/pacman-repo by default
#   --image IMAGE       the container to sign in; by default archlinux:latest,
#                       pinned to the digest below (docs/dev/RELEASE.md, "The
#                       Arch images", says how to move it on)
#   --host              sign with this machine's gpg and repo-add instead
#
# In a container (the default) it needs docker but not Arch's mirrors: gpg
# and repo-add come with pacman in the image, so nothing is installed and no
# code but Arch's own tools sees the key.
set -euo pipefail

die() { echo "sign-repo.sh: $*" >&2; exit 1; }

root=$(cd "$(dirname "$0")/../.." && pwd)
package='' fingerprint='' public_key=$root/contrib/arch/upnext-signing-key.asc out_dir='' image=archlinux:latest@sha256:4e77cf2ea5f410e6f8be5abf93ccf17ce2436e87a138c167208356711a405dbd host=false
while [ $# -gt 0 ]; do
  case $1 in
    --package) package=${2:?}; shift 2 ;;
    --fingerprint) fingerprint=${2:?}; shift 2 ;;
    --public-key) public_key=${2:?}; shift 2 ;;
    --out-dir) out_dir=${2:?}; shift 2 ;;
    --image) image=${2:?}; shift 2 ;;
    --host) host=true; shift ;;
    *) die "unknown option $1 (see the comment at the top)" ;;
  esac
done
[ -n "$package" ] || die "--package is required"
[ -f "$package" ] || die "$package isn't there"
[[ $(basename "$package") == upnext-bin-*.pkg.tar.zst ]] || die "$package isn't an upnext-bin package"
[ -n "$fingerprint" ] || die "--fingerprint is required"
# gpg prints fingerprints in groups of four; accept them that way too.
fpr=${fingerprint//[[:space:]]/}
fpr=${fpr^^}
[[ $fpr =~ ^([0-9A-F]{40}|[0-9A-F]{64})$ ]] || die "\"$fingerprint\" isn't a key fingerprint"
[ -f "$public_key" ] || die "$public_key isn't there"
[ -n "${PACMAN_SIGNING_KEY:-}" ] || die "PACMAN_SIGNING_KEY is empty, so there's no key to sign with"

out_dir=${out_dir:-$root/target/pacman-repo}
mkdir -p "$out_dir"
out_dir=$(cd "$out_dir" && pwd)
package=$(cd "$(dirname "$package")" && pwd)/$(basename "$package")
public_key=$(cd "$(dirname "$public_key")" && pwd)/$(basename "$public_key")

if ! $host; then
  # The same script again, inside the container, as the calling user so the
  # files it writes are theirs, with the files at the same paths so its
  # messages name them as the caller knows them. docker passes the key's
  # variables through by name, so their values never appear on a command line.
  docker run --rm -i --user "$(id -u):$(id -g)" -e HOME=/tmp \
    -e PACMAN_SIGNING_KEY -e PACMAN_SIGNING_KEY_PASSPHRASE \
    -v "$root/contrib/arch/sign-repo.sh:/sign-repo.sh:ro" \
    -v "$package:$package:ro" -v "$public_key:$public_key:ro" -v "$out_dir:$out_dir" \
    "$image" bash /sign-repo.sh --host --package "$package" \
      --fingerprint "$fpr" --public-key "$public_key" --out-dir "$out_dir"
  exit
fi

for tool in gpg gpgconf repo-add tar; do
  command -v "$tool" > /dev/null || die "$tool isn't installed"
done

GNUPGHOME=$(mktemp -d)
export GNUPGHOME
work=$(mktemp -d)
cleanup() {
  gpgconf --kill all 2> /dev/null || true
  rm -rf "$GNUPGHOME" "$work"
}
trap cleanup EXIT

# The fingerprints of the primary key (pub or sec) and of its signing
# subkeys (sub or ssb) in gpg's colon listing on standard input, as
# "primary <fpr>" and "signing <fpr> <secret>" lines, where <secret> is the
# listing's token field: "#" when the secret half is absent.
keys() {
  awk -F: '
    $1 == "pub" || $1 == "sec" { kind = "primary"; secret = $15; next }
    $1 == "sub" || $1 == "ssb" {
      # Only a subkey that can sign and is not expired, revoked or invalid.
      kind = ($12 ~ /s/ && $2 !~ /[eri]/) ? "signing" : ""; secret = $15; next
    }
    $1 == "fpr" && kind == "primary" { print "primary", $10, secret; kind = "" }
    $1 == "fpr" && kind == "signing" { print "signing", $10, secret; kind = "" }
  '
}

step() { echo "== $*"; }

step "the public key"
listing=$(gpg --batch --with-colons --show-keys "$public_key" 2> /dev/null) ||
  die "$public_key holds no key; until the signing key is made it is a placeholder (docs/dev/RELEASE.md, \"Making the signing key\")"
mapfile -t public < <(keys <<< "$listing")
primaries=$(printf '%s\n' "${public[@]}" | grep -c '^primary ' || true)
[ "$primaries" = 1 ] || die "$public_key should hold one key, not $primaries"
grep -qx "primary $fpr .*" < <(printf '%s\n' "${public[@]}") ||
  die "$public_key is not the key $fpr"

step "importing the signing key"
printf '%s\n' "$PACMAN_SIGNING_KEY" | gpg --batch --quiet --import 2> "$work/import.log" ||
  { cat "$work/import.log" >&2; die "PACMAN_SIGNING_KEY isn't a key gpg can import"; }
mapfile -t secret < <(gpg --batch --with-colons --list-secret-keys | keys)
primaries=$(printf '%s\n' "${secret[@]}" | grep -c '^primary ' || true)
[ "$primaries" = 1 ] || die "PACMAN_SIGNING_KEY should hold one key, not $primaries"
grep -qx "primary $fpr .*" < <(printf '%s\n' "${secret[@]}") ||
  die "PACMAN_SIGNING_KEY is not the key $fpr; check the PACMAN_KEY_FINGERPRINT variable"
grep -qx "primary $fpr #" < <(printf '%s\n' "${secret[@]}") ||
  die "PACMAN_SIGNING_KEY holds the primary key's secret half, which must stay offline; export only the subkey (gpg --export-secret-subkeys)"
subkey=''
for line in "${secret[@]}"; do
  read -r kind sub_fpr token <<< "$line"
  # A signing subkey whose secret half was imported, and which the public
  # key file carries too, so users who imported that file can check it.
  if [ "$kind" = signing ] && [ "$token" != '#' ] &&
    grep -q "^signing $sub_fpr " < <(printf '%s\n' "${public[@]}"); then
    subkey=$sub_fpr
  fi
done
[ -n "$subkey" ] ||
  die "PACMAN_SIGNING_KEY has no usable signing subkey that $public_key also has (expired, or the public key wasn't exported again after a new subkey)"
echo "signing with the subkey $subkey of $fpr"

# gpg, and repo-add's own calls to it, ask for the passphrase without a
# terminal and read it from a file; the agent then keeps it for the rest of
# the run. A subkey without a passphrase needs neither.
echo 'pinentry-mode loopback' > "$GNUPGHOME/gpg.conf"
if [ -n "${PACMAN_SIGNING_KEY_PASSPHRASE:-}" ]; then
  (umask 077 && printf '%s\n' "$PACMAN_SIGNING_KEY_PASSPHRASE" > "$GNUPGHOME/passphrase")
  echo "passphrase-file $GNUPGHOME/passphrase" >> "$GNUPGHOME/gpg.conf"
fi

# Checks that $1 is a good signature of $2 by the key, through the subkey.
check_signature() {
  local status
  status=$(gpg --batch --status-fd 1 --verify "$1" "$2" 2> /dev/null) ||
    die "$(basename "$1") isn't a good signature of $(basename "$2")"
  grep -Eq "^\[GNUPG:\] VALIDSIG $subkey .* $fpr\$" <<< "$status" ||
    die "$(basename "$1") wasn't made by the subkey $subkey of $fpr"
}

name=$(basename "$package")
cp "$package" "$work/$name"
cd "$work"

step "signing $name"
# The subkey's fingerprint with "!" makes gpg use exactly that subkey.
gpg --batch --yes --local-user "$subkey!" --detach-sign --no-armor --output "$name.sig" "$name" ||
  die "gpg couldn't sign with the subkey; if it has a passphrase, PACMAN_SIGNING_KEY_PASSPHRASE must be it"
check_signature "$name.sig" "$name"

step "repo-add"
# repo-add 6.1 and later put a package's signature into the database only
# when asked; older ones always did. With it there, pacman checks the
# package without fetching its .sig.
include_sigs=()
if grep -q -- --include-sigs <<< "$(repo-add --help 2>&1)"; then
  include_sigs=(--include-sigs)
fi
repo-add --sign --key "$subkey!" "${include_sigs[@]}" upnext.db.tar.gz "$name"
for db in upnext.db upnext.files; do
  [ -f "$db.tar.gz.sig" ] || die "repo-add didn't sign $db.tar.gz"
  check_signature "$db.tar.gz.sig" "$db.tar.gz"
done
entries=$(tar -tzf upnext.db.tar.gz | grep -c '/desc$' || true)
[ "$entries" = 1 ] || die "upnext.db should list one package, not $entries"
grep -qx '%PGPSIG%' <<< "$(tar -xzOf upnext.db.tar.gz --wildcards '*/desc')" ||
  die "upnext.db doesn't carry the package's signature"

step "the repository files"
for f in "$name.sig" upnext.db upnext.db.sig upnext.files upnext.files.sig; do
  cp -L "$f" "$out_dir/$f"
done
cp "$public_key" "$out_dir/upnext-signing-key.asc"
ls -l "$out_dir"
