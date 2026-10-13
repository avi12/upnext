#!/usr/bin/env python3
"""Gather the source of every library in the Steam copy's Linux depot.

The depot carries WebKitGTK, GTK, GLib and the other libraries the programs
need, copied from the build machine's Ubuntu packages (bundle-linux.py).
Most of them are under the LGPL or the GPL, which ask whoever ships them to
make their source available, so each release with a Steam build carries the
Ubuntu source packages of exactly the versions in its depot as one release
file. This makes that file:

    fetch-sources.py --depot DIR --out FILE
    fetch-sources.py --self-test     checks the Launchpad route offline

DIR is the Linux depot bundle-linux.py made; the source packages and
versions are read from its share/licenses/PACKAGES.txt. FILE is the tar to
write (upnext-steam-linux-sources.tar for a release), with FILE.sha256
beside it. Each source package (its .dsc and the tarballs the .dsc names)
goes in a folder of its own, with MANIFEST.txt listing every package,
version and file with its SHA-256, and the depot's PACKAGES.txt.

Run it on the machine that made the depot, an Ubuntu release, as a user who
may use apt. It sets up apt's source lists for that release in a folder of
its own, so the computer's apt settings are neither needed nor changed, and
apt checks each file against the archive's signed index as it does for
binary packages. A version the archive no longer lists (one that newer
updates replaced) comes from Launchpad, which keeps every version Ubuntu
published. That route is weaker: its .dsc is taken as Launchpad serves it
over HTTPS, with no signature checked, and the other files are checked
against the .dsc's SHA-256 and its Source and Version against the package
asked for. So it catches a damaged, cut-off or mismatched download, but not
a forged .dsc served together with matching forged files, by Launchpad
itself or by someone who can intercept its TLS. MANIFEST.txt says which
packages came that way.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.parse
import urllib.request
from pathlib import Path

# The name a release gives the file; RELEASE.md, the depot's PACKAGES.txt
# (bundle-linux.py) and the release workflow use it too.
RELEASE_NAME = "upnext-steam-linux-sources.tar"
TOP = "upnext-steam-linux-sources"

MIRROR = "http://archive.ubuntu.com/ubuntu"
LAUNCHPAD = "https://launchpad.net/ubuntu/+archive/primary/+sourcefiles"

# A PACKAGES.txt row, as bundle-linux.py writes it with dpkg-query.
ROW = re.compile(r"(\S+) (\S+) \(source: (\S+) (\S+)\)")

# Debian's patterns for a source package's name and its version (Debian
# Policy 5.6.1 and 5.6.12), checked before either becomes a folder name or
# an argument to apt: no slash, no leading dash, a colon only after the
# epoch. All of these are matched whole (`fullmatch`): `$` would let a
# trailing newline through.
SOURCE_NAME = re.compile(r"[a-z0-9][a-z0-9+.-]+")
VERSION = re.compile(r"(?:[0-9]+:)?[0-9][A-Za-z0-9.+~-]*")
# A file a .dsc names: a plain name in the folder.
FILE_NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9+.~_-]*")


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def read_packages(path: Path) -> dict[tuple[str, str], list[str]]:
    """Each (source package, version) in PACKAGES.txt, with the binary
    packages the depot took from it."""
    found: dict[tuple[str, str], list[str]] = {}
    for line in path.read_text().splitlines():
        m = ROW.fullmatch(line.strip())
        if m:
            if not (SOURCE_NAME.fullmatch(m.group(3)) and VERSION.fullmatch(m.group(4))):
                sys.exit(f"{path} has a source package or version that isn't Debian's form: {line.strip()!r}")
            found.setdefault((m.group(3), m.group(4)), []).append(f"{m.group(1)} {m.group(2)}")
    return found


def codename() -> str:
    text = Path("/etc/os-release").read_text()
    if not re.search(r'^ID="?ubuntu"?$', text, re.M):
        sys.exit("fetch-sources.py runs on the Ubuntu release that made the depot")
    m = re.search(r'^VERSION_CODENAME="?([a-z]+)"?$', text, re.M)
    if m is None:
        sys.exit("/etc/os-release names no VERSION_CODENAME")
    return m.group(1)


class Apt:
    """apt-get with source lists, indexes and downloads in a folder of its
    own, trusting the keys the computer's apt already trusts."""

    def __init__(self, root: Path, release: str) -> None:
        (root / "parts").mkdir(parents=True)
        (root / "lists" / "partial").mkdir(parents=True)
        (root / "cache" / "archives" / "partial").mkdir(parents=True)
        sources = root / "sources.list"
        sources.write_text(
            "".join(f"deb-src {MIRROR} {release}{pocket} main universe\n" for pocket in ("", "-updates", "-security"))
        )
        self.options = [
            "-o", f"Dir::Etc::SourceList={sources}",
            "-o", f"Dir::Etc::SourceParts={root / 'parts'}",
            "-o", f"Dir::State::Lists={root / 'lists'}",
            "-o", f"Dir::Cache={root / 'cache'}",
        ]
        # apt run as root hands downloads to its _apt user, which can't
        # write in this folder.
        if os.geteuid() == 0:
            self.options += ["-o", "APT::Sandbox::User=root"]

    def run(self, *args: str, cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
        return subprocess.run(["apt-get", "-q", *self.options, *args], cwd=cwd, capture_output=True, text=True)


def download(url: str, dst: Path) -> None:
    try:
        with urllib.request.urlopen(url, timeout=120) as response, open(dst, "wb") as f:
            shutil.copyfileobj(response, f)
    except OSError as e:
        sys.exit(f"couldn't download {url}: {e}")


def dsc_files(dsc: Path) -> dict[str, str]:
    """The files a .dsc names, with their SHA-256."""
    files: dict[str, str] = {}
    in_section = False
    for line in dsc.read_text(errors="replace").splitlines():
        if line.startswith("Checksums-Sha256:"):
            in_section = True
            continue
        if in_section:
            parts = line.split()
            if not line.startswith(" ") or len(parts) != 3:
                break
            files[parts[2]] = parts[0]
    return files


def dsc_field(dsc: Path, name: str) -> str | None:
    m = re.search(rf"^{name}:[ \t]*(\S+)[ \t]*$", dsc.read_text(errors="replace"), re.M)
    return m.group(1) if m else None


def check_dsc(dsc: Path, source: str, version: str) -> None:
    """Stop unless the .dsc is the package and version asked for."""
    said = (dsc_field(dsc, "Source"), dsc_field(dsc, "Version"))
    if said != (source, version):
        sys.exit(f"{dsc.name} is {said[0]} {said[1]}, not {source} {version}")


def dsc_name(source: str, version: str) -> str:
    return f"{source}_{version.split(':', 1)[-1]}.dsc"


def from_launchpad(source: str, version: str, dst: Path, base_url: str = LAUNCHPAD) -> None:
    """One source package from Launchpad: the .dsc as served, every other
    file checked against it (the module's docstring says what that does and
    doesn't prove)."""
    base = f"{base_url}/{urllib.parse.quote(source)}/{urllib.parse.quote(version)}"
    dsc = dst / dsc_name(source, version)
    download(f"{base}/{dsc.name}", dsc)
    check_dsc(dsc, source, version)
    files = dsc_files(dsc)
    if not files:
        sys.exit(f"{dsc.name} from Launchpad names no files")
    for name, want in files.items():
        if not FILE_NAME.fullmatch(name) or name == dsc.name:
            sys.exit(f"{dsc.name} names a file {name!r}, which isn't a plain file name")
        download(f"{base}/{urllib.parse.quote(name)}", dst / name)
        if sha256(dst / name) != want:
            sys.exit(f"{name} from Launchpad doesn't match the SHA-256 in {dsc.name}")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--depot", type=Path, help="the Linux depot bundle-linux.py made")
    ap.add_argument("--out", type=Path, help=f"the tar to write ({RELEASE_NAME} for a release)")
    ap.add_argument("--self-test", action="store_true", help="check the Launchpad route against a local server, offline")
    args = ap.parse_args()
    if args.self_test:
        return self_test()
    if args.depot is None or args.out is None:
        ap.error("give --depot and --out, or --self-test")

    listing = args.depot / "share" / "licenses" / "PACKAGES.txt"
    if not listing.is_file():
        sys.exit(f"{listing} isn't there: give the Linux depot bundle-linux.py made")
    packages = read_packages(listing)
    if not packages:
        sys.exit(f"{listing} lists no packages")
    out: Path = args.out.resolve()
    if out.exists():
        sys.exit(f"{out} exists already")
    release = codename()

    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)
        apt = Apt(work / "apt", release)
        update = apt.run("update")
        if update.returncode != 0:
            sys.exit("apt-get update failed:\n" + update.stdout + update.stderr)
        top = work / TOP
        top.mkdir()
        manifest = []
        for (source, version), binaries in sorted(packages.items()):
            dst = top / f"{source}_{version.split(':', 1)[-1]}"
            dst.mkdir()
            got = apt.run("source", "--download-only", "--only-source", f"{source}={version}", cwd=dst)
            origin = "Ubuntu's archive"
            if got.returncode != 0 or not (dst / dsc_name(source, version)).is_file():
                for f in dst.iterdir():
                    f.unlink()
                print(f"{source} {version}: not in the archive's index any more, so from Launchpad", file=sys.stderr)
                from_launchpad(source, version, dst)
                origin = "Launchpad, the .dsc unsigned"
            check_dsc(dst / dsc_name(source, version), source, version)
            files = sorted(f for f in dst.iterdir() if f.is_file())
            manifest.append(f"{source} {version} (from {origin}), for {', '.join(binaries)}\n")
            manifest.extend(f"  {sha256(f)}  {dst.name}/{f.name}\n" for f in files)
            size = sum(f.stat().st_size for f in files)
            print(f"{source} {version}: {len(files)} files, {size / 1e6:.1f} MB")

        shutil.copyfile(listing, top / "PACKAGES.txt")
        (top / "MANIFEST.txt").write_text(
            "The source packages of the libraries in UpNext's Steam copy for Linux, exactly the versions its depot\n"
            f"carries (PACKAGES.txt here, as in the depot's share/licenses/), from Ubuntu {release}'s archive.\n"
            "Each folder holds one source package: its .dsc and the tarballs it names. `dpkg-source -x` on the .dsc\n"
            "unpacks it with Ubuntu's changes applied. Every file is listed below with its SHA-256.\n\n"
            + "".join(manifest)
        )
        out.parent.mkdir(parents=True, exist_ok=True)
        # Already-compressed tarballs, so the release file is a plain tar.
        with tarfile.open(out, "w", format=tarfile.PAX_FORMAT) as tar:
            tar.add(top, arcname=TOP, filter=_plain)
    digest = sha256(out)
    Path(f"{out}.sha256").write_text(f"{digest}  {out.name}\n")
    print(f"{out}: {len(packages)} source packages, {out.stat().st_size / 1e6:.0f} MB, SHA-256 {digest}")
    return 0


