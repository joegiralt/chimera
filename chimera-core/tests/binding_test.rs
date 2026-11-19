//! Slot bindings (spec §5, § Testing "Bindings").

use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::block::ParamId;
use chimera_core::dsp::algo::params::{AlgoOpParams, AlgoParams};
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::ui::block_def::{BlockDef, SlotBinding, slot_addr};
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::nav::{PartChain, chain_def_for};
use chimera_core::ui::page::ValFmt;
use chimera_core::ui::view::{SlotCtx, view};

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
                SlotBinding::Empty
                | SlotBinding::SelectOp
                | SlotBinding::SelectPad
                | SlotBinding::SelectRoute
                | SlotBinding::FilterPanel(_)
                | SlotBinding::PadFilterPanel(_)
                | SlotBinding::FxPanel(_)
                | SlotBinding::EnvPanel(..)
                | SlotBinding::LfoPanel(..)
                | SlotBinding::ModalPanel(..)
                | SlotBinding::ReverbPanel(..)
                | SlotBinding::Machine(_)
                | SlotBinding::Knob(_)
                | SlotBinding::Source => {}
                SlotBinding::Param(_)
                | SlotBinding::SelectedOp(_)
                | SlotBinding::SelectedRoute(_)
                | SlotBinding::OpPanel(..) => {
                    assert!(slot.spec().is_some(), "{} slot {i}: no spec", def.name)
                }
                SlotBinding::Legacy(_) => {
                    panic!("{} slot {i}: Part pages must not be Legacy", def.name)
                }
                SlotBinding::SelectedRing(_) | SlotBinding::Orbit(_) => {
                    panic!("{} slot {i}: an ORBIT cell on a Part page", def.name)
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
        // The default routes live in the fixed FREQ column; the registry
        // holds only the wheel's PITCH and LFO 1 DEPTH (spec § 9, #876).
        assert_eq!(sound.dest_registry.len(), 2, "{engine:?}");
        assert_eq!(sound.mod_state.num_dests(), 2, "{engine:?}");
    }
}

/// Every page of every chain, main pages and sub-pages, has its own id: an
/// id is the page's key for focus memory and dirty tracking.
#[test]
fn block_def_ids_are_unique() {
    let mut seen: Vec<&BlockDef> = Vec::new();
    for chain in reg::ALL_CHAINS {
        for block in chain.blocks {
            for def in core::iter::once(block.def).chain(block.sub_pages.iter().copied()) {
                if let Some(o) = seen.iter().find(|o| o.id == def.id) {
                    assert!(
                        core::ptr::eq(*o, def),
                        "{} reuses id {} of {}",
                        def.name,
                        def.id,
                        o.name
                    );
                } else {
                    seen.push(def);
                }
            }
        }
    }
}

/// Every chain the navigation can reach is in `ALL_CHAINS`, so the
/// registry-wide checks cover it: each engine's, the mixer's, and every
/// SETTINGS leaf's, walked by `Location`.
#[test]
fn all_chains_holds_every_reachable_chain() {
    use chimera_core::project::PartId;
    use chimera_core::ui::nav::{Location, MixPage, NavCtx, OrbitCx, chain_def_for};
    use chimera_core::ui::settings::{Kind, rows};
    fn leaves(path: &mut Vec<u8>, out: &mut Vec<Location>) {
        for (i, r) in rows(path).iter().enumerate() {
            path.push(i as u8);
            match r.kind {
                Kind::Leaf(_) => out.push(Location::settings_at(path, 0)),
                Kind::List(_) => leaves(path, out),
                _ => {}
            }
            path.pop();
        }
    }
    let mut at = Vec::new();
    for engine in EngineType::ALL {
        at.push((
            PartChain::base(engine),
            Location::pages(PartId::ALL[0], chain_def_for(engine).home()),
        ));
    }
    at.push((
        PartChain::AlgoFormant,
        Location::pages(PartId::ALL[0], PartChain::AlgoFormant.def().home()),
    ));
    at.push((
        PartChain::Algo,
        Location::mixer(PartId::ALL[0], MixPage::Sends),
    ));
    let mut ls = Vec::new();
    leaves(&mut Vec::new(), &mut ls);
    assert!(!ls.is_empty());
    at.extend(ls.into_iter().map(|l| (PartChain::Algo, l)));
    for (chain, l) in at {
        let cx = NavCtx {
            chains: [chain; 6],
            dyn_rows: 0,
            orbit: OrbitCx::NONE,
            rev: chimera_core::dsp::reverb::ReverbType::Quad,
            pages: chimera_core::ui::theme_settings::Pages::Stack,
            opens: Default::default(),
        };
        let (def, _) = l.page(&cx).unwrap();
        assert!(
            reg::ALL_CHAINS.iter().any(|c| core::ptr::eq(*c, def)),
            "{}",
            def.name
        );
    }
}

