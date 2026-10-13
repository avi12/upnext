# Shared game data fixtures

`upnext-seed-2026-10-01.jsonl.gz` and its manifest `upnext-seed.json` are a small, made-up example of UpNext's shared game data, in format 1 (see `src/seed.rs`). They were written with Python's `gzip`, `json` and `hashlib`, not by UpNext, so the tests also check that the format can be produced without UpNext's own writer. Every text is invented for the tests ("Fixture text: …"); none comes from a player review.

The five games: Factorio and Firewatch (in the mock library, with their summaries taken out by the test), Firewatch's written with prompt version 3; Hades (the mock library's own summary at the same version wins); Fixture Quest, a GOG-only game matched by its GOG id; and a game no library owns. Firewatch carries a mood outside the vocabulary, which applying drops.
