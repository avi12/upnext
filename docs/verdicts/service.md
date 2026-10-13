# How the service decides

[Backlog verdicts](../VERDICTS.md) › How the service decides. The service in `services/verdicts/` ([its README](../../services/verdicts/README.md)) settles these details the contract leaves open, and its tests hold them.

- The client address is the `CF-Connecting-IP` header, counted only as an HMAC under a random salt made for the day and kept in D1. The next daily run deletes the counters and the salt, so there's no secret to rotate and no way to trace a counter back afterwards.
- A game's new verdicts are held once it gets more than 20 in a day, or, when it had verdicts in the 28 days before, at least 5 in a day and more than ten times its daily average. A game with no history has only the cap of 20.
- Sends and reports count against the per-address limit. Withdrawing never does, and a blocked key can still withdraw.
- A report needs a verdict a snapshot has published. Replacing a verdict with the same text and stopping note keeps a held or hidden text as it was, with its reports; a changed text is shown, and the reports about the old one are cleared.
- Every limit is a counter that carries its cap, bumped in the same batch as the write it counts, so parallel requests can't pass a limit together. A key's row is written with its first verdict, so refused requests leave nothing. One key has at most one live or held verdict a game (`migrations/0002_limits.sql`).
- A text or stopping note that fails today's text checks is left out of the snapshot, with its verdict's answers kept, so a stricter check can't make UpNext refuse the whole file.
- The Worker serves the snapshot from R2 at the endpoint's own address, so UpNext's one `verdicts_url` covers both and the bucket needs no public access.
