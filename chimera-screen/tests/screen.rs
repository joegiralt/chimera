//! The mirror against the unit's own paths: its values as `ed set`'s, its
//! pages as `ed describe`'s, its pixels as the desktop simulator's.

use chimera_core::block::DiskCode;
use chimera_core::console::{Console, EdAddr, EdState, Request, SetRefusal, ed_pages, ed_stored};
use chimera_core::dsp::drum::types::{Machine, PadId};
use chimera_core::params::EngineType;
use chimera_core::project::{PartFrom, PartSource};
use chimera_core::ui::UiState;
use chimera_core::ui::animation::UiTick;
use chimera_core::ui::kit_band;
use chimera_core::ui::nav::{Location, PartChain};
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::theme_settings::palette;
use chimera_screen::abi::{self, Status};
use chimera_screen::{Addr, Fb, Mirror, PageNo, Rect, Refused, Region, Shown, Slot, pages};
use embedded_graphics_core::pixelcolor::raw::{RawData, RawU16};

const ALG: PageNo = PageNo(0);
const MORPH_KNOB: Slot = Slot::new(2).unwrap();

fn addr(s: &str) -> Addr {
    Addr::parse(s).unwrap_or_else(|| panic!("{s}"))
}

fn mirror(e: EngineType) -> Mirror {
    let mut m = Mirror::new();
    m.init(e);
    m
}

fn algo_at(morph: f32, focus: Option<Slot>) -> Fb {
    let mut m = mirror(EngineType::Algo);
    m.set(addr("ALGO.MORPH"), morph).unwrap();
    let mut fb = Fb::new();
    let how = Shown { focus, pad: None };
    m.render(ALG, how, &mut fb).unwrap();
    fb
}

fn differ(a: &Fb, b: &Fb) -> usize {
    a.0.iter().zip(b.0.iter()).filter(|(x, y)| x != y).count()
}

#[test]
fn focus_moves_the_band_and_keeps_the_value() {
    let mut m = mirror(EngineType::Algo);
    m.set(addr("ALGO.MORPH"), 127.0).unwrap();
    let was = m.ed_line("ed get ALGO.MORPH");
    let how = Shown {
        focus: Some(MORPH_KNOB),
        pad: None,
    };
    m.render(ALG, how, &mut Fb::new()).unwrap();
    assert_eq!(m.ed_line("ed get ALGO.MORPH"), was);
    assert_eq!(m.ui().focused_slot(), 2);
    // No focus is slot a, the unit's until a turn: the mirror forgets.
    m.render(ALG, Shown::default(), &mut Fb::new()).unwrap();
    assert_eq!(m.ui().focused_slot(), 0);
}

