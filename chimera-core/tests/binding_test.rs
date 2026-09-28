//! Slot bindings (spec §5, § Testing "Bindings").

use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::block::ParamId;
use chimera_core::dsp::algo::params::AlgoOpParams;
use chimera_core::params::EngineType;
use chimera_core::ui::block_def::{BlockDef, SlotBinding, slot_addr};
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::chain::chain_def_for;
use chimera_core::ui::page::ValFmt;

/// Every page reachable from a Part chain (main pages and sub-pages).
fn part_defs() -> Vec<&'static BlockDef> {
    let mut defs = Vec::new();
    for engine in EngineType::ALL {
        for block in chain_def_for(engine).blocks {
            defs.push(block.def);
            defs.extend(block.sub_pages.iter().copied());
        }
    }
    defs
}

#[test]
fn every_part_slot_resolves_to_a_spec() {
    for def in part_defs() {
        for (i, slot) in def.params.iter().enumerate() {
            match slot.binding {
                SlotBinding::Empty | SlotBinding::SelectOp | SlotBinding::Route(_) => {}
                SlotBinding::Param(_) | SlotBinding::SelectedOp(_) => {
                    assert!(slot.spec().is_some(), "{} slot {i}: no spec", def.name)
                }
                SlotBinding::Legacy { .. } => {
                    panic!("{} slot {i}: Part pages must not be Legacy", def.name)
                }
            }
        }
    }
}

/// Spec §4: matrix source rows are what `Voice` produces — the eight
/// `ModSource`s on every Part chain.
#[test]
fn part_chains_offer_the_eight_sources() {
    for engine in EngineType::ALL {
        assert_eq!(
            chain_def_for(engine).mod_sources,
            chimera_core::ui::block_registry::PART_MOD_SOURCES,
            "{engine:?}"
        );
        let sound = chimera_core::preset::Sound::init(engine);
        assert!(
            sound.dest_registry.is_empty(),
            "{engine:?}: no pre-wired destinations"
        );
        assert_eq!(sound.mod_state.num_dests(), 0, "{engine:?}");
    }
}

#[test]
fn block_def_ids_are_unique() {
    let all: [&BlockDef; 36] = [
        &reg::MODAL_1,
        &reg::MODAL_2,
        &reg::ALGO_WAVE,
        &reg::ALGO_ALG,
        &reg::ALGO_LEVEL,
        &reg::DRIVE,
        &reg::FOLDER,
        &reg::FILTER,
        &reg::FILTER_MODE,
        &reg::ENVELOPE,
        &reg::LFO,
        &reg::EFX,
        &reg::MIXER,
        &reg::CHORUS,
        &reg::DELAY,
        &reg::DELAY_CHAR,
        &reg::TAPE,
        &reg::MASTER,
        &reg::MASTER_LEVEL,
        &reg::NOISE,
        &reg::MOD_MATRIX,
        &reg::PART,
        &reg::MIDI_CFG,
        &reg::EQ,
        &reg::SENDS,
        &reg::SYS_MIDI,
        &reg::SYS_TUNING,
        &reg::SYS_THEME,
        &reg::SYS_UPDATES,
        &reg::SYS_ABOUT,
        &reg::SYS_AUDIO,
        &reg::DEMO_WAVES,
        &reg::DEMO_SHAPES,
        &reg::DEMO_MOTION,
        &reg::DEMO_MATRIX,
        &reg::DEMO_FM,
    ];
    for (i, d) in all.iter().enumerate() {
        assert!(
            all[..i].iter().all(|o| o.id != d.id),
            "{} reuses id {}",
            d.name,
            d.id
        );
    }
}

/// Labels and formats of Part pages are exactly what they displayed before
/// bindings (spec labels + the two plan-D6 overrides; plan D5 BODY fix).
#[test]
fn part_pages_display_like_before() {
    use ValFmt::{Bi, Int, Uni};
    let want: [(&BlockDef, [(&str, ValFmt); 6]); 7] = [
        (
            &reg::MODAL_1,
            [
                ("MODE", Int(3)),
                ("EXCITE", Uni),
                ("DECAY", Uni),
                ("BRIGHT", Uni),
                ("POS", Uni),
                ("INHARM", Uni),
            ],
        ),
        (
            &reg::MODAL_2,
            [
                ("BODY", Uni),
                ("STIFF", Uni),
                ("FDBK", Uni),
                ("E.DPT", Uni),
                ("E.RAT", Uni),
                ("E.MIX", Uni),
            ],
        ),
        (
            &reg::DRIVE,
            [
                ("DRIVE", Uni),
                ("TONE", Bi),
                ("MIX", Bi),
                ("--", Uni),
                ("--", Uni),
                ("--", Uni),
            ],
        ),
        (
            &reg::FOLDER,
            [
                ("FOLD", Uni),
                ("SYM", Bi),
                ("MIX", Bi),
                ("--", Uni),
                ("--", Uni),
                ("--", Uni),
            ],
        ),
        (
            &reg::FILTER,
            [
                ("--", Uni),
                ("CUTOFF", Uni),
                ("RESO", Uni),
                (
                    "MODE",
                    ValFmt::Names(&chimera_core::dsp::filter::SVF_MODE_NAMES),
                ),
                ("ENV", ValFmt::Route),
                ("KEY", ValFmt::Route),
            ],
        ),
        (
            &reg::ENVELOPE,
            [
                ("ATK", Uni),
                ("DEC", Uni),
                ("SUS", Uni),
                ("REL", Uni),
                ("H", Uni),
                ("--", Uni),
            ],
        ),
        (
            &reg::LFO,
            [
                ("RATE", Uni),
                ("SHAPE", Int(4)),
                ("SYNC", Int(1)),
                ("PHASE", Uni),
                ("DEPTH", Uni),
                ("OFST", Bi),
            ],
        ),
    ];
    for (def, slots) in want {
        for (i, (label, fmt)) in slots.iter().enumerate() {
            assert_eq!(def.params[i].label(), *label, "{} slot {i}", def.name);
            assert_eq!(def.params[i].format(), *fmt, "{} slot {i}", def.name);
        }
    }
}

/// Fixed bindings ignore the operator selection (`SelectedOp` is tested in
/// `part_page_test`).
#[test]
fn slot_addr_resolves_fixed_bindings() {
    let level = |op| Some(ParamAddr::new(BlockRef::AlgoOp(op), AlgoOpParams::LEVEL));
    assert_eq!(slot_addr(&reg::ALGO_LEVEL, 3, Op::A), level(Op::D));
    assert_eq!(slot_addr(&reg::ALGO_LEVEL, 3, Op::F), level(Op::D));
    assert_eq!(slot_addr(&reg::ALGO_ALG, 5, Op::A), None); // empty
    assert_eq!(slot_addr(&reg::MIXER, 0, Op::A), None); // legacy
    assert_eq!(slot_addr(&reg::ALGO_ALG, 9, Op::A), None); // out of range
}

/// Each column of a group page edits its own operator; a copy-paste slip
/// would silently edit the wrong operator's sound.
#[test]
fn group_pages_bind_each_column_to_its_operator() {
    let pages: [(&BlockDef, ParamId); 2] = [
        (&reg::ALGO_WAVE, AlgoOpParams::WAVE),
        (&reg::ALGO_LEVEL, AlgoOpParams::LEVEL),
    ];
    for (def, id) in pages {
        for (i, op) in Op::ALL.into_iter().enumerate() {
            assert_eq!(
                slot_addr(def, i, Op::A),
                Some(ParamAddr::new(BlockRef::AlgoOp(op), id)),
                "{} slot {i}",
                def.name
            );
        }
    }
}
