//! UpNext as a library. The `upnext` binary (`src/main.rs`) parses arguments
//! and calls the handlers in `cli`. Everything else stays private to the crate
//! so unused code is still reported; export more as new entry points need it.

mod ai_plan;
mod ai_usage;
mod announce;
mod autostart;
mod backups;
mod build_id;
pub mod cli;
mod computers;
mod config;
mod db;
mod decide;
#[cfg(feature = "desktop")]
mod desktop;
mod digest;
mod discovery;
mod display;
mod ea;
mod enrich;
mod epic;
mod flatpak;
mod galaxy;
mod gog;
#[cfg(feature = "desktop")]
mod gog_window;
mod gogmeta;
mod heroic;
mod hltb;
mod http;
mod igdb;
mod installs;
mod jobs;
mod lifecycle;
mod lineup;
mod llm;
mod manual;
mod mock;
mod names;
mod oidc;
mod other_users;
#[cfg(all(feature = "desktop", target_os = "linux"))]
mod pad_layouts;
mod pcgw;
mod prices;
mod process;
mod prompt;
mod protondb;
mod ranking;
mod rawg;
mod recommend;
mod review_tags;
mod second_chance;
mod seed;
mod series;
mod server;
mod signin;
mod steam;
mod steam_copy;
mod steam_local;
mod steam_shortcut;
mod steamspy;
mod taste;
#[cfg(test)]
mod testutil;
mod ubisoft;
mod verdicts;
mod wikidata;
mod wishlist;

pub use config::Config;
pub use db::{GameFilter, Status};
pub use enrich::{EnrichOptions, SyncOptions};
pub use flatpak::restart_without_unreadable_cert_env;
#[cfg(feature = "desktop")]
pub use gog_window::run as run_gog_window;
pub use jobs::log_layer;
