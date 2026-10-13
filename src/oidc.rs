//! Signing in through an OpenID Connect provider, the
//! relying party's side: discovery from the issuer's address, the
//! authorization code flow with PKCE, and checking the ID token the
//! provider returns. The server's part, the routes and what a sign-in
//! makes, is `server::oidc`.
//!
//! The ID token's signature is checked against the provider's published
//! keys (RS256 and the other RSA, ECDSA and Ed25519 algorithms), with the
//! crypto library rustls already brings, rather than a crate of its own:
//! `openidconnect` would bring the `rsa` crate and its open advisory. An
//! unsigned token (`alg: none`) and one signed with the client secret
//! (`HS256`) are refused: the keys are the provider's word.
//!
//! The issuer is the household's setting, so its address is where every
//! request goes; tests point it at a `wiremock` server with a test key.

use crate::http::text_capped;
use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// The most a provider's answer may be: discovery, keys and tokens are a
/// few kilobytes.
const MAX_ANSWER: usize = 1 << 20;
/// How far the provider's clock may be from ours, in seconds.
const SKEW: i64 = 120;

/// What discovery says about a provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provider {
    /// The issuer as the provider names itself: an ID token's `iss` must be
    /// exactly this.
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub jwks_uri: String,
    /// How UpNext proves itself at the token endpoint, when it has a secret.
    pub auth: ClientAuth,
}

/// How the client secret goes to the token endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientAuth {
    /// `client_secret_basic`, the default.
    Basic,
    /// `client_secret_post`, for a provider that takes only that.
    Post,
}

#[derive(Deserialize)]
struct Discovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
    #[serde(default)]
    token_endpoint_auth_methods_supported: Option<Vec<String>>,
}

/// An address the provider gave: https, or plain http on this computer or
/// its network like the issuer itself.
fn endpoint_ok(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|u| u.scheme() == "https" || (u.scheme() == "http" && crate::config::is_local_server(url)))
}

/// A provider's answer as JSON, read with a cap.
async fn json_from(r: reqwest::Response, what: &str) -> Result<Value> {
    let status = r.status();
    let text = text_capped(r, MAX_ANSWER).await.with_context(|| format!("reading the provider's {what}"))?;
    if !status.is_success() {
        let v: Value = serde_json::from_str(&text).unwrap_or_default();
        let why = [v.get("error_description"), v.get("error")].into_iter().flatten().find_map(Value::as_str).unwrap_or("no reason given");
        bail!("the provider answered {what} with {status}: {}", why.chars().take(200).collect::<String>());
    }
    serde_json::from_str(&text).with_context(|| format!("the provider's {what} isn't JSON"))
}

/// Find a provider from its issuer address
/// (`<issuer>/.well-known/openid-configuration`). The issuer it names must
/// be the one asked for, give or take a trailing `/`.
pub async fn discover(client: &reqwest::Client, issuer: &str) -> Result<Provider> {
    let url = format!("{}/.well-known/openid-configuration", issuer.trim_end_matches('/'));
    let r = client.get(&url).send().await.with_context(|| format!("asking {url}"))?;
    let d: Discovery = serde_json::from_value(json_from(r, "discovery").await?).context("the provider's discovery document")?;
    if d.issuer.trim_end_matches('/') != issuer.trim_end_matches('/') {
        bail!("the provider at {issuer} calls itself {}: set the issuer to exactly that", d.issuer);
    }
    for (what, u) in [("authorization", &d.authorization_endpoint), ("token", &d.token_endpoint), ("keys", &d.jwks_uri)] {
        if !endpoint_ok(u) {
            bail!("the provider's {what} address isn't https: {u}");
        }
    }
    let methods = d.token_endpoint_auth_methods_supported.unwrap_or_default();
    let auth = if !methods.is_empty()
        && !methods.iter().any(|m| m == "client_secret_basic")
        && methods.iter().any(|m| m == "client_secret_post")
    {
        ClientAuth::Post
    } else {
        ClientAuth::Basic
    };
    Ok(Provider {
        issuer: d.issuer,
        authorization_endpoint: d.authorization_endpoint,
        token_endpoint: d.token_endpoint,
        jwks_uri: d.jwks_uri,
        auth,
    })
}

