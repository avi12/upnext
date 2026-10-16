# UpNext

**Pick what to play next from the games you already own.**

Launchers like Playnite and Heroic gather your games in one place. UpNext tells you which one to play tonight, with or without an AI.

Website: [upnext.games](https://upnext.games)

UpNext gathers your game libraries from Steam, Epic, GOG, Amazon, the EA app and Ubisoft Connect into one list and looks up what each game is like. Tell it what you're in the mood for ("about an hour, something calm with a bit of story") and it suggests a few games from your backlog that fit, with the reasons. The suggestions come from an AI you choose: your own Claude or ChatGPT subscription, an API key, or a model on your own machine. Without an AI, UpNext ranks your games itself.

<img src="docs/images/pick.png" width="800" alt="The Pick page: a request, and a lineup of three games with why each fits and what players say about it">

*The Pick page on the made-up library from `upnext mock-library`. The cover art belongs to the games' publishers.*

- **One library.** Every store in one list, with a game you own twice shown once.
- **Details for every game.** Genres, tags, review scores and what players say, from the stores and open game databases.
- **Picks that fit.** Ask for a length, a mood or "something I can actually finish". Turn a lineup down and say why, rate the picks, or have it decide between two games.
- **A taste profile you can read and edit.** Favourites, likes, hard nos and the patterns behind them, each with where it came from, and routines for the nights you play with friends or on the couch.
- **More ways in.** Second chances for games you abandoned, hidden gems, genres new to you, a roll of the dice, and a wishlist of games that follow on from series you played.

UpNext runs on your own computer and keeps everything in a local database. It has a web UI, an app window, a big-screen mode for a controller, and a command line. There are no accounts and no analytics. Besides your stores, game-data services and AI, it downloads UpNext's shared game data at setup and public backlog verdicts daily (a setup switch), and neither download carries anything about you.

## Install

| Platform | Get it | Guide |
|---|---|---|
| Windows 10 and 11 | [The installer](https://github.com/zytocc/upnext/releases/latest/download/upnext-x86_64-pc-windows-msvc.msi) (`.msi`) from the latest release | [Windows](docs/install/windows.md) |
| Debian, Ubuntu, Fedora | The `.deb` or `.rpm` from a release | [Linux](docs/install/linux.md) |
| Arch Linux | The release's `PKGBUILD` | [Linux](docs/install/linux.md#arch-linux) |
| Any Linux, Steam Deck | The Flatpak | [Flatpak and Steam Deck](docs/install/flatpak.md) |
| Other Linux | The release archive | [Linux](docs/install/linux.md#release-archive) |
| Home server, NAS | The container image | [Docker](docs/install/docker.md) |
| Android | The APK, to use UpNext on your computer from a phone or TV; it isn't in a release yet | [Android](docs/install/android.md) |
| From source | Rust and Node | [Building from source](docs/install/source.md) |

With Docker it's three lines:

```sh
git clone https://github.com/zytocc/upnext && cd upnext
docker compose -f contrib/docker/compose.yaml up -d
docker logs upnext      # shows the address and a code to pair your browser
```

The first time UpNext opens, a setup walks you through your stores, the AI and what runs when. `upnext doctor` prints every path it uses and what's missing.

## Documentation

[The documentation](docs/README.md) covers everything past the install:

- [Stores and accounts](docs/stores.md): what UpNext can read, and what each store needs.
- [Choosing an AI](docs/ai.md): the options and their defaults.
- [Privacy](docs/privacy.md): what is sent where.
- [Backlog verdicts](docs/backlog-verdicts.md): how a game fits a backlog, from other players.
- [Running UpNext](docs/running.md), [Big screen](docs/big-screen.md), [Players](docs/players.md) and [Other devices](docs/other-devices.md).
- [Commands](docs/commands.md) and the [HTTP API](docs/API.md).

## Contributing

Bug reports, ideas and pull requests are welcome. [CONTRIBUTING.md](CONTRIBUTING.md) has the setup and the checks; [SECURITY.md](SECURITY.md) says how to report a vulnerability privately.

UpNext is developed with AI coding assistants, mostly Claude Code. Every change goes through the same tests and CI as any other.

## Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by [SignPath Foundation](https://signpath.org/).

Signing starts with the first release after SignPath Foundation accepts UpNext; until then, 0.1.0 included, the Windows programs and installer aren't signed. Only files the `release` workflow (`.github/workflows/release.yml`) builds are signed: it builds every file on a release page from this repository, from the commit the release's tag names, on GitHub's own runners, and publishes each with its SHA-256 checksum.

- **Committers and reviewers:** [zytoc](https://github.com/zytoc). Every change from anyone else, such as a pull request, is reviewed by a committer before it's merged.
- **Approvers:** [zytoc](https://github.com/zytoc), who approves each release's signing by hand.

**Privacy policy:** UpNext has no accounts and no analytics. [Privacy](docs/privacy.md) lists every service it talks to, when, and what each is sent: your stores, the game-data services, the AI you choose, and UpNext's shared game data and backlog verdicts services, which see only the connection and UpNext's version unless you publish a verdict.

## Licence

UpNext is licensed under either of the [Apache License, Version 2.0](LICENSE-APACHE) or the [MIT licence](LICENSE-MIT), at your option. Unless you say otherwise, any contribution you submit for inclusion in UpNext is licensed the same way, without any additional terms or conditions.

UpNext is provided as is, with no support and no warranty; the licences say so in full. Bug reports and questions are welcome in the [GitHub issues](https://github.com/zytocc/upnext/issues), and are answered when someone can.

Game data comes from the services in [Third-party services](docs/THIRD_PARTY.md), under their own terms; Settings › About lists the credits. UpNext isn't affiliated with Valve, GOG, Epic, Amazon or any of those services. The libraries, fonts and icons built into UpNext are under their own licences, which [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt) lists with their texts; every download carries it, and Settings › About links to it.

## Supporting UpNext

UpNext is free and stays free. If you'd like to support the work, you can on [Ko-fi](https://ko-fi.com/zytocc), or through the Sponsor button at the top of the repository.
