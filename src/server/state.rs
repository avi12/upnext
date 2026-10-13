//! What every handler is given: the server's state, with the settings as
//! the household and each player have them, and what serving other devices
//! keeps while the server runs.

use super::steam::SteamSignIn;
use super::{approval, limits, listen, network, oidc, pairing, players, setup_sync};
use crate::config::Config;
use crate::db::{Db, PlayerId};
use crate::enrich::Clients;
use crate::jobs::JobRunner;
use crate::llm::LlmBackend;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
    /// Config from `.env`; app settings from the database are overlaid per request via `cfg()`.
    pub base_cfg: Config,
    pub clients: Arc<Clients>,
    pub jobs: Arc<JobRunner>,
    /// Other devices: the listener, the pairing code, the limits.
    pub remote: Arc<Remote>,
    /// Claude Code being installed in a container, for setup.
    pub claude_install: Arc<crate::llm::claude_install::Progress>,
    /// What the setup wizard asked to run in the background.
    pub setup_sync: Arc<setup_sync::WizardJobs>,
}

/// What serving other devices keeps while the server runs.
#[derive(Default)]
pub struct Remote {
    pub listen: listen::Control,
    pub pairing: pairing::Codes,
    /// Devices asking to be allowed with a tap.
    pub approvals: approval::Requests,
    /// What this server announces on the network.
    pub announcer: crate::announce::Announcer,
    /// The app window's request to another computer's UpNext.
    pub connecting: network::Connecting,
    pub limits: limits::Limits,
    /// Wrong PINs and open Hidden by me lists.
    pub pins: players::Pins,
    /// Each other player's GOG and Epic clients, kept so a sign-in
    /// window or browser sign-in they start is still there when they look.
    pub sign_ins: players::SignIns,
    /// Steam sign-ins started and not yet back, by state token.
    pub steam_sign_ins: std::sync::Mutex<std::collections::HashMap<String, SteamSignIn>>,
    /// Identity provider sign-ins on their way, and an armed link.
    pub oidc: oidc::SignIns,
}

impl AppState {
    /// What the server listens on now.
    pub fn listening(&self) -> listen::Listening {
        self.remote.listen.now(&self.cfg())
    }

    /// The household's settings as the default player has them, for what
    /// isn't anyone's own (listening, other devices, the jobs' schedules).
    pub fn cfg(&self) -> Config {
        self.base_cfg.clone().with_db(&self.db).unwrap_or_else(|_| self.base_cfg.clone())
    }

    /// The settings as player `p` has them: the household's, with their own
    /// and their AI account's laid over.
    pub fn cfg_for(&self, p: PlayerId) -> Config {
        self.base_cfg.clone().with_db_for(&self.db, p).unwrap_or_else(|_| self.base_cfg.clone())
    }

    /// The settings the library's own jobs run with: the AI account the
    /// household chose for them.
    pub fn library_cfg(&self) -> Config {
        self.base_cfg.clone().for_library(&self.db).unwrap_or_else(|_| self.base_cfg.clone())
    }

    /// Player `p`'s GOG client: their own sign-in. Player 1's is
    /// the one the jobs and `Clients` have always used.
    pub fn gog_for(&self, p: PlayerId) -> Arc<crate::gog::GogClient> {
        if p == PlayerId::FIRST {
            return self.clients.gog.clone();
        }
        let mut all = self.remote.sign_ins.gog.lock().unwrap_or_else(|e| e.into_inner());
        all.entry(p)
            .or_insert_with(|| {
                let store = crate::signin::DbSignIn::own(self.base_cfg.db_path.clone(), "gog", p, None);
                Arc::new(self.clients.gog.with_store(Arc::new(store)))
            })
            .clone()
    }

    /// Player `p`'s Epic client, as [`AppState::gog_for`].
    pub fn epic_for(&self, p: PlayerId) -> Arc<crate::epic::EpicClient> {
        if p == PlayerId::FIRST {
            return self.clients.epic.clone();
        }
        let mut all = self.remote.sign_ins.epic.lock().unwrap_or_else(|e| e.into_inner());
        all.entry(p)
            .or_insert_with(|| {
                let store = crate::signin::DbSignIn::own(self.base_cfg.db_path.clone(), "epic", p, None);
                Arc::new(self.clients.epic.with_store(Arc::new(store)))
            })
            .clone()
    }

    /// The library AI account's backend, for the jobs.
    pub fn library_llm(&self) -> Arc<dyn LlmBackend> {
        Arc::from(crate::llm::backend_from_config(&self.library_cfg()))
    }

    /// Player `p`'s AI, for their picks. Built per call so a backend or key
    /// changed in Settings applies at once.
    pub fn llm_for(&self, p: PlayerId) -> Arc<dyn LlmBackend> {
        Arc::from(crate::llm::backend_from_config(&self.cfg_for(p)))
    }

    /// The backend that writes review summaries, second chances and hidden
    /// gems: the library AI account's, the picks' one unless Settings names
    /// another.
    pub fn summary_llm(&self) -> Arc<dyn LlmBackend> {
        Arc::from(crate::llm::summary_backend(&self.library_cfg()))
    }

    /// The providers whose list prices the AI player `p` uses is priced at:
    /// their picks' backend's and the library's summaries'.
    pub(super) fn priced_providers(&self, p: PlayerId) -> Vec<crate::prices::Provider> {
        let mut providers: Vec<_> =
            [self.llm_for(p).pricing(), self.summary_llm().pricing()].into_iter().flatten().map(|p| p.provider).collect();
        providers.dedup();
        providers
    }
}
