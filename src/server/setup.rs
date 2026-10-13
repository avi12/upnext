//! First-run setup (docs/api/setup.md): what the
//! wizard reads at every step (`GET /setup`), whether it should show at all
//! and where it got to (`GET /setup/state`, `PUT /setup`), and a GOG Galaxy
//! database uploaded from another computer (`POST /galaxy/upload`). The
//! jobs it starts in the background are in `setup_sync`.
//!
//! The household's state lives in the settings table (`setup.state`), so
//! the desktop app and a browser on the same server share it. Each player's
//! own short setup and their tour are theirs, in `player_settings`.

use super::players::{Me, Who};
use super::{ApiError, ApiResult, AppState, access};
use crate::config::Config;
use crate::db::PlayerId;
use axum::body::Body;
use axum::extract::{Extension, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::{Json, Router};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

pub fn routes() -> Router<AppState> {
    use axum::routing::{get, post};
    Router::new()
        .route("/setup", get(setup_get).put(setup_put))
        .route("/setup/state", get(state_get))
        .route("/setup/reset", post(reset))
        .route("/setup/background", post(super::setup_sync::background))
        .route("/galaxy/upload", post(galaxy_upload))
}

/// The settings-table key the wizard's state is kept under.
const KEY: &str = "setup.state";

/// The wizard's steps, in order. `PUT /setup` takes one of these as `step`.
pub const STEPS: [&str; 13] =
    ["welcome", "where", "steam", "stores", "more_stores", "extras", "ai", "ai_jobs", "sync", "details", "taste", "players", "done"];

/// The private player setting a player's own setup and tour are kept under.
const PLAYER_KEY: &str = "setup.player";

/// A new player's own short setup (`docs/players.md`), in order: their name
/// and colour, the games offered to them, the AI, the taste questions, and
/// the tour's offer. `PUT /setup` takes one of these as `player_step`.
pub const PLAYER_STEPS: [&str; 5] = ["you", "games", "ai", "taste", "done"];

/// Seconds a title takes in Fetch details with the parallel lanes: a
/// 40-game sample took 122 s.
const DETAILS_SECONDS_PER_TITLE: f64 = 3.05;

/// The biggest Galaxy database taken. Galaxy's own file is tens of MB.
const MAX_GALAXY: u64 = 1 << 30;

/// Where setup or the tour stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum SetupProgress {
    /// Not finished or skipped yet.
    #[default]
    Open,
    Done,
    Skipped,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Stored {
    #[serde(default)]
    step: Option<String>,
    #[serde(default)]
    setup: SetupProgress,
    #[serde(default)]
    tour: SetupProgress,
}

fn stored(state: &AppState) -> anyhow::Result<Stored> {
    Ok(state.db.setting_get(PlayerId::FIRST, KEY)?.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default())
}

/// One player's own setup and tour. Unset parts take their defaults in
/// `setup_state`: the first player's own setup is the household wizard, and
/// their tour was the household's before players had their own.
#[derive(Debug, Default, Serialize, Deserialize)]
struct PlayerStored {
    #[serde(default)]
    step: Option<String>,
    #[serde(default)]
    setup: Option<SetupProgress>,
    #[serde(default)]
    tour: Option<SetupProgress>,
}

fn player_stored(db: &crate::db::Db, p: PlayerId) -> anyhow::Result<PlayerStored> {
    Ok(db.private_setting(p, PLAYER_KEY)?.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default())
}

fn player_store(db: &crate::db::Db, p: PlayerId, s: &PlayerStored) -> anyhow::Result<()> {
    db.set_private_setting(p, PLAYER_KEY, Some(&serde_json::to_string(s)?))
}

/// Mark player `p`'s own setup done, for a player who never needs it: the
/// mock library's second player, whose library is already filled in.
pub fn own_setup_done(db: &crate::db::Db, p: PlayerId) -> anyhow::Result<()> {
    let mut own = player_stored(db, p)?;
    own.setup = Some(SetupProgress::Done);
    player_store(db, p, &own)
}

/// Setup is neither done nor skipped: a first run.
pub(super) fn setup_open(state: &AppState) -> anyhow::Result<bool> {
    Ok(stored(state)?.setup == SetupProgress::Open)
}

/// Whether the wizard shows, and where it got to.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SetupState {
    /// Every page goes to the wizard: the library is empty and setup hasn't
    /// been finished or skipped. An install with games never needs it.
    pub needed: bool,
    /// The step reached (one of `STEPS`), or null before the first.
    pub step: Option<String>,
    pub setup: SetupProgress,
    /// The request's player's tour (each player has their own).
    pub tour: SetupProgress,
    /// The one-time "Show me around" offer: the tour hasn't been taken or
    /// turned down, neither the wizard nor the player's own setup is
    /// showing, and the library has games to show (an empty one gets the
    /// offer once a sync brings some).
    pub tour_offer: bool,
    /// Every page goes to the request's player's own short setup:
    /// there are two or more players, the household's wizard isn't needed,
    /// and this player hasn't finished or skipped theirs. The first player's
    /// own setup is the household wizard, and whoever finishes or skips the
    /// wizard has had theirs.
    pub player_needed: bool,
    /// The step of their own setup they reached (one of `PLAYER_STEPS`), or
    /// null before the first.
    pub player_step: Option<String>,
    pub player_setup: SetupProgress,
}

/// Where setup stands for player `p`; `None` while nobody has been chosen
/// (a default player with a PIN), which gets the household's part only.
fn setup_state(state: &AppState, p: Option<PlayerId>) -> anyhow::Result<SetupState> {
    let s = stored(state)?;
    let empty = state.db.all_games_shared(PlayerId::FIRST)?.is_empty();
    let needed = empty && s.setup == SetupProgress::Open;
    let own = match p {
        Some(p) => player_stored(&state.db, p)?,
        None => PlayerStored::default(),
    };
    let first = p.is_none_or(|p| p == PlayerId::FIRST);
    let player_setup = own.setup.unwrap_or(if first { SetupProgress::Done } else { SetupProgress::Open });
    let tour = own.tour.unwrap_or(if first { s.tour } else { SetupProgress::Open });
    let player_needed = p.is_some() && !needed && player_setup == SetupProgress::Open && state.db.players()?.len() > 1;
    Ok(SetupState {
        needed,
        step: s.step,
        setup: s.setup,
        tour,
        tour_offer: p.is_some() && !empty && !needed && !player_needed && tour == SetupProgress::Open,
        player_needed,
        player_step: own.step,
        player_setup,
    })
}

