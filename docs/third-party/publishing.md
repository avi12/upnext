# Third-party services: publishing

[Third-party services](../THIRD_PARTY.md) › Publishing. Also: [stores](stores.md), [game data](data.md), [AI](ai.md), [what the project uses](project.md).

## The backlog verdicts service

UpNext talks to it only at the `https://` address in `verdicts_url`: the project's own service, `https://verdicts.upnext.games`, unless you set another. It calls `/v1/verdicts` and `/v1/reports` with your player's random key when you publish, withdraw or report a verdict, and reads the daily snapshot's manifest and summary, which sends nothing about you, when "Backlog verdicts from other players" is on.

The service's own `/terms` is shown on the consent screen, and everything it publishes is CC0 1.0 ([Backlog verdicts](../VERDICTS.md)). A verdict leaves your computer only after you accept the consent screen, and the service stores only the SHA-256 of your key.

## The shared game data

People say, Vibes and review tags for popular Steam games, under CC0, with their Steam appids and GOG ids. An install applies them with `upnext seed apply` or in Settings › Library data ([the API](../api/seed.md)), and the format refuses any other field.

The maintainers build it with `upnext seed build`, `fetch`, `generate` and `export`, from SteamSpy's catalogue, the Steam store's reviews, Wikidata, and GOG's catalogue for GOG ids by store page. Steam's, GOG's and SteamSpy's terms apply ([stores](stores.md), [game data](data.md)). It holds AI-written summaries and tags only, a risk the project states ([decision 6](../THIRD_PARTY.md#decisions)).

## Publishing data

What the terms say about a CC0 release of summaries and tags made from store reviews:

- **Steam**: content is for "personal, non-commercial use", publishing it needs Valve's written consent, and reviewers license their reviews to Valve only. Summaries UpNext's AI writes are new text, but they're derived from that content, and the contract terms apply whatever the copyright.
- **GOG**: personal use only, and the ban on "extraction tools".
- **SteamSpy**: it publishes no licence, so nothing is granted, and its tags come from Steam's user tags.
- **Can't go into a CC0 release at all**: PCGamingWiki (CC BY-NC-SA), ProtonDB's data (ODbL), RAWG ("No data redistribution"), IGDB (no redistribution without an agreement) and HowLongToBeat.
- **Fine**: Wikidata (CC0).