/// Labels and formats of Part pages are exactly what they displayed before
/// bindings (spec labels + the two plan-D6 overrides; plan D5 BODY fix).
#[test]
fn part_pages_display_like_before() {
    use ValFmt::{Law, Names, Uni};
    use chimera_core::dsp::modulator::EnvSpeed::Med;
    use chimera_core::dsp::modulator::law::Law::{Attack, Cutoff, DecRel, Hold, LfoHz, Pct};
    let want: [(&BlockDef, [(&str, ValFmt); 6]); 7] = [
        (
            &reg::MODAL_1,
            [
                ("MODEL", Names(&chimera_core::dsp::modal::MODEL_NAMES)),
                ("STRUCT", Uni),
                ("BRIGHT", Uni),
                ("DAMP", Uni),
                ("POS", Uni),
                ("--", Uni),
            ],
        ),
        (
            &reg::MODAL_EXC,
            [
                ("EXCITE", Uni),
                ("COLOR", Uni),
                ("--", Uni),
                ("--", Uni),
                ("--", Uni),
                ("--", Uni),
            ],
        ),
        (
            &reg::MODAL_2,
            [
                ("BODY", Uni),
                ("ENS.D", Uni),
                ("ENS.R", Uni),
                ("ENS.M", Uni),
                ("--", Uni),
                ("--", Uni),
            ],
        ),
        (
            &reg::AMP,
            [
                ("VEL", Uni),
                ("LPG", Uni),
                ("DECAY", Uni),
                ("STRIKE", Uni),
                ("BASE", Uni),
                ("--", Uni),
            ],
        ),
        (
            &reg::FILTER,
            [
                (
                    "TYPE",
                    ValFmt::Names(&chimera_core::dsp::filter::KIND_NAMES),
                ),
                (
                    "FX",
                    ValFmt::Names(&chimera_core::dsp::voice_fx::params::FxType::NAMES),
                ),
                ("Q", Uni),
                (
                    "MODE",
                    ValFmt::Names(chimera_core::dsp::filter::FilterKind::Svf.mode_labels()),
                ),
                ("FX PLACE", ValFmt::Names(&["PRE", "POST"])),
                ("FREQ", Law(Cutoff)),
            ],
        ),
        (
            &reg::ENVELOPE,
            [
                ("ATTACK", Law(Attack(Med))),
                ("DECAY", Law(DecRel(Med))),
                ("SUSTAIN", Law(Pct)),
                ("RELEASE", Law(DecRel(Med))),
                ("HOLD", Law(Hold(Med))),
                ("TYPE", Names(&["A", "B"])),
            ],
        ),
        (
            &reg::LFO,
            [
                // Hz on its own linear curve (ADR 0177).
                ("RATE", Law(LfoHz)),
                ("SHAPE", Names(&["SINE", "TRI", "SAW", "SQR", "S&H"])),
                ("SYNC", Names(&["FREE", "RETRIG"])),
                ("PHASE", Uni),
                ("DEPTH", Uni),
                ("TYPE", Names(&["CLASSIC", "FUNC"])),
            ],
        ),
    ];
    // A panel slot's own label and format are empty: FLT, ENV, LFO, EXC
    // and MDL2 (here STRING's) read their views.
    let ctx = ctx();
    let panels: [&BlockDef; 5] = [
        &reg::FILTER,
        &reg::ENVELOPE,
        &reg::LFO,
        &reg::MODAL_EXC,
        &reg::MODAL_2,
    ];
    for (def, slots) in want {
        for (i, (label, fmt)) in slots.iter().enumerate() {
            let got = if panels.iter().any(|p| core::ptr::eq(*p, def)) {
                let v = view(def, i, &ctx);
                (v.label(), v.fmt())
            } else {
                (def.params[i].label(), def.params[i].format())
            };
            assert_eq!(got, (*label, *fmt), "{} slot {i}", def.name);
        }
    }
}

/// Fixed bindings ignore the operator selection (`SelectedOp` is tested in
/// `part_page_test`).
#[test]
fn slot_addr_resolves_fixed_bindings() {
    let level = |op| Some(ParamAddr::new(BlockRef::AlgoOp(op), AlgoOpParams::LEVEL));
    let on = |op| SlotCtx::read(&ParamSnapshot::default(), op);
    assert_eq!(slot_addr(&reg::ALGO_LEVEL, 3, &on(Op::A)), level(Op::D));
    assert_eq!(slot_addr(&reg::ALGO_LEVEL, 3, &on(Op::F)), level(Op::D));
    assert_eq!(
        slot_addr(&reg::ALGO_ALG, 5, &ctx()),
        Some(ParamAddr::new(BlockRef::Algo, AlgoParams::VOWEL))
    );
    assert_eq!(slot_addr(&reg::DEMO_WAVES, 0, &ctx()), None); // legacy
    assert_eq!(slot_addr(&reg::ALGO_ALG, 9, &ctx()), None); // out of range
}

