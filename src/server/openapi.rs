//! The API's contract: an OpenAPI 3.1 document made from the
//! handlers' `#[utoipa::path]`s, served at `/api/v1/openapi.json`. A copy is
//! committed as docs/openapi.json, and the tests keep it current.
//!
//! Who may call each operation comes from the guard itself
//! (`access::refused_for`), so the document and the guard can't disagree.

use super::access::{self, refused_for};
use super::{
    accounts, ai, approval, backups, community_verdicts, discovery, games, gog, gog_link, jobs, library, network, oidc, pairing, picks,
    players, ratings, routines, seed, settings, setup, setup_sync, steam, steam_shortcut, stores, taste, tokens, verdicts, wishlist,
};
use crate::db::Scope;
use axum::Json;
use axum::http::Method;
use std::sync::OnceLock;
use utoipa::openapi::extensions::ExtensionsBuilder;
use utoipa::openapi::path::{Operation, Parameter, ParameterBuilder, ParameterIn, PathItem};
use utoipa::openapi::schema::{KnownFormat, ObjectBuilder, SchemaFormat, Type};
use utoipa::openapi::security::{ApiKey, ApiKeyValue, HttpAuthScheme, HttpBuilder, SecurityRequirement, SecurityScheme};
use utoipa::openapi::{Content, OpenApi as Document, Ref, RefOr, Required, Response};
use utoipa::{Modify, OpenApi, ToSchema};

const DESCRIPTION: &str = "The HTTP API of UpNext, the game library that picks what to play next. \
This is the v1 contract: adding a field or an endpoint is fine within v1; removing or renaming one means v2. \
Paths are relative to `/api/v1`; `/api/...` answers the same until the first public release.\n\n\
This computer (a loopback address, addressed as localhost) needs no token. Another device needs a token \
(`Authorization: Bearer upnext_…`, scope `read` or `full`) or a paired browser's cookie (`upnext_device`, full). \
Each operation's `security` says which; an operation marked `x-local-only` answers only on the computer UpNext runs on.\n\n\
An operation with the `UpNext-Player` parameter answers for one player: a token bound to a player, else that header, \
else the browser's player cookie, else the default player.\n\n\
Errors are `{\"error\": \"…\"}` with the status.";

