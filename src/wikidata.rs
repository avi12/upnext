//! Wikidata cross-references: for a Steam appid or an exact English title,
//! the PCGamingWiki page, RAWG slug, HowLongToBeat id, IGDB slug and Epic
//! slug. Lets the other sources match by id instead of by fuzzy name.
//! No key; batched SPARQL queries against query.wikidata.org.

use crate::http::{Backoff, RateLimiter, base_url, send_with_backoff};
use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

const BASE: &str = "https://query.wikidata.org";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Xref {
    pub qid: String,
    pub pcgw_page: Option<String>,
    pub rawg_slug: Option<String>,
    pub hltb_id: Option<i64>,
    pub igdb_slug: Option<String>,
    pub epic_slug: Option<String>,
}

pub struct WikidataClient {
    http: Client,
    base: String,
    limiter: RateLimiter,
}

const OPTIONALS: &str = "OPTIONAL { ?item wdt:P6337 ?pcgw } OPTIONAL { ?item wdt:P9968 ?rawg } \
     OPTIONAL { ?item wdt:P2816 ?hltb } OPTIONAL { ?item wdt:P5794 ?igdb } OPTIONAL { ?item wdt:P6278 ?epic }";

fn sparql_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn cell(b: &Value, k: &str) -> Option<String> {
    b.pointer(&format!("/{k}/value")).and_then(Value::as_str).map(str::to_string).filter(|s| !s.is_empty())
}

/// Merge a result row into `out[key]`; the first value seen for each field wins
/// (Wikidata sometimes lists several IGDB slugs for editions).
fn merge_row(out: &mut HashMap<String, Xref>, key: String, b: &Value) {
    let Some(item) = cell(b, "item") else { return };
    let qid = item.rsplit('/').next().unwrap_or_default().to_string();
    let e = out.entry(key).or_insert_with(|| Xref { qid: qid.clone(), ..Default::default() });
    if e.qid != qid {
        return; // two items share the key; keep the first
    }
    e.pcgw_page = e.pcgw_page.take().or_else(|| cell(b, "pcgw"));
    e.rawg_slug = e.rawg_slug.take().or_else(|| cell(b, "rawg"));
    e.hltb_id = e.hltb_id.or_else(|| cell(b, "hltb").and_then(|s| s.parse().ok()));
    e.igdb_slug = e.igdb_slug.take().or_else(|| cell(b, "igdb"));
    e.epic_slug = e.epic_slug.take().or_else(|| cell(b, "epic"));
}

impl WikidataClient {
    /// `base` stands in for the query service (tests pass a local mock server).
    pub fn new(http: Client, base: Option<&str>) -> Self {
        Self { http, base: base_url(BASE, base), limiter: RateLimiter::new("Wikidata", Duration::from_millis(1000)) }
    }

    async fn query(&self, sparql: &str) -> Result<Vec<Value>> {
        let backoff = Backoff { attempts: 4, base: Duration::from_secs(5), max: Duration::from_secs(60), ..Default::default() };
        let resp = send_with_backoff(&self.limiter, &backoff, || {
            self.http
                .post(format!("{}/sparql", self.base))
                .header("accept", "application/sparql-results+json")
                .timeout(Duration::from_secs(90))
                .form(&[("query", sparql)])
        })
        .await?;
        let v: Value = resp.json().await.context("Wikidata returned non-JSON")?;
        // An answer without bindings is an error page, not "no results", so
        // the callers count it as a failure and ask again.
        v.pointer("/results/bindings").and_then(Value::as_array).cloned().context("Wikidata's answer has no results")
    }

    /// Cross-references keyed by Steam appid. Call with at most ~200 ids.
    pub async fn by_steam_appids(&self, appids: &[u32]) -> Result<HashMap<u32, Xref>> {
        if appids.is_empty() {
            return Ok(HashMap::new());
        }
        let values: Vec<String> = appids.iter().map(|a| sparql_str(&a.to_string())).collect();
        let q = format!(
            "SELECT ?item ?steam ?pcgw ?rawg ?hltb ?igdb ?epic WHERE {{ VALUES ?steam {{ {} }} ?item wdt:P1733 ?steam . {OPTIONALS} }}",
            values.join(" ")
        );
        Ok(xrefs_by_appid(&self.query(&q).await?))
    }

