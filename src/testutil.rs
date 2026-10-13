//! Test helpers: fixture files under `tests/fixtures/` (see its README for
//! where each one came from) and a config that stays off this machine.

use crate::config::Config;
use std::path::{Path, PathBuf};

pub fn fixture_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(rel)
}

pub fn fixture(rel: &str) -> String {
    std::fs::read_to_string(fixture_path(rel)).unwrap_or_else(|e| panic!("fixture {rel}: {e}"))
}

pub fn fixture_json(rel: &str) -> serde_json::Value {
    serde_json::from_str(&fixture(rel)).unwrap_or_else(|e| panic!("fixture {rel}: {e}"))
}

/// Config for a test: the built-in defaults (no `.env`, no environment, no
/// keys), paths under `dir`, no auto sync, Linux compatibility on, and no
/// verdicts service. Tests add the keys they need.
///
/// `steam_dir` and `heroic_dir` point at folders that don't exist, and in
/// tests `find_steam_dir` and `find_config_dir` look nowhere else, so the
/// machine's own Steam and Heroic are never read. A test that syncs must
/// create its own.
pub fn offline_config(dir: &Path) -> Config {
    let mut cfg = Config::built_in();
    cfg.data_dir = dir.to_path_buf();
    cfg.config_dir = dir.to_path_buf();
    cfg.cache_dir = dir.join("cache");
    cfg.db_path = dir.join("upnext.db");
    cfg.gog_token_path = dir.join("gog_token.json");
    cfg.static_dir = Some(dir.join("no-ui"));
    cfg.steam_dir = Some(dir.join("no-steam"));
    cfg.heroic_dir = Some(dir.join("no-heroic"));
    cfg.epic_manifests = Some(dir.join("no-epic"));
    cfg.linux_mode = "on".into();
    cfg.auto_sync = false;
    // The project's verdicts service is the built-in address; a test that
    // needs one points it at `wiremock`.
    cfg.verdicts_url = None;
    cfg
}

/// A Steam client folder under `dir` whose `appinfo.vdf` is 20 bytes with a
/// string table that claims 0xFFFFFFFF strings;
/// `packageinfo.vdf` and the login are sound. Returns the folder.
pub fn damaged_steam_folder(dir: &Path) -> PathBuf {
    let steam = dir.join("damaged-steam");
    std::fs::create_dir_all(steam.join("appcache")).unwrap();
    std::fs::create_dir_all(steam.join("config")).unwrap();
    let mut appinfo = vec![];
    appinfo.extend(0x07_56_44_29u32.to_le_bytes());
    appinfo.extend(1u32.to_le_bytes());
    appinfo.extend(16u64.to_le_bytes());
    appinfo.extend(0xFFFF_FFFFu32.to_le_bytes());
    std::fs::write(steam.join("appcache/appinfo.vdf"), appinfo).unwrap();
    let mut packageinfo = vec![];
    packageinfo.extend(0x06_56_55_28u32.to_le_bytes());
    packageinfo.extend(1u32.to_le_bytes());
    packageinfo.extend(0xFFFF_FFFFu32.to_le_bytes());
    std::fs::write(steam.join("appcache/packageinfo.vdf"), packageinfo).unwrap();
    std::fs::write(steam.join("config/loginusers.vdf"), r#""users" { "76561198000000002" { "MostRecent" "1" } }"#).unwrap();
    steam
}

/// One scripted answer: what the model replies to a request.
pub type Answer = Box<dyn FnOnce(&crate::llm::LlmRequest) -> Result<serde_json::Value, crate::llm::LlmError> + Send>;

/// A model that answers from a script, in order, and keeps every request.
/// Each call reports 100 tokens in and 10 out. It panics when the script
/// runs out: a test that makes a call it didn't plan for fails.
pub struct ScriptedLlm {
    answers: std::sync::Mutex<std::collections::VecDeque<Answer>>,
    pub asked: std::sync::Mutex<Vec<crate::llm::LlmRequest>>,
}

impl ScriptedLlm {
    pub fn new(answers: Vec<Answer>) -> Self {
        Self { answers: std::sync::Mutex::new(answers.into()), asked: Default::default() }
    }
}

#[async_trait::async_trait]
impl crate::llm::LlmBackend for ScriptedLlm {
    fn name(&self) -> &str {
        "scripted"
    }

    async fn available(&self) -> Result<(), crate::llm::LlmError> {
        Ok(())
    }

    async fn complete_json(&self, req: crate::llm::LlmRequest) -> Result<crate::llm::LlmResponse, crate::llm::LlmError> {
        let answer = self.answers.lock().unwrap().pop_front().expect("the script has no answer left for this call");
        self.asked.lock().unwrap().push(req.clone());
        let json = answer(&req)?;
        Ok(crate::llm::LlmResponse {
            json,
            model: Some("scripted".into()),
            usage: Some(crate::llm::Usage { input_tokens: 100, output_tokens: 10, ..Default::default() }),
            cost_usd: None,
            api_usd: None,
            would_cost_usd: None,
            duration_ms: 5,
            flex: false,
        })
    }
}

/// No AI set up: `available` says so, as a backend with no key does, and
/// any call panics, since nothing should make one.
pub struct NoAi;

#[async_trait::async_trait]
impl crate::llm::LlmBackend for NoAi {
    fn name(&self) -> &str {
        "none"
    }

    async fn available(&self) -> Result<(), crate::llm::LlmError> {
        Err(crate::llm::LlmError::NotAvailable("no AI set up".into()))
    }

    async fn complete_json(&self, _: crate::llm::LlmRequest) -> Result<crate::llm::LlmResponse, crate::llm::LlmError> {
        panic!("an AI call with no AI set up")
    }
}

/// The id a prompt lists a game under ("#12 Celeste | …").
pub fn id_in(prompt: &str, name: &str) -> i64 {
    let line = regex::Regex::new(&format!(r"(?m)^#(\d+) {}( \[| \||$)", regex::escape(name))).unwrap();
    line.captures(prompt).unwrap_or_else(|| panic!("{name} isn't listed in the prompt")).get(1).unwrap().as_str().parse().unwrap()
}
