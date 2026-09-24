//! Slot bindings (spec §5, § Testing "Bindings").

use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::block::ParamId;
use chimera_core::params::FmOpParams;
use chimera_core::preset::ChainType;
use chimera_core::ui::block_def::{slot_addr, BlockDef, SlotBinding};
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::chain::chain_def_for;
use chimera_core::ui::page::ValFmt;

/// Every page reachable from a Part chain (main pages and sub-pages).
fn part_defs() -> Vec<&'static BlockDef> {
    let mut defs = Vec::new();
    for ct in ChainType::ALL {
        for block in chain_def_for(ct).blocks {
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
                SlotBinding::Empty | SlotBinding::SelectOp => {}
                SlotBinding::Param(_) | SlotBinding::SelectedOp(_) => {
                    assert!(slot.spec().is_some(), "{} slot {i}: no spec", def.name)
                }
                SlotBinding::Legacy { .. } => panic!("{} slot {i}: Part pages must not be Legacy", def.name),
            }
        }
    }
}

/// Spec §4: matrix source rows are what `Voice` produces — ENV and LFO on
/// every Part chain (FM no longer lists four envelope rows).
#[test]
fn part_chains_offer_env_and_lfo_sources() {
    for ct in ChainType::ALL {
        assert_eq!(chain_def_for(ct).mod_sources, ["ENV", "LFO"], "{ct:?}");
        let sound = chimera_core::preset::Sound::init(ct);
        assert!(sound.dest_registry.is_empty(), "{ct:?}: no pre-wired destinations");
        assert_eq!(sound.mod_state.num_dests(), 0, "{ct:?}");
    }
    // The FM_ENV pages stay as MOD sub-pages; they are just not source rows.
    assert_eq!(chain_def_for(ChainType::Fm).blocks[4].sub_pages.len(), 4);
}

#[test]
fn block_def_ids_are_unique() {
    let all: [&BlockDef; 40] = [
        &reg::PIZZA, &reg::MODAL_1, &reg::MODAL_2, &reg::VA, &reg::FM_ALG, &reg::FM_OP,
        &reg::FM_RATIO, &reg::DRIVE, &reg::FOLDER, &reg::FILTER, &reg::ENVELOPE, &reg::LFO,
        &reg::ENV_AMP, &reg::ENV_FILTER, &reg::ENV_AUX, &reg::EFX, &reg::MIXER, &reg::CHORUS,
        &reg::DELAY, &reg::MASTER, &reg::NOISE, &reg::MOD_MATRIX, &reg::FM_ENV1, &reg::FM_ENV2,
        &reg::FM_ENV3, &reg::FM_ENV4, &reg::PART, &reg::MIDI_CFG, &reg::EQ, &reg::SENDS,
        &reg::SYS_MIDI, &reg::SYS_TUNING, &reg::SYS_THEME, &reg::SYS_UPDATES, &reg::SYS_ABOUT,
        &reg::DEMO_WAVES, &reg::DEMO_SHAPES, &reg::DEMO_MOTION, &reg::DEMO_MATRIX, &reg::DEMO_FM,
    ];
    for (i, d) in all.iter().enumerate() {
        assert!(all[..i].iter().all(|o| o.id != d.id), "{} reuses id {}", d.name, d.id);
    }
}

