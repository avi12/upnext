//! Types shared with the rest of the app: the views the API serves and the
//! rows sync and enrichment write.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Public model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Untouched,
    Sampled,
    /// Played recently: you're already playing it.
    Playing,
    /// Has an ending, put down before it: you mean to finish it.
    Shelved,
    /// Played past the sampled threshold, and neither recent nor shelved:
    /// likely finished, an endless game, or a staple.
    Played,
    /// Beat it.
    Done,
    /// An endless game you play now and then.
    StillPlaying,
    /// An endless game on a break, out of picks until it ends.
    BurntOut,
    /// Not planning to go back.
    Abandoned,
    /// Not for me: never recommend it.
    Hidden,
    NotGame,
}

impl Status {
    /// In the order the library sorts by status.
    pub const ALL: [Status; 11] = [
        Status::Untouched,
        Status::Sampled,
        Status::Playing,
        Status::Shelved,
        Status::Played,
        Status::Done,
        Status::StillPlaying,
        Status::BurntOut,
        Status::Abandoned,
        Status::Hidden,
        Status::NotGame,
    ];
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Untouched => "untouched",
            Status::Sampled => "sampled",
            Status::Playing => "playing",
            Status::Shelved => "shelved",
            Status::Played => "played",
            Status::Done => "done",
            Status::StillPlaying => "still_playing",
            Status::BurntOut => "burnt_out",
            Status::Abandoned => "abandoned",
            Status::Hidden => "hidden",
            Status::NotGame => "not_game",
        }
    }
    /// A status by name; the names before v18 ("paused", "dropped") still work.
    pub fn parse(s: &str) -> Option<Status> {
        match s {
            "paused" => Some(Status::Shelved),
            "dropped" => Some(Status::Abandoned),
            _ => Status::ALL.iter().copied().find(|st| st.as_str() == s),
        }
    }
}

/// A status you set on a game. Beat it (`done`), Shelved and Abandoned are
/// for games with an ending, Still playing and Burnt out for endless ones,
/// and Not for me (`hidden`) for either.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Override {
    Shelved,
    Done,
    Abandoned,
    Hidden,
    StillPlaying,
    BurntOut,
}

impl Override {
    pub const ALL: [Override; 6] =
        [Override::Done, Override::Shelved, Override::Abandoned, Override::StillPlaying, Override::BurntOut, Override::Hidden];
    pub fn as_str(&self) -> &'static str {
        match self {
            Override::Shelved => "shelved",
            Override::Done => "done",
            Override::Abandoned => "abandoned",
            Override::Hidden => "hidden",
            Override::StillPlaying => "still_playing",
            Override::BurntOut => "burnt_out",
        }
    }
    /// A status by name, including the names before v18 ("paused", "dropped").
    pub fn parse(s: &str) -> Option<Override> {
        match s {
            "paused" | "break" => Some(Override::Shelved),
            "dropped" => Some(Override::Abandoned),
            "hide" => Some(Override::Hidden),
            _ => Override::ALL.iter().copied().find(|o| o.as_str() == s),
        }
    }
    pub(crate) fn status(&self) -> Status {
        match self {
            Override::Shelved => Status::Shelved,
            Override::Done => Status::Done,
            Override::Abandoned => Status::Abandoned,
            Override::Hidden => Status::Hidden,
            Override::StillPlaying => Status::StillPlaying,
            Override::BurntOut => Status::BurntOut,
        }
    }
    /// Whether marking it asks why the game was put down.
    pub fn asks_why(&self) -> bool {
        matches!(self, Override::Shelved | Override::Abandoned | Override::BurntOut)
    }
}

pub const SOURCES: [&str; 4] = ["steam", "epic", "gog", "amazon"];

/// Launchers that run games on this machine own their rows; Galaxy imports
/// and the GOG account API only fill gaps.
pub fn is_native_launcher(l: &str) -> bool {
    matches!(l, "steam" | "heroic")
}

/// One player review, from Steam (thumbs) or GOG (stars).
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ReviewText {
    pub source: String,
    pub positive: Option<bool>,
    pub rating: Option<f64>,
    pub votes: i64,
    pub hours: Option<i64>,
    pub title: Option<String>,
    pub text: String,
    /// When it was posted, Unix seconds (None for reviews stored before 2026-09-30).
    #[serde(default)]
    pub posted: Option<i64>,
}

