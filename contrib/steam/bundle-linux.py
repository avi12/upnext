#!/usr/bin/env python3
"""Lay out the Linux depot of UpNext's Steam copy.

The release's Linux programs link WebKitGTK 4.1 and GTK 3, which a Steam
Deck and the Steam Linux Runtime don't have, so the depot carries them: the
two programs, every library they need except those in linux-exclude.txt,
WebKitGTK's helper programs, the few modules GTK and GLib load at run time,
and a launcher. Run it on the machine (or in the image) the programs were
built on, with WebKitGTK, GTK, librsvg's image loader and patchelf
installed, so the libraries copied are the ones the programs were built
against.

    bundle-linux.py --bin-dir DIR --out DIR

DIR is a folder with `upnext` and `upnext-app` (the release archive's
contents); the output folder must not exist yet. Nothing outside it is
changed. The depot's layout and why each piece is there are in
contrib/steam/README.md.
"""

from __future__ import annotations

import argparse
import fnmatch
import os
import re
import shutil
import stat
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent

# Where Debian and Ubuntu put x86-64 libraries; WebKitGTK's helper programs
# are under it too, and the path is compiled into libwebkit2gtk.
LIBDIR = Path("/usr/lib/x86_64-linux-gnu")
WEBKIT = "webkit2gtk-4.1"

# WebKitGTK finds its helper programs and its injected bundle by a path
# compiled into it, and reads no variable to move them outside a developer
# build. The depot's copy has that path rewritten in place to one of the same
# length that is relative to the working directory, where the launcher's
# `cd` puts the depot's own `lib/x86_64-linux-gnu/webkit2gtk-4.1`. Tauri's
# AppImages do the same.
WEBKIT_PATH = f"/usr/lib/x86_64-linux-gnu/{WEBKIT}".encode()
WEBKIT_RELATIVE = f"././/lib/x86_64-linux-gnu/{WEBKIT}".encode()

# The helper programs WebKitGTK starts, of which the GPU process is only in
# newer versions.
HELPERS = ["WebKitWebProcess", "WebKitNetworkProcess", "WebKitGPUProcess"]

# GIO modules: TLS for the GOG sign-in window's pages, and dconf, so GTK's
# settings (the file chooser's) are kept rather than held in memory.
GIO_MODULES = ["libgiognutls.so", "libdconfsettings.so"]

# gdk-pixbuf's SVG loader, for GTK's symbolic icons in its dialogs. PNG and
# JPEG are built into Debian's and Ubuntu's gdk-pixbuf.
PIXBUF_LOADERS = ["libpixbufloader-svg.so"]

# Where each release with a Steam build carries the bundled libraries'
# source, as fetch-sources.py names it.
REPO = "zytocc/upnext"
SOURCES_NAME = "upnext-steam-linux-sources.tar"

# The marker the programs look for beside themselves (src/steam_copy.rs).
MARKER = "steam-copy.txt"
MARKER_TEXT = """\
This copy of UpNext was installed by Steam. UpNext sees this file beside its
programs and leaves out what Steam already does for it: "Add to Steam" and a
launcher entry. Steam puts it back if it goes missing.
"""


def run(*args: str, check: bool = True) -> str:
    return subprocess.run(args, check=check, capture_output=True, text=True).stdout


def is_elf(path: Path) -> bool:
    with open(path, "rb") as f:
        return f.read(4) == b"\x7fELF"


def needed(path: Path) -> list[str]:
    """The sonames `path` asks the dynamic linker for."""
    return re.findall(r"\(NEEDED\)\s+Shared library: \[([^\]]+)\]", run("readelf", "-d", str(path)))


def system_libraries() -> dict[str, Path]:
    """Each x86-64 soname in the linker's cache, with its path."""
    found: dict[str, Path] = {}
    for line in run("ldconfig", "-p").splitlines():
        m = re.match(r"\s*(\S+) \((.*)\) => (\S+)", line)
        if m and "x86-64" in m.group(2):
            found.setdefault(m.group(1), Path(m.group(3)))
    return found


def excluded_patterns(path: Path) -> list[str]:
    lines = (line.strip() for line in path.read_text().splitlines())
    return [line for line in lines if line and not line.startswith("#")]


def excluded(soname: str, patterns: list[str]) -> bool:
    return any(fnmatch.fnmatchcase(soname, p) for p in patterns)


def copy_file(src: Path, dst: Path) -> None:
    """Copy the file `src` resolves to, as a plain file, writable so patchelf
    can change it."""
    dst.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(src.resolve(), dst)
    mode = 0o755 if os.access(src.resolve(), os.X_OK) else 0o644
    dst.chmod(mode)