/// What the server can reach where it runs.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SetupEnvironment {
    /// `linux`, `windows` or `macos`.
    pub os: String,
    /// `app` (inside `upnext app`), `service` (the systemd unit sets
    /// `UPNEXT_MODE=service`) or `serve`.
    pub mode: String,
    /// In UpNext's container image (`UPNEXT_CONTAINER=1`).
    pub container: bool,
    /// The Flatpak's app id (`io.github.zytocc.UpNext`) when UpNext runs in
    /// one, else null. Its sandbox reads Steam's and Heroic's usual folders
    /// and writes only to its own.
    pub flatpak: Option<String>,
    /// The server's computer has a desktop a window can open on: always in
    /// the app; on Linux a Wayland or X socket that answers (a service
    /// without the variables finds the session's Wayland socket); on Windows
    /// not for a service account or a service outside a sign-in; never in a
    /// container.
    pub display: bool,
    /// GOG's sign-in window can open: a desktop, and a build with windows
    /// (the `desktop` feature).
    pub gog_window: bool,
    /// The request came from the computer UpNext runs on, which needs no token.
    pub client_local: bool,
}

/// A local folder UpNext reads, as found.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct FolderFound {
    pub path: Option<String>,
    /// Why there's nothing: "not found", or "not mounted" in a container.
    pub missing: Option<String>,
    /// Reading it failed: why.
    pub error: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SteamFound {
    #[serde(flatten)]
    pub folder: FolderFound,
    /// The SteamID in use or detected, and where it came from
    /// (`app`, `env`, `steam client`).
    pub account: Option<String>,
    pub account_source: Option<String>,
    pub persona: Option<String>,
    /// Games the local client's caches list for this account, before any
    /// sync. For an account other than the one signed in to Steam last,
    /// only those it played or installed: Steam keeps that account's
    /// licences alone.
    pub local_games: Option<usize>,
    /// The Web API key and SteamID are both set.
    pub web_api: bool,
    /// Every account signed in on this computer's Steam client, the most
    /// recent first, with the player whose it is. A player other
    /// than the first is offered one nobody has.
    pub local_accounts: Vec<LocalSteamAccount>,
    /// Steam folders in other users' home folders on this computer, listed
    /// when this user has none and the request comes from this computer.
    /// Nothing in them is read; `UPNEXT_STEAM_DIR` names the one to read.
    pub other_users: Vec<String>,
}

/// An account signed in on this computer's Steam client. Another player's
/// shows only whose it is.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct LocalSteamAccount {
    /// Its SteamID, or null when it's another player's.
    pub steam_id: Option<String>,
    /// Its Steam name, or null when it's another player's.
    pub persona: Option<String>,
    /// The player whose Steam account it is, or null when it's nobody's yet.
    pub player: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct HeroicFound {
    #[serde(flatten)]
    pub folder: FolderFound,
    /// Games in each store's cache: `{"epic": 212, "gog": 30, "amazon": 4}`.
    pub games: BTreeMap<String, usize>,
    /// The player whose Heroic connection reads this folder, or null when
    /// it's nobody's. Heroic has one sign-in per user of the
    /// computer, so the first player's is the computer's own folder.
    pub owner: Option<String>,
    /// Heroic folders in other users' home folders on this computer that
    /// no player's connection reads yet, listed when this player has none
    /// of their own and the request comes from this computer. Nothing in
    /// them is read until the player's Heroic connection names one
    /// (`heroic_dir`, `PUT /connections/{id}`).
    pub other_users: Vec<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GalaxyFound {
    /// The Galaxy database set, or Galaxy's own place on Windows.
    pub path: Option<String>,
    pub exists: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GogFound {
    pub connected: bool,
    pub username: Option<String>,
}

/// One library a sync reads, and why the settings block it, if they do
/// (as `GET /sync/sources`).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SyncLibraryState {
    pub id: String,
    pub label: String,
    pub blocked: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct StoresFound {
    pub libraries: Vec<SyncLibraryState>,
    pub steam: SteamFound,
    pub heroic: HeroicFound,
    pub gog: GogFound,
    pub galaxy: GalaxyFound,
    pub epic: EpicFound,
    pub ea: InstallsFound,
    pub ubisoft: InstallsFound,
}

/// A launcher whose installed games the registry shows: the EA app or
/// Ubisoft Connect.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct InstallsFound {
    /// Its games installed on the computer UpNext runs on; 0 off Windows.
    pub installed: usize,
}

/// The Epic sign-in, and the Epic Games Launcher's installs on the computer
/// UpNext runs on.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct EpicFound {
    pub connected: bool,
    pub display_name: Option<String>,
    /// The launcher's manifests folder, when it's here.
    pub launcher_dir: Option<String>,
    /// Games the launcher installed, when it's here.
    pub installed: Option<usize>,
}

/// Which keys are set; never their values.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct KeysSet {
    pub steam_api_key: bool,
    pub steam_id: bool,
    pub rawg: bool,
    /// Both the Twitch client ID and its secret.
    pub igdb: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SetupAi {
    /// `claude-cli`, `codex-cli`, `api-key`, `openai-key` or `openai`.
    pub backend: String,
    /// The backend can run (the CLI starts, a key is set); no call is made.
    /// `POST /llm/test` makes one.
    pub available: bool,
    pub error: Option<String>,
    /// Where the Claude CLI is, whatever the backend, or null.
    pub claude: Option<String>,
    /// The server installing Claude Code (a container with
    /// `UPNEXT_INSTALL_CLAUDE=1`): installing now, or why it failed. Null
    /// when not asked for, and once installed (`claude` then says where).
    pub claude_install: Option<crate::llm::claude_install::InstallState>,
    /// `CLAUDE_CODE_OAUTH_TOKEN` is set for the server, from the environment
    /// or a `.env`: the sign-in a container's Claude Code uses (the token
    /// from `claude setup-token`). Never its value.
    pub claude_token: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct LibraryNow {
    pub titles: usize,
    pub copies: usize,
    /// Titles on two or more stores.
    pub multi_store: usize,
    /// Titles by store.
    pub by_source: BTreeMap<String, usize>,
    pub enriched: usize,
    pub unenriched: usize,
    /// About how long Fetch details takes for the titles without details.
    pub details_minutes: f64,
    pub last_sync: Option<i64>,
    /// The job running now, here or in a terminal.
    pub job: Option<crate::db::Job>,
}

/// The player running the wizard, for its "Who plays here?" step.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SetupYou {
    /// Their player id.
    pub id: i64,
    /// Their name now.
    pub name: String,
    /// The name is one UpNext made up ("Player 1"), not one anyone chose.
    pub placeholder: bool,
    /// A name to offer while theirs is made up: their Steam name, else, for
    /// a request from this computer, the user name UpNext runs under. Null
    /// when they have a name, nothing fits, or another player has it. It
    /// stays in the household, as every player's name does.
    pub suggested: Option<String>,
}

/// Everything the wizard reads.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Setup {
    pub environment: SetupEnvironment,
    pub stores: StoresFound,
    pub keys: KeysSet,
    pub ai: SetupAi,
    pub library: LibraryNow,
    pub setup: SetupState,
    pub you: SetupYou,
    /// What the wizard's background work waits on (`POST /setup/background`).
    pub background: super::setup_sync::SetupBackground,
}

#[utoipa::path(get, path = "/setup/state", tag = "setup",
    description = "Whether the first-run wizard shows (an empty library, setup neither finished nor skipped), the step it \
        reached, and where setup and the tour stand. Cheap: the UI asks it on every page.",
    responses((status = 200, body = SetupState)))]