/// One store's copy of a title.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Copy {
    pub game_id: i64,
    pub source: String,
    pub source_id: String,
    pub launcher: String,
    /// The store's own name for this copy (may carry an edition suffix).
    pub name: String,
    /// "Remastered", "Ultimate Edition", ... or null for the plain release.
    pub edition: Option<String>,
    pub installed: bool,
    pub playtime_minutes: i64,
    pub playtime_known: bool,
    pub last_played: i64,
    pub run_url: String,
    pub store_url: String,
    #[serde(skip)]
    pub has_gog_meta: bool,
    #[serde(skip)]
    pub has_gog_reviews: bool,
    /// The store account it is in, which decides who sees it and
    /// whose hours it carries.
    #[serde(skip)]
    pub account_id: i64,
    #[serde(skip)]
    pub playtime_2weeks: i64,
}

/// One game, aggregated over every store it is owned on. Shape mirrors docs/api/games.md.
/// One gameplay screenshot: a ~600px thumbnail and the full-size image.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Screenshot {
    pub thumb: String,
    pub full: String,
}

impl Screenshot {
    /// From a Steam store asset path (`steam/apps/…/ss_x.jpg?t=…`).
    pub(super) fn steam(path: &str) -> Self {
        let base = "https://shared.akamai.steamstatic.com/store_item_assets/";
        let (file, query) = path.split_once('?').map(|(f, q)| (f, format!("?{q}"))).unwrap_or((path, String::new()));
        let sized = |size: &str| match file.strip_suffix(".jpg") {
            Some(stem) => format!("{base}{stem}.{size}.jpg{query}"),
            None => format!("{base}{file}{query}"),
        };
        Screenshot { thumb: sized("600x338"), full: sized("1920x1080") }
    }

