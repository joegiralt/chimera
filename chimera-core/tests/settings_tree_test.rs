use chimera_core::ui::block_registry::ALL_CHAINS;
use chimera_core::ui::settings::leaves::{CHANNELS, OUTPUTS};
use chimera_core::ui::settings::{Kind, ROOT, Row, row_at, rows};

fn walk(row: &'static Row, path: &mut Vec<u8>, f: &mut dyn FnMut(&'static Row, &[u8])) {
    f(row, path);
    if let Kind::List(list) = row.kind {
        for (i, r) in list.iter().enumerate() {
            path.push(i as u8);
            walk(r, path, f);
            path.pop();
        }
    }
}

fn all() -> Vec<(&'static Row, Vec<u8>)> {
    let mut v = Vec::new();
    walk(&ROOT, &mut Vec::new(), &mut |r, p| v.push((r, p.to_vec())));
    v
}

#[test]
fn every_later_row_names_its_issue() {
    let mut labels = Vec::new();
    for (r, _) in all() {
        if let Kind::Later(n) = r.kind {
            labels.push((r.label, n.get()));
        }
    }
    let names: Vec<_> = labels.iter().map(|l| l.0).collect();
    assert_eq!(
        names,
        [
            "ORBIT",
            "SYNC",
            "PORT CONFIG",
            "SYSEX DUMP",
            "SENDS",
            "STORAGE",
            "FORMAT CARD",
            "USB CONFIG",
            "TEST TONE",
            "INPUT TEST"
        ]
    );
    assert!(labels.contains(&("SENDS", 259)));
    assert!(labels.contains(&("TEST TONE", 295)));
    assert!(labels.contains(&("INPUT TEST", 296)));
}

#[test]
fn later_issues_are_distinct() {
    let mut n: Vec<u16> = all()
        .iter()
        .filter_map(|(r, _)| match r.kind {
            Kind::Later(i) => Some(i.get()),
            _ => None,
        })
        .collect();
    n.sort();
    let len = n.len();
    n.dedup();
    assert_eq!(n.len(), len);
}

#[test]
fn leaf_chains_are_in_all_chains() {
    for (r, _) in all() {
        if let Kind::Leaf(c) = r.kind {
            assert!(
                ALL_CHAINS.iter().any(|a| core::ptr::eq(*a, c.chain())),
                "{}",
                r.label
            );
        }
    }
    assert_eq!(CHANNELS.id, 68);
    assert_eq!(OUTPUTS.id, 69);
}

#[test]
fn top_list_is_the_spec_order() {
    let labels: Vec<_> = rows(&[]).iter().map(|r| r.label).collect();
    assert_eq!(
        labels,
        [
            "PROJECT",
            "PART",
            "ORBIT",
            "MIDI CONFIG",
            "SYSEX DUMP",
            "AUDIO ROUTING",
            "PERSONALIZE",
            "SYSTEM"
        ]
    );
}

#[test]
fn paths_resolve() {
    assert_eq!(row_at(&[0, 0]).unwrap().label, "LOAD PROJECT");
    assert_eq!(row_at(&[6, 0]).unwrap().label, "THEME");
    assert!(row_at(&[9]).is_none());
    assert!(rows(&[0, 0]).is_empty());
}

#[test]
fn depth_fits_settings_at() {
    for (_, p) in all() {
        assert!(p.len() <= 4);
    }
}

fn path_of(labels: &[&str]) -> Vec<u8> {
    let mut path = Vec::new();
    for l in labels {
        path.push(rows(&path).iter().position(|r| r.label == *l).unwrap() as u8);
    }
    path
}

#[test]
fn system_is_the_spec_order() {
    let labels: Vec<_> = rows(&path_of(&["SYSTEM"]))
        .iter()
        .map(|r| r.label)
        .collect();
    assert_eq!(
        labels,
        [
            "OS UPGRADE",
            "STORAGE",
            "FORMAT CARD",
            "USB CONFIG",
            "DIAGNOSTICS",
            "ABOUT"
        ]
    );
}

/// DIAGNOSTICS holds DEMO in debug builds only (`--release` checks the
/// other side); DEMO has a row per page.
#[test]
fn demo_only_in_debug() {
    let diag = path_of(&["SYSTEM", "DIAGNOSTICS"]);
    let labels: Vec<_> = rows(&diag).iter().map(|r| r.label).collect();
    let mut want = vec!["AUDIO LOAD", "TEST TONE", "INPUT TEST"];
    if cfg!(debug_assertions) {
        want.push("DEMO");
        let demo = rows(&path_of(&["SYSTEM", "DIAGNOSTICS", "DEMO"]));
        let n = chimera_core::ui::block_registry::DEMO_BLOCKS.len();
        assert_eq!(demo.len(), n);
        assert!(demo.iter().all(|r| matches!(r.kind, Kind::Leaf(_))));
    }
    assert_eq!(labels, want);
}

#[test]
fn every_crumb_fits() {
    for (r, _) in all() {
        assert!(r.crumb.len() <= 8, "{}", r.crumb);
    }
}