fn ctx() -> SlotCtx {
    SlotCtx::read(&ParamSnapshot::default(), Op::A)
}

/// Each column of a group page edits its own operator; a copy-paste slip
/// would silently edit the wrong operator's sound.
#[test]
fn group_pages_bind_each_column_to_its_operator() {
    let pages: [(&BlockDef, ParamId); 3] = [
        (&reg::ALGO_WAVE, AlgoOpParams::WAVE),
        (&reg::ALGO_LEVEL, AlgoOpParams::LEVEL),
        (&reg::ALGO_FORMANT, AlgoOpParams::FRMT),
    ];
    for (def, id) in pages {
        for (i, op) in Op::ALL.into_iter().enumerate() {
            assert_eq!(
                slot_addr(def, i, &ctx()),
                Some(ParamAddr::new(BlockRef::AlgoOp(op), id)),
                "{} slot {i}",
                def.name
            );
        }
    }
}

/// The host's `factory-sounds fmt` and `about` read a Sound param through
/// `view::shown_fmt`: every page cell showing one reads in that format,
/// and a cell that shows text instead (a type-B ENV's SPEED) has none, so
/// the host writes no display the page doesn't draw.
#[test]
fn every_cell_reads_as_shown_fmt() {
    use chimera_core::addr::BlockRead;
    use chimera_core::dsp::filter::FilterKind;
    use chimera_core::dsp::modulator::{EnvForm, EnvSpeed, Func, LfoForm};
    use chimera_core::preset::Sound;
    use chimera_core::ui::view::{EnvKind, LfoKind, View, shown_fmt};
    let neutral = Sound::neutral(EngineType::Algo);
    let sound_param = |a: ParamAddr| neutral.params.block(a.block).is_some() && a.spec().is_some();
    let mut envs: Vec<EnvKind> = EnvSpeed::ALL.into_iter().map(EnvKind::A).collect();
    for f in EnvForm::ALL {
        envs.extend([EnvKind::B(Func::Env(f)), EnvKind::B(Func::Burst(f))]);
    }
    envs.extend(LfoForm::ALL.map(|f| EnvKind::B(Func::Lfo(f))));
    let lfos: Vec<LfoKind> = core::iter::once(LfoKind::Classic)
        .chain(LfoForm::ALL.map(LfoKind::Func))
        .collect();
    let base = SlotCtx::read(&neutral.params, Op::A);
    let mut bad = Vec::new();
    let (mut params, mut texts) = (0, 0);
    for (&e, &l, &kind) in envs
        .iter()
        .flat_map(|e| lfos.iter().map(move |l| (e, l)))
        .flat_map(|(e, l)| FilterKind::BUILT.iter().map(move |k| (e, l, k)))
    {
        let ctx = SlotCtx {
            envs: [e; 3],
            lfos: [l; 3],
            kind,
            ..base
        };
        for chain in reg::ALL_CHAINS {
            for block in chain.blocks {
                for def in core::iter::once(block.def).chain(block.sub_pages.iter().copied()) {
                    for i in 0..def.params.len() {
                        let (addr, fmt) = match view(def, i, &ctx) {
                            View::Param { addr, fmt, .. } => (addr, Some(fmt)),
                            View::Text { .. } => match def.params[i].binding {
                                SlotBinding::Param(a) => (a, None),
                                _ => continue,
                            },
                            _ => continue,
                        };
                        if !sound_param(addr) {
                            continue;
                        }
                        if fmt.is_some() {
                            params += 1;
                        } else {
                            texts += 1;
                        }
                        if shown_fmt(addr, &ctx) != fmt {
                            bad.push(format!(
                                "{} slot {i} {addr:?} under {e:?} {l:?} {kind:?}: page {fmt:?}, host {:?}",
                                def.name,
                                shown_fmt(addr, &ctx)
                            ));
                        }
                    }
                }
            }
        }
    }
    bad.dedup();
    assert!(
        params > 0 && texts > 0,
        "a value cell and a TYPE B cell are checked"
    );
    assert!(
        bad.is_empty(),
        "{} cells differ:\n{}",
        bad.len(),
        bad[..bad.len().min(20)].join("\n")
    );
}
