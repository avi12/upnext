//! A player's own backlog verdicts: the form, saving a
//! draft, accepting the consent, sending, withdrawing, listing what the
//! service has, and reporting a text. The API, the command line and the
//! scheduler call these; [`client`](super::client) talks to the service.
//!
//! Nothing is sent without the player pressing Send (or `upnext verdicts
//! send`) on a verdict, after accepting the current consent. A verdict the
//! service can't take now (it's busy or out of reach) is queued and sent
//! again by [`retry_due`] while the server runs, for up to a week; one it
//! refuses goes back to being a draft with the reason. A draft can carry
//! the service's id: then it's changes not yet sent to a verdict the
//! service still has as it was.

use super::client::{DEFAULT_RETRY_SECS, Listed, RemoteProblem, Service, ServiceError, new_key};
use super::consent::{self, CONSENT_VERSION};
use super::draft::{self, Draft};
use super::verdict::{REPORT_REASONS, Verdict};
use crate::config::Config;
use crate::db::{Db, MyVerdict, PlayerId, VerdictState, now};
use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// Why a verdict action was refused, with the HTTP status the API answers
/// it with and what the form shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub status: u16,
    pub error: String,
    pub problems: Vec<RemoteProblem>,
}

impl Refused {
    fn err(status: u16, error: impl Into<String>) -> anyhow::Error {
        anyhow::Error::new(Refused { status, error: error.into(), problems: vec![] })
    }
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.error)?;
        for p in &self.problems {
            write!(f, "; {} {}", p.field, p.why)?;
        }
        Ok(())
    }
}

impl std::error::Error for Refused {}

/// The answers a player gives in the form: a verdict without the game's
/// ids, the versions and the client, which UpNext sets.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct VerdictAnswers {
    #[serde(default)]
    pub outcome: Option<String>,
    #[serde(default)]
    pub worth: Option<String>,
    #[serde(default)]
    pub hours: Option<String>,
    #[serde(default)]
    pub clicked: Option<String>,
    #[serde(default)]
    pub sessions: Option<String>,
    #[serde(default)]
    pub limited_time: Option<String>,
    #[serde(default)]
    pub factors: Vec<String>,
    #[serde(default)]
    pub stop_point: Option<String>,
    #[serde(default)]
    pub stop_note: Option<String>,
    #[serde(default)]
    pub stopped_for: Vec<String>,
    #[serde(default)]
    pub text: Option<String>,
}

impl VerdictAnswers {
    /// The answers of a verdict.
    pub fn of(v: &Verdict) -> Self {
        VerdictAnswers {
            outcome: v.outcome.clone(),
            worth: v.worth.clone(),
            hours: v.hours.clone(),
            clicked: v.clicked.clone(),
            sessions: v.sessions.clone(),
            limited_time: v.limited_time.clone(),
            factors: v.factors.clone(),
            stop_point: v.stop_point.clone(),
            stop_note: v.stop_note.clone(),
            stopped_for: v.stopped_for.clone(),
            text: v.text.clone(),
        }
    }

    /// Put these answers on a verdict, leaving its game and versions. A
    /// blank answer or text is no answer.
    pub fn apply(self, v: &mut Verdict) {
        let given = |s: Option<String>| s.filter(|s| !s.trim().is_empty());
        v.outcome = given(self.outcome);
        v.worth = given(self.worth);
        v.hours = given(self.hours);
        v.clicked = given(self.clicked);
        v.sessions = given(self.sessions);
        v.limited_time = given(self.limited_time);
        v.factors = self.factors;
        v.stop_point = given(self.stop_point);
        v.stop_note = given(self.stop_note);
        v.stopped_for = self.stopped_for;
        v.text = given(self.text);
    }
}

/// The consent screen as the form shows it, and whether this player has
/// accepted its current version.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct VerdictConsent {
    /// The consent text's version.
    pub version: i64,
    pub title: &'static str,
    pub paragraphs: Vec<&'static str>,
    pub accept: &'static str,
    pub decline: &'static str,
    /// The line under the form's text box.
    pub text_box_line: &'static str,
    /// `CC0-1.0`.
    pub licence: &'static str,
    pub licence_url: &'static str,
    /// The service's terms, when a service is set.
    pub terms_url: Option<String>,
    /// The version this player accepted, if any.
    pub accepted: Option<i64>,
    /// The day they accepted it.
    pub accepted_on: Option<String>,
    /// They accepted the current version, so sending needs no more.
    pub current: bool,
}