/// 32 random bytes, base64url: a state, a nonce or a PKCE verifier.
pub fn random() -> Result<String> {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).map_err(|e| anyhow!("no randomness for a sign-in: {e}"))?;
    Ok(URL_SAFE_NO_PAD.encode(b))
}

/// PKCE's S256 challenge for a verifier.
pub fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Form encoding of one value (RFC 6749 appendix B), for the query, the
/// form and the Basic credentials alike.
pub fn form_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// What a sign-in sends the browser to the provider with.
pub struct Request<'a> {
    pub client_id: &'a str,
    pub redirect_uri: &'a str,
    pub state: &'a str,
    pub nonce: &'a str,
    pub verifier: &'a str,
    /// Ask for the `groups` scope too, for the allowed groups rule.
    pub groups: bool,
}

/// The provider's sign-in page for this request.
pub fn authorize_url(p: &Provider, r: &Request) -> String {
    let scope = if r.groups { "openid profile email groups" } else { "openid profile email" };
    let sep = if p.authorization_endpoint.contains('?') { '&' } else { '?' };
    format!(
        "{}{sep}response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&nonce={}&code_challenge={}&code_challenge_method=S256",
        p.authorization_endpoint,
        form_encode(r.client_id),
        form_encode(r.redirect_uri),
        form_encode(scope),
        form_encode(r.state),
        form_encode(r.nonce),
        challenge(r.verifier)
    )
}

/// Trade the code the provider sent back for its ID token (still unchecked).
pub async fn exchange(
    client: &reqwest::Client,
    p: &Provider,
    client_id: &str,
    secret: Option<&str>,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> Result<String> {
    let mut form = format!(
        "grant_type=authorization_code&code={}&redirect_uri={}&code_verifier={}",
        form_encode(code),
        form_encode(redirect_uri),
        form_encode(verifier)
    );
    let mut req = client.post(&p.token_endpoint).header(reqwest::header::ACCEPT, "application/json");
    match (secret, p.auth) {
        (Some(s), ClientAuth::Basic) => {
            let pair = format!("{}:{}", form_encode(client_id), form_encode(s));
            req = req.header(reqwest::header::AUTHORIZATION, format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(pair)));
        }
        (Some(s), ClientAuth::Post) => form.push_str(&format!("&client_id={}&client_secret={}", form_encode(client_id), form_encode(s))),
        (None, _) => form.push_str(&format!("&client_id={}", form_encode(client_id))),
    }
    let r = req
        .header(reqwest::header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(form)
        .send()
        .await
        .context("asking the provider for the sign-in's tokens")?;
    let v = json_from(r, "the code").await?;
    v.get("id_token").and_then(Value::as_str).map(str::to_string).ok_or_else(|| anyhow!("the provider sent no ID token"))
}

/// What UpNext takes from a checked ID token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// The provider's id for the account, which never changes.
    pub subject: String,
    /// `name`, else `preferred_username`: what a new player is called.
    pub name: Option<String>,
    /// `preferred_username` and `email`, shown on this computer when a
    /// sign-in waits to be confirmed as a player; never matched against
    /// anything, since the account may choose them.
    pub username: Option<String>,
    pub email: Option<String>,
    /// The `groups` claim, for the allowed groups rule.
    pub groups: Vec<String>,
}

/// The provider's published keys.
pub async fn keys(client: &reqwest::Client, p: &Provider) -> Result<Vec<Value>> {
    let r = client.get(&p.jwks_uri).send().await.context("asking the provider for its keys")?;
    let v = json_from(r, "keys").await?;
    Ok(v.get("keys").and_then(Value::as_array).cloned().unwrap_or_default())
}

fn b64(part: &str, what: &str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(part.trim_end_matches('=')).map_err(|_| anyhow!("the ID token's {what} isn't base64url"))
}

