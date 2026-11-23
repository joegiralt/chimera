// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The mixer's FX pictures: CHR's braid, moiré and ray gun (ADR 0324),
//! DLY's rings, REV's cube and tail, MST's curve, each drawn from the
//! values it is sent and, where it moves, by frame.

use chimera_core::console::{Schema, ed_schema_pages};
use chimera_screen::abi::{self, Status};
use chimera_screen::{Addr, Fb, Mirror, PageNo, Region, Shown, Slot, pages};

const CHR: PageNo = PageNo(0);
const DLY: PageNo = PageNo(1);
const REV: PageNo = PageNo(4);
const MST: PageNo = PageNo(8);

/// Each FX's picture: its page, region, a focus that draws it, a value
/// that moves it at two settings, and what else it is set to (MST's curve
/// is a straight line at 1:1, its default).
type Picture = (
    &'static str,
    PageNo,
    Region,
    u8,
    &'static str,
    f32,
    f32,
    (&'static str, f32),
);
const PICTURES: [Picture; 4] = [
    (
        "CHORUS",
        CHR,
        Region::Gauge,
        2,
        "CHORUS.DEPTH",
        0.1,
        0.9,
        ("CHORUS.MODE", 1.0),
    ),
    (
        "DELAY",
        DLY,
        Region::Gauge,
        2,
        "DELAY.FDBK",
        0.1,
        0.9,
        ("DELAY.TYPE", 0.0),
    ),
    (
        "REVERB",
        REV,
        Region::Gauge,
        5,
        "REVERB.MIX",
        0.1,
        0.9,
        ("REVERB.TYPE", 0.0),
    ),
    (
        "MASTER",
        MST,
        Region::Screen,
        0,
        "COMP.THRESH",
        0.1,
        0.9,
        ("COMP.RATIO", 4.0),
    ),
];

fn addr(s: &str) -> Addr {
    Addr::parse(s).unwrap_or_else(|| panic!("{s}"))
}

/// `r`'s pixels of page `n`, focus `slot`, at `frame`, after `vals`.
fn crop(m: &mut Mirror, n: PageNo, r: Region, slot: u8, frame: u32) -> Vec<u16> {
    let mut fb = Fb::new();
    let how = Shown {
        focus: Slot::new(slot),
        frame,
        ..Shown::default()
    };
    m.render(n, how, &mut fb).unwrap_or_else(|| panic!("{n:?}"));
    let rect = m.region(r).unwrap();
    (rect.y..rect.y + rect.h)
        .flat_map(|y| (rect.x..rect.x + rect.w).map(move |x| (x, y)))
        .map(|(x, y)| fb.0[usize::from(y) * 240 + usize::from(x)])
        .collect()
}

fn with(vals: &[(&str, f32)]) -> Mirror {
    let mut m = Mirror::mixer();
    let vals: Vec<_> = vals.iter().map(|&(a, v)| (addr(a), v)).collect();
    let done = m.apply(&vals);
    assert!(done.refused.is_empty(), "{:?}", done.refused);
    m
}

fn differ(a: &[u16], b: &[u16]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}

/// More than a flat line: the crop's most common colour leaves this many
/// pixels to the rest.
fn drawn(px: &[u16]) -> usize {
    let mut counts = std::collections::HashMap::new();
    for &p in px {
        *counts.entry(p).or_insert(0usize) += 1;
    }
    px.len() - counts.values().max().copied().unwrap_or(0)
}

#[test]
fn the_mixers_pages_are_ed_describe_mixers() {
    let mut m = Mirror::mixer();
    let count = ed_schema_pages(Schema::Mixer);
    let mut shared = 0;
    for n in 0..u16::from(count) {
        let got = m.ed_line(&format!("ed describe MIXER {n}"));
        match pages::mixer_at(PageNo(n)) {
            Some(def) => {
                shared += 1;
                let want = format!("page {n} {} of {count}\n", def.short);
                assert!(got.starts_with(&want), "{got:.60} not {want}");
                assert!(
                    m.render(PageNo(n), Shown::default(), &mut Fb::new())
                        .is_some()
                );
            }
            // A Part's own: not the FX's, not shown.
            None => assert!(got.starts_with(&format!("page {n} PART")), "{got:.60}"),
        }
    }
    assert!(shared >= 9, "{shared}");
    for (name, n, ..) in PICTURES {
        let short = pages::mixer_at(n).unwrap().short;
        let want = match name {
            "CHORUS" => "CHR",
            "DELAY" => "DLY",
            "REVERB" => "REV",
            _ => "MST",
        };
        assert_eq!(short, want);
    }
}

#[test]
fn each_picture_draws_moves_with_its_value_and_animates() {
    for (name, n, r, slot, a, lo, hi, base) in PICTURES {
        let mut low = with(&[base, (a, lo)]);
        let mut high = with(&[base, (a, hi)]);
        let still = crop(&mut low, n, r, slot, 40);
        assert!(
            drawn(&still) > 100,
            "{name}: {} pixels drawn",
            drawn(&still)
        );
        let d = differ(&still, &crop(&mut high, n, r, slot, 40));
        assert!(d > 20, "{name}: {a} {lo} to {hi} moves {d} pixels");
        // The same frame again: the same picture, whatever came between.
        assert_eq!(crop(&mut low, n, r, slot, 40), still, "{name}");
        if name != "MASTER" {
            let d = differ(&still, &crop(&mut low, n, r, slot, 47));
            assert!(d > 20, "{name}: frames 40 and 47 differ in {d} pixels");
        }
    }
}

/// TYPE switches CHR's picture: the braid (I), the moiré (FLANGER), the
/// ray gun (PHASER), each its own and each moving.
#[test]
fn chorus_type_switches_braid_moire_and_ray_gun() {
    let shots: Vec<Vec<u16>> = [1.0, 4.0, 5.0]
        .iter()
        .map(|&mode| {
            let mut m = with(&[("CHORUS.MODE", mode), ("CHORUS.RATE", 0.7)]);
            let a = crop(&mut m, CHR, Region::Gauge, 1, 30);
            assert!(drawn(&a) > 100, "MODE {mode}");
            let d = differ(&a, &crop(&mut m, CHR, Region::Gauge, 1, 33));
            assert!(d > 10, "MODE {mode}: frames differ in {d} pixels");
            a
        })
        .collect();
    for (i, j) in [(0, 1), (0, 2), (1, 2)] {
        let d = differ(&shots[i], &shots[j]);
        assert!(d > 200, "pictures {i} and {j} differ in {d} pixels");
    }
}

/// The reverb's TYPE switches its picture: QUAD's cube, the others' tail.
#[test]
fn reverb_type_switches_cube_and_tail() {
    let mut quad = with(&[("REVERB.TYPE", 0.0)]);
    let mut hall = with(&[("REVERB.TYPE", 2.0)]);
    let d = differ(
        &crop(&mut quad, REV, Region::Gauge, 5, 0),
        &crop(&mut hall, REV, Region::Gauge, 5, 0),
    );
    assert!(d > 100, "{d}");
}

/// Through the exports: MIXER is the engine past the five, its show takes
/// a frame, and its reset puts the FX back.
#[test]
fn the_abi_draws_the_mixer() {
    let mixer = chimera_core::params::EngineType::PLAYABLE.len() as u8;
    let put = |text: &str, vals: &[f32]| {
        // SAFETY: the shell's buffers, `TEXT_CAP` and `VALS_CAP` long,
        // written between calls on this thread alone.
        unsafe {
            core::ptr::copy_nonoverlapping(text.as_ptr(), abi::text_ptr(), text.len());
            core::ptr::copy_nonoverlapping(vals.as_ptr(), abi::vals_ptr(), vals.len());
        }
        text.len() as u32
    };
    let ok = i32::from(Status::Ok);
    let gauge = Region::Gauge as u8;
    let shot = |frame: u32| {
        assert_eq!(abi::show(mixer, 0, gauge, 1, 0, frame), ok);
        // SAFETY: the shell's frame, read between calls on this thread.
        unsafe { core::slice::from_raw_parts(abi::buf_ptr(), chimera_hal::FB_SIZE) }.to_vec()
    };
    let len = put("CHORUS.MODE\nCHORUS.RATE", &[4.0, 0.7]);
    assert_eq!(abi::apply(mixer, 2, len), ok);
    assert_eq!(abi::writes(), 2);
    let (a, b) = (shot(10), shot(13));
    assert!(differ(&a, &b) > 10);
    assert_eq!(shot(10), a);
    assert_eq!(
        abi::show(mixer, 10, gauge, -1, 0, 0),
        i32::from(Status::NotShown),
        "a Part's own page"
    );
    assert_eq!(
        abi::show(mixer, 8, gauge, -1, 0, 0),
        i32::from(Status::NoRegion),
        "MST is BigViz"
    );
    assert_eq!(abi::reset(mixer), ok);
    assert_eq!(abi::apply(mixer, 2, len), ok);
    assert_eq!(abi::writes(), 2, "the defaults again");
    assert_eq!(abi::apply(mixer + 1, 0, 0), i32::from(Status::NoSuchEngine));
}

/// Every FX page as the unit's own path draws it: `ed set` and its UI
/// pass, Part 1's mixer entered on the page, the still, the frame.
#[test]
fn the_fx_pages_are_the_units() {
    use chimera_core::console::{Console, EdState, Request};
    use chimera_core::ui::UiState;
    use chimera_core::ui::animation::UiTick;
    use chimera_core::ui::block_registry::MIXER_CHANNEL_CHAIN;
    use chimera_core::ui::nav::Location;
    use chimera_core::ui::perf::PerfStats;
    use chimera_core::ui::theme_settings::palette;
    use chimera_core::ui::view::{self, View};

    let vals = [
        ("CHORUS.MODE", "#5"),
        ("CHORUS.RATE", "0.6"),
        ("DELAY.FDBK", "0.7"),
        ("REVERB.TYPE", "#2"),
        ("REVERB.MIX", "0.4"),
        ("COMP.RATIO", "#3"),
        ("COMP.THRESH", "0.3"),
    ];
    let mut ui = Box::new(UiState::new());
    let mut q = EdState::new();
    for (a, v) in vals {
        let mut c = Console::new();
        let mut req = None;
        for &b in format!("ed set {a} {v}\n").as_bytes() {
            req = c.push(b);
        }
        let Some(Ok(Request::Ed(l))) = req else {
            panic!("{a}");
        };
        let mut out = String::new();
        chimera_core::bank2::ed_door(|d| d.answer(&l, &ui, Some(&mut q), &mut out))
            .unwrap()
            .unwrap();
        assert!(out.starts_with("OK"), "{a}: {out}");
        chimera_core::bank2::ed_apply(&mut q, &mut ui);
    }
    let mut m = Mirror::mixer();
    let sent: Vec<_> = vals
        .iter()
        .map(|&(a, _)| (addr(a), stored_on(&ui, a)))
        .collect();
    assert!(m.apply(&sent).refused.is_empty());
    let p = ui.active_part;
    let mut compared = 0;
    for n in 0..u16::from(ed_schema_pages(Schema::Mixer)) {
        // A Part's own page: not the FX's.
        let Some(def) = pages::mixer_at(PageNo(n)) else {
            continue;
        };
        for slot in 0..6u8 {
            let how = Shown {
                focus: Slot::new(slot),
                frame: 25 + u32::from(slot),
                ..Shown::default()
            };
            ui.go_for_test(Location::fx(p, MIXER_CHANNEL_CHAIN.locate(def.id).unwrap()));
            let empty = view::view(def, usize::from(slot), &ui.slot_ctx_for_test()) == View::Empty;
            let mut mine = Fb::new();
            let drew = m.render(PageNo(n), how, &mut mine).is_some();
            // Only an empty cell takes no focus.
            assert_eq!(drew, !empty, "{} slot {slot}", def.short);
            if empty {
                continue;
            }
            compared += 1;
            ui.focus_for_test(usize::from(slot));
            ui.still_for_test();
            ui.step_toast(60_000);
            ui.update(UiTick::for_test());
            ui.frame_for_test(how.frame);
            let mut theirs = Fb::new();
            ui.render_with_scope(
                &mut theirs,
                &PerfStats::zero(),
                &[0.0; chimera_core::scope::SCOPE_LEN],
            );
            let pal = palette(&ui.theme());
            theirs.0.iter_mut().for_each(|px| *px = pal.map_raw(*px));
            let d = differ(&mine.0[..], &theirs.0[..]);
            assert_eq!(d, 0, "{} slot {slot}: {d} pixels", def.short);
        }
    }
    assert!(compared > 30, "{compared} cells compared");
}

/// `a`'s stored value on `ui`, as `ed since` reports it.
fn stored_on(ui: &chimera_core::ui::UiState, a: &str) -> f32 {
    let at = chimera_core::console::EdAddr::parse(a.as_bytes()).unwrap();
    chimera_core::console::ed_stored(ui, at).unwrap()
}