/// ALG's chart morphs with MORPH off ALG A and B (MORPH's own knob, c);
/// on ALG A, slot a's focus, it shows ALG A itself (ADR 0153).
#[test]
fn morph_moves_the_algorithm_chart_with_morph_focused() {
    let (a, b) = (
        algo_at(0.0, Some(MORPH_KNOB)),
        algo_at(100.0, Some(MORPH_KNOB)),
    );
    let d = differ(&a, &b);
    assert!(d > 50, "{d} pixels differ");
    let r = Rect::of(
        Region::Screen,
        pages::page_at(EngineType::Algo, ALG).unwrap().0.layout,
    )
    .unwrap();
    let inside = (0..chimera_hal::FB_SIZE)
        .filter(|&i| a.0[i] != b.0[i])
        .filter(|&i| {
            let (x, y) = ((i % 240) as u16, (i / 240) as u16);
            (r.x..r.x + r.w).contains(&x) && (r.y..r.y + r.h).contains(&y)
        })
        .count();
    assert!(inside > 50, "{inside} of {d} inside {r:?}");
    // Unfocused, ALG A: MORPH moves no chart pixel.
    let (a, b) = (algo_at(0.0, None), algo_at(100.0, None));
    let inside = (0..chimera_hal::FB_SIZE)
        .filter(|&i| a.0[i] != b.0[i])
        .filter(|&i| {
            let (x, y) = ((i % 240) as u16, (i / 240) as u16);
            (r.x..r.x + r.w).contains(&x) && (r.y..r.y + r.h).contains(&y)
        })
        .count();
    assert_eq!(inside, 0);
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

/// The desktop simulator's Part 1 on `e`'s INIT, as `main` and
/// `qa::Desk` build it (`UiState::new` is `with_arena` on leaked ones),
/// edited through its console path: `ed set`, then its UI pass's
/// `ed_apply`.
struct Sim {
    ui: Box<UiState>,
    q: EdState,
}

impl Sim {
    fn new(e: EngineType) -> Sim {
        let mut ui = Box::new(UiState::new());
        let p = ui.active_part;
        ui.load_for_test(PartSource {
            part: p,
            from: PartFrom::Init(e),
        });
        Sim {
            ui,
            q: EdState::new(),
        }
    }

    fn ask(&mut self, line: &str) -> String {
        ask(&self.ui, &mut self.q, line)
    }

    /// `ed set`, applied by the next UI pass; the answer.
    fn set(&mut self, a: &str, v: &str) -> String {
        let got = self.ask(&format!("ed set {a} {v}"));
        chimera_core::bank2::ed_apply(&mut self.q, &mut self.ui);
        got
    }

    /// `ed get`'s stored value, as the editor reads it off the line.
    fn stored(&mut self, a: &str) -> f32 {
        let got = self.ask(&format!("ed get {a}"));
        got.split(' ').nth(3).unwrap().parse().unwrap()
    }

    /// Page `n` as the simulator's screen draws it: the same go, pad and
    /// focus as the mirror's `show`, then the desktop's frame and palette.
    fn render(&mut self, n: PageNo, how: Shown) -> Option<Fb> {
        let e = self.ui.project().part(self.ui.active_part).sound.engine();
        let (def, own) = pages::page_at(e, n)?;
        let p = self.ui.active_part;
        if e == EngineType::Drum {
            self.ui
                .select_pad(p, how.pad.or(own).unwrap_or(PadId::ALL[0]));
        }
        let chain = PartChain::of(&self.ui.project().part(p).sound)
            .ordered(self.ui.theme().algo_first)
            .def();
        self.ui
            .go_for_test(Location::pages(p, chain.locate(def.id)?));
        self.ui.focus_for_test(how.focus.map_or(0, Slot::index));
        // The same instant as the mirror's still: the display's own motion
        // at rest, whatever this simulator drew before (#1035).
        self.ui.still_for_test();
        // Its refusals' toasts gone: the editor's screens show none.
        self.ui.step_toast(60_000);
        self.ui.update(UiTick::for_test());
        let mut fb = Fb::new();
        self.ui.render_with_scope(
            &mut fb,
            &PerfStats::zero(),
            &[0.0; chimera_core::scope::SCOPE_LEN],
        );
        let pal = palette(&self.ui.theme());
        fb.0.iter_mut().for_each(|px| *px = pal.map_raw(*px));
        Some(fb)
    }
}

/// A seeded xorshift: the same Sound every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            v.swap(i, self.below(i + 1));
        }
    }
}

/// Every address `ed describe` lists for `e`, in its order, once each,
/// with the first `param` line's kind, min and max.
fn described(sim: &mut Sim, e: EngineType) -> Vec<(String, String, f32, f32)> {
    let mut out: Vec<(String, String, f32, f32)> = Vec::new();
    for n in 0..ed_pages(e) {
        let page = sim.ask(&format!("ed describe {} {n}", e.disk_ident()));
        for l in page.lines().filter(|l| l.starts_with("param ")) {
            let a = l.split(' ').nth(1).unwrap().to_string();
            let tail: Vec<&str> = l.rsplit_once("\" ").unwrap().1.split(' ').collect();
            if !out.iter().any(|(b, ..)| *b == a) {
                let (kind, min, max) = (tail[0], tail[2], tail[3]);
                out.push((a, kind.into(), min.parse().unwrap(), max.parse().unwrap()));
            }
        }
    }
    out
}

