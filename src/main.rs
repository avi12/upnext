use anyhow::Result;
use clap::builder::PossibleValuesParser;
use clap::{Parser, Subcommand};
use std::collections::BTreeMap;
use upnext::cli;
use upnext::{Config, EnrichOptions, GameFilter, SyncOptions};

#[derive(Parser)]
#[command(name = "upnext", version = env!("UPNEXT_VERSION"), about = "UpNext: Steam + Heroic + GOG Galaxy libraries, LLM-ranked picks")]
struct Cli {
    /// Who the command is for, by name or id (default: UPNEXT_PLAYER, else the default player).
    /// Picks, prompts, marks, the library, stats, the taste profile and a player's own settings
    /// are each player's
    #[arg(long, global = true)]
    player: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Pull libraries from Steam, Heroic and (optionally) a GOG Galaxy database
    #[command(after_help = cli::sync::libraries_help())]
    Sync {
        /// Only these libraries (comma-separated)
        #[arg(long, value_delimiter = ',', value_parser = PossibleValuesParser::new(cli::sync::library_ids()), hide_possible_values = true, conflicts_with = "skip")]
        only: Vec<String>,
        /// Every library but these
        #[arg(long, value_delimiter = ',', value_parser = PossibleValuesParser::new(cli::sync::library_ids()), hide_possible_values = true)]
        skip: Vec<String>,
        /// Read the local Steam client's cache instead of the Web API (automatic when no API key is set)
        #[arg(long)]
        steam_local: bool,
        /// Path to a galaxy-2.0.db copy (defaults to the galaxy_db setting)
        #[arg(long)]
        galaxy_db: Option<String>,
        /// Only this connection, by its id (`upnext player list` shows whose accounts are whose;
        /// GET /api/accounts lists the connections)
        #[arg(long)]
        account: Option<i64>,
    },
    /// Fetch what each source knows about each title; resumable. Sources are
    /// listed below; the ones marked "off" run only when asked for.
    #[command(after_help = cli::sync::sources_help())]
    Enrich {
        /// Only these sources (comma-separated)
        #[arg(long, value_delimiter = ',', value_parser = PossibleValuesParser::new(cli::sync::source_ids()), hide_possible_values = true, conflicts_with_all = ["skip", "with"])]
        only: Vec<String>,
        /// The usual sources but not these
        #[arg(long, value_delimiter = ',', value_parser = PossibleValuesParser::new(cli::sync::source_ids()), hide_possible_values = true)]
        skip: Vec<String>,
        /// The usual sources and these (e.g. --with hltb,digests)
        #[arg(long, value_delimiter = ',', value_parser = PossibleValuesParser::new(cli::sync::source_ids()), hide_possible_values = true)]
        with: Vec<String>,
        /// Re-fetch rows that are already enriched (the Steam store pages too, unless --only or --skip narrows the run)
        #[arg(long)]
        refresh: bool,
        /// Only process this many games
        #[arg(long)]
        limit: Option<usize>,
        /// Only these titles, by id (comma-separated; ids as `upnext list` shows them)
        #[arg(long, value_delimiter = ',')]
        titles: Vec<i64>,
    },
    /// Back up the database into the backups folder, as the scheduled backup does
    Backup {
        /// Keep this many of these backups (default: the backup job's setting, 4)
        #[arg(long, conflicts_with = "note")]
        keep: Option<usize>,
        /// Make a backup kept until you delete it, with this note, as Settings › Backups, "Back up now" does
        #[arg(long)]
        note: Option<String>,
    },
    /// Write a copy of the database without keys and sign-ins (what Settings' export downloads),
    /// for a test copy or to share. Only reads the database, so the server can keep running
    Export {
        /// The file to write, e.g. data/scratch/live-copy.db (its folder is made if missing)
        out: std::path::PathBuf,
        /// Copy this database instead, e.g. a backup (default: the one UpNext uses, UPNEXT_DB)
        #[arg(long)]
        from: Option<std::path::PathBuf>,
        /// Replace `out` if it exists
        #[arg(long)]
        force: bool,
    },
    /// Your taste profile: favourites, likes, dislikes, hard nos, the patterns UpNext counted and your habits
    Taste {
        /// Count the patterns again first, as the profile job does after each sync
        #[arg(long)]
        update: bool,
    },
    /// Fill an empty database with a made-up library and profile, to try UpNext without your own
    /// (point UPNEXT_DB at a new file first)
    MockLibrary {
        /// Only the first player: no second player, none of their games, nothing hidden
        #[arg(long)]
        one_player: bool,
    },
    /// The jobs that run by themselves while `serve` runs: schedules, last and next runs
    Jobs {
        #[command(subcommand)]
        action: Option<JobsCmd>,
    },
    /// Ask the LLM for picks (or UpNext's own ranking, with no AI set up)
    Pick {
        /// Free-text request, e.g. "2 hours, chill, story-heavy"
        prompt: Vec<String>,
        #[arg(short = 'n', long, default_value_t = 3)]
        count: usize,
        /// Print the result as JSON
        #[arg(long)]
        json: bool,
        /// Restrict to sources (steam, epic, gog, amazon); repeatable
        #[arg(long = "source")]
        sources: Vec<String>,
        /// Exclude game refs (see `show`); repeatable
        #[arg(long = "exclude")]
        exclude: Vec<String>,
        /// Pick from UpNext's own ranking, with no AI call (what picks do
        /// when no AI is set up)
        #[arg(long)]
        without_ai: bool,
        /// Pick for one of your routines, by name (Profile › Habits)
        #[arg(long)]
        routine: Option<String>,
    },
    /// Mark a game done (beat it), shelved, abandoned or hidden (not for me), or an endless game
    /// still_playing or burnt_out. REF: Steam appid, `source:id`, or `#id`
    Mark {
        #[arg(value_parser = ["done", "shelved", "abandoned", "hidden", "still_playing", "burnt_out", "paused", "dropped", "hide"])]
        status: String,
        r#ref: String,
        #[arg(long)]
        note: Option<String>,
    },
    /// Clear a manual override
    Unmark { r#ref: String },
    /// Merge two or more titles into one (first wins); persists across syncs
    Merge { refs: Vec<String> },
    /// Detach one store copy (e.g. `gog:1207658924`) into its own title
    Split { copy: String },
    /// List games
    List {
        #[arg(long, help = cli::library::status_help())]
        status: Option<String>,
        /// Comma list of stores: steam, gog, epic, amazon, or one Galaxy brings in (origin, xboxone…)
        #[arg(long)]
        source: Option<String>,
        #[arg(long)]
        installed: bool,
        #[arg(short, long)]
        q: Option<String>,
        #[arg(long)]
        genre: Option<String>,
        #[arg(long)]
        tag: Option<String>,
        /// name | playtime | last_played | review | hltb | release | metacritic | rating
        #[arg(long, default_value = "name")]
        sort: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Show one game
    Show {
        r#ref: String,
        #[arg(long)]
        json: bool,
    },
    /// Library statistics
    Stats,
    /// Print the prompt that `pick` would send (debugging)
    Prompt {
        prompt: Vec<String>,
        #[arg(short = 'n', long, default_value_t = 3)]
        count: usize,
        /// Leave the review feel out of the candidate lines, to compare picks with and without it
        #[arg(long)]
        without_feel: bool,
        /// Leave the taste profile out (the most-played list instead of About me, no hard nos or
        /// likes in the ranking), to compare picks with and without it
        #[arg(long)]
        without_profile: bool,
        /// Give each candidate other backlog players' `bv` line, whatever the
        /// verdicts_prompt setting says, to compare picks with and without them
        #[arg(long, conflicts_with = "without_verdicts")]
        with_verdicts: bool,
        /// Leave the `bv` lines out, whatever the verdicts_prompt setting says
        #[arg(long)]
        without_verdicts: bool,
        /// Print exactly what the model receives, as JSON: system, prompt and schema
        #[arg(long)]
        json: bool,
        /// The prompt for one of your routines, by name (Profile › Habits)
        #[arg(long)]
        routine: Option<String>,
    },
    /// Connect your GOG account (browser login; paste the redirect URL or code)
    GogLogin {
        /// The code or redirect URL. Omitted: prompts on stdin.
        code: Option<String>,
    },
    /// Forget the GOG sign-in
    GogLogout,
    /// Sign in to Epic for your whole Epic library and playtime (paste the code Epic's page shows)
    EpicLogin {
        /// The authorizationCode, or the whole page. Omitted: prompts on stdin.
        code: Option<String>,
    },
    /// Forget the stored Epic sign-in
    EpicLogout,
    /// Show or change in-app settings (they override .env)
    Config {
        #[command(subcommand)]
        action: ConfigCmd,
    },
    /// The people who play here: list, add, rename, remove, or set a PIN
    Player {
        #[command(subcommand)]
        action: PlayerCmd,
    },
    /// API tokens, for other devices and scripts (Settings › Devices and API makes them too)
    Token {
        #[command(subcommand)]
        action: TokenCmd,
    },
    /// UpNext's shared game data: People say, Vibes and review tags for popular games, CC0
    Seed {
        #[command(subcommand)]
        action: SeedCmd,
    },
    /// Backlog verdicts: how games fit a backlog and limited time, by other players, CC0
    Verdicts {
        #[command(subcommand)]
        action: VerdictsCmd,
    },
    /// Check configuration, database and the claude CLI, and show every path UpNext uses
    Doctor,
    /// Move a checkout's data folder (database, GOG sign-in, backups) into
    /// the data dir: copies, checks the copy, and renames the old folder to
    /// <name>.migrated-<date> instead of deleting it
    MigrateData {
        /// The folder with the old upnext.db, e.g. ./data in the repo
        #[arg(long)]
        from: std::path::PathBuf,
    },
    /// Open UpNext in its own window: on the UpNext already running on the port
    /// (the service), or with the server inside the window until it closes
    App {
        /// The port to look on, and to use when the window runs the server
        /// (default: the configured port, 8787)
        #[arg(long)]
        port: Option<u16>,
        /// Open full screen in big screen, the layout for a controller, a
        /// handheld or a TV (F11 leaves full screen); closing it stops UpNext
        /// when the window runs the server
        #[arg(long)]
        big_screen: bool,
        /// Use the library on another computer: find the computers on the
        /// network running UpNext (or type an address), and ask one to allow
        /// this one. The window then opens on it until switched back
        #[arg(long)]
        connect: bool,
    },
    /// Put UpNext in the desktop's launcher with its icon (in ~/.local/share), started by this binary
    #[command(hide = cfg!(not(target_os = "linux")))]
    InstallDesktop {
        /// Take it out of the launcher again
        #[arg(long)]
        remove: bool,
    },
    /// A new pairing code from the running server, for a browser on another
    /// device (or the host of UpNext's container: `docker exec upnext upnext pair`);
    /// or the devices asking to be allowed, and the answer to one
    Pair {
        /// The server's port (default: the configured port, 8787)
        #[arg(long, global = true)]
        port: Option<u16>,
        #[command(subcommand)]
        action: Option<PairCmd>,
    },
    /// Check that the server on this computer answers; exits 1 if not (the
    /// container image's health check)
    Healthcheck {
        /// The server's port (default: the configured port, 8787)
        #[arg(long)]
        port: Option<u16>,
    },
    /// Install Claude Code from Anthropic into the home folder (`/data` in UpNext's
    /// container), checked against Anthropic's manifest, as `UPNEXT_INSTALL_CLAUDE=1`
    /// does when the server starts: `docker exec upnext upnext install-claude`. Linux only
    #[command(hide = cfg!(not(target_os = "linux")))]
    InstallClaude,
    /// Run the HTTP API and web UI
    Serve {
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        static_dir: Option<String>,
    },
}

#[derive(Subcommand)]
enum JobsCmd {
    /// Every run, newest first, from the server, the UI or a terminal
    History {
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Only runs that failed or had failures
        #[arg(long)]
        failed: bool,
    },
    /// Print a job's log (`jobs history` lists their numbers)
    Log {
        id: i64,
        /// Warnings and errors only
        #[arg(long)]
        warnings: bool,
    },
    /// Change when a job runs: off, daily, weekly, monthly, or after:<job> (after:sync)
    Schedule {
        #[arg(value_parser = PossibleValuesParser::new(cli::jobs::task_ids()))]
        job: String,
        when: Option<String>,
        /// Backups kept (the backup job only)
        #[arg(long)]
        keep: Option<usize>,
    },
}

#[derive(Subcommand)]
enum VerdictsCmd {
    /// Print the verdicts' contract as JSON: the questions and their answers, the limits, the
    /// text checks and the consent text (docs/verdicts-contract.json)
    Contract,
    /// Show the verdict UpNext would pre-fill for a game from your marks, playtime, play diary
    /// and stop reasons. Sends nothing
    Draft {
        /// Steam appid, `source:id`, or `#id`
        game: String,
        #[arg(long)]
        json: bool,
    },
    /// Check a downloaded snapshot (verdicts.json with its files beside it, or one data file)
    /// and say what it holds. Applies nothing
    Check { file: std::path::PathBuf },
    /// List your verdicts as this computer keeps them: drafts, sent, queued and taken down
    List {
        #[arg(long)]
        json: bool,
    },
    /// Send your verdict about a game, pre-filled and changed by the flags. Shows it and, the
    /// first time, the consent text (it's public, CC0), and asks first
    Send {
        /// Steam appid, `source:id`, or `#id`
        game: String,
        #[command(flatten)]
        answers: Box<cli::verdicts::AnswerArgs>,
        /// Don't ask (for scripts); only once the consent was accepted
        #[arg(long)]
        yes: bool,
    },
    /// Take your verdict about a game down, or all of them and forget your key
    Withdraw {
        /// Steam appid, `source:id`, or `#id`
        game: Option<String>,
        #[arg(long, conflicts_with = "game")]
        all: bool,
    },
    /// Ask the service which verdicts your key sent, and bring this computer's list up to date
    Refresh,
    /// Download other players' backlog verdicts now, as the daily job does (Settings › Jobs).
    /// Sends nothing
    Sync,
    /// Put a downloaded snapshot of other players' verdicts in place, for an install that can't
    /// reach the service, and turn them on
    Apply {
        /// A folder, a manifest (verdicts.json) with its summary beside it, a summary file
        /// (verdicts-summary-<date>.jsonl.gz), or the same as a URL
        from: String,
        /// Put it in place even when it's no newer than the snapshot kept
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
enum SeedCmd {
    /// Apply shared data, so matching games get summaries without fetching reviews or an AI call.
    /// Kept, so games added later get theirs after a sync
    Apply {
        /// A manifest (upnext-seed.json) or data file (upnext-seed-<date>.jsonl.gz), as a path or URL
        /// (default: the newest published data's)
        from: Option<String>,
    },
    /// What shared data is applied, and when
    Status,
    /// Remove the summaries from shared data and the kept copy, so your own AI writes them
    Remove,
    /// Maintainers: start or carry on a build of the shared data, in a database of its own
    /// (UPNEXT_DATA_DIR set to an empty folder): SteamSpy's most-owned games, checked against
    /// the Steam store. Then `seed fetch`, `seed generate` and `seed export`
    Build {
        /// How many games (default 10,000)
        #[arg(long)]
        count: Option<usize>,
    },
    /// Maintainers: fetch the build's reviews from Steam, and its GOG ids from Wikidata and, by
    /// the store page's slug, from GOG's catalogue; resumable. A slug the catalogue doesn't list
    /// is kept as "none": delete seed-build/gog-ids-by-slug.json to ask every slug again
    Fetch,
    /// Maintainers: write the build's summaries on the AI its settings name (`upnext config set
    /// llm_backend openai`, `openai_base_url`, `openai_model`, `openai_thinking`); resumable
    Generate {
        /// Write at most this many summaries a run
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Maintainers: write the files to publish, upnext-seed-<date>.jsonl.gz and upnext-seed.json
    Export {
        /// The folder to write them in
        #[arg(long)]
        out: std::path::PathBuf,
        /// The data's date, YYYY-MM-DD (default: today)
        #[arg(long)]
        date: Option<String>,
    },
}

#[derive(Subcommand)]
enum TokenCmd {
    /// Make a token; it's printed once, on its own line. With --player (or UPNEXT_PLAYER) it's
    /// bound to that player: it acts only for them and can't change what is everyone's
    Create {
        /// What it's for: "phone", "Home Assistant"
        #[arg(long)]
        name: String,
        /// read: look, change nothing. full: everything, picks and settings included
        #[arg(long, value_parser = ["read", "full"])]
        scope: String,
    },
    /// Every token: name, scope, when it was made and last used
    List,
    /// Stop a token at once, by its id or name
    Revoke { token: String },
}

#[derive(Subcommand)]
enum PlayerCmd {
    /// Every player, with their PIN, the default and the accounts they own
    List,
    /// Add a player; what everyone shares is theirs to use at once
    Add { name: String },
    /// Rename a player
    Rename { player: String, name: String },
    /// Remove a player and only what is theirs, after a backup; asks first
    Remove {
        player: String,
        /// Don't ask (for scripts)
        #[arg(long)]
        yes: bool,
    },
    /// Set a player's PIN (asked for, never on the command line), or clear it
    Pin {
        player: String,
        #[arg(long)]
        clear: bool,
    },
}

#[derive(Subcommand)]
enum PairCmd {
    /// The devices waiting to be allowed, with the number each one shows
    Requests,
    /// Allow the device that shows this number
    Approve {
        /// The four digits the device shows
        number: String,
    },
    /// Refuse the device that shows this number
    Deny {
        /// The four digits the device shows
        number: String,
    },
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// List effective settings and where each comes from
    List,
    /// Set a value (`config list` shows the keys; the ones marked env-only can't be set here)
    Set { key: String, value: String },
    /// Remove the app value so .env / auto-detect / default applies
    Unset { key: String },
}

fn main() -> Result<()> {
    // Inside a Flatpak, a host's certificate file the sandbox can't read
    // would stop every HTTPS client; this starts UpNext again without it.
    upnext::restart_without_unreadable_cert_env();
    // The GOG login window owns the main thread (GUI event loop), so it is
    // dispatched before the async runtime starts. Spawned by the server.
    #[cfg(feature = "desktop")]
    if std::env::args().nth(1).as_deref() == Some("gog-window") {
        return upnext::run_gog_window();
    }
    // Double-clicked in Explorer: run UpNext and open the browser, rather
    // than print the usage into a console that closes at once.
    #[cfg(all(windows, feature = "desktop"))]
    if cli::double_click::started_alone() {
        return cli::double_click::main();
    }
    cli::init_logging();
    let cli = Cli::parse();
    let mut cfg = Config::load()?;
    cfg.cli_player = cli.player.clone().or_else(|| std::env::var("UPNEXT_PLAYER").ok());
    if !matches!(cli.cmd, Cmd::MigrateData { .. } | Cmd::Doctor | Cmd::InstallDesktop { .. } | Cmd::Healthcheck { .. }) {
        if let Some(why) = cli::migrate::unmoved_data(&cfg) {
            anyhow::bail!(why);
        }
        if let Some(hint) = cli::migrate::env_hint(&cfg) {
            eprintln!("{hint}");
        }
    }

    match cli.cmd {
        // The app window's event loop needs the main thread too, so the app
        // runs instead of the runtime below and makes its own.
        #[cfg(feature = "desktop")]
        Cmd::App { port, big_screen, connect } => cli::app::run(cfg, port, big_screen, connect),
        #[cfg(feature = "desktop")]
        Cmd::InstallDesktop { remove } => cli::app::install_desktop(remove),
        #[cfg(not(feature = "desktop"))]
        Cmd::App { .. } | Cmd::InstallDesktop { .. } => {
            anyhow::bail!("this UpNext has no app window: it was built without the `desktop` feature. `upnext serve` runs the web UI")
        }
        cmd => tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(run(cmd, cfg)),
    }
}

async fn run(cmd: Cmd, cfg: Config) -> Result<()> {
    match cmd {
        Cmd::Sync { only, skip, steam_local, galaxy_db, account } => {
            let mut libraries = BTreeMap::new();
            if !only.is_empty() {
                for id in cli::sync::library_ids() {
                    libraries.insert(id.to_string(), only.iter().any(|o| o == id));
                }
            }
            libraries.extend(skip.into_iter().map(|id| (id, false)));
            let opts = SyncOptions { libraries, steam_local, galaxy_db, connection: account };
            cli::sync::sync(&cfg, opts).await
        }
        Cmd::Enrich { only, skip, with, refresh, limit, titles } => {
            let mut sources = BTreeMap::new();
            if !only.is_empty() {
                for id in cli::sync::source_ids() {
                    sources.insert(id.to_string(), only.iter().any(|o| o == id));
                }
            }
            sources.extend(skip.into_iter().map(|id| (id, false)));
            sources.extend(with.into_iter().map(|id| (id, true)));
            cli::sync::enrich(&cfg, EnrichOptions { sources, refresh, limit, titles }).await
        }
        Cmd::Backup { keep, note } => cli::sync::backup(&cfg, keep, note).await,
        Cmd::Export { out, from, force } => cli::sync::export(&cfg, &out, from.as_deref(), force),
        Cmd::Taste { update } => cli::taste::show(&cfg, update).await,
        Cmd::MockLibrary { one_player } => cli::taste::mock_library(&cfg, one_player),
        Cmd::Jobs { action } => match action {
            None => cli::jobs::list(&cfg),
            Some(JobsCmd::History { limit, failed }) => cli::jobs::history(&cfg, limit, failed),
            Some(JobsCmd::Log { id, warnings }) => cli::jobs::log(&cfg, id, warnings),
            Some(JobsCmd::Schedule { job, when, keep }) => cli::jobs::schedule(&cfg, &job, when.as_deref(), keep),
        },
        Cmd::Pick { prompt, count, json, sources, exclude, without_ai, routine } => {
            cli::pick::pick(&cfg, prompt, count, json, sources, exclude, without_ai, routine.as_deref()).await
        }
        Cmd::Mark { status, r#ref, note } => cli::library::mark(&cfg, &status, &r#ref, note.as_deref()),
        Cmd::Unmark { r#ref } => cli::library::unmark(&cfg, &r#ref),
        Cmd::Merge { refs } => cli::library::merge(&cfg, &refs),
        Cmd::Split { copy } => cli::library::split(&cfg, &copy),
        Cmd::List { status, source, installed, q, genre, tag, sort, limit, json } => {
            let statuses = cli::library::statuses(status.as_deref())?;
            let filter = GameFilter {
                statuses,
                sources: source.map(|s| s.split(',').map(|x| x.trim().to_string()).collect()),
                installed: if installed { Some(true) } else { None },
                q,
                genre,
                tag,
                linux: None,
                sort,
                order: None,
                limit,
                offset: 0,
                ..Default::default()
            };
            cli::library::list(&cfg, &filter, json)
        }
        Cmd::Show { r#ref, json } => cli::library::show(&cfg, &r#ref, json),
        Cmd::Stats => cli::library::stats(&cfg),
        Cmd::Prompt { prompt, count, without_feel, without_profile, with_verdicts, without_verdicts, json, routine } => {
            let verdicts = (with_verdicts || without_verdicts).then_some(with_verdicts);
            cli::pick::prompt(&cfg, prompt, count, !without_feel, !without_profile, verdicts, json, routine.as_deref())
        }
        Cmd::GogLogin { code } => cli::gog::login(&cfg, code).await,
        Cmd::GogLogout => cli::gog::logout(&cfg).await,
        Cmd::EpicLogin { code } => cli::gog::epic_login(&cfg, code).await,
        Cmd::EpicLogout => cli::gog::epic_logout(&cfg).await,
        Cmd::Player { action } => match action {
            PlayerCmd::List => cli::player::list(&cfg),
            PlayerCmd::Add { name } => cli::player::add(&cfg, &name),
            PlayerCmd::Rename { player, name } => cli::player::rename(&cfg, &player, &name),
            PlayerCmd::Remove { player, yes } => cli::player::remove(&cfg, &player, yes),
            PlayerCmd::Pin { player, clear } => cli::player::pin(&cfg, &player, clear),
        },
        Cmd::Config { action } => match action {
            ConfigCmd::List => cli::config::list(&cfg),
            ConfigCmd::Set { key, value } => cli::config::set(&cfg, &key, &value),
            ConfigCmd::Unset { key } => cli::config::unset(&cfg, &key),
        },
        Cmd::Token { action } => match action {
            TokenCmd::Create { name, scope } => cli::token::create(&cfg, &name, &scope),
            TokenCmd::List => cli::token::list(&cfg),
            TokenCmd::Revoke { token } => cli::token::revoke(&cfg, &token),
        },
        Cmd::Seed { action } => match action {
            SeedCmd::Apply { from } => cli::seed::apply(&cfg, from.as_deref()).await,
            SeedCmd::Status => cli::seed::status(&cfg),
            SeedCmd::Remove => cli::seed::remove(&cfg),
            SeedCmd::Build { count } => cli::seed::build(&cfg, count).await,
            SeedCmd::Fetch => cli::seed::fetch(&cfg).await,
            SeedCmd::Generate { limit } => cli::seed::generate(&cfg, limit).await,
            SeedCmd::Export { out, date } => cli::seed::export(&cfg, &out, date.as_deref()),
        },
        Cmd::Verdicts { action } => match action {
            VerdictsCmd::Contract => cli::verdicts::contract(),
            VerdictsCmd::Draft { game, json } => cli::verdicts::draft(&cfg, &game, json),
            VerdictsCmd::Check { file } => cli::verdicts::check(&file),
            VerdictsCmd::List { json } => cli::verdicts::list(&cfg, json),
            VerdictsCmd::Send { game, answers, yes } => cli::verdicts::send(&cfg, &game, *answers, yes).await,
            VerdictsCmd::Withdraw { game, all } => cli::verdicts::withdraw(&cfg, game.as_deref(), all).await,
            VerdictsCmd::Refresh => cli::verdicts::refresh(&cfg).await,
            VerdictsCmd::Sync => cli::community::sync(&cfg).await,
            VerdictsCmd::Apply { from, force } => cli::community::apply(&cfg, &from, force).await,
        },
        Cmd::Doctor => cli::config::doctor(&cfg).await,
        Cmd::MigrateData { from } => cli::migrate::migrate_data(&cfg, &from),
        Cmd::Pair { port, action } => match action {
            None => cli::running::pair(&cfg, port).await,
            Some(PairCmd::Requests) => cli::running::pair_requests(&cfg, port).await,
            Some(PairCmd::Approve { number }) => cli::running::pair_answer(&cfg, port, &number, true).await,
            Some(PairCmd::Deny { number }) => cli::running::pair_answer(&cfg, port, &number, false).await,
        },
        Cmd::Healthcheck { port } => cli::running::healthcheck(&cfg, port).await,
        Cmd::InstallClaude => cli::serve::install_claude(&cfg).await,
        Cmd::Serve { port, host, static_dir } => cli::serve::serve(cfg, port, host, static_dir).await,
        Cmd::App { .. } | Cmd::InstallDesktop { .. } => unreachable!("handled before the runtime starts"),
    }
}
