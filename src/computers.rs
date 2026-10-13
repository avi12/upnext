//! The other computers this one uses UpNext on: the
//! app window's "Use the library on another computer" keeps each one it was
//! allowed on, with its token, in `computers.json` in the config folder,
//! readable by this user alone. The database isn't the place: the token is
//! another computer's, and a backup or an export of this library has no
//! business carrying it.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const FILE: &str = "computers.json";

/// A computer the window can open on.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Computer {
    /// Its install id, from its announcement and its answer to the request.
    pub id: String,
    pub name: String,
    /// What to open: `http://192.168.0.42:8787`, or its `public_url`.
    pub address: String,
    /// The device token it gave this computer.
    pub token: String,
    pub added_at: i64,
}

// By hand, so a log or a panic never prints the token.
impl std::fmt::Debug for Computer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Computer")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("address", &self.address)
            .field("token", &"…")
            .field("added_at", &self.added_at)
            .finish()
    }
}

/// What answers at a saved computer's address, as `reach` finds it.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(not(feature = "desktop"), allow(dead_code))]
pub enum Reach {
    /// The same UpNext, by its install id.
    Same,
    /// Another UpNext, with this id (empty when it gave none): the address
    /// went to another computer. Its token isn't sent there.
    Other(String),
    /// Nothing, or not UpNext, with why.
    Away(String),
}

/// Ask the computer's address, with no token, whether it's still the same
/// UpNext. Trust is on first use: the id is what it gave when it allowed
/// this one, and a computer that answers with it is taken as that one.
#[cfg_attr(not(feature = "desktop"), allow(dead_code))]
pub async fn reach(c: &Computer) -> Reach {
    let health = async {
        let http = reqwest::Client::builder().no_proxy().timeout(std::time::Duration::from_secs(3)).build()?;
        http.get(format!("{}/api/health", c.address)).send().await?.error_for_status()?.json::<serde_json::Value>().await
    };
    match health.await {
        Err(e) => Reach::Away(e.to_string()),
        Ok(v) if v["app"] != "upnext" => Reach::Away("something else answers there".into()),
        Ok(v) if v["id"].as_str() == Some(c.id.as_str()) => Reach::Same,
        Ok(v) => Reach::Other(v["id"].as_str().unwrap_or_default().to_string()),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Computers {
    /// The one the window opens on, by id; None for this computer's own.
    #[serde(default)]
    pub open: Option<String>,
    #[serde(default)]
    pub computers: Vec<Computer>,
}

impl Computers {
    pub fn path(config_dir: &Path) -> PathBuf {
        config_dir.join(FILE)
    }

    /// The saved list; empty when there's none yet. A file that doesn't
    /// parse is an error, so it isn't written over by accident.
    pub fn load(config_dir: &Path) -> Result<Self> {
        let path = Self::path(config_dir);
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).with_context(|| format!("reading {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    /// Write it, readable and writable by this user alone (on Windows the
    /// config folder is in the user's own profile already).
    pub fn save(&self, config_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(config_dir).with_context(|| format!("creating {}", config_dir.display()))?;
        let path = Self::path(config_dir);
        let fresh = path.with_extension("json.new");
        let text = serde_json::to_string_pretty(self)?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        {
            use std::io::Write;
            let mut f = options.open(&fresh).with_context(|| format!("writing {}", fresh.display()))?;
            f.write_all(text.as_bytes())?;
            f.sync_all()?;
        }
        std::fs::rename(&fresh, &path).with_context(|| format!("replacing {}", path.display()))
    }

    #[cfg_attr(not(feature = "desktop"), allow(dead_code))]
    pub fn get(&self, id: &str) -> Option<&Computer> {
        self.computers.iter().find(|c| c.id == id)
    }

    /// The one the window opens on, if it's still saved.
    #[cfg_attr(not(feature = "desktop"), allow(dead_code))]
    pub fn opened(&self) -> Option<&Computer> {
        self.open.as_deref().and_then(|id| self.get(id))
    }

    /// Keep `c`, in place of what was kept for the same computer.
    pub fn add(&mut self, c: Computer) {
        self.computers.retain(|k| k.id != c.id);
        self.computers.push(c);
    }

    /// Forget one. Whether it was there.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.computers.len();
        self.computers.retain(|c| c.id != id);
        if self.open.as_deref() == Some(id) {
            self.open = None;
        }
        self.computers.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn den() -> Computer {
        Computer {
            id: "4f2a9c01d3e85b67".into(),
            name: "den".into(),
            address: "http://192.168.0.42:8787".into(),
            token: "upnext_abc".into(),
            added_at: 1,
        }
    }

    #[test]
    fn saved_and_read_back_privately() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Computers::load(dir.path()).unwrap(), Computers::default());
        let mut c = Computers::default();
        c.add(den());
        c.add(Computer { name: "den again".into(), ..den() });
        c.open = Some(den().id);
        c.save(dir.path()).unwrap();
        let back = Computers::load(dir.path()).unwrap();
        assert_eq!(back.computers.len(), 1, "one entry per computer");
        assert_eq!(back.opened().map(|c| c.name.as_str()), Some("den again"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(Computers::path(dir.path())).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let mut back = back;
        assert!(back.remove(&den().id));
        assert_eq!(back.open, None, "forgetting the one it opens on opens this computer's");
        std::fs::write(Computers::path(dir.path()), "{not json").unwrap();
        assert!(Computers::load(dir.path()).is_err());
        assert!(!format!("{:?}", den()).contains("upnext_abc"), "no token in a log");
    }

    #[tokio::test]
    async fn the_same_computer_or_another_at_its_address() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let at = MockServer::start().await;
        let c = Computer { address: at.uri(), ..den() };
        assert!(matches!(reach(&c).await, Reach::Away(_)), "nothing answers yet");
        let health = |id: &str| ResponseTemplate::new(200).set_body_json(serde_json::json!({ "app": "upnext", "id": id }));
        Mock::given(method("GET")).and(path("/api/health")).respond_with(health(&den().id)).up_to_n_times(1).mount(&at).await;
        assert_eq!(reach(&c).await, Reach::Same);
        Mock::given(method("GET")).and(path("/api/health")).respond_with(health("0123456789abcdef")).mount(&at).await;
        assert_eq!(reach(&c).await, Reach::Other("0123456789abcdef".into()));
        let requests = at.received_requests().await.unwrap();
        assert!(requests.iter().all(|r| !r.headers.contains_key("authorization")), "the token isn't sent to check");
    }
}
