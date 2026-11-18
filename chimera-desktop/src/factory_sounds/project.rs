//! The effects demo projects (spec § 5): `[[project]]` tables in
//! `projects.toml`, each built on NEW's template into an ordinary project
//! file, `P000000n.A`, through the firmware's own `encode_project`. The
//! FX by frozen idents (`fx.REVERB.TYPE = "ROOM"`) over NEW's; each Part
//! by public Sound name (`part.1 = "NYLON, POLY, CH 1"`), loaded from a
//! pool slot with its own sends (ADR 0123); a Part not named stays NEW's.
//! `try` is the release notes' "what to try", and `hear.BLOCK.PARAM =
//! "A, B"` adds a render setting per value, so QA plays what it invites.
//! `held = "<reason>"` keeps a demo out of the build and the renders, still
//! checked; what only held demos play is pending, not unheard.
//! Pure but for `build`'s leaked arena: text in, `Demo`, project out.

use chimera_core::MidiChannel;
use chimera_core::addr::BlockRef;
use chimera_core::block::{Block, DiskCode};
use chimera_core::dsp::chorus::{ChorusMode, MODE_NAMES};
use chimera_core::dsp::fx_bus::{FX_SENDS, FxParams};
use chimera_core::dsp::reverb::ReverbType;
use chimera_core::dsp::{Gain, SendLevel};
use chimera_core::name::ProjectName;
use chimera_core::part::PartMode;
use chimera_core::part::PartParams;
use chimera_core::project::{
    PROJECT_DEMO_TOP, PartFrom, PartId, PartSource, Project, ReplaceGuard, encode_project,
};
use chimera_core::storage::{FileKind, Generation, Header, ProjectId, ValidAddr, write_file};

use super::source::{self, Source, VecSink};
use crate::listen::lined_tables;

/// Where the demos live, in `tools/factory-sounds/`; errors name it.
pub const FILE: &str = "projects.toml";

/// The FX blocks a demo sets: the project's, not the Sound's.
const FX_BLOCKS: [BlockRef; 3] = [BlockRef::Chorus, BlockRef::Delay, BlockRef::Reverb];

/// A demo's id: `P0000001`-`P0000009`, the ids a save never takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DemoId(ProjectId);

impl DemoId {
    pub fn new(n: u32) -> Option<Self> {
        (1..=PROJECT_DEMO_TOP)
            .contains(&n)
            .then(|| ProjectId::new(n))
            .flatten()
            .map(Self)
    }

    /// `P` and seven digits.
    fn parse(v: &str) -> Option<Self> {
        let d = v.strip_prefix('P').filter(|d| d.len() == 7)?;
        d.bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| Self::new(d.parse().ok()?))
            .flatten()
    }

    pub fn get(self) -> ProjectId {
        self.0
    }

    /// `P0000001`.
    pub fn stem(self) -> String {
        String::from_utf8_lossy(&self.0.stem()).into()
    }
}

/// `part.n = "<SOUND NAME>, POLY|MONO, CH n"`.
#[derive(Clone, Debug, PartialEq)]
pub struct DemoPart {
    pub line: usize,
    pub part: PartId,
    pub sound: String,
    pub mode: PartMode,
    pub channel: MidiChannel,
}

#[derive(Clone, Debug)]
pub struct Demo {
    pub id: DemoId,
    pub id_line: usize,
    pub name: ProjectName,
    pub name_line: usize,
    /// `try`: what a player is invited to do with it.
    pub tries: String,
    /// NEW's FX with the `fx.` lines set.
    pub fx: FxParams,
    /// `fx` with one `hear` value set, a setting per value.
    pub hear: Vec<FxParams>,
    pub parts: Vec<DemoPart>,
    /// `held`: why it waits, unbuilt and unrendered (an issue, say).
    pub held: Option<String>,
}

/// What QA must hear across the demos (spec § 4): each reverb TYPE, each
/// CHORUS mode, and the delay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    Reverb(ReverbType),
    Chorus(ChorusMode),
    Delay,
}

impl Effect {
    pub const ALL: [Effect; 10] = [
        Effect::Reverb(ReverbType::Quad),
        Effect::Reverb(ReverbType::Room),
        Effect::Reverb(ReverbType::Hall),
        Effect::Reverb(ReverbType::Bloom),
        Effect::Reverb(ReverbType::Gate),
        Effect::Reverb(ReverbType::Reverse),
        Effect::Chorus(ChorusMode::JunoI),
        Effect::Chorus(ChorusMode::JunoII),
        Effect::Chorus(ChorusMode::JunoBoth),
        Effect::Delay,
    ];

