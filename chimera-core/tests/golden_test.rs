//! Refactor lock (spec 2026-09-23 § Testing).
//!
//! Goldens freeze today's output, good or bad — they are not a quality claim.
//! They must match bit-for-bit after every refactor step. Re-record ONLY for
//! a change the spec lists as intended, in the task that makes it
//! (`common::golden`).

mod common;

use common::*;

use chimera_core::dsp::tape_engine::TapeEngineParams;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::{MidiNote, Velocity};

/// (case name, FNV-1a 64 over every sample's bits, sample bits at SPOT_IDX).
const GOLDENS: &[(&str, (u64, [u32; 8]))] = &[
    // Every row re-recorded at Task 17 for ADR 0060: the voice's DC blocker
    // (and on Modal the blocker on BANK and SYMP's zero-mean pluck); with
    // those four off, every row matched its value before. Each row's comment
    // names what came before that. The Algo rows, SQR BASS and the Algo
    // half of `algo_to_modal_switch` re-recorded for the filter's C1
    // `saturate` (ADR 0063): each moved only where a state passed 1. Then
    // every row for ADR 0063's INIT (OUT LEVEL 45/128, RR 5) and the
    // harness's 1.6 s release (`OFF_BLOCKS`).
    // Re-recorded: Modal 2 step A's resonators (spec § Tests), then ADR 0058's gain,
    // then Task 18's DAMP make-up and relative silence (ADR 0056),
    // then the free ring on release (ADR 0062).
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then STRING's out_gain 5.7 for it (ADR 0063's peak rule).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "modal_init",
        (
            0x5176d0ac65f9a3d1,
            [
                0, 1048126220, 1052218609, 3177234998, 1048314789, 1048569258, 1040495686,
                3182558310,
            ],
        ),
    ),
    // Re-recorded: Modal 2 step A's resonators (spec § Tests), then ADR 0058's gain,
    // then Task 18's DAMP make-up and relative silence (ADR 0056),
    // then the free ring on release (ADR 0062).
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then STRING's out_gain 5.7 for it (ADR 0063's peak rule).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "modal_lfo_cutoff",
        (
            0x102a7b03053e661d,
            [
                0, 1048126220, 1053102339, 3177236354, 1048314741, 1048569210, 1040458604,
                996184393,
            ],
        ),
    ),
    // Re-recorded: Modal 2 step A's resonators (spec § Tests), then ADR 0058's gain,
    // then Task 18's DAMP make-up and relative silence (ADR 0056),
    // then the free ring on release (ADR 0062), then SYMP's out clip, stmlib's
    // rational curve for `tanhf` (ADR 0097).
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "modal_sympathetic",
        (
            0xc40e2c3dd96cc9d7,
            [
                1012689062, 1038551816, 1036302816, 3198907799, 1055292148, 1054094316, 1051530843,
                3193926805,
            ],
        ),
    ),
    // Re-recorded, every Algo case: the output scale is the carrier power
    // (ADR 0049); INIT routes four operators. Factory Sounds compensate in
    // their data and match 937b89f to about 1e-7, but MORPH PAD (#192).
    // Recorded after the Algo sanity gate (ADR 0011).
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_init",
        (
            0xac18195fc3101ceb,
            [
                972060177, 3198287341, 1049035940, 1041192466, 3197134115, 3197551485, 3169230522,
                3155283520,
            ],
        ),
    ),
    // Re-recorded: CUTOFF routes in octaves, g ramped per block (filter-routing spec § 3).
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_lfo_cutoff",
        (
            0x1dab49a9800513aa,
            [
                972060177, 3198287341, 1049133884, 1041193425, 3197134037, 3197551406, 3168592718,
                1023771867,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_t1",
        (
            0x338914f3a4a9afb3,
            [
                973136596, 3199095550, 1029401004, 3168216480, 3188781221, 3166622429, 3185833548,
                3160620186,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_t2",
        (
            0x1d58e515e3735a87,
            [
                973545296, 1049607733, 3177018799, 3184382992, 3195443999, 3191636009, 3185821175,
                3161709992,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_t3",
        (
            0x99e521972604a034,
            [
                975175686, 1051148883, 3178282459, 3181992097, 3195881675, 3192897117, 3185609824,
                3163500002,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_t4",
        (
            0xd442c1935b585ad3,
            [
                973193445, 3198350446, 3193788259, 3172016707, 1048064525, 1044971578, 3184260242,
                3159716904,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_t5",
        (
            0x87063bb722952de9,
            [
                972913803, 1052282164, 1052238905, 3177438969, 3197288083, 3196992014, 3186424013,
                3161057541,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_t6",
        (
            0x6b5b7b394ee8e944,
            [
                973714044, 1050250631, 1052049530, 3174298559, 3197277840, 3196853927, 3180957860,
                3158113119,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_t7",
        (
            0xb08ecc3848fdbdb7,
            [
                970009862, 1038215128, 1052175815, 3162593418, 3197461555, 3197296821, 3182571825,
                3154648884,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_t8",
        (
            0xa1c30bf3e89efef8,
            [
                971589691, 1032249938, 1052339992, 3160236107, 3197701223, 3197614217, 3183235600,
                3152545449,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_morph_static",
        (
            0x39ed449b407c938c,
            [
                981272904, 3178920668, 1036455980, 3173319077, 3170829742, 3180559784, 3173539048,
                3147980193,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_morph_sweep",
        (
            0x39888b5354294798,
            [
                981272904, 3182365268, 1034303929, 3161722387, 1023475985, 1022702980, 3173462416,
                991728612,
            ],
        ),
    ),
    // Re-recorded: Modal 2 step A's resonators (spec § Tests), then ADR 0058's gain,
    // then Task 18's DAMP make-up and relative silence (ADR 0056),
    // then the free ring on release (ADR 0062).
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then STRING's out_gain 5.7 for it (ADR 0063's peak rule).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_to_modal_switch",
        (
            0xb8704edef6f53de2,
            [
                972060177, 3198287341, 1049035940, 3201774263, 992947158, 1024834130, 3148631123,
                3195747569,
            ],
        ),
    ),
    // Filter-routing spec § Migration: recorded before the change; re-recorded
    // only for ADR 0060's DC blocker.
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "factory_0",
        (
            0x2b0219df38902ede,
            [
                966633155, 3199652310, 3193953164, 1045452874, 3188637170, 3188420254, 0, 0,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "factory_1",
        (
            0x0e3d85b43ff9ccf9,
            [
                979074595, 1032020609, 1055785139, 3166978861, 3201981805, 3202000541, 3116682528,
                0,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "factory_2",
        (
            0xfe00848f39d1981e,
            [
                919634797, 1043285534, 982936894, 3161177423, 3140431246, 1007666467, 3158423231,
                3100585116,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "factory_3",
        (
            0x901698e789a3ccf9,
            [
                986599185, 1055273736, 3186656317, 1056926714, 1025655748, 3147485307, 3192124968,
                1021142159,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "factory_4",
        (
            0x68328532bf9779a7,
            [
                908219966, 1055265999, 1049507872, 1030503929, 3191651237, 3191270130, 3113279447,
                0,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    // Then the voice-end fade (ADR 0096): its filter tail, −65 dB, fades
    // over the last block instead of cutting.
    // Re-recorded in Task 14: DRV is SOFT, voiced at LEVEL 0.7626 to cancel
    // its `MAKE_UP` 0.483 (the factory Sounds keep their loudness).
    (
        "factory_5",
        (
            0xa778b804866845e5,
            [
                898827728, 3209682936, 3209857559, 1059996833, 3206496509, 3206489051, 0, 0,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "factory_6",
        (
            0x144e8d10ce6ac51e,
            [
                834352396, 1009650854, 1036674498, 968914263, 3182119491, 3181374484, 3177329682,
                993036649,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    // Then the voice-end fade past −80 dB (#487): its DC blocker's tail,
    // −69 dB, ramps out over the last block instead of cutting.
    (
        "factory_7",
        (
            0x13448df1ea3c0499,
            [
                974638215, 1046941265, 1045727262, 999705034, 1033284198, 1032940907, 3133150842, 0,
            ],
        ),
    ),
    // The formant demo Sounds (spec § Demo clips): CHOIR, TALK BASS, BREATH
    // and VOX.
    (
        "factory_8",
        (
            0xb7c2d51dd428a7ff,
            [
                809965482, 3130400255, 1016199384, 3141166119, 3193279306, 3194216468, 1011502379,
                992344631,
            ],
        ),
    ),
    // Re-recorded in Task 14: DRV is SOFT, voiced at LEVEL 0.7626 to cancel
    // its `MAKE_UP` 0.483 (the factory Sounds keep their loudness).
    (
        "factory_9",
        (
            0x7b15bf77e7717f9e,
            [
                886068374, 3196599706, 995173522, 3194527354, 3191504998, 3179667062, 3126567846, 0,
            ],
        ),
    ),
    (
        "factory_10",
        (
            0x5404971f6c4c2a98,
            [
                837617169, 1017595293, 3179822900, 1032463702, 1044536702, 1044567220, 999749922,
                959177971,
            ],
        ),
    ),
    // Re-recorded in Task 14: DRV is SOFT, voiced at LEVEL 0.7626 to cancel
    // its `MAKE_UP` 0.483 (the factory Sounds keep their loudness).
    (
        "factory_11",
        (
            0xdf3d7bdae606ed60,
            [
                944197035, 1056502661, 1053648234, 1031350218, 3173886431, 3173428979, 0, 0,
            ],
        ),
    ),
    // Recorded at VA Task 10: INIT at VA_OUT_GAIN 3.631 (ADR 0109).
    (
        "va_init",
        (
            0xc22f6c59684767d8,
            [
                0, 1035274050, 1057410685, 3194645188, 3211538507, 3211140287, 0, 0,
            ],
        ),
    ),
];

/// TAPE through one voice on INIT (ENV 1 into VCA), each model and LOOP
/// with AGE and TAPE at 0.5 (`tape_render`). Recorded at TAPE Task 12; the
/// machines' rows appended at Task 12b, the rest unchanged; re-recorded for
/// `out_gain` (Task 19), then the smoothstep note-on ramp and LOOP's
/// smoothstep fades (#385), then CLEAN's ACCURATE tan, exact divides, k law
/// and trims (#57, ADR 0096) on main's merge.
const TAPE_GOLDENS: &[(&str, (u64, [u32; 8]))] = &[
    (
        "tape_loop",
        (
            0x10bf5c97f6b9701c,
            [
                0, 3194043769, 3167776144, 1051385461, 3188220587, 3188087319, 3197822067,
                1032249790,
            ],
        ),
    ),
    (
        "tape_reverse",
        (
            0x2337937b073fd48e,
            [
                0, 1045879065, 1017140382, 3199036123, 1040238763, 1040038395, 1050120794,
                3189053675,
            ],
        ),
    ),
    (
        "tape_pingpong",
        (
            0xe90418ab471afaec,
            [
                0, 3194043769, 3167776144, 1051385461, 3188220587, 3188087319, 3197822067,
                1032249790,
            ],
        ),
    ),
    (
        "tape_transwave",
        (
            0xbe0ffa82b93e1071,
            [
                0, 3201375958, 3191849488, 1050983835, 3203617097, 3203933371, 1046438110,
                3199102357,
            ],
        ),
    ),
    (
        "tape_scrub",
        (
            0x10bf5c97f6b9701c,
            [
                0, 3194043769, 3167776144, 1051385461, 3188220587, 3188087319, 3197822067,
                1032249790,
            ],
        ),
    ),
    (
        "tape_stretch",
        (
            0x3dedea3977e0a8dd,
            [
                0, 3193362778, 3163524242, 3185853581, 1035472109, 1035708095, 1024605169,
                3164996604,
            ],
        ),
    ),
    (
        "tape_loop_age",
        (
            0x5a6e4d4b9921ab3c,
            [
                3070985076, 3194124313, 3167105100, 1051448124, 3188507217, 3188200258, 3189962063,
                1036580912,
            ],
        ),
    ),
    (
        "tape_loop_tape",
        (
            0x6a09d793db3a854e,
            [
                0, 3193926882, 3159894369, 1052712166, 3177682647, 3177190977, 3196410297,
                1051157288,
            ],
        ),
    ),
    (
        "tape_oneshot",
        (
            0xd204e8cc8e9c4c87,
            [
                0, 3194043769, 3167776144, 1051385461, 3188220587, 3188087319, 3197822067,
                1032249790,
            ],
        ),
    ),
    // Re-recorded for the voice-end fade past −80 dB (#487): the slice
    // ended on a −39 dB step, now ramped out over the last block.
    (
        "tape_oneshot_slice7",
        (
            0x66b4834f036719a1,
            [0, 1050811635, 1053320054, 0, 0, 0, 0, 0],
        ),
    ),
    // Re-recorded for the voice-end fade past −80 dB (#487): the tape ran
    // out on a −39 dB step, now ramped out over the last block.
    (
        "tape_tron_held",
        (
            0xe90f31aa5cb2475e,
            [
                0, 3194043769, 3167776144, 1051385461, 3188220587, 3188087319, 3197822067,
                1032249790,
            ],
        ),
    ),
    (
        "tape_tron_released",
        (
            0xcdf279ecda19efe4,
            [
                0, 3194043769, 3167776144, 1051385461, 3188220587, 3188087319, 3197822067,
                1032249790,
            ],
        ),
    ),
    (
        "tape_stretch_reverse",
        (
            0x45ceaedf606095ea,
            [
                0, 1045191811, 1010431416, 1037391874, 3183836458, 3184088462, 3173983647,
                1012150857,
            ],
        ),
    ),
    (
        "tape_loop_warble",
        (
            0x1e951224f88514b1,
            [
                0, 3194037626, 3166929527, 1051513146, 3188467278, 3188333103, 1051724949,
                3192587643,
            ],
        ),
    ),
];

/// Goldens whose locked output no longer reflects intended behaviour, each
/// with its issue. Empty: Modal 2 step A closed #10, and Modal's four
/// goldens were re-recorded then; `known_broken` guards whatever comes next.
const KNOWN_BROKEN: &[(&str, &str)] = &[];

#[test]
fn goldens_match() {
    let got: Vec<_> = Case::ALL
        .iter()
        .map(|&case| {
            let out = render_case(case);
            (case.name(), (fnv1a(&out), spots(&out)))
        })
        .collect();
    golden::check(GOLDENS, &got);
}

/// Spec § Testing: part 1's mono bus through the voice pool matches every
/// golden bit-for-bit.
#[test]
fn goldens_match_through_the_instrument() {
    let got: Vec<_> = Case::ALL
        .iter()
        .map(|&case| {
            let out = render_case_through_instrument(case);
            (case.name(), (fnv1a(&out), spots(&out)))
        })
        .collect();
    // Compares even under GOLDEN_RECORD; `goldens_match` alone records.
    let failures = golden::mismatches(GOLDENS, &got);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn known_broken_goldens_have_issues() {
    let cases: Vec<&str> = Case::ALL.iter().map(|c| c.name()).collect();
    golden::known_broken(KNOWN_BROKEN, &cases);
}

#[test]
fn harness_is_deterministic() {
    for case in [Case::AlgoInit, Case::ModalInit] {
        assert_eq!(
            fnv1a(&render_case(case)),
            fnv1a(&render_case(case)),
            "{}",
            case.name()
        );
    }
}

/// A route that silently does nothing would make its golden a copy of the
/// unmodulated one and lock nothing.
#[test]
fn modulated_cases_differ_from_unmodulated() {
    for (modulated, plain) in [
        (Case::ModalLfoCutoff, Case::ModalInit),
        (Case::AlgoLfoCutoff, Case::AlgoInit),
        (Case::AlgoMorphSweep, Case::AlgoMorphStatic),
    ] {
        assert_ne!(
            fnv1a(&render_case(modulated)),
            fnv1a(&render_case(plain)),
            "{} renders the same as {}",
            modulated.name(),
            plain.name()
        );
    }
}

/// Spec § Testing: a patch per T1–T8, each a different algorithm.
#[test]
fn the_eight_tx_algorithms_render_differently() {
    let hashes: Vec<u64> = (0..8)
        .map(|t| fnv1a(&render_case(Case::AlgoTx(t))))
        .collect();
    for (i, h) in hashes.iter().enumerate() {
        assert!(
            !hashes[..i].contains(h),
            "T{} renders like an earlier T",
            i + 1
        );
    }
}

/// Filter-routing spec § Migration: every factory Sound is locked.
#[test]
fn every_factory_sound_has_a_golden() {
    for i in 0..chimera_core::factory::FACTORY_LEN {
        let name = format!("factory_{i}");
        assert!(GOLDENS.iter().any(|g| g.0 == name), "{name}");
    }
}

/// The shared check fails a changed value, a row left behind by a renamed
/// or deleted case, and a case with no row (#108).
#[test]
fn golden_check_catches_every_disagreement() {
    let table = [("a", 1u64), ("b", 2), ("d", 4), ("a", 1)];
    let got = [("a", 1u64), ("c", 3), ("d", 5)];
    assert_eq!(
        golden::mismatches(&table, &got),
        [
            "c: no golden recorded (run with GOLDEN_RECORD=1)",
            "d: 0x0000000000000005 (want 0x0000000000000004)",
            "b: recorded, but no such case",
            "a: recorded twice",
        ]
    );
    assert!(golden::mismatches(&table[..3], &table[..3]).is_empty());
}

/// The TAPE cases: each model, then LOOP with AGE, then with TAPE, at 0.5;
/// then the machines (TAPE Task 12b). `tape_reverse` is Task 12's REVERSE,
/// now LOOP with END before START: its row is unchanged.
fn tape_cases() -> Vec<(&'static str, TapeEngineParams)> {
    use chimera_core::dsp::tape_engine::TapeModel as M;
    let p = TapeEngineParams::default();
    let model = |model| TapeEngineParams { model, ..p };
    let rev = |model| TapeEngineParams {
        model,
        start: 1.0,
        end: 0.0,
        ..p
    };
    vec![
        ("tape_loop", model(M::Loop)),
        ("tape_reverse", rev(M::Loop)),
        ("tape_pingpong", model(M::PingPong)),
        ("tape_transwave", model(M::Transwave)),
        // A moving tape: the neutral's MOTION 0 is a stopped, silent one.
        (
            "tape_scrub",
            TapeEngineParams {
                motion: 0.5,
                ..model(M::Scrub)
            },
        ),
        ("tape_stretch", model(M::Stretch)),
        ("tape_loop_age", TapeEngineParams { age: 0.5, ..p }),
        ("tape_loop_tape", TapeEngineParams { tape: 0.5, ..p }),
        ("tape_oneshot", model(M::Oneshot)),
        // SLICE 7: its value 8.
        (
            "tape_oneshot_slice7",
            TapeEngineParams {
                slice: chimera_core::dsp::tape_engine::SliceCode::new(8),
                ..model(M::Oneshot)
            },
        ),
        // Held: a 1.4 s tape runs out before the key up; released: an 8 s
        // one fades at it.
        (
            "tape_tron_held",
            TapeEngineParams {
                size: 0.5,
                ..model(M::Tron)
            },
        ),
        ("tape_tron_released", model(M::Tron)),
        ("tape_stretch_reverse", rev(M::Stretch)),
        (
            "tape_loop_warble",
            TapeEngineParams {
                tape: 0.5,
                wow: chimera_core::dsp::tape_engine::Wow::Warble,
                ..p
            },
        ),
    ]
}

/// `t` on INIT through one voice: on the saw with its burst at 24 and at
/// 8 kHz (TRANSWAVE: the saw-to-square transwave), C3, C4 and C5, each 2 s,
/// its last 0.5 s released.
fn tape_render(t: &TapeEngineParams) -> Vec<f32> {
    use chimera_core::dsp::tape_engine::TapeModel;
    use common::tape::{held_plain, saw_to_square, saw_with_burst};
    let mut s = Sound::init(EngineType::Tape);
    s.params.tape = *t;
    let sources = if t.model == TapeModel::Transwave {
        vec![saw_to_square()]
    } else {
        [24_000, 8_000]
            .map(|r| held_plain(&saw_with_burst(r, 1.0), r))
            .to_vec()
    };
    let mut out = Vec::new();
    let mut b = [0.0f32; chimera_hal::BLOCK_SIZE];
    for held in &sources {
        let mut v = Rig::with_source(SR, held);
        for n in [48, 60, 72] {
            v.note_on(
                MidiNote::new(n).unwrap(),
                Velocity::new(VEL).unwrap(),
                &s.params,
            );
            for i in 0..1_500 {
                if i == 1_125 {
                    v.note_off();
                }
                v.render(&mut b, &s.params, &s.mod_state);
                out.extend_from_slice(&b);
            }
        }
    }
    out
}

/// Task 12's REVERSE Sound (MODEL code 1, START and END at their base),
/// loaded through `translate_v1`: LOOP with END before START, its render
/// the recorded `tape_reverse` row, bit for bit (#369).
#[test]
fn reverse_golden_replays_bit_identically() {
    use chimera_core::dsp::tape_engine::TapeModel;
    use chimera_core::storage::{MIGRATIONS, TRANSLATIONS, decode_block};
    let mut s = Sound::init(EngineType::Tape);
    let mut payload = vec![34u8, 0];
    payload.extend_from_slice(&1u32.to_le_bytes());
    decode_block(&payload, MIGRATIONS, TRANSLATIONS, Some(&mut s.params)).unwrap();
    let t = s.params.tape;
    assert_eq!((t.model, t.start, t.end), (TapeModel::Loop, 1.0, 0.0));
    let out = tape_render(&t);
    let want = TAPE_GOLDENS
        .iter()
        .find(|g| g.0 == "tape_reverse")
        .unwrap()
        .1;
    assert_eq!((fnv1a(&out), spots(&out)), want);
}

#[test]
fn tape_goldens_match() {
    let got: Vec<_> = tape_cases()
        .iter()
        .map(|(name, t)| {
            let out = tape_render(t);
            assert!(out.iter().any(|&x| x.abs() > 1e-3), "{name} silent");
            (*name, (fnv1a(&out), spots(&out)))
        })
        .collect();
    golden::check(TAPE_GOLDENS, &got);
}
