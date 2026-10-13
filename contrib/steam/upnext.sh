#!/bin/sh
# The launcher in the Linux depot of UpNext's Steam copy (contrib/steam/,
# README.md there). Steam runs it with the launch option's arguments,
# `--big-screen` for "UpNext". bundle-linux.py puts the glibc the depot
# needs in place of @GLIBC@.
#
# The programs find the depot's libraries by their own RPATH, so nothing
# here sets LD_LIBRARY_PATH, and programs UpNext starts (xdg-open, Claude
# Code) see the computer's libraries, not the depot's.

here=$(dirname "$(readlink -f "$0")")

# The depot leaves glibc to the computer (or to the Steam Linux Runtime,
# which uses the computer's when it's newer). Say so plainly rather than
# fail with a symbol error.
need=@GLIBC@
have=$(getconf GNU_LIBC_VERSION 2>/dev/null | sed -n 's/^glibc //p')
if [ -n "$have" ] && [ "$(printf '%s\n%s\n' "$need" "$have" | sort -V | head -n 1)" != "$need" ]; then
  echo "UpNext's Steam copy needs glibc $need or newer, and this computer has $have." >&2
  echo "UpNext's Flatpak or its package for this system will run here instead." >&2
  exit 1
fi

# The window needs a desktop to open on; without one GTK stops the program
# with a panic, so say what is missing instead.
if [ -z "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
  echo "UpNext's window needs a desktop session, and this one has no display (neither DISPLAY nor WAYLAND_DISPLAY is set)." >&2
  echo "Start it from Steam on a desktop or in Gaming mode, or run bin/upnext serve for the server alone." >&2
  exit 1
fi

# gdk-pixbuf's loader list names each loader by its full path, which is
# only known now. A list per process, renamed into place, so two starts at
# once don't read half a file. The path is put in as plain text, since a
# Steam library's folder may hold an & that gsub would read as the match.
cache="${XDG_CACHE_HOME:-$HOME/.cache}/upnext/steam-copy"
if mkdir -p "$cache" 2>/dev/null &&
  BUNDLE="$here" awk '{
      out = ""; s = $0
      while ((i = index(s, "@BUNDLE@")) > 0) { out = out substr(s, 1, i - 1) ENVIRON["BUNDLE"]; s = substr(s, i + 8) }
      print out s
    }' "$here/lib/gdk-pixbuf/loaders.cache.in" >"$cache/loaders.cache.$$" &&
  mv -f "$cache/loaders.cache.$$" "$cache/loaders.cache"; then
  export GDK_PIXBUF_MODULE_FILE="$cache/loaders.cache"
fi

# GLib's modules and GTK's settings schemas from the depot, built for its
# GLib and GTK.
export GIO_MODULE_DIR="$here/lib/gio/modules"
export GSETTINGS_SCHEMA_DIR="$here/share/glib-2.0/schemas"

# No GStreamer plugins (lib/gstreamer-1.0/README.txt): the computer's,
# built for another GStreamer, stay out of WebKitGTK.
export GST_PLUGIN_SYSTEM_PATH_1_0="$here/lib/gstreamer-1.0"
unset GST_PLUGIN_PATH GST_PLUGIN_PATH_1_0
export GST_REGISTRY_1_0="$cache/gstreamer-registry.bin"

# libwebkit2gtk finds its helper programs at ././/lib/x86_64-linux-gnu/,
# relative to the working directory (bundle-linux.py says why). Steam
# starts a game in its own folder anyway.
cd "$here" || exit 1
exec "$here/bin/upnext-app" "$@"
