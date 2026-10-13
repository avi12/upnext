//! GOG's public store metadata and user reviews (no login needed).

use crate::http::{Backoff, RateLimiter, base_url, send_with_backoff};
use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct GogMeta {
    pub tags: Vec<String>,
    pub properties: Vec<String>,
    pub features: Vec<String>,
    pub description: Option<String>,
    /// The game's GOG store page.
    pub store_url: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct GogReviews {
    pub avg_rating: Option<f64>,
    pub rating_count: Option<i64>,
    /// (title, content, rating 1..5, votes, posted in Unix seconds)
    pub items: Vec<(String, String, f64, i64, Option<i64>)>,
}

const API: &str = "https://api.gog.com";
const REVIEWS: &str = "https://reviews.gog.com";
/// The store's catalogue search, what gog.com's own store pages call.
const CATALOG: &str = "https://catalog.gog.com";

pub struct GogMetaClient {
    http: Client,
    api: String,
    reviews: String,
    catalog: String,
    limiter: RateLimiter,
}

fn names(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.get("name").and_then(Value::as_str)).map(str::to_string).collect())
        .unwrap_or_default()
}

fn strip_html(s: &str) -> String {
    let re = regex::Regex::new(r"<[^>]+>").unwrap();
    let t = re.replace_all(s, " ");
    crate::steam::html_unescape(&t).split_whitespace().collect::<Vec<_>>().join(" ")
}

impl GogMetaClient {
    /// `base` stands in for GOG's store and review APIs (tests pass a local
    /// mock server); None means the real ones.
    pub fn new(http: Client, base: Option<&str>) -> Self {
        Self {
            http,
            api: base_url(API, base),
            reviews: base_url(REVIEWS, base),
            catalog: base_url(CATALOG, base),
            limiter: RateLimiter::new("GOG", Duration::from_millis(350)),
        }
    }

    /// `Ok(None)` when GOG does not know the product (or it is not a game page).
    pub async fn game(&self, gog_id: &str) -> Result<Option<GogMeta>> {
        let resp = send_with_backoff(&self.limiter, &Backoff { attempts: 3, ..Default::default() }, || {
            self.http.get(format!("{}/v2/games/{gog_id}", self.api)).query(&[("locale", "en-US")])
        })
        .await;
        let resp = match resp {
            Ok(r) => r,
            Err(e) if crate::http::is_not_found(&e) => return Ok(None),
            Err(e) => return Err(e),
        };
        let v: Value = resp.json().await.context("GOG v2 game returned non-JSON")?;
        Ok(Some(parse_game(&v)))
    }

    /// The store image for a product (series covers), or None on any failure.
    /// Not rate limited: the one caller paces itself.
    pub async fn product_image(&self, id: &str) -> Option<String> {
        let url = format!("{}/products/{id}", self.api);
        let v: Value = self.http.get(&url).send().await.ok()?.error_for_status().ok()?.json().await.ok()?;
        let src = v.pointer("/images/logo2x").or_else(|| v.pointer("/images/background"))?.as_str()?.to_string();
        Some(if src.starts_with("//") { format!("https:{src}") } else { src })
    }

    /// The product id of the store page `game/<slug>`, from GOG's store
    /// catalogue: one search for the slug's words, then the product whose
    /// slug is exactly this one, never the nearest title. `Ok(None)` when the
    /// catalogue lists no such product (taken off sale, renamed, or sold only
    /// in an edition).
    pub async fn id_by_slug(&self, slug: &str) -> Result<Option<String>> {
        let words = slug.replace('_', " ");
        let resp = send_with_backoff(&self.limiter, &Backoff { attempts: 4, ..Default::default() }, || {
            self.http.get(format!("{}/v1/catalog", self.catalog)).query(&[
                ("limit", "48"),
                ("query", &format!("like:{words}")),
                ("order", "desc:score"),
                ("countryCode", "US"),
                ("locale", "en-US"),
                ("currencyCode", "USD"),
            ])
        })
        .await?;
        let v: Value = resp.json().await.context("GOG's catalogue returned non-JSON")?;
        let products = v.get("products").and_then(Value::as_array).context("GOG's catalogue answer has no products")?;
        Ok(id_with_slug(products, slug))
    }