async fn state_get(State(state): State<AppState>, who: Option<Extension<Who>>) -> ApiResult<SetupState> {
    // Only a request the guard didn't see has no `Who`; it acts for the
    // first player, as `Me` does.
    let p = who.map_or(Some(PlayerId::FIRST), |w| w.player);
    Ok(Json(setup_state(&state, p)?))
}

#[utoipa::path(get, path = "/setup", tag = "setup",
    description = "What the first-run wizard reads at every step: where UpNext runs, the libraries it found (with game \
        counts read from the launchers' caches, before any sync), which keys are set (never their values), the AI \
        backend and whether it can run (no call is made), the library and the running job, the wizard's state, and what \
        its background work waits on.",
    responses((status = 200, body = Setup)))]
async fn setup_get(State(state): State<AppState>, Me(me): Me, caller: Option<axum::Extension<access::Caller>>) -> ApiResult<Setup> {
    let cfg = state.cfg_for(me);
    let container = state.base_cfg.container;
    let mode = state.base_cfg.run_mode();
    let display = crate::display::detected(container, mode).await.available;
    let environment = SetupEnvironment {
        os: std::env::consts::OS.into(),
        mode: mode.into(),
        container,
        flatpak: crate::flatpak::get().map(|f| f.id.clone()),
        display,
        gog_window: display && cfg!(feature = "desktop"),
        client_local: caller.is_some_and(|c| c.is_local()),
    };

    let libraries = crate::enrich::sync_libraries()
        .iter()
        .map(|l| SyncLibraryState {
            id: l.id().into(),
            label: l.label().into(),
            blocked: l.blocked(&cfg, &state.clients).map(str::to_string),
        })
        .collect();
    let read_cfg = cfg.clone();
    let whose = LocalOwners::of(&state, me)?;
    let local = environment.client_local;
    let (steam, heroic) = tokio::task::spawn_blocking(move || {
        // Other users' folders are named only to someone at this computer,
        // who alone can choose one (`heroic_dir` is set only from here).
        let homes = if local && !container { crate::other_users::homes(read_cfg.users_dirs.as_deref()) } else { vec![] };
        (find_steam(&read_cfg, container, &whose, &homes), find_heroic(&read_cfg, container, &whose, &homes))
    })
    .await
    .map_err(anyhow::Error::from)?;
    let gog = state.gog_for(me).status();
    let galaxy = GalaxyFound {
        path: cfg.galaxy_db.as_ref().map(|p| p.display().to_string()),
        exists: cfg.galaxy_db.as_ref().is_some_and(|p| p.is_file()),
    };
    let epic_status = state.epic_for(me).status();
    let epic_dir = crate::epic::find_manifests_dir(cfg.epic_manifests.as_deref());
    let epic = EpicFound {
        connected: epic_status.connected,
        display_name: epic_status.display_name,
        launcher_dir: epic_dir.as_ref().map(|d| d.display().to_string()),
        installed: epic_dir.and_then(|d| crate::epic::read_manifests(&d).ok()).map(|g| g.len()),
    };
    let apps = crate::installs::uninstall_entries();
    let ea = InstallsFound { installed: crate::ea::installed(&apps).len() };
    let ubisoft_keys = crate::installs::machine_subkey_values(crate::ubisoft::INSTALLS_KEY, "InstallDir");
    let ubisoft = InstallsFound { installed: crate::ubisoft::installed(&ubisoft_keys, &apps).len() };
    let stores = StoresFound {
        libraries,
        steam,
        heroic,
        gog: GogFound { connected: gog.connected, username: gog.username },
        galaxy,
        epic,
        ea,
        ubisoft,
    };

    let keys = KeysSet {
        steam_api_key: cfg.steam_api_key.is_some(),
        steam_id: cfg.steam_id.is_some(),
        rawg: cfg.rawg_api_key.is_some(),
        igdb: cfg.igdb_client_id.is_some() && cfg.igdb_client_secret.is_some(),
    };

    let error = state.llm_for(me).available().await.err().map(|e| e.reason());
    let ai = SetupAi {
        backend: cfg.llm_backend.clone(),
        available: error.is_none(),
        error,
        claude: crate::config::find_program(&cfg.claude_bin).map(|(p, _)| p.display().to_string()),
        claude_install: state.claude_install.now(),
        claude_token: std::env::var_os("CLAUDE_CODE_OAUTH_TOKEN").is_some_and(|v| !v.is_empty()),
    };

    let stats = state.db.stats(me)?;
    let names = state.db.hidden_names(me)?;
    let library = LibraryNow {
        titles: stats.total,
        copies: stats.copies,
        multi_store: stats.multi_store,
        by_source: stats.by_source.into_iter().collect(),
        enriched: stats.enriched,
        unenriched: stats.unenriched,
        details_minutes: (stats.unenriched as f64 * DETAILS_SECONDS_PER_TITLE / 60.0 * 10.0).round() / 10.0,
        last_sync: state.db.meta_get("last_sync")?.and_then(|v| v.parse().ok()),
        job: state.db.job_running()?.map(|mut j| {
            names.job(&mut j);
            j
        }),
    };

    let you = you(&state, me, &stores.steam, environment.client_local.then_some(cfg.login_name.as_deref()).flatten())?;
    let background = state.setup_sync.now();
    Ok(Json(Setup { environment, stores, keys, ai, library, setup: setup_state(&state, Some(me))?, you, background }))
}

/// Player `me`'s name, and the one to offer while theirs is made up: the
/// persona of the Steam account setup found for them, the first player's
/// Steam profile from a sync, then `login`.
fn you(state: &AppState, me: PlayerId, steam: &SteamFound, login: Option<&str>) -> anyhow::Result<SetupYou> {
    let players = state.db.players()?;
    let name = players.iter().find(|p| p.id == me).map(|p| p.name.clone()).unwrap_or_default();
    let placeholder = crate::db::placeholder_name(&name);
    let synced = || -> anyhow::Result<Option<String>> {
        if me != PlayerId::FIRST {
            return Ok(None);
        }
        let profile = state.db.meta_get("profile")?.and_then(|s| serde_json::from_str::<crate::db::Profile>(&s).ok());
        Ok(profile.and_then(|p| p.persona_name))
    };
    let fits = |n: &String| {
        !n.is_empty()
            && n.chars().count() <= 40
            && !crate::db::placeholder_name(n)
            && !players.iter().any(|p| p.id != me && p.name.to_lowercase() == n.to_lowercase())
    };
    let suggested = if placeholder {
        [steam.persona.clone(), synced()?, login.map(str::to_string)].into_iter().flatten().map(|n| crate::db::clean_name(&n)).find(fits)
    } else {
        None
    };
    Ok(SetupYou { id: me.0, name, placeholder, suggested })
}

