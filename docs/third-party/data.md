# Third-party services: game data

[Third-party services](../THIRD_PARTY.md) › Game data. Also: [stores](stores.md), [AI](ai.md), [publishing](publishing.md), [what the project uses](project.md).

## SteamSpy

`steamspy.com/api.php?request=appdetails`, and `request=all` (the catalogue sorted by owners) only to build the [shared game data](publishing.md#the-shared-game-data). The [API page](https://steamspy.com/api.php) publishes no terms or licence. It asks for one request a second, one a minute for `all`, and says the data changes daily, so there's no need to ask more than once a day. Courtesy credit.

## HowLongToBeat

The site's search, found in its JavaScript the way howlongtobeatpy does, with a browser User-Agent. There's no public API, and the site belongs to Ziff Davis (IGN), which licenses machine access commercially through TollBit, so scraping is very likely outside its terms.

It's off by default everywhere: `upnext enrich --with hltb`, its own job, which has no schedule until you set one, and the wishlist's times, which follow that job's schedule. It sends game names one every 1.5 seconds and stops after three failures in a row. [Privacy](../privacy.md) says what it sends. Settings › About lists it, "only when you turn it on".

## PCGamingWiki

`pcgamingwiki.com/w/api.php` (`action=parse`, wikitext) and `api/appid.php`. The content is under [CC BY-NC-SA 3.0](https://creativecommons.org/licenses/by-nc-sa/3.0/); its Copyrights page refused the request, so the licence is as Wikipedia's article on the site gives it. See also its [API page](https://www.pcgamingwiki.com/wiki/PCGamingWiki:API). Anonymous Cargo queries are refused, which is why UpNext reads wikitext.

The game sheet's details come partly from its genres, themes, perspectives, pacing and series. The licence asks for the title, source and licence, for non-commercial use, and for adaptations that are shared to be shared alike. Settings › About carries "PCGamingWiki content is available under CC BY-NC-SA 3.0." with the licence link, and the game sheet links the wiki page where there is one. Share-alike and non-commercial mean PCGamingWiki's data can't go into a CC0 release.

## Wikidata

`query.wikidata.org/sparql`, for cross-references by Steam app id and name, series, and the shared game data's GOG ids and store pages. Wikidata is [CC0](https://www.wikidata.org/wiki/Wikidata:Licensing), so no attribution is needed. It asks for an informative [User-Agent](https://meta.wikimedia.org/wiki/User-Agent_policy) with contact information, and [limits](https://www.mediawiki.org/wiki/Wikidata_Query_Service/User_Manual#Query_limits) each client to 60 seconds of query time a minute and 30 failed queries a minute. Courtesy credit, with CC0.

## ProtonDB

`protondb.com/api/v1/reports/summaries/<appid>.json`, only in Linux mode. No terms were found for the site. The endpoint is unofficial and undocumented. Its [report exports](https://github.com/bdefore/protondb-data) are under ODbL 1.0 (attribution, share-alike), which covers those exports, not this endpoint. Courtesy credit.

## RAWG

`api.rawg.io/api/games` and `/games/<slug>`, only with your own key, so the quota is yours. The [API docs and terms](https://rawg.io/apidocs) ask for a key on every request and "Attribute RAWG as the source of the data and/or images and add an active hyperlink from every page where the data of RAWG is used." It's free for commercial use under 100,000 monthly users, with 20,000 requests a month on the free tier, and the data may not be redistributed.

RAWG's tags and genres are merged into a game's tags, which show on the game sheet and in the library. Settings › About links rawg.io, and when a game has RAWG data its sheet links the game's RAWG page twice: in the "Info from" line at the bottom, and beside RAWG's tags in the breakdown by source ([decision 1](../THIRD_PARTY.md#decisions)). The library's tag filter has no link of its own.

## IGDB

`id.twitch.tv/oauth2/token` and `api.igdb.com/v4/games`, with images from `images.igdb.com`, using your own Twitch app's client id and secret. UpNext ships no keys. Neither the [Twitch Developer Services Agreement](https://legal.twitch.com/en/legal/developer-agreement/) nor the [IGDB API docs](https://api-docs.igdb.com/) could be loaded, so the facts come from archived copies.

They ask that the client id and secret stay secret, allow 4 requests a second with 8 open at once, make it free for non-commercial use with commercial use through partner@igdb.com, and forbid redistributing the data without an agreement. The agreement has a clause about using Twitch's "Program Materials" with open-source-licensed software. UpNext ships none of Twitch's materials, and many open-source IGDB clients work the same way. Settings › About credits it, with the agreement's link.

## YouTube

Trailers are plain links (`youtube.com/watch?v=…`) opened in your browser, with nothing embedded. Plain links aren't use of the YouTube API Services.

## Pricing pages

Once a week: `platform.claude.com/docs/en/about-claude/pricing.md`, `developers.openai.com/api/docs/pricing.md` and `openrouter.ai/api/v1/models`, under the sites' own terms. They're public pages published in Markdown for machine reading, and OpenRouter's public models API, read once a week per install.
