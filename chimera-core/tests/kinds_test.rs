//! Operator kinds and the formant budget (formant-operators plan, Tasks 3
//! and 4).

mod common;
mod screen;

use chimera_core::addr::{BlockRef, Blocks, Op};

use chimera_core::budget::{
    FORMANT_BUDGET, FORMANT_BUDGET_FRMT, FORMANT_N, FORMANT_N_UNLINKED, HEAVY_OVERHEAD,
};
use chimera_core::dsp::algo::algorithms::{ALGO_COUNT, ALGORITHMS, AlgoId};
use chimera_core::dsp::algo::engine::AlgoEngine;
use chimera_core::dsp::algo::kinds::{
    Kinds, Linked, OP_KIND_IDENTS, OpKind, OverBudget, fits, step_kind,
};
use chimera_core::dsp::algo::params::{AlgoOpParams, AlgoParams, step_alg};
use chimera_core::dsp::algo::plan::OPS;
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::dsp::filter::{CLEAN_MOVE_COST, FILTER_LEVEL_COST};
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::voice::{HighNote, Voice};
use chimera_core::hw::{CPU_HZ_REV_V, FLOOR_VOICES, FLOOR_VOICES_FORMANT, SampleBudget};
use chimera_core::modulation::ModRouting;
use chimera_core::params::EngineType;
use chimera_core::part::PartParams;
use chimera_core::preset::Sound;
use chimera_core::project::{Line, PartId};
use chimera_core::storage::{
    Decode, FileError, Framer, MIGRATIONS, RecordTag, SoundDecoder, TRANSLATIONS, decode_block,
};
use chimera_core::ui::UiState;
use chimera_core::ui::block_registry::ALGO_OP_PAGES;
use chimera_core::ui::busy::ToastStep;
use chimera_core::ui::held;
use chimera_core::ui::nav::PartChain;
use chimera_hal::EncoderId;

#[test]
fn op_kind_codes_are_frozen() {
    assert_eq!(OpKind::from_code(15), Some(OpKind::Table(WaveId::SSAW)));
    assert_eq!(OpKind::from_code(16), Some(OpKind::Formant));
    assert_eq!(OpKind::from_code(17), Some(OpKind::Noise));
    assert_eq!(OpKind::from_code(18), None);
    assert_eq!(OpKind::clamped(200), OpKind::Noise);
    assert_eq!(&OP_KIND_IDENTS[16..], &["FRMT", "NOIS"]);
    for c in 0..18 {
        assert_eq!(OpKind::from_code(c).unwrap().code(), c);
    }
}

#[test]
fn the_budget_is_derived_from_the_cost_model() {
    let bus = SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost().0
        - FxBus::COST_TAPE.0
        - chimera_core::budget::CONTROL_RESERVE.0;
    let table = AlgoEngine::COST_BASE.0
        + 12 * AlgoEngine::COST_LINK.0
        + 6 * (AlgoEngine::COST_OP.0 + AlgoEngine::COST_FEEDBACK.0);
    let heavy = ModRouting::HEAVY.0 + CLEAN_MOVE_COST.0 + FILTER_LEVEL_COST.0 + Voice::CHAIN_COST.0;
    assert_eq!(HEAVY_OVERHEAD.0, heavy);
    assert_eq!(FORMANT_BUDGET.0, bus / FLOOR_VOICES - heavy - table);
    assert_eq!(
        FORMANT_BUDGET_FRMT.0,
        (bus - HighNote::PART_COST.0) / FLOOR_VOICES_FORMANT
            - heavy
            - table
            - HighNote::VOICE_COST.0
    );
    let (linked, unlinked) = (OpKind::Formant.charge(true), OpKind::Formant.charge(false));
    assert_eq!(
        linked,
        AlgoEngine::COST_FRMT_LINKED.0 + AlgoEngine::COST_FRMT_MOVE_LINKED.0
            - AlgoEngine::COST_OP.0
    );
    assert_eq!(
        unlinked,
        AlgoEngine::COST_FRMT.0 + AlgoEngine::COST_FRMT_MOVE.0 - AlgoEngine::COST_OP.0
    );
    assert_eq!(
        OpKind::Noise.charge(true),
        AlgoEngine::COST_NOIS_LINKED.0 - AlgoEngine::COST_OP.0
    );
    assert_eq!(
        OpKind::Noise.charge(false),
        AlgoEngine::COST_NOIS.0 - AlgoEngine::COST_OP.0
    );
    assert_eq!(FORMANT_N, FORMANT_BUDGET_FRMT.0 / linked);
    assert_eq!(FORMANT_N_UNLINKED, FORMANT_BUDGET_FRMT.0 / unlinked);
    println!(
        "bus {bus} heavy {heavy} table {table} | budget4 {} budget3 {} | charge linked {} unlinked {} nois {} | N linked {FORMANT_N} unlinked {FORMANT_N_UNLINKED}",
        FORMANT_BUDGET.0,
        FORMANT_BUDGET_FRMT.0,
        linked,
        unlinked,
        OpKind::Noise.charge(false)
    );
    println!("nois linked {}", OpKind::Noise.charge(true));
}

