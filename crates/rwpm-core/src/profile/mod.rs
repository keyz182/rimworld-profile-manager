//! Profiles: stable ids, `profile.toml`, the store, and the running-game probe.

pub mod copy_settings;
pub mod file;
pub mod guard;
pub mod health;
pub mod id;
pub mod last_run;
pub(crate) mod lock;
pub mod mod_list;
pub mod offline;
pub mod relocate;
pub mod running;
pub mod store;

pub use file::{FrozenMod, ModSource, Mode, ProfileFile};
pub use id::ProfileId;
pub use last_run::{LastRun, RunOutcome, StartedBy};
pub use running::{GAME_PROCESS_NAMES, ProcessProbe, RunningProbe, any_game_running};
pub use store::{Listing, Location, OsTrash, Profile, ProfileStore, Trash, savedatafolder_arg};