/// A change to the wizard's state; every field is optional.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SetupChange {
    /// The step reached, one of welcome, where, steam, stores, more_stores,
    /// extras, ai, ai_jobs, sync, details, taste, players, done.
    pub step: Option<String>,
    /// `done` or `skipped` ends the wizard; `open` starts it over.
    pub setup: Option<SetupProgress>,
    /// `done` or `skipped` ends the tour's offer; `open` makes it again.
    /// The request's player's own.
    pub tour: Option<SetupProgress>,
    /// The step of the request's player's own setup they reached, one of
    /// you, games, ai, taste, done.
    pub player_step: Option<String>,
    /// `done` or `skipped` ends the request's player's own setup; `open`
    /// runs it again.
    pub player_setup: Option<SetupProgress>,
}

#[utoipa::path(put, path = "/setup", tag = "setup",
    description = "Record the step the wizard reached, and that setup or the tour is done, skipped or open again; and \
        the same for the request's player's own short setup. Answers as GET /setup/state.",
    request_body = SetupChange,
    responses((status = 200, body = SetupState), (status = 400, description = "An unknown step"),
        (status = 403, description = "The household's wizard from a token bound to one player")))]
async fn setup_put(
    State(state): State<AppState>,
    Me(me): Me,
    who: Option<Extension<Who>>,
    Json(change): Json<SetupChange>,
) -> ApiResult<SetupState> {
    let bound = who.is_some_and(|w| w.bound);
    if bound && (change.step.is_some() || change.setup.is_some()) {
        return Err(ApiError::forbidden("this token is bound to one player, so it can't change everyone's setup"));
    }
    let mut s = stored(&state)?;
    let mut own = player_stored(&state.db, me)?;
    if let Some(step) = change.step {
        if !STEPS.contains(&step.as_str()) {
            return Err(ApiError::bad(format!("no step {step:?}; the steps are {}", STEPS.join(", "))));
        }
        s.step = Some(step);
    }
    if let Some(step) = change.player_step {
        if !PLAYER_STEPS.contains(&step.as_str()) {
            return Err(ApiError::bad(format!("no step {step:?}; a player's steps are {}", PLAYER_STEPS.join(", "))));
        }
        own.step = Some(step);
    }
    if let Some(p) = change.setup {
        s.setup = p;
        // Leaving the wizard gives the schedules back their say on Fetch details.
        if p != SetupProgress::Open {
            state.setup_sync.release();
            // Whoever went through the household's wizard has been through
            // their own setup too.
            own.setup = Some(p);
        }
    }
    if let Some(p) = change.player_setup {
        own.setup = Some(p);
    }
    if let Some(p) = change.tour {
        own.tour = Some(p);
    }
    state.db.setting_set(PlayerId::FIRST, KEY, Some(&serde_json::to_string(&s).map_err(anyhow::Error::from)?))?;
    player_store(&state.db, me, &own)?;
    Ok(Json(setup_state(&state, Some(me))?))
}

/// What `POST /setup/reset` clears before the wizard starts again.
/// The wizard itself never deletes anything; this is the one way to start
/// over, and a backup is taken first. With two or more players, the taste
/// profile and the history are the request's player's own; the
/// whole library is everyone's.
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
pub struct ResetBody {
    /// Store sign-ins and keys: Steam's Web API key and SteamID (and the
    /// Steam name and picture shown for it), IGDB's and RAWG's keys, the
    /// Galaxy file, and the request's player's GOG and Epic sign-ins. With
    /// two or more players, only the request player's own: their SteamID,
    /// Galaxy file and sign-ins; the keys that are everyone's stay.
    #[serde(default)]
    pub accounts: bool,
    /// The AI choice: the backend, its models, keys, server address and
    /// thinking setting.
    #[serde(default)]
    pub ai: bool,
    /// The taste profile: what you told it, the patterns it found, and the
    /// play diary.
    #[serde(default)]
    pub taste: bool,
    /// Statuses and history: the statuses and star ratings you set, why you
    /// stopped, Play later, Second Chance answers, "This game can be played
    /// endlessly", and every lineup with what you did with its picks and
    /// how you rated them.
    #[serde(default)]
    pub history: bool,
    /// The whole library: every game, what was fetched and written about
    /// it, and the wishlist; the taste profile and the history go with it.
    #[serde(default)]
    pub library: bool,
}

/// The settings each part clears. The rest (schedules, looks, the
/// thresholds statuses follow) are how UpNext runs, not who you are.
const ACCOUNT_SETTINGS: &[&str] = &["steam_api_key", "steam_id", "igdb_client_id", "igdb_client_secret", "rawg_api_key", "galaxy_db"];
const AI_SETTINGS: &[&str] = &[
    "llm_backend",
    "digest_backend",
    "model",
    "digest_model",
    "codex_model",
    "codex_digest_model",
    "codex_effort",
    "anthropic_api_key",
    "openai_key",
    "openai_key_model",
    "openai_key_digest_model",
    "openai_key_effort",
    "openai_base_url",
    "openai_model",
    "openai_thinking",
    "openai_digest_model",
    "openai_api_key",
];

/// What the reset did.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ResetDone {
    /// The backup taken first, as Settings › Backups lists it: the way back.
    pub backup: String,
    /// Settings that still have a value from the environment or a `.env`
    /// file, which the app can't clear.
    pub from_env: Vec<String>,
}

#[utoipa::path(post, path = "/setup/reset", tag = "setup",
    description = "Clear what's chosen before the setup wizard starts again: sign-ins and keys, the AI choice, the \
        taste profile, statuses and history, or the whole library. A backup kept until it's deleted is taken first. \
        Only on the computer UpNext runs on.",
    request_body = ResetBody,
    responses((status = 200, body = ResetDone),
        (status = 400, description = "Nothing chosen to clear"),
        (status = 403, description = "From another device"),
        (status = 409, description = "A job is running")))]