def glibc_needed(path: Path) -> tuple[int, int]:
    """The newest GLIBC_x.y version `path` asks for."""
    newest = (2, 0)
    for major, minor in re.findall(r"GLIBC_(\d+)\.(\d+)", run("readelf", "-V", "-W", str(path))):
        newest = max(newest, (int(major), int(minor)))
    return newest


def package_of(path: Path) -> str | None:
    """The Debian package that installed `path`, trying the merged-/usr
    spellings too, since dpkg knows each file by the path its package
    used."""
    real = path.resolve()
    tries = {str(path), str(real)}
    for p in list(tries):
        if p.startswith("/usr/lib/"):
            tries.add(p[len("/usr"):])
        elif p.startswith("/lib/"):
            tries.add("/usr" + p)
    for p in sorted(tries):
        out = subprocess.run(["dpkg-query", "-S", p], capture_output=True, text=True)
        if out.returncode == 0:
            return out.stdout.split(":")[0].split(",")[0].strip()
    return None


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--bin-dir", required=True, type=Path, help="folder with upnext and upnext-app")
    ap.add_argument("--out", required=True, type=Path, help="the depot folder to make (must not exist)")
    ap.add_argument("--exclude", type=Path, default=HERE / "linux-exclude.txt", help="sonames to leave out")
    args = ap.parse_args()

    for tool in ("readelf", "ldconfig", "ldd", "patchelf", "glib-compile-schemas", "dpkg-query"):
        if shutil.which(tool) is None and not Path("/sbin", tool).exists():
            sys.exit(f"bundle-linux.py needs {tool}")
    out: Path = args.out.resolve()
    if out.exists():
        sys.exit(f"{out} exists already; give a new folder")
    patterns = excluded_patterns(args.exclude)
    cache = system_libraries()

    # The programs, beside the marker.
    bin_dir = out / "bin"
    for name in ("upnext", "upnext-app"):
        src = args.bin_dir / name
        if not src.is_file():
            sys.exit(f"{src} isn't there: give the folder of the release's Linux archive")
        copy_file(src, bin_dir / name)
        (bin_dir / name).chmod(0o755)
    (bin_dir / MARKER).write_text(MARKER_TEXT)

    # The pieces loaded by path rather than by soname, each with the file it
    # came from, for its package's licence.
    sources: dict[Path, Path] = {}

    def take(src: Path, dst: Path) -> None:
        copy_file(src, dst)
        sources[dst] = src
        roots.append(dst)

    webkit_dir = out / "lib" / "x86_64-linux-gnu" / WEBKIT
    roots: list[Path] = [bin_dir / "upnext", bin_dir / "upnext-app"]
    for helper in HELPERS:
        src = LIBDIR / WEBKIT / helper
        if src.is_file():
            take(src, webkit_dir / helper)
    for helper in HELPERS[:2]:
        if not (webkit_dir / helper).is_file():
            sys.exit(f"WebKitGTK's {helper} isn't in {LIBDIR / WEBKIT}: install libwebkit2gtk-4.1-0")
    injected = LIBDIR / WEBKIT / "injected-bundle" / "libwebkit2gtkinjectedbundle.so"
    take(injected, webkit_dir / "injected-bundle" / injected.name)

    gio_dir = out / "lib" / "gio" / "modules"
    for name in GIO_MODULES:
        src = LIBDIR / "gio" / "modules" / name
        if not src.is_file():
            sys.exit(f"GIO's {name} isn't in {src.parent}: install glib-networking and dconf-gsettings-backend")
        take(src, gio_dir / name)

    pixbuf_dir = out / "lib" / "gdk-pixbuf" / "loaders"
    pixbuf_src = LIBDIR / "gdk-pixbuf-2.0" / "2.10.0" / "loaders"
    for name in PIXBUF_LOADERS:
        if not (pixbuf_src / name).is_file():
            sys.exit(f"gdk-pixbuf's {name} isn't in {pixbuf_src}: install librsvg2-common")
        take(pixbuf_src / name, pixbuf_dir / name)

    # Every library those need, by soname, stopping at the excluded ones.
    lib_dir = out / "lib"
    left_out: set[str] = set()
    queue = list(roots)
    seen: set[str] = set()
    while queue:
        elf = queue.pop()
        for soname in needed(elf):
            if soname in seen:
                continue
            seen.add(soname)
            if excluded(soname, patterns):
                left_out.add(soname)
                continue
            src = cache.get(soname)
            if src is None:
                sys.exit(f"{elf.name} needs {soname}, which isn't installed here")
            dst = lib_dir / soname
            copy_file(src, dst)
            sources[dst] = src
            queue.append(dst)
    # Each program and library finds the depot's libraries through its own
    # RPATH, which comes before LD_LIBRARY_PATH (the Steam Runtime's, in its
    # older form), so nothing needs LD_LIBRARY_PATH and programs UpNext
    # starts (xdg-open, Claude Code) don't inherit the depot's libraries.
    elves = sorted(p for p in out.rglob("*") if p.is_file() and not p.is_symlink() and is_elf(p))
    for elf in elves:
        rel = os.path.relpath(lib_dir, elf.parent)
        rpath = "$ORIGIN" if rel == "." else f"$ORIGIN/{rel}"
        subprocess.run(["patchelf", "--force-rpath", "--set-rpath", rpath, str(elf)], check=True)

    webkit_lib = lib_dir / "libwebkit2gtk-4.1.so.0"
    data = webkit_lib.read_bytes()
    count = data.count(WEBKIT_PATH)
    if count < 2:
        sys.exit(f"{webkit_lib.name} names {WEBKIT_PATH.decode()} {count} times, not at least twice; the helpers' path changed")
    assert len(WEBKIT_PATH) == len(WEBKIT_RELATIVE)
    webkit_lib.write_bytes(data.replace(WEBKIT_PATH, WEBKIT_RELATIVE))

    # gdk-pixbuf's loader list, with the depot's folder as @BUNDLE@ for the
    # launcher to fill in, since the list holds full paths.
    query = LIBDIR / "gdk-pixbuf-2.0" / "gdk-pixbuf-query-loaders"
    listing = run(str(query), *(str(pixbuf_dir / n) for n in PIXBUF_LOADERS))
    (out / "lib" / "gdk-pixbuf" / "loaders.cache.in").write_text(listing.replace(str(out), "@BUNDLE@"))

    # GTK's own settings schemas: the file chooser stops the program when
    # its schema is missing.
    schemas = out / "share" / "glib-2.0" / "schemas"
    schemas.mkdir(parents=True)
    for xml in sorted(Path("/usr/share/glib-2.0/schemas").glob("org.gtk.Settings.*.gschema.xml")):
        shutil.copyfile(xml, schemas / xml.name)
    subprocess.run(["glib-compile-schemas", str(schemas)], check=True)

    # No GStreamer plugins: the UI plays no audio or video, and pointing
    # GStreamer at this empty folder keeps the computer's plugins, built for
    # another GStreamer, out of WebKitGTK.
    gst = out / "lib" / "gstreamer-1.0"
    gst.mkdir(parents=True)
    (gst / "README.txt").write_text(
        "No GStreamer plugins, on purpose: UpNext's pages play no audio or video, and the launcher points\n"
        "GStreamer here so the computer's own plugins, built for another GStreamer, aren't loaded.\n"
    )

    # The launcher, with the glibc the depot needs.
    floor = max(glibc_needed(e) for e in elves)
    launcher = (HERE / "upnext.sh").read_text().replace("@GLIBC@", f"{floor[0]}.{floor[1]}")
    (out / "upnext.sh").write_text(launcher)
    (out / "upnext.sh").chmod(0o755)

    # The release archive's own files (licences, third-party notices, README,
    # CHANGELOG).
    for extra in sorted(args.bin_dir.iterdir()):
        if extra.is_file() and extra.suffix.lower() in (".md", ".txt", "") and extra.name.upper().startswith(
            ("LICENSE", "THIRD_PARTY_NOTICES", "README", "CHANGELOG", "COPYRIGHT")
        ):
            shutil.copyfile(extra, out / extra.name)
    # A development build's folder (target/release) has none of these, so
    # they come from the checkout this script is in.
    for name in ("LICENSE-MIT", "LICENSE-APACHE", "THIRD_PARTY_NOTICES.txt"):
        if not (out / name).is_file():
            fallback = HERE.parent.parent / name
            if not fallback.is_file():
                sys.exit(f"neither {args.bin_dir} nor the checkout has {name}; the depot would lack it")
            shutil.copyfile(fallback, out / name)

    # The bundled libraries' licences, and their packages and versions, whose
    # sources each release with a Steam build carries (fetch-sources.py).
    packages: set[str] = set()
    for dst, src in sorted(sources.items()):
        if dst.is_relative_to(bin_dir):
            continue
        pkg = package_of(src)
        if pkg is None:
            sys.exit(f"no package installed {src}; the depot's licences would be incomplete")
        packages.add(pkg)
    licenses = out / "share" / "licenses"
    rows = []
    common: set[str] = set()
    unlicensed = []
    for pkg in sorted(packages):
        name = pkg.split(":")[0]
        copyright_file = Path("/usr/share/doc") / name / "copyright"
        # A library whose licence can't go beside it can't be shipped: the
        # LGPL asks for its text with every copy.
        if not copyright_file.is_file():
            unlicensed.append(f"{pkg} ({copyright_file})")
            continue
        (licenses / name).mkdir(parents=True, exist_ok=True)
        shutil.copyfile(copyright_file, licenses / name / "copyright")
        found = re.findall(r"/usr/share/common-licenses/([A-Za-z0-9.+-]+)", copyright_file.read_text(errors="replace"))
        common.update(f.rstrip(".") for f in found)
        rows.append(run("dpkg-query", "-W", "-f", "${Package} ${Version} (source: ${source:Package} ${source:Version})\n", pkg))
    if unlicensed:
        sys.exit("these packages have no copyright file, so the depot can't carry their licences:\n  " + "\n  ".join(unlicensed))
    licenses.mkdir(parents=True, exist_ok=True)
    # Debian's copyright files name the LGPL, the GPL and other common
    # licences by their path on the build machine instead of quoting them,
    # so the texts they point at go in the depot as well.
    # Some files spell a name that isn't there ("GPL-2.0" for "GPL-2"), and a
    # few name one that doesn't exist at all, which is reported and skipped.
    for licence in sorted(common):
        texts = [Path("/usr/share/common-licenses") / n for n in (licence, re.sub(r"\.0$", "", licence))]
        text = next((t for t in texts if t.is_file()), None)
        if text is None:
            print(f"warning: a copyright file names /usr/share/common-licenses/{licence}, which isn't on this computer", file=sys.stderr)
            continue
        (licenses / "common-licenses").mkdir(exist_ok=True)
        shutil.copyfile(text.resolve(), licenses / "common-licenses" / text.name)
    os_release = Path("/etc/os-release").read_text()
    distro = re.search(r'^PRETTY_NAME="?([^"\n]*)', os_release, re.M)
    # The release this depot is made from, for the address of its sources:
    # `upnext --version` says "upnext 0.1.0 (abc1234)", and the tag is v0.1.0.
    said = subprocess.run([str(bin_dir / "upnext"), "--version"], capture_output=True, text=True, timeout=30).stdout
    version = re.search(r"^upnext (\d+\.\d+\.\d+\S*)", said)
    releases = f"https://github.com/{REPO}/releases"
    if version:
        where = (
            f"Their source, exactly these versions, is attached to UpNext's release {version.group(1)} on GitHub"
            f" as {SOURCES_NAME}:\n{releases}/download/v{version.group(1)}/{SOURCES_NAME}\n"
        )
    else:
        where = (
            f"Their source, exactly these versions, is attached as {SOURCES_NAME} to UpNext's release"
            f" of the same version on GitHub ({releases}).\n"
        )
    if re.search(r"^ID=\"?ubuntu\"?$", os_release, re.M):
        where += "It comes from Ubuntu's archive, where Launchpad keeps every published version too (https://launchpad.net/ubuntu/+source/<source>/<version>).\n"
    else:
        where += "It comes from the distribution's archive, under these versions.\n"
    (licenses / "PACKAGES.txt").write_text(
        "The libraries in lib/ come from these packages of "
        + (distro.group(1) if distro else "the build machine's distribution")
        + ", unchanged except for their RPATH and, in libwebkit2gtk-4.1, the helper programs' folder.\n"
        + where
        + "Each package's licence is in its folder here, and the common licences those name are in common-licenses/.\n"
        + "UpNext's own licences, and the notices of the crates and npm packages built into its programs"
        + " (THIRD_PARTY_NOTICES.txt), are in the depot's top folder.\n\n"
        + "".join(rows)
    )

    # Every program and library must now find each library it asks for
    # either in the depot or among the excluded ones the computer gives. The
    # excluded ones' own dependencies are the computer's business, so only
    # each file's direct needs are checked, as the linker resolves them.
    env = {k: v for k, v in os.environ.items() if k != "LD_LIBRARY_PATH"}
    problems = []
    for elf in elves:
        listing = subprocess.run(["ldd", str(elf)], capture_output=True, text=True, env=env).stdout
        resolved = {}
        for line in listing.splitlines():
            m = re.match(r"\s*(\S+) => (.*?)( \(0x[0-9a-f]+\))?$", line)
            if m:
                resolved[m.group(1)] = m.group(2)
        for soname in needed(elf):
            if excluded(soname, patterns):
                continue
            where = resolved.get(soname, "not found")
            if where == "not found":
                problems.append(f"{elf.relative_to(out)}: {soname} not found")
            elif not Path(where).resolve().is_relative_to(out):
                problems.append(f"{elf.relative_to(out)}: {soname} comes from {where}, outside the depot")
    if problems:
        sys.exit("The depot isn't self-contained:\n" + "\n".join(problems))

    size = sum(p.stat().st_size for p in out.rglob("*") if p.is_file())
    print(f"Linux depot in {out}: {len(elves)} programs and libraries, {size / 1e6:.0f} MB, glibc {floor[0]}.{floor[1]} or newer")
    print("Left out, from the computer: " + " ".join(sorted(left_out)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