/// One of the player's own verdicts as the API shows it.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct MyVerdictView {
    pub title_id: i64,
    /// The title's name.
    pub name: String,
    pub answers: VerdictAnswers,
    pub steam_appid: Option<u32>,
    pub gog_id: Option<String>,
    /// The service's id for it, once sent: the id the snapshot publishes it under.
    pub remote_id: Option<String>,
    pub state: VerdictState,
    /// When a queued verdict is tried again (Unix seconds).
    pub retry_at: Option<i64>,
    /// Why the last try didn't go through.
    pub error: Option<String>,
    /// When the service last took it (Unix seconds).
    pub sent_at: Option<i64>,
    pub updated_at: i64,
}

impl MyVerdictView {
    pub fn of(m: &MyVerdict, name: &str) -> Self {
        MyVerdictView {
            title_id: m.title_id,
            name: name.to_string(),
            answers: VerdictAnswers::of(&m.verdict),
            steam_appid: m.verdict.steam_appid,
            gog_id: m.verdict.gog_id.clone(),
            remote_id: m.remote_id.clone(),
            state: m.state,
            retry_at: m.retry_at,
            error: m.error.clone(),
            sent_at: m.sent_at,
            updated_at: m.updated_at,
        }
    }
}

/// Everything the form needs for one game.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct VerdictForm {
    pub title_id: i64,
    pub name: String,
    /// Why this game can't get a verdict, when it can't (no Steam appid or GOG id).
    pub cant_send: Option<String>,
    /// The answers to show: the player's saved or sent ones, or else the
    /// ones UpNext fills in.
    pub answers: VerdictAnswers,
    /// Where each pre-filled answer came from, by field: mark, playing,
    /// playtime, diary, stop_reasons or no_ending. Empty for saved answers.
    #[schema(value_type = std::collections::BTreeMap<String, String>)]
    pub from: std::collections::BTreeMap<&'static str, draft::Source>,
    /// What's left before it can be sent.
    pub problems: Vec<RemoteProblem>,
    /// The player's verdict as saved or sent, if any.
    pub mine: Option<MyVerdictView>,
    /// A sent verdict's outcome no longer matches the player's mark, so the
    /// page offers "Update your verdict".
    pub outdated: bool,
    /// A service address is set, so the verdict can be sent.
    pub service: bool,
    pub consent: VerdictConsent,
}

/// The player's verdicts, and what sending needs.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct VerdictsOverview {
    /// A service address is set.
    pub service: bool,
    /// The player has a key, so the service knows their verdicts.
    pub has_key: bool,
    pub consent: VerdictConsent,
    pub verdicts: Vec<MyVerdictView>,
}

/// What a send ended as.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct VerdictSent {
    pub verdict: MyVerdictView,
    /// What happened, in the form's words.
    pub message: String,
}

/// What withdrawing all of a player's verdicts did.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct VerdictsWithdrawnAll {
    /// Verdicts the service took down.
    pub withdrawn: i64,
    /// The player's key was forgotten.
    pub forgot_key: bool,
}

/// What listing the player's verdicts at the service did.
#[derive(Debug, Clone, Default, Serialize, utoipa::ToSchema)]
pub struct VerdictsRefreshed {
    /// Verdicts the service listed.
    pub listed: usize,
    /// Of them, verdicts this computer didn't have, now kept.
    pub added: usize,
    /// Of them, verdicts whose state changed (held, removed).
    pub changed: usize,
    /// Verdicts about games not in this library.
    pub unmatched: usize,
}

fn today() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

