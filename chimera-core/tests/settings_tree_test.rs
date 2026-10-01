use chimera_core::ui::block_registry::ALL_CHAINS;
use chimera_core::ui::settings::leaves::{CHANNELS, OUTPUTS};
use chimera_core::ui::settings::{Kind, ROOT, Row, Status, row_at, rows};

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
        if let Status::Later(n) = r.status {
            assert!(n > 0, "{}", r.label);
            labels.push((r.label, n));
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
            "USB CONFIG"
        ]
    );
    assert!(labels.contains(&("SENDS", 259)));
}

#[test]
fn leaf_chains_are_in_all_chains() {
    for (r, _) in all() {
        if let Kind::Leaf(c) = r.kind {
            assert!(
                ALL_CHAINS.iter().any(|a| core::ptr::eq(*a, c)),
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

#[cfg(debug_assertions)]
#[test]
fn demo_only_in_debug() {
    assert_eq!(rows(&[7]).last().unwrap().label, "DEMO");
}

#[test]
fn every_crumb_fits() {
    for (r, _) in all() {
        assert!(r.crumb.len() <= 8, "{}", r.crumb);
    }
}