/// A random Sound on `e`, built on the sim's path: every described param
/// set to a random value in a random order (selectors among them, so some
/// land after the params they reset), twice over. Its values, as `ed get`
/// reads them, in describe order.
fn random_sound(e: EngineType, seed: u64) -> (Sim, Vec<(String, f32)>) {
    let mut sim = Sim::new(e);
    let mut rng = Rng(seed);
    let params = described(&mut sim, e);
    assert!(params.len() > 40, "{e:?}: {}", params.len());
    for _ in 0..2 {
        let mut order: Vec<usize> = (0..params.len()).collect();
        rng.shuffle(&mut order);
        for i in order {
            let (a, kind, min, max) = &params[i];
            let v = if kind == "cont" {
                format!("{:.6}", min + rng.unit() * (max - min))
            } else {
                format!("#{}", rng.below(*max as usize + 1))
            };
            // A choice the variant lacks is `ERR`'d, a refused turn left:
            // either way the Sound holds what it holds.
            sim.set(a, &v);
        }
    }
    let vals = params
        .iter()
        .map(|(a, ..)| (a.clone(), sim.stored(a)))
        .collect();
    (sim, vals)
}

/// The mirror's values and pixels against the sim's, every page.
fn same_as_sim(sim: &mut Sim, m: &mut Mirror, vals: &[(String, f32)], how: &str) {
    let e = m.engine();
    for (a, _) in vals {
        let get = format!("ed get {a}");
        assert_eq!(m.ed_line(&get), sim.ask(&get), "{e:?} {how}: {a}");
    }
    // Bit for bit, not to `ed get`'s text alone: what it printed read
    // back as the value the sim holds (#1035).
    for (a, _) in vals {
        let at = EdAddr::parse(a.as_bytes()).unwrap();
        let (mine, theirs) = (m.stored(addr(a)), ed_stored(&sim.ui, at));
        assert_eq!(
            mine.map(f32::to_bits),
            theirs.map(f32::to_bits),
            "{e:?} {how}: {a} is {mine:?}, the sim's {theirs:?}"
        );
    }
    let mut drawn = 0;
    for n in 0..u16::from(ed_pages(e)) {
        let shown = Shown::default();
        let mut fb = Fb::new();
        let mine = m.render(PageNo(n), shown, &mut fb).map(|()| fb);
        let theirs = sim.render(PageNo(n), shown);
        match (mine, theirs) {
            (Some(a), Some(b)) => {
                let d = differ(&a, &b);
                assert_eq!(d, 0, "{e:?} {how}: page {n} differs in {d} pixels");
                drawn += 1;
            }
            (None, None) => {}
            (a, b) => panic!(
                "{e:?} {how}: page {n} {:?} vs {:?}",
                a.is_some(),
                b.is_some()
            ),
        }
    }
    assert!(drawn > 0);
}

/// The seeds `a_render_is_the_desktop_simulators` and the stills draw
/// from: several, so no one Sound's luck passes them (#1035 hid behind
/// 0x5EED_0000 until a new param moved its draws).
const SEEDS: [u64; 4] = [0x5EED_0000, 0x5EED_0100, 0x0BAD_C0DE, 0x1035_1035];