/// The consent as this player sees it.
pub fn consent_view(db: &Db, cfg: &Config, p: PlayerId) -> Result<VerdictConsent> {
    let accepted = db.verdicts_consent(p)?;
    Ok(VerdictConsent {
        version: CONSENT_VERSION,
        title: consent::TITLE,
        paragraphs: consent::PARAGRAPHS.to_vec(),
        accept: consent::ACCEPT,
        decline: consent::DECLINE,
        text_box_line: consent::TEXT_BOX_LINE,
        licence: consent::LICENCE,
        licence_url: consent::LICENCE_URL,
        terms_url: cfg.verdicts_url.as_ref().map(|u| format!("{}{}", u.trim_end_matches('/'), consent::TERMS_PATH)),
        current: accepted.as_ref().is_some_and(|(v, _)| *v == CONSENT_VERSION),
        accepted_on: accepted.as_ref().and_then(|(_, day)| day.clone()),
        accepted: accepted.map(|(v, _)| v),
    })
}

/// Accept the consent's `version`, which must be the current one.
pub fn accept_consent(db: &Db, cfg: &Config, p: PlayerId, version: i64) -> Result<VerdictConsent> {
    if version != CONSENT_VERSION {
        return Err(Refused::err(400, format!("the consent text is version {CONSENT_VERSION}; read it again and accept that one")));
    }
    db.set_verdicts_consent(p, version, &today())?;
    consent_view(db, cfg, p)
}

fn problems_of(v: &Verdict) -> Vec<RemoteProblem> {
    v.check().into_iter().filter(|p| p.field != "game").map(|p| RemoteProblem { field: p.field.into(), why: p.why }).collect()
}

/// The pre-filled draft for a title, or 404.
fn draft_for(db: &Db, cfg: &Config, title_id: i64) -> Result<Draft> {
    draft::for_title(db, cfg, title_id)?.ok_or_else(|| Refused::err(404, format!("game {title_id} not found")))
}

/// The form for a title: the player's saved or sent verdict, else what
/// UpNext fills in.
pub fn form(db: &Db, cfg: &Config, p: PlayerId, title_id: i64) -> Result<VerdictForm> {
    let d = draft_for(db, cfg, title_id)?;
    let mine = db.my_verdict(p, title_id)?;
    let (answers, from, problems) = match &mine {
        // A withdrawn or removed verdict starts the next one, as if saved.
        Some(m) => (VerdictAnswers::of(&m.verdict), Default::default(), problems_of(&m.verdict)),
        None => (VerdictAnswers::of(&d.verdict), d.from.clone(), problems_of(&d.verdict)),
    };
    let outdated = mine.as_ref().is_some_and(|m| m.state.is_out() && d.verdict.outcome.is_some() && m.verdict.outcome != d.verdict.outcome);
    Ok(VerdictForm {
        title_id,
        name: d.name.clone(),
        cant_send: d.cant_send.clone(),
        answers,
        from,
        problems,
        mine: mine.as_ref().map(|m| MyVerdictView::of(m, &d.name)),
        outdated,
        service: cfg.verdicts_url.is_some(),
        consent: consent_view(db, cfg, p)?,
    })
}

/// The player's verdicts and what sending needs.
pub fn overview(db: &Db, cfg: &Config, p: PlayerId) -> Result<VerdictsOverview> {
    Ok(VerdictsOverview {
        service: cfg.verdicts_url.is_some(),
        has_key: db.verdicts_key(p)?.is_some(),
        consent: consent_view(db, cfg, p)?,
        verdicts: db.my_verdicts(p)?.iter().map(|(m, name)| MyVerdictView::of(m, name)).collect(),
    })
}

/// The verdict to keep for a title with these answers: the game's ids as
/// the service knows them (a sent verdict's own, so it's replaced), and
/// this build's versions.
fn verdict_for(d: &Draft, mine: Option<&MyVerdict>, answers: VerdictAnswers) -> Result<Verdict> {
    if let Some(why) = &d.cant_send {
        return Err(Refused::err(400, why.clone()));
    }
    let (appid, gog) = match mine.filter(|m| m.remote_id.is_some()) {
        Some(m) => (m.verdict.steam_appid, m.verdict.gog_id.clone()),
        None => (d.verdict.steam_appid, d.verdict.gog_id.clone()),
    };
    let mut v = Verdict::new(appid, gog);
    answers.apply(&mut v);
    Ok(v)
}