def self_test() -> int:
    """The Launchpad route and the checks on names, against a made-up
    package served from 127.0.0.1, with no network."""
    import functools
    import http.server
    import threading

    for source, version, ok in [
        ("glib2.0", "2.72.4-0ubuntu2.10", True),
        ("gtk+3.0", "3.24.33-1ubuntu2.2", True),
        ("libcap2", "1:2.44-1ubuntu0.22.04.3", True),
        ("../../etc", "1.0-1", False),
        ("-oDebug::pkgAcquire=1", "1.0-1", False),
        ("glib2.0", "1.0/../../x", False),
        ("glib2.0", "-1.0", False),
        ("glib2.0", "1:2:3", False),
        ("glib2.0\n", "1.0-1", False),
        ("glib2.0", "1.0-1\n", False),
    ]:
        if bool(SOURCE_NAME.fullmatch(source) and VERSION.fullmatch(version)) != ok:
            sys.exit(f"self-test: {source!r} {version!r} should be {'accepted' if ok else 'refused'}")
    for name, ok in [("glib2.0_2.72.4.orig.tar.xz", True), ("glib2.0_2.72.4.orig.tar.xz\n", False), ("../x", False)]:
        if bool(FILE_NAME.fullmatch(name)) != ok:
            sys.exit(f"self-test: the file name {name!r} should be {'accepted' if ok else 'refused'}")

    source, version = "upnext-selftest", "1:1.0-1"
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        served = root / "srv" / source / version
        served.mkdir(parents=True)
        orig = served / "upnext-selftest_1.0.orig.tar.gz"
        orig.write_bytes(b"made-up upstream tarball")
        debian = served / "upnext-selftest_1.0-1.debian.tar.xz"
        debian.write_bytes(b"made-up packaging")

        def write_dsc(src: str, ver: str, extra: str = "") -> None:
            lines = "".join(f" {sha256(f)} {f.stat().st_size} {f.name}\n" for f in (orig, debian))
            (served / dsc_name(source, version)).write_text(
                f"Format: 3.0 (quilt)\nSource: {src}\nVersion: {ver}\nChecksums-Sha256:\n{lines}{extra}"
            )

        class Quiet(http.server.SimpleHTTPRequestHandler):
            def log_message(self, *args: object) -> None:
                pass

        handler = functools.partial(Quiet, directory=str(root / "srv"))
        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        os.environ["no_proxy"] = os.environ["NO_PROXY"] = "127.0.0.1"
        base = f"http://127.0.0.1:{server.server_address[1]}"

        def attempt(name: str) -> bool:
            dst = root / name
            dst.mkdir()
            try:
                from_launchpad(source, version, dst, base)
            except SystemExit as e:
                print(f"  {name}: refused ({e.code})")
                return False
            print(f"  {name}: accepted, {len(list(dst.iterdir()))} files")
            return True

        try:
            write_dsc(source, version)
            results = [attempt("good") is True]
            write_dsc(source, "1:1.0-2")
            results.append(attempt("other-version") is False)
            write_dsc(source, version, " " + "0" * 64 + " 1 ../evil\n")
            results.append(attempt("path-in-dsc") is False)
            write_dsc(source, version)
            orig.write_bytes(b"changed after the .dsc was written")
            results.append(attempt("changed-file") is False)
        finally:
            server.shutdown()
    if not all(results):
        sys.exit("self-test failed")
    print("self-test passed")
    return 0


def _plain(info: tarfile.TarInfo) -> tarfile.TarInfo:
    """Files owned by nobody in particular, as in dist's archives."""
    info.uid = info.gid = 0
    info.uname = info.gname = ""
    return info


if __name__ == "__main__":
    sys.exit(main())