/// `n` FRMT on operators 1–`n`, the rest W1.
fn frmts(n: usize) -> [OpKind; OPS] {
    core::array::from_fn(|i| {
        if i < n {
            OpKind::Formant
        } else {
            OpKind::Table(WaveId::W1)
        }
    })
}

#[test]
fn the_budget_rule_is_exact() {
    use OpKind::*;
    // A1: six carriers, no links.
    let none = Linked::of(AlgoId::A1, AlgoId::A1);
    assert_eq!(none, Linked::NONE);
    let n = FORMANT_N_UNLINKED as usize;
    assert!(fits(&frmts(n), none));
    assert!(!fits(&frmts(n + 1), none));
    // A11: operator 1 is modulated (4 → 1), so a FRMT there bills linked.
    let a11 = Linked::of(AlgoId::A11, AlgoId::A11);
    assert!(a11.has(0) && a11.has(1) && a11.has(2) && a11.has(3));
    assert!(!a11.has(4) && !a11.has(5));
    let n = FORMANT_N as usize;
    assert!(fits(&frmts(n), a11));
    assert!(!fits(&frmts(n + 1), a11));
    // Noise bands alone keep the four-voice budget, each billed linked
    // where a link runs into it: six on A1 fit; A11 links four of them.
    for l in [none, a11] {
        let spend: u32 = (0..OPS).map(|i| Noise.charge(l.has(i))).sum();
        assert_eq!(fits(&[Noise; OPS], l), spend <= FORMANT_BUDGET.0);
    }
    assert!(fits(&[Noise; OPS], none));
}

/// The operators each algorithm links into, from its table; operator 6
/// is never modulated (only higher operators modulate lower ones).
#[test]
fn linked_is_the_union_of_both_algorithms_links() {
    for a in 0..ALGO_COUNT as u8 {
        for b in 0..ALGO_COUNT as u8 {
            let (x, y) = (AlgoId::clamped(a), AlgoId::clamped(b));
            let l = Linked::of(x, y);
            for op in 0..OPS {
                let into = |id: AlgoId| {
                    ALGORITHMS[id.get() as usize]
                        .mods
                        .iter()
                        .any(|m| m & 1 << op != 0)
                };
                assert_eq!(l.has(op), into(x) || into(y), "{a} {b} op {op}");
            }
            assert!(!l.has(5));
            assert_eq!(l, Linked::of(y, x));
        }
    }
}

#[test]
fn unlinked_formant_carriers_fit_three_at_three_voices() {
    assert_eq!(FORMANT_N_UNLINKED, 3);
    let mut p = AlgoParams::default();
    p.try_set_algs(AlgoId::A1, AlgoId::A1).unwrap();
    for i in 0..3 {
        p.try_set_kind(i, OpKind::Formant).unwrap();
    }
    assert_eq!(p.try_set_kind(3, OpKind::Formant), Err(OverBudget));
}

#[test]
fn one_linked_formant_fits_and_two_do_not() {
    assert_eq!(FORMANT_N, 1);
    // T1: 4 → 3 → 2 → 1 and 6 → 5; operators 4 and 6 have no link in.
    let mut p = AlgoParams::default();
    p.try_set_algs(AlgoId::T1, AlgoId::T1).unwrap();
    p.try_set_kind(0, OpKind::Formant).unwrap();
    assert_eq!(p.try_set_kind(1, OpKind::Formant), Err(OverBudget));
    // Linked and unlinked share the budget: 1 linked + 1 unlinked fits
    // (590), 1 linked + 2 unlinked doesn't (790 > 659).
    p.try_set_kind(3, OpKind::Formant).unwrap();
    assert_eq!(p.try_set_kind(5, OpKind::Formant), Err(OverBudget));
    assert!(OpKind::Formant.charge(true) + OpKind::Formant.charge(false) <= FORMANT_BUDGET_FRMT.0);
    assert!(
        OpKind::Formant.charge(true) + 2 * OpKind::Formant.charge(false) > FORMANT_BUDGET_FRMT.0
    );
}

/// MORPH runs ALG A's and B's links at once: an operator linked in either
/// bills linked, whatever the MORPH, so ALG B can't add a link to a full
/// patch that ALG A leaves unlinked.
#[test]
fn morph_bills_each_operator_at_the_heavier_algorithm() {
    let mut p = AlgoParams::default();
    p.try_set_algs(AlgoId::A1, AlgoId::A1).unwrap();
    for i in 0..3 {
        p.try_set_kind(i, OpKind::Formant).unwrap();
    }
    p.morph = 0;
    // A9 links 6 → 5 → 4 → 1: operator 1 becomes linked.
    assert_eq!(p.try_set_algs(AlgoId::A1, AlgoId::A9), Err(OverBudget));
    assert_eq!(p.try_set_algs(AlgoId::A9, AlgoId::A1), Err(OverBudget));
    assert_eq!(p.algs(), (AlgoId::A1, AlgoId::A1));
    // With one FRMT the pair fits, and it costs what B alone would.
    // Operator 1 alone sounding: A9's one link into it, 4 → 1.
    let mut q = AlgoParams::single(WaveId::W1);
    q.try_set_algs(AlgoId::A1, AlgoId::A1).unwrap();
    q.try_set_kind(0, OpKind::Formant).unwrap();
    q.try_set_algs(AlgoId::A1, AlgoId::A9).unwrap();
    let all = [false; OPS];
    let ab = AlgoEngine::cost(&q, &all).0;
    q.try_set_algs(AlgoId::A9, AlgoId::A9).unwrap();
    assert_eq!(ab, AlgoEngine::cost(&q, &all).0);
    q.try_set_algs(AlgoId::A1, AlgoId::A1).unwrap();
    assert_eq!(
        ab - AlgoEngine::cost(&q, &all).0,
        AlgoEngine::frmt_cost(1, true) - AlgoEngine::frmt_cost(1, false) + AlgoEngine::COST_LINK.0
    );
}

