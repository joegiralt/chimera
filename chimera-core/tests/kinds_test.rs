//! Operator kinds and the formant budget (formant-operators plan, Tasks 3
//! and 4).

mod common;
mod screen;

use chimera_core::addr::{BlockRef, Blocks, Op};

use chimera_core::budget::{FORMANT_BUDGET, FORMANT_N, HEAVY_OVERHEAD};
use chimera_core::dsp::algo::engine::AlgoEngine;
use chimera_core::dsp::algo::kinds::{Kinds, OP_KIND_IDENTS, OpKind, OverBudget, fits, step_kind};
use chimera_core::dsp::algo::params::{AlgoOpParams, AlgoParams};
use chimera_core::dsp::algo::plan::OPS;
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::dsp::filter::{CLEAN_MOVE_COST, FILTER_LEVEL_COST};
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::{CPU_HZ_REV_V, FLOOR_VOICES, SampleBudget};
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
    let share =
        (SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost().0 - FxBus::COST_TAPE.0) / FLOOR_VOICES;
    let table = AlgoEngine::COST_BASE.0
        + 12 * AlgoEngine::COST_LINK.0
        + 6 * (AlgoEngine::COST_OP.0 + AlgoEngine::COST_FEEDBACK.0);
    let heavy = ModRouting::HEAVY.0 + CLEAN_MOVE_COST.0 + FILTER_LEVEL_COST.0 + Voice::CHAIN_COST.0;
    assert_eq!(HEAVY_OVERHEAD.0, heavy);
    assert_eq!(FORMANT_BUDGET.0, share - heavy - table);
    assert_eq!(
        OpKind::Formant.charge(),
        AlgoEngine::COST_FRMT.0 - AlgoEngine::COST_OP.0
    );
    assert_eq!(FORMANT_N, FORMANT_BUDGET.0 / OpKind::Formant.charge());
}

#[test]
fn the_budget_rule_is_exact() {
    use OpKind::*;
    let t = Table(WaveId::W1);
    let n = FORMANT_N as usize;
    let mut k = [t; OPS];
    for slot in k.iter_mut().take(n) {
        *slot = Formant;
    }
    assert!(fits(&k));
    k[n] = Formant;
    assert!(!fits(&k));
    assert_eq!(
        fits(&[Noise; OPS]),
        6 * OpKind::Noise.charge() <= FORMANT_BUDGET.0
    );
}

// ── Kinds: every write path checks the budget (Task 4) ─────────────────

fn fits_all(p: &AlgoParams) -> bool {
    fits(&p.kinds().all())
}

/// A UiState on Part 1's OP pages.
struct TestUi(Box<UiState>);

fn test_ui_with(s: &Sound) -> TestUi {
    let mut ui = Box::new(UiState::new());
    screen::init_part(&mut ui, PartId::ALL[0]);
    screen::to_osc(&mut ui);
    let mut t = TestUi(ui);
    t.load(s);
    t
}

impl TestUi {
    fn load(&mut self, s: &Sound) {
        **self.0.project_mut().edit_part(PartId::ALL[0]).sound = s.clone();
    }

    /// OP`n`'s WAVE cell (its first) turned `delta`; the toast it raised.
    fn turn_wave(&mut self, op: Op, delta: i8) -> ToastStep {
        screen::to_osc_sub(&mut self.0, &ALGO_OP_PAGES[op.index()]);
        self.0.step_toast(60_000);
        screen::feed(&mut self.0, screen::Input::turn(EncoderId::A, delta));
        self.0.step_toast(0)
    }

    fn sound(&self) -> &Sound {
        &self.0.project().part(PartId::ALL[0]).sound
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
                let t = ui.turn_wave(op, if rng & 1 == 0 { 2 } else { -1 });
                toasts += usize::from(t == ToastStep::Show(Line::new("FORMANT BUDGET FULL")));
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
                .with(op.index(), OpKind::Formant)
                .is_ok(),
        );
    }
    assert!(
        changed.iter().all(|&n| n > 100),
        "every path writes: {changed:?}"
    );
    assert!(full > 100, "the budget was reached: {full}");
    assert!(toasts > 0, "the WAVE cell toasts a refused kind");
}

#[test]
fn the_wave_encoder_steps_over_refused_kinds() {
    let mut p = AlgoParams::default();
    for i in 0..FORMANT_N as usize {
        p.try_set_kind(i, OpKind::Formant).unwrap();
    }
    let n = FORMANT_N as usize;
    p.try_set_kind(n, OpKind::Table(WaveId::SSAW)).unwrap();
    let s = step_kind(p.kinds(), n, 1);
    assert_eq!(s.to, OpKind::Noise);
    assert!(s.skipped);
    p.try_set_kind(n, OpKind::Noise).unwrap();
    let s = step_kind(p.kinds(), n, -1);
    assert_eq!(s.to, OpKind::Table(WaveId::SSAW));
    assert!(s.skipped);
    let s = step_kind(p.kinds(), n, -2);
    assert_eq!(s.to, OpKind::Table(WaveId::RSQR));
    // The list's ends are no refusal.
    let s = step_kind(p.kinds(), n, 1);
    assert_eq!((s.to, s.skipped), (OpKind::Noise, false));
}

#[test]
fn an_over_budget_file_decodes_its_excess_as_w1() {
    let file = sound_file_with_kinds(&[16; 6]);
    let (s, over) = decode_sound(&file).unwrap();
    let k = s.params.algo.kinds().all();
    let n = FORMANT_N as usize;
    assert!(k[..n].iter().all(|&x| x == OpKind::Formant));
    assert!(k[n..].iter().all(|&x| x == OpKind::Table(WaveId::W1)));
    assert!(over);
    let (_, over) = decode_sound(&sound_file_with_kinds(&[17, 0, 1, 2, 3, 16])).unwrap();
    assert!(!over);
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
