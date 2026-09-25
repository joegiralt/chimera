//! Every spec table is well-formed (spec § Testing "Specs").

use chimera_core::addr::BlockRef;
use chimera_core::block::{ParamKind, ParamSpec, ValFmt};

/// Every block's spec table, via the one exhaustive `BlockRef` list.
fn all_specs() -> Vec<(String, &'static [ParamSpec])> {
    BlockRef::ALL
        .iter()
        .map(|b| (format!("{b:?}"), b.specs()))
        .collect()
}

fn check(name: &str, specs: &[ParamSpec]) {
    for (i, s) in specs.iter().enumerate() {
        let what = format!("{name}.{}", s.label);
        assert!(
            specs[..i].iter().all(|o| o.id != s.id),
            "{what}: duplicate id {:?}",
            s.id
        );
        assert!(s.min < s.max, "{what}: min {} >= max {}", s.min, s.max);
        assert!(
            s.default >= s.min && s.default <= s.max,
            "{what}: default {} out of range",
            s.default
        );
        assert!(s.step > 0.0, "{what}: step {} <= 0", s.step);
        if s.kind == ParamKind::Enum {
            assert!(!s.modulatable, "{what}: Enum params are never modulatable");
            assert_eq!(s.min, 0.0, "{what}: Enum min must be 0");
        }
        if let ValFmt::Int(n) = s.fmt {
            assert!(
                s.kind != ParamKind::Continuous,
                "{what}: Int({n}) on a Continuous param"
            );
            assert_eq!(
                s.max - s.min,
                n as f32,
                "{what}: Int({n}) but range is {}..={}",
                s.min,
                s.max
            );
        }
    }
}

#[test]
fn every_spec_table_is_well_formed() {
    for (name, specs) in all_specs() {
        check(&name, specs);
    }
}