/// Save the answers as a draft. A verdict the service has is replaced by
/// sending, not by saving, so the page always shows what was sent.
pub fn save_draft(db: &Db, cfg: &Config, p: PlayerId, title_id: i64, answers: VerdictAnswers) -> Result<MyVerdictView> {
    let d = draft_for(db, cfg, title_id)?;
    let mine = db.my_verdict(p, title_id)?;
    if mine.as_ref().is_some_and(|m| m.state.is_out()) {
        return Err(Refused::err(409, "this verdict is sent; send it again to replace it, or take it down"));
    }
    let v = verdict_for(&d, mine.as_ref(), answers)?;
    let mut m = mine.unwrap_or_else(|| MyVerdict::draft(title_id, v.clone()));
    m.verdict = v;
    m.state = VerdictState::Draft;
    m.retry_at = None;
    m.error = None;
    m.updated_at = now();
    db.save_my_verdict(p, &m)?;
    Ok(MyVerdictView::of(&m, &d.name))
}

/// The service's address, or why there's none.
fn service_url(cfg: &Config) -> Result<&str> {
    cfg.verdicts_url.as_deref().ok_or_else(|| {
        Refused::err(503, "no verdicts service is set (Settings › Library data, or verdicts_url), so nothing can be sent yet")
    })
}

/// The player's key, made now if they have none.
fn key_for(db: &Db, p: PlayerId) -> Result<String> {
    if let Some(k) = db.verdicts_key(p)? {
        return Ok(k);
    }
    let k = new_key()?;
    db.set_verdicts_key(p, Some(&k))?;
    Ok(k)
}

/// Send the answers about a title, after the player accepted the current
/// consent. A busy or unreachable service queues it (`state` queued); a
/// refusal is an error and leaves what the service had as it was.
pub async fn send(db: &Db, http: &Client, cfg: &Config, p: PlayerId, title_id: i64, answers: VerdictAnswers) -> Result<VerdictSent> {
    let url = service_url(cfg)?;
    if !consent_view(db, cfg, p)?.current {
        return Err(Refused::err(409, "accept the consent screen first: everything you send is published under CC0"));
    }
    let d = draft_for(db, cfg, title_id)?;
    let mine = db.my_verdict(p, title_id)?;
    let v = verdict_for(&d, mine.as_ref(), answers)?;
    let problems = problems_of(&v);
    if !problems.is_empty() {
        return Err(anyhow::Error::new(Refused { status: 400, error: "the verdict isn't ready to send".into(), problems }));
    }
    let key = key_for(db, p)?;
    let mut m = mine.unwrap_or_else(|| MyVerdict::draft(title_id, v.clone()));
    let before = m.clone();
    m.verdict = v;
    m.updated_at = now();
    let out = deliver(&Service::new(http, url, &key), &mut m, now()).await;
    match out {
        Ok(message) => {
            db.save_my_verdict(p, &m)?;
            Ok(VerdictSent { verdict: MyVerdictView::of(&m, &d.name), message })
        }
        Err(r) => {
            // A refused replacement leaves the sent verdict as it was; a
            // refused new one is kept as a draft with the reason.
            if !before.state.is_out() {
                m.state = VerdictState::Draft;
                m.retry_at = None;
                m.error = Some(r.error.clone());
                db.save_my_verdict(p, &m)?;
            }
            Err(anyhow::Error::new(r))
        }
    }
}