    /// GOG product ids keyed by Steam appid, for UpNext's shared data, so
    /// GOG copies match it too. The numeric product id GOG copies carry is
    /// "GOG product ID" (P12727); "GOG application ID" (P2725) is the store
    /// page's slug ("game/portal_2"), asked for only to count the games on GOG
    /// that Wikidata gives no product id and to look those up in GOG's own
    /// catalogue. An appid with two product ids or two store pages (editions)
    /// is left out rather than guessed. Call with at most ~200 ids.
    pub async fn gog_ids_by_steam_appids(&self, appids: &[u32]) -> Result<GogIds> {
        if appids.is_empty() {
            return Ok(GogIds::default());
        }
        let values: Vec<String> = appids.iter().map(|a| sparql_str(&a.to_string())).collect();
        let q = format!(
            "SELECT DISTINCT ?steam ?gog ?page WHERE {{ VALUES ?steam {{ {} }} ?item wdt:P1733 ?steam . \
             OPTIONAL {{ ?item wdt:P12727 ?gog }} OPTIONAL {{ ?item wdt:P2725 ?page }} FILTER(BOUND(?gog) || BOUND(?page)) }}",
            values.join(" ")
        );
        Ok(gog_ids_by_appid(&self.query(&q).await?))
    }

    /// Cross-references keyed by the exact English label or alias of a video
    /// game. Names shared by several games (remakes, unrelated games with the
    /// same title) are dropped rather than guessed. Call with at most ~50 names.
    pub async fn by_names(&self, names: &[String]) -> Result<HashMap<String, Xref>> {
        if names.is_empty() {
            return Ok(HashMap::new());
        }
        let values: Vec<String> = names.iter().flat_map(|n| [format!("{}@en", sparql_str(n)), format!("{}@mul", sparql_str(n))]).collect();
        let q = format!(
            "SELECT ?item ?name ?pcgw ?rawg ?hltb ?igdb ?epic WHERE {{ VALUES ?name {{ {} }} ?item rdfs:label|skos:altLabel ?name ; wdt:P31/wdt:P279* wd:Q7889 . {OPTIONALS} }}",
            values.join(" ")
        );
        Ok(xrefs_by_name(&self.query(&q).await?))
    }
}

fn xrefs_by_appid(rows: &[Value]) -> HashMap<u32, Xref> {
    let mut out: HashMap<String, Xref> = HashMap::new();
    for b in rows {
        if let Some(s) = cell(b, "steam") {
            merge_row(&mut out, s, b);
        }
    }
    out.into_iter().filter_map(|(k, v)| k.parse().ok().map(|k| (k, v))).collect()
}

/// What one GOG id query found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GogIds {
    /// Steam appid to GOG product id.
    pub ids: HashMap<u32, String>,
    /// Appids with a GOG store page on Wikidata but no product id, which no
    /// GOG copy can match until GOG's catalogue gives the page's id.
    pub page_only: usize,
    /// Steam appid to the slug of its one GOG store page ("portal_2"),
    /// with or without a product id.
    pub pages: HashMap<u32, String>,
    /// Appids with two product ids, left out.
    pub ambiguous: usize,
}

/// Steam appid to GOG product id, from rows of `?steam ?gog ?page`. Only
/// numeric ids count (what GOG copies carry), and an appid with two is
/// dropped; the rest are counted, so a run that finds none says why.
fn gog_ids_by_appid(rows: &[Value]) -> GogIds {
    let mut ids: HashMap<u32, std::collections::BTreeSet<String>> = HashMap::new();
    // A page that isn't a game's (None here) still says the game is on GOG.
    let mut pages: HashMap<u32, std::collections::BTreeSet<Option<String>>> = HashMap::new();
    for b in rows {
        let Some(appid) = cell(b, "steam").and_then(|s| s.parse().ok()) else { continue };
        if let Some(page) = cell(b, "page") {
            pages.entry(appid).or_default().insert(crate::gogmeta::store_slug(&page).map(str::to_string));
        }
        if let Some(gog) = cell(b, "gog").filter(|g| g.chars().all(|c| c.is_ascii_digit())) {
            ids.entry(appid).or_default().insert(gog);
        }
    }
    let ambiguous = ids.values().filter(|g| g.len() > 1).count();
    let page_only = pages.keys().filter(|a| !ids.contains_key(a)).count();
    let ids = ids.into_iter().filter(|(_, g)| g.len() == 1).filter_map(|(a, g)| Some((a, g.into_iter().next()?))).collect();
    let pages = pages.into_iter().filter(|(_, p)| p.len() == 1).filter_map(|(a, p)| Some((a, p.into_iter().next()??))).collect();
    GogIds { ids, page_only, pages, ambiguous }
}