    /// As the unit names it: `REVERB HALL`, `CHORUS I+II`, `DELAY`.
    pub fn label(self) -> String {
        match self {
            Effect::Reverb(t) => format!("REVERB {}", ReverbType::NAMES[t.index()]),
            Effect::Chorus(m) => format!("CHORUS {}", MODE_NAMES[m.disk_code() as usize]),
            Effect::Delay => "DELAY".into(),
        }
    }
}

/// A render setting: the FX it plays through, and what of them is heard.
#[derive(Clone, Debug)]
pub struct Setting {
    pub fx: FxParams,
    pub heard: Vec<Effect>,
}

/// Every `[[project]]` in `text`; each error names `projects.toml:line`.
pub fn parse(text: &str) -> Result<Vec<Demo>, String> {
    lined_tables(text, FILE)?
        .into_iter()
        .map(|t| {
            if t.kind != "project" {
                return Err(format!(
                    "{FILE}:{}: [[{}]], not [[project]]",
                    t.line, t.kind
                ));
            }
            demo(t.line, &t.entries)
        })
        .collect()
}

const HEAD: [&str; 4] = ["id", "name", "try", "held"];

fn at(line: usize, msg: String) -> String {
    format!("{FILE}:{line}: {msg}")
}

fn demo(header: usize, e: &[(usize, String, String)]) -> Result<Demo, String> {
    for (i, (line, k, _)) in e.iter().enumerate() {
        if e[..i].iter().any(|x| x.1 == *k) {
            return Err(at(*line, format!("{k} twice")));
        }
    }
    let need = |k: &str| {
        e.iter()
            .find(|x| x.1 == k)
            .map(|x| (x.0, x.2.as_str()))
            .ok_or_else(|| at(header, format!("a [[project]] without {k}")))
    };
    let (id_line, v) = need("id")?;
    let id = DemoId::parse(v)
        .ok_or_else(|| at(id_line, format!("id {v} is outside P0000001-P0000009")))?;
    let (name_line, v) = need("name")?;
    let name =
        ProjectName::new(v).map_err(|_| at(name_line, format!("name {v}: not a project name")))?;
    let tries = need("try")?.1.to_string();
    let held = match need("held") {
        Ok((line, v)) if v.trim().is_empty() => {
            return Err(at(line, "held: give the reason".into()));
        }
        Ok((_, v)) => Some(v.to_string()),
        Err(_) => None,
    };

    let mut fx = FxParams::DEFAULT;
    let mut hear = Vec::new();
    let mut parts: Vec<DemoPart> = Vec::new();
    for (line, k, v) in e.iter().filter(|x| !HEAD.contains(&x.1.as_str())) {
        let line = *line;
        let unknown = || at(line, format!("unknown key {k}"));
        if let Some(p) = k.strip_prefix("fx.") {
            let a = fx_addr(p).ok_or_else(unknown)?;
            set_fx(&mut fx, a, v).map_err(|m| at(line, m))?;
        } else if let Some(p) = k.strip_prefix("hear.") {
            hear.push((line, fx_addr(p).ok_or_else(unknown)?, v));
        } else if let Some(n) = k.strip_prefix("part.") {
            parts.push(part(line, k, n, v).map_err(|m| at(line, m))?);
        } else {
            return Err(unknown());
        }
    }
    // On the finished FX, whatever the lines' order; each value one the
    // `try` line names, as the unit shows it.
    let named: Vec<&str> = tries
        .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .filter(|w| !w.is_empty())
        .collect();
    let mut settings = Vec::new();
    for (line, a, values) in hear {
        for v in values.split(',').map(str::trim) {
            let mut f = fx;
            set_fx(&mut f, a, v).map_err(|m| at(line, m))?;
            let shown = shown(&f, a, v);
            if !named.contains(&shown) {
                return Err(at(
                    line,
                    format!("hear {v}: the try line does not name {shown}"),
                ));
            }
            settings.push(f);
        }
    }
    // A Part left as NEW's keeps its channel: a named Part there would
    // stack with it.
    for p in &parts {
        let stays = PartId::ALL.into_iter().find(|&o| {
            !parts.iter().any(|q| q.part == o)
                && PartParams::for_part(o.index()).channel == p.channel
        });
        if let Some(o) = stays {
            return Err(at(
                p.line,
                format!(
                    "part.{}: CH {} is Part {}'s, which stays NEW's; name Part {} or pick another channel",
                    p.part.index() + 1,
                    p.channel.get() + 1,
                    o.index() + 1,
                    o.index() + 1
                ),
            ));
        }
    }
    Ok(Demo {
        id,
        id_line,
        name,
        name_line,
        tries,
        fx,
        hear: settings,
        parts,
        held,
    })
}

