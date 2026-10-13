# How it works

1. **Sync** brings in games keyed by store and store id. Steam brings playtime; Heroic doesn't track it, so its games count as untouched until you mark them. A Galaxy import fills playtime for anything Galaxy has seen, and the Epic sign-in for Epic games.
2. **Titles** group a game's copies across stores, by Steam app id, then exact name, then name without edition words. Remasters and editions merge, and manual merges and splits persist.
3. **Fetch details** matches each title to the Steam store, then fetches store details, review scores, SteamSpy tags, a few whole player reviews, GOG's tags and reviews, Wikidata cross-references, PCGamingWiki's genres and series, ProtonDB tiers on Linux, and with your keys IGDB and RAWG. Each source is cached, resumable and rate-limited (the Steam store to about one request every 1.6 seconds).
4. **The AI's summaries** (optional) read each game's player reviews and write "People say" and "Vibes", with review tags from a fixed vocabulary: difficulty, pace, session length, moods, and what players praise or criticize.
5. **Status**: untouched, sampled (under 2 hours), playing, shelved (has an ending, under 80% through, untouched 60 days), abandoned (shelved for 2 years) or played, plus what you mark yourself. The thresholds are settings.
6. **Picks**: the candidates are your untouched, sampled and shelved games, one second chance at a time, and endless games and staples as a session. They're ranked by reviews, your request's words and time, and your profile, then cut to about 120. The AI picks from them and must answer in a strict format; every id it returns is checked, with one retry.
7. **Playing**: picks link straight into Steam, Heroic, Galaxy, the Epic Games Launcher or Ubisoft Connect.

The AI sits behind one interface (`src/llm/`), so every backend does every AI task. [Where things go](contributing/code-map.md) says where each part lives in the code, and [the HTTP API](API.md) has the details of each step.
