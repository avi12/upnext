# Players, PINs and your data

## Players, PINs and hidden games

Several people can each have their own player ([Players](../players.md)). Players keep the household's picks, marks and taste apart; they don't keep secrets from each other.

- **No login.** Anyone at this computer, or at an allowed device that isn't bound to one player, can switch to any player who has no PIN, and anyone who can read UpNext's files can read every player's data, hidden games included.
- **PINs** are four to eight digits, kept only as an Argon2id hash. The picker asks for one before switching to its player, and so do their Hidden by me list, removing them, and binding a token or device to them. Five wrong tries lock that player's PIN for a minute, then two, four and so on, counted in memory across every place a PIN is asked. Anyone with the database file can guess a PIN this short offline, so UpNext never calls it security, and the command line on this computer doesn't ask.
- **Choosing a player.** A browser's choice is an `HttpOnly`, `SameSite=Strict` cookie (`Secure` over HTTPS) holding the player's id with an HMAC-SHA-256 signature, made with a random key kept in the database, so nobody can choose a player with a PIN by writing the cookie by hand. The `UpNext-Player` header names a player for a script, but never one with a PIN. While the default player has a PIN and nobody has been chosen, every per-player request answers `403` until someone is.
- **Bound devices and tokens.** A phone allowed in, paired or given a token for one player always acts for that player, whatever header or cookie it sends (`403` for another). It can't switch, add, rename or remove players, change another player's accounts or sharing, or change what is everyone's (the household settings, the schedules, setup). It can change its own player's settings and PIN.
- **Shared accounts.** A store or AI account's owner decides who it's offered to, and only the owner changes that, renames it or removes it. Its keys and sign-ins stay write-only for everyone, the owner included: a player using a shared AI account has its key used for them, never shown.
- **Hidden games** are hidden in UpNext only. The database, its backups and exports, and the command line on this computer hold them all. Job messages, their logs, the log download and `GET /api/logs` name a game hidden from a player as "a hidden game" to that player; the command line's own log isn't masked. A game hidden in the Steam client never reaches UpNext while it reads that client's files and `steam_include_hidden` is off.
- **Removing a player** is only on this computer, like a restore, and takes a backup first. It revokes the player's bound tokens and devices.

## Your data and keys

- The database and the backups are files under your user's data folder: `~/.local/share/upnext` on Linux, `%LOCALAPPDATA%\upnext` on Windows, or the `/data` volume in a container. `upnext doctor` prints the paths.
- API keys you enter (Steam, RAWG, IGDB, Anthropic, OpenAI, an OpenAI-compatible server) are stored in that database in plain text, as are the GOG and Epic sign-ins. Anyone who can read your files can read them. On Linux and macOS UpNext makes the database and the backups readable by your user only (mode 600). On Windows they are under `%LOCALAPPDATA%`, which only your account and administrators can read, so UpNext doesn't change their permissions.
- Keys are never sent back by the API: settings report whether a key is set, not its value. Nor can a token send one somewhere new: the OpenAI-compatible server's key goes to the address set beside it, so changing that address from another device clears the saved key unless the same change brings a new one (and isn't allowed there when the key is in `.env`). That server's error pages aren't passed back, only its API error messages.
- A backup download or `upnext export` "without keys" leaves out keys, tokens and sign-ins, and what names you or your computer: the players' names (each becomes "Player N"), PINs and identity provider accounts, the cookie's signing key, store accounts' names and ids, every setting that names a file, folder or address, and every job's log, result and error and every AI call's error, which can name folders and hosts. Restored on the same computer, its own settings come back, but the players' identity provider links and the folders on other players' connections don't.

## Backlog verdicts

A player's [backlog verdict](../VERDICTS.md) is sent only when they press Send, with their random key. The key goes only to `verdicts_url`, which only this computer can change, and the service keeps only its SHA-256.

Other players' verdicts are a daily download that sends nothing about you. It isn't signed: the manifest's SHA-256 and size catch a damaged download, and only the HTTPS connection says where it came from. So `verdicts_url`, and an address given to `upnext verdicts apply`, must be `https://` (plain `http://` only to `localhost` or `127.0.0.1`); one saved before that rule is ignored. UpNext never follows a redirect from `https://` to plain `http://`. Only a newer day's snapshot replaces the kept one, and `upnext verdicts apply` of a file is trusted as much as the file.

[Privacy](../privacy.md) lists what is sent to which service, the AI included.