/// Check an ID token: signed by one of `keys` with an algorithm UpNext
/// takes, from the provider, for this client, not expired and not before
/// its time (`iat`, `nbf`), and with the nonce this sign-in was given.
pub fn verify(token: &str, keys: &[Value], p: &Provider, client_id: &str, nonce: &str, now: i64) -> Result<Identity> {
    let parts: Vec<&str> = token.split('.').collect();
    let [head, body, sig] = parts[..] else { bail!("the ID token isn't a signed JWT") };
    let header: Value = serde_json::from_slice(&b64(head, "header")?).context("the ID token's header")?;
    if header.get("crit").is_some() {
        bail!("the ID token has critical header fields UpNext doesn't know");
    }
    let alg = header.get("alg").and_then(Value::as_str).unwrap_or_default();
    let kid = header.get("kid").and_then(Value::as_str);
    let signed = format!("{head}.{body}");
    let sig = b64(sig, "signature")?;
    let fits = |k: &&Value| {
        let s = |f: &str| k.get(f).and_then(Value::as_str);
        kid.is_none_or(|kid| s("kid") == Some(kid)) && s("use").is_none_or(|u| u == "sig") && s("alg").is_none_or(|a| a == alg)
    };
    let candidates: Vec<&Value> = keys.iter().filter(fits).collect();
    if candidates.is_empty() {
        bail!("none of the provider's keys signed this ID token (alg {alg}, kid {})", kid.unwrap_or("none"));
    }
    if !candidates.iter().any(|k| check_signature(alg, k, signed.as_bytes(), &sig).unwrap_or(false)) {
        check_signature(alg, candidates[0], signed.as_bytes(), &sig)?;
        bail!("the ID token's signature doesn't match the provider's key");
    }

    let claims: Value = serde_json::from_slice(&b64(body, "claims")?).context("the ID token's claims")?;
    let text = |f: &str| claims.get(f).and_then(Value::as_str);
    if text("iss") != Some(p.issuer.as_str()) {
        bail!("the ID token is from {}, not the provider", text("iss").unwrap_or("nobody"));
    }
    let audiences: Vec<&str> = match claims.get("aud") {
        Some(Value::String(a)) => vec![a.as_str()],
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
        _ => vec![],
    };
    if !audiences.contains(&client_id) {
        bail!("the ID token is for another client");
    }
    if let Some(azp) = text("azp")
        && azp != client_id
    {
        bail!("the ID token was given to another client");
    }
    let number = |f: &str| claims.get(f).and_then(Value::as_i64);
    match number("exp") {
        Some(exp) if exp + SKEW > now => {}
        Some(_) => bail!("the ID token has expired: check this computer's clock"),
        None => bail!("the ID token has no expiry"),
    }
    if number("iat").is_some_and(|iat| iat > now + SKEW) {
        bail!("the ID token was issued in the future: check this computer's clock");
    }
    if number("nbf").is_some_and(|nbf| nbf > now + SKEW) {
        bail!("the ID token isn't valid yet: check this computer's clock");
    }
    let given = text("nonce").unwrap_or_default();
    if given.len() != nonce.len() || !given.bytes().zip(nonce.bytes()).all(|(a, b)| a == b) {
        bail!("the ID token isn't for this sign-in");
    }
    let subject = text("sub").filter(|s| !s.is_empty() && s.len() <= 255).ok_or_else(|| anyhow!("the ID token names no account"))?;
    let name = [text("name"), text("preferred_username")].into_iter().flatten().map(str::trim).find(|n| !n.is_empty()).map(str::to_string);
    let groups = match claims.get("groups") {
        Some(Value::Array(g)) => g.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        Some(Value::String(g)) => vec![g.clone()],
        _ => vec![],
    };
    let shown = |f: &str| text(f).map(str::trim).filter(|v| !v.is_empty()).map(|v| v.chars().take(120).collect::<String>());
    Ok(Identity { subject: subject.to_string(), name, username: shown("preferred_username"), email: shown("email"), groups })
}

