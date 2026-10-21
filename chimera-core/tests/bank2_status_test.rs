//! Bank 2 as the boot judged it: `status`'s line and ABOUT's label (ADR 0137).

mod screen;

use chimera_core::bank2::{Bank2Status, Version};
use chimera_core::ui::UiState;
use chimera_core::ui::about_page::viz_lines;
use chimera_core::ui::perf::PerfStats;
use chimera_image::{Header, Verdict};
use screen::*;

fn line(s: &Bank2Status) -> String {
    let mut w = String::new();
    s.line(&mut w).unwrap();
    w
}

const SEAL: [u8; 8] = [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef];

fn open() -> Bank2Status {
    Bank2Status::Open {
        version: Version::new("0.2.0 02e440e2"),
        seal: SEAL,
    }
}

#[test]
fn bank2_status_lines() {
    use Bank2Status::*;
    let cases = [
        (Unsealed, "bank2 unsealed"),
        (Empty, "bank2 empty"),
        (Torn, "bank2 torn"),
        (Foreign, "bank2 foreign"),
        (Format(7), "bank2 format 7"),
        (Corrupt, "bank2 corrupt"),
        (
            Stale {
                have: Version::new("0.1.9 aaaaaaaa"),
            },
            "bank2 stale have 0.1.9 aaaaaaaa",
        ),
        (open(), "bank2 open 0.2.0 02e440e2 seal=0123456789abcdef"),
        (
            EccEraseFailed(chimera_image::FlashFail::Timeout),
            "bank2 ecc erase failed timeout",
        ),
        (
            EccEraseFailed(chimera_image::FlashFail::NoClock),
            "bank2 ecc erase failed no cycle counter",
        ),
    ];
    for (s, want) in cases {
        assert_eq!(line(&s), want);
    }
}

#[test]
fn only_open_is_ok() {
    use Bank2Status::*;
    assert_eq!(open().label(), "OK");
    let erase = EccEraseFailed(chimera_image::FlashFail::Busy);
    for s in [Unsealed, Empty, Torn, Foreign, Format(2), Corrupt, erase] {
        assert_eq!(s.label(), "NEEDS UPDATE");
    }
    let have = Version::new("0.1.9 aaaaaaaa");
    assert_eq!(Stale { have }.label(), "NEEDS UPDATE");
}

#[test]
fn from_verdict_keeps_what_the_judge_saw() {
    let have = Version::new("0.1.9 aaaaaaaa");
    let h = Header {
        body_len: 32,
        body_crc: 0,
        door_off: 256,
        seal: SEAL,
        version: Version::new("0.2.0 02e440e2"),
    };
    let cases = [
        (Verdict::Empty, Bank2Status::Empty),
        (Verdict::Torn, Bank2Status::Torn),
        (Verdict::Foreign, Bank2Status::Foreign),
        (Verdict::Format(3), Bank2Status::Format(3)),
        (Verdict::Corrupt, Bank2Status::Corrupt),
        // A read the flash refused is no image to trust.
        (Verdict::Unreadable, Bank2Status::Corrupt),
        (Verdict::Stale { have }, Bank2Status::Stale { have }),
        (Verdict::Good(h), open()),
    ];
    for (v, want) in cases {
        assert_eq!(Bank2Status::from_verdict(&v), want);
    }
}

#[test]
fn about_shows_bank2() {
    let label = |s: Option<Bank2Status>| viz_lines(s.as_ref())[2].as_str().to_string();
    assert_eq!(label(Some(Bank2Status::Empty)), "NEEDS UPDATE");
    assert_eq!(label(Some(Bank2Status::Unsealed)), "NEEDS UPDATE");
    assert_eq!(label(Some(open())), "OK");
    // No bank 2 on this target (the desktop): no line.
    assert_eq!(label(None), "");
}

#[test]
fn about_draws_the_label() {
    let shot = |s: Option<Bank2Status>| {
        let mut ui = UiState::new();
        if let Some(s) = s {
            ui.set_bank2(s);
        }
        to_leaf(&mut ui, &["SYSTEM", "ABOUT"]);
        settle(&mut ui);
        let mut fb = Fb::new();
        ui.render_with_audio(&mut fb, &PerfStats::zero(), None, &scope_fixture());
        fb.hash()
    };
    let (none, empty, ok) = (
        shot(None),
        shot(Some(Bank2Status::Empty)),
        shot(Some(open())),
    );
    assert_ne!(none, empty);
    assert_ne!(empty, ok);
}