    pub async fn reviews(&self, gog_id: &str, limit: usize) -> Result<Option<GogReviews>> {
        let resp = send_with_backoff(&self.limiter, &Backoff { attempts: 3, ..Default::default() }, || {
            self.http
                .get(format!("{}/v1/products/{gog_id}/reviews", self.reviews))
                .query(&[("limit", limit.to_string()), ("order", "desc:votes".into())])
        })
        .await;
        let resp = match resp {
            Ok(r) => r,
            Err(e) if crate::http::is_not_found(&e) => return Ok(None),
            Err(e) => return Err(e),
        };
        let v: Value = resp.json().await.context("GOG reviews returned non-JSON")?;
        Ok(Some(parse_reviews(&v)))
    }
}

/// The slug of a GOG store page as Wikidata keeps it ("game/portal_2", now
/// and then with a language in front), or None for anything that isn't a
/// game's page.
pub fn store_slug(page: &str) -> Option<&str> {
    let page = page.trim().trim_matches('/');
    let page = page.strip_prefix("en/").unwrap_or(page);
    let slug = page.strip_prefix("game/")?;
    (!slug.is_empty() && slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')).then_some(slug)
}

/// The numeric id of the product with exactly this slug among a catalogue
/// search's products (GOG sends ids as strings or numbers).
fn id_with_slug(products: &[Value], slug: &str) -> Option<String> {
    let p = products.iter().find(|p| p.get("slug").and_then(Value::as_str) == Some(slug))?;
    let id = match p.get("id")? {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_digit())).then_some(id)
}

fn parse_game(v: &Value) -> GogMeta {
    let e = v.get("_embedded").cloned().unwrap_or(Value::Null);
    let description = v
        .get("overview")
        .or_else(|| v.get("description"))
        .and_then(Value::as_str)
        .map(strip_html)
        .map(|s| s.chars().take(600).collect::<String>())
        .filter(|s| !s.is_empty());
    GogMeta {
        tags: names(e.get("tags")),
        properties: names(e.get("properties")),
        features: names(e.get("features")),
        description,
        store_url: v.pointer("/_links/store/href").and_then(Value::as_str).map(str::to_string),
    }
}

