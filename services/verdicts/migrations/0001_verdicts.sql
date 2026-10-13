-- The service's tables (`docs/verdicts/service.md`). Days are UTC
-- dates (YYYY-MM-DD); nothing here holds a time finer than a day, an IP
-- address, or anything that names a person.

-- One row per verdict ever sent, so an id is never used twice. A withdrawn
-- or removed verdict keeps only its id, game, key hash and days once the
-- daily run has cleared its answers and text.
CREATE TABLE verdicts (
  id TEXT PRIMARY KEY,
  key_hash TEXT NOT NULL,
  steam_appid INTEGER,
  gog_id TEXT,
  outcome TEXT,
  worth TEXT,
  hours TEXT,
  clicked TEXT,
  sessions TEXT,
  limited_time TEXT,
  factors TEXT,
  stop_point TEXT,
  stop_note TEXT,
  stopped_for TEXT,
  text TEXT,
  -- shown, held (reported by three keys, waiting for a moderator) or hidden
  -- (by a moderator).
  text_state TEXT NOT NULL DEFAULT 'shown',
  -- live, held (its game was being brigaded), withdrawn or removed.
  state TEXT NOT NULL DEFAULT 'live',
  vocab INTEGER,
  licence TEXT,
  consent INTEGER,
  client_version TEXT,
  created_day TEXT NOT NULL,
  updated_day TEXT NOT NULL,
  -- The day of the last snapshot that published it, so only published
  -- verdicts can be reported.
  published_day TEXT,
  CHECK (steam_appid IS NOT NULL OR gog_id IS NOT NULL)
);
CREATE INDEX verdicts_key ON verdicts (key_hash, state);
CREATE INDEX verdicts_steam ON verdicts (steam_appid, created_day);
CREATE INDEX verdicts_gog ON verdicts (gog_id, created_day);
CREATE INDEX verdicts_state ON verdicts (state, updated_day);

-- A key is known only by the SHA-256 of the whole key.
CREATE TABLE keys (
  key_hash TEXT PRIMARY KEY,
  first_day TEXT NOT NULL,
  blocked INTEGER NOT NULL DEFAULT 0
);

-- One report per key per verdict, so a key's reports count once.
CREATE TABLE reports (
  verdict_id TEXT NOT NULL,
  key_hash TEXT NOT NULL,
  reason TEXT NOT NULL,
  day TEXT NOT NULL,
  PRIMARY KEY (verdict_id, key_hash)
);

CREATE TABLE moderation_log (
  at TEXT NOT NULL,
  moderator TEXT NOT NULL,
  action TEXT NOT NULL,
  verdict_id TEXT,
  key_hash TEXT,
  note TEXT
);
CREATE INDEX moderation_log_at ON moderation_log (at);

-- The day's counters for the rate limits, by name: a key's new verdicts,
-- replacements and reports, and a client address's writes. The address is
-- in a name only as an HMAC under the day's salt. The daily run deletes
-- every counter from before its day.
CREATE TABLE counters (
  name TEXT NOT NULL,
  day TEXT NOT NULL,
  n INTEGER NOT NULL,
  PRIMARY KEY (name, day)
);

-- A random salt per day for the addresses' HMACs. The daily run deletes
-- the salts of earlier days, so nobody, the operator included, can tell
-- afterwards which address a counter was.
CREATE TABLE salts (
  day TEXT PRIMARY KEY,
  salt TEXT NOT NULL
);