/// FDBK isn't a link: it moves only the carrier's phase, never the grain
/// delay (`pm_cycles` is the links' PM alone), so a FRMT with FDBK and no
/// link runs the unlinked spans and bills unlinked plus `COST_FEEDBACK`.
#[test]
fn formant_feedback_is_not_a_link() {
    let mut p = AlgoParams::default();
    p.try_set_algs(AlgoId::A1, AlgoId::A1).unwrap();
    p.ops[0].level = 99;
    p.try_set_kind(0, OpKind::Formant).unwrap();
    let all = [false; OPS];
    let dry = AlgoEngine::cost(&p, &all).0;
    p.ops[0].feedback = 7;
    assert_eq!(
        AlgoEngine::cost(&p, &all).0,
        dry + AlgoEngine::COST_FEEDBACK.0
    );
    for i in 1..FORMANT_N_UNLINKED as usize {
        p.try_set_kind(i, OpKind::Formant).unwrap();
        p.ops[i].feedback = 7;
    }
    assert!(fits(&p.kinds().all(), p.linked()));
}

// ── Kinds: every write path checks the budget (Task 4) ─────────────────

fn fits_all(p: &AlgoParams) -> bool {
    fits(&p.kinds().all(), p.linked())
}

/// A UiState on Part 1's OP pages.
struct TestUi(Box<UiState>);

fn test_ui_with(s: &Sound) -> TestUi {
    let mut ui = Box::new(crate::screen::stack_ui());
    screen::init_part(&mut ui, PartId::ALL[0]);
    screen::to_osc(&mut ui);
    let mut t = TestUi(ui);
    t.load(s);
    t
}

impl TestUi {
    fn load(&mut self, s: &Sound) {
        let was = PartChain::of(self.sound());
        **self.0.project_mut().edit_part(PartId::ALL[0]).sound = s.clone();
        self.0.replaced_for_test(was);
    }

    /// ALG A (`side` 0) or B (1) turned `delta` on Part 1's ALG page; the
    /// toast it raised.
    fn turn_alg(&mut self, side: usize, delta: i8) -> ToastStep {
        let alg = chimera_core::ui::block_registry::ALGO_ALG.id;
        screen::to_osc(&mut self.0);
        for _ in 0..40 {
            if self.0.page_def().id == alg {
                break;
            }
            screen::feed(
                &mut self.0,
                screen::Input::press(chimera_hal::ButtonId::Minus),
            );
        }
        assert_eq!(self.0.page_def().id, alg);
        self.0.step_toast(60_000);
        let e = [EncoderId::A, EncoderId::B][side];
        screen::feed(&mut self.0, screen::Input::turn(e, delta));
        self.0.step_toast(0)
    }

    /// OP`n`'s WAVE cell (its first) turned `delta`; the toast it raised.
    fn turn_wave(&mut self, op: Op, delta: i8) -> ToastStep {
        screen::to_osc_sub(&mut self.0, &ALGO_OP_PAGES[op.index()]);
        self.0.step_toast(60_000);
        screen::feed(&mut self.0, screen::Input::turn(EncoderId::A, delta));
        self.0.step_toast(0)
    }

    /// Encoder A turned on the page it is on.
    fn again(&mut self, delta: i8) {
        screen::feed(&mut self.0, screen::Input::turn(EncoderId::A, delta));
    }

    /// `turn_wave` with MIX held: the snap path.
    fn snap_wave(&mut self, op: Op, delta: i8) -> ToastStep {
        screen::to_osc_sub(&mut self.0, &ALGO_OP_PAGES[op.index()]);
        self.0.step_toast(60_000);
        screen::feed(
            &mut self.0,
            screen::Input::turn(EncoderId::A, delta).and_held(chimera_hal::ButtonId::Mix),
        );
        self.0.step_toast(0)
    }

    fn sound(&self) -> &Sound {
        &self.0.project().part(PartId::ALL[0]).sound
    }

    /// The focused cell's refused choice and its reason, as the band shows
    /// them.
    fn refusal(&self) -> Option<(u8, String)> {
        let s = self.0.held_choice()?;
        Some((s.held.code, held::reason(s.why).as_str().to_string()))
    }
}

/// A one-entry OP_x record (WAVE = `code`) through `decode_block`.
fn decode_one(p: &mut chimera_core::params::ParamSnapshot, op: Op, code: u8) {
    let mut payload = vec![3 + op.index() as u8, AlgoOpParams::WAVE.0];
    payload.extend_from_slice(&u32::from(code).to_le_bytes());
    decode_block(
        &payload,
        MIGRATIONS,
        TRANSLATIONS,
        Some(p as &mut dyn Blocks),
    )
    .unwrap();
}