/// `v` as the unit shows it: a CHORUS MODE by its page name (`I+II`).
fn shown<'a>(fx: &FxParams, a: ValidAddr, v: &'a str) -> &'a str {
    if a.addr().block == BlockRef::Chorus && a.spec().ident == "MODE" {
        MODE_NAMES[ChorusMode::from_u8(fx.chorus.mode).disk_code() as usize]
    } else {
        v
    }
}

/// `BLOCK.PARAM` of an FX block, by its frozen idents.
fn fx_addr(k: &str) -> Option<ValidAddr> {
    let (block, param) = k.split_once('.')?;
    let b = FX_BLOCKS
        .into_iter()
        .find(|b| b.disk_ident() == Some(block))?;
    ValidAddr::of_block(b).find(|a| a.spec().ident == param)
}

fn set_fx(fx: &mut FxParams, a: ValidAddr, v: &str) -> Result<(), String> {
    let blk: &mut dyn Block = match a.addr().block {
        BlockRef::Chorus => &mut fx.chorus,
        BlockRef::Delay => &mut fx.delay,
        _ => &mut fx.reverb,
    };
    source::set(blk, a, v)
}

/// `part.n`'s Part and `"<SOUND NAME>, POLY|MONO, CH n"`.
fn part(line: usize, k: &str, n: &str, v: &str) -> Result<DemoPart, String> {
    let part = n
        .parse::<usize>()
        .ok()
        .and_then(|n| PartId::ALL.get(n.checked_sub(1)?).copied())
        .ok_or_else(|| format!("{k}: part.1-part.{}", PartId::ALL.len()))?;
    let form = || format!("{k} {v:?}: \"<SOUND NAME>, POLY|MONO, CH n\"");
    // From the right: the name is everything before the mode.
    let mut it = v.rsplitn(3, ',').map(str::trim);
    let (Some(ch), Some(mode), Some(sound)) = (it.next(), it.next(), it.next()) else {
        return Err(form());
    };
    let mode = match mode {
        "POLY" => PartMode::Poly,
        "MONO" => PartMode::Mono,
        m => return Err(format!("{k}: {m}: POLY or MONO")),
    };
    let channel = ch
        .strip_prefix("CH ")
        .and_then(|c| c.parse::<u8>().ok())
        .filter(|c| (1..=16).contains(c))
        .and_then(|c| MidiChannel::new(c - 1))
        .ok_or_else(|| format!("{k}: {ch}: CH 1-16"))?;
    Ok(DemoPart {
        line,
        part,
        sound: sound.into(),
        mode,
        channel,
    })
}

/// Ids and names unique, and each Part's Sound one the public set builds.
pub fn check(demos: &[Demo], sounds: &[Source]) -> Result<(), String> {
    for (i, d) in demos.iter().enumerate() {
        if demos[..i].iter().any(|o| o.id == d.id) {
            return Err(at(d.id_line, format!("id {} twice", d.id.stem())));
        }
        if demos[..i].iter().any(|o| o.name == d.name) {
            return Err(at(d.name_line, format!("name {} twice", d.name.as_str())));
        }
        for p in &d.parts {
            sound_of(p, sounds)?;
        }
    }
    Ok(())
}

/// The public Sound `p` names. A private port (a `from` line) is refused
/// by name: the demos ship in the public set.
fn sound_of<'a>(p: &DemoPart, sounds: &'a [Source]) -> Result<&'a Source, String> {
    let s = sounds
        .iter()
        .find(|s| s.name.as_str() == p.sound)
        .ok_or_else(|| at(p.line, format!("no Sound named {}", p.sound)))?;
    if s.from.is_some() {
        return Err(at(
            p.line,
            format!(
                "{} is a private Sound; a demo holds public Sounds only",
                p.sound
            ),
        ));
    }
    if s.is_placeholder() {
        return Err(at(
            p.line,
            format!("{} is a placeholder: not built", p.sound),
        ));
    }
    Ok(s)
}

/// The least gain into an effect that counts as hearing it (−20 dB).
pub const MIN_SEND: Gain = Gain::new(0.1);

/// What `fx` plays audibly when Parts send `sends` (each effect's most).
/// An effect is heard when `MIN_SEND` or more reaches it, its return is
/// above 0 and it is on; the reverb's send includes the delay's REV SEND.
pub fn heard(fx: &FxParams, sends: [SendLevel; FX_SENDS]) -> Vec<Effect> {
    let fed = |s: SendLevel| s.gain() >= MIN_SEND;
    let [chorus, delay, reverb] = sends;
    let delay_on = fed(delay) && fx.delay.mix > 0.0;
    let reverb_in = fed(reverb) || (delay_on && fed(fx.delay.rev_send));
    let mode = ChorusMode::from_u8(fx.chorus.mode);
    let mut out = Vec::new();
    if fed(chorus) && fx.chorus.mix > 0.0 && mode != ChorusMode::Off {
        out.push(Effect::Chorus(mode));
    }
    if delay_on {
        out.push(Effect::Delay);
    }
    if reverb_in && fx.reverb.is_on() {
        out.push(Effect::Reverb(fx.reverb.kind));
    }
    out
}