    /// From an IGDB image id.
    pub(super) fn igdb(id: &str) -> Self {
        let url = |size: &str| format!("https://images.igdb.com/igdb/image/upload/{size}/{id}.jpg");
        Screenshot { thumb: url("t_screenshot_med"), full: url("t_1080p") }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct GameView {
    /// Title id (stable; used in URLs, overrides and picks).
    pub id: i64,
    pub name: String,
    pub appid: Option<u32>,
    pub steam_matched: bool,
    /// Sources this title is owned on, e.g. ["steam", "gog"].
    pub owned_on: Vec<String>,
    /// The copy we would launch: installed first, then Steam, GOG, Epic, Amazon.
    pub primary_source: String,
    /// Distinct editions across copies, e.g. ["Remastered"]. Empty when every copy is the plain release.
    pub editions: Vec<String>,
    pub copies: Vec<Copy>,
    // Primary copy, kept flat for convenience.
    pub source: String,
    pub source_id: String,
    pub launcher: String,
    pub installed: bool,
    pub playtime_minutes: i64,
    pub playtime_known: bool,
    pub playtime_2weeks: i64,
    pub last_played: i64,
    pub status: Status,
    #[serde(rename = "override")]
    /// The status you set, as it stands now (`lifecycle::Standing::mark`).
    pub override_: Option<Override>,
    pub override_note: Option<String>,
    /// Whether it has an ending (the switch's value, or else inferred).
    pub kind: crate::lifecycle::Kind,
    /// The game page's "This game has no ending" switch: None until set.
    pub no_ending: Option<bool>,
    /// One of your staples: played at least `staple_hours`.
    pub staple: bool,
    /// When UpNext found it shelved or abandoned (not set when you marked it).
    pub shelved_at: Option<i64>,
    pub abandoned_at: Option<i64>,
    /// When a Burnt out game comes back to picks.
    pub burnt_out_until: Option<i64>,
    /// Why you put it down, the latest time you said.
    pub stop_reasons: Option<crate::lifecycle::StopReasons>,
    /// Times it was turned down in picks while it has this status
    /// (`lifecycle::Standing::rejections`).
    #[serde(default)]
    pub rejections: u32,
    /// Abandoned because it was turned down twice in picks.
    #[serde(default)]
    pub abandoned_by_picks: bool,
    /// The last "It's too ___" said about it in a lineup: a light note.
    #[serde(default)]
    pub too: Option<TooNote>,
    /// The last time it was chosen from a lineup with Play.
    #[serde(default)]
    pub chosen: Option<ChosenPick>,
    /// Your own rating of the game, 1 to 5 stars, and when you
    /// gave it.
    #[serde(default)]
    pub my_rating: Option<u8>,
    #[serde(default)]
    pub rated_at: Option<i64>,
    /// The latest rating it had as a pick, from any lineup.
    #[serde(default)]
    pub pick_rating: Option<RatedPick>,
    /// Shelved again from the Second Chance page ("Willing to try again"):
    /// turned down once in picks, it's abandoned for good.
    #[serde(default)]
    pub second_try: bool,
    /// The Second Chance page's "Now" and answer, once it has either.
    #[serde(default)]
    pub second_chance: Option<SecondChanceNote>,
    /// It was on the wishlist before a sync found it in the library (plan
    /// 14): the game page's "From your wishlist". Only `GET /api/games/{id}` fills it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_wishlist: Option<crate::db::FromWishlist>,
    /// "Hide this game" as this player set it: `me`, `others` or
    /// `everyone`; None when they haven't hidden it.
    #[serde(default)]
    pub hidden_scope: Option<crate::db::HideScope>,
    /// A copy of it is in a store account of this player's own, so they can
    /// hide it from the others.
    #[serde(default)]
    pub own_copy: bool,
    #[serde(rename = "type")]
    pub type_: Option<String>,
    pub short_description: Option<String>,
    pub genres: Vec<String>,
    pub categories: Vec<String>,
    /// Merged tags from every source (SteamSpy, review tags, GOG, RAWG, IGDB,
    /// PCGamingWiki), most relevant first.
    pub tags: Vec<String>,
    pub tags_by_source: HashMap<String, Vec<String>>,
    /// IGDB themes / perspectives / modes, when known.
    pub themes: Vec<String>,
    pub header_image: String,
    pub art_square: Option<String>,
    pub metacritic: Option<i64>,
    pub release_date: Option<String>,
    pub developers: Vec<String>,
    pub review_score: Option<i64>,
    pub review_desc: Option<String>,
    pub review_positive: Option<i64>,
    pub review_total: Option<i64>,
    pub review_pct: Option<i64>,
    pub hltb_main: Option<f64>,
    pub hltb_extra: Option<f64>,
    pub hltb_complete: Option<f64>,
    pub hltb_url: Option<String>,
    pub gog_rating: Option<f64>,
    pub gog_rating_count: Option<i64>,
    pub igdb_rating: Option<f64>,
    pub igdb_rating_count: Option<i64>,
    /// Most helpful player reviews (Steam thumbs, GOG stars), for reading.
    pub review_texts: Vec<ReviewText>,
    /// People say: an AI-written critical look at what the reviews say, its
    /// strengths, weaknesses and what sets it apart.
    pub digest: Option<String>,
    /// Vibes: what playing it feels like, written from the same reviews.
    /// None until a summary is written at `DIGEST_VERSION` 3, or when the
    /// reviews say nothing about the experience.
    #[serde(default)]
    pub vibes: Option<String>,
    /// What the reviews say the game is like, written with the summary.
    /// None until a summary is written at `DIGEST_VERSION` 2 or later.
    #[serde(default)]
    pub review_tags: Option<crate::review_tags::ReviewTags>,
    /// The digest prompt version the summary was written with (None: before
    /// versions were recorded).
    #[serde(skip)]
    pub digest_version: Option<i64>,
    /// The date of UpNext's shared data the summary, Vibes and
    /// review tags came from, as YYYY-MM-DD; None when this install wrote
    /// them itself, or there are none.
    #[serde(default)]
    pub seed_date: Option<String>,
    /// What other backlog players say about it, from the last
    /// snapshot of their verdicts, when "Backlog verdicts from other
    /// players" is on and it has at least 3.
    #[serde(default)]
    pub backlog: Option<crate::verdicts::community::Backlog>,
    pub enriched: bool,
    pub store_url: String,
    pub run_url: String,
    /// RAWG community rating (0-5) and vote count.
    #[serde(default)]
    pub rawg_rating: Option<f64>,
    #[serde(default)]
    pub rawg_ratings_count: Option<i64>,
    /// RAWG game page (attribution link required by RAWG's free tier).
    #[serde(default)]
    pub rawg_url: Option<String>,
    #[serde(default)]
    pub pcgw_url: Option<String>,
    #[serde(default)]
    pub wikidata_url: Option<String>,
    /// Game series from PCGamingWiki and Wikidata.
    #[serde(default)]
    pub series: Vec<String>,
    /// Wikidata QID of each series, by series key.
    #[serde(skip)]
    pub series_qids: HashMap<String, String>,
    /// Original release year (earliest Wikidata publication date).
    #[serde(default)]
    pub first_year: Option<i32>,
    /// Gameplay screenshots (Steam first, else IGDB), thumbnail + full size.
    #[serde(default)]
    pub screenshots: Vec<Screenshot>,
    /// Portrait (2:3) and hero art from Steam's asset list, when known.
    #[serde(default)]
    pub portrait_url: Option<String>,
    #[serde(default)]
    pub hero_url: Option<String>,
    /// How well a gamepad works with it, as Steam's store says: `full` or
    /// `partial`; None when Steam doesn't say, which mostly means a mouse
    /// and keyboard. Big screen shows it and filters on it.
    #[serde(default)]
    pub controller_support: Option<String>,
    /// When the status override was set (unix seconds), if there is one.
    #[serde(default)]
    pub override_at: Option<i64>,
    /// When the game was saved to Play later (unix seconds), if it is.
    #[serde(default)]
    pub saved_at: Option<i64>,
    /// A YouTube trailer (IGDB video id) or, failing that, a YouTube search.
    #[serde(default)]
    pub trailer_url: Option<String>,
    /// Steam lists a Linux build (None = unknown).
    #[serde(default)]
    pub linux_native: Option<bool>,
    /// ProtonDB tier (platinum/gold/silver/bronze/borked/pending) and report count.
    #[serde(default)]
    pub protondb_tier: Option<String>,
    #[serde(default)]
    pub protondb_trending: Option<String>,
    #[serde(default)]
    pub protondb_total: Option<i64>,
    /// "native" when a Linux build exists, else the ProtonDB tier; None = unknown.
    #[serde(default)]
    pub linux: Option<String>,
    // Internal flags for the enrich pipeline; not part of the API shape.
    #[serde(skip)]
    pub has_xref: bool,
    #[serde(skip)]
    pub has_title_series: bool,
    #[serde(skip)]
    pub has_trailer_lookup: bool,
    #[serde(skip)]
    pub xref: Option<crate::wikidata::Xref>,
    #[serde(skip)]
    pub has_rawg: bool,
    #[serde(skip)]
    pub has_pcgw: bool,
    #[serde(skip)]
    pub has_protondb: bool,
    #[serde(skip)]
    pub has_details: bool,
    #[serde(skip)]
    pub has_reviews: bool,
    #[serde(skip)]
    pub has_tags: bool,
    #[serde(skip)]
    pub has_hltb: bool,
    #[serde(skip)]
    pub has_match_attempt: bool,
    #[serde(skip)]
    pub has_review_texts: bool,
    #[serde(skip)]
    pub has_igdb: bool,
    #[serde(skip)]
    pub has_digest: bool,
}

impl GameView {
    /// Whether picks can offer it, and how (`lifecycle::offer`).
    pub fn offer(&self) -> Option<crate::lifecycle::Offer> {
        let same = self.second_chance.as_ref().is_some_and(SecondChanceNote::still_the_same);
        crate::lifecycle::offer(self.status, self.kind, self.staple, self.rejections, same)
    }

    /// For a game offered as a second chance (or shelved again for one),
    /// the gist of what reviews since it was put down say about why: the
    /// Second Chance page's "Now", while it's up to date.
    pub fn reviews_since_line(&self) -> Option<&str> {
        use crate::lifecycle::Offer;
        let second = match self.offer() {
            Some(Offer::SecondChance) => true,
            Some(Offer::Shelved) => self.second_try,
            _ => false,
        };
        let note = self.second_chance.as_ref().filter(|n| second && !n.outdated)?;
        note.line.as_deref().filter(|l| !l.is_empty())
    }

    /// A copy for list responses: without the review texts, per-source tags
    /// and screenshots, which only the game sheet shows (it fetches the full title).
    pub fn slim(&self) -> GameView {
        let mut g = self.clone();
        g.review_texts = Vec::new();
        g.tags_by_source = HashMap::new();
        g.screenshots = Vec::new();
        g
    }

    /// (year, month, day) from the store date: "Nov 1, 2000", "1 Nov, 2000",
    /// "2019-07-23" or a bare year. Month/day are 0 when unknown.
    pub fn release_key(&self) -> Option<(i32, u32, u32)> {
        let rd = self.release_date.as_ref()?;
        let nums: Vec<&str> = rd.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).collect();
        let year = nums.iter().find(|s| s.len() == 4)?.parse().ok()?;
        const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
        let lower = rd.to_lowercase();
        let (month, day) = if let Some(m) = MONTHS.iter().position(|m| lower.contains(m)) {
            (m as u32 + 1, nums.iter().find(|s| s.len() <= 2).and_then(|d| d.parse().ok()).unwrap_or(0))
        } else if nums.len() >= 3 && nums[0].len() == 4 {
            (nums[1].parse().unwrap_or(0), nums[2].parse().unwrap_or(0))
        } else {
            (0, 0)
        };
        Some((year, month, day))
    }

    pub fn release_year(&self) -> Option<String> {
        let rd = self.release_date.as_ref()?;
        rd.split(|c: char| !c.is_ascii_digit()).find(|s| s.len() == 4).map(str::to_string)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SourceInfo {
    pub source: String,
    pub launcher: String,
    pub label: String,
    pub configured: bool,
    pub count: i64,
    pub last_sync: Option<i64>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, utoipa::ToSchema)]
pub struct Profile {
    pub steam_id: String,
    pub persona_name: Option<String>,
    pub avatar: Option<String>,
    pub profile_url: Option<String>,
    pub last_sync: Option<i64>,
    /// Number of titles (deduplicated games).
    pub game_count: i64,
    #[serde(default)]
    pub copy_count: i64,
    #[serde(default)]
    pub sources: Vec<SourceInfo>,
}

#[derive(Debug, Clone, Default)]
pub struct GameFilter {
    pub statuses: Option<Vec<Status>>,
    /// Only your staples (`GameView::staple`).
    pub staple: Option<bool>,
    pub sources: Option<Vec<String>>,
    pub installed: Option<bool>,
    /// Only games Steam says a gamepad works with, fully or in part.
    pub controller: Option<bool>,
    pub q: Option<String>,
    pub genre: Option<String>,
    pub tag: Option<String>,
    /// Linux compatibility: "ok" (native/platinum/gold), "native", "issues"
    /// (silver/bronze/borked) or "unknown".
    pub linux: Option<String>,
    /// Review-tag facets a title must all have, "sessions:short", "mood:cozy".
    pub facets: Vec<String>,
    /// Aspects players must have praised ("story").
    pub praised: Vec<String>,
    /// Aspects players must not have criticized or split on ("grind"). Only
    /// titles with review tags qualify: without them nobody has said.
    pub not_criticized: Vec<String>,
    /// Your own rating: "rated", "unrated", "high" (4 or 5 stars)
    /// or "low" (1 or 2).
    pub rating: Option<String>,
    /// What other backlog players say: "respects_time" (a clear
    /// majority say it respects limited time, or mostly) or "clicks_fast"
    /// (a clear majority say it clicked in the first hour).
    pub backlog: Option<String>,
    pub sort: String,
    pub order: Option<String>,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct NameCount {
    pub name: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct Stats {
    pub total: usize,
    pub copies: usize,
    pub multi_store: usize,
    pub by_status: HashMap<String, usize>,
    pub by_source: HashMap<String, usize>,
    pub installed: usize,
    pub candidates: usize,
    /// Your staples (`GameView::staple`).
    pub staples: usize,
    /// Games you rated.
    pub rated: usize,
    pub enriched: usize,
    pub unenriched: usize,
    pub with_reviews: usize,
    pub with_tags: usize,
    pub with_hltb: usize,
    pub with_review_texts: usize,
    pub with_digests: usize,
    pub with_xref: usize,
    pub with_rawg: usize,
    pub with_pcgw: usize,
    /// Titles by Linux compatibility ("native", ProtonDB tier, or "unknown").
    pub by_linux: HashMap<String, usize>,
    pub total_playtime_minutes: i64,
    pub genres: Vec<NameCount>,
    pub tags: Vec<NameCount>,
    pub top_played: Vec<GameView>,
}

/// What was done with a pick on its card (`pick_feedback.action`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackAction {
    /// Play: chosen from the lineup.
    Chosen,
    /// A soft, mood-based no. Nothing about the game changes.
    NotTonight,
    /// "It's too ___", with the word.
    Too,
    /// Taken off by the same "It's too ___" call as another pick: the AI
    /// found it has the same problem. Not a rejection: you didn't say it.
    SameProblem,
    /// Never recommend this.
    NotForMe,
    /// "I've started it…", with how it went (an override's name).
    Started,
}

impl FeedbackAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chosen => "chosen",
            Self::NotTonight => "not_tonight",
            Self::Too => "too",
            Self::SameProblem => "same_problem",
            Self::NotForMe => "not_for_me",
            Self::Started => "started",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Chosen, Self::NotTonight, Self::Too, Self::SameProblem, Self::NotForMe, Self::Started].into_iter().find(|a| a.as_str() == s)
    }