/// `sweep::Bench::set`'s path: the Part's `block_mut`, then `Block::set`.
fn sweep_set(bench: &mut common::sweep::Bench, s: &mut Sound, op: Op, code: u8) {
    bench.edit_sound(|x| *x = s.clone());
    let p = common::sweep::Param {
        block: BlockRef::AlgoOp(op),
        spec: &chimera_core::dsp::algo::params::ALGO_OP_SPECS[0],
    };
    bench.set(&p, f32::from(code));
    *s = (**bench.perf.edit(PartId::ALL[0]).sound).clone();
}

/// Hand-built bytes: an Algo Sound whose operators' WAVE codes are `codes`.
fn sound_file_with_kinds(codes: &[u8; OPS]) -> Vec<u8> {
    use chimera_core::block::DiskCode;
    let mut records = vec![(RecordTag::Engine, vec![EngineType::Algo.disk_code()])];
    for (i, &c) in codes.iter().enumerate() {
        let mut p = vec![3 + i as u8, AlgoOpParams::WAVE.0];
        p.extend_from_slice(&u32::from(c).to_le_bytes());
        records.push((RecordTag::Block, p));
    }
    common::codec_util::sound_file(&records)
}

/// Two passes, as a load runs them; whether the file was over the budget.
fn decode_sound(bytes: &[u8]) -> Result<(Sound, bool), FileError> {
    let mut s = Sound::neutral(EngineType::Algo);
    common::codec_util::pass(&mut s, bytes, false)?;
    let mut d = SoundDecoder::new(&mut s);
    let mut f = Framer::new(bytes.len() as u32)?;
    f.push(bytes, &mut |e| d.apply(e))?;
    f.finish()?;
    d.commit()?;
    let over = d.over_budget();
    Ok((s, over))
}

#[test]
fn try_set_kind_refuses_overspend_and_changes_nothing() {
    // INIT's T1 ∪ A1 links into operators 1–3 and 5.
    let mut p = AlgoParams::default();
    for i in 0..FORMANT_N as usize {
        p.try_set_kind(i, OpKind::Formant).unwrap();
    }
    let before = p.kinds();
    assert_eq!(
        p.try_set_kind(FORMANT_N as usize, OpKind::Formant),
        Err(OverBudget)
    );
    assert_eq!(p.kinds(), before);
}

#[test]
fn every_write_path_checks_the_budget() {
    let mut rng = 0x1234_5678u32;
    let mut s = Sound::init(EngineType::Algo);
    let mut ui = test_ui_with(&s);
    let mut bench = common::sweep::Bench::new(
        common::sweep::Patch::AlgoInit,
        PartParams::default(),
        Default::default(),
    );
    let (mut toasts, mut changed, mut full) = (0, [0; 6], 0);
    for i in 0..10_000 {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        let before = s.params.algo.kinds();
        let op = Op::ALL[(rng % 6) as usize];
        let code = ((rng >> 8) % 18) as u8;
        let a = BlockRef::AlgoOp(op);
        match i % 6 {
            0 => s
                .params
                .block_mut(a)
                .unwrap()
                .set(AlgoOpParams::WAVE, code as f32),
            1 => {
                let _ = s
                    .params
                    .block_mut(a)
                    .unwrap()
                    .set_enum_code(AlgoOpParams::WAVE, code);
            }
            2 => {
                let d = if rng & 1 == 0 { 1 } else { -3 };
                s.params.block_mut(a).unwrap().nudge(AlgoOpParams::WAVE, d)
            }
            3 => decode_one(&mut s.params, op, code),
            4 => {
                ui.load(&s);
                ui.turn_wave(op, if rng & 1 == 0 { 2 } else { -1 });
                toasts += usize::from(ui.refusal().is_some());
                s = ui.sound().clone();
            }
            _ => sweep_set(&mut bench, &mut s, op, code),
        }
        assert!(fits_all(&s.params.algo), "step {i}");
        changed[i % 6] += usize::from(s.params.algo.kinds() != before);
        full += usize::from(
            !s.params
                .algo
                .kinds()
                .with(op.index(), OpKind::Formant, s.params.algo.linked())
                .is_ok(),
        );
    }
    assert!(
        changed.iter().all(|&n| n > 100),
        "every path writes: {changed:?}"
    );
    assert!(full > 100, "the budget was reached: {full}");
    assert!(toasts > 0, "the WAVE cell shows a refused kind dimmed");
}

/// Shift+turn on WAVE runs to the list's ends; an end the budget refuses
/// is shown dimmed, not written.
#[test]
fn shift_turn_on_wave_runs_to_the_ends() {
    let mut s = Sound::init(EngineType::Algo);
    let n = FORMANT_N as usize;
    for i in 0..n {
        s.params.algo.try_set_kind(i, OpKind::Formant).unwrap();
    }
    let mut ui = test_ui_with(&s);
    let op = Op::ALL[n];
    ui.snap_wave(op, 1);
    assert_eq!(ui.sound().params.algo.kinds().get(n), OpKind::Noise);
    assert!(fits_all(&ui.sound().params.algo));
    ui.snap_wave(op, -1);
    assert_eq!(
        ui.sound().params.algo.kinds().get(n),
        OpKind::Table(WaveId::W1)
    );
}