/// Names matching exactly one item; shared names are dropped, not guessed.
fn xrefs_by_name(rows: &[Value]) -> HashMap<String, Xref> {
    let mut items: HashMap<String, std::collections::HashSet<String>> = HashMap::new();
    for b in rows {
        if let (Some(n), Some(i)) = (cell(b, "name"), cell(b, "item")) {
            items.entry(n).or_default().insert(i);
        }
    }
    let mut out: HashMap<String, Xref> = HashMap::new();
    for b in rows {
        if let Some(n) = cell(b, "name")
            && items.get(&n).is_some_and(|s| s.len() == 1)
        {
            merge_row(&mut out, n, b);
        }
    }
    out
}

/// A title's series: (English label, series QID) pairs, and its earliest year.
pub type SeriesInfo = (Vec<(String, String)>, Option<i32>);

fn qid_of(uri: &str) -> String {
    uri.rsplit('/').next().unwrap_or_default().to_string()
}

fn valid_qid(q: &str) -> bool {
    q.len() > 1 && q.starts_with('Q') && q[1..].chars().all(|c| c.is_ascii_digit())
}

/// Series (P179, English label and QID) and the earliest publication year
/// (P577) for Wikidata items, keyed by QID. Call with at most ~200 QIDs.
pub async fn series_for(client: &WikidataClient, qids: &[String]) -> Result<HashMap<String, SeriesInfo>> {
    let values: Vec<String> = qids.iter().filter(|q| valid_qid(q)).map(|q| format!("wd:{q}")).collect();
    if values.is_empty() {
        return Ok(HashMap::new());
    }
    let q = format!(
        "SELECT ?item ?s ?sl (MIN(?y) AS ?year) WHERE {{ VALUES ?item {{ {} }} \
         OPTIONAL {{ ?item wdt:P179 ?s . ?s rdfs:label ?sl FILTER(lang(?sl) IN (\"en\", \"mul\")) }} \
         OPTIONAL {{ ?item wdt:P577 ?d BIND(YEAR(?d) AS ?y) }} }} GROUP BY ?item ?s ?sl",
        values.join(" ")
    );
    Ok(series_info(&client.query(&q).await?))
}

fn series_info(rows: &[Value]) -> HashMap<String, SeriesInfo> {
    let mut out: HashMap<String, SeriesInfo> = HashMap::new();
    for b in rows {
        let Some(item) = cell(b, "item") else { continue };
        let e = out.entry(qid_of(&item)).or_default();
        if let (Some(sl), Some(s)) = (cell(b, "sl"), cell(b, "s")) {
            let pair = (sl, qid_of(&s));
            if !e.0.contains(&pair) {
                e.0.push(pair);
            }
        }
        if let Some(y) = cell(b, "year").and_then(|y| y.parse::<i32>().ok()) {
            e.1 = Some(e.1.map_or(y, |old| old.min(y)));
        }
    }
    out
}

/// A game in a series, whether or not the person owns it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct SeriesEntry {
    pub qid: String,
    pub name: String,
    pub year: Option<i32>,
    pub steam_appid: Option<u32>,
    /// IGDB slug and GOG product id: where to find a cover when Steam has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub igdb_slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gog_id: Option<String>,
    /// The slug of its GOG store page ("portal_2"), which GOG's catalogue
    /// turns into the product id when Wikidata has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gog_slug: Option<String>,
    /// Cover art URL, once looked up.
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub cover_checked: bool,
}

