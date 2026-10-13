//! The schema, and upgrades from older databases.
//!
//! A new database gets `SCHEMA` and starts at `SCHEMA_VERSION`. UpNext's
//! first release opens databases from `BASELINE_VERSION` on. Development
//! builds before it upgraded older databases one step at a time; the
//! release dropped those steps, so this build refuses an older database
//! rather than guess at it.
//!
//! A schema change bumps `SCHEMA_VERSION`, changes `SCHEMA` for new
//! databases, and adds a step to `UPGRADES` that brings a database at the
//! version before to the same tables (`migration_tests` checks both).

use super::Db;
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};

pub const SCHEMA_VERSION: i64 = 38;

/// The oldest schema this build opens: the one UpNext was first released with.
pub const BASELINE_VERSION: i64 = 29;

const SCHEMA: &str = r#"
-- The people who play here. Every database has at least one; the
-- first owns whatever the database held before there were players.
-- pin_hash is an Argon2id hash, NULL for no PIN. oidc_issuer and
-- oidc_subject (v37): the identity provider's account that signs in as the
-- player, both NULL until one is linked.
CREATE TABLE IF NOT EXISTS players (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    name         TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    colour       TEXT    NOT NULL,
    pin_hash     TEXT,
    created_at   INTEGER NOT NULL,
    oidc_issuer  TEXT,
    oidc_subject TEXT
);
CREATE UNIQUE INDEX IF NOT EXISTS players_oidc ON players(oidc_issuer, oidc_subject) WHERE oidc_subject IS NOT NULL;
-- A player's own settings (db::players::PLAYER_KEYS), and which AI account
-- they use (ai_account).
CREATE TABLE IF NOT EXISTS player_settings (
    player_id INTEGER NOT NULL REFERENCES players(id),
    key       TEXT    NOT NULL,
    value     TEXT    NOT NULL,
    PRIMARY KEY (player_id, key)
);
-- How UpNext reads a player's libraries: kind is the launcher (steam, gog,
-- heroic, galaxy, epic, origin, uplay, manual). settings is JSON (the
-- SteamID, the Heroic folder, the Galaxy database and its skip list, the
-- Epic Games Launcher's manifests); secret is a sign-in (GOG's or Epic's
-- token), never shown. last_sync and count are
-- the connection's whole import, for one that spans several stores.
CREATE TABLE IF NOT EXISTS store_connections (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    kind       TEXT    NOT NULL,
    owner_id   INTEGER NOT NULL REFERENCES players(id),
    settings   TEXT    NOT NULL DEFAULT '{}',
    secret     TEXT,
    last_sync  INTEGER,
    count      INTEGER,
    created_at INTEGER NOT NULL
);
-- One store's library of one player, read through a connection: store is
-- a copy's source (steam, gog, epic, amazon and the rest), external_id the
-- store's own id for the account when it says. sharing is who it is offered
-- to: everyone, chosen players (store_account_players.offered) or only its
-- owner, the connection's.
CREATE TABLE IF NOT EXISTS store_accounts (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    connection_id INTEGER NOT NULL REFERENCES store_connections(id),
    store         TEXT    NOT NULL,
    external_id   TEXT,
    name          TEXT    NOT NULL,
    sharing       TEXT    NOT NULL DEFAULT 'everyone' CHECK (sharing IN ('everyone','chosen','owner')),
    created_at    INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS store_accounts_connection ON store_accounts(connection_id);
CREATE INDEX IF NOT EXISTS store_connections_owner ON store_connections(owner_id, kind);
-- Each other player's part in an account: offered (for chosen sharing),
-- uses (their own switch, on unless they turned it off) and plays_on (the
-- owner's tick: its playtime counts for them too).
CREATE TABLE IF NOT EXISTS store_account_players (
    account_id INTEGER NOT NULL REFERENCES store_accounts(id),
    player_id  INTEGER NOT NULL REFERENCES players(id),
    offered    INTEGER NOT NULL DEFAULT 0,
    uses       INTEGER NOT NULL DEFAULT 1,
    plays_on   INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (account_id, player_id)
);
-- An AI backend with its settings, owned by a player and shared like a
-- store account. backend is the llm_backend setting (NULL: the
-- environment's or the built-in one); settings is JSON of the other AI
-- settings, secret JSON of its keys, never shown.
CREATE TABLE IF NOT EXISTS ai_accounts (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    owner_id   INTEGER NOT NULL REFERENCES players(id),
    backend    TEXT,
    settings   TEXT    NOT NULL DEFAULT '{}',
    secret     TEXT,
    sharing    TEXT    NOT NULL DEFAULT 'everyone' CHECK (sharing IN ('everyone','chosen','owner')),
    created_at INTEGER NOT NULL
);
-- The players an AI account with chosen sharing is offered to.
CREATE TABLE IF NOT EXISTS ai_account_players (
    account_id INTEGER NOT NULL REFERENCES ai_accounts(id),
    player_id  INTEGER NOT NULL REFERENCES players(id),
    PRIMARY KEY (account_id, player_id)
);
-- "Hide this game": me (from this player's own views), others (this
-- player's copies, from every other player) or everyone (both). Not a
-- judgement of the game, unlike the hidden status ("Not for me").
CREATE TABLE IF NOT EXISTS hidden_titles (
    player_id  INTEGER NOT NULL REFERENCES players(id),
    title_id   INTEGER NOT NULL REFERENCES titles(id),
    scope      TEXT    NOT NULL CHECK (scope IN ('me','others','everyone')),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (player_id, title_id)
);
CREATE TABLE IF NOT EXISTS titles (
    id     INTEGER PRIMARY KEY AUTOINCREMENT,
    key    TEXT    NOT NULL UNIQUE,
    name   TEXT    NOT NULL,
    appid  INTEGER,
    -- The "This game has no ending" switch (1 no ending, 0 has one,
    -- NULL to infer it, lifecycle::infer_kind).
    no_ending INTEGER
);
CREATE INDEX IF NOT EXISTS titles_appid ON titles(appid);
-- A copy: one store account's ownership of a title (the account
-- belongs to a player, who may share it).
CREATE TABLE IF NOT EXISTS games (
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
CREATE INDEX IF NOT EXISTS games_title ON games(title_id);
CREATE INDEX IF NOT EXISTS games_account ON games(account_id);
-- Manual grouping decisions; they survive re-syncs and regrouping.
CREATE TABLE IF NOT EXISTS merge_rules (
    game_id      INTEGER PRIMARY KEY REFERENCES games(id),
    with_game_id INTEGER NOT NULL REFERENCES games(id),
    created_at   INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS split_rules (
    game_id    INTEGER PRIMARY KEY REFERENCES games(id),
    created_at INTEGER NOT NULL
);
-- What a merge folded away (v36): a title merged into another leaves its
-- own marks here, on its lowest-id copy, as they were before the merge, so
-- splitting that copy back out gives its new title them again
-- (`Db::split_copy`). title_name is the old title's name, for the record;
-- no_ending its ending switch.
CREATE TABLE IF NOT EXISTS merge_snapshots (
    game_id    INTEGER PRIMARY KEY REFERENCES games(id),
    title_name TEXT    NOT NULL,
    no_ending  INTEGER,
    merged_at  INTEGER NOT NULL
);
-- Each player's marks on that title: the status and note, your rating,
-- Play later and a hide, each NULL when the player had none.
CREATE TABLE IF NOT EXISTS merge_snapshot_marks (
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
CREATE TABLE IF NOT EXISTS steam_match (
    title_id     INTEGER PRIMARY KEY REFERENCES titles(id),
    fetched_at   INTEGER NOT NULL,
    success      INTEGER NOT NULL,
    appid        INTEGER,
    matched_name TEXT,
    similarity   REAL
);
CREATE TABLE IF NOT EXISTS details (
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
CREATE TABLE IF NOT EXISTS reviews (
    appid             INTEGER PRIMARY KEY,
    fetched_at        INTEGER NOT NULL,
    success           INTEGER NOT NULL,
    review_score      INTEGER,
    review_score_desc TEXT,
    total_positive    INTEGER,
    total_negative    INTEGER,
    total_reviews     INTEGER
);
CREATE TABLE IF NOT EXISTS tags (
    appid      INTEGER PRIMARY KEY,
    fetched_at INTEGER NOT NULL,
    success    INTEGER NOT NULL,
    tags       TEXT
);
CREATE TABLE IF NOT EXISTS review_texts (
    appid      INTEGER PRIMARY KEY,
    fetched_at INTEGER NOT NULL,
    success    INTEGER NOT NULL,
    items      TEXT
);
CREATE TABLE IF NOT EXISTS gog_meta (
    gog_id       TEXT PRIMARY KEY,
    fetched_at   INTEGER NOT NULL,
    success      INTEGER NOT NULL,
    tags         TEXT,
    properties   TEXT,
    features     TEXT,
    description  TEXT,
    store_url    TEXT
);
CREATE TABLE IF NOT EXISTS gog_reviews (
    gog_id       TEXT PRIMARY KEY,
    fetched_at   INTEGER NOT NULL,
    success      INTEGER NOT NULL,
    avg_rating   REAL,
    rating_count INTEGER,
    items        TEXT
);
CREATE TABLE IF NOT EXISTS igdb_meta (
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
CREATE TABLE IF NOT EXISTS xref (
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
CREATE TABLE IF NOT EXISTS rawg_meta (
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
CREATE TABLE IF NOT EXISTS pcgw_meta (
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
CREATE TABLE IF NOT EXISTS protondb (
    appid         INTEGER PRIMARY KEY,
    fetched_at    INTEGER NOT NULL,
    success       INTEGER NOT NULL,
    tier          TEXT,
    trending_tier TEXT,
    confidence    TEXT,
    score         REAL,
    total         INTEGER
);
CREATE TABLE IF NOT EXISTS title_series (
    title_id    INTEGER PRIMARY KEY REFERENCES titles(id),
    fetched_at  INTEGER NOT NULL,
    series      TEXT,
    first_year  INTEGER,
    series_qids TEXT
);
CREATE TABLE IF NOT EXISTS series_keys (
    series_key  TEXT PRIMARY KEY,
    fetched_at  INTEGER NOT NULL,
    qid         TEXT
);
CREATE TABLE IF NOT EXISTS series_catalog (
    series_qid  TEXT PRIMARY KEY,
    fetched_at  INTEGER NOT NULL,
    entries     TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS steam_art (
    appid       INTEGER PRIMARY KEY,
    fetched_at  INTEGER NOT NULL,
    portrait    TEXT,
    hero        TEXT,
    screenshots TEXT
);
CREATE TABLE IF NOT EXISTS play_later (
    player_id INTEGER NOT NULL REFERENCES players(id),
    title_id  INTEGER NOT NULL REFERENCES titles(id),
    added_at  INTEGER NOT NULL,
    PRIMARY KEY (player_id, title_id)
);
CREATE TABLE IF NOT EXISTS trailers (
    title_id    INTEGER PRIMARY KEY REFERENCES titles(id),
    fetched_at  INTEGER NOT NULL,
    youtube_id  TEXT,
    source      TEXT,
    first_year  INTEGER,
    screenshots TEXT
);
CREATE TABLE IF NOT EXISTS series_guides (
    series_key   TEXT PRIMARY KEY,
    generated_at INTEGER NOT NULL,
    model        TEXT,
    members_hash TEXT NOT NULL,
    start_id     INTEGER,
    order_json   TEXT,
    side_json    TEXT,
    note         TEXT
);
CREATE TABLE IF NOT EXISTS digests (
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
-- Each time shared game data was applied, and what it did.
CREATE TABLE IF NOT EXISTS seed_applies (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    applied_at INTEGER NOT NULL,
    seed_date  TEXT NOT NULL,
    source     TEXT NOT NULL,
    entries    INTEGER NOT NULL,
    matched    INTEGER NOT NULL,
    written    INTEGER NOT NULL,
    kept_own   INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS hltb (
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
CREATE TABLE IF NOT EXISTS overrides (
    player_id  INTEGER NOT NULL REFERENCES players(id),
    title_id   INTEGER NOT NULL REFERENCES titles(id),
    status     TEXT NOT NULL CHECK (status IN ('shelved','done','abandoned','hidden','still_playing','burnt_out')),
    note       TEXT,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (player_id, title_id)
);
-- Why a started game was put down, every time it was asked.
-- reasons is a JSON list of lifecycle::REASONS codes; source is where they
-- were given (lifecycle::REASON_SOURCES).
CREATE TABLE IF NOT EXISTS stop_reasons (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    title_id INTEGER NOT NULL REFERENCES titles(id),
    reasons  TEXT    NOT NULL,
    note     TEXT,
    source   TEXT    NOT NULL,
    at       INTEGER NOT NULL,
    player_id INTEGER NOT NULL DEFAULT 1 REFERENCES players(id)
);
CREATE INDEX IF NOT EXISTS stop_reasons_title ON stop_reasons(title_id, at);
CREATE INDEX IF NOT EXISTS stop_reasons_player ON stop_reasons(player_id, title_id, at);
-- What was done with a pick on its card, one of
-- db::FeedbackAction: chosen, not_tonight, too (text: the word),
-- same_problem (taken off by the same "too" call), not_for_me, started
-- (text: how it went).
CREATE TABLE IF NOT EXISTS pick_feedback (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_id TEXT    NOT NULL REFERENCES pick_batches(id),
    title_id INTEGER NOT NULL,
    action   TEXT    NOT NULL,
    text     TEXT,
    at       INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS pick_feedback_title ON pick_feedback(title_id);
-- How well a pick fit what its lineup was asked for: good or
-- not_a_fit, with an optional reason for not_a_fit (db::MissReason). One
-- row per pick of a lineup; taking the rating back deletes it. Not an
-- action: the pick stays where it is.
CREATE TABLE IF NOT EXISTS pick_ratings (
    batch_id TEXT    NOT NULL REFERENCES pick_batches(id),
    title_id INTEGER NOT NULL,
    verdict  TEXT    NOT NULL CHECK (verdict IN ('good','not_a_fit')),
    reason   TEXT,
    at       INTEGER NOT NULL,
    PRIMARY KEY (batch_id, title_id)
);
CREATE INDEX IF NOT EXISTS pick_ratings_title ON pick_ratings(title_id);
-- Routines: named sessions on the Profile, chosen on the Pick page.
-- days and times are JSON lists of words (taste::routine::Day, Time);
-- empty lists mean any day or any time. who, place and input are words
-- too (Who, Place, Input), NULL when the routine doesn't say.
CREATE TABLE IF NOT EXISTS routines (
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
CREATE INDEX IF NOT EXISTS routines_player ON routines(player_id);
-- Your own rating of a game, 1 to 5 stars.
CREATE TABLE IF NOT EXISTS game_ratings (
    player_id INTEGER NOT NULL REFERENCES players(id),
    title_id  INTEGER NOT NULL REFERENCES titles(id),
    stars     INTEGER NOT NULL CHECK (stars BETWEEN 1 AND 5),
    at        INTEGER NOT NULL,
    PRIMARY KEY (player_id, title_id)
);
-- Backlog verdicts: each player's own verdict about a game, as
-- they last saved or sent it. The answers are vocab codes; factors and
-- stopped_for are JSON lists. steam_appid and gog_id are the ids it was sent
-- with. remote_id is the service's id once sent. state is draft (saved,
-- not sent), queued (sent when the service takes it, at retry_at), sent,
-- held (the service holds it), withdrawn or removed (by the service's
-- moderators); error is why the last try failed.
CREATE TABLE IF NOT EXISTS my_verdicts (
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
-- Backlog verdicts from other players: the last snapshot's
-- games, replaced whole by each snapshot. A game is matched to the library
-- when the library is read (by Steam appid, then GOG id), so a game added
-- later gets its verdicts without a new download. counts and texts are as
-- published (JSON). The snapshot's date, where it came from and the
-- manifest's ETag are meta rows (verdicts_date, verdicts_source,
-- verdicts_etag, verdicts_applied_at).
CREATE TABLE IF NOT EXISTS community_verdicts (
    id          INTEGER PRIMARY KEY,
    steam_appid INTEGER,
    gog_id      TEXT,
    n           INTEGER NOT NULL,
    counts      TEXT    NOT NULL,
    texts       TEXT    NOT NULL DEFAULT '[]'
);
CREATE INDEX IF NOT EXISTS community_verdicts_appid ON community_verdicts(steam_appid);
CREATE INDEX IF NOT EXISTS community_verdicts_gog ON community_verdicts(gog_id);
-- The Second Chance page, a row per abandoned title it has
-- something for. "Now" is what the AI read in player reviews about why the
-- game was put down (verdict: better, partly, same or unclear; line: its
-- gist for picks); inputs is what it was written from
-- (second_chance::inputs), so new reasons or reviews show it's out of date.
-- answer is the page's: willing (back to Shelved, and one rejection in picks
-- ends it), still_no (out of picks for good) or not_for_me. Willing and
-- not_for_me set a mark; prev_status and prev_at are the mark before it
-- (NULL: none), put back when the answer is taken back.
CREATE TABLE IF NOT EXISTS second_chances (
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
-- The taste profile, one row per thing it says about you.
-- kind: favourite (title_id is the game), like, dislike or hard_no (about
-- is what it's about: "facet:grind:heavy", "tag:Horror", "length:short" or
-- "text:<your words>"), context (about is weeknight, weekend, times, who or
-- where) or pattern (about is its key, "drift:facet:grind:heavy").
-- why is JSON: a favourite's reasons (taste::why_choices keys), or a
-- context's answer (minutes, or a list of choices). source is stated (you
-- said it) or derived (the profile job found it, patterns only); evidence
-- (JSON) and confidence are a pattern's. hidden: you said "Not really".
CREATE TABLE IF NOT EXISTS profile_statements (
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
CREATE UNIQUE INDEX IF NOT EXISTS profile_favourites ON profile_statements(player_id, title_id) WHERE kind = 'favourite';
CREATE UNIQUE INDEX IF NOT EXISTS profile_about ON profile_statements(player_id, kind = 'context', kind = 'pattern', about) WHERE kind <> 'favourite';
-- The play diary, written only while the play_diary setting
-- is on. source steam: minutes a game's Steam playtime grew between two
-- hourly readings, in the hour they were played (hour is the unix time the
-- hour starts); launch: the Play button in UpNext, at the exact time (at),
-- with no minutes. title_id is NULL for a game not in the library yet.
-- player_id is who pressed Play (launch rows); account_id is the Steam
-- account a reading came from (steam rows).
CREATE TABLE IF NOT EXISTS play_diary (
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
CREATE INDEX IF NOT EXISTS play_diary_hour ON play_diary(hour);
CREATE INDEX IF NOT EXISTS play_diary_player ON play_diary(player_id, hour);
-- API tokens, what lets another device or a script use the
-- API. hash is the token's SHA-256 in hex; the token itself is never
-- stored. hint is its first characters, to tell tokens apart. scope is read
-- or full; kind is token (made by hand), device (a paired browser) or oidc
-- (a browser signed in through the identity provider, v37).
-- player_id: the player a token acts for, always; NULL for an
-- unbound one, which acts for whoever the request names.
CREATE TABLE IF NOT EXISTS api_tokens (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    name         TEXT    NOT NULL,
    scope        TEXT    NOT NULL CHECK (scope IN ('read','full')),
    kind         TEXT    NOT NULL DEFAULT 'token' CHECK (kind IN ('token','device','oidc')),
    hash         TEXT    NOT NULL UNIQUE,
    hint         TEXT    NOT NULL,
    created_at   INTEGER NOT NULL,
    last_used_at INTEGER,
    player_id    INTEGER REFERENCES players(id)
);
-- Hidden gems, the AI's ratings of unplayed, highly rated,
-- short games: how little-known the studio is (studio), how rare its
-- mechanics are in the library (mechanics) and how odd and interesting its
-- play is (play), each 0 to 3; party: a party game, kept off the shelf;
-- line: what makes it unusual, in one sentence. prompt_version is
-- discovery::gems::PROMPT_VERSION when it was written.
CREATE TABLE IF NOT EXISTS hidden_gems (
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
-- The recommended wishlist, one row per game you don't own
-- that UpNext suggested, you added, or is on your Steam wishlist, keyed by
-- its Steam appid (games you don't own are fetched into the appid-keyed
-- store tables: details, reviews, tags, steam_art). source: collection,
-- pick, steam or manual. state: suggested, wishlisted, dismissed (Not
-- interested: never suggested again), owned_elsewhere (I own it elsewhere:
-- counts as owned) or in_library (a sync found it in the library; title_id
-- is the title, and wishlisted_at says it was on the wishlist). on_steam:
-- on your Steam wishlist now. series_key or batch_id: the collection or
-- pick batch it came from. hltb_main: HowLongToBeat's main story, since
-- the hltb table is per title; hltb_at: when it was looked up.
CREATE TABLE IF NOT EXISTS wishlist (
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
CREATE INDEX IF NOT EXISTS wishlist_title ON wishlist(title_id);
CREATE TABLE IF NOT EXISTS pick_batches (
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
CREATE INDEX IF NOT EXISTS pick_batches_player ON pick_batches(player_id, created_at);
-- position ranks a batch's games: the lineup first, then its spares, then
-- any spares asked for when those ran out.
CREATE TABLE IF NOT EXISTS picks (
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
CREATE INDEX IF NOT EXISTS picks_batch ON picks(batch_id);
-- Each store account's last sync: the launcher that ran it, and how many
-- copies it reported.
CREATE TABLE IF NOT EXISTS source_sync (
    account_id INTEGER PRIMARY KEY REFERENCES store_accounts(id),
    launcher   TEXT    NOT NULL,
    last_sync  INTEGER NOT NULL,
    count      INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
-- Background jobs (sync, enrich), written by whichever process runs
-- one (the server or the command line) and read by both. A job is running
-- while its heartbeat is fresh; `cancel` asks the process running it to stop.
CREATE TABLE IF NOT EXISTS jobs (
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
CREATE INDEX IF NOT EXISTS jobs_status ON jobs(status);
-- The scheduled task a job ran as (jobs::Task), for each task's last run.
CREATE INDEX IF NOT EXISTS jobs_task ON jobs(task, status);
CREATE TABLE IF NOT EXISTS job_log (
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
CREATE INDEX IF NOT EXISTS job_log_job ON job_log(job_id, id);
-- What failed for which title, so a job's failures can be run again.
CREATE TABLE IF NOT EXISTS job_failures (
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
CREATE INDEX IF NOT EXISTS job_failures_job ON job_failures(job_id);
-- Every AI call, when it came back, with the tokens its backend
-- reported (NULL when it didn't). input_tokens is all of the input; the
-- cache columns are parts of it. Kept for a year, longer than the jobs
-- they ran in, so job_id is not a foreign key.
CREATE TABLE IF NOT EXISTS ai_calls (
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
CREATE INDEX IF NOT EXISTS ai_calls_at ON ai_calls(at);
CREATE INDEX IF NOT EXISTS ai_calls_job ON ai_calls(job_id);
CREATE INDEX IF NOT EXISTS ai_calls_batch ON ai_calls(batch_id);
-- A subscription's usage bars as its CLI reported them, read around
-- UpNext's own AI calls. last_call is the newest ai_calls id recorded when
-- the reading began, so each call falls on one side of it. windows is JSON
-- (ai_plan::Window). Kept for a year, like ai_calls.
CREATE TABLE IF NOT EXISTS plan_readings (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    at         INTEGER NOT NULL,
    backend    TEXT    NOT NULL,
    plan       TEXT,
    last_call  INTEGER NOT NULL,
    windows    TEXT    NOT NULL,
    -- The subscription's AI account; not a foreign key, like ai_calls'.
    ai_account_id INTEGER
);
CREATE INDEX IF NOT EXISTS plan_readings_at ON plan_readings(at);
"#;

/// An upgrade step: it changes a database at the version before it to its
/// version's tables, and moves or repairs rows where it must.
type Step = fn(&Connection) -> Result<()>;

/// The steps from `BASELINE_VERSION` to `SCHEMA_VERSION`, one per version, in
/// order, each with the version it brings a database to.
const UPGRADES: &[(i64, Step)] = &[(30, v30), (31, v31), (32, v32), (33, v33), (34, v34), (35, v35), (36, v36), (37, v37), (38, v38)];

/// v30: ratings of picks and of games.
fn v30(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE pick_ratings (
            batch_id TEXT    NOT NULL REFERENCES pick_batches(id),
            title_id INTEGER NOT NULL,
            verdict  TEXT    NOT NULL CHECK (verdict IN ('good','not_a_fit')),
            reason   TEXT,
            at       INTEGER NOT NULL,
            PRIMARY KEY (batch_id, title_id)
        );
        CREATE INDEX pick_ratings_title ON pick_ratings(title_id);
        CREATE TABLE game_ratings (
            title_id INTEGER PRIMARY KEY REFERENCES titles(id),
            stars    INTEGER NOT NULL CHECK (stars BETWEEN 1 AND 5),
            at       INTEGER NOT NULL
        );",
    )?;
    Ok(())
}

/// v31: routines, and the routine a lineup was picked for.
fn v31(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE routines (
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
            updated_at INTEGER NOT NULL
        );
        ALTER TABLE pick_batches ADD COLUMN routine TEXT;",
    )?;
    Ok(())
}

/// v32: players. Everything already in the database becomes the
/// first player's: player 1 owns a connection for each launcher in use, a
/// store account for each store in the library and one AI account made from
/// the AI settings, every one shared with everyone, and every status, list,
/// lineup, rating and statement. Per-player settings move to player 1's own
/// rows and account settings onto the accounts, so a single-player database
/// reads exactly as before.
fn v32(c: &Connection) -> Result<()> {
    // The settings' homes as they were at v32, kept here so a later change
    // to the lists in `players` can't change what this step did.
    const PLAYER_KEYS: [&str; 10] = [
        "theme",
        "theme_follow_system",
        "sampled_minutes",
        "shelve_after_days",
        "abandon_after_days",
        "staple_hours",
        "burnout_days",
        "pattern_half_life_days",
        "pattern_min_games",
        "wishlist",
    ];
    const CONNECTION_KEYS: [(&str, &str); 4] =
        [("steam_id", "steam"), ("steam_include_hidden", "steam"), ("galaxy_db", "galaxy"), ("galaxy_skip", "galaxy")];
    const AI_SETTINGS: [&str; 12] = [
        "model",
        "digest_model",
        "codex_model",
        "codex_digest_model",
        "codex_effort",
        "openai_key_model",
        "openai_key_digest_model",
        "openai_key_effort",
        "openai_base_url",
        "openai_model",
        "openai_thinking",
        "openai_digest_model",
    ];
    const AI_SECRETS: [&str; 3] = ["anthropic_api_key", "openai_key", "openai_api_key"];
    // Player-request AI calls, which player 1 made: picks, "It's too",
    // Help me decide and Second Chance's "Now".
    const PLAYER_CALLS: &str = "'pick','too','decide','second_chances'";

    let at = super::now();
    c.execute_batch(
        "CREATE TABLE players (
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
        );",
    )?;

    // Player 1, named after the Steam persona when there is one.
    let persona: Option<String> = c
        .query_row("SELECT value FROM meta WHERE key='profile'", [], |r| r.get::<_, String>(0))
        .optional()?
        .and_then(|p| serde_json::from_str::<serde_json::Value>(&p).ok())
        .and_then(|p| p.get("persona_name")?.as_str().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty());
    c.execute(
        "INSERT INTO players(id, name, colour, created_at) VALUES (1, ?1, ?2, ?3)",
        params![persona.as_deref().unwrap_or("Player 1"), super::players::COLOURS[0], at],
    )?;

    let settings: std::collections::HashMap<String, String> = {
        let mut st = c.prepare("SELECT key, value FROM settings")?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
    };
    let json_of = |keys: &mut dyn Iterator<Item = &str>| -> Option<String> {
        let map: serde_json::Map<String, serde_json::Value> =
            keys.filter_map(|k| Some((k.to_string(), serde_json::Value::String(settings.get(k)?.clone())))).collect();
        (!map.is_empty()).then(|| serde_json::Value::Object(map).to_string())
    };

    // A connection for each launcher the library came through, and for the
    // Steam and Galaxy sign-ins set up without a game yet.
    let mut kinds: Vec<String> = {
        let mut st = c.prepare("SELECT launcher FROM games UNION SELECT launcher FROM source_sync WHERE source <> 'galaxy'")?;
        st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
    };
    for (key, kind) in CONNECTION_KEYS {
        if settings.contains_key(key) && !kinds.iter().any(|k| k == kind) {
            kinds.push(kind.to_string());
        }
    }
    let order = |k: &str| ["steam", "gog", "heroic", "galaxy"].iter().position(|x| *x == k).unwrap_or(4);
    kinds.sort_by(|a, b| order(a).cmp(&order(b)).then_with(|| a.cmp(b)));
    let mut connections: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for kind in &kinds {
        let own = json_of(&mut CONNECTION_KEYS.iter().filter(|(_, k)| k == kind).map(|(key, _)| *key));
        c.execute(
            "INSERT INTO store_connections(kind, owner_id, settings, created_at) VALUES (?1, 1, ?2, ?3)",
            params![kind, own.unwrap_or_else(|| "{}".into()), at],
        )?;
        connections.insert(kind.clone(), c.last_insert_rowid());
    }

    // A store account for each store, under the connection most of its
    // copies came through (a store's own launcher first), and for a store
    // synced with nothing in it yet.
    let stores: Vec<(String, String)> = {
        let mut st = c.prepare(
            "SELECT source, launcher FROM (
                 SELECT source, launcher, ROW_NUMBER() OVER (
                     PARTITION BY source ORDER BY launcher IN ('steam','heroic') DESC, COUNT(*) DESC, launcher) AS n
                 FROM games GROUP BY source, launcher)
             WHERE n = 1
             UNION
             SELECT source, launcher FROM source_sync WHERE source <> 'galaxy' AND source NOT IN (SELECT source FROM games)
             ORDER BY source",
        )?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
    };
    for (store, launcher) in &stores {
        let external = (store == "steam").then(|| settings.get("steam_id").cloned()).flatten();
        c.execute(
            "INSERT INTO store_accounts(connection_id, store, external_id, name, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![connections[launcher], store, external, crate::galaxy::platform_label(store), at],
        )?;
    }
    let galaxy_sync: Option<(i64, i64)> =
        c.query_row("SELECT last_sync, count FROM source_sync WHERE source='galaxy'", [], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
    if let (Some(&id), Some((last, count))) = (connections.get("galaxy"), galaxy_sync) {
        c.execute("UPDATE store_connections SET last_sync=?2, count=?3 WHERE id=?1", params![id, last, count])?;
    }

    // Each copy is in its store's account. The table is rebuilt for the new
    // unique key, and source_sync for its new one.
    rebuild(
        c,
        "games",
        "CREATE TABLE games_v32 (
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
        )",
        "INSERT INTO games_v32 (id, title_id, account_id, source, source_id, launcher, name, appid, steam_matched, installed,
             playtime_forever, playtime_known, playtime_2weeks, rtime_last_played, art_cover, art_square, l_description, l_genres,
             l_developer, l_release, store_url, run_url, edition, first_seen_at, last_synced_at, removed)
         SELECT g.id, g.title_id, a.id, g.source, g.source_id, g.launcher, g.name, g.appid, g.steam_matched, g.installed,
             g.playtime_forever, g.playtime_known, g.playtime_2weeks, g.rtime_last_played, g.art_cover, g.art_square,
             g.l_description, g.l_genres, g.l_developer, g.l_release, g.store_url, g.run_url, g.edition, g.first_seen_at,
             g.last_synced_at, g.removed
         FROM games g JOIN store_accounts a ON a.store = g.source",
        &["CREATE INDEX games_title ON games(title_id)", "CREATE INDEX games_account ON games(account_id)"],
    )?;
    rebuild(
        c,
        "source_sync",
        "CREATE TABLE source_sync_v32 (
            account_id INTEGER PRIMARY KEY REFERENCES store_accounts(id),
            launcher   TEXT    NOT NULL,
            last_sync  INTEGER NOT NULL,
            count      INTEGER NOT NULL
        )",
        "INSERT INTO source_sync_v32 (account_id, launcher, last_sync, count)
         SELECT a.id, s.launcher, s.last_sync, s.count FROM source_sync s JOIN store_accounts a ON a.store = s.source",
        &[],
    )?;

    // The tables keyed by title alone become player 1's.
    rebuild(
        c,
        "overrides",
        "CREATE TABLE overrides_v32 (
            player_id  INTEGER NOT NULL REFERENCES players(id),
            title_id   INTEGER NOT NULL REFERENCES titles(id),
            status     TEXT NOT NULL CHECK (status IN ('shelved','done','abandoned','hidden','still_playing','burnt_out')),
            note       TEXT,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY (player_id, title_id)
        )",
        "INSERT INTO overrides_v32 (player_id, title_id, status, note, updated_at) SELECT 1, title_id, status, note, updated_at FROM overrides",
        &[],
    )?;
    rebuild(
        c,
        "play_later",
        "CREATE TABLE play_later_v32 (
            player_id INTEGER NOT NULL REFERENCES players(id),
            title_id  INTEGER NOT NULL REFERENCES titles(id),
            added_at  INTEGER NOT NULL,
            PRIMARY KEY (player_id, title_id)
        )",
        "INSERT INTO play_later_v32 (player_id, title_id, added_at) SELECT 1, title_id, added_at FROM play_later",
        &[],
    )?;
    rebuild(
        c,
        "game_ratings",
        "CREATE TABLE game_ratings_v32 (
            player_id INTEGER NOT NULL REFERENCES players(id),
            title_id  INTEGER NOT NULL REFERENCES titles(id),
            stars     INTEGER NOT NULL CHECK (stars BETWEEN 1 AND 5),
            at        INTEGER NOT NULL,
            PRIMARY KEY (player_id, title_id)
        )",
        "INSERT INTO game_ratings_v32 (player_id, title_id, stars, at) SELECT 1, title_id, stars, at FROM game_ratings",
        &[],
    )?;
    rebuild(
        c,
        "second_chances",
        "CREATE TABLE second_chances_v32 (
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
        )",
        "INSERT INTO second_chances_v32 (player_id, title_id, now, verdict, line, inputs, written_at, model, answer, answered_at,
             prev_status, prev_at)
         SELECT 1, title_id, now, verdict, line, inputs, written_at, model, answer, answered_at, prev_status, prev_at FROM second_chances",
        &[],
    )?;
    rebuild(
        c,
        "wishlist",
        "CREATE TABLE wishlist_v32 (
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
        )",
        "INSERT INTO wishlist_v32 (id, player_id, appid, name, source, reason, state, on_steam, series_key, batch_id, title_id,
             hltb_main, hltb_at, added_at, updated_at, wishlisted_at)
         SELECT id, 1, appid, name, source, reason, state, on_steam, series_key, batch_id, title_id, hltb_main, hltb_at, added_at,
             updated_at, wishlisted_at FROM wishlist",
        &["CREATE INDEX wishlist_title ON wishlist(title_id)"],
    )?;

    // Tables with rows of their own gain whose they are. SQLite takes a new
    // column with a foreign key and a default only while foreign keys are
    // off, as they are until `migrate` has run every step.
    c.execute_batch(
        "ALTER TABLE stop_reasons ADD COLUMN player_id INTEGER NOT NULL DEFAULT 1 REFERENCES players(id);
         CREATE INDEX stop_reasons_player ON stop_reasons(player_id, title_id, at);
         ALTER TABLE profile_statements ADD COLUMN player_id INTEGER NOT NULL DEFAULT 1 REFERENCES players(id);
         DROP INDEX profile_favourites;
         DROP INDEX profile_about;
         CREATE UNIQUE INDEX profile_favourites ON profile_statements(player_id, title_id) WHERE kind = 'favourite';
         CREATE UNIQUE INDEX profile_about ON profile_statements(player_id, kind = 'context', kind = 'pattern', about) WHERE kind <> 'favourite';
         ALTER TABLE pick_batches ADD COLUMN player_id INTEGER NOT NULL DEFAULT 1 REFERENCES players(id);
         CREATE INDEX pick_batches_player ON pick_batches(player_id, created_at);
         ALTER TABLE routines ADD COLUMN player_id INTEGER NOT NULL DEFAULT 1 REFERENCES players(id);
         CREATE INDEX routines_player ON routines(player_id);
         ALTER TABLE play_diary ADD COLUMN player_id INTEGER REFERENCES players(id);
         ALTER TABLE play_diary ADD COLUMN account_id INTEGER REFERENCES store_accounts(id);
         CREATE INDEX play_diary_player ON play_diary(player_id, hour);
         UPDATE play_diary SET player_id = 1 WHERE source = 'launch';
         UPDATE play_diary SET account_id = (SELECT id FROM store_accounts WHERE store = 'steam') WHERE source = 'steam';
         ALTER TABLE api_tokens ADD COLUMN player_id INTEGER REFERENCES players(id);
         ALTER TABLE ai_calls ADD COLUMN ai_account_id INTEGER;
         ALTER TABLE ai_calls ADD COLUMN player_id INTEGER;
         ALTER TABLE plan_readings ADD COLUMN ai_account_id INTEGER;
         ALTER TABLE job_log ADD COLUMN title_id INTEGER;",
    )?;

    // The AI settings become player 1's AI account, which they use and the
    // library's own AI work runs on.
    let ai_settings = json_of(&mut AI_SETTINGS.iter().copied()).unwrap_or_else(|| "{}".into());
    c.execute(
        "INSERT INTO ai_accounts(owner_id, backend, settings, secret, created_at) VALUES (1, ?1, ?2, ?3, ?4)",
        params![settings.get("llm_backend"), ai_settings, json_of(&mut AI_SECRETS.iter().copied()), at],
    )?;
    let ai = c.last_insert_rowid();
    c.execute("UPDATE ai_calls SET ai_account_id = ?1", params![ai])?;
    c.execute(&format!("UPDATE ai_calls SET player_id = 1 WHERE kind IN ({PLAYER_CALLS})"), [])?;
    c.execute("UPDATE plan_readings SET ai_account_id = ?1", params![ai])?;
    c.execute("INSERT INTO player_settings(player_id, key, value) VALUES (1, 'ai_account', ?1)", params![ai.to_string()])?;
    c.execute("INSERT INTO settings(key, value) VALUES ('library_ai', ?1)", params![ai.to_string()])?;

    // The settings leave the household's table for their new homes.
    for key in PLAYER_KEYS {
        if let Some(v) = settings.get(key) {
            c.execute("INSERT INTO player_settings(player_id, key, value) VALUES (1, ?1, ?2)", params![key, v])?;
        }
    }
    let moved = PLAYER_KEYS.iter().chain(CONNECTION_KEYS.iter().map(|(k, _)| k)).chain(&AI_SETTINGS).chain(&AI_SECRETS);
    for key in moved.chain(&["llm_backend"]) {
        c.execute("DELETE FROM settings WHERE key = ?1", params![key])?;
    }
    Ok(())
}

/// v33: each player's own backlog verdicts.
fn v33(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE my_verdicts (
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
        );",
    )?;
    Ok(())
}

/// v34: sync per connection. The Heroic folder and the
/// Epic Games Launcher's manifests folder, when stored, move onto player
/// 1's Heroic and Epic connections, as v32 moved the Steam and Galaxy
/// settings; the play diary's last Steam reading becomes their first Steam
/// connection's, since each account now keeps its own; and connections are
/// looked up by owner and kind. GOG's and Epic's sign-ins were files the
/// database can't see: each moves onto its connection the first time it is
/// read (`crate::signin`).
fn v34(c: &Connection) -> Result<()> {
    // The keys' homes as they were at v34, as v32 keeps its own copies.
    const CONNECTION_KEYS: [(&str, &str); 2] = [("heroic_dir", "heroic"), ("epic_manifests", "epic")];
    c.execute_batch("CREATE INDEX store_connections_owner ON store_connections(owner_id, kind);")?;
    let own = |kind: &str| -> Result<Option<i64>> {
        Ok(c.query_row("SELECT id FROM store_connections WHERE owner_id=1 AND kind=?1 ORDER BY id LIMIT 1", params![kind], |r| r.get(0))
            .optional()?)
    };
    for (key, kind) in CONNECTION_KEYS {
        let value: Option<String> = c.query_row("SELECT value FROM settings WHERE key=?1", params![key], |r| r.get(0)).optional()?;
        let Some(value) = value.filter(|v| !v.trim().is_empty()) else {
            c.execute("DELETE FROM settings WHERE key=?1", params![key])?;
            continue;
        };
        let id = match own(kind)? {
            Some(id) => id,
            None => {
                c.execute("INSERT INTO store_connections(kind, owner_id, created_at) VALUES (?1, 1, ?2)", params![kind, super::now()])?;
                c.last_insert_rowid()
            }
        };
        c.execute("UPDATE store_connections SET settings = json_set(settings, '$.' || ?2, ?3) WHERE id=?1", params![id, key, value])?;
        c.execute("DELETE FROM settings WHERE key=?1", params![key])?;
    }
    if let Some(steam) = own("steam")? {
        c.execute("UPDATE meta SET key = 'diary_reading.' || ?1 WHERE key = 'diary_reading'", params![steam])?;
    }
    Ok(())
}

/// v35: backlog verdicts from other players, and the job that
/// downloads them, whose daily clock starts now rather than when the
/// database was made, so it isn't due the moment the server restarts.
fn v35(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE community_verdicts (
            id          INTEGER PRIMARY KEY,
            steam_appid INTEGER,
            gog_id      TEXT,
            n           INTEGER NOT NULL,
            counts      TEXT    NOT NULL,
            texts       TEXT    NOT NULL DEFAULT '[]'
        );
        CREATE INDEX community_verdicts_appid ON community_verdicts(steam_appid);
        CREATE INDEX community_verdicts_gog ON community_verdicts(gog_id);",
    )?;
    let since = super::now();
    c.execute(
        "INSERT OR IGNORE INTO settings(key, value) VALUES ('schedule.verdicts', ?1)",
        params![format!("{{\"every\":\"daily\",\"since\":{since}}}")],
    )?;
    Ok(())
}

/// v36: what a merge folded away, so a split can give it back.
fn v36(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE merge_snapshots (
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
        );",
    )?;
    Ok(())
}

/// v37: the identity provider's account linked to
/// each player, and browsers signed in through it as their own kind of
/// token. A CHECK can't change in place, so `api_tokens` is rebuilt.
fn v37(c: &Connection) -> Result<()> {
    c.execute_batch(
        "ALTER TABLE players ADD COLUMN oidc_issuer TEXT;
         ALTER TABLE players ADD COLUMN oidc_subject TEXT;
         CREATE UNIQUE INDEX players_oidc ON players(oidc_issuer, oidc_subject) WHERE oidc_subject IS NOT NULL;",
    )?;
    rebuild(
        c,
        "api_tokens",
        "CREATE TABLE api_tokens_v37 (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            name         TEXT    NOT NULL,
            scope        TEXT    NOT NULL CHECK (scope IN ('read','full')),
            kind         TEXT    NOT NULL DEFAULT 'token' CHECK (kind IN ('token','device','oidc')),
            hash         TEXT    NOT NULL UNIQUE,
            hint         TEXT    NOT NULL,
            created_at   INTEGER NOT NULL,
            last_used_at INTEGER,
            player_id    INTEGER REFERENCES players(id)
        )",
        "INSERT INTO api_tokens_v37(id, name, scope, kind, hash, hint, created_at, last_used_at, player_id)
         SELECT id, name, scope, kind, hash, hint, created_at, last_used_at, player_id FROM api_tokens",
        &[],
    )
}

/// v38: no table changes. The comments in `SCHEMA`, which SQLite keeps
/// with each table's text, were reworded to describe the tables without
/// pointing at the development notes, so a new database is made with the
/// new wording. A database from before keeps the comments it was made
/// with; nothing reads them (`migration_tests` compares tables with their
/// comments left out), so there is nothing to rewrite and no rows to move.
fn v38(_c: &Connection) -> Result<()> {
    Ok(())
}

/// Replace `table` with one made by `create` (as `<table>_v32`, or whatever
/// name it gives) and filled by `fill`, then rename it into place and make
/// `indexes`. Indexes the old table had beyond those (a development build's)
/// are made again on the new one.
fn rebuild(c: &Connection, table: &str, create: &str, fill: &str, indexes: &[&str]) -> Result<()> {
    let new = create.split_whitespace().nth(2).context("a CREATE TABLE statement")?;
    let extra: Vec<String> = {
        let mut st = c.prepare("SELECT name, sql FROM sqlite_master WHERE type='index' AND tbl_name=?1 AND sql IS NOT NULL")?;
        let rows = st.query_map(params![table], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter(|(name, _)| !indexes.iter().any(|i| i.split_whitespace().nth(2) == Some(name.as_str())))
            .map(|(_, sql)| sql)
            .collect()
    };
    c.execute_batch(create)?;
    c.execute_batch(fill)?;
    c.execute_batch(&format!("DROP TABLE {table}; ALTER TABLE {new} RENAME TO {table};"))?;
    for sql in indexes.iter().map(|s| s.to_string()).chain(extra) {
        c.execute_batch(&sql)?;
    }
    Ok(())
}

pub(super) fn table_exists(c: &Connection, table: &str) -> Result<bool> {
    Ok(c.query_row("SELECT name FROM sqlite_master WHERE type='table' AND name=?1", params![table], |r| r.get::<_, String>(0))
        .optional()?
        .is_some())
}

/// Why a database at `version` that holds a library is too old to open, as
/// the end of a sentence about it.
pub(crate) fn too_old(version: i64) -> String {
    format!(
        "is from a development build of UpNext made before its first release (schema v{version}); this build opens v{BASELINE_VERSION} and later and can't upgrade it"
    )
}

impl Db {
    /// Create the schema, or bring a database from `BASELINE_VERSION` on up
    /// to SCHEMA_VERSION. A database at SCHEMA_VERSION is left as it is.
    pub(super) fn migrate(c: &Connection) -> Result<()> {
        let mut version: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version == 0 && !table_exists(c, "games")? {
            let tx = c.unchecked_transaction()?;
            tx.execute_batch(SCHEMA)?;
            // Schedules count from when the database was made, so a task
            // that has never run is due one period from now, not at once.
            tx.execute("INSERT OR IGNORE INTO meta(key, value) VALUES ('schedules_since', ?1)", params![super::now().to_string()])?;
            super::players::make_first_player(&tx)?;
            tx.execute_batch(&format!("PRAGMA user_version={SCHEMA_VERSION};"))?;
            tx.commit()?;
            // `SCHEMA` is already the newest; no upgrade step applies.
            version = SCHEMA_VERSION;
        } else if version < BASELINE_VERSION {
            bail!("the database {}", too_old(version));
        }
        for &(to, step) in UPGRADES.iter().filter(|(to, _)| *to > version) {
            let tx = c.unchecked_transaction()?;
            step(&tx)?;
            tx.execute_batch(&format!("PRAGMA user_version={to};"))?;
            tx.commit()?;
        }
        c.execute_batch("PRAGMA foreign_keys=ON;")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrades_run_from_the_baseline_to_the_current_version_one_at_a_time() {
        let mut version = BASELINE_VERSION;
        for (to, _) in UPGRADES {
            assert_eq!(*to, version + 1, "after v{version}");
            version = *to;
        }
        assert_eq!(version, SCHEMA_VERSION, "the last step brings a database to SCHEMA_VERSION");
    }
}