/// Try to hand `m` to the service: on success it's sent or held; when the
/// service is busy or out of reach it's queued to try again. The words for
/// the player, or the service's refusal.
async fn deliver(service: &Service<'_>, m: &mut MyVerdict, at: i64) -> Result<String, Refused> {
    match service.send(&m.verdict).await {
        Ok(taken) => {
            m.remote_id = Some(taken.id);
            m.state = if taken.state == "held" { VerdictState::Held } else { VerdictState::Sent };
            m.sent_at = Some(at);
            m.retry_at = None;
            m.error = None;
            Ok(match (m.state, taken.replaced) {
                (VerdictState::Held, _) => {
                    "Sent. The service is holding it for now, as this game is getting a lot of verdicts; it's counted once a \
                     moderator looks, or in a week."
                        .into()
                }
                (_, true) => "Sent. It replaces your earlier verdict from the next daily update.".into(),
                _ => "Sent. It's published with the next daily update.".into(),
            })
        }
        Err(ServiceError::Limited { retry_after }) => {
            let e = ServiceError::Limited { retry_after }.to_string();
            m.state = VerdictState::Queued;
            m.retry_at = Some(at + retry_after);
            m.error = Some(e);
            Ok(format!("Saved. The service is taking no more for now, so UpNext sends it in {}.", super::client::wait_words(retry_after)))
        }
        Err(ServiceError::Down(why)) => {
            m.state = VerdictState::Queued;
            m.retry_at = Some(at + DEFAULT_RETRY_SECS);
            m.error = Some(ServiceError::Down(why).to_string());
            Ok("Saved. The service couldn't be reached, so UpNext tries again in an hour while it runs.".into())
        }
        Err(ServiceError::Refused { status, error, problems }) => Err(refusal(status, error, problems)),
    }
}

/// A refusal from the service, as the API answers it.
fn refusal(status: u16, error: String, problems: Vec<RemoteProblem>) -> Refused {
    match status {
        400 => Refused { status: 400, error, problems },
        403 => Refused { status: 403, error: format!("the verdicts service blocked this player's key: {error}"), problems },
        404 => Refused { status: 404, error, problems },
        // 401, 413, 415 and anything else: UpNext sent something the
        // service didn't expect, which the player can't fix.
        _ => Refused { status: 502, error: format!("the verdicts service refused it ({status}): {error}"), problems },
    }
}

/// How long a queued verdict is tried again before it goes back to being a
/// draft: a week from when the player sent it.
pub const QUEUE_SECS: i64 = 7 * 86_400;

/// Send the queued verdicts whose time has come, every player's. A player
/// who hasn't accepted the current consent, or a verdict queued for over a
/// week, goes back to being a draft (unsent changes, when the service has
/// an earlier one). How many the service took.
pub async fn retry_due(db: &Db, http: &Client, cfg: &Config, at: i64) -> Result<usize> {
    let Some(url) = cfg.verdicts_url.as_deref() else { return Ok(0) };
    let mut taken = 0;
    for (p, mut m) in db.queued_verdicts(at)? {
        let key = db.verdicts_key(p)?;
        let consented = db.verdicts_consent(p)?.is_some_and(|(v, _)| v == CONSENT_VERSION);
        let expired = at - m.updated_at > QUEUE_SECS;
        let Some(key) = key.filter(|_| consented && !expired) else {
            m.state = VerdictState::Draft;
            m.retry_at = None;
            m.error = Some(if expired {
                "Not sent: the service didn't take it within a week. Send it again from the game page.".into()
            } else {
                "Not sent: accept the consent screen again to send it.".into()
            });
            db.save_my_verdict(p, &m)?;
            continue;
        };
        m.verdict.consent = CONSENT_VERSION;
        match deliver(&Service::new(http, url, &key), &mut m, at).await {
            Ok(_) => {
                if m.state.is_out() {
                    taken += 1;
                }
            }
            // Refused: the answers here were never taken, so they're a
            // draft again; any earlier verdict stays on the service as it was.
            Err(r) => {
                m.state = VerdictState::Draft;
                m.retry_at = None;
                m.error = Some(r.to_string());
            }
        }
        db.save_my_verdict(p, &m)?;
    }
    Ok(taken)
}