/// Every game (not DLC or expansion) in each series, keyed by series QID.
/// Call with at most ~25 series QIDs.
pub async fn series_members(client: &WikidataClient, series: &[String]) -> Result<HashMap<String, Vec<SeriesEntry>>> {
    let values: Vec<String> = series.iter().filter(|q| valid_qid(q)).map(|q| format!("wd:{q}")).collect();
    if values.is_empty() {
        return Ok(HashMap::new());
    }
    // Q7889 video game; Q209163 expansion pack; Q1066707 downloadable content.
    // Only games you could get on PC: Windows (Q1406), or on Steam, GOG or Epic.
    // The GOG id is the numeric product id (P12727) GOG's store image is
    // found by. Next to no item has one, so P2725, the store page's slug, is
    // asked for too, for GOG's catalogue to give the id.
    let q = format!(
        "SELECT ?s ?g ?gl (MIN(?y) AS ?year) (SAMPLE(?steam) AS ?appid) (SAMPLE(?igdb) AS ?igdbslug) (SAMPLE(?gog) AS ?gogid) \
         (SAMPLE(?gogpage) AS ?gogslug) WHERE {{ VALUES ?s {{ {} }} \
         ?g wdt:P179 ?s ; wdt:P31/wdt:P279* wd:Q7889 ; rdfs:label ?gl FILTER(lang(?gl) IN (\"en\", \"mul\")) \
         FILTER NOT EXISTS {{ ?g wdt:P31/wdt:P279* wd:Q209163 }} FILTER NOT EXISTS {{ ?g wdt:P31/wdt:P279* wd:Q1066707 }} \
         FILTER EXISTS {{ {{ ?g wdt:P400 wd:Q1406 }} UNION {{ ?g wdt:P1733 [] }} UNION {{ ?g wdt:P2725 [] }} UNION {{ ?g wdt:P6278 [] }} }} \
         OPTIONAL {{ ?g wdt:P577 ?d BIND(YEAR(?d) AS ?y) }} OPTIONAL {{ ?g wdt:P1733 ?steam }} \
         OPTIONAL {{ ?g wdt:P5794 ?igdb }} OPTIONAL {{ ?g wdt:P12727 ?gog }} OPTIONAL {{ ?g wdt:P2725 ?gogpage }} }} GROUP BY ?s ?g ?gl",
        values.join(" ")
    );
    Ok(series_entries(&client.query(&q).await?))
}

fn series_entries(rows: &[Value]) -> HashMap<String, Vec<SeriesEntry>> {
    let mut out: HashMap<String, Vec<SeriesEntry>> = HashMap::new();
    for b in rows {
        let (Some(s), Some(g), Some(name)) = (cell(b, "s"), cell(b, "g"), cell(b, "gl")) else { continue };
        let list = out.entry(qid_of(&s)).or_default();
        let qid = qid_of(&g);
        if list.iter().any(|e| e.qid == qid) {
            continue;
        }
        list.push(SeriesEntry {
            qid,
            name,
            year: cell(b, "year").and_then(|y| y.parse().ok()),
            steam_appid: cell(b, "appid").and_then(|a| a.parse().ok()),
            igdb_slug: cell(b, "igdbslug"),
            gog_id: cell(b, "gogid"),
            gog_slug: cell(b, "gogslug").and_then(|p| crate::gogmeta::store_slug(&p).map(str::to_string)),
            cover: None,
            cover_checked: false,
        });
    }
    out
}

/// Series items (a "video game series", Q7058673, or a "media franchise",
/// Q196600, that some video game is part of) by exact English label, for series named by other sources (PCGamingWiki) without a QID.
pub async fn series_by_labels(client: &WikidataClient, labels: &[String]) -> Result<HashMap<String, String>> {
    if labels.is_empty() {
        return Ok(HashMap::new());
    }
    // Wikidata now keeps many names only as the language-neutral "mul" label.
    let values: Vec<String> = labels.iter().flat_map(|l| [format!("{}@en", sparql_str(l)), format!("{}@mul", sparql_str(l))]).collect();
    let q = format!(
        "SELECT ?l ?s WHERE {{ VALUES ?l {{ {} }} VALUES ?kind {{ wd:Q7058673 wd:Q196600 }} ?s rdfs:label ?l ; wdt:P31/wdt:P279* ?kind . \
         FILTER EXISTS {{ ?g wdt:P179 ?s ; wdt:P31/wdt:P279* wd:Q7889 }} }}",
        values.join(" ")
    );
    Ok(series_qids_by_label(&client.query(&q).await?))
}