/// Whether `key` (a JWK) signed `msg` with `alg`. An error for a key or an
/// algorithm UpNext can't check.
fn check_signature(alg: &str, key: &Value, msg: &[u8], sig: &[u8]) -> Result<bool> {
    use aws_lc_rs::signature as s;
    let field = |f: &str| -> Result<Vec<u8>> {
        let v = key.get(f).and_then(Value::as_str).ok_or_else(|| anyhow!("the provider's key has no {f}"))?;
        b64(v, "key")
    };
    let kty = key.get("kty").and_then(Value::as_str).unwrap_or_default();
    let crv = key.get("crv").and_then(Value::as_str).unwrap_or_default();
    let rsa = |params: &'static s::RsaParameters| -> Result<bool> {
        if kty != "RSA" {
            bail!("an {alg} token needs an RSA key");
        }
        let (n, e) = (field("n")?, field("e")?);
        Ok(s::RsaPublicKeyComponents { n: &n[..], e: &e[..] }.verify(params, msg, sig).is_ok())
    };
    let ec = |want: &str, alg_: &'static s::EcdsaVerificationAlgorithm| -> Result<bool> {
        if kty != "EC" || crv != want {
            bail!("an {alg} token needs an EC {want} key");
        }
        let point = [vec![4u8], field("x")?, field("y")?].concat();
        Ok(s::UnparsedPublicKey::new(alg_, point).verify(msg, sig).is_ok())
    };
    match alg {
        "RS256" => rsa(&s::RSA_PKCS1_2048_8192_SHA256),
        "RS384" => rsa(&s::RSA_PKCS1_2048_8192_SHA384),
        "RS512" => rsa(&s::RSA_PKCS1_2048_8192_SHA512),
        "PS256" => rsa(&s::RSA_PSS_2048_8192_SHA256),
        "PS384" => rsa(&s::RSA_PSS_2048_8192_SHA384),
        "PS512" => rsa(&s::RSA_PSS_2048_8192_SHA512),
        "ES256" => ec("P-256", &s::ECDSA_P256_SHA256_FIXED),
        "ES384" => ec("P-384", &s::ECDSA_P384_SHA384_FIXED),
        "EdDSA" => {
            if kty != "OKP" || crv != "Ed25519" {
                bail!("an EdDSA token needs an Ed25519 key");
            }
            Ok(s::UnparsedPublicKey::new(&s::ED25519, field("x")?).verify(msg, sig).is_ok())
        }
        other => bail!("the ID token is signed with {other:?}, which UpNext doesn't take (RS256, PS256, ES256, EdDSA and their kin)"),
    }
}

/// A test provider: a signing key and the ID tokens it makes, for
/// `wiremock` (here and in the server's tests).
#[cfg(test)]
pub mod test_provider {
    use super::*;
    use aws_lc_rs::rand::SystemRandom;
    use aws_lc_rs::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    pub const CLIENT_ID: &str = "upnext";
    pub const SECRET: &str = "test-secret";

    /// An ES256 key, with the JWK the provider publishes for it.
    pub struct Key {
        pair: EcdsaKeyPair,
        pub kid: String,
    }

    impl Key {
        pub fn new(kid: &str) -> Self {
            Self { pair: EcdsaKeyPair::generate(&ECDSA_P256_SHA256_FIXED_SIGNING).unwrap(), kid: kid.into() }
        }

        pub fn jwk(&self) -> Value {
            let point = self.pair.public_key().as_ref();
            json!({"kty": "EC", "crv": "P-256", "use": "sig", "alg": "ES256", "kid": self.kid,
                "x": URL_SAFE_NO_PAD.encode(&point[1..33]), "y": URL_SAFE_NO_PAD.encode(&point[33..65])})
        }

        /// A token with these claims, signed with this key.
        pub fn sign(&self, claims: &Value) -> String {
            self.sign_as("ES256", claims)
        }

        pub fn sign_as(&self, alg: &str, claims: &Value) -> String {
            let head = URL_SAFE_NO_PAD.encode(json!({"alg": alg, "kid": self.kid, "typ": "JWT"}).to_string());
            let body = URL_SAFE_NO_PAD.encode(claims.to_string());
            let msg = format!("{head}.{body}");
            let sig = self.pair.sign(&SystemRandom::new(), msg.as_bytes()).unwrap();
            format!("{msg}.{}", URL_SAFE_NO_PAD.encode(sig.as_ref()))
        }
    }

    /// The claims of a good ID token for `sub`, from `issuer`, with `nonce`.
    pub fn claims(issuer: &str, sub: &str, nonce: &str, now: i64) -> Value {
        json!({"iss": issuer, "sub": sub, "aud": CLIENT_ID, "exp": now + 300, "iat": now, "nonce": nonce})
    }

