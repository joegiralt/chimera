//! TINZ, the owner's TAPE Sound (saved on the unit, build 14fecc46): the
//! file as the firmware decodes it, and the factory build of its recipe,
//! param for param.

use chimera_core::addr::{BlockRead, BlockRef};
use chimera_core::modulation::{FixedCol, ModSource};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::storage::{Check, Decode, Framer, SoundDecoder, sound_crc};
use std::fmt::Write as _;

/// The owner's file, as `get` fetched it (decoded from its base64).
const TINZ: &[u8] = include_bytes!("fixtures/TINZ.A");

/// `bytes` decoded as a load decodes them: a check pass, then apply.
fn load(bytes: &[u8]) -> Sound {
    let mut s = Sound::neutral(EngineType::Algo);
    for apply in [false, true] {
        let mut d = SoundDecoder::new(&mut s);
        let mut f = Framer::new(bytes.len() as u32).unwrap();
        f.push(bytes, &mut |e| if apply { d.apply(e) } else { d.event(e) })
            .unwrap();
        f.finish().unwrap();
        if apply { d.commit() } else { d.end() }.unwrap();
    }
    s
}

/// Every stored param, `*` where it is off `Sound::neutral`, then the
/// SOURCE, routes, registry and MIDI map.
fn dump(s: &Sound) -> String {
    let neutral = Sound::neutral(s.engine());
    let mut out = format!("name {}\nengine {:?}\n", s.name.as_str(), s.engine());
    for b in BlockRef::ALL {
        let Some(blk) = s.params.block(b) else {
            continue;
        };
        let n = neutral.params.block(b);
        for spec in blk.specs().iter().filter(|p| p.stored) {
            let v = blk.get(spec.id);
            let off = n.as_ref().is_none_or(|n| n.get(spec.id) != v);
            let shown = blk
                .enum_ident(spec.id)
                .map_or(format!("{v}"), |i| format!("{i} ({v})"));
            let _ = writeln!(
                out,
                "{} {:?}.{} = {shown}",
                if off { "*" } else { " " },
                b,
                spec.label
            );
        }
    }
    match s.source {
        Some(t) => {
            let _ = writeln!(
                out,
                "source crc {:08X} frames {} rate {} tune {:?}",
                t.r.crc, t.r.frames, t.r.rate, t.tune
            );
        }
        None => out.push_str("source none\n"),
    }
    for i in 0..s.dest_registry.len() {
        if let Some(e) = s.dest_registry.get(i) {
            let _ = writeln!(
                out,
                "registry {:?}.{} \"{}\"",
                e.addr.block,
                e.addr.param.0,
                e.label_str()
            );
        }
    }
    let m = &s.mod_state;
    let _ = writeln!(out, "mod sources {}", m.num_sources());
    for d in 0..m.num_dests() {
        let a = m.dest(d);
        for src in ModSource::ALL.into_iter().take(m.num_sources()) {
            if m.present(d) & 1 << src.index() != 0 {
                let _ = writeln!(
                    out,
                    "route {} -> {:?}.{} {}",
                    src.name(),
                    a.block,
                    label(a.block, a.param.0),
                    m.amount(src.index(), d)
                );
            }
        }
    }
    for c in FixedCol::ALL {
        for src in ModSource::ALL.into_iter().take(m.num_sources()) {
            if m.fixed_present(c) & 1 << src.index() != 0 {
                let _ = writeln!(
                    out,
                    "route {} -> {} {}",
                    src.name(),
                    c.tag(),
                    m.fixed_amount(src.index(), c)
                );
            }
        }
    }
    for (op, c, a) in s.params.taps.entries() {
        let _ = writeln!(out, "tap {op:?} -> {} {a}", c.tag());
    }
    for e in s.map.entries() {
        let _ = writeln!(
            out,
            "map {:?} -> {:?}.{} {}..{}",
            e.input,
            e.target.addr().block,
            label(e.target.addr().block, e.target.addr().param.0),
            e.span.min,
            e.span.max
        );
    }
    out
}

