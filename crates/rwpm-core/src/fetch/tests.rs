//! The reading of the helper's lines. The tests that start a (fake) helper are in
//! `tests/fetch.rs`: they spawn processes, which must not share a test program with the lock
//! tests.

use super::*;

#[test]
fn lines_are_read_and_anything_else_is_ignored() {
    assert_eq!(parse_line("ok"), Some(Line::Ready));
    assert_eq!(
        parse_line("ok 5 new"),
        Some(Line::Ok {
            id: 5,
            fresh: Fresh::New
        })
    );
    assert_eq!(
        parse_line("ok 5 already\n"),
        Some(Line::Ok {
            id: 5,
            fresh: Fresh::Already
        })
    );
    assert_eq!(
        parse_line("progress 5 10 20"),
        Some(Line::Progress {
            id: 5,
            done: 10,
            total: 20
        })
    );
    assert_eq!(
        parse_line("fail 5 no luck at all"),
        Some(Line::Fail {
            id: 5,
            reason: "no luck at all".into()
        })
    );
    assert_eq!(
        parse_line("error Steam is not running"),
        Some(Line::Error("Steam is not running".into()))
    );
    for junk in [
        "",
        "hello",
        "ok x new",
        "ok 5 maybe",
        "ok 5",
        "progress 1 2",
        "progress a b c",
        "fail",
        "fail x why",
        "weird 9",
        "\u{1b}[31mok 5 new",
    ] {
        assert_eq!(parse_line(junk), None, "{junk:?}");
    }
}

#[test]
fn the_package_id_the_pack_names_is_compared_in_lower_case_without_the_steam_suffix() {
    assert_eq!(plain_id("Good.Mod"), "good.mod");
    assert_eq!(plain_id("good.mod_steam"), "good.mod");
    assert_eq!(plain_id("Good.Mod_STEAM"), "good.mod");
    assert_eq!(plain_id("steamy.mod"), "steamy.mod");
}
