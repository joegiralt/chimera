//! The mirror against the unit's own paths: its sets as `ed set`'s, its
//! pages as `ed describe`'s, its pixels as the desktop simulator's.

use chimera_core::block::DiskCode;
use chimera_core::console::{Console, EdState, Request, ed_pages};
use chimera_core::params::EngineType;
use chimera_core::project::arena::{ARENA_BYTES, ArenaWriter};
use chimera_core::project::{Kits, PartFrom, PartSource};
use chimera_core::source_table::SourceTable;
use chimera_core::ui::UiState;
use chimera_core::ui::animation::UiTick;
use chimera_core::ui::nav::{Location, PartChain};
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::theme_settings::palette;
use chimera_screen::{Fb, Mirror, PageNo, Rect, Region, Slot, pages};

const ALG: PageNo = PageNo(0);
const MORPH_KNOB: Slot = Slot::new(2).unwrap();

/// A mirror on `e`'s INIT, page `n` drawn.
fn drawn(e: EngineType, n: PageNo) -> Mirror {
    let mut m = Mirror::new();
    m.init(e);
    m.render(n, &mut Fb::new()).unwrap();
    m
}

fn algo_at(morph: f32) -> Fb {
    let mut m = Mirror::new();
    m.init(EngineType::Algo);
    // ALG's chart shows ALG A itself while ALG A has the focus.
    m.focus(ALG, MORPH_KNOB).unwrap();
    m.set("ALGO.MORPH", morph).unwrap();
    let mut fb = Fb::new();
    m.render(ALG, &mut fb).unwrap();
    fb
}

#[test]
fn focus_keeps_the_value_at_an_end() {
    let mut m = Mirror::new();
    m.init(EngineType::Algo);
    m.set("ALGO.MORPH", 127.0).unwrap();
    let was = m.ed_line("ed get ALGO.MORPH");
    m.focus(ALG, MORPH_KNOB).unwrap();
    assert_eq!(m.ed_line("ed get ALGO.MORPH"), was);
    assert_eq!(m.ui().focused_slot(), 2);
}

#[test]
fn morph_moves_the_algorithm_chart() {
    let (a, b) = (algo_at(0.0), algo_at(100.0));
    let differ = a.0.iter().zip(b.0.iter()).filter(|(x, y)| x != y).count();
    assert!(differ > 50, "{differ} pixels differ");
    // The chart moved, inside the page's art.
    let r = drawn(EngineType::Algo, ALG).region(Region::Screen).unwrap();
    let inside = (0..chimera_hal::FB_SIZE)
        .filter(|&i| a.0[i] != b.0[i])
        .filter(|&i| {
            let (x, y) = ((i % 240) as u16, (i / 240) as u16);
            (r.x..r.x + r.w).contains(&x) && (r.y..r.y + r.h).contains(&y)
        })
        .count();
    assert!(inside > 50, "{inside} of {differ} inside {r:?}");
}

/// A console line answered by `ed`, on `ui` with `q`.
fn ask(ui: &UiState, q: &mut EdState, line: &str) -> String {
    let mut c = Console::new();
    let mut req = None;
    for &b in line.as_bytes().iter().chain(b"\n") {
        req = c.push(b);
    }
    let Some(Ok(Request::Ed(l))) = req else {
        panic!("{line}: no ed line");
    };
    let mut out = String::new();
    chimera_core::bank2::ed_door(|d| d.answer(&l, ui, Some(q), &mut out))
        .unwrap()
        .unwrap();
    out
}

/// A UiState as the desktop simulator builds one (`main`, `qa::Desk`):
/// its own arena and kits, leaked.
fn desktop_ui() -> Box<UiState> {
    let table: &'static SourceTable = Box::leak(Box::new(SourceTable::new()));
    let arena = ArenaWriter::new(Box::leak(vec![0; ARENA_BYTES].into_boxed_slice()), table);
    Box::new(UiState::with_arena(
        arena,
        Box::leak(Box::new(Kits::DEFAULT)),
    ))
}

#[test]
fn a_render_is_the_desktop_simulators() {
    let mut ui = desktop_ui();
    let p = ui.active_part;
    ui.load_for_test(PartSource {
        part: p,
        from: PartFrom::Init(EngineType::Algo),
    });
    // The sim's console path: `ed set`, then its UI pass's `ed_apply`.
    let mut q = EdState::new();
    assert_eq!(ask(&ui, &mut q, "ed set ALGO.MORPH 77"), "OK queued\n");
    chimera_core::bank2::ed_apply(&mut q, &mut ui);
    let chain = PartChain::of(&ui.project().part(p).sound).def();
    let def = pages::page_at(EngineType::Algo, ALG).unwrap().0;
    ui.go_for_test(Location::pages(p, chain.locate(def.id).unwrap()));
    ui.update(UiTick::for_test());
    // `DesktopDisplay`: the frame, then its palette (`to_rgb`'s first step).
    let mut sim = Fb::new();
    ui.render_with_scope(
        &mut sim,
        &PerfStats::zero(),
        &[0.0; chimera_core::scope::SCOPE_LEN],
    );
    let pal = palette(&ui.theme());
    sim.0.iter_mut().for_each(|px| *px = pal.map_raw(*px));

    let mut m = Mirror::new();
    m.init(EngineType::Algo);
    m.set("ALGO.MORPH", 77.0).unwrap();
    let mut fb = Fb::new();
    m.render(ALG, &mut fb).unwrap();
    assert!(fb.0 == sim.0, "the mirror's frame is not the sim's");
}