/// Labels and formats of Part pages are exactly what they displayed before
/// bindings (spec labels + the two plan-D6 overrides; plan D5 BODY fix), except
/// the operator selector and the algorithm, shown 1–4 and 1–8 since the UI
/// refresh.
#[test]
fn part_pages_display_like_before() {
    use ValFmt::{Bi, Int, OneBased, Uni};
    let want: [(&BlockDef, [(&str, ValFmt); 6]); 15] = [
        (&reg::PIZZA, [("SHAPE", Uni), ("CRUSH", Uni), ("LEVEL", Uni), ("--", Uni), ("--", Uni), ("--", Uni)]),
        (&reg::MODAL_1, [("MODE", Int(3)), ("EXCITE", Uni), ("DECAY", Uni), ("BRIGHT", Uni), ("POS", Uni), ("INHARM", Uni)]),
        (&reg::MODAL_2, [("BODY", Uni), ("STIFF", Uni), ("FDBK", Uni), ("E.DPT", Uni), ("E.RAT", Uni), ("E.MIX", Uni)]),
        (&reg::FM_ALG, [("ALG", OneBased(7)), ("--", Uni), ("LEVEL", Uni), ("--", Uni), ("--", Uni), ("--", Uni)]),
        (&reg::FM_OP, [("OP", OneBased(3)), ("WAVE", Int(7)), ("LEVEL", Uni), ("FDBK", Int(7)), ("DETUN", Bi), ("V.SNS", Int(7))]),
        (&reg::FM_RATIO, [("OP1", Int(63)), ("OP2", Int(63)), ("OP3", Int(63)), ("OP4", Int(63)), ("FINE", Int(15)), ("--", Uni)]),
        (&reg::DRIVE, [("DRIVE", Uni), ("TONE", Bi), ("MIX", Bi), ("--", Uni), ("--", Uni), ("--", Uni)]),
        (&reg::FOLDER, [("FOLD", Uni), ("SYM", Bi), ("MIX", Bi), ("--", Uni), ("--", Uni), ("--", Uni)]),
        (&reg::FILTER, [("CUTOFF", Uni), ("RESO", Uni), ("DRIVE", Uni), ("FM", Uni), ("ENV", Bi), ("TRACK", Uni)]),
        (&reg::ENVELOPE, [("ATK", Uni), ("DEC", Uni), ("SUS", Uni), ("REL", Uni), ("DEPTH", Uni), ("VEL", Uni)]),
        (&reg::LFO, [("RATE", Uni), ("SHAPE", Int(4)), ("SYNC", Int(1)), ("PHASE", Uni), ("DEPTH", Uni), ("OFST", Bi)]),
        (&reg::FM_ENV1, [("AR", Int(31)), ("D1R", Int(31)), ("D1L", Int(15)), ("D2R", Int(31)), ("RR", Int(15)), ("RS", Int(3))]),
        (&reg::FM_ENV2, [("AR", Int(31)), ("D1R", Int(31)), ("D1L", Int(15)), ("D2R", Int(31)), ("RR", Int(15)), ("RS", Int(3))]),
        (&reg::FM_ENV3, [("AR", Int(31)), ("D1R", Int(31)), ("D1L", Int(15)), ("D2R", Int(31)), ("RR", Int(15)), ("RS", Int(3))]),
        (&reg::FM_ENV4, [("AR", Int(31)), ("D1R", Int(31)), ("D1L", Int(15)), ("D2R", Int(31)), ("RR", Int(15)), ("RS", Int(3))]),
    ];
    for (def, slots) in want {
        for (i, (label, fmt)) in slots.iter().enumerate() {
            assert_eq!(def.params[i].label(), *label, "{} slot {i}", def.name);
            assert_eq!(def.params[i].format(), *fmt, "{} slot {i}", def.name);
        }
    }
}

/// Spec §5: `SelectedOp` resolves to the operator selected when the address
/// is built; fixed bindings ignore the selection.
#[test]
fn slot_addr_resolves_selected_op_at_build_time() {
    assert_eq!(
        slot_addr(&reg::FM_OP, 2, Op::C),
        Some(ParamAddr::new(BlockRef::FmOp(Op::C), FmOpParams::LEVEL))
    );
    assert_eq!(
        slot_addr(&reg::FM_RATIO, 1, Op::D),
        Some(ParamAddr::new(BlockRef::FmOp(Op::B), FmOpParams::COARSE))
    );
    assert_eq!(
        slot_addr(&reg::FM_RATIO, 4, Op::D),
        Some(ParamAddr::new(BlockRef::FmOp(Op::D), FmOpParams::FINE))
    );
    assert_eq!(slot_addr(&reg::FM_OP, 0, Op::A), None); // the selector
    assert_eq!(slot_addr(&reg::PIZZA, 5, Op::A), None); // empty
    assert_eq!(slot_addr(&reg::MIXER, 0, Op::A), None); // legacy
    assert_eq!(slot_addr(&reg::PIZZA, 9, Op::A), None); // out of range
    // FM_RATIO slot 3 is the OP4 column: a fixed binding, so `sel_op` doesn't matter.
    assert_eq!(
        slot_addr(&reg::FM_RATIO, 3, Op::A),
        Some(ParamAddr::new(BlockRef::FmOp(Op::D), FmOpParams::COARSE))
    );
}

/// Each FM_ENVn page edits its own operator's envelope; a copy-paste op slip
/// (e.g. FM_ENV3 accidentally reading FmOp(B)) would silently edit the wrong
/// operator's sound.
#[test]
fn fm_env_pages_bind_to_their_own_operator() {
    let env_params: [ParamId; 6] = [
        FmOpParams::ATTACK_RATE,
        FmOpParams::DECAY1_RATE,
        FmOpParams::DECAY1_LEVEL,
        FmOpParams::DECAY2_RATE,
        FmOpParams::RELEASE_RATE,
        FmOpParams::RATE_SCALING,
    ];
    let pages: [(&BlockDef, Op); 4] =
        [(&reg::FM_ENV1, Op::A), (&reg::FM_ENV2, Op::B), (&reg::FM_ENV3, Op::C), (&reg::FM_ENV4, Op::D)];
    for (def, op) in pages {
        for (i, &id) in env_params.iter().enumerate() {
            assert_eq!(slot_addr(def, i, Op::A), Some(ParamAddr::new(BlockRef::FmOp(op), id)), "{} slot {i}", def.name);
        }
    }
}
