-- UpNext schema v36, made by this build's schema (schema only, no rows).
CREATE TABLE players (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    colour     TEXT    NOT NULL,
    pin_hash   TEXT,
    created_at INTEGER NOT NULL
);
CREATE TABLE player_settings (
    player_id INTEGER NOT NULL REFERENCES players(id),
    key       TEXT    NOT NULL,
    value     TEXT    NOT NULL,
    PRIMARY KEY (player_id, key)
);
CREATE TABLE store_connections (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    kind       TEXT    NOT NULL,
    owner_id   INTEGER NOT NULL REFERENCES players(id),
    settings   TEXT    NOT NULL DEFAULT '{}',
    secret     TEXT,
    last_sync  INTEGER,
    count      INTEGER,
    created_at INTEGER NOT NULL
);
CREATE TABLE store_accounts (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    connection_id INTEGER NOT NULL REFERENCES store_connections(id),
    store         TEXT    NOT NULL,
    external_id   TEXT,
    name          TEXT    NOT NULL,
    sharing       TEXT    NOT NULL DEFAULT 'everyone' CHECK (sharing IN ('everyone','chosen','owner')),
    created_at    INTEGER NOT NULL
);
CREATE INDEX store_accounts_connection ON store_accounts(connection_id);
CREATE INDEX store_connections_owner ON store_connections(owner_id, kind);
CREATE TABLE store_account_players (
    account_id INTEGER NOT NULL REFERENCES store_accounts(id),
    player_id  INTEGER NOT NULL REFERENCES players(id),
    offered    INTEGER NOT NULL DEFAULT 0,
    uses       INTEGER NOT NULL DEFAULT 1,
    plays_on   INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (account_id, player_id)
);
CREATE TABLE ai_accounts (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    owner_id   INTEGER NOT NULL REFERENCES players(id),
    backend    TEXT,
    settings   TEXT    NOT NULL DEFAULT '{}',
    secret     TEXT,
    sharing    TEXT    NOT NULL DEFAULT 'everyone' CHECK (sharing IN ('everyone','chosen','owner')),
    created_at INTEGER NOT NULL
);
CREATE TABLE ai_account_players (
    account_id INTEGER NOT NULL REFERENCES ai_accounts(id),
    player_id  INTEGER NOT NULL REFERENCES players(id),
    PRIMARY KEY (account_id, player_id)
);
CREATE TABLE hidden_titles (
    player_id  INTEGER NOT NULL REFERENCES players(id),
    title_id   INTEGER NOT NULL REFERENCES titles(id),
    scope      TEXT    NOT NULL CHECK (scope IN ('me','others','everyone')),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (player_id, title_id)
);
CREATE TABLE titles (
    id     INTEGER PRIMARY KEY AUTOINCREMENT,
    key    TEXT    NOT NULL UNIQUE,
    name   TEXT    NOT NULL,
    appid  INTEGER,
    -- The "This game has no ending" switch (1 no ending, 0 has one,
    -- NULL to infer it, lifecycle::infer_kind).
    no_ending INTEGER
);
CREATE INDEX titles_appid ON titles(appid);
CREATE TABLE games (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    title_id          INTEGER REFERENCES titles(id),
    account_id        INTEGER NOT NULL REFERENCES store_accounts(id),
    source            TEXT    NOT NULL,
    source_id         TEXT    NOT NULL,
    launcher          TEXT    NOT NULL,
    name              TEXT    NOT NULL,
    appid             INTEGER,
    steam_matched     INTEGER NOT NULL DEFAULT 0,
    installed         INTEGER NOT NULL DEFAULT 0,
    playtime_forever  INTEGER NOT NULL DEFAULT 0,
    playtime_known    INTEGER NOT NULL DEFAULT 0,
    playtime_2weeks   INTEGER NOT NULL DEFAULT 0,
    rtime_last_played INTEGER NOT NULL DEFAULT 0,
    art_cover         TEXT,
    art_square        TEXT,
    l_description     TEXT,
    l_genres          TEXT,
    l_developer       TEXT,
    l_release         TEXT,
    store_url         TEXT,
    run_url           TEXT,
    edition           TEXT,
    first_seen_at     INTEGER NOT NULL,
    last_synced_at    INTEGER NOT NULL,
    removed           INTEGER NOT NULL DEFAULT 0,
    UNIQUE(account_id, source_id)
);
CREATE INDEX games_title ON games(title_id);
CREATE INDEX games_account ON games(account_id);
CREATE TABLE merge_rules (
    game_id      INTEGER PRIMARY KEY REFERENCES games(id),
    with_game_id INTEGER NOT NULL REFERENCES games(id),
    created_at   INTEGER NOT NULL
);
CREATE TABLE split_rules (
    game_id    INTEGER PRIMARY KEY REFERENCES games(id),
    created_at INTEGER NOT NULL
);
CREATE TABLE merge_snapshots (
    game_id    INTEGER PRIMARY KEY REFERENCES games(id),
    title_name TEXT    NOT NULL,
    no_ending  INTEGER,
    merged_at  INTEGER NOT NULL
);
CREATE TABLE merge_snapshot_marks (
    game_id       INTEGER NOT NULL REFERENCES merge_snapshots(game_id),
    player_id     INTEGER NOT NULL REFERENCES players(id),
    status        TEXT,
    note          TEXT,
    updated_at    INTEGER,
    stars         INTEGER,
    rated_at      INTEGER,
    play_later_at INTEGER,
    hidden_scope  TEXT,
    hidden_at     INTEGER,
    PRIMARY KEY (game_id, player_id)
);
CREATE TABLE steam_match (
    title_id     INTEGER PRIMARY KEY REFERENCES titles(id),
    fetched_at   INTEGER NOT NULL,
    success      INTEGER NOT NULL,
    appid        INTEGER,
    matched_name TEXT,
    similarity   REAL
);
CREATE TABLE details (
    appid             INTEGER PRIMARY KEY,
    fetched_at        INTEGER NOT NULL,
    success           INTEGER NOT NULL,
    type              TEXT,
    short_description TEXT,
    genres            TEXT,
    categories        TEXT,
    header_image      TEXT,
    metacritic        INTEGER,
    release_date      TEXT,
    developers        TEXT,
    publishers        TEXT,
    is_free           INTEGER,
    controller_support TEXT,
    platforms         TEXT
);
CREATE TABLE reviews (
    appid             INTEGER PRIMARY KEY,
    fetched_at        INTEGER NOT NULL,
    success           INTEGER NOT NULL,
    review_score      INTEGER,
    review_score_desc TEXT,
    total_positive    INTEGER,
    total_negative    INTEGER,
    total_reviews     INTEGER
);
CREATE TABLE tags (
    appid      INTEGER PRIMARY KEY,
    fetched_at INTEGER NOT NULL,
    success    INTEGER NOT NULL,
    tags       TEXT
);
CREATE TABLE review_texts (
    appid      INTEGER PRIMARY KEY,
    fetched_at INTEGER NOT NULL,
    success    INTEGER NOT NULL,
    items      TEXT
);
CREATE TABLE gog_meta (
    gog_id       TEXT PRIMARY KEY,
    fetched_at   INTEGER NOT NULL,
    success      INTEGER NOT NULL,
    tags         TEXT,
    properties   TEXT,
    features     TEXT,
    description  TEXT,
    store_url    TEXT
);
CREATE TABLE gog_reviews (
    gog_id       TEXT PRIMARY KEY,
    fetched_at   INTEGER NOT NULL,
    success      INTEGER NOT NULL,
    avg_rating   REAL,
    rating_count INTEGER,
    items        TEXT
);
CREATE TABLE igdb_meta (
    title_id     INTEGER PRIMARY KEY REFERENCES titles(id),
    fetched_at   INTEGER NOT NULL,
    success      INTEGER NOT NULL,
    igdb_id      INTEGER,
    name         TEXT,
    similarity   REAL,
    genres       TEXT,
    themes       TEXT,
    keywords     TEXT,
    perspectives TEXT,
    modes        TEXT,
    summary      TEXT,
    rating       REAL,
    rating_count INTEGER
);
CREATE TABLE xref (
    title_id     INTEGER PRIMARY KEY REFERENCES titles(id),
    fetched_at   INTEGER NOT NULL,
    success      INTEGER NOT NULL,
    qid          TEXT,
    pcgw_page    TEXT,
    rawg_slug    TEXT,
    hltb_id      INTEGER,
    igdb_slug    TEXT,
    epic_slug    TEXT
);
CREATE TABLE rawg_meta (
    title_id      INTEGER PRIMARY KEY REFERENCES titles(id),
    fetched_at    INTEGER NOT NULL,
    success       INTEGER NOT NULL,
    rawg_id       INTEGER,
    slug          TEXT,
    name          TEXT,
    similarity    REAL,
    genres        TEXT,
    tags          TEXT,
    rating        REAL,
    ratings_count INTEGER,
    metacritic    INTEGER,
    developers    TEXT
);
CREATE TABLE pcgw_meta (
    title_id     INTEGER PRIMARY KEY REFERENCES titles(id),
    fetched_at   INTEGER NOT NULL,
    success      INTEGER NOT NULL,
    page         TEXT,
    genres       TEXT,
    themes       TEXT,
    perspectives TEXT,
    pacing       TEXT,
    modes        TEXT,
    controls     TEXT,
    art_styles   TEXT,
    series       TEXT
);
CREATE TABLE protondb (
    appid         INTEGER PRIMARY KEY,
    fetched_at    INTEGER NOT NULL,
    success       INTEGER NOT NULL,
    tier          TEXT,
    trending_tier TEXT,
    confidence    TEXT,
    score         REAL,
    total         INTEGER
);
CREATE TABLE title_series (
    title_id    INTEGER PRIMARY KEY REFERENCES titles(id),
    fetched_at  INTEGER NOT NULL,
    series      TEXT,
    first_year  INTEGER,
    series_qids TEXT
);
CREATE TABLE series_keys (
    series_key  TEXT PRIMARY KEY,
    fetched_at  INTEGER NOT NULL,
    qid         TEXT
);
CREATE TABLE series_catalog (
    series_qid  TEXT PRIMARY KEY,
    fetched_at  INTEGER NOT NULL,
    entries     TEXT NOT NULL
);
CREATE TABLE steam_art (
    appid       INTEGER PRIMARY KEY,
    fetched_at  INTEGER NOT NULL,
    portrait    TEXT,
    hero        TEXT,
    screenshots TEXT
);
CREATE TABLE play_later (
    player_id INTEGER NOT NULL REFERENCES players(id),
    title_id  INTEGER NOT NULL REFERENCES titles(id),
    added_at  INTEGER NOT NULL,
    PRIMARY KEY (player_id, title_id)
);
CREATE TABLE trailers (
    title_id    INTEGER PRIMARY KEY REFERENCES titles(id),
    fetched_at  INTEGER NOT NULL,
    youtube_id  TEXT,
    source      TEXT,
    first_year  INTEGER,
    screenshots TEXT
);
CREATE TABLE series_guides (
    series_key   TEXT PRIMARY KEY,
    generated_at INTEGER NOT NULL,
    model        TEXT,
    members_hash TEXT NOT NULL,
    start_id     INTEGER,
    order_json   TEXT,
    side_json    TEXT,
    note         TEXT
);
CREATE TABLE digests (
    title_id     INTEGER PRIMARY KEY REFERENCES titles(id),
    generated_at INTEGER NOT NULL,
    model        TEXT,
    digest       TEXT NOT NULL,
    tags         TEXT,
    prompt_version INTEGER,
    vibes        TEXT,
    -- A summary from UpNext's shared data: the date of the
    -- seed it came from and when it was applied. NULL for one this install
    -- wrote itself.
    seed_date       TEXT,
    seed_applied_at INTEGER
);
CREATE TABLE seed_applies (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    applied_at INTEGER NOT NULL,
    seed_date  TEXT NOT NULL,
    source     TEXT NOT NULL,
    entries    INTEGER NOT NULL,
    matched    INTEGER NOT NULL,
    written    INTEGER NOT NULL,
    kept_own   INTEGER NOT NULL
);
CREATE TABLE hltb (
    title_id      INTEGER PRIMARY KEY REFERENCES titles(id),
    fetched_at    INTEGER NOT NULL,
    success       INTEGER NOT NULL,
    hltb_id       INTEGER,
    hltb_name     TEXT,
    similarity    REAL,
    main_story    REAL,
    main_extra    REAL,
    completionist REAL
);
CREATE TABLE overrides (
    player_id  INTEGER NOT NULL REFERENCES players(id),
    title_id   INTEGER NOT NULL REFERENCES titles(id),
    status     TEXT NOT NULL CHECK (status IN ('shelved','done','abandoned','hidden','still_playing','burnt_out')),
    note       TEXT,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (player_id, title_id)
);
CREATE TABLE stop_reasons (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    title_id INTEGER NOT NULL REFERENCES titles(id),
    reasons  TEXT    NOT NULL,
    note     TEXT,
    source   TEXT    NOT NULL,
    at       INTEGER NOT NULL,
    player_id INTEGER NOT NULL DEFAULT 1 REFERENCES players(id)
);
CREATE INDEX stop_reasons_title ON stop_reasons(title_id, at);
CREATE INDEX stop_reasons_player ON stop_reasons(player_id, title_id, at);
CREATE TABLE pick_feedback (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_id TEXT    NOT NULL REFERENCES pick_batches(id),
    title_id INTEGER NOT NULL,
    action   TEXT    NOT NULL,
    text     TEXT,
    at       INTEGER NOT NULL
);
CREATE INDEX pick_feedback_title ON pick_feedback(title_id);
CREATE TABLE pick_ratings (
    batch_id TEXT    NOT NULL REFERENCES pick_batches(id),
    title_id INTEGER NOT NULL,
    verdict  TEXT    NOT NULL CHECK (verdict IN ('good','not_a_fit')),
    reason   TEXT,
    at       INTEGER NOT NULL,
    PRIMARY KEY (batch_id, title_id)
);
CREATE INDEX pick_ratings_title ON pick_ratings(title_id);
CREATE TABLE routines (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT    NOT NULL,
    days       TEXT    NOT NULL DEFAULT '[]',
    times      TEXT    NOT NULL DEFAULT '[]',
    who        TEXT,
    place      TEXT,
    input      TEXT,
    minutes    INTEGER,
    note       TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    -- Whose routine it is.
    player_id  INTEGER NOT NULL DEFAULT 1 REFERENCES players(id)
);
CREATE INDEX routines_player ON routines(player_id);
CREATE TABLE game_ratings (
    player_id INTEGER NOT NULL REFERENCES players(id),
    title_id  INTEGER NOT NULL REFERENCES titles(id),
    stars     INTEGER NOT NULL CHECK (stars BETWEEN 1 AND 5),
    at        INTEGER NOT NULL,
    PRIMARY KEY (player_id, title_id)
);
CREATE TABLE my_verdicts (
    player_id    INTEGER NOT NULL REFERENCES players(id),
    title_id     INTEGER NOT NULL REFERENCES titles(id),
    steam_appid  INTEGER,
    gog_id       TEXT,
    outcome      TEXT,
    worth        TEXT,
    hours        TEXT,
    clicked      TEXT,
    sessions     TEXT,
    limited_time TEXT,
    factors      TEXT    NOT NULL DEFAULT '[]',
    stop_point   TEXT,
    stop_note    TEXT,
    stopped_for  TEXT    NOT NULL DEFAULT '[]',
    text         TEXT,
    vocab        INTEGER NOT NULL,
    licence      TEXT    NOT NULL,
    consent      INTEGER NOT NULL,
    remote_id    TEXT,
    state        TEXT    NOT NULL CHECK (state IN ('draft','queued','sent','held','withdrawn','removed')),
    retry_at     INTEGER,
    error        TEXT,
    sent_at      INTEGER,
    updated_at   INTEGER NOT NULL,
    PRIMARY KEY (player_id, title_id)
);
CREATE TABLE community_verdicts (
    id          INTEGER PRIMARY KEY,
    steam_appid INTEGER,
    gog_id      TEXT,
    n           INTEGER NOT NULL,
    counts      TEXT    NOT NULL,
    texts       TEXT    NOT NULL DEFAULT '[]'
);
CREATE INDEX community_verdicts_appid ON community_verdicts(steam_appid);
CREATE INDEX community_verdicts_gog ON community_verdicts(gog_id);
CREATE TABLE second_chances (
    player_id   INTEGER NOT NULL REFERENCES players(id),
    title_id    INTEGER NOT NULL REFERENCES titles(id),
    now         TEXT,
    verdict     TEXT,
    line        TEXT,
    inputs      TEXT,
    written_at  INTEGER,
    model       TEXT,
    answer      TEXT,
    answered_at INTEGER,
    prev_status TEXT,
    prev_at     INTEGER,
    PRIMARY KEY (player_id, title_id)
);
CREATE TABLE profile_statements (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    kind       TEXT    NOT NULL CHECK (kind IN ('favourite','like','dislike','hard_no','context','pattern')),
    title_id   INTEGER REFERENCES titles(id),
    about      TEXT,
    why        TEXT,
    note       TEXT,
    source     TEXT    NOT NULL CHECK (source IN ('stated','derived')),
    evidence   TEXT,
    confidence REAL,
    hidden     INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    player_id  INTEGER NOT NULL DEFAULT 1 REFERENCES players(id)
);
CREATE UNIQUE INDEX profile_favourites ON profile_statements(player_id, title_id) WHERE kind = 'favourite';
CREATE UNIQUE INDEX profile_about ON profile_statements(player_id, kind = 'context', kind = 'pattern', about) WHERE kind <> 'favourite';
CREATE TABLE play_diary (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    hour     INTEGER NOT NULL,
    at       INTEGER NOT NULL,
    title_id INTEGER REFERENCES titles(id),
    appid    INTEGER,
    name     TEXT    NOT NULL,
    minutes  INTEGER NOT NULL DEFAULT 0,
    source   TEXT    NOT NULL CHECK (source IN ('steam','launch')),
    player_id  INTEGER REFERENCES players(id),
    account_id INTEGER REFERENCES store_accounts(id)
);
CREATE INDEX play_diary_hour ON play_diary(hour);
CREATE INDEX play_diary_player ON play_diary(player_id, hour);
CREATE TABLE api_tokens (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    name         TEXT    NOT NULL,
    scope        TEXT    NOT NULL CHECK (scope IN ('read','full')),
    kind         TEXT    NOT NULL DEFAULT 'token' CHECK (kind IN ('token','device')),
    hash         TEXT    NOT NULL UNIQUE,
    hint         TEXT    NOT NULL,
    created_at   INTEGER NOT NULL,
    last_used_at INTEGER,
    player_id    INTEGER REFERENCES players(id)
);
CREATE TABLE hidden_gems (
    title_id       INTEGER PRIMARY KEY REFERENCES titles(id),
    studio         INTEGER NOT NULL,
    mechanics      INTEGER NOT NULL,
    play           INTEGER NOT NULL,
    party          INTEGER NOT NULL DEFAULT 0,
    line           TEXT    NOT NULL,
    prompt_version INTEGER NOT NULL,
    model          TEXT,
    written_at     INTEGER NOT NULL
);
CREATE TABLE wishlist (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    player_id     INTEGER NOT NULL REFERENCES players(id),
    appid         INTEGER NOT NULL,
    name          TEXT    NOT NULL,
    source        TEXT    NOT NULL CHECK (source IN ('collection','pick','steam','manual')),
    reason        TEXT,
    state         TEXT    NOT NULL CHECK (state IN ('suggested','wishlisted','dismissed','owned_elsewhere','in_library')),
    on_steam      INTEGER NOT NULL DEFAULT 0,
    series_key    TEXT,
    batch_id      TEXT,
    title_id      INTEGER,
    hltb_main     REAL,
    hltb_at       INTEGER,
    added_at      INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    wishlisted_at INTEGER,
    UNIQUE(player_id, appid)
);
CREATE INDEX wishlist_title ON wishlist(title_id);
CREATE TABLE pick_batches (
    id          TEXT PRIMARY KEY,
    created_at  INTEGER NOT NULL,
    prompt      TEXT NOT NULL,
    model       TEXT,
    duration_ms INTEGER,
    cost_usd    REAL,
    candidates  INTEGER,
    attempts    INTEGER,
    -- How many picks the lineup shows; the rows past them are spares.
    shown       INTEGER,
    -- Picked without AI: what UpNext's ranking went by (JSON list of
    -- plain sentences). NULL when the AI picked.
    ranked_by   TEXT,
    -- A discovery mode: new_to_you or gems. NULL for a
    -- lineup from the whole library.
    mode        TEXT,
    -- A game you don't own that the AI suggested with the lineup:
    -- its wishlist row, and the AI's reason for this request.
    suggestion_id     INTEGER,
    suggestion_reason TEXT,
    -- The routine it was picked for, as it was then: JSON, a
    -- taste::routine::Routine. NULL when none was chosen.
    routine           TEXT,
    -- Whose request it answered; its picks, feedback and
    -- ratings are theirs through batch_id.
    player_id         INTEGER NOT NULL DEFAULT 1 REFERENCES players(id)
);
CREATE INDEX pick_batches_player ON pick_batches(player_id, created_at);
CREATE TABLE picks (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_id TEXT NOT NULL REFERENCES pick_batches(id),
    position INTEGER NOT NULL,
    title_id INTEGER NOT NULL,
    appid    INTEGER,
    name     TEXT NOT NULL,
    reason   TEXT NOT NULL,
    -- Where in the lineup it shows now; NULL for a spare not used yet
    -- or a pick taken off.
    slot     INTEGER,
    -- A wishlisted game you don't own as a pick: its
    -- wishlist row, with title_id holding minus that id (title_id stays
    -- NOT NULL; no title has a negative id); and the owned game "Switch
    -- with owned game" puts in its place, with its reason. A wishlisted
    -- pick's row with no slot was switched.
    wishlist_id      INTEGER,
    alternate_id     INTEGER,
    alternate_reason TEXT
);
CREATE INDEX picks_batch ON picks(batch_id);
CREATE TABLE source_sync (
    account_id INTEGER PRIMARY KEY REFERENCES store_accounts(id),
    launcher   TEXT    NOT NULL,
    last_sync  INTEGER NOT NULL,
    count      INTEGER NOT NULL
);
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE jobs (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    kind         TEXT    NOT NULL,
    source       TEXT    NOT NULL,
    options      TEXT    NOT NULL,
    status       TEXT    NOT NULL,
    started_at   INTEGER NOT NULL,
    finished_at  INTEGER,
    heartbeat_at INTEGER NOT NULL,
    total        INTEGER NOT NULL DEFAULT 0,
    done         INTEGER NOT NULL DEFAULT 0,
    current      TEXT,
    message      TEXT    NOT NULL DEFAULT '',
    lanes        TEXT,
    per_minute   REAL,
    eta_secs     INTEGER,
    failed       INTEGER NOT NULL DEFAULT 0,
    summary      TEXT,
    error        TEXT,
    cancel       INTEGER NOT NULL DEFAULT 0,
    task         TEXT
);
CREATE INDEX jobs_status ON jobs(status);
CREATE INDEX jobs_task ON jobs(task, status);
CREATE TABLE job_log (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id   INTEGER NOT NULL REFERENCES jobs(id),
    at       INTEGER NOT NULL,
    level    TEXT    NOT NULL,
    lane     TEXT,
    message  TEXT    NOT NULL,
    -- The title a line is about, so players it is hidden from can be shown
    -- "a hidden game" in its place. Not a foreign key: logs
    -- outlive merges.
    title_id INTEGER
);
CREATE INDEX job_log_job ON job_log(job_id, id);
CREATE TABLE job_failures (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id   INTEGER NOT NULL REFERENCES jobs(id),
    at       INTEGER NOT NULL,
    lane     TEXT    NOT NULL,
    source   TEXT    NOT NULL,
    options  TEXT    NOT NULL,
    title_id INTEGER,
    name     TEXT,
    error    TEXT    NOT NULL
);
CREATE INDEX job_failures_job ON job_failures(job_id);
CREATE TABLE ai_calls (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    at                 INTEGER NOT NULL,
    kind               TEXT    NOT NULL,
    backend            TEXT    NOT NULL,
    model              TEXT,
    error              TEXT,
    duration_ms        INTEGER NOT NULL,
    input_tokens       INTEGER,
    output_tokens      INTEGER,
    cache_read_tokens  INTEGER,
    cache_write_tokens INTEGER,
    cost_usd           REAL,
    would_cost_usd     REAL,
    items              INTEGER,
    batch_id           TEXT,
    job_id             INTEGER,
    -- The part of cache_write_tokens kept for an hour, which the API
    -- prices higher than five minutes, and what the call costs at the API's
    -- list prices (prices::PriceList), charged or not.
    cache_write_1h_tokens INTEGER,
    api_usd            REAL,
    -- The AI account that ran it, and the player whose request it answered
    -- (NULL for the library's own jobs). Not foreign keys: calls are kept
    -- after an account or a player is removed.
    ai_account_id      INTEGER,
    player_id          INTEGER
);
CREATE INDEX ai_calls_at ON ai_calls(at);
CREATE INDEX ai_calls_job ON ai_calls(job_id);
CREATE INDEX ai_calls_batch ON ai_calls(batch_id);
CREATE TABLE plan_readings (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    at         INTEGER NOT NULL,
    backend    TEXT    NOT NULL,
    plan       TEXT,
    last_call  INTEGER NOT NULL,
    windows    TEXT    NOT NULL,
    -- The subscription's AI account; not a foreign key, like ai_calls'.
    ai_account_id INTEGER
);
CREATE INDEX plan_readings_at ON plan_readings(at);
PRAGMA user_version=36;