async fn reset(
    State(state): State<AppState>,
    Me(me): Me,
    caller: Option<axum::Extension<access::Caller>>,
    Json(body): Json<ResetBody>,
) -> ApiResult<ResetDone> {
    // No caller means the guard didn't run: refuse, as for every local-only route.
    if !caller.is_some_and(|c| c.is_local()) {
        return Err(ApiError::forbidden("what setup clears is chosen only on the computer UpNext runs on"));
    }
    let ResetBody { accounts, ai, taste, history, library } = body;
    let mut cleared: Vec<&str> = vec![];
    for (on, what) in [
        (accounts, "sign-ins and keys"),
        (ai, "the AI choice"),
        (taste, "the taste profile"),
        (history, "statuses and history"),
        (library, "the whole library"),
    ] {
        if on {
            cleared.push(what);
        }
    }
    if cleared.is_empty() {
        return Err(ApiError::bad("choose something to clear, or start the wizard without clearing anything"));
    }
    if let Some(running) = state.db.job_running()? {
        return Err(ApiError::conflict(format!("{}; wait for it or cancel it", crate::jobs::busy_text(&running))));
    }
    let cfg = state.cfg_for(me);
    let note = format!("Before restarting setup, which cleared {}", cleared.join(", "));
    let backup = {
        let cfg = cfg.clone();
        tokio::task::spawn_blocking(move || crate::jobs::backup::manual(&cfg, Some(&note))).await.map_err(anyhow::Error::from)??
    };
    // With other players here, the keys that are everyone's (Steam's Web
    // API key, IGDB's and RAWG's) stay: the asker clears only their own
    // SteamID, Galaxy file and sign-ins. The Steam name and
    // picture shown in the top bar are the first player's Steam account's.
    let others = state.db.players()?.len() > 1;
    let steam_profile = accounts && (!others || me == PlayerId::FIRST);
    let mut parts = crate::db::ResetParts { taste, history, library, steam_profile, settings: vec![], player: me };
    if accounts {
        parts.settings.extend(ACCOUNT_SETTINGS.iter().filter(|k| !others || !crate::db::household_setting(k)));
    }
    if ai {
        parts.settings.extend(AI_SETTINGS);
    }
    state.db.reset(&parts)?;
    // GOG and Epic are signed out only once the database has been cleared,
    // so a failed clear leaves everything as it was, the sign-ins included.
    if accounts {
        state.gog_for(me).disconnect().await?;
        state.epic_for(me).disconnect().await?;
    }
    if taste || history || library {
        crate::taste::update(&state.db, me, &cfg.taste_rules())?;
    }
    let cfg = state.cfg_for(me);
    let from_env = parts.settings.iter().filter(|k| cfg.source(k) == "env").map(|k| k.to_string()).collect();
    Ok(Json(ResetDone { backup: backup.name, from_env }))
}

fn missing(container: bool) -> Option<String> {
    Some(if container { "not mounted" } else { "not found" }.into())
}

/// Whose this computer's Steam accounts and Heroic folders are:
/// what each player's connections name, and the first player's first
/// connections, which read this computer's own when they name nothing.
pub(crate) struct LocalOwners {
    me: PlayerId,
    /// SteamIDs linked by a player, with whose they are.
    steam: Vec<(String, PlayerId, String)>,
    /// The first player, with their name, when their Steam reads the client's most
    /// recent account (no SteamID of their own).
    steam_first_reads_client: Option<(PlayerId, String)>,
    /// Heroic folders read by a player's connection, with their name.
    heroic: Vec<(std::path::PathBuf, String)>,
    /// The folder the first player's Heroic reads: this computer's own,
    /// unless they named another.
    heroic_first: Option<std::path::PathBuf>,
    /// One of this player's Heroic connections reads a folder.
    heroic_mine: bool,
}

impl LocalOwners {
    pub(crate) fn of(state: &AppState, me: PlayerId) -> anyhow::Result<LocalOwners> {
        let players = state.db.players()?;
        let name = |p: PlayerId| players.iter().find(|x| x.id == p).map(|x| x.name.clone()).unwrap_or_default();
        let first = state.cfg_for(PlayerId::FIRST);
        let mut owners =
            LocalOwners { me, steam: vec![], steam_first_reads_client: None, heroic: vec![], heroic_first: None, heroic_mine: false };
        for conn in state.db.sync_connections("steam")? {
            let set = conn.setting("steam_id").map(str::to_string);
            match set.or_else(|| conn.first.then(|| first.steam_id.clone()).flatten()) {
                Some(id) => owners.steam.push((id, conn.owner, name(conn.owner))),
                None if conn.first => owners.steam_first_reads_client = Some((conn.owner, name(conn.owner))),
                None => {}
            }
        }
        for conn in state.db.sync_connections("heroic")? {
            let set = conn.setting("heroic_dir").map(std::path::PathBuf::from);
            // Only the first player's first connection falls back to this
            // computer's own folder, as sync reads it.
            let dir = match (set, conn.first) {
                (Some(d), _) => Some(d).filter(|d| d.join("store_cache").is_dir()),
                (None, true) => crate::heroic::find_config_dir(first.heroic_dir.as_deref()),
                (None, false) => None,
            };
            if let Some(d) = dir {
                if conn.first {
                    owners.heroic_first = Some(d.clone());
                }
                owners.heroic_mine |= conn.owner == me;
                owners.heroic.push((d, name(conn.owner)));
            }
        }
        Ok(owners)
    }

    /// The player whose Steam account `id` is, if anyone's.
    fn steam_owner(&self, id: &str, most_recent: Option<&str>) -> Option<(PlayerId, String)> {
        self.steam
            .iter()
            .find(|(s, _, _)| s == id)
            .map(|(_, p, n)| (*p, n.clone()))
            .or_else(|| self.steam_first_reads_client.clone().filter(|_| most_recent == Some(id)))
    }
}

fn find_steam(cfg: &Config, container: bool, whose: &LocalOwners, homes: &[std::path::PathBuf]) -> SteamFound {
    let dir = crate::steam_local::find_steam_dir(cfg.steam_dir.as_deref());
    let users = crate::steam::local_users(dir.as_deref());
    let recent = users.first().map(|u| u.steam_id.as_str());
    // Another player's account is named by whose it is alone: its SteamID
    // and persona are theirs to show, not every caller's.
    let local_accounts: Vec<LocalSteamAccount> = users
        .iter()
        .map(|u| match whose.steam_owner(&u.steam_id, recent) {
            Some((p, name)) if p != whose.me => LocalSteamAccount { steam_id: None, persona: None, player: Some(name) },
            owner => {
                LocalSteamAccount { steam_id: Some(u.steam_id.clone()), persona: u.persona.clone(), player: owner.map(|(_, name)| name) }
            }
        })
        .collect();
    // The first player reads the client's most recent account; any other
    // player is offered the first one nobody has.
    let detected = if whose.me == PlayerId::FIRST {
        crate::steam::detect_steam_id(dir.as_deref())
    } else {
        local_accounts.iter().find(|a| a.player.is_none()).and_then(|a| a.steam_id.clone())
    };
    let account = cfg.steam_id.clone().or(detected);
    let account_source =
        cfg.steam_id.as_ref().map(|_| cfg.source("steam_id").to_string()).or(account.as_ref().map(|_| "steam client".into()));
    let persona = match users.iter().find(|u| Some(&u.steam_id) == account.as_ref()) {
        Some(u) => u.persona.clone(),
        None if whose.me == PlayerId::FIRST => crate::steam::detect_persona_name(dir.as_deref()),
        None => None,
    };
    let (local_games, error) = match &dir {
        Some(d) => match crate::steam_local::read_library(d, account.as_deref()) {
            Ok(l) => (Some(l.games.len()), None),
            Err(e) => (None, Some(format!("{e:#}"))),
        },
        None => (None, None),
    };
    SteamFound {
        folder: FolderFound {
            missing: if dir.is_none() { missing(container) } else { None },
            path: dir.as_ref().map(|d| d.display().to_string()),
            error,
        },
        persona,
        account,
        account_source,
        local_games,
        web_api: cfg.steam_credentials().is_ok(),
        local_accounts,
        other_users: if dir.is_none() { folders(crate::steam_local::steam_dirs_in(homes)) } else { vec![] },
    }
}

