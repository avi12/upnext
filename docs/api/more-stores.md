# Epic, Galaxy and the other stores

[API index](../API.md) · [Steam](steam.md) · [GOG](gog.md) · [Games](games.md#sources-and-launchers)

## Epic

Epic games come from Heroic where it's installed, and without it from either of two more places. Both make copies with `source` and `launcher` `epic` and the Epic app name as `source_id`, as Heroic does, so a game seen both ways is one row and Heroic keeps it.

- **The Epic sign-in.** UpNext signs in as the Epic Games Launcher does: `auth_url` is Epic's sign-in page, which ends on a page of JSON holding an `authorizationCode`, and that code (or the whole page) is posted back. The sign-in is kept in the database, on the player's Epic connection, and renewed at each sync. A sync reads the owned library, each item's store details (cached in `epic-catalog.json` in the cache folder) and the playtime Epic keeps. Add-ons, demos, engine downloads and private test builds are left out; games that play through the EA app or Ubisoft Connect are kept, as Epic copies.
- **The Epic Games Launcher's installs.** On Windows, the launcher's manifests in `%ProgramData%\Epic\EpicGamesLauncher\Data\Manifests` (elsewhere with `epic_manifests` or `UPNEXT_EPIC_MANIFESTS`) list the installed games, with no sign-in. Without the sign-in, a game uninstalled since stays, as owned and not installed.

`GET /api/epic` gives `{"connected": false, "display_name": null, "auth_url": "https://www.epicgames.com/id/login?redirectUrl=…", "error": null}`. `error` says why the sign-in ended when Epic refused to renew it at a sync; the stored sign-in is then deleted.

`POST /api/epic/login` with `{"code": "<the authorizationCode, or the whole page>"}` answers in that shape, connected, with the `display_name`. `400` when there's no code in it or Epic turns it down (a code works once, for a few minutes).

`DELETE /api/epic` deletes the stored sign-in and answers in that shape. The Epic games already read stay, as owned, at later syncs too: signing out doesn't give a game back, as uninstalling doesn't, and hiding a game takes it out. `upnext epic-login [code]` and `upnext epic-logout` do the same from a terminal.

`GET /api/settings` has `epic` (that shape) and `epic_launcher_dir` (the manifests folder found, or null), and `GET /api/config` has `epic_connected`. The `epic` profile source counts as configured with Heroic, the sign-in or the launcher, with a `note` naming which.

## The EA app and Ubisoft Connect

On Windows, a sync reads what these installed (sync options `ea` and `ubisoft`). Neither has a sign-in UpNext may use for the owned library, so GOG Galaxy's plugins or games added by hand bring the rest. A game uninstalled since stays, as owned and not installed. Neither tracks playtime for UpNext.

- **The EA app**: Windows' Apps list, where an entry is EA's when its uninstall command runs through `EAInstaller` or its folder holds `__Installer\installerdata.xml`, which gives the English title and EA's content id (`source_id`; the folder's name as `dir-…` when there's none). Copies have `source` and `launcher` `origin` and no `run_url`, since EA's launch links take offer ids the installer data doesn't hold.
- **Ubisoft Connect**: each install under `HKLM\SOFTWARE\Ubisoft\Launcher\Installs\<game id>` (the 32-bit view first) with its `InstallDir`, named by the Apps list's `Uplay Install <game id>` (else the folder's name). A key whose folder is gone is left out. Copies have `source` and `launcher` `uplay`, the game id as `source_id`, and `run_url` `uplay://launch/{id}/0`.

[`GET /api/setup`](setup-wizard.md)'s `stores` has `ea` and `ubisoft`, `{"installed": 3}` (0 off Windows). The profile lists the `origin` ("EA app") and `uplay` ("Ubisoft Connect") sources once they have games.

## Games added by hand

The EA app, Ubisoft Connect, Battle.net and the Xbox app have no sign-in a program like UpNext may use, so a list of titles typed or pasted in brings their games in. Each title is an owned, uninstalled copy with the store's `source` (`origin`, `uplay`, `battlenet` or `xboxone`, as Galaxy names them), `launcher` `manual` and the title's own key as `source_id`. Enrich finds its details on Steam by name.

- `GET /api/library/by-hand` gives `{"stores": [{"store": "origin", "label": "EA app", "titles": ["Dead Space", "Mass Effect Legendary Edition"]}, {"store": "uplay", "label": "Ubisoft Connect", "titles": []}]}`: every such store, in that order.
- `PUT /api/library/by-hand/{store}` with `{"titles": ["Dead Space", "Far Cry 5\nAnno 1800"]}` replaces the store's list and answers `{"total": 3, "added": 2, "removed": 1, "lists": {…}}`. Titles are one a line (an entry may hold several lines); blank lines, bullets, ™, ® and © signs and repeats are dropped. A title left off leaves the library, as a game a sync no longer sees does, and `{"titles": []}` takes them all out. `400` for any other store or more than 5,000 titles.

## GOG Galaxy

With a Galaxy database (`galaxy_db`, or one uploaded in the [wizard](setup-background.md#post-apigalaxyupload)), a sync reads every store Galaxy is linked to. It leaves out games Galaxy says aren't owned (Steam Family Sharing), subscription titles (`@subscription` keys, EA Play), and betas, test servers, playtests, demos and teasers.

- `galaxy_skip`, such as `["origin"]`: the integrations a sync leaves out. `PUT` takes them comma-separated (`"origin,xboxone"`; `ea` and `xbox` are taken for those two, and `gog` is refused). A sync removes the copies of a store it names.
- `galaxy_integrations`: every store a Galaxy sync has brought games from, GOG's own first, kept after the store is gone from Galaxy (its copies stay too): `[{"platform": "origin", "label": "EA app", "games": 55, "last_added": "2024-05-18", "connected": true, "left_out": false, "seen_at": 1790000000}]`. `connected` is what Galaxy's `PlatformConnections` says, or null. Galaxy calls unlinked plugins connected too, so it's reported and never acted on.
