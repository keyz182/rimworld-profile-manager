//! Reading what steamcmd prints (R52). Its output is coloured with ANSI codes, and there is
//! no line break between one item's result and the next, so results are found by their
//! wording in a growing buffer, not by lines. The exit code is not used: it was 0, 7 and 0 on
//! three systems that all downloaded the same mods.

const SUCCESS: &str = "Success. Downloaded item ";
const FAILURE: &str = "ERROR! Download item ";
/// Enough of the end of the buffer to hold a result that arrives in two pieces.
const KEEP: usize = 512;
/// The most that is remembered to explain a silence.
const TAIL: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Done(u64),
    Failed { id: u64, reason: String },
}

#[derive(Debug, Default)]
pub struct Reader {
    buffer: String,
    tail: String,
}

impl Reader {
    /// Adds what steamcmd just printed. Returns the results that are now complete.
    pub fn feed(&mut self, text: &str) -> Vec<Event> {
        self.buffer.push_str(text);
        self.buffer = strip_ansi(&self.buffer);
        let mut events = Vec::new();
        while let Some((event, end)) = next_event(&self.buffer) {
            events.push(event);
            self.buffer.drain(..end);
        }
        self.remember();
        events
    }

    /// The last of the printed text, for explaining why nothing was delivered.
    pub fn tail(&self) -> &str {
        &self.tail
    }

    fn remember(&mut self) {
        if self.buffer.len() > KEEP {
            let cut = self.buffer.len() - KEEP;
            let cut = (cut..=self.buffer.len())
                .find(|i| self.buffer.is_char_boundary(*i))
                .unwrap_or(self.buffer.len());
            self.tail.push_str(&self.buffer[..cut]);
            self.buffer.drain(..cut);
        }
        if self.tail.len() > TAIL {
            let cut = self.tail.len() - TAIL;
            let cut = (cut..=self.tail.len())
                .find(|i| self.tail.is_char_boundary(*i))
                .unwrap_or(self.tail.len());
            self.tail.drain(..cut);
        }
    }
}

/// Everything printed so far that is not a result, plus the buffer.
pub fn explain(reader: &Reader) -> String {
    let all = format!("{}{}", reader.tail, reader.buffer);
    explain_text(&all)
}

/// What a silence means, in plain words, judged from what steamcmd printed (or the system said).
pub fn explain_text(text: &str) -> String {
    // Shells write the same system text in different cases (bash: "Bad CPU type in
    // executable", zsh: "bad CPU type in executable"), so case is ignored.
    let lower = text.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| lower.contains(w));
    if has(&["bad cpu type"]) {
        "steamcmd could not start. It needs Rosetta 2 on this Mac: run `softwareupdate --install-rosetta` and try again."
    } else if has(&[
        "shared libraries",
        "no such file or directory",
        "required file not found",
    ]) {
        "steamcmd could not start. On Linux it needs the 32-bit system libraries (for example `lib32-glibc` on Arch, `libc6-i386` on Debian and Ubuntu, `glibc.i686` on Fedora). Install them and try again."
    } else if has(&["login failure", "failed login", "failed (no connection)"]) {
        "steamcmd could not sign in to Steam. Check the internet connection and try again."
    } else {
        "steamcmd gave no answer for this mod."
    }
    .to_owned()
}

/// Removes ANSI escape sequences. A sequence cut off at the end stays, to be finished by the
/// next piece.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            // Cut off after the escape itself.
            None => {
                out.push_str(&text[at..]);
                break;
            }
            Some((_, '[')) => {
                chars.next();
                let finished = chars.by_ref().any(|(_, f)| ('@'..='~').contains(&f));
                if !finished {
                    out.push_str(&text[at..]);
                    break;
                }
            }
            // A lone escape that is not a colour code: dropped.
            Some(_) => {}
        }
    }
    out
}

/// The first complete result in `text` and where it ends.
fn next_event(text: &str) -> Option<(Event, usize)> {
    let ok = text.find(SUCCESS).map(|at| (at, true));
    let bad = text.find(FAILURE).map(|at| (at, false));
    let (at, success) = match (ok, bad) {
        (Some(a), Some(b)) => {
            if a.0 <= b.0 {
                a
            } else {
                b
            }
        }
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => return None,
    };
    let start = at
        + if success {
            SUCCESS.len()
        } else {
            FAILURE.len()
        };
    let rest = &text[start..];
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    // A number that ends the buffer may not be finished.
    if digits == 0 || digits == rest.len() {
        return None;
    }
    let id: u64 = rest[..digits].parse().ok()?;
    let after = &rest[digits..];
    if success {
        return Some((Event::Done(id), start + digits));
    }
    let open = after.strip_prefix(" failed (")?;
    let close = open.find(')')?;
    let reason = open[..close].trim().to_owned();
    let end = start + digits + " failed (".len() + close + 1;
    Some((Event::Failed { id, reason }, end))
}