#[test]
fn a_set_stores_what_ed_set_stores() {
    for (addr, stored, typed) in [
        ("ALGO.MORPH", 64.0, "64"),
        ("OP_A.WAVE", 9.0, "SAW"),
        ("FILTER.CUTOFF", 0.37, "0.37"),
    ] {
        let mut m = Mirror::new();
        m.init(EngineType::Algo);
        m.set(addr, stored).unwrap();

        let mut ui = desktop_ui();
        let p = ui.active_part;
        ui.load_for_test(PartSource {
            part: p,
            from: PartFrom::Init(EngineType::Algo),
        });
        let mut q = EdState::new();
        assert_eq!(
            ask(&ui, &mut q, &format!("ed set {addr} {typed}")),
            "OK queued\n"
        );
        chimera_core::bank2::ed_apply(&mut q, &mut ui);

        let get = format!("ed get {addr}");
        let unit = ask(&ui, &mut q, &get);
        let mine = m.ed_line(&get);
        assert_eq!(mine, unit, "{addr}");
        let fresh = Mirror::new().ed_line(&get);
        assert_ne!(mine, fresh, "{addr} didn't move");
    }
}

#[test]
fn a_set_the_unit_refuses_is_refused() {
    let mut m = Mirror::new();
    m.init(EngineType::Algo);
    assert!(m.set("NOPE.X", 1.0).is_err());
    assert!(m.set("OP_A.WAVE", 99.0).is_err());
    assert!(m.set("ALGO.MORPH", f32::NAN).is_err());
}

#[test]
fn init_switches_engines_after_edits() {
    let mut m = Mirror::new();
    m.init(EngineType::Algo);
    m.set("ALGO.MORPH", 50.0).unwrap();
    for e in EngineType::ALL {
        m.init(e);
        assert_eq!(m.engine(), e);
        m.init(EngineType::Algo);
        assert_eq!(
            m.ed_line("ed get ALGO.MORPH"),
            "val ALGO.MORPH 0 0 \"0\"\nOK\n"
        );
    }
}

#[test]
fn pages_are_ed_describes() {
    let mut m = Mirror::new();
    for e in EngineType::ALL {
        let count = ed_pages(e);
        for n in 0..u16::from(count) {
            let (def, pad) = pages::page_at(e, PageNo(n)).unwrap();
            let pad = pad.map_or(String::new(), |p| format!("PAD{}_", p.number()));
            let want = format!("page {n} {pad}{} of {count}\n", def.short);
            let got = m.ed_line(&format!("ed describe {} {n}", e.disk_ident()));
            assert!(got.starts_with(&want), "{got:.60} not {want}");
        }
        assert!(pages::page_at(e, PageNo(count.into())).is_none());
    }
}

#[test]
fn every_page_of_every_engine_draws() {
    let mut m = Mirror::new();
    let mut fb = Fb::new();
    for e in EngineType::ALL {
        m.init(e);
        let mut drawn = 0;
        for n in 0..u16::from(ed_pages(e)) {
            if m.render(PageNo(n), &mut fb).is_some() {
                drawn += 1;
                assert!(m.region(Region::Screen).is_some());
            }
        }
        assert!(drawn > 0, "{e:?}");
    }
}

#[test]
fn a_second_render_is_the_same() {
    let mut m = drawn(EngineType::Va, PageNo(0));
    let (mut a, mut b) = (Fb::new(), Fb::new());
    m.render(PageNo(0), &mut a).unwrap();
    m.render(PageNo(0), &mut b).unwrap();
    assert!(a.0 == b.0);
}

#[test]
fn engine_order_is_the_scripts() {
    // screen.js names engines by `EngineType::ALL`'s index.
    let names: Vec<_> = EngineType::ALL.iter().map(|e| e.disk_ident()).collect();
    assert_eq!(names, ["ALGO", "MODAL", "TAPE", "VA", "DRUM"]);
}

#[test]
fn regions_sit_on_the_screen() {
    for l in [
        chimera_core::ui::page::PageLayout::BigViz,
        chimera_core::ui::page::PageLayout::CellGrid,
        chimera_core::ui::page::PageLayout::Matrix,
    ] {
        for r in [Region::Full, Region::Screen, Region::Gauge] {
            if let Some(Rect { x, y, w, h }) = Rect::of(r, l) {
                assert!(
                    w > 0 && h > 0 && x + w <= 240 && y + h <= 320,
                    "{r:?} {l:?}"
                );
            }
        }
    }
}
