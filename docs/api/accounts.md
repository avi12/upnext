# Store accounts

[API index](../API.md) · [Players](players.md) · [AI accounts](ai-accounts.md)

Every route here acts for the request's player, so each is `403` while nobody has been chosen ([Who a request is for](players.md#who-a-request-is-for)) and takes the `UpNext-Player` header. Only the owner changes a connection or an account. None of them returns a sign-in or a key's value.

## Store connections and accounts

- A **StoreConnection** is a library a player linked, shown only to its owner: `id`; `kind` (`steam`, `gog`, `epic`, `heroic` or `galaxy`); `settings` (text values, the keys under `POST` below); `signed_in` (whether it holds a GOG or Epic sign-in); `last_sync` (when a sync last read it, or null); `count` (the games that sync found, or null).
- A **StoreAccount** is one store's library read through a connection, as the asker sees it: `id`, `connection_id`; `kind` (the connection's, or `origin`, `uplay` or `manual`); `store` (a copy's source); `name`; `owner_id` and `owner`; `sharing` (`everyone`, `chosen` or `owner`); `games` (copies in it, not counting removed ones); `mine`; `uses` (the asker sees its games: their own, or offered and switched on); `plays_on` (its playtime counts for the asker); and `players`, for its owner one entry per other player, `{"player_id": 2, "name": "Sam", "offered": false, "uses": true, "plays_on": false}`, otherwise `[]`. `offered` is whether `chosen` sharing offers it to them, `uses` their own switch (on until they turn it off), and `plays_on` the owner's tick that its playtime counts for them.

### Routes

- `GET /api/accounts` answers `{"connections": [StoreConnection], "accounts": [StoreAccount]}`: this player's connections, and every store account they own or are offered, theirs first.
- `POST /api/connections` `{"kind": "steam", "settings": {...}}` links a library for this player: `201` with the connection, which the next sync fills. `settings` holds only its kind's keys: `steam_id` (a SteamID64) and `steam_include_hidden` for Steam, `heroic_dir` for Heroic, `galaxy_db` and `galaxy_skip` for Galaxy, `epic_manifests` for Epic, none for GOG. `400` for an unknown kind, a key its kind doesn't have, a bad value or a `steam_id` another player's connection has; `403` for `galaxy_db`, `heroic_dir` or `epic_manifests` from another device, since they name a folder or file on this computer.
- `PUT /api/connections/{id}` `{"settings": {...}}` answers with the connection. A key set to `null` or `""` is cleared and the others are kept. `400` as for `POST`; `403` for a connection that isn't theirs, or a folder or file key from another device; `404` for no such connection.
- `DELETE /api/connections/{id}` answers `204`, removing the connection with its accounts and their games. Everyone's marks and history on those games stay, so linking it again brings them back. `403` for one that isn't theirs; `404` for no such connection.
- `PUT /api/accounts/{id}` `{"name", "sharing", "players": [{"player_id", "offered", "plays_on"}]}`, each optional, answers with the account. The name is 1 to 60 characters. Hours count only from a player's own accounts unless `plays_on` ("also plays on it") is set for them. `400` for a bad name or `sharing`, or a `player_id` that's the owner's or no player's; `403` for an account that isn't theirs; `404` for no such account.
- `PUT /api/accounts/{id}/use` `{"uses": false}` answers with the account: a player's own switch for an account offered to them ("Use these games"). Off takes its games out of their library and keeps their marks and history. `400` for their own account; `403` for one not offered to them; `404` for no such account.

A sync reads every connection; `upnext sync --account <id>`, or a sync job's `connection` option, reads one. The GOG and Epic sign-ins (`/api/gog/…`, `/api/epic/…`) are the requesting player's own.

AI accounts are on [AI accounts](ai-accounts.md).
