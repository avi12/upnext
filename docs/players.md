# Several people, one computer

A partner, a sibling or a child who plays on the same computer can have a player of their own. Add one in the setup wizard's "Who plays here?" (which also asks your own name, offering your Steam name or this computer's user name), in Settings › Players, with "Add a player" in the picker, or with `upnext player add Sam`.

Each player has their own picks and History, marks and reasons, Play later, ratings, routines, wishlist and taste profile. The games, their details and their summaries are shared and fetched once. With one player nothing changes. With two or more, UpNext asks "Who's playing?" (each time it opens in the app window, once per browser elsewhere), the top bar switches players in one click, in big screen too, and a new player gets a short setup of their own the first time they're chosen: their name and colour, the games and AI offered to them, a couple of questions about what they love, and the tour.

## Offering and choosing accounts

A store account (a Steam sign-in, GOG, a Heroic folder, a Galaxy database) and an AI account (a key, a subscription, a local model) belong to the player who links them. The owner offers each one to everyone, to chosen players or to nobody else, in Settings › Sharing. Everyone is the default, so a new player sees the library at once. Each player turns on the offered accounts they want.

Games are shared, not hours: an account's hours count for its owner and for the players the owner ticks as also playing on it. A shared AI account's key is used for the others and never shown to them, and Settings › AI usage shows its owner what each player's picks cost.

## PINs

A player can set a PIN of four to eight digits in Settings › Players. The picker asks for it before switching to them, their Hidden by me list asks again, and so does making a token or adding a device for them. A PIN keeps the household out of each other's players; it doesn't hide anything from someone who can open this computer's files. A forgotten PIN is cleared on the computer UpNext runs on with `upnext player pin Sam --clear`.

## Phones and scripts

With two or more players, allowing a phone or making a pairing code asks whose device it is, or "Everyone's" for a tablet on the coffee table. A device that is one player's always acts for them: it can't switch players or change what is everyone's. `upnext token create --player Sam` binds a script's token the same way, and `--player` (or `UPNEXT_PLAYER`) runs `pick`, `prompt`, `mark`, `list`, `stats` and the rest for one player.

## Hiding games

"Hide this game", in a game's "Mark as…" menu, keeps a game out of sight:

- **Just from me** takes it out of your own library, search and picks. Nobody else is affected.
- **From everyone else** keeps your copy out of every other player's UpNext: their library, search, picks and statistics, unless they own it themselves.
- **From everyone** does both.

Hiding isn't the same as Not for me: it tells UpNext nothing about your taste. Everything you hid is on the Lists page under Hidden by me, where you can unhide it; with a PIN, it asks for it again. It's hidden in UpNext only, so anyone who can open this computer's files can still find it. To keep a game out of UpNext completely, hide it in Steam, which works while UpNext reads the Steam client on this computer ([Stores](stores.md#steam)).

## Removing a player

Removing a player (Settings › Players, or `upnext player remove`) works only on the computer UpNext runs on, and takes a backup first.

[Players, PINs and your data](security/data.md) says what players, PINs and hiding protect against, and what they don't.