/// A still is its Sound's, its page's and its focus's alone: the same
/// page drawn first, again, and after every other page is the same
/// picture, on Sounds whose LFOs move the cells shown (#1035: the display
/// LFOs ran on from paint to paint, so a gauge sat where the paints before
/// it left it).
#[test]
fn a_render_depends_on_nothing_drawn_before() {
    let mut moved = 0;
    for seed in SEEDS {
        for (i, e) in EngineType::PLAYABLE.into_iter().enumerate() {
            let (_, vals) = random_sound(e, seed + i as u64);
            let batch: Vec<(Addr, f32)> = vals.iter().map(|(a, v)| (addr(a), *v)).collect();
            let mut m = mirror(e);
            m.apply(&batch);
            let draw = |m: &mut Mirror, n: u16| {
                let mut fb = Fb::new();
                m.render(PageNo(n), Shown::default(), &mut fb).map(|()| fb)
            };
            let pages = u16::from(ed_pages(e)).min(40);
            let first: Vec<Option<Fb>> = (0..pages).map(|n| draw(&mut m, n)).collect();
            // A second pass, backwards: every page after all the others.
            for n in (0..pages).rev() {
                let (a, b) = (draw(&mut m, n), draw(&mut m, n));
                match (&first[usize::from(n)], a, b) {
                    (Some(f), Some(a), Some(b)) => {
                        assert_eq!(differ(f, &a), 0, "{e:?} {seed:#x}: page {n} after the rest");
                        assert_eq!(differ(&a, &b), 0, "{e:?} {seed:#x}: page {n} twice");
                    }
                    (None, None, None) => {}
                    _ => panic!("{e:?} {seed:#x}: page {n} comes and goes"),
                }
            }
            // The Sound moves something: the test would hold of a still
            // Sound whatever the mirror did.
            let sound = &m.ui().project().part(m.ui().active_part).sound;
            moved += usize::from(!sound.mod_state.is_empty());
        }
    }
    assert!(moved > 0, "no Sound here routes anything");
}

/// The mirror replays a random Sound's values, in describe order and
/// shuffled, to the sim's values and pixels on every page of every engine,
/// on each of `SEEDS`.
#[test]
fn a_render_is_the_desktop_simulators() {
    for seed in SEEDS {
        a_render_is_the_sims(seed);
    }
}

fn a_render_is_the_sims(seed: u64) {
    for (i, e) in EngineType::PLAYABLE.into_iter().enumerate() {
        let (mut sim, vals) = random_sound(e, seed + i as u64);
        let batch: Vec<(Addr, f32)> = vals.iter().map(|(a, v)| (addr(a), *v)).collect();

        let mut m = mirror(e);
        let done = m.apply(&batch);
        assert!(done.refused.is_empty(), "{e:?}: {:?}", done.refused);
        same_as_sim(&mut sim, &mut m, &vals, "describe order");

        let mut shuffled = batch.clone();
        Rng(0xC0FFEE + i as u64).shuffle(&mut shuffled);
        let mut m = mirror(e);
        let done = m.apply(&shuffled);
        assert!(done.refused.is_empty(), "{e:?}: {:?}", done.refused);
        same_as_sim(&mut sim, &mut m, &vals, "shuffled");
        println!(
            "{seed:#x} {e:?}: {} values, shuffled in {} passes",
            vals.len(),
            done.passes
        );
    }
}

/// A batch the mirror holds writes nothing: a spec's diff is what moved.
#[test]
fn a_held_batch_writes_nothing_and_a_change_writes_itself() {
    let (_, vals) = random_sound(EngineType::Drum, 7);
    let batch: Vec<(Addr, f32)> = vals.iter().map(|(a, v)| (addr(a), *v)).collect();
    let mut m = mirror(EngineType::Drum);
    assert!(m.apply(&batch).writes > 0);
    let again = m.apply(&batch);
    assert_eq!((again.writes, again.passes), (0, 1));
    let mut one = batch.clone();
    let i = one
        .iter()
        .position(|(a, _)| *a == addr("PAD4.LEVEL"))
        .unwrap();
    one[i].1 = if one[i].1 == 10.0 { 11.0 } else { 10.0 };
    assert_eq!(m.apply(&one).writes, 1);
}