#[derive(OpenApi)]
#[openapi(
    info(title = "UpNext API", version = env!("CARGO_PKG_VERSION"), description = DESCRIPTION),
    servers((url = "/api/v1")),
    paths(
        super::health,
        document,
        library::profile,
        settings::config,
        ai::llm_test,
        ai::llm_models,
        ai::llm_codex,
        ai::llm_openai,
        library::collections,
        library::stats,
        games::games,
        games::review_tags,
        games::stop_reasons,
        games::game,
        games::set_override,
        games::set_ending,
        ratings::game_rating,
        games::next_in_series,
        games::game_series,
        games::later_add,
        games::later_remove,
        games::play_later,
        games::second_chances_get,
        games::second_chance_post,
        picks::versus_decide,
        picks::picks,
        picks::history,
        picks::pick_feedback,
        picks::pick_too,
        picks::pick_more,
        picks::pick_switch,
        ratings::pick_rating,
        jobs::jobs_get,
        jobs::jobs_post,
        jobs::scheduled_get,
        jobs::scheduled_put,
        jobs::jobs_cancel,
        jobs::job_get,
        jobs::job_log_text,
        jobs::job_retry,
        jobs::logs_get,
        ai::ai_usage_get,
        ai::ai_estimates_get,
        ai::ai_plan_get,
        ai::ai_prices_get,
        ai::ai_prices_refresh,
        gog::gog_get,
        gog::gog_logout,
        gog::gog_login,
        gog::gog_window,
        gog_link::browser_start,
        gog_link::callback,
        gog_link::bookmark,
        stores::epic_get,
        stores::epic_login,
        stores::epic_logout,
        stores::by_hand_get,
        stores::by_hand_put,
        library::titles_merge,
        library::titles_split,
        backups::export_db,
        backups::list,
        backups::create,
        backups::download,
        backups::restore,
        backups::import,
        backups::delete,
        taste::page,
        taste::create,
        taste::edit,
        taste::delete,
        taste::sources,
        taste::source_rows,
        taste::delete_source,
        taste::export,
        taste::questions,
        taste::diary_switch,
        taste::diary_launch,
        routines::list,
        routines::create,
        routines::replace,
        routines::delete,
        tokens::list,
        tokens::create,
        tokens::revoke,
        pairing::current,
        pairing::outcome,
        pairing::start,
        pairing::cancel,
        pairing::pair,
        oidc::status,
        oidc::login,
        oidc::callback,
        oidc::logout,
        approval::ask,
        approval::collect,
        approval::open,
        approval::waiting,
        approval::answer,
        network::computers,
        network::forget,
        network::connect,
        network::connect_status,
        network::connect_cancel,
        steam_shortcut::status,
        steam_shortcut::add,
        discovery::gems_get,
        discovery::ways_get,
        wishlist::wishlist_get,
        wishlist::wishlist_post,
        wishlist::wishlist_put,
        wishlist::wishlist_delete,
        setup::state_get,
        setup::setup_get,
        setup::setup_put,
        setup::reset,
        setup_sync::background,
        setup::galaxy_upload,
        seed::seed_get,
        seed::seed_post,
        seed::seed_upload,
        seed::seed_offer,
        seed::seed_delete,
        seed::game_reviews,
        verdicts::overview,
        verdicts::vocab,
        verdicts::consent_get,
        verdicts::consent_post,
        verdicts::form,
        verdicts::save,
        verdicts::send,
        verdicts::withdraw,
        verdicts::withdraw_all,
        verdicts::refresh,
        verdicts::report,
        community_verdicts::community_status,
        community_verdicts::game_verdicts,
        settings::settings_get,
        settings::settings_put,
        settings::enrich_sources,
        settings::sync_sources,
        steam::steam_test,
        steam::steam_login,
        steam::steam_callback,
        players::list,
        players::add,
        players::change,
        players::pin_put,
        players::remove,
        players::current_get,
        players::current_put,
        players::hidden_list,
        players::hidden_unlock,
        players::hidden_lock,
        players::hide,
        players::unhide,
        oidc::arm,
        oidc::disarm,
        oidc::confirm,
        oidc::unlink,
        accounts::accounts_get,
        accounts::account_put,
        accounts::account_use,
        accounts::connection_add,
        accounts::connection_put,
        accounts::connection_delete,
        accounts::ai_get,
        accounts::ai_add,
        accounts::ai_put,
        accounts::ai_delete,
        accounts::ai_use,
    ),
    components(schemas(Error)),
    modifiers(&Contract),
    tags(
        (name = "games", description = "One game: its page, its status, Play later, Second Chance"),
        (name = "picks", description = "Lineups of picks, their cards' actions, Help me decide"),
        (name = "library", description = "The library as a whole: profile, counts, series, merging titles"),
        (name = "jobs", description = "Sync, enrich and the other background jobs, and their schedules"),
        (name = "ai", description = "The AI backend, its models, usage, estimates and prices"),
        (name = "settings", description = "In-app settings"),
        (name = "players", description = "The people who play here, who's playing, and the games hidden from some of them"),
        (name = "accounts", description = "Store accounts and AI accounts, who they're shared with, and the Steam, GOG and Epic sign-ins"),
        (name = "backups", description = "Backups, restores, imports and the database export"),
        (name = "setup", description = "The first-run wizard: what it reads and where it got to"),
        (name = "seed", description = "UpNext's shared game data: summaries for popular games, applied without an AI"),
        (name = "verdicts", description = "Backlog verdicts: your own, sent to the verdicts service under CC0, and other players', from its daily snapshot"),
        (name = "taste", description = "The taste profile, routines, first-run questions and the play diary"),
        (name = "devices", description = "Tokens, paired browsers, and signing in with an identity provider"),
        (name = "system", description = "Health, logs and this document"),
    )
)]
pub struct ApiDoc;

