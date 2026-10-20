//! What the helper prints: one line per event. The program that starts it reads these lines
//! and ignores any it does not know. Words and numbers only: a reason never holds a line
//! break, so one line is always one event.

use std::fmt;

/// Longest reason kept.
const MAX_REASON: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fresh {
    /// This run made the change.
    New,
    /// It was so before this run.
    Already,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// `check` found Steam and the game.
    Ready,
    Progress {
        id: u64,
        done: u64,
        total: u64,
    },
    Ok {
        id: u64,
        fresh: Fresh,
    },
    Fail {
        id: u64,
        reason: String,
    },
    /// Nothing can be done: Steam is not running, or does not know the game.
    Error(String),
}

/// One line of plain text.
fn one_line(reason: &str) -> String {
    let clean: String = reason
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    clean.trim().chars().take(MAX_REASON).collect()
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ready => write!(f, "ok"),
            Self::Progress { id, done, total } => write!(f, "progress {id} {done} {total}"),
            Self::Ok { id, fresh } => {
                let word = match fresh {
                    Fresh::New => "new",
                    Fresh::Already => "already",
                };
                write!(f, "ok {id} {word}")
            }
            Self::Fail { id, reason } => write!(f, "fail {id} {}", one_line(reason)),
            Self::Error(reason) => write!(f, "error {}", one_line(reason)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_one_plain_line_each() {
        let lines = [
            (Event::Ready, "ok"),
            (
                Event::Progress {
                    id: 5,
                    done: 10,
                    total: 20,
                },
                "progress 5 10 20",
            ),
            (
                Event::Ok {
                    id: 5,
                    fresh: Fresh::New,
                },
                "ok 5 new",
            ),
            (
                Event::Ok {
                    id: 5,
                    fresh: Fresh::Already,
                },
                "ok 5 already",
            ),
            (
                Event::Fail {
                    id: 5,
                    reason: "no luck".into(),
                },
                "fail 5 no luck",
            ),
            (
                Event::Error("Steam is not running".into()),
                "error Steam is not running",
            ),
        ];
        for (event, line) in lines {
            assert_eq!(event.to_string(), line);
        }
    }

    #[test]
    fn a_reason_can_never_break_the_line_or_grow_without_limit() {
        let hostile = Event::Fail {
            id: 1,
            reason: "first\nok 2 new\r\nerror x\u{0}".into(),
        };
        let line = hostile.to_string();
        assert!(!line.contains(['\n', '\r', '\0']), "{line:?}");
        let long = Event::Error("x".repeat(5_000)).to_string();
        assert!(long.len() <= "error ".len() + MAX_REASON);
    }
}