/// What a failure reason means to a player.
pub fn plain_reason(reason: &str) -> String {
    match reason {
        "File Not Found" | "No match" => {
            "Steam has no such mod. It may have been removed from the Workshop.".to_owned()
        }
        other => format!("steamcmd could not download it ({other})."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK: &str = "Success. Downloaded item 3552152340 to \"/x/steamapps/workshop/content/294100/3552152340\" (722271 bytes)";

    #[test]
    fn results_run_together_without_line_breaks() {
        let mut reader = Reader::default();
        let text = format!("{OK}ERROR! Download item 1 failed (File Not Found).{OK}");
        let events = reader.feed(&text);
        assert_eq!(
            events,
            vec![
                Event::Done(3_552_152_340),
                Event::Failed {
                    id: 1,
                    reason: "File Not Found".into()
                },
                Event::Done(3_552_152_340),
            ]
        );
    }

    #[test]
    fn colour_codes_inside_a_result_do_not_hide_it() {
        let mut reader = Reader::default();
        let text = "\u{1b}[1m\u{1b}[32mSuccess.\u{1b}[0m Downloaded item 42 to \"/x\" (1 bytes)";
        assert_eq!(reader.feed(text), vec![Event::Done(42)]);
    }

    #[test]
    fn a_result_in_pieces_is_found_when_the_last_piece_arrives() {
        let mut reader = Reader::default();
        assert!(reader.feed("Success. Downloaded it").is_empty());
        assert!(
            reader.feed("em 12").is_empty(),
            "the number may not be finished"
        );
        assert_eq!(reader.feed("34 to \"/x\""), vec![Event::Done(1234)]);
        assert!(reader.feed("ERROR! Download item 5 failed (Fi").is_empty());
        assert_eq!(
            reader.feed("le Not Found)."),
            vec![Event::Failed {
                id: 5,
                reason: "File Not Found".into()
            }]
        );
        let mut split = Reader::default();
        assert!(split.feed("Succ\u{1b}[").is_empty());
        assert!(split.feed("32mess. Downloaded item 9 to").len() <= 1);
    }

    #[test]
    fn junk_and_endless_output_are_ignored_and_bounded() {
        let mut reader = Reader::default();
        for _ in 0..2000 {
            assert!(
                reader
                    .feed("Update state (0x61) downloading, progress: 1.00 \u{e9}\n")
                    .is_empty()
            );
        }
        assert!(reader.tail().len() <= TAIL + 4);
        assert_eq!(reader.feed(OK), vec![Event::Done(3_552_152_340)]);
        assert!(next_event("Success. Downloaded item abc").is_none());
    }

    #[test]
    fn silences_are_explained_in_plain_words() {
        assert!(explain_text("error while loading shared libraries: libc.so.6").contains("32-bit"));
        assert!(explain_text("sh: linux32/steamcmd: No such file or directory").contains("32-bit"));
        assert!(explain_text("zsh: bad CPU type\nBad CPU type in executable").contains("Rosetta"));
        // Real text, from running steamcmd.sh with its 32-bit loader hidden (Linux, 2026-10-19):
        let linux =
            "./steamcmd.sh: line 85: /x/linux32/steamcmd: cannot execute: required file not found";
        assert!(
            explain_text(linux).contains("32-bit"),
            "the real Linux message"
        );
        // macOS's own text for a missing Rosetta (strerror(EBADARCH)), in bash's and zsh's wording.
        assert!(
            explain_text("./steamcmd.sh: line 9: /x/steamcmd: Bad CPU type in executable")
                .contains("Rosetta")
        );
        assert!(explain_text("zsh: bad CPU type in executable: ./steamcmd").contains("Rosetta"));
        // A folder that does not allow programs is not a 32-bit problem.
        let noexec =
            "./steamcmd.sh: line 85: /x/linux32/steamcmd: cannot execute: Permission denied";
        assert!(
            !explain_text(noexec).contains("32-bit"),
            "{}",
            explain_text(noexec)
        );
        assert!(explain_text("FAILED login with result code Timeout").contains("sign in"));
        assert_eq!(explain_text(""), "steamcmd gave no answer for this mod.");
        assert!(plain_reason("File Not Found").contains("no such mod"));
        assert!(plain_reason("No match").contains("no such mod"));
        assert!(plain_reason("Timeout").contains("(Timeout)"));
    }
}
