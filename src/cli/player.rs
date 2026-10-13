//! `player list|add|rename|remove|pin`: the people who play on this
//! install, as Settings › Players manages them.

use super::format::{dim, green};
use super::open_db;
use crate::config::Config;
use crate::db::{Db, Player};
use anyhow::{Result, anyhow, bail};
use comfy_table::{ContentArrangement, Table, presets::UTF8_FULL_CONDENSED};
use std::io::Write;

/// A player by name or id, or a list of who there is.
fn named(db: &Db, which: &str) -> Result<Player> {
    match db.find_player(which)? {
        Some(p) => Ok(p),
        None => {
            let names: Vec<String> = db.players()?.into_iter().map(|p| p.name).collect();
            bail!("no player {which:?}; the players are {}", names.join(", "))
        }
    }
}

pub fn list(cfg: &Config) -> Result<()> {
    let (db, _cfg) = open_db(cfg)?;
    let default = db.default_player()?;
    let mut table = Table::new();
    table.load_style(UTF8_FULL_CONDENSED).set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(["Id", "Name", "PIN", "Default", "Accounts"]);
    for p in db.players()? {
        let accounts: Vec<String> = db.store_accounts_for(p.id)?.into_iter().filter(|a| a.mine).map(|a| a.name).collect();
        table.add_row([
            p.id.0.to_string(),
            p.name.clone(),
            if p.has_pin { "yes" } else { "" }.to_string(),
            if p.id == default { "yes" } else { "" }.to_string(),
            accounts.join(", "),
        ]);
    }
    println!("{table}");
    Ok(())
}

pub fn add(cfg: &Config, name: &str) -> Result<()> {
    let (db, _cfg) = open_db(cfg)?;
    let id = db.add_player(name)?;
    let p = db.player(id)?.ok_or_else(|| anyhow!("the new player vanished"))?;
    println!("{} {} added (id {}). Everything shared with everyone is theirs to use too.", green("✓"), p.name, p.id.0);
    Ok(())
}

pub fn rename(cfg: &Config, which: &str, name: &str) -> Result<()> {
    let (db, _cfg) = open_db(cfg)?;
    let p = named(&db, which)?;
    db.rename_player(p.id, name)?;
    println!("{} {} is now {}", green("✓"), p.name, db.player(p.id)?.map(|p| p.name).unwrap_or_default());
    Ok(())
}

/// Remove a player and only what is theirs, after a backup. Asks first
/// unless `yes`.
pub fn remove(cfg: &Config, which: &str, yes: bool) -> Result<()> {
    let (db, cfg) = open_db(cfg)?;
    let p = named(&db, which)?;
    if db.players()?.len() <= 1 {
        bail!("{} is the only player, and the last player can't be removed", p.name);
    }
    let takes = db.removal_takes(p.id)?;
    println!("Removing {} takes their marks, picks, profile, ratings, routines, wishlist and hidden games with them.", p.name);
    if !takes.is_empty() {
        println!("Their accounts go too, with the games only they brought: {}.", takes.join(", "));
    }
    if !yes {
        print!("Remove {}? A backup is made first. [y/N] ", p.name);
        std::io::stdout().flush()?;
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        if !matches!(line.trim(), "y" | "Y" | "yes") {
            println!("{}", dim("Nothing was removed."));
            return Ok(());
        }
    }
    let new = crate::backups::New {
        kind: crate::backups::Kind::Manual,
        about: crate::backups::About { note: None, reason: Some(format!("Before removing {}", p.name)) },
        keep: None,
        job: None,
    };
    let dir = crate::jobs::backup::backups_dir(&cfg);
    let made = crate::backups::write(&cfg.db_path, &dir, &crate::backups::Kind::Manual.stem(), &new)
        .map_err(|e| anyhow!("backing up first, so nothing was removed: {e:#}"))?;
    db.remove_player(p.id)?;
    println!("{} {} removed; the backup {} has everything as it was", green("✓"), p.name, made.name);
    Ok(())
}

/// Set a player's PIN (asked for, not taken from the command line, where
/// the shell's history would keep it), or clear it.
pub fn pin(cfg: &Config, which: &str, clear: bool) -> Result<()> {
    let (db, _cfg) = open_db(cfg)?;
    let p = named(&db, which)?;
    if clear {
        db.set_pin(p.id, None)?;
        println!("{} {} has no PIN now", green("✓"), p.name);
        return Ok(());
    }
    // Not echoed in a terminal; read as a line when it's piped in.
    let read = |ask: &str| -> Result<String> {
        print!("{ask}");
        std::io::stdout().flush()?;
        let term = console::Term::stdout();
        if term.is_term() {
            return Ok(term.read_secure_line()?.trim().to_string());
        }
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        Ok(line.trim().to_string())
    };
    let pin = read(&format!("New PIN for {} (four to eight digits): ", p.name))?;
    if !crate::db::pin_ok(&pin) {
        bail!("a PIN is four to eight digits");
    }
    if read("The same again: ")? != pin {
        bail!("the two didn't match, so the PIN wasn't changed");
    }
    db.set_pin(p.id, Some(&pin))?;
    println!("{} {} has a PIN now", green("✓"), p.name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn players_are_added_renamed_chosen_and_removed_after_a_backup() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = crate::testutil::offline_config(dir.path());
        add(&cfg, "Sam").unwrap();
        rename(&cfg, "sam", "Sam K").unwrap();
        let (db, _) = open_db(&cfg).unwrap();
        let sam = db.find_player("Sam K").unwrap().expect("renamed");

        // `--player` picks who a command acts for, by name or id.
        let with = |p: &str| {
            let mut c = cfg.clone();
            c.cli_player = Some(p.into());
            c
        };
        assert_eq!(open_db(&with("Sam K")).unwrap().1.whose(), sam.id);
        assert_eq!(open_db(&with(&sam.id.0.to_string())).unwrap().1.whose(), sam.id);
        let nobody = with("Kit");
        let refused = open_db(&nobody).err().expect("no such player");
        assert!(refused.to_string().contains("the players are"), "names who there is");

        remove(&cfg, "Sam K", true).unwrap();
        assert_eq!(db.players().unwrap().len(), 1);
        let backups = crate::jobs::backup::backups_dir(&cfg);
        assert_eq!(
            std::fs::read_dir(&backups).unwrap().filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "db")).count(),
            1
        );
        assert!(remove(&cfg, "1", true).is_err(), "the last player stays");
    }
}