/// Its own FX, then each `hear` setting, with what each is heard through.
pub fn settings(d: &Demo, sounds: &[Source]) -> Result<Vec<Setting>, String> {
    let mut sends = [SendLevel::OFF; FX_SENDS];
    for p in &d.parts {
        let s = source::to_sound(sound_of(p, sounds)?)?.params.out.sends;
        for (m, v) in sends.iter_mut().zip(s) {
            if v > *m {
                *m = v;
            }
        }
    }
    Ok(std::iter::once(d.fx)
        .chain(d.hear.iter().copied())
        .map(|fx| Setting {
            fx,
            heard: heard(&fx, sends),
        })
        .collect())
}

/// What the demos play of `Effect::ALL`, in its order: `unheard` by any
/// demo (a failure), and `pending` on held demos alone (each with them).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Coverage {
    pub unheard: Vec<Effect>,
    pub pending: Vec<(Effect, Vec<DemoId>)>,
}

impl Coverage {
    /// `pending: REVERB HALL (held: P0000001)`, one per pending effect.
    pub fn pending_lines(&self) -> Vec<String> {
        self.pending
            .iter()
            .map(|(e, ids)| {
                let ids: Vec<String> = ids.iter().map(|i| i.stem()).collect();
                format!("pending: {} (held: {})", e.label(), ids.join(", "))
            })
            .collect()
    }
}

/// Each effect against every demo's settings, held ones apart.
pub fn coverage(demos: &[Demo], sounds: &[Source]) -> Result<Coverage, String> {
    let mut live = Vec::new();
    let mut held: Vec<(DemoId, Vec<Effect>)> = Vec::new();
    for d in demos {
        let seen = settings(d, sounds)?.into_iter().flat_map(|s| s.heard);
        match d.held {
            None => live.extend(seen),
            Some(_) => held.push((d.id, seen.collect())),
        }
    }
    let mut c = Coverage::default();
    for e in Effect::ALL.into_iter().filter(|e| !live.contains(e)) {
        let ids: Vec<DemoId> = held
            .iter()
            .filter(|h| h.1.contains(&e))
            .map(|h| h.0)
            .collect();
        if ids.is_empty() {
            c.unheard.push(e);
        } else {
            c.pending.push((e, ids));
        }
    }
    Ok(c)
}

/// `d` on NEW's template: its name and FX, then each Part loaded from a
/// pool slot holding its Sound, with its mode and channel. The slot is
/// found by a library load's own check (`Project::needs_for`): any slot
/// already holding the Sound (by `sound_crc`, NEW's own included), else
/// the first free one, refused unless the arena fits it.
pub fn build(d: &Demo, sounds: &[Source]) -> Result<Box<Project>, String> {
    let (mut p, template) = Project::boxed();
    p.set_name(d.name);
    *p.edit_fx() = d.fx;
    for dp in &d.parts {
        let src = sound_of(dp, sounds)?;
        let snd = source::to_sound(src)?;
        let fail = |m: String| at(dp.line, m);
        let no_slot = || fail("no free pool slot".into());
        let slot = match p.needs_for(&snd, None) {
            Ok(Some(s)) => s,
            Ok(None) => {
                let s = p.pool().first_free().ok_or_else(no_slot)?;
                p.pool_store(s, &snd).map_err(|e| fail(format!("{e:?}")))?;
                s
            }
            Err(None) => return Err(no_slot()),
            Err(Some(full)) => {
                return Err(fail(format!(
                    "the pool has no room: needs {:?}, free {:?}",
                    full.needs, full.free
                )));
            }
        };
        let to = PartSource {
            part: dp.part,
            from: PartFrom::Slot(slot),
        };
        let c = ReplaceGuard::check(&p, template, to)
            .map_err(|n| fail(format!("the load asks {:?}", n.prompt())))?;
        p.replace_part(c).map_err(|e| fail(format!("{e:?}")))?;
        let e = p.edit_part(dp.part);
        e.mix.set_mode(dp.mode);
        e.mix.channel = dp.channel;
    }
    Ok(p)
}

/// `p` as a first-generation project file, as a save writes one.
pub fn file_bytes(p: &Project) -> Result<Vec<u8>, String> {
    let h = Header {
        kind: FileKind::Project,
        generation: Generation::FIRST,
        name: Some(p.meta().name()),
    };
    let mut sink = VecSink(Vec::new());
    write_file(&mut sink, &h, &mut |w| encode_project(p, w))
        .map_err(|e| format!("{}: {e:?}", p.meta().name().as_str()))?;
    Ok(sink.0)
}
