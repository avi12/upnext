#!/bin/sh
# The packages' post-install script (contrib/linux/nfpm.yaml). It only prints:
# the user service stays off until each user enables it, and nothing here
# reads or writes anyone's library.
case "$1" in
  # dpkg: "configure", then the version being upgraded from, if any.
  configure) [ -z "$2" ] || exit 0 ;;
  # rpm: how many copies are installed once this one is, so 1 on a first install.
  1) ;;
  *) exit 0 ;;
esac
cat <<'MSG'
UpNext is installed. Open it from your launcher (under Games), or run
`upnext app` for the window or `upnext serve` for the web UI at
http://127.0.0.1:8787.

To keep it running from login, with the daily sync, each user who wants
that runs:

  systemctl --user enable --now upnext

Your library stays in ~/.local/share/upnext; removing the package leaves it.
MSG
exit 0
