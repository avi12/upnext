# Docker

For a home server or a NAS. The image, `ghcr.io/zytocc/upnext`, has the server and the web UI, without the app window. It's published for amd64 and arm64 (a Raspberry Pi 4 or 5 on a 64-bit OS, an arm64 NAS), and Docker pulls the right one.

```sh
git clone https://github.com/zytocc/upnext && cd upnext
docker compose -f contrib/docker/compose.yaml up -d
docker logs upnext      # "Open http://localhost:8787 and enter 482 913 to pair your browser"
```

Open `http://localhost:8787` and type the code. Inside a container no browser counts as "this computer", so every browser pairs, the host's own too. The first code is in the log when UpNext starts and lasts an hour, until something is paired; `docker exec upnext upnext pair` prints a new one. For a script's token: `docker exec upnext upnext token create --name ha --scope read`.

## Data and security

The database (with the GOG and Epic sign-ins), backups and an optional `.env` are in the `/data` volume. The image runs as uid 1000; for another user, run it with `--user <uid>:<gid>` (compose: `user:`). It runs read-only with a tmpfs `/tmp`, with no Linux capabilities and no way to gain privileges. The compose file sets all four; `docker run` takes `--read-only --tmpfs /tmp --security-opt no-new-privileges --cap-drop ALL`.

## Local libraries

Steam and Heroic folders come in only when mounted, read-only: uncomment the Steam or Heroic volume and its `UPNEXT_STEAM_DIR` or `UPNEXT_HEROIC_DIR` line, or copy the lines setup's "Where UpNext runs" step writes for your system. Without them, Steam works through the Web API, GOG through its account, and Galaxy through an upload. GOG signs in with a "Send to UpNext" bookmark, since no window can open in a container.

## Your network

The compose file publishes the port on `127.0.0.1` only. Drop the `127.0.0.1:` to reach it from other devices, and set "Address for other devices". A device that asks to be allowed is shown in the log, with the command that allows it: `docker exec upnext upnext pair approve 4821`. Docker's own network keeps UpNext's announcement from reaching yours, so other computers find it only with `network_mode: host`; otherwise type its address.

## The AI

The image has no Claude or Codex CLI. An API key, or an OpenAI-compatible server such as Ollama on the host at `http://host.docker.internal:11434/v1` (the compose file adds that name), works as it is.

**Your Claude subscription.** Claude Code's licence doesn't allow passing it on, so the image doesn't carry it. With `UPNEXT_INSTALL_CLAUDE: "1"` the container installs it from Anthropic into `/data` when it starts and finds none (about 250 MB, checked against Anthropic's manifest), and `docker exec upnext upnext install-claude` installs it by hand. The version is whatever Anthropic's stable channel names at the time, and it updates itself as it does on a desktop, so the Claude Code in the container can change without a new image or a line in UpNext's log. A restart doesn't download it again.

It signs in with a token: `claude setup-token` on any computer where you use Claude prints one, and the container takes it as `CLAUDE_CODE_OAUTH_TOKEN`. The compose file has both lines, commented out, and setup's AI step shows whether Claude Code is installed and has the token. The token signs in as you, so rather than leave it in the compose file, which is easy to share or commit, keep it in an `upnext.env` file only you can read (`chmod 600`) and use the compose file's `env_file:` line. `UPNEXT_CLAUDE_RELEASES` points the install at a copy of Anthropic's release site, for the smoke test.

## Running it

`TZ` sets the hour the daily sync runs at. Stopping lets a running job record where it got to, and the next start resumes it; the compose file gives it 30 seconds. The health check is `upnext healthcheck`.

## Building it yourself

`docker build -t upnext .` at the repository's root, then `image: upnext` in the compose file. `docker buildx build --platform linux/arm64 -t upnext:arm64 --load .` builds the arm64 image on an amd64 machine; the server is cross-compiled, with no emulation. `contrib/docker/smoke-test.sh upnext` checks an image the way CI does.

Next: [Stores and accounts](../stores.md).
