//! Steam: the Web API when a key and SteamID are set, else the local Steam
//! client's caches.

use super::{Library, Outcomes, SyncRun};
use crate::db::PlayerId;
use crate::db::{ImportedGame, SyncConnection, SyncSummary};
use crate::enrich::Progress;
use crate::steam::SteamClient;
use anyhow::Result;
use async_trait::async_trait;

pub struct Steam;

#[async_trait]
impl Library for Steam {
    fn id(&self) -> &'static str {
        "steam"
    }
    fn label(&self) -> &'static str {
        "Steam"
    }

    async fn sync(&self, run: &SyncRun<'_>, progress: &mut Progress) -> Result<Vec<SyncSummary>> {
        let mut outcomes = Outcomes::default();
        // Player 1's own first Steam account keeps the Steam profile the
        // top bar shows (`meta.profile`), as the only one did before.
        let household = run.db.own_sync_connection(PlayerId::FIRST, "steam")?.id;
        for mut conn in run.connections("steam")? {
            let profile = conn.owner == PlayerId::FIRST && conn.id == household;
            let result = sync_account(run, progress, &mut conn, profile).await;
            outcomes.add(run, "Steam", &conn, result)?;
        }
        outcomes.finish()
    }
}

/// One Steam account: the Web API when there's a key and a SteamID, else
/// the local Steam client's files for its user. Player 1's first
/// account falls back to the SteamID in Settings or the environment, or the
/// user signed in to the Steam client; another needs its own SteamID.
async fn sync_account(run: &SyncRun<'_>, progress: &mut Progress, conn: &mut SyncConnection, profile: bool) -> Result<Vec<SyncSummary>> {
    let (db, cfg, clients, opts, sink) = (run.db, run.cfg, run.clients, run.opts, run.sink);
    let whose = run.whose(conn);
    let mut summaries = vec![];
    let own_id = conn.setting("steam_id").map(str::to_string);
    let steam_id = own_id.clone().or_else(|| if conn.first { cfg.steam_id.clone() } else { None });
    let include_hidden = conn.flag("steam_include_hidden").unwrap_or(conn.first && cfg.steam_include_hidden);
    if steam_id.is_none() && !conn.first {
        sink.log(format!("Steam{whose} skipped: no SteamID set for this account"));
        return Ok(summaries);
    }
    progress.message = "Fetching owned games from the Steam Web API".into();
    sink.update(progress);
    let creds = if opts.steam_local {
        Err(anyhow::anyhow!("local Steam client requested"))
    } else if steam_id == cfg.steam_id {
        cfg.steam_credentials()
    } else {
        match (&cfg.steam_api_key, &steam_id) {
            (Some(key), Some(id)) if id.len() == 17 && id.chars().all(|c| c.is_ascii_digit()) => Ok((key.as_str(), id.as_str())),
            (Some(_), Some(id)) => Err(anyhow::anyhow!("the SteamID must be a 17-digit SteamID64 (got {id:?})")),
            _ => Err(anyhow::anyhow!("no Steam Web API key")),
        }
    };
    let api = match creds {
        Ok((key, id)) => {
            // Credentials can change at runtime (Settings page), so build from the current config.
            let steam = SteamClient::new(clients.http.clone(), Some(key.to_string()), Some(id.to_string()), clients.base_url.as_deref());
            match steam.owned_games().await {
                Ok(owned) => Ok((steam, id, owned)),
                // A key Steam turns down (revoked, mistyped) is read past
                // like a missing one: the local client still has the library.
                Err(e) if crate::steam::key_refused(&e) => Err(anyhow::anyhow!("Steam turned the Web API key down")),
                Err(e) => return Err(e),
            }
        }
        Err(e) => Err(e),
    };
    match api {
        Ok((steam, id, owned)) => {
            // The API owns the library and playtime; the local client (if
            // present) knows what is installed.
            let installed = crate::steam_local::find_steam_dir(cfg.steam_dir.as_deref())
                .map(|d| crate::steam_local::installed_appids(&d))
                .unwrap_or_default();
            let hidden = hidden_in_steam(cfg, id, include_hidden);
            let before = owned.len();
            let owned: Vec<_> = owned.into_iter().filter(|g| !hidden.contains(&g.appid)).collect();
            if owned.len() < before {
                let n = before - owned.len();
                sink.log(format!("Steam{whose}: left out {n} {} hidden in Steam", if n == 1 { "game" } else { "games" }));
            }
            let games: Vec<ImportedGame> = owned
                .into_iter()
                .map(|g| ImportedGame {
                    source: "steam".into(),
                    source_id: g.appid.to_string(),
                    launcher: "steam".into(),
                    name: g.name,
                    appid: Some(g.appid),
                    installed: installed.contains(&g.appid),
                    playtime_minutes: Some(g.playtime_forever),
                    playtime_2weeks: g.playtime_2weeks,
                    last_played: g.rtime_last_played,
                    art_cover: None,
                    art_square: None,
                    description: None,
                    genres: vec![],
                    developer: None,
                    release_date: None,
                    store_url: Some(format!("https://store.steampowered.com/app/{}", g.appid)),
                    run_url: Some(format!("steam://run/{}", g.appid)),
                })
                .collect();
            let s = db.sync_into(conn, "steam", &games)?;
            db.set_account_external_id(conn, "steam", id)?;
            sink.log(format!(
                "Steam{whose}: {} ({} new, {} updated, {} removed)",
                crate::enrich::plural(s.total, "game", "games"),
                s.added,
                s.updated,
                s.removed
            ));
            summaries.push(s);
            match steam.player_summary().await {
                Ok(Some(p)) => {
                    if profile {
                        db.set_profile(&p)?;
                    }
                    sink.log(format!("Steam profile{whose}: {}", p.persona_name.clone().unwrap_or_default()));
                }
                Ok(None) => sink.log(format!("Steam profile{whose}: not found")),
                Err(e) => sink.log(format!("Steam profile lookup failed{whose} (non-fatal): {e}")),
            }
        }
        Err(e) => {
            // No API key (or forced): read the local Steam client's caches.
            match crate::steam_local::find_steam_dir(cfg.steam_dir.as_deref()) {
                Some(dir) => {
                    if !opts.steam_local {
                        sink.log(format!("Steam Web API not usable{whose} ({e}); reading the local Steam client instead"));
                    }
                    progress.message = "Reading the local Steam client's caches".into();
                    sink.update(progress);
                    // A damaged cache skips Steam with the reason in the log;
                    // the other libraries still sync.
                    let local = match crate::steam_local::read_library(&dir, steam_id.as_deref()) {
                        Ok(local) => local,
                        Err(e) => {
                            sink.log(format!("Steam{whose} skipped: {e:#}"));
                            return Ok(summaries);
                        }
                    };
                    let hidden = hidden_in_steam(cfg, &local.steam_id, include_hidden);
                    let read = local.games.len();
                    let games: Vec<ImportedGame> =
                        local.games.into_iter().filter(|g| g.appid.is_none_or(|a| !hidden.contains(&a))).collect();
                    let skipped = format!(
                        "{} and {} skipped",
                        crate::enrich::plural(local.skipped_non_games, "non-game app", "non-game apps"),
                        crate::enrich::plural(read - games.len(), "hidden game", "hidden games")
                    );
                    let s = if local.whole_library {
                        let s = db.sync_into(conn, "steam", &games)?;
                        sink.log(format!(
                            "Steam{whose} (local client, {}): {} ({} new, {} updated, {} removed); {skipped}",
                            dir.display(),
                            crate::enrich::plural(s.total, "game", "games"),
                            s.added,
                            s.updated,
                            s.removed
                        ));
                        s
                    } else {
                        // Not the account signed in to Steam (B-STORES-8):
                        // only its played and installed games are known, and
                        // the rest it had stay.
                        let s = db.sync_seen_into(conn, "steam", &games)?;
                        sink.log(format!(
                            "Steam{whose} (local client, {}): {} this account played or installed ({} new, {} updated); {skipped}. \
                             Steam keeps only the library of the account signed in last, so the rest of this one's comes in \
                             when it's the one signed in to Steam, or with a Steam Web API key",
                            dir.display(),
                            crate::enrich::plural(s.total, "game", "games"),
                            s.added,
                            s.updated
                        ));
                        s
                    };
                    db.set_account_external_id(conn, "steam", &local.steam_id)?;
                    if profile {
                        let mut profile = db.profile(PlayerId::FIRST, Some(&local.steam_id))?;
                        if profile.persona_name.is_none() {
                            profile.steam_id = local.steam_id.clone();
                            profile.persona_name = crate::steam::detect_persona_name(Some(&dir));
                            db.set_profile(&profile)?;
                        }
                    }
                    summaries.push(s);
                }
                None => sink.log(format!("Steam{whose} skipped: {e}; no local Steam client found either")),
            }
        }
    }
    Ok(summaries)
}

/// Appids hidden in the Steam client for this account, which a sync leaves
/// out unless the account's `steam_include_hidden` is on. Only the local
/// client knows them: with the Web API alone nothing is hidden.
fn hidden_in_steam(cfg: &crate::config::Config, steam_id: &str, include_hidden: bool) -> std::collections::HashSet<u32> {
    if include_hidden {
        return Default::default();
    }
    crate::steam_local::find_steam_dir(cfg.steam_dir.as_deref())
        .map(|d| crate::steam_local::hidden_appids(&d, steam_id))
        .unwrap_or_default()
}