/// Take a verdict down. One never sent is forgotten (None); a sent one is
/// withdrawn at the service and kept as withdrawn.
pub async fn withdraw(db: &Db, http: &Client, cfg: &Config, p: PlayerId, title_id: i64) -> Result<Option<MyVerdictView>> {
    let Some(mut m) = db.my_verdict(p, title_id)? else {
        return Err(Refused::err(404, format!("there's no verdict of yours about game {title_id}")));
    };
    let Some(id) = m.remote_id.clone().filter(|_| m.state != VerdictState::Withdrawn) else {
        if m.state == VerdictState::Withdrawn {
            return Err(Refused::err(409, "that verdict is taken down already"));
        }
        db.delete_my_verdict(p, title_id)?;
        return Ok(None);
    };
    let url = service_url(cfg)?;
    let Some(key) = db.verdicts_key(p)? else {
        return Err(Refused::err(
            409,
            "this player's verdicts key is gone, so the service can't tell this verdict is yours; report it from the game page instead",
        ));
    };
    let found = match Service::new(http, url, &key).withdraw(&id).await {
        Ok(found) => found,
        Err(ServiceError::Down(why)) => {
            return Err(Refused::err(502, format!("{}; nothing changed, so try again", ServiceError::Down(why))));
        }
        Err(e @ ServiceError::Limited { .. }) => return Err(Refused::err(429, e.to_string())),
        Err(ServiceError::Refused { status, error, problems }) => return Err(anyhow::Error::new(refusal(status, error, problems))),
    };
    // Not found: a moderator removed it, or it was taken down already
    // (from a copy of this database, say). It's gone either way.
    if found {
        m.state = VerdictState::Withdrawn;
        m.error = None;
    } else {
        m.state = VerdictState::Removed;
        m.error = Some("The service no longer had it: its moderators removed it, or it was taken down already.".into());
    }
    m.retry_at = None;
    m.updated_at = now();
    db.save_my_verdict(p, &m)?;
    let name = db.game(p, title_id)?.map(|g| g.name).unwrap_or_default();
    Ok(Some(MyVerdictView::of(&m, &name)))
}

/// "Take down all my verdicts and forget my key": withdraw every verdict of
/// the player's key at the service, then forget the key. Queued verdicts go
/// back to being drafts, so nothing is sent under a new key unasked.
pub async fn withdraw_all(db: &Db, http: &Client, cfg: &Config, p: PlayerId) -> Result<VerdictsWithdrawnAll> {
    let key = db.verdicts_key(p)?;
    let mut withdrawn = 0;
    if let Some(key) = &key {
        let url = service_url(cfg)?;
        withdrawn = match Service::new(http, url, key).withdraw_all().await {
            Ok(n) => n,
            Err(ServiceError::Down(why)) => {
                return Err(Refused::err(502, format!("{}; nothing changed and the key is kept, so try again", ServiceError::Down(why))));
            }
            Err(e @ ServiceError::Limited { .. }) => return Err(Refused::err(429, e.to_string())),
            Err(ServiceError::Refused { status, error, problems }) => return Err(anyhow::Error::new(refusal(status, error, problems))),
        };
    }
    for (mut m, _) in db.my_verdicts(p)? {
        let next = match (m.state, m.remote_id.is_some()) {
            // Unsent changes to a verdict the service had: that one is gone now.
            (VerdictState::Sent | VerdictState::Held | VerdictState::Queued | VerdictState::Draft, true) => VerdictState::Withdrawn,
            (VerdictState::Queued, false) => VerdictState::Draft,
            (s, _) => s,
        };
        if next != m.state {
            m.state = next;
            m.retry_at = None;
            m.error = None;
            m.updated_at = now();
            db.save_my_verdict(p, &m)?;
        }
    }
    db.set_verdicts_key(p, None)?;
    Ok(VerdictsWithdrawnAll { withdrawn, forgot_key: key.is_some() })
}

/// The state a listed verdict is in, as this install keeps it.
fn listed_state(l: &Listed) -> VerdictState {
    match l.state.as_str() {
        "held" => VerdictState::Held,
        "withdrawn" => VerdictState::Withdrawn,
        "removed" => VerdictState::Removed,
        _ => VerdictState::Sent,
    }
}

/// Midnight UTC of a `YYYY-MM-DD` day.
fn day_start(day: &str) -> Option<i64> {
    chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").ok().and_then(|d| d.and_hms_opt(0, 0, 0)).map(|t| t.and_utc().timestamp())
}