/// Turning WAVE onto a kind the budget refuses shows it dimmed with its
/// reason and writes nothing; the next turn goes on from it.
#[test]
fn the_wave_encoder_lands_on_refused_kinds_dimmed() {
    let mut p = AlgoParams::default();
    for i in 0..FORMANT_N as usize {
        p.try_set_kind(i, OpKind::Formant).unwrap();
    }
    let n = FORMANT_N as usize;
    p.try_set_kind(n, OpKind::Table(WaveId::SSAW)).unwrap();
    let l = p.linked();
    assert!(l.has(n), "INIT links into operator {}", n + 1);
    // SSAW (15) +1 is FRMT: refused, and +2 NOIS fits.
    let s = step_kind(p.kinds(), l, n, OpKind::Table(WaveId::SSAW), 1);
    assert_eq!((s.to, s.refused), (OpKind::Formant, true));
    let s = step_kind(p.kinds(), l, n, OpKind::Formant, 1);
    assert_eq!((s.to, s.refused), (OpKind::Noise, false));
    // The list's ends clamp.
    let s = step_kind(p.kinds(), l, n, OpKind::Noise, 1);
    assert_eq!((s.to, s.refused), (OpKind::Noise, false));
    let s = step_kind(p.kinds(), l, n, OpKind::Noise, -127);
    assert_eq!(s.to, OpKind::Table(WaveId::W1));
}

#[test]
fn a_modulated_operator_shows_frmt_dimmed_with_the_reason() {
    let mut s = Sound::init(EngineType::Algo);
    let n = FORMANT_N as usize;
    for i in 0..n {
        s.params.algo.try_set_kind(i, OpKind::Formant).unwrap();
    }
    // Op n+1 is modulated (INIT links into it); FRMT is two steps from W1
    // only by the list, so land on it from NOIS' neighbour, SSAW.
    s.params
        .algo
        .try_set_kind(n, OpKind::Table(WaveId::SSAW))
        .unwrap();
    let mut ui = test_ui_with(&s);
    let op = Op::ALL[n];
    ui.turn_wave(op, 1);
    let (code, why) = ui.refusal().expect("FRMT is shown dimmed");
    assert_eq!(code, 16);
    assert_eq!(why, format!("OP {} MODULATED · OVER BUDGET", n + 1));
    assert_eq!(
        ui.sound().params.algo.kinds().get(n),
        OpKind::Table(WaveId::SSAW),
        "the kind doesn't change"
    );
    // The next turn goes on from the dimmed kind: NOIS fits and lands.
    ui.again(1);
    assert_eq!(ui.refusal(), None);
    assert_eq!(ui.sound().params.algo.kinds().get(n), OpKind::Noise);
}

#[test]
fn an_unmodulated_operator_takes_frmt_normally() {
    let mut s = Sound::init(EngineType::Algo);
    s.params
        .algo
        .try_set_kind(5, OpKind::Table(WaveId::SSAW))
        .unwrap();
    let l = s.params.algo.linked();
    assert!(!l.has(5), "INIT leaves operator 6 unmodulated");
    let mut ui = test_ui_with(&s);
    ui.turn_wave(Op::ALL[5], 1);
    assert_eq!(ui.refusal(), None);
    assert_eq!(ui.sound().params.algo.kinds().get(5), OpKind::Formant);
}

#[test]
fn a_full_budget_says_so() {
    let mut s = Sound::init(EngineType::Algo);
    s.params.algo.try_set_algs(AlgoId::A1, AlgoId::A1).unwrap();
    for i in 0..FORMANT_N_UNLINKED as usize {
        s.params.algo.try_set_kind(i, OpKind::Formant).unwrap();
    }
    let n = FORMANT_N_UNLINKED as usize;
    s.params
        .algo
        .try_set_kind(n, OpKind::Table(WaveId::SSAW))
        .unwrap();
    let mut ui = test_ui_with(&s);
    ui.turn_wave(Op::ALL[n], 1);
    let (code, why) = ui.refusal().expect("FRMT is refused");
    assert_eq!(code, 16);
    assert_eq!(why, "BUDGET FULL · OTHER FORMANTS");
}

/// The reason is drawn in ALERT on the hint line, and only while held.
#[test]
fn the_band_draws_the_reason_while_the_kind_is_held() {
    use chimera_core::ui::{hint, theme};
    let mut s = Sound::init(EngineType::Algo);
    let n = FORMANT_N as usize;
    for i in 0..n {
        s.params.algo.try_set_kind(i, OpKind::Formant).unwrap();
    }
    s.params
        .algo
        .try_set_kind(n, OpKind::Table(WaveId::SSAW))
        .unwrap();
    let mut ui = test_ui_with(&s);
    let alert_on_line = |ui: &mut TestUi| {
        screen::settle(&mut ui.0);
        let mut fb = screen::Fb::new();
        ui.0.render_with_scope(
            &mut fb,
            &chimera_core::ui::perf::PerfStats::zero(),
            &screen::scope_fixture(),
        );
        (hint::FOCUS_Y - 9..=hint::FOCUS_Y)
            .flat_map(|y| (0..screen::W as i32).map(move |x| (x, y)))
            .filter(|&(x, y)| fb.at(x, y) == theme::ALERT)
            .count()
    };
    assert_eq!(alert_on_line(&mut ui), 0);
    ui.turn_wave(Op::ALL[n], 1);
    assert!(alert_on_line(&mut ui) > 50, "the reason is drawn");
    ui.again(1);
    assert_eq!(alert_on_line(&mut ui), 0, "NOIS lands and the line clears");
}

