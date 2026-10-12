//! Errors surfaced to front ends. Messages are plain words; front ends may add help text.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("`{0}` is not a valid profile id (use a-z, 0-9 and -)")]
    InvalidId(String),
    #[error("a profile name cannot be empty")]
    EmptyName,
    #[error("no profile with id `{0}`")]
    NotFound(String),
    #[error("the Default profile cannot be {0}")]
    DefaultImmutable(&'static str),
    #[error("profile `{0}` is in use by a running RimWorld; close the game first")]
    Running(String),
    #[error("RimWorld is running; close the game first")]
    GameRunning,
    #[error("choose which copy to use for: {}", .0.join(", "))]
    Unresolved(Vec<String>),
    #[error("two mods use the folder name `{0}`; remove one of them first")]
    FolderClash(String),
    #[error("Steam is updating `{0}`; try again in a minute")]
    SourceChanging(String),
    #[error("cannot move the profiles there: {0}")]
    BadMoveTarget(String),
    #[error("the copy of `{}` does not match the original; nothing was moved", .0.display())]
    VerifyFailed(PathBuf),
    #[error("`{0}` contains `=`, which RimWorld cannot read as a save folder")]
    PathHasEquals(PathBuf),
    #[error("the profiles folder must be a full path: `{0}`")]
    RootNotAbsolute(PathBuf),
    #[error("`{0}` is not a valid UTF-8 path")]
    NonUtf8(PathBuf),
    #[error("no free profile id left for `{0}`")]
    NoFreeId(String),
    #[error(
        "`{path}` was written by a newer version (schema {found}; this version reads {supported})"
    )]
    SchemaTooNew {
        path: PathBuf,
        found: u32,
        supported: u32,
    },
    #[error("cannot read `{path}`: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("cannot {op} `{path}`: {source}")]
    Io {
        op: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
}

impl ProfileError {
    pub(crate) fn io(
        op: &'static str,
        path: impl Into<PathBuf>,
    ) -> impl FnOnce(std::io::Error) -> Self {
        let path = path.into();
        move |source| Self::Io { op, path, source }
    }
}