/// A MACHINE after its knobs resets them; the mirror puts them back.
#[test]
fn a_selector_after_its_params_is_replayed_under_them() {
    let mut sim = Sim::new(EngineType::Drum);
    // Another machine than INIT's on pad 2: its knobs' defaults differ.
    let machine = (sim.stored("PAD2.MACHINE") as usize + 1) % Machine::ALL.len();
    assert_eq!(
        sim.set("PAD2.MACHINE", &format!("#{machine}")),
        "OK queued\n"
    );
    sim.set("PAD2.KNOB_A", "3");
    let knob = sim.stored("PAD2.KNOB_A");
    assert_eq!(knob, 3.0);
    let batch = [
        (addr("PAD2.KNOB_A"), knob),
        (addr("PAD2.MACHINE"), machine as f32),
    ];
    let mut m = mirror(EngineType::Drum);
    let done = m.apply(&batch);
    assert!(done.refused.is_empty(), "{:?}", done.refused);
    assert_eq!(
        done.passes, 3,
        "the knob, the machine resetting it, then the knob again"
    );
    assert_eq!(m.stored(addr("PAD2.KNOB_A")), Some(3.0));
    assert_eq!(m.stored(addr("PAD2.MACHINE")), Some(machine as f32));
}

/// Kits of 8 voice groups each (ADR 0280), CHOKE's options by number
/// (OFF, A, B, C, D): pads 1-2 on `pair`, 3-4 C, 5-7 D, the rest OFF.
fn eight_groups(pair: f32) -> Vec<(Addr, f32)> {
    (1..=12)
        .map(|n| {
            let g = match n {
                1 | 2 => pair,
                3 | 4 => 3.0,
                5..=7 => 4.0,
                _ => 0.0,
            };
            (addr(&format!("PAD{n}.CHOKE")), g)
        })
        .collect()
}

/// S (pads 1-2 on A) to T (on B): each step alone makes a 9th group, so
/// the box's check would never let the diff land. The unit holds T, so
/// the mirror writes it: its values are the unit's, already taken.
#[test]
fn a_diff_between_two_legal_kits_lands_past_the_group_check() {
    let mut m = mirror(EngineType::Drum);
    for (kit, pair) in [("S", 1.0), ("T", 2.0)] {
        let batch = eight_groups(pair);
        let done = m.apply(&batch);
        assert!(done.refused.is_empty(), "{kit}: {:?}", done.refused);
        for (a, v) in batch {
            assert_eq!(m.stored(a), Some(v), "{kit}: {a}");
        }
    }
}

#[test]
fn a_set_stores_what_ed_set_stores() {
    for (a, stored, typed) in [
        ("ALGO.MORPH", 64.0, "64"),
        ("OP_A.WAVE", 9.0, "SAW"),
        ("FILTER.CUTOFF", 0.37, "0.37"),
    ] {
        let mut m = mirror(EngineType::Algo);
        assert_eq!(m.set(addr(a), stored), Ok(true));
        let mut sim = Sim::new(EngineType::Algo);
        assert_eq!(sim.set(a, typed), "OK queued\n");
        let get = format!("ed get {a}");
        let mine = m.ed_line(&get);
        assert_eq!(mine, sim.ask(&get), "{a}");
        assert_ne!(
            mine,
            mirror(EngineType::Algo).ed_line(&get),
            "{a} didn't move"
        );
    }
}

/// The value's kind picks `#n` or a number: a choice takes a whole option,
/// a continuous param any finite number.
#[test]
fn a_set_the_unit_refuses_is_refused_typed() {
    let mut m = mirror(EngineType::Algo);
    let (wave, morph) = (addr("OP_A.WAVE"), addr("ALGO.MORPH"));
    assert_eq!(
        m.set(wave, 99.0),
        Err(Refused::Set(wave, SetRefusal::NoSuchOption))
    );
    assert_eq!(
        m.set(wave, 2.5),
        Err(Refused::Set(wave, SetRefusal::NoSuchOption))
    );
    assert_eq!(m.set(morph, f32::NAN), Err(Refused::NotFinite(morph)));
    assert_eq!(m.set(morph, 12.0), Ok(true));
    // Another engine's address.
    let tape = addr("TAPE_ENG.MODEL");
    assert_eq!(m.set(tape, 1.0), Err(Refused::NotHere(tape)));
    assert!(Addr::parse("NOPE.X").is_none());
}