fn find_heroic(cfg: &Config, container: bool, whose: &LocalOwners, homes: &[std::path::PathBuf]) -> HeroicFound {
    // A player other than the first sees the folder their own connection
    // names, else this computer's own, which the first player's reads, with
    // whose it is.
    let dir = match (&cfg.heroic_dir, whose.me == PlayerId::FIRST) {
        (None, false) => whose.heroic_first.clone().or_else(|| crate::heroic::find_config_dir(None)),
        (own, _) => crate::heroic::find_config_dir(own.as_deref()),
    };
    let owner = dir.as_ref().and_then(|d| whose.heroic.iter().find(|(h, _)| same_folder(h, d)).map(|(_, n)| n.clone()));
    let (games, error) = match &dir {
        Some(d) => match crate::heroic::read_libraries(d) {
            Ok(libs) => {
                let error = (!libs.unread.is_empty()).then(|| libs.unread.join("; "));
                (libs.stores.into_iter().map(|(source, g)| (source, g.len())).collect(), error)
            }
            Err(e) => (BTreeMap::new(), Some(format!("{e:#}"))),
        },
        None => (BTreeMap::new(), None),
    };
    HeroicFound {
        folder: FolderFound {
            missing: if dir.is_none() { missing(container) } else { None },
            path: dir.as_ref().map(|d| d.display().to_string()),
            error,
        },
        games,
        owner,
        // A folder some player's connection reads is theirs already.
        other_users: if whose.heroic_mine {
            vec![]
        } else {
            folders(crate::heroic::config_dirs_in(homes).into_iter().filter(|d| !whose.heroic.iter().any(|(h, _)| same_folder(h, d))))
        },
    }
}

fn folders(dirs: impl IntoIterator<Item = std::path::PathBuf>) -> Vec<String> {
    dirs.into_iter().map(|d| d.display().to_string()).collect()
}

fn same_folder(a: &std::path::Path, b: &std::path::Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
}

/// A Galaxy database taken in.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GalaxyUploaded {
    /// Where it's kept, which `galaxy_db` now names.
    pub path: String,
    /// Games in it, as a sync would read them.
    pub games: usize,
}

/// One Galaxy upload at a time: each can write up to `MAX_GALAXY`, so
/// several side by side could fill the disk. A second waits for the first,
/// up to `TURN_WAIT`.
static TAKING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// How long an upload waits for the one before it to finish before it's
/// turned away with a 409, so a stuck upload can't keep the next one
/// waiting with no answer.
const TURN_WAIT: Duration = Duration::from_secs(30);

/// How long an upload may go without a byte arriving before it's given up
/// with a 408. The server has no read timeout of its own, so without this a
/// client that stops sending would hold `TAKING` until its connection died.
const STALL: Duration = Duration::from_secs(60);

/// How long a whole upload may take before it's given up with a 408. A
/// client that sends a little at a time never trips `STALL`, and would hold
/// `TAKING` for as long as it kept dripping. Galaxy's database is tens of
/// megabytes, so this is generous even for the 1 GB limit on a slow link.
const DEADLINE: Duration = Duration::from_secs(15 * 60);

/// The start of an upload's work file's name, beside the database.
const WORK_PREFIX: &str = ".galaxy-upload-";

/// An upload's work file, removed however the request ends. A client that
/// disconnects midway makes axum drop the handler, so a removal after the
/// upload's last `await` would never run and leave up to 1 GB behind.
struct WorkFile(PathBuf);

impl Drop for WorkFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Remove work files an upload left behind (a crash, or a version before
/// `WorkFile`). The server does this when it starts, and each upload does it
/// while it holds `TAKING`, when no other upload can be writing one.
pub fn sweep_galaxy_uploads(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(WORK_PREFIX) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Where an upload is kept: `galaxy-2.0.db` beside the database.
pub fn galaxy_upload_path(cfg: &Config) -> PathBuf {
    // Each player's Galaxy connection has its own copy; player 1's
    // keeps the name it always had.
    match cfg.whose() {
        PlayerId::FIRST => cfg.db_dir().join("galaxy-2.0.db"),
        p => cfg.db_dir().join(format!("galaxy-2.0.player-{}.db", p.0)),
    }
}

#[utoipa::path(post, path = "/galaxy/upload", tag = "accounts",
    description = "Take a copy of GOG Galaxy's `galaxy-2.0.db` from another computer (up to 1 GB, the file as the \
        request's body). It must open as SQLite with Galaxy's tables; it's kept as `galaxy-2.0.db` beside UpNext's \
        database, replacing an earlier upload, and `galaxy_db` is set to it, so the next sync reads it.",
    request_body(content = Vec<u8>, content_type = "application/octet-stream", description = "galaxy-2.0.db"),
    responses((status = 200, body = GalaxyUploaded),
        (status = 400, description = "Empty, not SQLite, or not a Galaxy database, with why"),
        (status = 408, description = "Nothing arrived for a minute, or the whole upload took over 15 minutes"),
        (status = 409, description = "Another upload is still running, or the earlier upload is in use (a sync reading it)"),
        (status = 413, description = "Over 1 GB")))]
async fn galaxy_upload(State(state): State<AppState>, Me(me): Me, headers: HeaderMap, body: Body) -> ApiResult<GalaxyUploaded> {
    let declared = headers.get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
    if declared.is_some_and(|n| n > MAX_GALAXY) {
        return Err(too_big());
    }
    let cfg = state.cfg_for(me);
    let to = galaxy_upload_path(&cfg);
    let dir = to.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    std::fs::create_dir_all(&dir).map_err(anyhow::Error::from)?;
    let _one = turn(&TAKING, TURN_WAIT).await?;
    sweep_galaxy_uploads(&dir);
    let work = WorkFile(dir.join(format!("{WORK_PREFIX}{}.db", uuid::Uuid::new_v4().simple())));
    let games = take_galaxy(body, &work.0, &to, MAX_GALAXY, STALL, DEADLINE).await?;
    let path = to.display().to_string();
    state.db.setting_set(me, "galaxy_db", Some(&path))?;
    Ok(Json(GalaxyUploaded { path, games }))
}

/// The upload's turn: `lock` once the upload before it is done, or a 409
/// after `wait`.
async fn turn(lock: &tokio::sync::Mutex<()>, wait: Duration) -> Result<tokio::sync::MutexGuard<'_, ()>, ApiError> {
    tokio::time::timeout(wait, lock.lock())
        .await
        .map_err(|_| ApiError::conflict("another Galaxy upload is still running; try again when it's done"))
}

fn too_big() -> ApiError {
    ApiError(StatusCode::PAYLOAD_TOO_LARGE, format!("that file is over {} GB; Galaxy's database is far smaller", MAX_GALAXY >> 30))
}