    /// A real no, which counts toward taking shelved and abandoned games out
    /// of picks. Not tonight is mood and never counts.
    pub fn is_rejection(self) -> bool {
        matches!(self, Self::Too | Self::NotForMe)
    }
}

/// The last thing done with a pick on its card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct PickFeedback {
    pub action: FeedbackAction,
    /// The word for `too` and `same_problem`, how it went for `started`.
    pub text: Option<String>,
    pub at: i64,
}

/// How well a pick fit what its lineup was asked for: the
/// thumbs on its card. A rating of the suggestion, not of the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PickVerdict {
    /// "Good pick".
    Good,
    /// "Not a fit".
    NotAFit,
}

impl PickVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Good => "good",
            Self::NotAFit => "not_a_fit",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Good, Self::NotAFit].into_iter().find(|v| v.as_str() == s)
    }
}

/// Why a pick was not a fit, when you said.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MissReason {
    WrongLength,
    WrongMood,
    WrongGenre,
    PlayedOut,
}

impl MissReason {
    pub const ALL: [MissReason; 4] = [Self::WrongLength, Self::WrongMood, Self::WrongGenre, Self::PlayedOut];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::WrongLength => "wrong_length",
            Self::WrongMood => "wrong_mood",
            Self::WrongGenre => "wrong_genre",
            Self::PlayedOut => "played_out",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.as_str() == s)
    }

    /// "wrong length", for the prompt and reasons.
    pub fn words(self) -> &'static str {
        match self {
            Self::WrongLength => "wrong length",
            Self::WrongMood => "wrong mood",
            Self::WrongGenre => "wrong genre",
            Self::PlayedOut => "played out on games like it",
        }
    }
}