#[test]
fn the_reasons_fit_the_band() {
    use chimera_core::dsp::algo::kinds::Why;
    use chimera_core::ui::{draw, theme};
    for w in [Why::Modulated(5), Why::Full, Why::Links] {
        let t = held::reason(w);
        let px = draw::text_width(&theme::FONT_LABEL, t.as_str(), theme::LABEL_TRACKING);
        assert!(px <= theme::CONTENT_W, "{t:?} is {px} px");
    }
}

/// What a load keeps of `want` on `l`: each operator in order, refused
/// kinds left at the base's W1.
fn greedy(want: [OpKind; OPS], l: Linked) -> [OpKind; OPS] {
    let mut k = [OpKind::Table(WaveId::W1); OPS];
    for i in 0..OPS {
        let mut t = k;
        t[i] = want[i];
        if fits(&t, l) {
            k = t;
        }
    }
    k
}

#[test]
fn an_over_budget_file_decodes_its_excess_as_w1() {
    let file = sound_file_with_kinds(&[16; 6]);
    let (s, over) = decode_sound(&file).unwrap();
    let p = &s.params.algo;
    // The neutral base's T1 ∪ A1: operators 1 and 4 hold a FRMT (390 +
    // 200), the rest load W1.
    let k = p.kinds().all();
    assert_eq!(k, greedy([OpKind::Formant; OPS], p.linked()));
    assert_eq!(k.iter().filter(|&&x| x == OpKind::Formant).count(), 2);
    assert!(over);
    let (_, over) = decode_sound(&sound_file_with_kinds(&[17, 0, 1, 2, 3, 16])).unwrap();
    assert!(!over);
}

/// An ALG record (Sounds write it before their operators): the operators
/// that follow are checked against its links.
fn sound_file_with_algs(a: AlgoId, b: AlgoId, codes: &[u8; OPS], algs_last: bool) -> Vec<u8> {
    use chimera_core::block::DiskCode;
    let mut alg = vec![BlockRef::Algo.disk_code().unwrap()];
    for (id, v) in [(AlgoParams::ALG_A, a), (AlgoParams::ALG_B, b)] {
        alg.push(id.0);
        alg.extend_from_slice(&u32::from(v.get()).to_le_bytes());
    }
    let mut records = vec![(RecordTag::Engine, vec![EngineType::Algo.disk_code()])];
    if !algs_last {
        records.push((RecordTag::Block, alg.clone()));
    }
    for (i, &c) in codes.iter().enumerate() {
        let mut p = vec![3 + i as u8, AlgoOpParams::WAVE.0];
        p.extend_from_slice(&u32::from(c).to_le_bytes());
        records.push((RecordTag::Block, p));
    }
    if algs_last {
        records.push((RecordTag::Block, alg));
    }
    common::codec_util::sound_file(&records)
}

#[test]
fn a_file_s_algorithm_degrades_the_formants_it_cannot_hold() {
    // A1's carriers hold four FRMT; the file asks five: the fifth loads W1.
    let (s, over) = decode_sound(&sound_file_with_algs(
        AlgoId::A1,
        AlgoId::A1,
        &[16; 6],
        false,
    ))
    .unwrap();
    let p = &s.params.algo;
    assert_eq!(p.algs(), (AlgoId::A1, AlgoId::A1));
    assert_eq!(
        p.kinds().all(),
        greedy([OpKind::Formant; OPS], Linked::NONE)
    );
    assert!(over);
    // A11 links into operators 1–4: one FRMT there, then 5 and 6 unlinked
    // would be 390 + 400 = 790, over.
    let (s, over) = decode_sound(&sound_file_with_algs(
        AlgoId::A11,
        AlgoId::A11,
        &[16; 6],
        false,
    ))
    .unwrap();
    let p = &s.params.algo;
    assert_eq!(p.algs(), (AlgoId::A11, AlgoId::A11));
    assert_eq!(p.kinds().all(), greedy([OpKind::Formant; OPS], p.linked()));
    assert!(fits_all(p) && over);
    // Written after its operators (no Sound is), an algorithm that would
    // link into a full patch is refused and reported, never lost.
    // On the neutral base's T1 ∪ A1, FRMT on 1 (linked) and 4 (not) and a NOIS
    // fit (390 + 200 + 40 = 630); A11 would link 4 too (800).
    let (s, over) = decode_sound(&sound_file_with_algs(
        AlgoId::A11,
        AlgoId::A11,
        &[16, 0, 0, 16, 17, 0],
        true,
    ))
    .unwrap();
    let p = &s.params.algo;
    assert_eq!(
        p.algs(),
        Sound::neutral(EngineType::Algo).params.algo.algs()
    );
    assert_eq!(p.kinds().get(0), OpKind::Formant);
    assert_eq!(p.kinds().get(3), OpKind::Formant);
    assert!(fits_all(p) && over);
    // Within the budget: nothing to report.
    let (s, over) = decode_sound(&sound_file_with_algs(
        AlgoId::A1,
        AlgoId::T1,
        &[0, 0, 0, 16, 0, 16],
        false,
    ))
    .unwrap();
    assert!(!over);
    assert_eq!(s.params.algo.kinds().get(3), OpKind::Formant);
}

