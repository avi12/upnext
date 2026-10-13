//! `gog-login` and `gog-logout`, and Epic's `epic-login` and `epic-logout`,
//! which work the same way.

use super::format::green;
use crate::config::Config;
use crate::enrich::Clients;
use anyhow::Result;

/// `code` is the code or redirect URL; when omitted it is read from stdin.
pub async fn login(cfg: &Config, code: Option<String>) -> Result<()> {
    let clients = Clients::new(cfg)?;
    println!("1. Open this URL and log in to GOG:\n\n   {}\n", crate::gog::auth_url());
    println!("2. After login you land on a mostly blank page. Copy its full URL (it contains code=...).");
    let code = match code {
        Some(c) => c,
        None => {
            print!("3. Paste it here: ");
            use std::io::Write;
            std::io::stdout().flush()?;
            let mut line = String::new();
            std::io::stdin().read_line(&mut line)?;
            line
        }
    };
    let status = clients.gog.login(&code).await?;
    println!(
        "{} connected as {} (user id {}). The sign-in is kept in {}. Run `upnext sync` to pull playtime.",
        green("✓"),
        status.username.clone().unwrap_or_else(|| "?".into()),
        status.user_id.clone().unwrap_or_default(),
        cfg.db_path.display()
    );
    Ok(())
}

pub async fn logout(cfg: &Config) -> Result<()> {
    let clients = Clients::new(cfg)?;
    clients.gog.disconnect().await?;
    println!("{} GOG sign-in removed", green("✓"));
    Ok(())
}

/// `code` is the code or Epic's whole page; when omitted it is read from stdin.
pub async fn epic_login(cfg: &Config, code: Option<String>) -> Result<()> {
    let clients = Clients::new(cfg)?;
    println!("1. Open this URL and sign in to Epic:\n\n   {}\n", crate::epic::auth_url());
    println!("2. Epic then shows a short page of text with \"authorizationCode\" in it. Copy the code (or the whole page).");
    let code = match code {
        Some(c) => c,
        None => {
            print!("3. Paste it here: ");
            use std::io::Write;
            std::io::stdout().flush()?;
            let mut line = String::new();
            std::io::stdin().read_line(&mut line)?;
            line
        }
    };
    let status = clients.epic.login(&code).await?;
    println!(
        "{} signed in to Epic as {}. The sign-in is kept in {}. Run `upnext sync` to read your Epic library.",
        green("✓"),
        status.display_name.unwrap_or_else(|| "?".into()),
        cfg.db_path.display()
    );
    Ok(())
}

pub async fn epic_logout(cfg: &Config) -> Result<()> {
    let clients = Clients::new(cfg)?;
    clients.epic.disconnect().await?;
    println!("{} Epic sign-in removed; the Epic games already read stay", green("✓"));
    Ok(())
}