/// A pick's rating on its card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct PickRating {
    pub verdict: PickVerdict,
    /// Why it was not a fit, when you said; always None for a good pick.
    pub reason: Option<MissReason>,
    pub at: i64,
}

/// The latest rating a game had as a pick, with what its lineup was asked
/// for. Picks read it for `PICK_RATING_DAYS`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RatedPick {
    pub verdict: PickVerdict,
    pub reason: Option<MissReason>,
    pub at: i64,
    /// The lineup's request; empty for one asked with no words.
    pub prompt: String,
}

/// How long a pick's rating speaks for the game in picks: it was about a
/// request, so it doesn't last as a rating of the game does.
pub const PICK_RATING_DAYS: i64 = 90;

/// "It's too ___", as last said about a game.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct TooNote {
    pub word: String,
    pub at: i64,
}

/// The Second Chance page's "Now" for a game and its answer
/// (`second_chances` table).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SecondChanceNote {
    /// What player reviews since you stopped say about why, by the AI.
    pub now: Option<String>,
    pub verdict: Option<crate::second_chance::Verdict>,
    /// The gist of `now`, for picks: "2025 reviews say the crashes are fixed".
    pub line: Option<String>,
    pub written_at: Option<i64>,
    pub model: Option<String>,
    /// Written before your reasons or the reviews last changed.
    pub outdated: bool,
    pub answer: Option<crate::second_chance::Answer>,
    pub answered_at: Option<i64>,
    /// What `now` was written from (`second_chance::inputs`).
    #[serde(skip)]
    pub inputs: Option<String>,
}

