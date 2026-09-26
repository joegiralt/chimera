//! Port choice for the desktop's midir input: a pure function so "no
//! device" and "loopback only" are host-tested without opening a real
//! port.

pub fn pick_port(names: &[String], wanted: Option<&str>) -> Option<usize> {
    let lower = |s: &str| s.to_ascii_lowercase();
    match wanted {
        Some(w) => names.iter().position(|n| lower(n).contains(&lower(w))),
        None => names.iter().position(|n| !lower(n).contains("through")),
    }
}

#[cfg(test)]
mod tests {
    use super::pick_port;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_first_real_port_wins() {
        let n = names(&[
            "Midi Through:Midi Through Port-0 14:0",
            "KeyStep 37:KeyStep 37 MIDI 1 24:0",
        ]);
        assert_eq!(pick_port(&n, None), Some(1));
    }

    #[test]
    fn a_wanted_name_matches_case_insensitively() {
        let n = names(&["Arturia KeyStep", "nanoKEY2"]);
        assert_eq!(pick_port(&n, Some("NANOkey")), Some(1));
        assert_eq!(pick_port(&n, Some("launchpad")), None);
    }

    #[test]
    fn no_ports_or_only_loopback_picks_nothing() {
        assert_eq!(pick_port(&[], None), None);
        assert_eq!(
            pick_port(&names(&["Midi Through:Midi Through Port-0 14:0"]), None),
            None
        );
    }
}