#[test]
fn the_alg_encoder_steps_over_refused_algorithms() {
    let mut p = AlgoParams::default();
    p.try_set_algs(AlgoId::A1, AlgoId::A1).unwrap();
    for i in 0..3 {
        p.try_set_kind(i, OpKind::Formant).unwrap();
    }
    // Past A1 every algorithm links into one of operators 1–3; T8, just
    // before it, links only 6 → 5.
    for side in [AlgoParams::ALG_A, AlgoParams::ALG_B] {
        let from = p.side(side);
        let s = step_alg(&p, side, from, 1);
        assert_eq!((s.to, s.refused), (AlgoId::A2, true));
        let s = step_alg(&p, side, from, -1);
        assert_eq!((s.to, s.refused), (AlgoId::T8, false));
    }
    // With one FRMT on operator 6 (never linked), every algorithm fits.
    let mut q = AlgoParams::default();
    q.try_set_kind(5, OpKind::Formant).unwrap();
    let s = step_alg(&q, AlgoParams::ALG_A, q.side(AlgoParams::ALG_A), 3);
    assert_eq!(
        (s.to.get(), s.refused),
        (q.side(AlgoParams::ALG_A).get() + 3, false)
    );
    // Operator 1 FRMT and operator 4 FRMT: T1 (1 linked, 4 not) fits; T3
    // (4 → 1, 3 → 2 → 1) links into 1 only: fits; A8 links 4, 5, 6 → 1:
    // fits. A11 links into 1 and 4: refused.
    let mut r = AlgoParams::default();
    r.try_set_algs(AlgoId::A1, AlgoId::A1).unwrap();
    r.try_set_kind(0, OpKind::Formant).unwrap();
    r.try_set_kind(3, OpKind::Formant).unwrap();
    assert!(r.try_set_algs(AlgoId::T1, AlgoId::A1).is_ok());
    assert_eq!(r.try_set_algs(AlgoId::A11, AlgoId::A1), Err(OverBudget));
    let at = r.algs();
    let mut b = r;
    use chimera_core::block::Block;
    b.nudge(AlgoParams::ALG_A, 1);
    assert_eq!(
        b.algs().0,
        step_alg(&r, AlgoParams::ALG_A, r.side(AlgoParams::ALG_A), 1).to
    );
    assert!(fits_all(&b));
    assert_eq!(at.0, AlgoId::T1);
}

/// An ALG turn that would add a link to a full patch is refused and the
/// cell toasts, as WAVE's does.
#[test]
fn an_alg_edit_adding_a_link_to_a_full_patch_is_refused() {
    let mut s = Sound::init(EngineType::Algo);
    s.params.algo.try_set_algs(AlgoId::A1, AlgoId::A1).unwrap();
    for i in 0..3 {
        s.params.algo.try_set_kind(i, OpKind::Formant).unwrap();
    }
    let mut ui = test_ui_with(&s);
    for side in 0..2 {
        ui.turn_alg(side, 1);
        let (code, why) = ui.refusal().expect("A2 is shown dimmed");
        assert_eq!(code, AlgoId::A2.get());
        assert_eq!(why, "LINKS A FORMANT · OVER BUDGET");
        assert_eq!(ui.sound().params.algo.algs(), (AlgoId::A1, AlgoId::A1));
        assert_eq!(ui.sound().params.algo.kinds(), s.params.algo.kinds());
    }
    // Room for the link: no toast, and the algorithm moves.
    let mut s2 = Sound::init(EngineType::Algo);
    s2.params.algo.try_set_algs(AlgoId::A1, AlgoId::A1).unwrap();
    s2.params.algo.try_set_kind(5, OpKind::Formant).unwrap();
    ui.load(&s2);
    ui.turn_alg(0, 1);
    assert_eq!(ui.refusal(), None);
    assert_eq!(ui.sound().params.algo.algs().0, AlgoId::A2);
}