impl SecondChanceNote {
    /// Reviews since you stopped say what stopped you is still there.
    pub fn still_the_same(&self) -> bool {
        self.verdict == Some(crate::second_chance::Verdict::Same) && !self.outdated
    }
}

/// When a game was last chosen from a lineup with Play, and what the
/// lineup was asked for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ChosenPick {
    pub at: i64,
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Pick {
    pub id: i64,
    pub appid: Option<u32>,
    pub name: String,
    pub reason: String,
    pub game: Option<GameView>,
    /// Where it shows in the lineup now, from 0; None for a spare not used
    /// yet or a pick taken off.
    #[serde(default)]
    pub slot: Option<usize>,
    /// The last thing done with it on its card, in this lineup.
    #[serde(default)]
    pub feedback: Option<PickFeedback>,
    /// How well it fit the lineup's request, as rated on its card.
    #[serde(default)]
    pub rating: Option<PickRating>,
    /// False for a game on your wishlist that you don't own: its
    /// `id` is minus its wishlist entry's id, `game` is None, `wishlist`
    /// says what it is, and `alternate` is the owned game to switch to.
    #[serde(default = "owned")]
    pub owned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wishlist: Option<crate::db::WishGame>,
    /// On a game you don't own: the owned game "Switch with owned game"
    /// puts in its place, chosen by the same call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternate: Option<Alternate>,
    /// What other backlog players say about it, the parts the request asks
    /// about first: "Backlog players: clicks fast, fine in short
    /// sessions (23)". Worked out when the lineup is read, never stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backlog: Option<String>,
}

