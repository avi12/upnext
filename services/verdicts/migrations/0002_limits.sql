-- The limits made to hold under parallel requests (`docs/VERDICTS.md`, "Limits").

-- Each counter carries its cap, and a bump past it fails its CHECK, which
-- aborts the whole batch that bump is in, the verdict or report with it.
-- Counters only ever hold the current day's counts, so dropping them is
-- safe.
DROP TABLE counters;
CREATE TABLE counters (
  name TEXT NOT NULL,
  day TEXT NOT NULL,
  n INTEGER NOT NULL,
  cap INTEGER NOT NULL,
  PRIMARY KEY (name, day),
  CHECK (n <= cap)
);

-- One live or held verdict per key per game, as the plan's unique
-- (key_hash, steam_appid, gog_id) means it: withdrawn and removed verdicts
-- stay as rows, and a NULL never counts as a duplicate in a plain unique
-- index, so these two partial indexes say it instead. A game is its appid,
-- or its GOG id when it has none.
CREATE UNIQUE INDEX verdicts_one_per_steam_game ON verdicts (key_hash, steam_appid)
  WHERE state IN ('live', 'held') AND steam_appid IS NOT NULL;
CREATE UNIQUE INDEX verdicts_one_per_gog_game ON verdicts (key_hash, gog_id)
  WHERE state IN ('live', 'held') AND steam_appid IS NULL;