#[test]
fn status_has_the_bank2_line() {
    let mut ui = UiState::new();
    let mut w = String::new();
    chimera_core::console::write_status(&ui, &mut w).unwrap();
    assert!(!w.contains("bank2"), "no bank 2 on this target, no line");
    ui.set_bank2(Bank2Status::Empty);
    let mut w = String::new();
    chimera_core::console::write_status(&ui, &mut w).unwrap();
    assert!(w.ends_with("\nbank2 empty\n"), "{w}");
}

/// `INSTALLING <version>` inside the screen: the hash goes first, then
/// whatever still doesn't fit.
#[test]
fn installing_fits_the_screen() {
    use chimera_core::ui::{draw, installing, theme};
    let fit = |v: &str| {
        let l = installing::line(v);
        let w = draw::text_width(&theme::FONT_VALUE, l.as_str(), theme::LABEL_TRACKING);
        assert!(w <= installing::MAX_W, "{} is {w} px", l.as_str());
        l.as_str().to_owned()
    };
    assert_eq!(fit("0.2.0 02e440e2"), "INSTALLING 0.2.0 02e440e2");
    assert_eq!(fit("0.10.12 02e440e2"), "INSTALLING 0.10.12");
    let long = fit("WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW");
    assert!(long.starts_with("INSTALLING WW"), "{long}");
    assert_eq!(fit(""), "INSTALLING");
}

/// The gate's hold line, each phase, fits as the version line does.
#[cfg(feature = "bank2-gate")]
#[test]
fn hold_line_fits_the_screen() {
    use chimera_core::ui::{draw, installing, theme};
    use chimera_image::Phase;
    for p in [Phase::Erasing, Phase::Writing, Phase::Sealing] {
        let l = installing::hold_line(p, 3);
        let w = draw::text_width(&theme::FONT_VALUE, l.as_str(), theme::LABEL_TRACKING);
        assert!(w <= installing::MAX_W, "{} is {w} px", l.as_str());
    }
    assert_eq!(
        installing::hold_line(Phase::Erasing, 3).as_str(),
        "ERASING  PULL NOW 3"
    );
}

/// While bank 2 isn't open the reverb plays nothing (ADR 0142): its TYPE
/// reads NEEDS UPDATE. Open, or with no bank 2 (the desktop), its TYPE.
#[test]
fn a_closed_bank2_labels_the_reverb_type() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::bank2::closed_text;
    use chimera_core::dsp::reverb::ReverbParams;
    let ty = ParamAddr::new(BlockRef::Reverb, ReverbParams::TYPE);
    assert_eq!(
        closed_text(ty, 0.0, Some(Bank2Status::Empty)),
        Some("NEEDS UPDATE")
    );
    assert_eq!(
        closed_text(ty, 0.0, Some(Bank2Status::Unsealed)),
        Some("NEEDS UPDATE")
    );
    assert_eq!(closed_text(ty, 0.0, Some(open())), None);
    assert_eq!(closed_text(ty, 0.0, None), None, "no bank 2 on this target");
    let mix = ParamAddr::new(BlockRef::Reverb, ReverbParams::MIX);
    assert_eq!(closed_text(mix, 0.0, Some(Bank2Status::Empty)), None);
}

/// The focus band reads NEEDS UPDATE; the TYPE cell, a column wide,
/// UPDATE, leaving SIZE's cell beside it alone.
#[test]
fn the_rev_page_draws_needs_update_closed() {
    use chimera_core::bank2::NEEDS_UPDATE_CELL;
    use chimera_core::ui::draw::text_width;
    use chimera_core::ui::theme::{CELL_COL_W, CELL_LABEL_Y, CELL_ROW_H, FONT_VALUE, MARGIN_X};
    assert!(text_width(&FONT_VALUE, NEEDS_UPDATE_CELL, 0) <= CELL_COL_W - 4);
    let shot = |s: Option<Bank2Status>| {
        let mut ui = UiState::new();
        if let Some(s) = s {
            ui.set_bank2(s);
        }
        to_fx(&mut ui, REV);
        settle(&mut ui);
        render_ui(&ui)
    };
    let (none, empty, ok) = (
        shot(None),
        shot(Some(Bank2Status::Empty)),
        shot(Some(open())),
    );
    assert_eq!(none.hash(), ok.hash(), "open, REV draws as it always did");
    assert_ne!(empty.hash(), ok.hash());
    let size_cell = |fb: &Fb| {
        let x0 = MARGIN_X + CELL_COL_W;
        let y0 = CELL_LABEL_Y - 4;
        (y0..y0 + CELL_ROW_H)
            .flat_map(|y| (x0..x0 + CELL_COL_W).map(move |x| (x, y)))
            .map(|(x, y)| fb.at(x, y))
            .collect::<Vec<_>>()
    };
    assert!(
        size_cell(&empty) == size_cell(&ok),
        "TYPE's text ran into SIZE"
    );
}