/// Label -> series QID; labels naming two different series are dropped.
fn series_qids_by_label(rows: &[Value]) -> HashMap<String, String> {
    let mut out: HashMap<String, String> = HashMap::new();
    for b in rows {
        if let (Some(l), Some(s)) = (cell(b, "l"), cell(b, "s")) {
            // The same item can match under both "en" and "mul".
            // Ambiguous labels (two series items) are dropped rather than guessed.
            let qid = qid_of(&s);
            match out.get(&l) {
                Some(existing) if existing != &qid => {
                    out.insert(l, String::new());
                }
                _ => {
                    out.insert(l, qid);
                }
            }
        }
    }
    out.retain(|_, q| !q.is_empty());
    out
}

/// Names worth trying as exact Wikidata labels for a title: the name as-is,
/// without trademark glyphs and edition suffixes, and "X, The" as "The X".
pub fn label_candidates(name: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![name.trim().to_string()];
    let clean = crate::names::normalize_name(name);
    out.push(clean.clone());
    for n in [name.trim().to_string(), clean] {
        if let Some(base) = n.strip_suffix(", The") {
            out.push(format!("The {base}"));
        }
    }
    out.retain(|s| !s.is_empty());
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gog_ids() {
        let row = |steam: &str, gog: &str| serde_json::json!({"steam": {"value": steam}, "gog": {"value": gog}});
        let page = |steam: &str, page: &str| serde_json::json!({"steam": {"value": steam}, "page": {"value": page}});
        let mut both = row("620", "1207658930");
        both["page"] = serde_json::json!({"value": "game/portal_2"});
        let rows = [
            both,
            row("10", "1"),
            row("10", "2"),
            row("70", "half-life"),
            row("x", "3"),
            page("400", "game/portal"),
            page("220", "game/half_life_2"),
            page("220", "game/half_life_2_episode_one"),
            page("240", "movie/a_film"),
        ];
        let found = gog_ids_by_appid(&rows);
        assert_eq!(found.ids, HashMap::from([(620, "1207658930".to_string())]), "two ids, a slug and a bad appid are left out");
        assert_eq!(
            (found.page_only, found.ambiguous),
            (3, 1),
            "Portal, appids 220 and 240 have store pages and no product id; appid 10 has two ids"
        );
        let pages = HashMap::from([(620, "portal_2".to_string()), (400, "portal".to_string())]);
        assert_eq!(found.pages, pages, "two pages, or one that isn't a game's, are left out");
    }

    #[test]
    fn labels() {
        assert_eq!(label_candidates("Witcher 3: Wild Hunt, The"), vec!["Witcher 3: Wild Hunt, The", "The Witcher 3: Wild Hunt"]);
        assert_eq!(label_candidates("Grime™ - Definitive Edition"), vec!["Grime™ - Definitive Edition", "Grime"]);
    }
}

#[cfg(test)]
mod row_tests {
    use super::*;
    use crate::testutil::fixture_json;

    fn rows(rel: &str) -> Vec<Value> {
        fixture_json(rel).pointer("/results/bindings").and_then(Value::as_array).cloned().unwrap()
    }

    #[test]
    fn by_appid() {
        let x = xrefs_by_appid(&rows("wikidata/by_steam_appids.json"));
        assert_eq!(x.len(), 1, "rows with a non-numeric appid are dropped");
        let p = &x[&620];
        assert_eq!(p.qid, "Q279446");
        assert_eq!((p.pcgw_page.as_deref(), p.rawg_slug.as_deref()), (Some("Portal_2"), Some("portal-2")));
        assert_eq!((p.hltb_id, p.igdb_slug.as_deref(), p.epic_slug.as_deref()), (Some(7231), Some("portal-2"), None));
    }

