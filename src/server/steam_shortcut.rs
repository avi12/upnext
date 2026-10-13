//! "Add to Steam" (docs/api/steam-shortcut.md): whether UpNext's big
//! screen is in each Steam account's library on this computer, and adding
//! it. Both are this computer's alone (`access::refused_for`): they read and
//! write Steam's files here, and name the program's path.

use super::{ApiError, ApiResult, AppState};
use crate::steam_shortcut::{Account, Refused, Target};
use axum::extract::State;
use axum::http::StatusCode;
use axum::{Json, Router};
use serde::Serialize;
use std::path::PathBuf;

pub fn routes() -> Router<AppState> {
    use axum::routing::get;
    Router::new().route("/steam/shortcut", get(status).post(add))
}

/// Where big screen can be added to Steam and other launchers from.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SteamShortcut {
    /// What a launcher runs to open UpNext in big screen: the app window's
    /// program and its arguments. Null where UpNext has no app window to
    /// open (`unavailable` says why).
    target: Option<Target>,
    /// The Steam accounts that have signed in on this computer, each with
    /// whether its library has UpNext, or why its file can't be read.
    accounts: Vec<Account>,
    /// Steam is running: adding waits until it's closed, since Steam
    /// rewrites its shortcuts as it exits.
    steam_running: bool,
    /// Why "Add to Steam" can't be used on this computer, or null when it can.
    unavailable: Option<String>,
}

/// What the shortcut runs here, or why there's nothing to run.
fn target(state: &AppState) -> Result<Target, String> {
    if state.base_cfg.container {
        return Err("UpNext runs in a container here, with no app window for Steam to open".into());
    }
    if !cfg!(feature = "desktop") {
        return Err("this UpNext was built without its app window, so Steam would have nothing to open".into());
    }
    let exe = std::env::current_exe().map_err(|e| format!("couldn't find UpNext's own program: {e}"))?;
    let target = Target::for_exe(&exe, cfg!(windows));
    // A test runs from cargo's `deps` folder, with no `upnext-app.exe` beside
    // it; `there` has its own test.
    if cfg!(test) {
        return Ok(target);
    }
    there(target, &exe)
}

/// The target, if its program is there. On Windows the shortcut runs
/// `upnext-app.exe` from beside this program, which a copy run from
/// elsewhere may not have.
fn there(target: Target, exe: &std::path::Path) -> Result<Target, String> {
    if !target.program.is_file() {
        return Err(format!(
            "{} isn't there, so Steam would have nothing to open: install UpNext with its installer, or put it beside {}",
            target.program.display(),
            exe.display()
        ));
    }
    Ok(target)
}

/// Steam's folder on this computer, or why there's none to write to.
fn steam_dir(state: &AppState) -> Result<PathBuf, String> {
    crate::steam_local::find_steam_dir(state.cfg().steam_dir.as_deref())
        .ok_or_else(|| "Steam isn't on this computer, or nobody has signed in to it yet".to_string())
}

/// Whether Steam runs. Tests never look at the machine they run on.
fn running() -> bool {
    !cfg!(test) && crate::steam_shortcut::steam_running()
}

/// In a Flatpak (`flatpak`): what a launcher runs, `flatpak run` with the
/// app's id, and why "Add to Steam" can't be used. The sandbox shows Steam's
/// folders read-only and can't see whether Steam runs, so the steps say how
/// to add it in Steam instead.
fn in_flatpak(sandbox: &crate::flatpak::Flatpak) -> (Target, String) {
    let (program, run) = sandbox.run_command();
    let target = Target { program, arguments: format!("{run} {}", crate::autostart::BIG_SCREEN_FLAG) };
    let why = "UpNext runs as a Flatpak here, which can read Steam's files but not change them, so add it in Steam: in Steam's \
        desktop window, Games › Add a Non-Steam Game to My Library, tick UpNext and add it; then in its Properties, put \
        --big-screen at the end of the launch options, and under the controller icon on its page, choose the layout, \
        Templates, Gamepad, since Steam gives a game it didn't install the desktop layout, where the pad moves the mouse";
    (target, why.into())
}

/// In UpNext's Steam copy (`steam_copy`): what a launcher runs, the
/// copy's own launcher, and why "Add to Steam" isn't needed. Steam lists
/// the copy already, and a shortcut to it would start a second UpNext that
/// Steam doesn't count as its own.
fn in_steam_copy(copy: &crate::steam_copy::SteamCopy) -> (Target, String) {
    let target = Target { program: copy.launcher.clone(), arguments: crate::autostart::BIG_SCREEN_FLAG.to_string() };
    let why = "This is UpNext's Steam copy, which is in your Steam library already: start it there, where its UpNext \
        launch option opens big screen";
    (target, why.into())
}

