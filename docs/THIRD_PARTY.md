# Third-party services

Every service UpNext talks to, what it calls, the terms that apply and what they ask of an app like this one. UpNext runs on your own computer, so every request goes from your machine, with your own keys and sign-ins where a service needs them. Apart from UpNext's own backlog verdicts service, which gets only the verdicts a player chooses to publish and the daily download's requests, there's no UpNext server in between. [Privacy](privacy.md) lists what each service is sent.

The facts come from the services' own pages; where a page couldn't be loaded, the entry says where they came from instead. Nothing here is legal advice. Settings › About shows the attributions (`web/src/lib/credits.ts`); keep the two in step when a source is added.

- [Stores](third-party/stores.md): Steam, GOG, Epic, their image servers, and the files UpNext reads locally
- [Game data](third-party/data.md): SteamSpy, HowLongToBeat, PCGamingWiki, Wikidata, ProtonDB, RAWG, IGDB, YouTube and the pricing pages
- [AI](third-party/ai.md): the Claude Code and Codex CLIs, the Anthropic and OpenAI APIs, OpenAI-compatible servers and Microsoft Copilot
- [Publishing](third-party/publishing.md): the backlog verdicts service, the shared game data, and what the terms say about publishing data
- [What the project uses](third-party/project.md): hosting and stores, the Steam copy's Linux libraries, and the Android app's libraries

## Notices

The full list of what is built into UpNext is `THIRD_PARTY_NOTICES.txt` at the repository's root: every Rust crate and npm package (the fonts among them), Microsoft's WebView2 loader and the container image's tini, each with its version, licence, and the licence and notice texts it asks to be passed on. `contrib/notices/generate.mjs` makes it from the lock files, the `notices` workflow keeps it current, every release carries it, and the programs serve it at `/third-party-notices.txt`, which Settings › About links to. It repeats the credits here.

## Decisions

The project's choices about these terms:

1. **RAWG's link "from every page".** RAWG is linked on the game sheet wherever its data shows: the sheet's "Info from" line and the RAWG label in the tags' breakdown by source both link the game's RAWG page, and Settings › About links rawg.io.
2. **Steam's store endpoints and the Subscriber Agreement's section 4.C.** UpNext keeps using them, documented here, as every library tool does.
3. **HowLongToBeat.** The scraping stays, off by default, and on only when you turn it on.
4. **GOG's unofficial API.** UpNext uses it, documented as unofficial, as Heroic and Lutris do. That includes the store's catalogue search, which looks up a store page's product id by its slug: one search per slug, paced as the other GOG calls are, keeping only an exact match. Wikidata has GOG's product id for next to no game, so the shared data's GOG ids and the series covers from GOG need it.
5. **The Claude Code CLI.** It's the default AI backend, described plainly, with the API key listed beside it.
6. **The shared game data.** Publish only AI-written summaries and review tags derived from Steam reviews: no review text, and nothing from PCGamingWiki, ProtonDB, RAWG, IGDB or HowLongToBeat. This goes against the letter of Steam's terms (personal, non-commercial use; publishing needs Valve's written consent) and GOG's ([Publishing data](third-party/publishing.md#publishing-data)), and is a risk the project accepts and each release states.
7. **IGDB.** It's used on each user's own credentials.