    #[test]
    fn by_name_drops_shared_names() {
        let x = xrefs_by_name(&rows("wikidata/by_names.json"));
        assert_eq!(x.keys().collect::<Vec<_>>(), ["Portal 2"], "\"Prey\" names two items: dropped");
        assert_eq!(x["Portal 2"].hltb_id, Some(7231));
    }

    #[test]
    fn series() {
        let s = series_info(&rows("wikidata/series_for.json"));
        assert_eq!(s["Q279446"], (vec![("Portal".to_string(), "Q2130538".to_string())], Some(2011)), "one pair per series; earliest year");
        assert_eq!(s["Q42"], (vec![], Some(1999)));

        let m = series_entries(&rows("wikidata/series_members.json"));
        let list = &m["Q2130538"];
        assert_eq!(list.len(), 2, "duplicate rows for one game collapse");
        assert_eq!((list[0].name.as_str(), list[0].steam_appid, list[0].igdb_slug.as_deref()), ("Portal 2", Some(620), Some("portal-2")));
        assert_eq!((list[1].qid.as_str(), list[1].gog_id.as_deref(), list[1].year), ("Q17013", Some("1207658930"), Some(2007)));

        let l = series_qids_by_label(&rows("wikidata/series_by_labels.json"));
        assert_eq!(l.len(), 1, "\"Doom\" names two series: dropped");
        assert_eq!(l["Portal"], "Q2130538", "the same item under en and mul counts once");
    }
}

#[cfg(test)]
mod client_tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client(s: &MockServer) -> WikidataClient {
        WikidataClient::new(Client::new(), Some(&s.uri()))
    }

    /// Every caller shares `query`: an empty list of bindings is an answer
    /// ("nothing on Wikidata"), while an answer without bindings is an error,
    /// so the cross-references, series and GOG ids count it and ask again.
    #[tokio::test]
    async fn empty_bindings_are_none_and_missing_bindings_fail() {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/sparql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"head": {"vars": []}, "results": {"bindings": []}})))
            .mount(&s)
            .await;
        let wd = client(&s);
        assert!(wd.by_steam_appids(&[620]).await.unwrap().is_empty());
        assert!(wd.by_names(&["Portal 2".into()]).await.unwrap().is_empty());
        assert!(series_for(&wd, &["Q279446".into()]).await.unwrap().is_empty());
        assert_eq!(wd.gog_ids_by_steam_appids(&[620]).await.unwrap(), GogIds::default());

        let odd = MockServer::start().await;
        Mock::given(path("/sparql")).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({}))).mount(&odd).await;
        let wd = client(&odd);
        let e = wd.by_steam_appids(&[620]).await.unwrap_err();
        assert!(format!("{e:#}").contains("no results"), "{e:#}");
        assert!(wd.by_names(&["Portal 2".into()]).await.is_err());
        assert!(series_for(&wd, &["Q279446".into()]).await.is_err());
        assert!(series_members(&wd, &["Q2130538".into()]).await.is_err());
        assert!(series_by_labels(&wd, &["Portal".into()]).await.is_err());
    }

    /// A series entry's GOG id is the numeric product id (P12727), the one
    /// GOG's store image is looked up by; P2725 holds only the store page's
    /// slug, so asking for it left every entry without one.
    #[tokio::test]
    async fn series_members_ask_for_the_gog_product_id() {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/sparql"))
            .and(body_string_contains("wdt%3AP12727+%3Fgog"))
            .and(body_string_contains("wdt%3AP2725+%3Fgogpage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(crate::testutil::fixture_json("wikidata/series_members.json")))
            .expect(1)
            .mount(&s)
            .await;
        let m = series_members(&client(&s), &["Q2130538".into()]).await.unwrap();
        assert_eq!(m["Q2130538"][1].gog_id.as_deref(), Some("1207658930"));
        assert_eq!(m["Q2130538"][1].gog_slug.as_deref(), Some("portal"), "the store page's slug, for an entry without an id");
    }
}