/// The TYPE cell itself reads UPDATE closed.
#[test]
fn the_type_cell_reads_update_closed() {
    use chimera_core::bank2::NEEDS_UPDATE_CELL;
    let mut ui = UiState::new();
    ui.set_bank2(Bank2Status::Empty);
    to_fx(&mut ui, REV);
    settle(&mut ui);
    assert!(first_cell_reads(&render_ui(&ui), NEEDS_UPDATE_CELL));
    let mut ui = UiState::new();
    ui.set_bank2(open());
    to_fx(&mut ui, REV);
    settle(&mut ui);
    assert!(!first_cell_reads(&render_ui(&ui), NEEDS_UPDATE_CELL));
}

/// While bank 2 isn't open VA FM, LOOPFM, SOFT SYNC and PHASE are silent (ADR 0163):
/// MODE reads NEEDS UPDATE at them, and its own name at the bank-1 MODEs.
#[test]
fn a_closed_bank2_labels_va_fm() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::bank2::closed_text;
    use chimera_core::dsp::va::{MODE_NAMES, VaMode, VaParams};
    let mode = ParamAddr::new(BlockRef::Va, VaParams::MODE);
    let at = |m: VaMode| m as u8 as f32 / (MODE_NAMES.len() - 1) as f32;
    let closed = Some(Bank2Status::Empty);
    // LOOPFM's whole render is bank 2's too
    for m in [VaMode::Fm, VaMode::LoopFm, VaMode::SoftSync, VaMode::Phase] {
        assert!(m.in_bank2(), "{m:?}");
        assert_eq!(closed_text(mode, at(m), closed), Some("NEEDS UPDATE"));
        assert_eq!(closed_text(mode, at(m), Some(open())), None);
        assert_eq!(
            closed_text(mode, at(m), None),
            None,
            "{m:?}: no bank 2 on this target"
        );
    }
    for m in VaMode::ALL.into_iter().filter(|m| !m.in_bank2()) {
        assert_eq!(closed_text(mode, at(m), closed), None, "{m:?}");
    }
    let wave = ParamAddr::new(BlockRef::Va, VaParams::WAVE1);
    assert_eq!(closed_text(wave, at(VaMode::Fm), closed), None);
}

/// VA's home with MODE turned `by` from MIX, bank 2 at `s`.
fn va_home(by: i8, s: Option<Bank2Status>) -> Fb {
    use chimera_core::params::EngineType;
    use chimera_hal::EncoderId;
    let mut ui = stack_ui();
    if let Some(s) = s {
        ui.set_bank2(s);
    }
    load_init(&mut ui, EngineType::Va);
    feed(&mut ui, Input::turn(EncoderId::C, by));
    settle(&mut ui);
    render_ui(&ui)
}