fn owned() -> bool {
    true
}

impl Pick {
    /// A pick of a game in the library, before it has a slot.
    pub fn of(g: &GameView, reason: String) -> Self {
        Pick {
            id: g.id,
            appid: g.appid,
            name: g.name.clone(),
            reason,
            game: Some(g.slim()),
            slot: None,
            feedback: None,
            rating: None,
            owned: true,
            wishlist: None,
            alternate: None,
            backlog: None,
        }
    }
}

/// The owned game behind a wishlisted pick ("Switch with owned game").
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Alternate {
    /// Its title id.
    pub id: i64,
    pub name: String,
    pub reason: String,
    pub game: Option<GameView>,
}

/// A game you don't own that the AI suggested with a lineup,
/// checked against Steam's store and your library.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Suggestion {
    pub game: crate::db::WishGame,
    /// Why it's right down your alley, for this request.
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct PickBatch {
    pub batch_id: String,
    pub prompt: String,
    pub created_at: i64,
    pub model: Option<String>,
    pub duration_ms: Option<i64>,
    /// Charged for its calls: an API key or a hosted provider.
    pub cost_usd: Option<f64>,
    /// Its calls at the API's list prices, charged or not (on a
    /// subscription, what they'd cost on the API). None before v15, or when
    /// the model has no known price.
    #[serde(default)]
    pub api_usd: Option<f64>,
    pub candidates_considered: Option<i64>,
    pub attempts: Option<i64>,
    /// Its calls' tokens: the pick (two calls when the first answer was
    /// rejected), and any "It's too ___" and new spares since. None for
    /// batches from before v15, or when the backend didn't say.
    #[serde(default)]
    pub usage: Option<crate::llm::Usage>,
    /// How many picks the lineup shows. A slot with no pick in it is
    /// waiting for new spares.
    #[serde(default)]
    pub shown: usize,
    /// The picks on screen, in slot order.
    pub picks: Vec<Pick>,
    /// Spares not used yet, best first: they replace picks taken off.
    #[serde(default)]
    pub spares: Vec<Pick>,
    /// Picks taken off the lineup, in the order they were, with why.
    #[serde(default)]
    pub removed: Vec<Pick>,
    /// Picked without AI, from UpNext's own ranking: what it was ranked
    /// by, in plain words. None for a lineup the AI picked.
    #[serde(default)]
    pub ranked_by: Option<Vec<String>>,
    /// A discovery mode: picked only from games new to you, or
    /// from hidden gems. None for a lineup from the whole library.
    #[serde(default)]
    pub mode: Option<crate::discovery::Mode>,
    /// A game you don't own, right down your alley for this request (plan
    /// 14), with what you answered since in its `state`. None when the AI
    /// suggested none or it didn't pass the checks, and with the wishlist
    /// turned off.
    #[serde(default)]
    pub suggestion: Option<Suggestion>,
    /// The routine it was picked for, as it was when the lineup was
    /// made; its new spares keep to it. None when none was chosen.
    #[serde(default)]
    pub routine: Option<crate::taste::routine::Routine>,
}

