//! The wishlist in picks: wishlisted games the AI may
//! put in a lineup (one at most, with the owned game to switch to), and
//! the one game you don't own it may suggest under a lineup, checked
//! against Steam's store and your library before it's shown.

use super::{judge, owned_as, settle};
use crate::db::PlayerId;
use crate::db::{Db, GameView, NewWish, StoreFacts, Suggestion, WishGame, WishSource, WishState};
use crate::enrich::Clients;
use crate::taste::Profile;
use anyhow::Result;
use std::time::Duration;

/// How long checking a suggestion may add to a pick, at most. Each request
/// in it is tried once (`http::quick`); a suggestion not checked in time
/// isn't shown.
pub const CHECK_WAIT: Duration = Duration::from_secs(12);

/// Wishlisted games you don't own, with what the store says, for the pick
/// prompt. A game a sync found in the library leaves first. Hard nos are
/// left out where the prompt is built, with the profile.
pub fn for_picks(db: &Db, p: PlayerId, games: &[GameView]) -> Result<Vec<WishGame>> {
    settle(db, p, games)?;
    let rows: Vec<_> = db.wishlist_rows(p)?.into_iter().filter(|r| r.state == WishState::Wishlisted).collect();
    let facts = db.store_facts(&rows.iter().map(|r| r.appid).collect::<Vec<_>>())?;
    Ok(rows
        .iter()
        .filter(|r| owned_as(games, Some(r.appid), &r.name).is_none())
        .map(|r| WishGame::new(r, facts.get(&r.appid).unwrap_or(&StoreFacts::default())))
        .collect())
}

/// The AI's suggestion, checked: it's on Steam's store (a close match by
/// name, a page of type game, rated as well as the wishlist's suggestions
/// must be), you don't own it on any store UpNext syncs, you haven't
/// answered it before (Not interested, I own it elsewhere, or it's on your
/// wishlist already), and it isn't a hard no. A suggestion already waiting
/// on the Lists page is shown as it is. Passed, it waits there as a
/// suggestion from this lineup. Err says why it wasn't shown.
#[allow(clippy::too_many_arguments)]
pub async fn check_suggestion(
    db: &Db,
    p: PlayerId,
    clients: &Clients,
    games: &[GameView],
    profile: Option<&Profile>,
    name: &str,
    reason: &str,
    batch_id: &str,
) -> std::result::Result<Suggestion, String> {
    let checked = crate::http::quick(check(db, p, clients, games, profile, name, reason, batch_id));
    match tokio::time::timeout(CHECK_WAIT, checked).await {
        Ok(r) => r,
        Err(_) => Err(format!("Steam's store took over {} seconds to check it", CHECK_WAIT.as_secs())),
    }
}

#[allow(clippy::too_many_arguments)]
async fn check(
    db: &Db,
    p: PlayerId,
    clients: &Clients,
    games: &[GameView],
    profile: Option<&Profile>,
    name: &str,
    reason: &str,
    batch_id: &str,
) -> std::result::Result<Suggestion, String> {
    let (name, reason) = (name.trim(), reason.trim());
    if name.is_empty() || reason.is_empty() {
        return Err("it came without a name or a reason".into());
    }
    if let Some(g) = owned_as(games, None, name) {
        return Err(format!("you own it, as {}", g.name));
    }
    let found = crate::steam::store_search(&clients.steam, name).await.map_err(|e| format!("Steam's store didn't answer: {e:#}"))?;
    let Some((appid, found, _)) = found else {
        return Err("Steam's store has no game by that name".into());
    };
    if let Some(g) = owned_as(games, Some(appid), &found) {
        return Err(format!("you own it, as {}", g.name));
    }
    let known = db.wishlist_by_appid(p, appid).map_err(|e| format!("{e:#}"))?;
    match known.as_ref().map(|r| r.state) {
        Some(WishState::Wishlisted) => return Err(format!("{found} is on your wishlist already")),
        Some(WishState::Dismissed) => return Err(format!("you said Not interested to {found}")),
        Some(WishState::OwnedElsewhere) => return Err(format!("you own {found} elsewhere")),
        Some(WishState::InLibrary) => return Err(format!("{found} is in your library")),
        Some(WishState::Suggested) | None => {}
    }
    let facts = fetch(db, clients, appid, profile.is_some_and(Profile::checks_hard_nos)).await?;
    judge(&facts, profile)?;
    let id = match known {
        Some(r) => r.id,
        None => {
            let new = NewWish {
                appid,
                name: found,
                source: WishSource::Pick,
                reason: Some(reason.to_string()),
                state: WishState::Suggested,
                on_steam: false,
                series_key: None,
                batch_id: Some(batch_id.to_string()),
            };
            db.wishlist_add(p, &new).map_err(|e| format!("{e:#}"))?.0
        }
    };
    let game = db.wish_games(&[id]).map_err(|e| format!("{e:#}"))?.remove(&id).ok_or("its wishlist entry went missing")?;
    Ok(Suggestion { game, reason: reason.to_string() })
}

/// What the store tables lack for a suggestion, all at once: the review
/// summary, store details, Steam's tags when there are hard nos to check
/// them against, and art. The reviews and details must come back; the tags
/// too when they're needed.
async fn fetch(db: &Db, clients: &Clients, appid: u32, need_tags: bool) -> std::result::Result<StoreFacts, String> {
    let err = |what: &str, e: anyhow::Error| format!("Steam's store didn't give its {what}: {e:#}");
    let have = db.store_facts(&[appid]).map_err(|e| format!("{e:#}"))?.remove(&appid).unwrap_or_default();
    let reviews = async { if have.reviews_checked { Ok(None) } else { clients.steam.review_summary(appid).await.map(Some) } };
    let details = async { if have.details_checked { Ok(None) } else { clients.steam.app_details(appid).await.map(Some) } };
    let tags = async { if have.tags_checked || !need_tags { Ok(None) } else { clients.spy.tags(appid).await.map(Some) } };
    let art = async { if have.art_checked { Ok(None) } else { crate::steam::store_art(&clients.steam, &[appid]).await.map(Some) } };
    let (reviews, details, tags, art) = tokio::join!(reviews, details, tags, art);
    let store = |r: Result<()>| r.map_err(|e| format!("{e:#}"));
    if let Some(r) = reviews.map_err(|e| err("reviews", e))? {
        store(db.put_reviews(appid, r.as_ref()))?;
    }
    if let Some(d) = details.map_err(|e| err("details", e))? {
        store(db.put_details(appid, d.as_ref()))?;
    }
    match tags {
        Ok(Some(t)) => store(db.put_tags(appid, t.as_deref()))?,
        Ok(None) => {}
        Err(e) => return Err(format!("{}, so it couldn't be checked against your hard nos", err("tags", e))),
    }
    // Art is for the card; a game without it still shows.
    match art {
        Ok(Some(found)) => {
            let (portrait, hero, shots) = found.get(&appid).cloned().unwrap_or_default();
            store(db.put_steam_art(appid, portrait.as_deref(), hero.as_deref(), &shots))?;
        }
        Ok(None) => {}
        Err(e) => tracing::info!("art for suggestion {appid}: {e:#}"),
    }
    Ok(db.store_facts(&[appid]).map_err(|e| format!("{e:#}"))?.remove(&appid).unwrap_or_default())
}
