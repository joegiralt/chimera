//! The one golden check every harness uses (#108). A case matches its row
//! in the harness's `GOLDENS` table bit-for-bit; a case with no row, or a
//! row with no case, fails too. Re-record only for an intended change,
//! with `GOLDEN_RECORD=1 cargo test -p chimera-core --test <harness> --
//! --nocapture`, which prints the whole table, in case order, to paste over
//! `GOLDENS`. Goldens that no longer reflect intended behaviour stay locked
//! and are listed with their issue URL (`known_broken`).

use std::fmt::Debug;

/// A recorded value, printable as the table literal it came from.
pub trait Golden: PartialEq + Debug + Copy {
    fn literal(&self) -> String;
}

/// FNV-1a 64 over a render.
impl Golden for u64 {
    fn literal(&self) -> String {
        format!("0x{self:016x}")
    }
}

/// The hash and the sample bits at `SPOT_IDX`.
impl Golden for (u64, [u32; 8]) {
    fn literal(&self) -> String {
        format!("(0x{:016x}, {:?})", self.0, self.1)
    }
}

/// Compare each `(name, value)` in `got` with `table`, or with
/// `GOLDEN_RECORD` set print the table to paste instead.
pub fn check<V: Golden>(table: &[(&str, V)], got: &[(&str, V)]) {
    if std::env::var_os("GOLDEN_RECORD").is_some() {
        for (name, v) in got {
            println!("    (\"{name}\", {}),", v.literal());
        }
        return;
    }
    let failures = mismatches(table, got);
    assert!(
        failures.is_empty(),
        "golden mismatch:\n{}",
        failures.join("\n")
    );
}

/// Every way `got` and `table` disagree: a value that differs, a case with
/// no row, a row with no case.
pub fn mismatches<V: Golden>(table: &[(&str, V)], got: &[(&str, V)]) -> Vec<String> {
    let mut failures = Vec::new();
    for &(name, v) in got {
        match table.iter().find(|r| r.0 == name) {
            None => failures.push(format!("{name}: no golden recorded")),
            Some(&(_, want)) if want != v => {
                failures.push(format!("{name}: {} (want {})", v.literal(), want.literal()))
            }
            Some(_) => {}
        }
    }
    for &(name, _) in table {
        if !got.iter().any(|g| g.0 == name) {
            failures.push(format!("{name}: recorded, but no such case"));
        }
    }
    failures
}

/// Every known-broken golden names a case and a GitHub issue URL.
pub fn known_broken(list: &[(&str, &str)], cases: &[&str]) {
    const TRACKER: &str = "https://github.com/joegiralt/chimera/issues/";
    for (case, issue) in list {
        assert!(cases.contains(case), "unknown case {case}");
        let num = issue.strip_prefix(TRACKER);
        assert!(
            num.is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())),
            "{case}: {issue} is not a GitHub issue URL"
        );
    }
}