impl PickBatch {
    /// Put each pick's line from other backlog players on it, from the
    /// verdicts its game has now.
    pub fn fill_backlog_lines(&mut self) {
        let asked = crate::prompt::verdict_wishes(&self.prompt);
        for p in self.picks.iter_mut().chain(self.spares.iter_mut()).chain(self.removed.iter_mut()) {
            p.backlog = p.game.as_ref().and_then(|g| g.backlog.as_ref()).and_then(|b| b.card_line(&asked));
        }
    }

    /// Slots with no pick in them.
    pub fn empty_slots(&self) -> Vec<usize> {
        (0..self.shown).filter(|s| !self.picks.iter().any(|p| p.slot == Some(*s))).collect()
    }

    /// Every game in the batch: on screen, spare or taken off.
    pub fn all_ids(&self) -> Vec<i64> {
        self.picks.iter().chain(&self.spares).chain(&self.removed).map(|p| p.id).collect()
    }
}

// Input structs -----------------------------------------------------------------

/// A game as reported by one launcher during sync.
#[derive(Debug, Clone, Default)]
pub struct ImportedGame {
    pub source: String,
    pub source_id: String,
    pub launcher: String,
    pub name: String,
    pub appid: Option<u32>,
    pub installed: bool,
    /// `None` when the launcher does not track playtime.
    pub playtime_minutes: Option<i64>,
    pub playtime_2weeks: i64,
    pub last_played: i64,
    pub art_cover: Option<String>,
    pub art_square: Option<String>,
    pub description: Option<String>,
    pub genres: Vec<String>,
    pub developer: Option<String>,
    pub release_date: Option<String>,
    pub store_url: Option<String>,
    pub run_url: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct DetailsRow {
    /// The store's name for it. Not stored: the library has its own names;
    /// the wishlist takes it for a game Steam's wishlist gave no name for.
    pub name: Option<String>,
    pub type_: Option<String>,
    pub short_description: Option<String>,
    pub genres: Vec<String>,
    pub categories: Vec<String>,
    pub header_image: Option<String>,
    pub metacritic: Option<i64>,
    pub release_date: Option<String>,
    pub developers: Vec<String>,
    pub publishers: Vec<String>,
    pub is_free: Option<bool>,
    pub controller_support: Option<String>,
    pub platforms: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ReviewRow {
    pub review_score: Option<i64>,
    pub review_score_desc: Option<String>,
    pub total_positive: Option<i64>,
    pub total_negative: Option<i64>,
    pub total_reviews: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct HltbRow {
    pub hltb_id: Option<i64>,
    pub hltb_name: Option<String>,
    pub similarity: Option<f64>,
    pub main_story: Option<f64>,
    pub main_extra: Option<f64>,
    pub completionist: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncSummary {
    pub source: String,
    pub total: usize,
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub skipped: usize,
    pub merged_titles: usize,
}
