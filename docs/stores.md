# Stores and accounts

UpNext needs one way into your games and, for AI picks, one AI. Everything else adds detail. The setup wizard walks through each of these, and Settings › Accounts changes them later.

| What | Brings | Without it |
|---|---|---|
| The Steam client on this computer | Your Steam library, playtime and installed games, from the client's own files | Use the Web API instead |
| A Steam Web API key ([get one](https://steamcommunity.com/dev/apikey)) and Sign in with Steam | Your Steam library from anywhere, recent play for the play diary, your Steam wishlist | The local client's files |
| [Heroic](https://heroicgameslauncher.com) | Epic, GOG and Amazon games installed through Heroic | Those stores are missing, unless GOG or Epic comes from its account |
| A GOG account | Your GOG games and playtime, including Galaxy sessions on other machines | Heroic or a Galaxy database |
| An Epic account, or the Epic Games Launcher | Your Epic games and playtime; the launcher's installs on Windows | Heroic or a Galaxy database |
| A GOG Galaxy 2.0 database | Games from every store Galaxy is linked to, with playtime | |
| A list you type in | EA app, Ubisoft Connect, Battle.net and Xbox games | Those stores are missing |
| An AI | Picks, review summaries, series guides, second chances, hidden gems | Picks from UpNext's own ranking ([Choosing an AI](ai.md)) |
| IGDB (a free Twitch developer app) | More genres, themes and keywords; trailers | Tags from Steam, SteamSpy and PCGamingWiki |
| A RAWG API key | More tags and genres | As above |

A game you own on several stores is one title in UpNext.

## Steam

There are three ways in:

1. **No key.** When the Steam client is installed on this computer, a sync reads its files: owned games, names, playtime and what's installed. This is what happens whenever no API key is set.
2. **The Web API.** Sign in with Steam in Settings (or let UpNext find your SteamID in the client), then paste a Web API key from [steamcommunity.com/dev/apikey](https://steamcommunity.com/dev/apikey); any domain name works there, such as `localhost`. Steam has no sign-in that grants library access, which is why sites that show your library after "Sign in with Steam" use a key of their own. Your profile's "Game details" must be public.
3. **`.env`**, with `STEAM_API_KEY` and optionally `STEAM_ID`.

Games you hid in the Steam client stay out of UpNext when it can read the client's files on this computer. `upnext config set steam_include_hidden true` brings them in.

## GOG

Connect GOG in Settings, or with `upnext gog-login`. A GOG login window opens, or you paste the link GOG lands on. Sync then brings your GOG games and their playtime from GOG's cloud, including sessions played through Galaxy on another machine.

A Galaxy `galaxy-2.0.db` can be imported as well. On Windows it's at `C:\ProgramData\GOG.com\Galaxy\storage\galaxy-2.0.db` and UpNext finds it by itself. Galaxy reaches other stores through plugins, which can disconnect or disappear, so a sync keeps what it read before, and Settings › Accounts lists the stores it brought in: untick one you no longer use and its games go at the next sync. Family-shared games you don't own, subscription titles and betas are left out.

## Epic

On Linux, Heroic brings Epic games. Without it, sign in to Epic in Settings (or `upnext epic-login`): open Epic's sign-in page and paste back the code the page ends on. Sync then reads your Epic games and their playtime. On Windows, the Epic Games Launcher's list of installed games is read too, with no sign-in. Epic's own games, and EA and Ubisoft games bought on Epic, come in; add-ons, demos and engine downloads don't.

## Games added by hand

The EA app, Ubisoft Connect, Battle.net and Xbox have no sign-in UpNext can use. On Windows, the games the EA app and Ubisoft Connect installed are read from the registry, and GOG Galaxy's plugins bring the rest if you use Galaxy. Otherwise, Settings › Accounts › Games added by hand takes a list of titles, one a line, for each of those stores.

## HowLongToBeat

HowLongToBeat has no public API, so UpNext reads its website's search the way a browser does, one request every 1.5 seconds, and their terms may not allow that. It's off unless you choose it: run it from Settings › Jobs (HowLongToBeat, Run now), give it a schedule there, or run `upnext enrich --with hltb`. It sends each game's name and stores the main-story, extras and completionist hours it finds. With its job on a schedule, wishlist games get their times too. Without it, picks know how long a game takes only where its review tags say its sessions are short.

## Next

- [Choosing an AI](ai.md)
- [Privacy](privacy.md): what each of these services is sent.
- [Third-party services](THIRD_PARTY.md): their terms.
