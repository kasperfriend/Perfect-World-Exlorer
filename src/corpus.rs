//! The one place a shipped-corpus test says it cannot run.
//!
//! A test that reads the retail archives (`gfx.pck`, `world.ecwld`, the loose
//! `element` directory) has to skip on a machine that does not stage them: CI has
//! no such machine, and the archives are 97 MB and more, so they are not in the
//! repository. Printing `SKIPPED: …` is better than returning a bare `ok`, but it
//! still means a corpus assertion can rot for months while every run is green —
//! and exactly that happened here, where four expectations about the shipped
//! waterfall, beams and portal were written, reviewed and shipped without ever
//! being executed.
//!
//! `PWE_CORPUS=require` turns every skip into a failure, so a machine that *does*
//! stage the archives can run the suite as a gate:
//!
//! ```sh
//! PWE_CORPUS=require cargo test --bin perfect-world-explorer
//! ```
//!
//! It is deliberately all-or-nothing: a machine missing one of the archives fails
//! with the path it is missing rather than quietly testing the rest, because the
//! point of the switch is to know that the shipped data was checked, not which
//! fraction of it was.

/// Environment variable that turns a corpus skip into a failure.
pub const ENV: &str = "PWE_CORPUS";

/// Whether the caller asked for the corpus suite to be run as a gate.
pub fn required() -> bool {
    required_with(std::env::var(ENV).ok().as_deref())
}

/// [`required`] over an already-read value, so the rule can be tested without
/// mutating the process environment.
pub fn required_with(value: Option<&str>) -> bool {
    value.is_some_and(|value| value.eq_ignore_ascii_case("require"))
}

/// Report a missing shipped archive, and fail when the corpus is required.
///
/// `what` names the archive the way a reader would stage it, e.g.
/// `` `google_drive/gfx.pck` ``.
pub fn absent(what: &str) {
    absent_with(required(), what);
}

/// [`absent`] with the decision already made.
pub fn absent_with(required: bool, what: &str) {
    if required {
        panic!("{ENV}=require, but this machine has no {what}");
    }
    eprintln!(
        "SKIPPED: no {what} on this machine; this test proves nothing about the shipped data \
         (set {ENV}=require to make this a failure)"
    );
}

#[cfg(test)]
mod tests {
    use super::{absent_with, required_with};

    /// The default has to be "off": CI stages no archives, so a corpus test that
    /// failed there would fail every run for a reason the change cannot fix.
    #[test]
    fn only_the_word_require_turns_the_gate_on() {
        assert!(!required_with(None));
        assert!(!required_with(Some("")));
        assert!(!required_with(Some("0")));
        assert!(!required_with(Some("skip")));
        assert!(required_with(Some("require")));
        assert!(required_with(Some("REQUIRE")));
        assert!(required_with(Some("Require")));
    }

    /// With the gate on, a missing archive is a failure that names the path —
    /// that message is the whole point, because it tells the operator which
    /// archive to stage.
    #[test]
    #[should_panic(expected = "PWE_CORPUS=require, but this machine has no `google_drive/gfx.pck`")]
    fn the_gate_fails_instead_of_skipping() {
        absent_with(true, "`google_drive/gfx.pck`");
    }

    /// With the gate off it prints and returns, which is what every run without a
    /// staged archive does.
    #[test]
    fn the_ordinary_case_skips() {
        absent_with(false, "`google_drive/gfx.pck`");
    }
}
