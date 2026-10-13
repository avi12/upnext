#!/usr/bin/env bash
# Builds the Debian and Fedora packages from binaries that are
# already built: upnext_<version>-1_<arch>.deb and upnext-<version>-1.<arch>.rpm
# (amd64 or arm64, x86_64 or aarch64),
# each with a .sha256 beside it. The release workflow's build-linux-packages
# job runs it on the binaries in dist's Linux archives, and it runs on a
# development build the same way:
#
#   cargo build --release --locked --bins
#   contrib/linux/build-packages.sh --bin-dir target/release
#
# Options:
#   --bin-dir DIR   the folder with upnext and upnext-app (required)
#   --arch ARCH     amd64 or arm64; this machine's by default
#   --version V     the packages' version; Cargo.toml's by default
#   --out-dir DIR   where the packages go; target/linux-packages by default
#   --nfpm PATH     an nfpm to use instead of downloading one
#
# Without --nfpm it downloads nfpm's release archive for this machine from
# nfpm's GitHub release, checks its SHA-256 and unpacks it into target/; it
# installs nothing. The package's contents are in contrib/linux/nfpm.yaml.
set -euo pipefail

nfpm_version=2.47.0
# SHA-256 of nfpm_<version>_Linux_<arch>.tar.gz, from the release's checksums.txt.
# A new nfpm_version needs both of these too.
declare -A nfpm_sha256=(
  [x86_64]=0660ca602b2d2d2ae4781a06c692b3eeb9d437ffea05b831d76e41f4a3188783
  [arm64]=1c0f5f2999b9a974bfb04fdb0cc3306096de530ac5dbb25d739cc5f5219c919c
)

die() { echo "build-packages.sh: $*" >&2; exit 1; }

root=$(cd "$(dirname "$0")/../.." && pwd)
bin_dir='' arch='' version='' out_dir='' nfpm=''
while [ $# -gt 0 ]; do
  case $1 in
    --bin-dir) bin_dir=${2:?}; shift 2 ;;
    --arch) arch=${2:?}; shift 2 ;;
    --version) version=${2:?}; shift 2 ;;
    --out-dir) out_dir=${2:?}; shift 2 ;;
    --nfpm) nfpm=${2:?}; shift 2 ;;
    *) die "unknown option $1 (see the comment at the top)" ;;
  esac
done
[ -n "$bin_dir" ] || die "--bin-dir is required"
for exe in upnext upnext-app; do
  [ -x "$bin_dir/$exe" ] || die "$bin_dir/$exe isn't there (build with the default features: cargo build --release --locked --bins)"
done

if [ -z "$arch" ]; then
  case $(uname -m) in
    x86_64) arch=amd64 ;;
    aarch64) arch=arm64 ;;
    *) die "no --arch, and $(uname -m) isn't amd64 or arm64" ;;
  esac
fi
case $arch in amd64 | arm64) ;; *) die "--arch must be amd64 or arm64, not $arch" ;; esac

if [ -z "$version" ]; then
  version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)
fi
# A Cargo-style version: numbers, then an optional prerelease such as -rc.1.
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] || die "the version \"$version\" isn't X.Y.Z or X.Y.Z-pre"

out_dir=${out_dir:-$root/target/linux-packages}
mkdir -p "$out_dir"
out_dir=$(cd "$out_dir" && pwd)

if [ -z "$nfpm" ]; then
  case $(uname -m) in
    x86_64) host=x86_64 ;;
    aarch64) host=arm64 ;;
    *) die "no nfpm build to download for $(uname -m); pass --nfpm" ;;
  esac
  nfpm_dir=$root/target/nfpm-$nfpm_version-$host
  nfpm=$nfpm_dir/nfpm
  if [ ! -x "$nfpm" ]; then
    archive="nfpm_${nfpm_version}_Linux_${host}.tar.gz"
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    echo "Downloading nfpm $nfpm_version"
    curl --proto '=https' --tlsv1.2 -fsSL -o "$tmp/$archive" \
      "https://github.com/goreleaser/nfpm/releases/download/v$nfpm_version/$archive"
    echo "${nfpm_sha256[$host]}  $tmp/$archive" | sha256sum -c --quiet - || die "$archive doesn't match its SHA-256"
    mkdir -p "$nfpm_dir"
    tar -xzf "$tmp/$archive" -C "$nfpm_dir" nfpm
  fi
fi

# The programs and the packaged user service go in target/linux-stage, where
# nfpm.yaml finds them. The service is contrib/systemd/upnext.service with the
# packaged binary, and a header that says how to turn it on in place of the
# one about copying the file.
stage=$root/target/linux-stage
rm -rf "$stage"
mkdir -p "$stage"
install -m 755 "$bin_dir/upnext" "$bin_dir/upnext-app" "$stage/"
{
  cat <<'EOF'
# UpNext as a user service: it starts when you log in and keeps the daily
# sync running. The package installs it and enables it for no one; each user
# who wants it runs:
#
#   systemctl --user enable --now upnext
#
# Logs: journalctl --user -u upnext. Settings: ~/.config/upnext/.env and the
# app. To keep it running while you're logged out: loginctl enable-linger.
# Generated from contrib/systemd/upnext.service by contrib/linux/build-packages.sh.
EOF
  sed -e '1,/^$/{/^#/d}' -e 's|^ExecStart=%h/\.cargo/bin/upnext |ExecStart=/usr/bin/upnext |' \
    "$root/contrib/systemd/upnext.service"
} > "$stage/upnext.service"
grep -qx 'ExecStart=/usr/bin/upnext serve' "$stage/upnext.service" ||
  die "contrib/systemd/upnext.service's ExecStart changed; update the sed above"

# The names follow each format's convention (upnext_0.1.0-1_amd64.deb,
# upnext-0.1.0-1.x86_64.rpm), with the version as the tag writes it: inside
# the packages a prerelease is 0.2.0~rc.1, but GitHub would rename a release
# file with a ~ in it, and its .sha256 would no longer match.
case $arch in amd64) rpm_arch=x86_64 ;; arm64) rpm_arch=aarch64 ;; esac
cd "$root"
for target in "deb:upnext_${version}-1_${arch}.deb" "rpm:upnext-${version}-1.${rpm_arch}.rpm"; do
  packager=${target%%:*} file=${target#*:}
  VERSION=$version ARCH=$arch \
    "$nfpm" package --config contrib/linux/nfpm.yaml --packager "$packager" --target "$out_dir/$file"
  (cd "$out_dir" && sha256sum "$file" > "$file.sha256")
done