fn parse_reviews(v: &Value) -> GogReviews {
    let mut out = GogReviews {
        avg_rating: v.get("overallAvgRating").and_then(Value::as_f64),
        rating_count: v.get("ratingCount").and_then(Value::as_i64),
        items: vec![],
    };
    // The API returns `_embedded.items` with `content: {title, description}`,
    // `rating: {value}` and `votes: {upvotes, downvotes}`; the flat forms are
    // kept as fallbacks.
    let list = v.pointer("/_embedded/items").or_else(|| v.pointer("/_embedded/reviews"));
    for r in list.and_then(Value::as_array).into_iter().flatten() {
        let text = r.pointer("/content/description").or_else(|| r.get("content")).and_then(Value::as_str);
        let content = text.map(strip_html).unwrap_or_default();
        if content.is_empty() {
            continue;
        }
        let title = r
            .pointer("/content/title")
            .or_else(|| r.get("title"))
            .or_else(|| r.get("contentTitle"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let rating = r.pointer("/rating/value").or_else(|| r.get("rating")).and_then(Value::as_f64).unwrap_or(0.0);
        let votes =
            r.pointer("/votes/upvotes").or_else(|| r.pointer("/votes/up")).or_else(|| r.get("votes")).and_then(Value::as_i64).unwrap_or(0);
        let posted = r
            .get("date")
            .or_else(|| r.get("creationDate"))
            .and_then(Value::as_str)
            .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
            .map(|d| d.timestamp());
        // Whole, like Steam's; GOG's reviews are shorter than Steam's limit anyway.
        out.items.push((title, content.chars().take(8000).collect(), rating, votes, posted));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::fixture_json;

    #[test]
    fn game() {
        let m = parse_game(&fixture_json("gog/v2_game.json"));
        assert_eq!(m.tags, ["Role-playing", "Action", "Fantasy"]);
        assert_eq!(m.store_url.as_deref(), Some("https://www.gog.com/en/game/the_witcher"));
        let d = m.description.unwrap();
        assert!(!d.contains('<'), "HTML stripped: {d}");
        assert!(d.contains("RPG") && d.contains("Choices matter."));
    }

    #[test]
    fn store_slugs() {
        assert_eq!(store_slug("game/portal_2"), Some("portal_2"));
        assert_eq!(store_slug("en/game/the_witcher_2/"), Some("the_witcher_2"));
        for page in ["movie/a_film", "game/", "game/Has Spaces", "portal_2", ""] {
            assert_eq!(store_slug(page), None, "{page:?}");
        }
    }

    /// Only the product with exactly the slug asked counts, whichever way
    /// GOG writes its id; a search that lists only near titles finds none.
    #[test]
    fn id_by_exact_slug() {
        let products = fixture_json("gog/catalog.json")["products"].as_array().unwrap().clone();
        assert_eq!(id_with_slug(&products, "the_witcher_2").as_deref(), Some("1207658930"), "not its soundtrack, listed first");
        assert_eq!(id_with_slug(&products, "the_witcher").as_deref(), Some("1207658924"), "a numeric id");
        assert_eq!(id_with_slug(&products, "the_witcher_3_wild_hunt"), None);
    }

    #[test]
    fn reviews_summary() {
        let r = parse_reviews(&fixture_json("gog/reviews.json"));
        assert_eq!((r.avg_rating, r.rating_count), (Some(4.6), Some(14076)));
    }

    #[test]
    fn review_texts() {
        // Live shape: `_embedded.items`, `content: {title, description}`,
        // `rating: {value}`, `votes: {upvotes, downvotes}`.
        let r = parse_reviews(&fixture_json("gog/reviews.json"));
        assert_eq!(r.items.len(), 2, "empty reviews are skipped");
        let (title, content, rating, votes, posted) = &r.items[0];
        assert_eq!(title, "Worth the slow start");
        assert_eq!(*posted, Some(1496324525), "the date, as Unix seconds");
        assert_eq!(r.items[1].4, None);
        assert_eq!(r.items[1].0, "");
        assert!(content.starts_with("Long, dark and worth it") && !content.contains('<'), "{content}");
        assert_eq!((*rating, *votes), (5.0, 634));
        assert_eq!((r.items[1].2, r.items[1].3), (3.0, 12));
    }
}

#[cfg(test)]
mod client_tests {
    use super::*;
    use crate::testutil::fixture_json;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// One search per slug, for the slug's words; a 429 is waited out and
    /// asked again, and an answer without a product list is an error, not
    /// "not on GOG".
    #[tokio::test]
    async fn a_slug_is_looked_up_in_the_catalogue() {
        let s = MockServer::start().await;
        Mock::given(path("/v1/catalog")).respond_with(ResponseTemplate::new(429)).up_to_n_times(1).with_priority(1).mount(&s).await;
        Mock::given(method("GET"))
            .and(path("/v1/catalog"))
            .and(query_param("query", "like:the witcher 2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(fixture_json("gog/catalog.json")))
            .expect(1)
            .mount(&s)
            .await;
        let gog = GogMetaClient::new(Client::new(), Some(&s.uri()));
        assert_eq!(gog.id_by_slug("the_witcher_2").await.unwrap().as_deref(), Some("1207658930"));

        let odd = MockServer::start().await;
        Mock::given(path("/v1/catalog")).respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({}))).mount(&odd).await;
        let gog = GogMetaClient::new(Client::new(), Some(&odd.uri()));
        assert!(gog.id_by_slug("the_witcher_2").await.is_err());
    }
}