async fn look(state: &AppState) -> Result<SteamShortcut, ApiError> {
    if let Some(sandbox) = crate::flatpak::get() {
        let (target, why) = in_flatpak(sandbox);
        return Ok(SteamShortcut { target: Some(target), accounts: vec![], steam_running: false, unavailable: Some(why) });
    }
    if let Some(copy) = crate::steam_copy::get() {
        let (target, why) = in_steam_copy(copy);
        return Ok(SteamShortcut { target: Some(target), accounts: vec![], steam_running: false, unavailable: Some(why) });
    }
    let target = target(state);
    let dir = steam_dir(state);
    let (shown, unavailable) = match (&target, &dir) {
        (Err(why), _) | (_, Err(why)) => (target.as_ref().ok().cloned(), Some(why.clone())),
        (Ok(target), Ok(_)) => (Some(target.clone()), None),
    };
    let (accounts, steam_running) = match (target, dir) {
        (Ok(target), Ok(dir)) => tokio::task::spawn_blocking(move || (crate::steam_shortcut::status(&dir, &target), running()))
            .await
            .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
        _ => (vec![], false),
    };
    let unavailable = unavailable.or_else(|| accounts.is_empty().then(|| Refused::NoAccounts.to_string()));
    Ok(SteamShortcut { target: shown, accounts, steam_running, unavailable })
}

#[utoipa::path(get, path = "/steam/shortcut", tag = "setup", operation_id = "steam_shortcut",
    description = "Big screen in Steam and other launchers: what a launcher runs to open UpNext in big screen, the Steam \
        accounts on this computer and whether each library has UpNext, whether Steam is running, and why \"Add to Steam\" \
        can't be used here, if it can't. Only on the computer UpNext runs on.",
    responses((status = 200, body = SteamShortcut)))]
async fn status(State(state): State<AppState>) -> ApiResult<SteamShortcut> {
    Ok(Json(look(&state).await?))
}

#[utoipa::path(post, path = "/steam/shortcut", tag = "setup", operation_id = "steam_shortcut_add",
    description = "Add UpNext's big screen to every Steam account's library on this computer, as a non-Steam shortcut with \
        UpNext's library images, unless it's there already. Each file is copied aside before UpNext first changes it, and its other shortcuts \
        stay as they were. Only while Steam is closed, and only on the computer UpNext runs on.",
    responses(
        (status = 200, body = SteamShortcut, description = "Added (or already there) for every account"),
        (status = 409, description = "Steam is running, an account's shortcuts can't be read (then no account is changed), or it \
            can't be added here (no app window, no Steam, no account, UpNext runs as a Flatpak, or this is UpNext's Steam copy, \
            which Steam lists already)")))]
async fn add(State(state): State<AppState>) -> ApiResult<SteamShortcut> {
    if let Some(sandbox) = crate::flatpak::get() {
        return Err(ApiError::conflict(in_flatpak(sandbox).1));
    }
    if let Some(copy) = crate::steam_copy::get() {
        return Err(ApiError::conflict(in_steam_copy(copy).1));
    }
    let target = target(&state).map_err(ApiError::conflict)?;
    let dir = steam_dir(&state).map_err(ApiError::conflict)?;
    let written = tokio::task::spawn_blocking(move || crate::steam_shortcut::add(&dir, &target, running))
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if let Err(e) = written {
        return Err(match e.downcast_ref::<Refused>() {
            Some(r) => ApiError::conflict(r.to_string()),
            None => ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("couldn't add UpNext to Steam: {e:#}")),
        });
    }
    Ok(Json(look(&state).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flatpak_shows_its_command_and_the_steps_in_steam() {
        let sandbox = crate::flatpak::Flatpak::example();
        let (target, why) = in_flatpak(&sandbox);
        assert_eq!(target.program, PathBuf::from("/usr/bin/flatpak"));
        assert_eq!(target.arguments, "run io.github.zytocc.UpNext --big-screen");
        assert!(why.starts_with("UpNext runs as a Flatpak here") && why.contains("Add a Non-Steam Game"), "{why}");
        assert!(why.contains("Templates, Gamepad"), "the steps include the layout Add to Steam would set: {why}");
    }

    #[test]
    fn the_steam_copy_shows_its_launcher_and_needs_no_shortcut() {
        let copy = crate::steam_copy::SteamCopy::example();
        let (target, why) = in_steam_copy(&copy);
        assert_eq!(target.program, copy.launcher, "a launcher other than Steam runs the copy's own launcher");
        assert_eq!(target.arguments, "--big-screen");
        assert!(why.starts_with("This is UpNext's Steam copy") && why.contains("in your Steam library already"), "{why}");
    }

    #[test]
    fn a_missing_app_program_is_named() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("upnext.exe");
        std::fs::write(&exe, b"").unwrap();
        let why = there(Target::for_exe(&exe, true), &exe).unwrap_err();
        assert!(why.contains("upnext-app.exe isn't there"), "{why}");
        std::fs::write(dir.path().join("upnext-app.exe"), b"").unwrap();
        assert_eq!(there(Target::for_exe(&exe, true), &exe).unwrap().program, dir.path().join("upnext-app.exe"));
    }
}