/// Write the body to `work` (at most `max` bytes, with no gap longer than
/// `stall` and all of it within `deadline`), read it as Galaxy's database,
/// then move it to `to`. The number of games in it.
async fn take_galaxy(body: Body, work: &Path, to: &Path, max: u64, stall: Duration, deadline: Duration) -> Result<usize, ApiError> {
    use tokio::time::Instant;
    let mut file = tokio::fs::File::create(work).await.map_err(anyhow::Error::from)?;
    let mut stream = body.into_data_stream();
    let mut size = 0u64;
    let end = Instant::now() + deadline;
    let gave_up = || {
        let why = if Instant::now() >= end {
            let minutes = deadline.as_millis().div_ceil(60_000);
            let unit = if minutes == 1 { "minute" } else { "minutes" };
            format!("the upload took over {minutes} {unit}; try again on a faster connection")
        } else {
            "the upload stopped arriving; try again".into()
        };
        ApiError(StatusCode::REQUEST_TIMEOUT, why)
    };
    loop {
        let next = tokio::time::timeout_at(end.min(Instant::now() + stall), stream.next()).await.map_err(|_| gave_up())?;
        let Some(chunk) = next else { break };
        // The timeout above never fires while chunks are always ready, since
        // it polls the stream first, so the deadline is checked here too.
        if Instant::now() >= end {
            return Err(gave_up());
        }
        let chunk = chunk.map_err(|e| ApiError::bad(format!("the upload broke off: {e}")))?;
        size += chunk.len() as u64;
        if size > max {
            return Err(too_big());
        }
        file.write_all(&chunk).await.map_err(anyhow::Error::from)?;
    }
    file.flush().await.map_err(anyhow::Error::from)?;
    drop(file);
    if size == 0 {
        return Err(ApiError::bad("the upload was empty; choose galaxy-2.0.db"));
    }
    let check = work.to_path_buf();
    let read = tokio::task::spawn_blocking(move || crate::galaxy::read_library(&check, &[])).await.map_err(anyhow::Error::from)?;
    let lib = read.map_err(|e| ApiError::bad(format!("that isn't a GOG Galaxy database: {}", root_cause(&e))))?;
    replace_galaxy(work, to, REPLACE_TRIES, REPLACE_WAIT).await?;
    Ok(lib.games.len())
}

/// How many times a checked upload tries to take the earlier one's place,
/// and how long it waits between tries.
const REPLACE_TRIES: u32 = 5;
const REPLACE_WAIT: Duration = Duration::from_millis(400);

/// Move the checked upload over the earlier one. On Windows that fails
/// while something holds the earlier one open without letting it be
/// deleted, as SQLite does during a sync, and the browser was shown a 500
/// with "Access is denied. (os error 5)" (Windows retest). A hold that ends
/// soon is waited out; a longer one is a 409 that says what's in the way.
/// Any other failure (a full disk, a folder UpNext can't write to) is a 500
/// at once that names the folder. The system's own words go to the log.
async fn replace_galaxy(work: &Path, to: &Path, tries: u32, wait: Duration) -> Result<(), ApiError> {
    replace_galaxy_with(work, to, tries, wait, |from, to| std::fs::rename(from, to)).await
}

