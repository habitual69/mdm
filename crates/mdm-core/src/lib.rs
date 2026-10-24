//! mdm-core: job model, SQLite queue, engine trait, event bus.

#![forbid(unsafe_code)]

mod config;
mod db;
mod engine;
mod error;
mod events;
mod model;
mod queue;

pub use config::{config_path, data_dir, ensure_dir, load_config, save_config, Config};
pub use engine::{bin_path, find_bin, find_js_runtime, Engine, EngineCtx, JobDone, JobError, ResolveInfo, RunnerMsg, RunnerTx};
pub use error::{Error, Result};
pub use events::{Event, EventRx, EventTx};
pub use model::{category_for, category_for_with_title, EngineKind, Job, JobKind, JobPatch, JobState};
pub use queue::{NewJobReq, Queue};

pub const APP_NAME: &str = "mdm";