#[test]
fn init_switches_engines_after_edits() {
    let mut m = mirror(EngineType::Algo);
    m.set(addr("ALGO.MORPH"), 50.0).unwrap();
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
    let mut fb = Fb::new();
    for e in EngineType::ALL {
        let mut m = mirror(e);
        let mut drawn = 0;
        for n in 0..u16::from(ed_pages(e)) {
            if m.render(PageNo(n), Shown::default(), &mut fb).is_some() {
                drawn += 1;
                assert!(m.region(Region::Screen).is_some());
            }
        }
        assert!(drawn > 0, "{e:?}");
    }
}

#[test]
fn a_second_render_is_the_same() {
    let mut m = mirror(EngineType::Va);
    let (mut a, mut b) = (Fb::new(), Fb::new());
    m.render(PageNo(0), Shown::default(), &mut a).unwrap();
    m.render(PageNo(0), Shown::default(), &mut b).unwrap();
    assert!(a.0 == b.0);
}

/// KIT draws the pad the spec selects, as the unit does with that pad
/// selected, never the one the mirror selected last; with none, the
/// page's own.
#[test]
fn kit_draws_the_pad_it_is_shown_with() {
    let kit = PageNo(9);
    assert_eq!(
        pages::page_at(EngineType::Drum, kit).unwrap().0.short,
        "KIT"
    );
    let pad = |n: u8| Shown {
        focus: None,
        pad: Some(PadId::new(n - 1).unwrap()),
    };
    let mut m = mirror(EngineType::Drum);
    let mut sim = Sim::new(EngineType::Drum);
    let mut five = Fb::new();
    m.render(kit, pad(5), &mut five).unwrap();
    assert_eq!(differ(&five, &sim.render(kit, pad(5)).unwrap()), 0);
    // Another pad's page, then KIT with pad 5 again: the same.
    let last = ed_pages(EngineType::Drum) as u16 - 1;
    m.render(PageNo(last), Shown::default(), &mut Fb::new())
        .unwrap();
    let mut again = Fb::new();
    m.render(kit, pad(5), &mut again).unwrap();
    assert_eq!(differ(&five, &again), 0);
    // None: PAD1_KIT's own pad.
    let mut one = Fb::new();
    m.render(kit, Shown::default(), &mut one).unwrap();
    assert!(differ(&five, &one) > 0, "pad 1 selected draws otherwise");
    assert_eq!(differ(&one, &sim.render(kit, pad(1)).unwrap()), 0);
}

/// A pad's icon is the firmware's for its machine, at the focus band's
/// scale and inks.
#[test]
fn a_pad_icon_is_its_machines() {
    let p3 = PadId::new(2).unwrap();
    let mut m = mirror(EngineType::Drum);
    for (i, &machine) in Machine::ALL.iter().enumerate() {
        m.set(addr("PAD3.MACHINE"), i as f32).unwrap();
        let mut fb = Fb::new();
        m.pad_icon(p3, &mut fb);
        let mut want = Fb::new();
        want.0
            .fill(RawU16::from(chimera_core::ui::theme::BG).into_inner());
        kit_band::icon(
            &mut want,
            machine,
            0,
            0,
            kit_band::Scale::FOCUS,
            chimera_core::ui::theme::ACCENT,
            chimera_core::ui::theme::INK2,
        );
        let pal = palette(&m.ui().theme());
        want.0.iter_mut().for_each(|px| *px = pal.map_raw(*px));
        let r = Rect::PAD_ICON;
        for y in 0..usize::from(r.h) {
            for x in 0..usize::from(r.w) {
                let k = y * 240 + x;
                assert_eq!(fb.0[k], want.0[k], "{machine:?} at {x},{y}");
            }
        }
    }
}

