# Third-party services: stores

[Third-party services](../THIRD_PARTY.md) › Stores. Also: [game data](data.md), [AI](ai.md), [publishing](publishing.md), [what the project uses](project.md).

## Steam

**Web API.** UpNext calls `IPlayerService/GetOwnedGames`, `GetRecentlyPlayedGames`, `ISteamUser/GetPlayerSummaries`, `IWishlistService/GetWishlist` and `IStoreBrowseService/GetItems` with your own key. The [Steam Web API Terms of Use](https://steamcommunity.com/dev/apiterms) ask for a key on every call, kept confidential; at most 100,000 calls a day; data only about a user who asks for it, for their personal use; Valve's name and links as the docs describe; a privacy statement for nonpublic data; and nothing that implies Valve endorses the app.

They grant Steam data for "personal use via your Application", which fits a local app that fetches only your own library when you ask. Keys are each user's own and stay in their database. The privacy statement the terms ask for is written for hosted apps; [Privacy](../privacy.md) says what's stored and sent.

**Sign-in.** `steamcommunity.com/openid` runs in your browser to learn your SteamID. The same terms apply, and UpNext never handles your Steam password: the browser goes to Steam.

**Store.** `store.steampowered.com/api/appdetails`, `/appreviews/<appid>` (summaries and review texts) and `/api/storesearch`, under the [Steam Subscriber Agreement](https://store.steampowered.com/agreement) and [the reviews endpoint's docs](https://partner.steamgames.com/doc/store/getreviews). Only the reviews endpoint is documented, and Valve marks it deprecated in favour of `IUserReviewsService/GetAppReviews`, which "takes the same filters and returns the same fields". The agreement licenses content for personal, non-commercial use and forbids publishing it without consent.

Its section 4.C bans "Automation" that "interact[s] with Content and Services on Steam in any manner". Its examples are account creation, fake playtime and rewards, so it's aimed at abuse, but read literally it covers any automated client. These store endpoints are what every library tool uses, and UpNext paces them: about one request every 1.6 seconds, stopping after five failures in a row. Review text is kept in your own database and shown back to you, and is never published.

**Images.** Cover art, heroes and screenshots are hot-linked by your browser from `shared.akamai.steamstatic.com` and `cdn.cloudflare.steamstatic.com`. No specific terms were found. The art belongs to the publishers, and UpNext never copies or bundles it. The README's screenshot (`docs/images/pick.png`) shows three games' store art, which stays their publishers'.

**Credit in Settings › About:** "Data from the Steam Web API and store. UpNext is not affiliated with Valve."

## GOG

UpNext calls `auth.gog.com/token` (OAuth with GOG Galaxy's public client), `embed.gog.com` (owned games), `gameplay.gog.com` (playtime), `api.gog.com` (product data), `reviews.gog.com`, and `catalog.gog.com/v1/catalog`. That last one is the store's catalogue search, which finds a store page's product id by its slug, for series covers and the shared data's GOG ids.

GOG has no public developer API or terms. The [GOG User Agreement](https://support.gog.com/hc/en-us/articles/212632089-GOG-COM-User-Agreement), read through an archived copy, limits use to personal enjoyment and, in 11.1(e), forbids bots, scripts and "extraction tools" that interact with GOG services. Heroic, Lutris and gogdl have used the same client and endpoints for years, and GOG has said nothing for or against.

The OAuth client in `src/gog.rs` is the one every Galaxy install ships. It isn't a secret of UpNext's, and `.gitleaks.toml` allowlists it. The endpoints are unofficial and can change. GOG gets a courtesy credit.

## Epic Games

Only after you sign in, UpNext calls `account-public-service-prod03.ol.epicgames.com/account/api/oauth/token` (the authorization-code sign-in with the Epic Games Launcher's public client), `library-service.live.use1a.on.epicgames.com` (owned games and playtime) and `catalog-public-service-prod06.ol.epicgames.com` (titles, art and categories).

Epic's Terms of Service (`epicgames.com/site/tos`) apply, and there are no public developer terms for these endpoints. Legendary, Heroic and Playnite have used the same client and endpoints for years.

The client in `src/epic.rs` is the one every launcher install ships, and `.gitleaks.toml` allowlists it. You sign in on Epic's page in your own browser and paste the code that page shows, so UpNext never sees your password; the token is kept in the database, as GOG's is. The endpoints are unofficial and can change. Without a sign-in, UpNext reads only the launcher's install manifests on your disk. Epic gets a courtesy credit.

## GOG and Epic images

Cover art is hot-linked by your browser from `images.gog.com`, `images-1.gog-statics.com` and `cdn1.epicgames.com`. No specific terms were found. The art belongs to the publishers, and UpNext never copies or bundles it.

## Files on your computer

A GOG Galaxy database (`galaxy-2.0.db`) you point at or upload, Heroic's files, the local Steam client's, the Epic Games Launcher's manifests, the EA app's and Ubisoft Connect's installs, and Windows' registry are all read locally. No terms apply.