/// Every error the API answers with.
#[derive(ToSchema)]
#[allow(dead_code)]
pub struct Error {
    /// What went wrong, in plain words.
    error: String,
}

/// The document, made once.
pub fn doc() -> &'static Document {
    static DOC: OnceLock<Document> = OnceLock::new();
    DOC.get_or_init(ApiDoc::openapi)
}

#[utoipa::path(get, path = "/openapi.json", tag = "system", operation_id = "openapi",
    description = "This document: the API's contract, as OpenAPI 3.1.",
    responses((status = 200, body = Object, description = "The OpenAPI document")))]
pub(super) async fn document() -> Json<&'static Document> {
    Json(doc())
}

/// Who may make a request, as the guard decides it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Anyone who can reach the server: the health check and pairing.
    Anyone,
    /// Only the computer UpNext runs on, whatever the token.
    LocalOnly,
    /// A full token or a paired browser.
    Full,
    /// Any token, or a paired browser.
    Read,
}

/// `path` as the document writes it (`/games/{id}`).
pub fn access(method: &Method, path: &str) -> Access {
    let path = format!("/api{path}");
    if access::open_to_anyone(method, &path) {
        Access::Anyone
    } else if refused_for(Scope::Full, method, &path).is_some() {
        Access::LocalOnly
    } else if refused_for(Scope::Read, method, &path).is_some() {
        Access::Full
    } else {
        Access::Read
    }
}

fn operations(item: &mut PathItem) -> impl Iterator<Item = (Method, &mut Operation)> {
    [
        (Method::GET, &mut item.get),
        (Method::PUT, &mut item.put),
        (Method::POST, &mut item.post),
        (Method::DELETE, &mut item.delete),
        (Method::PATCH, &mut item.patch),
        (Method::HEAD, &mut item.head),
        (Method::OPTIONS, &mut item.options),
        (Method::TRACE, &mut item.trace),
    ]
    .into_iter()
    .filter_map(|(m, op)| op.as_mut().map(|op| (m, op)))
}

/// The operations that answer the same whoever asks: the
/// household's backups, devices, the network, jobs, seed data and setup,
/// and the players themselves. Every other operation answers for one player, so it takes the
/// `UpNext-Player` parameter; a test checks this list against the handlers.
const SHARED: &[(&str, &str)] = &[
    ("GET", "/health"),
    ("GET", "/openapi.json"),
    ("GET", "/steam/callback"),
    ("GET", "/gog/bookmark"),
    ("GET", "/backups"),
    ("POST", "/backups"),
    ("POST", "/backups/import"),
    ("DELETE", "/backups/{name}"),
    ("GET", "/backups/{name}/download"),
    ("POST", "/backups/{name}/restore"),
    ("GET", "/export"),
    ("POST", "/pair"),
    ("GET", "/pairing"),
    ("DELETE", "/pairing"),
    ("GET", "/tokens"),
    ("POST", "/tokens"),
    ("DELETE", "/tokens/{id}"),
    ("GET", "/stop-reasons"),
    ("GET", "/enrich/sources"),
    ("GET", "/sync/sources"),
    ("POST", "/jobs"),
    ("GET", "/jobs/scheduled"),
    ("PUT", "/jobs/scheduled/{task}"),
    ("POST", "/jobs/{id}/retry-failed"),
    ("POST", "/players"),
    ("PUT", "/players/{id}"),
    ("DELETE", "/players/{id}"),
    ("PUT", "/players/current"),
    ("GET", "/seed"),
    ("POST", "/seed"),
    ("DELETE", "/seed"),
    ("POST", "/seed/offer"),
    ("POST", "/seed/upload"),
    ("POST", "/setup/background"),
    ("GET", "/steam/shortcut"),
    ("POST", "/steam/shortcut"),
    ("GET", "/verdicts/vocab"),
    ("GET", "/verdicts/community"),
    ("GET", "/pair/open"),
    ("POST", "/pair/request"),
    ("GET", "/pair/request/{id}"),
    ("GET", "/pairing/outcome"),
    ("GET", "/pairing/requests"),
    ("POST", "/pairing/requests/{id}"),
    ("GET", "/network/computers"),
    ("DELETE", "/network/computers/{id}"),
    ("GET", "/network/connect"),
    ("POST", "/network/connect"),
    ("DELETE", "/network/connect"),
    ("GET", "/oidc"),
    ("GET", "/oidc/login"),
    ("GET", "/oidc/callback"),
    ("POST", "/oidc/logout"),
    ("POST", "/players/{id}/oidc/link"),
    ("DELETE", "/players/{id}/oidc/link"),
    ("POST", "/players/{id}/oidc/confirm"),
    ("DELETE", "/players/{id}/oidc"),
];

