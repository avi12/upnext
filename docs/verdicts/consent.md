# Consent and keys

[Backlog verdicts](../VERDICTS.md) · [A verdict](verdict.md) · [Text checks](text-checks.md)

Everything published is under **CC0 1.0** (`CC0-1.0`), the licence of UpNext's shared game data. A player sees a consent screen of its own before their first verdict leaves the computer:

> **Your verdict will be public.**
> Everything you send, the answers and anything you type, is published for anyone to read, copy and reuse, for any purpose, with no credit needed. It's released under CC0 1.0, which puts it in the public domain.
> It isn't linked to your name, your Steam or GOG account or your library, and UpNext doesn't publish which verdicts came from the same person. Your hours are sent rounded to a range.
> You can take a verdict down at any time. It's left out of the next daily update and gone from UpNext's servers within a week, but copies people have already downloaded can't be recalled.
> Don't type anything you wouldn't post publicly, such as your name, contact details or someone else's.
>
> [ Publish my verdicts under CC0 ] [ Not now ]

The text is consent version 1. Every verdict carries the licence and the version its sender accepted, the service refuses a verdict without them, and a changed text is a new version that every player accepts again (a test pins the words to the version). The screen links the service's terms at `/terms`.

## Keys

There are no accounts. The first time a player sends a verdict, their install makes a key: `upnext_v_` and 64 lowercase hexadecimal characters (32 random bytes). It's a secret setting of that player, in backups and never in an export without keys. Requests carry it as `Authorization: Bearer upnext_v_…`. The service stores only the SHA-256 (hex) of the whole key, and that hash is the contributor's only identity: enough to edit and withdraw their own verdicts, to rate-limit and to block, and nothing that names anyone. Two players on one computer have two keys. A lost key means its verdicts can't be withdrawn by their writer; the operator can still remove a verdict that's reported.