/// Ask the service for the player's verdicts and bring this computer's
/// list up to date: verdicts it lost (a reset, or a restored backup older
/// than them) come back, and held or removed ones say so. A queued change
/// waiting to be sent is left as it is.
pub async fn refresh(db: &Db, http: &Client, cfg: &Config, p: PlayerId) -> Result<VerdictsRefreshed> {
    let url = service_url(cfg)?;
    let Some(key) = db.verdicts_key(p)? else { return Ok(VerdictsRefreshed::default()) };
    let listed = match Service::new(http, url, &key).mine().await {
        Ok(l) => l,
        Err(ServiceError::Down(why)) => return Err(Refused::err(502, ServiceError::Down(why).to_string())),
        Err(e @ ServiceError::Limited { .. }) => return Err(Refused::err(429, e.to_string())),
        Err(ServiceError::Refused { status, error, problems }) => return Err(anyhow::Error::new(refusal(status, error, problems))),
    };
    let mut out = VerdictsRefreshed { listed: listed.len(), ..Default::default() };
    let mine = db.my_verdicts(p)?;
    for l in &listed {
        let state = listed_state(l);
        if let Some((m, _)) = mine.iter().find(|(m, _)| m.remote_id.as_deref() == Some(l.id.as_str())) {
            // Changes waiting or refused (queued, or a draft that keeps the
            // service's id) and a verdict taken down stay as they are: the
            // row holds answers the service doesn't have.
            let unsent = matches!(m.state, VerdictState::Queued | VerdictState::Draft | VerdictState::Withdrawn);
            if m.state != state && !unsent {
                let mut m = m.clone();
                m.state = state;
                m.updated_at = now();
                db.save_my_verdict(p, &m)?;
                out.changed += 1;
            }
            continue;
        }
        let Some(title_id) = db.title_for_verdict(l.steam_appid, l.gog_id.as_deref())? else {
            out.unmatched += 1;
            continue;
        };
        // A verdict this computer keeps about the game under another id, or
        // a newer draft, wins over the service's listing.
        if mine.iter().any(|(m, _)| m.title_id == title_id) || db.my_verdict(p, title_id)?.is_some() {
            continue;
        }
        let mut m = MyVerdict::draft(title_id, l.verdict());
        m.remote_id = Some(l.id.clone());
        m.state = state;
        m.sent_at = day_start(&l.updated_day);
        m.updated_at = now();
        db.save_my_verdict(p, &m)?;
        out.added += 1;
    }
    Ok(out)
}

/// Report a published text to the service with the player's key.
pub async fn report(db: &Db, http: &Client, cfg: &Config, p: PlayerId, verdict_id: &str, reason: &str) -> Result<()> {
    if !REPORT_REASONS.iter().any(|(code, _)| *code == reason) {
        let codes: Vec<&str> = REPORT_REASONS.iter().map(|(c, _)| *c).collect();
        return Err(Refused::err(400, format!("a report's reason is one of {}", codes.join(", "))));
    }
    if verdict_id.len() != 26 || !verdict_id.bytes().all(|b| b.is_ascii_lowercase() || (b'2'..=b'7').contains(&b)) {
        return Err(Refused::err(400, format!("{verdict_id:?} isn't a verdict id")));
    }
    let url = service_url(cfg)?;
    // A report goes with the player's key, made the first time; the consent
    // screen is where the player learns about the key, so it comes first.
    if !consent_view(db, cfg, p)?.current {
        return Err(Refused::err(409, "accept the consent screen first: reports are sent with your anonymous verdicts key"));
    }
    let key = key_for(db, p)?;
    match Service::new(http, url, &key).report(verdict_id, reason).await {
        Ok(()) => Ok(()),
        Err(ServiceError::Refused { status: 404, .. }) => Err(Refused::err(404, "that verdict isn't published, or was taken down")),
        Err(e @ ServiceError::Limited { .. }) => Err(Refused::err(429, e.to_string())),
        Err(ServiceError::Down(why)) => Err(Refused::err(502, ServiceError::Down(why).to_string())),
        Err(ServiceError::Refused { status, error, problems }) => Err(anyhow::Error::new(refusal(status, error, problems))),
    }
}
