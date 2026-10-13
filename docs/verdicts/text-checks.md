# Text checks

[Backlog verdicts](../VERDICTS.md) · [A verdict](verdict.md) · [The published snapshot](snapshot.md)

`text` and `stop_note` are checked in the app as the person types and again by the service. A text is refused, saying which check it failed, when it is blank, too long, or matches one of these patterns (case-insensitive, the same strings in Rust and JavaScript, which write any other character as a four-digit `\uXXXX` escape so they read the same without JavaScript's `u` flag; the first that matches is the one named). Before the patterns run, the characters that show as nothing and change no words are taken out: the soft hyphen, the combining grapheme joiner, the Mongolian vowel separator, the zero-width space, non-joiner and joiner, the left-to-right and right-to-left marks (and Arabic's), the word joiner and invisible operators, and the byte order mark (`text_ignored` in the contract, a pattern with the `g` flag). That way "example" with a zero-width space before ".com" is still a link, while the joiners emoji, Persian and Indic scripts need stay in the text that's sent.

| Code | Refuses |
|---|---|
| `control` | Invisible control characters, C0 and C1 (new lines, tabs and carriage returns are fine), and the characters that reorder text as it's shown (bidirectional embeddings, overrides and isolates, U+202A to U+202E and U+2066 to U+2069), which could make a reversed address read the right way round |
| `wide` | Full-width letters, digits, `＠`, `．` and `／`, which read like the ordinary ones but would slip past the checks below; the full-width `：`, `！`, `？` and brackets Chinese and Japanese text uses are fine |
| `steam_profile` | `steamcommunity`, `s.team/` and `steam://` links |
| `email` | Email addresses |
| `link` | Anything with `://`, `www.`, or a word ending in a common domain (`.com`, `.net`, `.org`, `.io`, `.gg`, `.tv` and others) |
| `handle` | `@handles` |
| `long_number` | Any run of 17 or more digits, which could be a Steam ID |
| `phone` | Nine or more digits with at most two spaces, dots, dashes or brackets between each |

The patterns are in [`verdicts-contract.json`](../verdicts-contract.json) under `text_checks`, and `tests/fixtures/verdicts/text-checks.json` is a list of texts with the check each should fail (or none), read by UpNext's tests and the service's. The checks are a guard against accidents, not moderation: a game called "Agar.io" in a text is refused as a link, and the person is told why, and so is Japanese typed with full-width digits. Look-alike letters from other alphabets aren't caught; reports and the operator are the guard there. An install applies the same checks to every text it downloads and refuses a snapshot with a text that fails one, since other players read them.