/// screen.js names engines by their place in the wasm's list: the
/// firmware's `PLAYABLE`, which `editor/tests/fixtures/engines.txt` holds
/// (chimera-core's `writes_editor_engines_fixture`).
#[test]
fn engine_names_are_the_fixtures() {
    // SAFETY: the shell's names, `engines_len` bytes, never changed once
    // built; read on this thread between calls.
    let names =
        unsafe { core::slice::from_raw_parts(abi::engines_ptr(), abi::engines_len() as usize) };
    let fixture = include_str!("../../editor/tests/fixtures/engines.txt");
    assert_eq!(str::from_utf8(names).unwrap(), fixture.trim_end());
}

#[test]
fn regions_sit_on_the_screen() {
    for l in [
        chimera_core::ui::page::PageLayout::BigViz,
        chimera_core::ui::page::PageLayout::CellGrid,
        chimera_core::ui::page::PageLayout::Matrix,
    ] {
        for r in [Region::Full, Region::Screen, Region::Gauge, Region::PadIcon] {
            if let Some(Rect { x, y, w, h }) = Rect::of(r, l) {
                assert!(
                    w > 0 && h > 0 && x + w <= 240 && y + h <= 320,
                    "{r:?} {l:?}"
                );
            }
        }
    }
}

/// The exports answer typed statuses, through the buffers JS writes.
#[test]
fn the_abi_answers_statuses() {
    let algo = 0;
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
    let len = put("ALGO.MORPH\nOP_A.WAVE", &[64.0, 9.0]);
    assert_eq!(abi::apply(algo, 2, len), ok);
    assert_eq!(abi::writes(), 2);
    assert_eq!(abi::apply(algo, 2, len), ok);
    assert_eq!(abi::writes(), 0, "held: nothing written");
    let len = put("OP_A.WAVE", &[99.0]);
    assert_eq!(abi::apply(algo, 1, len), i32::from(Status::Refused));
    assert_eq!(abi::refused(), 1);
    let len = put("NOPE.X", &[1.0]);
    assert_eq!(abi::apply(algo, 1, len), i32::from(Status::BadBatch));
    assert_eq!(abi::apply(algo, 2, len), i32::from(Status::BadBatch));
    assert_eq!(abi::apply(9, 0, 0), i32::from(Status::NoSuchEngine));
    let screen = Region::Screen as u8;
    assert_eq!(abi::show(algo, 0, screen, 2, 0), ok);
    assert_eq!(
        abi::show(algo, 999, screen, -1, 0),
        i32::from(Status::NotShown)
    );
    assert_eq!(
        abi::show(algo, 0, 9, -1, 0),
        i32::from(Status::NoSuchRegion)
    );
    assert_eq!(
        abi::show(algo, 0, screen, 6, 0),
        i32::from(Status::NoSuchCell)
    );
    let drum = EngineType::PLAYABLE
        .iter()
        .position(|&e| e == EngineType::Drum)
        .unwrap() as u8;
    let icon = Region::PadIcon as u8;
    assert_eq!(abi::show(drum, 0, icon, -1, 3), ok);
    assert_eq!(
        abi::show(drum, 0, icon, -1, 0),
        i32::from(Status::NoSuchPad)
    );
    assert_eq!(
        abi::show(drum, 0, screen, -1, 13),
        i32::from(Status::NoSuchPad)
    );
    assert_eq!(abi::reset(algo), ok);
    let len = put("ALGO.MORPH", &[64.0]);
    assert_eq!(abi::apply(algo, 1, len), ok);
    assert_eq!(abi::writes(), 1, "INIT again");
}