/// Every ALG write path checks the budget, as WAVE's do.
#[test]
fn every_alg_write_path_checks_the_budget() {
    let mut rng = 0x9e37_79b9u32;
    let mut s = Sound::init(EngineType::Algo);
    let mut ui = test_ui_with(&s);
    let mut bench = common::sweep::Bench::new(
        common::sweep::Patch::AlgoInit,
        PartParams::default(),
        Default::default(),
    );
    let (mut toasts, mut changed, mut refused) = (0, [0; 6], 0);
    for i in 0..6_000 {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        // Kinds churn too, so the budget is reached under every algorithm.
        let op = (rng % 6) as usize;
        let kind = [
            OpKind::Formant,
            OpKind::Formant,
            OpKind::Noise,
            OpKind::Table(WaveId::W1),
        ][((rng >> 4) % 4) as usize];
        let _ = s.params.algo.try_set_kind(op, kind);
        let before = s.params.algo.algs();
        let side = [AlgoParams::ALG_A, AlgoParams::ALG_B][((rng >> 7) & 1) as usize];
        let code = ((rng >> 8) % ALGO_COUNT as u32) as u8;
        let a = BlockRef::Algo;
        let want = AlgoId::clamped(code);
        let l = if side == AlgoParams::ALG_A {
            Linked::of(want, before.1)
        } else {
            Linked::of(before.0, want)
        };
        refused += usize::from(!fits(&s.params.algo.kinds().all(), l));
        match i % 6 {
            0 => s.params.block_mut(a).unwrap().set(side, f32::from(code)),
            1 => {
                let _ = s.params.block_mut(a).unwrap().set_enum_code(side, code);
            }
            2 => {
                let d = if rng & 1 == 0 { 1 } else { -3 };
                s.params.block_mut(a).unwrap().nudge(side, d)
            }
            3 => {
                let mut payload = vec![BlockRef::Algo.disk_code().unwrap(), side.0];
                payload.extend_from_slice(&u32::from(code).to_le_bytes());
                decode_block(
                    &payload,
                    MIGRATIONS,
                    TRANSLATIONS,
                    Some(&mut s.params as &mut dyn Blocks),
                )
                .unwrap();
            }
            4 => {
                ui.load(&s);
                ui.turn_alg(
                    usize::from(side == AlgoParams::ALG_B),
                    if rng & 1 == 0 { 2 } else { -1 },
                );
                toasts += usize::from(ui.refusal().is_some());
                s = ui.sound().clone();
            }
            _ => {
                bench.edit_sound(|x| *x = s.clone());
                let p = common::sweep::Param {
                    block: BlockRef::Algo,
                    spec: &chimera_core::dsp::algo::params::ALGO_SPECS[side.0 as usize],
                };
                bench.set(&p, f32::from(code));
                s = (**bench.perf.edit(PartId::ALL[0]).sound).clone();
            }
        }
        assert!(fits_all(&s.params.algo), "step {i}");
        changed[i % 6] += usize::from(s.params.algo.algs() != before);
    }
    assert!(
        changed.iter().all(|&n| n > 100),
        "every path writes: {changed:?}"
    );
    assert!(refused > 100, "the budget refused: {refused}");
    assert!(toasts > 0, "the ALG cells show a refused algorithm dimmed");
}

#[test]
#[should_panic]
fn kinds_new_rejects_overspend() {
    let _ = Kinds::new([OpKind::Formant; 6]);
}

#[test]
fn the_wave_cell_reads_and_writes_the_kind() {
    let mut s = Sound::init(EngineType::Algo);
    {
        let mut b = s.params.block_mut(BlockRef::AlgoOp(Op::C)).unwrap();
        assert!(b.set_enum_code(AlgoOpParams::WAVE, 17));
        assert_eq!(b.get(AlgoOpParams::WAVE), 17.0);
        assert_eq!(b.enum_ident(AlgoOpParams::WAVE), Some("NOIS"));
        assert!(!b.set_enum_code(AlgoOpParams::WAVE, 18));
    }
    assert_eq!(s.params.algo.kinds().get(2), OpKind::Noise);
    assert_eq!(chimera_core::dsp::algo::kinds::OP_KIND_NAMES[17], "NOISE");
}

/// The budget's floor rises with a FRMT, so a patch legal whole can be
/// over it in operator order: the Sound decodes whole all the same.
#[test]
fn a_patch_that_fits_whole_round_trips_whatever_its_order() {
    use common::formant::{FRMT_LAST, frmt_last};
    let s = frmt_last();
    let l = s.params.algo.linked();
    assert!((0..OPS - 1).all(|i| l.has(i)) && !l.has(OPS - 1));
    let mut no_frmt = FRMT_LAST;
    no_frmt[OPS - 1] = OpKind::Table(WaveId::W1);
    assert!(fits(&FRMT_LAST, l) && !fits(&no_frmt, l));
    let (d, over) = decode_sound(&common::codec_util::encode(&s)).unwrap();
    assert!(!over, "nothing refused");
    assert_eq!(d.params.algo.kinds().all(), FRMT_LAST);
    assert!(d.bits_eq(&s));
}

/// Op 6's FRMT moves to every kind the patch fits with whole, and to none
/// that would leave it over its floor.
#[test]
fn a_frmt_steps_to_every_kind_the_whole_patch_fits() {
    let s = common::formant::frmt_last();
    let p = &s.params.algo;
    let (k, l, op) = (p.kinds(), p.linked(), OPS - 1);
    for c in 0..=17u8 {
        let to = OpKind::from_code(c).unwrap();
        let mut whole = k.all();
        whole[op] = to;
        assert_eq!(k.with(op, to, l).is_ok(), fits(&whole, l), "{to:?}");
    }
    // NOIS on 1–5 alone is over the four-voice floor: op 6 stays FRMT.
    let st = step_kind(k, l, op, k.get(op), -1);
    assert_eq!((st.to, st.refused), (OpKind::Table(WaveId::SSAW), true));
    // With NOIS on 1–4, every kind fits whole and every step lands.
    let mut q = *p;
    q.try_set_kind(4, OpKind::Table(WaveId::W1)).unwrap();
    for d in -16..=1i8 {
        let st = step_kind(q.kinds(), l, op, q.kinds().get(op), d);
        assert_eq!(st.to.code() as i16, 16 + i16::from(d), "{d}");
        assert!(!st.refused);
    }
}