/// The `UpNext-Player` parameter's name in the components.
const PLAYER_PARAMETER: &str = "UpNextPlayer";

fn player_parameter() -> Parameter {
    ParameterBuilder::new()
        .name("UpNext-Player")
        .parameter_in(ParameterIn::Header)
        .required(Required::False)
        .description(Some(
            "The player to answer for, by id, when no token bound to a player says it. An unknown id is a 400; a player \
             with a PIN is a 403 (a script acts for them with a token bound to them), as is another player than the \
             token's own.",
        ))
        .schema(Some(ObjectBuilder::new().schema_type(Type::Integer).format(Some(SchemaFormat::KnownFormat(KnownFormat::Int64)))))
        .build()
}

/// Security schemes, each operation's security from the guard, the guard's
/// own refusals, and the error body on every error.
struct Contract;

impl Modify for Contract {
    fn modify(&self, doc: &mut Document) {
        let components = doc.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "token",
            SecurityScheme::Http(
                HttpBuilder::new().scheme(HttpAuthScheme::Bearer).description(Some("Authorization: Bearer upnext_…")).build(),
            ),
        );
        components.add_security_scheme(
            "device",
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::with_description(
                pairing::OLD_COOKIE,
                "A paired browser (Settings › Devices and API, Add a device): a device token of full scope, in a cookie. \
                 The server sets it as `upnext_device_<port>` (the port it listens on, 8787 by default), so two UpNexts \
                 on one host that listen on different ports keep their own; a container listens on 8787 inside whatever \
                 host port it's published on, so it shares the cookie with another UpNext on 8787 on that host. \
                 `upnext_device`, as an app that keeps the token sets it itself, is taken too.",
            ))),
        );
        components.parameters.insert(PLAYER_PARAMETER.into(), RefOr::T(player_parameter()));
        let requirement = |name: &str, scope: &str| SecurityRequirement::new(name, [scope]);
        for (path, item) in doc.paths.paths.iter_mut() {
            for (method, op) in operations(item) {
                // The description says it; a summary would be the doc
                // comment, written for this code's readers.
                op.summary = None;
                if let Some(RefOr::T(body)) = &mut op.request_body {
                    binary(body.content.iter_mut());
                }
                for response in op.responses.responses.values_mut() {
                    if let RefOr::T(r) = response {
                        binary(r.content.iter_mut());
                    }
                }
                if !SHARED.contains(&(method.as_str(), path.as_str())) {
                    let player = RefOr::Ref(Ref::new(format!("#/components/parameters/{PLAYER_PARAMETER}")));
                    op.parameters.get_or_insert_with(Vec::new).push(player);
                }
                let access = access(&method, path);
                op.security = Some(match access {
                    Access::Anyone => vec![SecurityRequirement::default()],
                    Access::LocalOnly => vec![],
                    Access::Full => vec![requirement("token", "full"), requirement("device", "full")],
                    Access::Read => vec![requirement("token", "read"), requirement("device", "full")],
                });
                if access == Access::LocalOnly {
                    op.extensions.get_or_insert_with(Default::default).merge(ExtensionsBuilder::new().add("x-local-only", true).build());
                    let line = "Only on the computer UpNext runs on.";
                    op.description = Some(match op.description.take() {
                        Some(d) if d.contains(line) => d,
                        Some(d) => format!("{d}\n\n{line}"),
                        None => line.to_string(),
                    });
                }
                if matches!(access, Access::Full | Access::Read) {
                    let responses = &mut op.responses.responses;
                    // An operation's own reason for a status comes first, and
                    // the guard's refusals still follow it, since they apply
                    // too. Each is its own sentence: both are often lists
                    // joined with "or", which one sentence would run together.
                    let mut guard = |status: &str, description: &str| match responses.get_mut(status) {
                        Some(RefOr::T(r)) if !r.description.contains(description) => {
                            r.description = format!("{}. {description}", r.description.trim_end_matches('.'));
                        }
                        Some(_) => {}
                        None => {
                            responses.insert(status.to_string(), RefOr::T(Response::new(description)));
                        }
                    };
                    guard("401", "From another device: no token, or one that isn't valid here");
                    guard("403", "Refused: the token's scope doesn't allow it, a cross-site request, or a token over plain HTTP");
                    guard("429", "Too many requests with this token (about 10 a second); see Retry-After");
                }
                for (status, response) in op.responses.responses.iter_mut() {
                    if let RefOr::T(r) = response
                        && status.as_str() >= "400"
                        && r.content.is_empty()
                    {
                        r.content.insert("application/json".into(), RefOr::T(Content::new(Some(Ref::from_schema_name("Error")))));
                    }
                }
            }
        }
    }
}

