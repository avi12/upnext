# Contributing to UpNext

Thanks for looking. UpNext is a Rust server (axum, rusqlite, clap) in `src/` with a React, Vite and Tailwind UI in `web/`. This page covers setting up and the project's conventions. [Checks](docs/contributing/checks.md) has the tests CI runs, and [Where things go](docs/contributing/code-map.md) is a map of the code. `AGENTS.md` has the same rules in short, for contributors who use an AI coding agent.

UpNext itself is developed with AI coding assistants, mostly Claude Code, as the README says. A pull request written with an assistant's help is welcome on the same terms as any other: it passes the checks, and you stand behind every line of it.

## Setting up

You need:

- Rust 1.90 or newer (`rust-version` in Cargo.toml; CI uses current stable), from [rustup](https://rustup.rs).
- Node 22.18 or newer, on an even-numbered release (22, 24, 26 and on, as `engines` in web/package.json says; CI uses 24), and npm. The third-party notices script needs 22.18, which reads TypeScript.
- On Linux, for the app window and the GOG login window: WebKitGTK 4.1 and GTK 3 with their headers (`libwebkit2gtk-4.1-dev libgtk-3-dev pkg-config` on Debian and Ubuntu, `webkit2gtk-4.1 gtk3` on Arch). Without them, build with `--no-default-features`: the server alone, which is all most changes need.

```sh
git clone https://github.com/zytocc/upnext && cd upnext
npm ci --prefix web                    # a release build embeds web/dist, and won't compile without it
npm run build --prefix web
cargo build
```

### A library to work on

Don't develop against your real library. `upnext mock-library` writes a made-up library and taste profile into an empty database:

```sh
export UPNEXT_DATA_DIR=$PWD/data/dev    # its own database and backups
cargo run -- mock-library
cargo run -- serve                       # http://127.0.0.1:8787
```

It has two players, so the UI asks who's playing; `mock-library --one-player` makes only the first, as most installs are. `data/` is in `.gitignore`. `upnext doctor` prints every path in use and where it came from.

A second copy of the desktop app beside an installed one, such as a scratch build on Windows, needs its own `UPNEXT_DATA_DIR`. Its window keeps its WebView2 data under `cache` in that folder, unless `UPNEXT_CACHE_DIR` names another, because WebView2 won't start on a folder a running copy holds. Set both in the environment rather than in a `.env`. When a bad setting stops the app, the window that says so (and GOG's sign-in window) can't trust the `.env` and reads only the environment, so with them only in a `.env` it falls back to the installed copy's folder.

### The UI with hot reload

Run the server as above, then in another terminal:

```sh
cd web && npm run dev
```

Vite serves the UI on its own port and proxies `/api` to `127.0.0.1:8787` (`web/vite.config.ts`). It passes the browser's `Host` through unchanged, so the server's Origin check (a write must come from a page on the same port as the `Host` it names) keeps working. A debug build (`cargo run`) serves `web/dist` from disk, so `npm run build` shows up without restarting the server.

### The AI without spending anything

Picks and the other AI features call a model. For development you don't need one:

- `upnext prompt "a short chill game"` prints the exact prompt a pick would send, without sending it.
- Point UpNext at any OpenAI-compatible server: `UPNEXT_LLM=openai UPNEXT_OPENAI_BASE_URL=http://127.0.0.1:11434/v1 UPNEXT_OPENAI_MODEL=<model>` for Ollama, or the fake model the end-to-end test uses (`node web/e2e/fake-llm.mjs`, which answers on port 8799).
- To make real calls, use your own Claude Code CLI, Codex CLI or API key, as [AI](docs/ai.md) describes.

## Conventions

- Refactors change structure only. A change in behaviour goes in its own commit, with a test.
- Never commit a database, a token, a `.env`, or fixtures cut from a real library. Fixtures in `tests/fixtures/` are made up or trimmed public responses.
- Keep the README, the guides in `docs/` and the API guide (`docs/API.md` and its pages in `docs/api/`) true to the code in the same change.
- `docs/dev/` holds the maintainers' runbooks: releases, the self-hosted CI runners and the backlog verdicts service. You don't need them to contribute.
- Releases are tags; `docs/dev/RELEASE.md` has the steps and how the release workflow (dist) is set up.

## Pull requests

Open an issue first for anything large, so we can agree on the shape before you write it. Keep a pull request to one change, fill in the template, and make sure [the checks](docs/contributing/checks.md) pass. By contributing, you agree that your contribution is licensed as the project is, under MIT OR Apache-2.0, without any additional terms or conditions.
