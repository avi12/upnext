//! `token create|list|revoke`: API tokens for other devices and scripts,
//! as Settings › Devices and API makes them, for installs whose Settings
//! page isn't reachable yet (a headless server, a container).

use super::format::{dim, green};
use super::jobs::relative;
use super::open_db;
use crate::config::Config;
use crate::db::{ApiToken, Scope};
use anyhow::{Result, anyhow, bail};
use comfy_table::{ContentArrangement, Table, presets::UTF8_FULL_CONDENSED};

/// Make a token. The token goes to stdout on its own, so a script can keep
/// it (`TOKEN=$(upnext token create --name ha --scope read)`); the rest to stderr.
pub fn create(cfg: &Config, name: &str, scope: &str) -> Result<()> {
    let (db, bound) = open_db(cfg)?;
    let scope = Scope::parse(scope).ok_or_else(|| anyhow!("--scope is read or full"))?;
    // Bound only when a player is named: without one, the token uses the
    // header or the default player, as a browser does.
    let player = cfg.cli_player.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(|_| bound.whose());
    let made = db.token_create_for(name, scope, "token", player)?;
    let whose = match player.map(|p| db.player(p)).transpose()?.flatten() {
        Some(p) => format!(", bound to {}", p.name),
        None => String::new(),
    };
    eprintln!("{} token {:?} made ({}{whose}). It's shown this once; copy it now:", green("✓"), made.info.name, scope.as_str());
    println!("{}", made.token);
    eprintln!("{}", dim("Send it as `Authorization: Bearer <token>`. `upnext token revoke` stops it."));
    Ok(())
}

pub fn list(cfg: &Config) -> Result<()> {
    let (db, _cfg) = open_db(cfg)?;
    let tokens = db.tokens()?;
    if tokens.is_empty() {
        println!("No tokens. Make one with `upnext token create --name phone --scope read`.");
        return Ok(());
    }
    let mut table = Table::new();
    table.load_style(UTF8_FULL_CONDENSED).set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(["Id", "Name", "Scope", "Player", "Token", "Made", "Last used"]);
    let players = db.players()?;
    for t in &tokens {
        let player = t.player_id.and_then(|p| players.iter().find(|x| x.id == p)).map(|p| p.name.clone()).unwrap_or_default();
        let name = if t.kind == "device" { format!("{} (paired device)", shown(&t.name)) } else { shown(&t.name) };
        table.add_row([
            t.id.to_string(),
            name,
            t.scope.as_str().to_string(),
            player,
            format!("{}…", t.hint),
            relative(t.created_at),
            t.last_used_at.map(relative).unwrap_or_else(|| "never".into()),
        ]);
    }
    println!("{table}");
    Ok(())
}

/// A name as the terminal shows it: names are kept clean (`db::clean_name`),
/// but one from before that rule could still hold an escape sequence, so
/// those characters are printed as `\u{1b}` rather than sent to the terminal.
fn shown(name: &str) -> String {
    name.chars().map(|c| if crate::db::hidden_char(c) { c.escape_unicode().to_string() } else { c.to_string() }).collect()
}

/// Revoke by id, or by name (`find`).
pub fn revoke(cfg: &Config, which: &str) -> Result<()> {
    let (db, _cfg) = open_db(cfg)?;
    let tokens = db.tokens()?;
    let t = find(&tokens, which)?;
    db.token_revoke(t.id)?;
    println!("{} token {:?} revoked: it stops working at once", green("✓"), t.name);
    Ok(())
}

/// The token `which` names: its id (`3`, or `#3`) or its name. A number
/// that is one token's id and another's name is refused rather than
/// guessed, since revoking the wrong token can't be undone; `#3` always
/// means the id, so either token can still be named.
fn find<'a>(tokens: &'a [ApiToken], which: &str) -> Result<&'a ApiToken> {
    let which = which.trim();
    let by_id = |id: i64| tokens.iter().find(|t| t.id == id);
    let by_name = tokens.iter().find(|t| t.name.eq_ignore_ascii_case(which));
    if let Some(t) = which.strip_prefix('#').and_then(|n| n.parse().ok()).and_then(by_id) {
        return Ok(t);
    }
    match (which.parse().ok().and_then(by_id), by_name) {
        (Some(id), Some(named)) if id.id != named.id => bail!(
            "{which:?} is both token {} ({:?}) and the name of token {}; say which with `upnext token revoke '#{}'` or `upnext token revoke '#{}'`",
            id.id,
            id.name,
            named.id,
            id.id,
            named.id
        ),
        (Some(t), _) | (None, Some(t)) => Ok(t),
        (None, None) => bail!("no token {which:?}; `upnext token list` shows them"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[test]
    fn a_number_that_is_one_tokens_id_and_anothers_name_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("upnext.db"), 120).unwrap();
        for name in ["reader", "phone", "phone2", "3", "42"] {
            db.token_create(name, Scope::Read, "token").unwrap();
        }
        let tokens = db.tokens().unwrap();
        let name = |which: &str| find(&tokens, which).map(|t| t.name.as_str()).map_err(|e| e.to_string());

        let refused = name("3").unwrap_err();
        assert!(refused.contains("token 3 (\"phone2\")") && refused.contains("token 4"), "{refused}");
        assert!(refused.contains("'#3'") && refused.contains("'#4'"), "it says how to choose: {refused}");
        assert_eq!(name("#3"), Ok("phone2"));
        assert_eq!(name("#4"), Ok("3"));

        assert_eq!(name("2"), Ok("phone"), "an id no token is named after");
        assert_eq!(name(" Phone2 "), Ok("phone2"), "a name, in any case");
        assert_eq!(name("42"), Ok("42"), "a name that is no token's id");
        assert_eq!(name("5"), Ok("42"), "and that token by its id");
        assert!(name("#9").is_err() && name("tablet").is_err());
    }

    #[test]
    fn names_are_printed_without_escape_sequences() {
        assert_eq!(shown("phone"), "phone");
        assert_eq!(shown("evil\u{1b}]0;pwned\u{7}"), "evil\\u{1b}]0;pwned\\u{7}");
        assert_eq!(shown("\u{202e}enohp"), "\\u{202e}enohp");
    }
}