fn label(b: BlockRef, id: u8) -> &'static str {
    b.specs()
        .iter()
        .find(|s| s.id.0 == id)
        .map_or("?", |s| s.label)
}

/// Writes the decoded TINZ to `$TINZ_DUMP` (the ledger's `tinz.txt`).
#[test]
#[ignore = "writes a file: TINZ_DUMP=path"]
fn dump_tinz() {
    let s = load(TINZ);
    let text = dump(&s);
    std::fs::write(std::env::var("TINZ_DUMP").unwrap(), text).unwrap();
}

/// The factory build of TINZ's recipe is the owner's file, param for param.
#[test]
fn factory_tinz_is_the_owners() {
    use chimera_desktop::factory_tapes::{audio, originals, read_recipes, source, tape_sound};
    let r = read_recipes().unwrap();
    let s = r.sounds.iter().find(|s| s.name.as_str() == "TINZ").unwrap();
    let src = r.sources.iter().find(|x| x.name == s.source).unwrap();
    let built = source(src, audio(src, &originals()).unwrap().unwrap()).unwrap();
    let (built, owner) = (tape_sound(s, &built.head), load(TINZ));
    assert_eq!(sound_crc(&built), sound_crc(&owner), "the files' records");
    let (ours, theirs) = (dump(&built), dump(&owner));
    if ours != theirs {
        for (a, b) in ours.lines().zip(theirs.lines()).filter(|(a, b)| a != b) {
            eprintln!("built {a}\nowner {b}");
        }
        eprintln!(
            "lines built {} owner {}",
            ours.lines().count(),
            theirs.lines().count()
        );
        panic!("TINZ differs from the owner's file");
    }
}

/// `snd` on `b`, `(seconds, note, velocity; 0 is off)` through P1 for
/// `secs`: the DAC's first pair after the limiter, and the most gain
/// reduction the limiter took, dB (as factory-sounds' QA meters it).
fn play(
    snd: &Sound,
    b: &chimera_desktop::import::tape::Built,
    ev: &[(f32, u8, u8)],
    secs: f32,
) -> (Vec<f32>, f32) {
    use chimera_core::dsp::fx_bus::FxBus;
    use chimera_core::dsp::limiter::OUTPUT_GAIN;
    use chimera_core::hw::SampleBudget;
    use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
    use chimera_core::note_queue::{NoteEvent, NoteKind};
    use chimera_core::project::Project;
    use chimera_core::scope::{ScopeWriter, scope_buffer};
    use chimera_core::{MidiNote, Velocity};
    use chimera_desktop::factory_tapes::held;
    use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};

    let (mut p, _) = Project::boxed();
    let id = p.source_store(b.head.sref, &held(b)).unwrap();
    let mut tok = p.pool().table().reader().unwrap();
    let mut shared = Box::<AudioShared>::default();
    *shared.parts[0].params = snd.params.clone();
    *shared.parts[0].mod_state = snd.mod_state.clone();
    shared.parts[0].source = Some(id);
    let mut inst = Box::new(Instrument::new(
        SAMPLE_RATE,
        SampleBudget::for_cpu(u32::MAX),
    ));
    inst.land_knobs();
    let (mut fx, mut dac) = (Box::new(FxBus::new()), Box::new(DacBlocks::new()));
    let mut scope = ScopeWriter::new(Box::leak(Box::new(scope_buffer())).split().0);
    let ch = shared.parts[0].mix.channel;
    let sr = SAMPLE_RATE as f32;
    let energy = |p: &[f32]| p.iter().map(|v| v * v).sum::<f32>();
    let (mut out, mut next, mut reduced, mut prev_in) = (Vec::new(), 0, 0f32, 0f32);
    for blk in 0..(secs * sr) as usize / BLOCK_SIZE {
        let t = (blk * BLOCK_SIZE) as f32 / sr;
        let view = tok.begin_block();
        while next < ev.len() && ev[next].0 <= t {
            let (_, note, vel) = ev[next];
            let kind = Velocity::new(vel).map_or(NoteKind::Off, NoteKind::On);
            let note = MidiNote::new(note).unwrap();
            inst.handle_with(
                NoteEvent {
                    channel: ch,
                    note,
                    kind,
                },
                &shared,
                &view,
            );
            next += 1;
        }
        inst.render_with(&mut fx, &mut dac, &shared, &mut scope, &view);
        tok.store_sounding(inst.source_mask());
        let o = &dac.out()[0];
        if prev_in > 1e-12 {
            let gr = 20.0
                * ((energy(o) / prev_in).sqrt() / OUTPUT_GAIN)
                    .max(1e-9)
                    .log10();
            reduced = reduced.max(-gr);
        }
        prev_in = energy(&dac.input()[0]);
        out.extend_from_slice(o);
    }
    (out, reduced)
}

