//! The command line: `rwpm-steam <check|download|subscribe|unsubscribe> [id…]`. Every id is a
//! number, so nothing else can reach Steam or a log line (the ids come from a file that
//! someone else made).

use std::collections::BTreeSet;

/// Most ids one run takes. A bigger pack is sent in several runs.
pub const MAX_IDS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Download,
    Subscribe,
    Unsubscribe,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Is Steam there and does it know the game?
    Check,
    Run {
        op: Op,
        ids: Vec<u64>,
    },
}

pub const USAGE: &str = "usage: rwpm-steam <check|download|subscribe|unsubscribe> [id...]";

/// `args` are the words after the program name. Repeated ids count once, in first-seen order.
pub fn parse(args: &[String]) -> Result<Command, String> {
    let Some((word, rest)) = args.split_first() else {
        return Err(USAGE.into());
    };
    let op = match word.as_str() {
        "check" if rest.is_empty() => return Ok(Command::Check),
        "check" => return Err("check takes no ids".into()),
        "download" => Op::Download,
        "subscribe" => Op::Subscribe,
        "unsubscribe" => Op::Unsubscribe,
        _ => return Err(USAGE.into()),
    };
    if rest.is_empty() {
        return Err("give at least one id".into());
    }
    let (mut seen, mut ids) = (BTreeSet::new(), Vec::new());
    for word in rest {
        let id = parse_id(word)?;
        if seen.insert(id) {
            ids.push(id);
        }
    }
    if ids.len() > MAX_IDS {
        return Err(format!("at most {MAX_IDS} ids at a time"));
    }
    Ok(Command::Run { op, ids })
}

/// Plain digits, no sign, no spaces, not zero.
fn parse_id(word: &str) -> Result<u64, String> {
    let digits = !word.is_empty() && word.bytes().all(|b| b.is_ascii_digit());
    match word.parse::<u64>() {
        Ok(id) if digits && id > 0 => Ok(id),
        _ => Err("an id is a number above 0".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| (*a).to_owned()).collect()
    }

    #[test]
    fn the_commands_parse() {
        assert_eq!(parse(&words(&["check"])), Ok(Command::Check));
        assert_eq!(
            parse(&words(&["download", "3", "1", "3", "2"])),
            Ok(Command::Run {
                op: Op::Download,
                ids: vec![3, 1, 2]
            })
        );
        for (word, op) in [
            ("subscribe", Op::Subscribe),
            ("unsubscribe", Op::Unsubscribe),
        ] {
            assert_eq!(
                parse(&words(&[word, "7"])),
                Ok(Command::Run { op, ids: vec![7] })
            );
        }
    }

    #[test]
    fn anything_else_is_refused() {
        let refused = [
            words(&[]),
            words(&["check", "5"]),
            words(&["download"]),
            words(&["fly", "5"]),
            words(&["download", "0"]),
            words(&["download", "-5"]),
            words(&["download", "+5"]),
            words(&["download", "5 +quit"]),
            words(&["download", "1/../.."]),
            words(&["download", "abc"]),
            words(&["download", ""]),
            words(&["download", "99999999999999999999999"]),
            words(&["download", "1.5"]),
        ];
        for args in refused {
            assert!(parse(&args).is_err(), "{args:?} must be refused");
        }
    }

    #[test]
    fn the_number_of_ids_is_limited() {
        let many: Vec<String> = (1..=MAX_IDS as u64 + 1).map(|i| i.to_string()).collect();
        let mut args = words(&["download"]);
        args.extend(many.iter().take(MAX_IDS).cloned());
        assert!(parse(&args).is_ok());
        args.push(many[MAX_IDS].clone());
        assert!(parse(&args).is_err());
    }
}