/// A file's bytes (a database) as OpenAPI 3.1 has them: the media type,
/// with no schema.
fn binary<'a>(content: impl IntoIterator<Item = (&'a String, &'a mut RefOr<Content>)>) {
    for (media_type, c) in content {
        if let RefOr::T(c) = c
            && !(media_type.contains("json") || media_type.starts_with("text/"))
        {
            c.schema = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    const COMMITTED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/docs/openapi.json");

    fn generated() -> String {
        format!("{}\n", doc().to_pretty_json().expect("the document serializes"))
    }

    /// Every (method, path) in the document.
    fn documented() -> BTreeSet<(String, String)> {
        let mut doc = doc().clone();
        let mut out = BTreeSet::new();
        for (path, item) in doc.paths.paths.iter_mut() {
            for (method, _) in operations(item) {
                out.insert((method.to_string(), path.clone()));
            }
        }
        out
    }

    #[test]
    fn the_committed_document_is_current() {
        let now = generated();
        if std::env::var_os("UPNEXT_UPDATE_OPENAPI").is_some() {
            std::fs::write(COMMITTED, &now).expect("write docs/openapi.json");
            return;
        }
        let committed = std::fs::read_to_string(COMMITTED).unwrap_or_default();
        assert!(
            committed == now,
            "docs/openapi.json is not the API the code serves: run `UPNEXT_UPDATE_OPENAPI=1 cargo test openapi` and commit it"
        );
    }

    #[test]
    fn the_guards_refusals_follow_a_routes_own_as_a_sentence() {
        let doc: serde_json::Value = serde_json::from_str(&generated()).unwrap();
        let refused = &doc["paths"]["/ai-accounts/{id}"]["put"]["responses"]["403"]["description"];
        assert_eq!(
            refused,
            "Not the owner, or the library's AI from a bound token. \
             Refused: the token's scope doesn't allow it, a cross-site request, or a token over plain HTTP"
        );
    }

    #[test]
    fn every_path_is_in_api_md() {
        // The guide is docs/API.md, an index, and its pages in docs/api/.
        let docs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs");
        let mut pages = vec![docs.join("API.md")];
        for entry in std::fs::read_dir(docs.join("api")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "md") {
                pages.push(path);
            }
        }
        let api_md: String = pages.iter().map(|p| std::fs::read_to_string(p).unwrap()).collect::<Vec<_>>().join("\n");
        let boundary = |c: char| !(c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.' | '{' | '}'));
        let missing: Vec<String> = documented()
            .into_iter()
            .map(|(_, path)| format!("/api{path}"))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|p| !api_md.match_indices(p.as_str()).any(|(i, _)| api_md[i + p.len()..].chars().next().is_none_or(boundary)))
            .collect();
        assert!(missing.is_empty(), "docs/API.md and docs/api/ don't mention {missing:?}");
    }

    /// The routes as `router()` and the modules' `routes()` declare them.
    fn routed() -> BTreeSet<(String, String)> {
        // server.rs and every module beside it, so a new one's routes count too.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = vec![dir.join("server.rs")];
        for entry in std::fs::read_dir(dir.join("server")).unwrap() {
            let path = entry.unwrap().path();
            // Not this file (it mentions `.route(` in the scan) or the
            // tests' stand-in servers.
            let skip = path.ends_with("openapi.rs") || path.ends_with("api_tests.rs");
            if path.extension().is_some_and(|e| e == "rs") && !skip {
                files.push(path);
            }
        }
        let sources: Vec<String> = files.iter().map(|f| std::fs::read_to_string(f).unwrap()).collect();
        let method = regex::Regex::new(r"\b(get|post|put|delete|patch)\(").unwrap();
        let mut out = BTreeSet::new();
        for src in &sources {
            for (start, _) in src.match_indices(".route(\"") {
                let rest = &src[start + ".route(".len()..];
                // The call's arguments, up to its closing parenthesis.
                let mut depth = 1;
                let end = rest
                    .char_indices()
                    .find(|&(_, c)| {
                        depth += match c {
                            '(' => 1,
                            ')' => -1,
                            _ => 0,
                        };
                        depth == 0
                    })
                    .map(|(i, _)| i)
                    .expect("a closed .route(");
                let args = &rest[..end];
                let path = args[1..].split('"').next().unwrap().to_string();
                // The one route outside /api: a text file, not part of the API.
                if path == "/third-party-notices.txt" {
                    continue;
                }
                for m in method.captures_iter(args) {
                    out.insert((m[1].to_uppercase(), path.clone()));
                }
            }
        }
        out
    }

    #[test]
    fn every_route_is_documented() {
        let (routed, documented) = (routed(), documented());
        assert!(routed.len() > 70, "found only {} routes: has router() moved?", routed.len());
        let undocumented: Vec<_> = routed.difference(&documented).collect();
        let unrouted: Vec<_> = documented.difference(&routed).collect();
        assert!(undocumented.is_empty(), "routes without #[utoipa::path] in openapi.rs: {undocumented:?}");
        assert!(unrouted.is_empty(), "documented but not routed: {unrouted:?}");
    }

    /// The (method, path) of every handler that reads the player: it takes
    /// `Me` or the request's `Who`.
    fn per_player_handlers() -> BTreeSet<(String, String)> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = vec![dir.join("server.rs")];
        for entry in std::fs::read_dir(dir.join("server")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "rs") && !path.ends_with("openapi.rs") {
                files.push(path);
            }
        }
        let attr = regex::Regex::new(r#"(?s)#\[utoipa::path\((\w+),\s*path\s*=\s*"([^"]+)".*?\bfn \w+\s*\((.*?)\)\s*->"#).unwrap();
        let mut out = BTreeSet::new();
        for f in files {
            let src = std::fs::read_to_string(f).unwrap();
            for c in attr.captures_iter(&src) {
                if c[3].contains("Me(") || c[3].contains("Who>") {
                    out.insert((c[1].to_uppercase(), c[2].to_string()));
                }
            }
        }
        out
    }

    #[test]
    fn per_player_operations_take_the_player_header() {
        let json: serde_json::Value = serde_json::from_str(&generated()).unwrap();
        let mut with = BTreeSet::new();
        for (path, item) in json["paths"].as_object().unwrap() {
            for (method, op) in item.as_object().unwrap() {
                let refs = op["parameters"].as_array().into_iter().flatten();
                if refs.filter_map(|p| p["$ref"].as_str()).any(|r| r.ends_with(PLAYER_PARAMETER)) {
                    with.insert((method.to_uppercase(), path.clone()));
                }
            }
        }
        let handlers = per_player_handlers();
        assert!(handlers.len() > 80, "found only {} handlers that read the player: has the scan broken?", handlers.len());
        let missing: Vec<_> = handlers.difference(&with).collect();
        let extra: Vec<_> = with.difference(&handlers).collect();
        assert!(missing.is_empty(), "these read the player but SHARED lists them: {missing:?}");
        assert!(extra.is_empty(), "these answer the same for everyone, so SHARED should list them: {extra:?}");
        assert_eq!(json["components"]["parameters"][PLAYER_PARAMETER]["in"], "header");
    }

    #[test]
    fn operation_ids_are_unique() {
        let mut doc = doc().clone();
        let mut seen = BTreeSet::new();
        for item in doc.paths.paths.values_mut() {
            for (_, op) in operations(item) {
                let id = op.operation_id.clone().expect("every operation has an id");
                assert!(seen.insert(id.clone()), "operation id {id} is used twice");
            }
        }
    }

    #[test]
    fn path_parameters_are_in_the_path() {
        // A query struct's IntoParams without `parameter_in = Query` comes
        // out as required path parameters.
        let mut doc = doc().clone();
        for (path, item) in doc.paths.paths.iter_mut() {
            for (method, op) in operations(item) {
                for p in op.parameters.iter().flatten() {
                    let utoipa::openapi::RefOr::T(p) = p else { continue };
                    if matches!(p.parameter_in, utoipa::openapi::path::ParameterIn::Path) {
                        assert!(path.contains(&format!("{{{}}}", p.name)), "{method} {path}: {} isn't in the path", p.name);
                    }
                }
            }
        }
    }

    #[test]
    fn security_follows_the_guard() {
        assert_eq!(access(&Method::GET, "/health"), Access::Anyone);
        assert_eq!(access(&Method::POST, "/pair"), Access::Anyone);
        assert_eq!(access(&Method::GET, "/games"), Access::Read);
        assert_eq!(access(&Method::POST, "/picks"), Access::Full);
        assert_eq!(access(&Method::GET, "/tokens"), Access::Full);
        assert_eq!(access(&Method::GET, "/export"), Access::Full);
        assert_eq!(access(&Method::POST, "/gog/window"), Access::LocalOnly);
        assert_eq!(access(&Method::POST, "/tokens"), Access::LocalOnly);
        assert_eq!(access(&Method::POST, "/pairing"), Access::LocalOnly);
        assert_eq!(access(&Method::GET, "/backups"), Access::Read);
        assert_eq!(access(&Method::GET, "/backups/{name}/download"), Access::LocalOnly);
        assert_eq!(access(&Method::POST, "/backups/import"), Access::LocalOnly);
        assert_eq!(access(&Method::GET, "/steam/callback"), Access::Anyone);
        assert_eq!(access(&Method::GET, "/steam/login"), Access::Full);
        assert_eq!(access(&Method::POST, "/gog/browser"), Access::Full);
        assert_eq!(access(&Method::POST, "/gog/callback"), Access::Full);
        assert_eq!(access(&Method::GET, "/gog/bookmark"), Access::Full);
        let json: serde_json::Value = serde_json::from_str(&generated()).unwrap();
        let op = |path: &str, method: &str| json["paths"][path][method].clone();
        assert_eq!(op("/health", "get")["security"], serde_json::json!([{}]));
        assert_eq!(op("/gog/window", "post")["security"], serde_json::json!([]));
        assert_eq!(op("/gog/window", "post")["x-local-only"], serde_json::json!(true));
        assert_eq!(op("/games", "get")["security"], serde_json::json!([{"token": ["read"]}, {"device": ["full"]}]));
        assert_eq!(op("/settings", "put")["security"], serde_json::json!([{"token": ["full"]}, {"device": ["full"]}]));
        assert_eq!(json["openapi"], "3.1.0");
    }
}