/// The factory TINZ through the whole instrument (P1, its own sends, the
/// limiter), raw levels: the lead phrase from C4 at its velocities, to
/// `$TINZ_WAV` (stereo, 48 kHz) for an A/B by ear against the unit; and
/// factory-sounds' gates on it (limiter, DC, finite; aliasing on a held
/// C6 and C7).
#[test]
#[ignore = "writes a file: TINZ_WAV=path"]
fn render_tinz() {
    use chimera_desktop::factory_sounds::phrase::phrase_plan;
    use chimera_desktop::factory_sounds::qa::measure::{
        ALIAS_NOTES, aliasing, dc_db, finite, limiter_bar,
    };
    use chimera_desktop::factory_sounds::source::Phrase;
    use chimera_desktop::factory_tapes::{audio, originals, read_recipes, source, tape_sound};

    let r = read_recipes().unwrap();
    let s = r.sounds.iter().find(|s| s.name.as_str() == "TINZ").unwrap();
    let src = r.sources.iter().find(|x| x.name == s.source).unwrap();
    let b = source(src, audio(src, &originals()).unwrap().unwrap()).unwrap();
    let snd = tape_sound(s, &b.head);
    let plan = phrase_plan(Phrase::Lead, 60);
    let mut ev: Vec<(f32, u8, u8)> = plan
        .hits
        .iter()
        .flat_map(|h| [(h.on, h.note, h.vel), (h.off, h.note, 0)])
        .collect();
    // Offs before ons at the same time, so a re-struck note sounds.
    ev.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.2.cmp(&b.2)));
    let (out, reduced) = play(&snd, &b, &ev, plan.secs);
    let peak = out.iter().fold(0f32, |m, v| m.max(v.abs()));
    let last_off = plan.hits.iter().map(|h| h.off).fold(0.0, f32::max);
    println!(
        "lead: peak {:.2} dBFS, limiter {reduced:.2} dB (bar {}), DC {:.1} dB, non-finite {}",
        20.0 * peak.max(1e-9).log10(),
        limiter_bar(None),
        dc_db(&out, 0, (last_off * 48_000.0) as usize),
        finite(&out).0
    );
    for n in ALIAS_NOTES {
        let (x, red) = play(&snd, &b, &[(0.0, n, 100), (1.5, n, 0)], 1.5);
        let mono: Vec<f32> = x.chunks(2).map(|f| 0.5 * (f[0] + f[1])).collect();
        // PITCH −12: it sounds an octave under the key.
        let f0 = 440.0 * 2f32.powf((f32::from(n) - 12.0 - 69.0) / 12.0);
        let (v, verdict) = aliasing(&mono[24_000..], f0, true);
        println!("note {n}: harmonic over inharmonic {v:.1} dB {verdict:?}, limiter {red:.2} dB");
    }
    chimera_desktop::listen::write_wav_stereo(
        std::path::Path::new(&std::env::var("TINZ_WAV").unwrap()),
        &out,
    )
    .unwrap();
}