    /// Discovery and the keys, served by `server` as issuer `issuer`.
    pub async fn serve(server: &MockServer, issuer: &str, key: &Key) {
        let base = server.uri();
        Mock::given(method("GET"))
            .and(path("/.well-known/openid-configuration"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "issuer": issuer,
                "authorization_endpoint": format!("{base}/authorize"),
                "token_endpoint": format!("{base}/token"),
                "jwks_uri": format!("{base}/jwks"),
                "token_endpoint_auth_methods_supported": ["client_secret_basic"],
            })))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"keys": [key.jwk()]})))
            .mount(server)
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::test_provider::*;
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn provider(issuer: &str) -> Provider {
        Provider {
            issuer: issuer.into(),
            authorization_endpoint: format!("{issuer}/authorize"),
            token_endpoint: format!("{issuer}/token"),
            jwks_uri: format!("{issuer}/jwks"),
            auth: ClientAuth::Basic,
        }
    }

    #[tokio::test]
    async fn discovery_finds_the_provider_and_holds_it_to_its_name() {
        let server = MockServer::start().await;
        let issuer = server.uri();
        let key = Key::new("k1");
        serve(&server, &issuer, &key).await;
        let client = crate::http::client().unwrap();
        let p = discover(&client, &format!("{issuer}/")).await.unwrap();
        assert_eq!(p, provider(&issuer), "a trailing / is the same issuer");
        assert!(discover(&client, &format!("{issuer}/realms/other")).await.is_err(), "nothing there");

        let other = MockServer::start().await;
        serve(&other, "https://sso.den.example.org", &key).await;
        let e = discover(&client, &other.uri()).await.unwrap_err().to_string();
        assert!(e.contains("calls itself https://sso.den.example.org"), "{e}");
    }

    #[test]
    fn the_sign_in_page_carries_pkce_state_and_nonce() {
        let p = provider("https://sso.den.example.org");
        let r = Request {
            client_id: "up next",
            redirect_uri: "https://upnext.lan/api/v1/oidc/callback",
            state: "s",
            nonce: "n",
            verifier: "v",
            groups: true,
        };
        let url = authorize_url(&p, &r);
        assert!(url.starts_with("https://sso.den.example.org/authorize?response_type=code&client_id=up%20next&"), "{url}");
        assert!(url.contains("redirect_uri=https%3A%2F%2Fupnext.lan%2Fapi%2Fv1%2Foidc%2Fcallback"), "{url}");
        assert!(url.contains("scope=openid%20profile%20email%20groups&state=s&nonce=n"), "{url}");
        assert!(url.ends_with(&format!("code_challenge={}&code_challenge_method=S256", challenge("v"))), "{url}");
        // RFC 7636's example.
        assert_eq!(challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[tokio::test]
    async fn the_code_is_traded_with_the_secret_and_the_verifier() {
        let server = MockServer::start().await;
        let p = provider(&server.uri());
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(header("authorization", "Basic dXBuZXh0OnRlc3Qtc2VjcmV0"))
            .and(body_string_contains(
                "grant_type=authorization_code&code=c%2F1&redirect_uri=https%3A%2F%2Fupnext.lan%2Fcb&code_verifier=v",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id_token": "a.b.c", "token_type": "Bearer"})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error": "invalid_grant"})))
            .mount(&server)
            .await;
        let client = crate::http::client().unwrap();
        let got = exchange(&client, &p, CLIENT_ID, Some(SECRET), "c/1", "https://upnext.lan/cb", "v").await.unwrap();
        assert_eq!(got, "a.b.c");
        let e = exchange(&client, &p, CLIENT_ID, Some(SECRET), "used", "https://upnext.lan/cb", "v").await.unwrap_err().to_string();
        assert!(e.contains("invalid_grant"), "{e}");
    }

    #[test]
    fn an_id_token_is_checked_in_full() {
        let iss = "https://sso.den.example.org";
        let p = provider(iss);
        let key = Key::new("k1");
        let keys = vec![key.jwk()];
        let now = 1_800_000_000;
        let good = claims(iss, "u-1", "n1", now);
        let mut named = good.clone();
        named["preferred_username"] = json!("sam");
        named["groups"] = json!(["games", "family"]);
        let id = verify(&key.sign(&named), &keys, &p, CLIENT_ID, "n1", now).unwrap();
        let want = Identity {
            subject: "u-1".into(),
            name: Some("sam".into()),
            username: Some("sam".into()),
            email: None,
            groups: vec!["games".into(), "family".into()],
        };
        assert_eq!(id, want);

        let refused = |token: String, why: &str| {
            let e = verify(&token, &keys, &p, CLIENT_ID, "n1", now).unwrap_err().to_string();
            assert!(e.contains(why), "{why}: {e}");
        };
        let with = |f: &str, v: Value| {
            let mut c = good.clone();
            c[f] = v;
            key.sign(&c)
        };
        refused(with("iss", json!("https://evil.example")), "not the provider");
        refused(with("aud", json!(["someone-else"])), "another client");
        refused(with("azp", json!("someone-else")), "another client");
        refused(with("exp", json!(now - 600)), "expired");
        refused(with("iat", json!(now + 600)), "issued in the future");
        refused(with("nbf", json!(now + 600)), "isn't valid yet");
        refused(with("nonce", json!("n2")), "isn't for this sign-in");
        refused(with("sub", json!("")), "names no account");
        // Another key, an unsigned token, one signed with the secret, and a changed claim.
        refused(Key::new("k1").sign(&good), "doesn't match");
        refused(Key::new("k9").sign(&good), "none of the provider's keys");
        let unsigned = key.sign_as("none", &good);
        refused(unsigned, "none of the provider's keys");
        let mut hs = key.jwk();
        hs["alg"] = json!("HS256");
        let e = verify(&key.sign_as("HS256", &good), &[hs], &p, CLIENT_ID, "n1", now).unwrap_err().to_string();
        assert!(e.contains("doesn't take"), "{e}");
        let token = key.sign(&good);
        let (rest, sig) = token.rsplit_once('.').unwrap();
        let (head, _) = rest.split_once('.').unwrap();
        let forged = format!("{head}.{}.{sig}", URL_SAFE_NO_PAD.encode(with_sub(&good, "u-2").to_string()));
        refused(forged, "doesn't match");
    }

    fn with_sub(c: &Value, sub: &str) -> Value {
        let mut c = c.clone();
        c["sub"] = json!(sub);
        c
    }

    /// The modulus and exponent of a DER `RSAPublicKey` (RFC 8017), as a
    /// JWK holds them: big-endian, without a leading zero.
    fn rsa_public_key(der: &[u8]) -> (&[u8], &[u8]) {
        // A tag, then a length: short, or 0x8N and N bytes of it.
        fn item(b: &[u8]) -> (&[u8], &[u8]) {
            let (len, at) = match b[1] {
                l if l < 0x80 => (l as usize, 2),
                l => {
                    let n = (l & 0x7f) as usize;
                    (b[2..2 + n].iter().fold(0usize, |acc, x| acc << 8 | *x as usize), 2 + n)
                }
            };
            (&b[at..at + len], &b[at + len..])
        }
        let (seq, _) = item(der);
        let (n, rest) = item(seq);
        let (e, _) = item(rest);
        (n.strip_prefix(&[0]).unwrap_or(n), e)
    }

    #[test]
    fn rsa_keys_are_checked_too() {
        use aws_lc_rs::rand::SystemRandom;
        use aws_lc_rs::rsa::KeySize;
        use aws_lc_rs::signature::{KeyPair, RSA_PKCS1_SHA256, RsaKeyPair};
        let pair = RsaKeyPair::generate(KeySize::Rsa2048).unwrap();
        let (n, e) = rsa_public_key(pair.public_key().as_ref());
        let jwk = json!({"kty": "RSA", "kid": "r1", "n": URL_SAFE_NO_PAD.encode(n), "e": URL_SAFE_NO_PAD.encode(e)});
        let iss = "https://sso.den.example.org";
        let now = 1_800_000_000;
        let head = URL_SAFE_NO_PAD.encode(json!({"alg": "RS256", "kid": "r1"}).to_string());
        let body = URL_SAFE_NO_PAD.encode(claims(iss, "u-1", "n1", now).to_string());
        let msg = format!("{head}.{body}");
        let mut sig = vec![0; pair.public_modulus_len()];
        pair.sign(&RSA_PKCS1_SHA256, &SystemRandom::new(), msg.as_bytes(), &mut sig).unwrap();
        let token = format!("{msg}.{}", URL_SAFE_NO_PAD.encode(&sig));
        let id = verify(&token, std::slice::from_ref(&jwk), &provider(iss), CLIENT_ID, "n1", now).unwrap();
        assert_eq!(id.subject, "u-1");
        sig[10] ^= 1;
        let bad = format!("{msg}.{}", URL_SAFE_NO_PAD.encode(&sig));
        assert!(verify(&bad, &[jwk], &provider(iss), CLIENT_ID, "n1", now).is_err());
    }
}