/// `replace_galaxy`, with the rename to use, so a test can hold the file.
async fn replace_galaxy_with(
    work: &Path,
    to: &Path,
    tries: u32,
    wait: Duration,
    rename: impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> Result<(), ApiError> {
    for left in (0..tries.max(1)).rev() {
        match rename(work, to) {
            Ok(()) => return Ok(()),
            Err(e) if !held(&e) => {
                tracing::warn!("couldn't replace {}: {e}", to.display());
                let dir = to.parent().unwrap_or(to);
                return Err(ApiError(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("UpNext couldn't save the Galaxy database in {}; its log says why", dir.display()),
                ));
            }
            Err(e) if left > 0 => {
                tracing::debug!("couldn't replace {} yet: {e}", to.display());
                tokio::time::sleep(wait).await;
            }
            Err(e) => tracing::warn!("couldn't replace {}: {e}", to.display()),
        }
    }
    Err(ApiError::conflict("the Galaxy database UpNext keeps is in use, most likely by a sync reading it; try again once the sync is done"))
}

/// Whether a rename failed because something holds the file. On Windows a
/// sharing or lock violation, and "access denied", which is what replacing
/// a file open without delete sharing gives (the retest's os error 5); that
/// also covers a folder UpNext may not write to, which then waits its 1.6
/// seconds before the 409. Elsewhere a busy file.
fn held(e: &std::io::Error) -> bool {
    #[cfg(windows)]
    const HELD: &[i32] = &[5, 32, 33]; // ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION, ERROR_LOCK_VIOLATION
    #[cfg(not(windows))]
    const HELD: &[i32] = &[16, 26]; // EBUSY, ETXTBSY
    e.raw_os_error().is_some_and(|code| HELD.contains(&code))
}

/// The innermost reason, without the temporary file's name.
fn root_cause(e: &anyhow::Error) -> String {
    let text = e.root_cause().to_string();
    match text.split_once(" does not look like a Galaxy 2.0 database ") {
        Some((_, why)) => why.trim_matches(['(', ')']).to_string(),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owners(me: i64, heroic: Vec<(std::path::PathBuf, String)>, heroic_mine: bool) -> LocalOwners {
        LocalOwners { me: PlayerId(me), steam: vec![], steam_first_reads_client: None, heroic, heroic_first: None, heroic_mine }
    }

    /// Other users' Heroic folders are offered to a player with none of
    /// their own, except one a player's connection reads already.
    #[test]
    fn other_users_heroic_folders_are_offered_to_a_player_without_one() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = crate::testutil::offline_config(dir.path());
        let (sam, alex, kit) = (dir.path().join("home/sam"), dir.path().join("home/alex"), dir.path().join("home/kit"));
        let sams = sam.join(".config/heroic");
        let alexs = alex.join(".var/app/com.heroicgameslauncher.hgl/config/heroic");
        for d in [&sams, &alexs] {
            std::fs::create_dir_all(d.join("store_cache")).unwrap();
        }
        // Kit's has no store cache, so it isn't Heroic's.
        std::fs::create_dir_all(kit.join(".config/heroic")).unwrap();
        let homes = [sam, alex, kit];
        let alex_reads = vec![(alexs.clone(), "Alex".to_string())];

        let found = find_heroic(&cfg, false, &owners(2, alex_reads.clone(), false), &homes);
        assert_eq!(found.other_users, [sams.display().to_string()]);
        assert!(found.games.is_empty(), "nothing in it is read");
        assert!(find_heroic(&cfg, false, &owners(2, alex_reads, true), &homes).other_users.is_empty(), "they have their own");
        assert!(find_heroic(&cfg, false, &owners(2, vec![], false), &[]).other_users.is_empty(), "no homes, as for another device");
    }

    /// Other users' Steam folders are listed only when this user's own
    /// Steam isn't found.
    #[test]
    fn other_users_steam_folders_are_listed_when_steam_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = crate::testutil::offline_config(dir.path());
        let sam = dir.path().join("home/sam");
        let steam = sam.join(".local/share/Steam");
        std::fs::create_dir_all(steam.join("config")).unwrap();
        std::fs::create_dir_all(steam.join("appcache")).unwrap();
        std::fs::write(steam.join("config/loginusers.vdf"), crate::testutil::fixture("steam/loginusers.vdf")).unwrap();
        std::fs::write(steam.join("appcache/appinfo.vdf"), "").unwrap();
        let homes = [sam, dir.path().join("home/alex")];

        let found = find_steam(&cfg, false, &owners(1, vec![], false), &homes);
        assert_eq!((found.folder.path, found.other_users), (None, vec![steam.display().to_string()]));
        assert!(found.local_accounts.is_empty(), "nothing in it is read");
        cfg.steam_dir = Some(steam.clone());
        assert!(find_steam(&cfg, false, &owners(1, vec![], false), &homes).other_users.is_empty(), "Steam is found");
    }

    #[tokio::test]
    async fn an_upload_over_the_limit_stops_at_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let (work, to) = (dir.path().join("work.db"), dir.path().join("galaxy-2.0.db"));
        let Err(e) = take_galaxy(Body::from(vec![0u8; 2048]), &work, &to, 1024, STALL, DEADLINE).await else { panic!("taken") };
        assert_eq!(e.0, StatusCode::PAYLOAD_TOO_LARGE);
        assert!(!to.exists());
    }

    #[tokio::test]
    async fn an_upload_that_stops_arriving_is_given_up() {
        let dir = tempfile::tempdir().unwrap();
        let (work, to) = (dir.path().join("work.db"), dir.path().join("galaxy-2.0.db"));
        let first = futures::stream::once(async { Ok::<_, std::io::Error>(axum::body::Bytes::from(vec![0u8; 1024])) });
        let body = Body::from_stream(first.chain(futures::stream::pending()));
        let Err(e) = take_galaxy(body, &work, &to, MAX_GALAXY, Duration::from_millis(100), DEADLINE).await else { panic!("taken") };
        assert_eq!((e.0, e.1.as_str()), (StatusCode::REQUEST_TIMEOUT, "the upload stopped arriving; try again"));
        assert!(!to.exists());
    }

    /// A byte at a time, never stopping long enough to stall, is given up
    /// at the deadline (security review L6).
    #[tokio::test]
    async fn an_upload_that_drips_is_given_up_at_the_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let (work, to) = (dir.path().join("work.db"), dir.path().join("galaxy-2.0.db"));
        let drip = futures::stream::unfold((), |()| async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            Some((Ok::<_, std::io::Error>(axum::body::Bytes::from_static(b"x")), ()))
        });
        let started = std::time::Instant::now();
        let deadline = Duration::from_millis(300);
        let Err(e) = take_galaxy(Body::from_stream(drip), &work, &to, MAX_GALAXY, Duration::from_millis(100), deadline).await else {
            panic!("taken")
        };
        assert_eq!(e.0, StatusCode::REQUEST_TIMEOUT);
        assert_eq!(e.1, "the upload took over 1 minute; try again on a faster connection");
        assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
        assert!(!to.exists());
    }

    /// A sender with a chunk always ready is given up at the deadline too.
    #[tokio::test]
    async fn an_upload_that_never_pauses_is_given_up_at_the_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let (work, to) = (dir.path().join("work.db"), dir.path().join("galaxy-2.0.db"));
        let flood = futures::stream::repeat_with(|| Ok::<_, std::io::Error>(axum::body::Bytes::from_static(b"x")));
        let started = std::time::Instant::now();
        let deadline = Duration::from_millis(300);
        let Err(e) = take_galaxy(Body::from_stream(flood), &work, &to, MAX_GALAXY, STALL, deadline).await else { panic!("taken") };
        assert_eq!((e.0, e.1.as_str()), (StatusCode::REQUEST_TIMEOUT, "the upload took over 1 minute; try again on a faster connection"));
        assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
        assert!(!to.exists());
    }

    /// An earlier upload that stays in use is tried a few times, then
    /// turned down with a 409 that says why, not a 500 with the system's
    /// text, and the checked upload is left for the work file's cleanup; one
    /// that frees up is replaced. Any other failure (here a full disk) is a
    /// 500 at once that names the folder.
    #[tokio::test]
    async fn an_earlier_upload_in_use_is_a_409_that_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let (work, to) = (dir.path().join("work.db"), dir.path().join("galaxy-2.0.db"));
        std::fs::write(&work, b"new").unwrap();
        let busy = || std::io::Error::from_raw_os_error(if cfg!(windows) { 32 } else { 16 });
        let wait = Duration::from_millis(20);

        let tried = std::cell::Cell::new(0);
        let start = std::time::Instant::now();
        let held = |_: &Path, _: &Path| {
            tried.set(tried.get() + 1);
            Err(busy())
        };
        let Err(e) = replace_galaxy_with(&work, &to, 3, wait, held).await else { panic!("replaced") };
        assert_eq!(tried.get(), 3);
        assert!(start.elapsed() >= Duration::from_millis(40), "tried again in between");
        assert_eq!(e.0, StatusCode::CONFLICT);
        assert_eq!(e.1, "the Galaxy database UpNext keeps is in use, most likely by a sync reading it; try again once the sync is done");
        assert!(work.is_file());

        let tried = std::cell::Cell::new(0);
        let frees_up = |from: &Path, to: &Path| {
            tried.set(tried.get() + 1);
            if tried.get() < 3 { Err(busy()) } else { std::fs::rename(from, to) }
        };
        assert!(replace_galaxy_with(&work, &to, 5, wait, frees_up).await.is_ok());
        assert_eq!(std::fs::read(&to).unwrap(), b"new");

        // A full disk: ENOSPC, or ERROR_DISK_FULL on Windows.
        let full = |_: &Path, _: &Path| Err(std::io::Error::from_raw_os_error(if cfg!(windows) { 112 } else { 28 }));
        let start = std::time::Instant::now();
        let Err(e) = replace_galaxy_with(&work, &to, 3, Duration::from_secs(5), full).await else { panic!("replaced") };
        assert!(start.elapsed() < Duration::from_secs(5), "not tried again");
        assert_eq!(e.0, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(e.1, format!("UpNext couldn't save the Galaxy database in {}; its log says why", dir.path().display()));
    }

    #[tokio::test]
    async fn an_upload_waits_its_turn_only_so_long() {
        let lock = tokio::sync::Mutex::new(());
        let Ok(first) = turn(&lock, Duration::from_millis(100)).await else { panic!("the first waits for nothing") };
        let Err(e) = turn(&lock, Duration::from_millis(100)).await else { panic!("two at once") };
        assert_eq!(e.0, StatusCode::CONFLICT);
        drop(first);
        assert!(turn(&lock, Duration::from_millis(100)).await.is_ok(), "the next upload goes once the first is done");
    }

    #[test]
    fn steps_are_the_wizards() {
        assert_eq!(STEPS.first(), Some(&"welcome"));
        assert_eq!(STEPS.last(), Some(&"done"));
    }
}