/// The VA page draws MODE's cell as UPDATE at FM and SOFT SYNC closed; at
/// MIX, or open, as it always did.
#[test]
fn the_va_page_draws_update_at_fm_and_soft_sync_closed() {
    let empty = Some(Bank2Status::Empty);
    assert_eq!(
        va_home(0, empty).hash(),
        va_home(0, None).hash(),
        "MIX changed closed"
    );
    assert_eq!(va_home(5, Some(open())).hash(), va_home(5, None).hash());
    assert_ne!(va_home(5, empty).hash(), va_home(5, None).hash());
    use chimera_core::bank2::{NEEDS_UPDATE, NEEDS_UPDATE_CELL};
    let closed = va_home(5, empty);
    assert!(cell_reads(&closed, 2, NEEDS_UPDATE_CELL), "MODE's cell");
    assert!(band_reads(&closed, NEEDS_UPDATE), "the focus band");
    let fm = va_home(5, Some(open()));
    assert!(cell_reads(&fm, 2, "FM"));
    assert!(!band_reads(&fm, NEEDS_UPDATE));
    // LOOPFM (MODE 8 from MIX) likewise
    let n = chimera_core::dsp::va::VaMode::LoopFm as i8;
    let lf = va_home(n, empty);
    assert!(cell_reads(&lf, 2, NEEDS_UPDATE_CELL), "LOOPFM's cell");
    assert!(band_reads(&lf, NEEDS_UPDATE), "LOOPFM's band");
    assert!(!band_reads(&va_home(n, Some(open())), NEEDS_UPDATE));
    // SOFT SYNC likewise
    let at = chimera_core::dsp::va::VaMode::SoftSync as i8;
    let closed = va_home(at, empty);
    assert!(cell_reads(&closed, 2, NEEDS_UPDATE_CELL), "SOFT's cell");
    assert!(band_reads(&closed, NEEDS_UPDATE), "SOFT's focus band");
    assert_eq!(va_home(at, Some(open())).hash(), va_home(at, None).hash());
    let soft = va_home(at, Some(open()));
    assert!(cell_reads(&soft, 2, "SOFT"));
    assert!(!band_reads(&soft, NEEDS_UPDATE));
    // PHASE likewise
    let at = chimera_core::dsp::va::VaMode::Phase as i8;
    let closed = va_home(at, empty);
    assert!(cell_reads(&closed, 2, NEEDS_UPDATE_CELL), "PHASE's cell");
    assert!(band_reads(&closed, NEEDS_UPDATE), "PHASE's focus band");
    assert_eq!(va_home(at, Some(open())).hash(), va_home(at, None).hash());
    let phase = va_home(at, Some(open()));
    assert!(cell_reads(&phase, 2, "PHASE"));
    assert!(!band_reads(&phase, NEEDS_UPDATE));
}

/// While bank 2 isn't open FUZZ passes dry (ADR 0157): the FX TYPE reads
/// NEEDS UPDATE at FUZZ, and its own name at the bank-1 types.
#[test]
fn fuzz_type_reads_needs_update_while_closed() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::bank2::closed_text;
    use chimera_core::dsp::voice_fx::params::{FxType, VoiceFxParams};
    let ty = ParamAddr::new(BlockRef::VoiceFx, VoiceFxParams::TYPE);
    let at = |t: FxType| {
        FxType::LIST.iter().position(|&x| x == t).unwrap() as f32 / (FxType::LIST.len() - 1) as f32
    };
    for closed in [Bank2Status::Empty, Bank2Status::Unsealed] {
        assert_eq!(
            closed_text(ty, at(FxType::Fuzz), Some(closed)),
            Some("NEEDS UPDATE")
        );
        for t in [FxType::Off, FxType::Soft, FxType::Fold] {
            assert_eq!(closed_text(ty, at(t), Some(closed)), None, "{t:?}");
        }
    }
    assert_eq!(closed_text(ty, at(FxType::Fuzz), Some(open())), None);
    assert_eq!(
        closed_text(ty, at(FxType::Fuzz), None),
        None,
        "no bank 2 here"
    );
    let p1 = ParamAddr::new(BlockRef::VoiceFx, VoiceFxParams::P1);
    assert_eq!(
        closed_text(p1, at(FxType::Fuzz), Some(Bank2Status::Empty)),
        None
    );
}

/// SEM's kernel is bank 2's (ADR 0192): closed, TYPE reads NEEDS UPDATE at
/// SEM and its block plays dry; the other TYPEs read their names.
#[test]
fn a_closed_bank2_labels_the_sem_filter() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::bank2::closed_text;
    use chimera_core::dsp::filter::FilterKind;
    use chimera_core::params::FilterParams;
    let kind = ParamAddr::new(BlockRef::Filter, FilterParams::KIND);
    let n = FilterKind::BUILT.len();
    let at = |i: usize| i as f32 / (n - 1) as f32;
    let closed = Some(Bank2Status::Empty);
    for (i, k) in FilterKind::BUILT.into_iter().enumerate() {
        let want = (k == FilterKind::Sem).then_some("NEEDS UPDATE");
        assert_eq!(k.in_bank2(), k == FilterKind::Sem);
        assert_eq!(closed_text(kind, at(i), closed), want, "{k:?}");
        assert_eq!(closed_text(kind, at(i), Some(open())), None);
        assert_eq!(closed_text(kind, at(i), None), None, "{k:?}");
    }
}
