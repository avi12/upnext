# Privacy

Everything UpNext knows is in its database on your computer: `~/.local/share/upnext` on Linux, `%LOCALAPPDATA%\upnext` on Windows, the `/data` volume in Docker. There's no account and no analytics. UpNext's one server is the backlog verdicts service, and it hears from your computer only when you publish a verdict or while verdicts from other players are on (the setup wizard turns them on; Settings › Library data turns them off). These are the services UpNext talks to, and what each gets:

| Service | When | What it's sent |
|---|---|---|
| Steam Web API | Sync, the play diary (hourly, if on), the wishlist | Your Steam API key and SteamID; it answers with your games, playtime, recent play, profile name and wishlist |
| Steam sign-in | When you choose Sign in with Steam | Your browser goes to Steam; UpNext gets back your SteamID |
| Steam store | Fetching details | Steam app ids, and the names of games from other stores to find them on Steam |
| SteamSpy, ProtonDB | Fetching details (ProtonDB only on Linux) | Steam app ids |
| GOG | Sync, once you connect your account; fetching details and series covers, with no account | Your GOG sign-in, for sync; GOG product ids, and series games' store-page names to find their ids |
| Epic Games | Sync, once you sign in to Epic | Your Epic sign-in; it answers with your games, their store details and your playtime |
| Wikidata, PCGamingWiki | Fetching details and series | Game names and Steam app ids |
| IGDB, RAWG | Fetching details, with your own credentials | Your credentials, game names |
| HowLongToBeat | Only when you turn it on | Game names |
| Your identity provider | Only if you set one up, when someone signs in with it | Your browser goes to it; UpNext reads its public settings and keys and sends it the sign-in's code with UpNext's client id and secret; it answers with who signed in |
| UpNext's shared game data, on GitHub's release pages | Once during setup, unless you untick it, and when you ask for it again (Settings › Library data, or `upnext seed apply`) | Nothing about you; GitHub sees your address, as any server does |
| UpNext's backlog verdicts service (`https://verdicts.upnext.games`, or another address in `verdicts_url`) | Daily, if verdicts from other players are on; and when you publish, withdraw or report a verdict | The daily download sends nothing about you; like any request, it shows the service your address and UpNext's version. A verdict you publish goes with your player's random key, never your name, accounts or library, and is published under CC0 ([Backlog verdicts](backlog-verdicts.md)) |
| Steam, GOG, IGDB and Epic image servers | When a page shows cover art | Your browser loads the images directly, so these servers see your IP address |
| Anthropic, OpenAI and OpenRouter pricing pages | Weekly, to show costs | Nothing about you |
| **The AI you chose** | When you ask for picks, or run an AI job | Below |

## What goes to the AI

UpNext sends the AI only what the task needs, and never your keys, SteamID, account names, sign-ins or file paths.

- **Picks**: your request; up to about 120 candidate games from your library, each with its name, stores, genres and tags, length, review scores, a short summary of what players say, your playtime and whether you shelved or abandoned it; your most-played games (up to 8, when you have fewer than five favourites); your taste profile and your ratings of games; the routine the lineup is for, if one is chosen; what you said about earlier lineups and how picks you rated in the last 90 days landed; games recommended recently; and wishlist games, if the wishlist is on.
- **Review summaries**: each game's name and public player reviews from Steam and GOG.
- **Series guides**: a series' name and its games, each with its year, length, review score and how long you played it.
- **Second chances**: an abandoned game, what you said about why you stopped, and player reviews written since.
- **Hidden gems**: unplayed games with their details, tags, publishers and review summaries.

With the Claude Code or Codex CLI, the CLI adds its own instructions to each call, as it does for any use. With an API key, the call goes to Anthropic or OpenAI under your key. With a local server, nothing leaves your machine. `upnext prompt "…"` prints a pick's exact prompt without sending it.

## On your network

The Android app talks only to the computers you add to it, and sends each one its device's name and kind when it asks to be allowed.

While other devices are allowed, UpNext announces on your home network that it runs there, with the computer's name (or one you give it), an install id, its port and version. Settings › Devices and API turns that off.

[Third-party services](THIRD_PARTY.md) lists every service with its terms, and [SECURITY.md](../SECURITY.md) covers who can reach the server.
