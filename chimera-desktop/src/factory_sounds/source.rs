//! A factory Sound's text Source (spec § 3, ADR 0134): `[[sound]]` tables
//! read by `listen::lined_tables`, keyed by the frozen idents of
//! `disk_codes_v1.txt`. Pure: text in, `Source`, `Sound` and card bytes
//! out; the files are `build`'s.

use chimera_core::addr::{BlockRead, BlockRef, Blocks, Op, ParamAddr};
use chimera_core::block::{Block, DiskCode, ParamSpec, ValFmt};
use chimera_core::mod_path::LABEL_LEN;
use chimera_core::modulation::{FixedCol, ModSource};
use chimera_core::name::SoundName;
use chimera_core::params::EngineType;
use chimera_core::part::PartMode;
use chimera_core::preset::Sound;
use chimera_core::storage::{
    BuildStamp, Crc32, DiskValue, FactoryId, FileKind, Generation, Header, LibId, LibKind, Note,
    ValidAddr, encode_library_sound, read_value, sound_crc, write_file,
};
use chimera_core::taps::TapOp;
use chimera_core::ui::browser::{NOTE_W, note_width};
use chimera_core::ui::fmt::{FmtBuf, fmt_val};
use chimera_core::ui::mod_grid::block_tag;
use chimera_core::ui::view::{SlotCtx, shown_fmt};
use chimera_hal::store::{ByteSink, StoreError};

use crate::listen::{lined_tables, value_line};
use crate::privacy::PORT_IDS;

/// ADR 0109's reference (ADR 0063's at the master gain): ALGO INIT's C4
/// at velocity 100 on P1.
pub const REFERENCE_LUFS: f32 = -11.0;
/// The loudness gate's half-width.
pub const LOUDNESS_LU: f32 = 2.0;

/// The showcase's id block (ADR 0133); TAPE's is below it.
/// Formant placeholders the owner retired: never reassigned.
const RETIRED_IDS: core::ops::RangeInclusive<u32> = 9_100_028..=9_100_030;
pub const SHOWCASE_IDS: core::ops::RangeInclusive<u32> = 9_100_001..=9_199_999;

/// A render script (spec § 4); the QA task plays them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Script {
    Bass,
    Stab,
    Short,
    Long,
    Texture,
    Glide,
}

impl Script {
    pub const ALL: [Script; 6] = [
        Script::Bass,
        Script::Stab,
        Script::Short,
        Script::Long,
        Script::Texture,
        Script::Glide,
    ];

    pub const fn word(self) -> &'static str {
        match self {
            Script::Bass => "bass",
            Script::Stab => "stab",
            Script::Short => "short",
            Script::Long => "long",
            Script::Texture => "texture",
            Script::Glide => "glide",
        }
    }
}

/// The phrase a Sound is auditioned over on the owner's page (`qa`'s
/// phrase tables): a listening render, never a gate. Every VA Sound names
/// one (`phrase = "bass" | "lead" | "pad" | "arp" | "wonky" | "sweep"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phrase {
    Bass,
    Lead,
    Pad,
    /// A syncopated arpeggio / broken-beat lead figure.
    Arp,
    /// A wonky melodic bassline: odd accents, legato slides.
    Wonky,
    /// Long held chords over twelve bars, each swelling the filter open and
    /// closed (`phrase::sweeps` holds the swell to the phrase).
    Sweep,
}

impl Phrase {
    pub const ALL: [Phrase; 6] = [
        Phrase::Bass,
        Phrase::Lead,
        Phrase::Pad,
        Phrase::Arp,
        Phrase::Wonky,
        Phrase::Sweep,
    ];

    pub const fn word(self) -> &'static str {
        match self {
            Phrase::Bass => "bass",
            Phrase::Lead => "lead",
            Phrase::Pad => "pad",
            Phrase::Arp => "arp",
            Phrase::Wonky => "wonky",
            Phrase::Sweep => "sweep",
        }
    }
}

/// A route into a destination the matrix (or a tap) reaches, with the
/// 1-based line it was written on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Route {
    pub line: usize,
    pub dest: ParamAddr,
    pub amount: i8,
}

#[derive(Clone, Debug)]
pub struct Source {
    pub file: String,
    pub id: FactoryId,
    pub id_line: usize,
    pub name: SoundName,
    pub name_line: usize,
    pub engine: EngineType,
    pub note: Note,
    pub demo: PartMode,
    pub plays: Vec<Script>,
    /// The phrase it is auditioned over; a VA Sound must name one.
    pub phrase: Option<Phrase>,
    /// The MIDI note its level and aliasing are measured at (C4 = 60).
    pub home: u8,
    /// `harmonic = "yes" | "no"`: a harmonic spectrum runs the aliasing
    /// gate. VA's default is yes; ALGO's and MODAL's no (bells, modes).
    pub harmonic: bool,
    /// `(line, param, value)` in file order; a value resolves in `patch`,
    /// where an enum's ident may hang on another param (MODE on KIND).
    pub params: Vec<(usize, ValidAddr, String)>,
    pub mods: Vec<(ModSource, Vec<Route>)>,
    /// A tap's routes, each into a fixed column.
    pub taps: Vec<(TapOp, Vec<(Route, FixedCol)>)>,
    /// dB on OUT LEVEL, and its line (0: none written).
    pub trim: f32,
    pub trim_line: usize,
    /// Provenance, ports only (spec § 3): `(line, from)`.
    pub from: Option<(usize, PortFrom)>,
    /// What a port changes, by category (ports only).
    pub edits: Option<Edits>,
    /// `level = "by ear -16.2: <why>"`: a level set off-centre by ear,
    /// pinned to its LUFS.
    pub level: Option<ByEar>,
    pub status: Status,
}

/// A port's provenance: `from = "tx81z <bank sha-256, first 8 hex> #<voice>"`,
/// never the Yamaha voice name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortFrom {
    /// The bank file's SHA-256, its first 4 bytes.
    pub bank: u32,
    /// 1-32, as the TX81Z numbers a bank's voices.
    pub voice: u8,
}

/// A category a port's `edits` names (spec § 3; formant by owner
/// direction 2026-10-13: ops 5-6 as a formant layer).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    Filter,
    Fold,
    Lfo,
    Envelope,
    Effects,
    Formant,
}

impl Edit {
    pub const ALL: [Edit; 6] = [
        Edit::Filter,
        Edit::Fold,
        Edit::Lfo,
        Edit::Envelope,
        Edit::Effects,
        Edit::Formant,
    ];

    /// As `edits` writes it, any case.
    pub const fn word(self) -> &'static str {
        match self {
            Edit::Filter => "filter",
            Edit::Fold => "fold",
            Edit::Lfo => "lfo",
            Edit::Envelope => "envelope",
            Edit::Effects => "effects",
            Edit::Formant => "formant",
        }
    }
}

/// `edits = "filter: 2040 LP4; lfo: slower"`: at least one category, each
/// with what changed.
#[derive(Clone, Debug, PartialEq)]
pub struct Edits {
    pub line: usize,
    /// As written, for the page.
    pub text: String,
    pub items: Vec<(Edit, String)>,
}

/// A level set by ear (`level = "by ear -16.2: <why>"`): the loudness gate
/// holds it within `measure::PIN_LU` of `lufs`. One inside the −11 ± 2 LU band
/// is pointless and fails `check`.
#[derive(Clone, Debug, PartialEq)]
pub struct ByEar {
    pub line: usize,
    pub lufs: f32,
    pub why: String,
}

/// The owner's listening round: `round 2`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Round(pub u8);

impl core::fmt::Display for Round {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "ROUND {}", self.0)
    }
}

/// Where a Source stands: a placeholder (reserves its id and name, never
/// built), NEW, or approved in a round as heard with `approval_crc` `crc`
/// (`approved = "round 2 1A2B3C4D"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Placeholder,
    New,
    Approved { round: Round, crc: u32 },
}

/// Where a Source stands with the owner, as the listening page badges it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Badge {
    Approved,
    New,
    Placeholder,
}

impl Badge {
    pub const fn word(self) -> &'static str {
        match self {
            Badge::Approved => "APPROVED",
            Badge::New => "NEW",
            Badge::Placeholder => "PLACEHOLDER",
        }
    }
}

impl Source {
    pub fn at(&self, line: usize) -> String {
        format!("{}:{line}", self.file)
    }

    pub fn is_placeholder(&self) -> bool {
        self.status == Status::Placeholder
    }

    /// Its badge, the Sound now built having `crc`: APPROVED only while
    /// it is the Sound approved.
    pub fn badge(&self, crc: u32) -> Badge {
        match self.status {
            Status::Placeholder => Badge::Placeholder,
            Status::Approved { crc: c, .. } if c == crc => Badge::Approved,
            _ => Badge::New,
        }
    }

    /// The round it was approved in, when the Sound has changed since.
    pub fn changed(&self, crc: u32) -> Option<Round> {
        match self.status {
            Status::Approved { round, crc: c } if c != crc => Some(round),
            _ => None,
        }
    }
}

/// The built Sound's `sound_crc` (the manifest's `sound`).
pub fn crc(s: &Source) -> Result<u32, String> {
    Ok(sound_crc(&to_sound(s)?))
}

/// What `approved` binds: what the owner heard. CRC-32 over the Sound's
/// `sound_crc` (4 bytes, little-endian), then the render-shaping fields
/// as text, `demo=poly\nhome=48\nplays=long texture\nphrase=pad\n`
/// (`phrase=-` without one). Changing any of them demotes the Sound.
pub fn approval_crc(s: &Source) -> Result<u32, String> {
    let plays: Vec<&str> = s.plays.iter().map(|p| p.word()).collect();
    let text = format!(
        "demo={}\nhome={}\nplays={}\nphrase={}\n",
        match s.demo {
            PartMode::Mono => "mono",
            PartMode::Poly => "poly",
        },
        s.home,
        plays.join(" "),
        s.phrase.map_or("-", Phrase::word)
    );
    let mut c = Crc32::new();
    c.update(&crc(s)?.to_le_bytes());
    c.update(text.as_bytes());
    Ok(c.finish())
}

/// `round <n> <CRC>`, the CRC 8 uppercase hex digits.
fn approval(v: &str) -> Option<Status> {
    let (round, hex) = v.rsplit_once(' ')?;
    let n = round
        .strip_prefix("round ")
        .filter(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))?
        .parse::<u8>()
        .ok()
        .filter(|&n| n > 0)?;
    let ok = hex.len() == 8 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'A'..=b'F'));
    Some(Status::Approved {
        round: Round(n),
        crc: u32::from_str_radix(hex, 16).ok().filter(|_| ok)?,
    })
}

/// `by ear <LUFS>: <why>`.
fn by_ear(v: &str, line: usize) -> Option<ByEar> {
    let (lufs, why) = v.strip_prefix("by ear ")?.split_once(':')?;
    Some(ByEar {
        line,
        lufs: lufs.parse::<f32>().ok().filter(|l| l.is_finite())?,
        why: Some(why.trim()).filter(|w| !w.is_empty())?.into(),
    })
}

/// Every `[[sound]]` in `text`; each error names `file:line`.
pub fn parse(text: &str, file: &str) -> Result<Vec<Source>, String> {
    lined_tables(text, file)?
        .into_iter()
        .map(|t| {
            if t.kind != "sound" {
                return Err(format!("{file}:{}: [[{}]], not [[sound]]", t.line, t.kind));
            }
            sound(file, t.line, &t.entries)
        })
        .collect()
}

const HEAD: [&str; 7] = ["id", "name", "engine", "note", "demo", "plays", "home"];

fn sound(file: &str, header: usize, e: &[(usize, String, String)]) -> Result<Source, String> {
    let at = |line: usize, msg: String| format!("{file}:{line}: {msg}");
    for (i, (line, k, _)) in e.iter().enumerate() {
        if e[..i].iter().any(|x| x.1 == *k) {
            return Err(at(*line, format!("{k} twice")));
        }
    }
    let need = |k: &str| {
        e.iter()
            .find(|x| x.1 == k)
            .map(|x| (x.0, x.2.as_str()))
            .ok_or_else(|| at(header, format!("a [[sound]] without {k}")))
    };

    let (id_line, v) = need("id")?;
    let id = match LibId::parse(v).map(LibId::kind) {
        Some(LibKind::Factory(f)) if RETIRED_IDS.contains(&f.get()) => {
            return Err(at(
                id_line,
                format!("id {v} is retired and never reassigned"),
            ));
        }
        Some(LibKind::Factory(f)) if SHOWCASE_IDS.contains(&f.get()) => f,
        _ => return Err(at(id_line, format!("id {v} is outside S9100001-S9199999"))),
    };
    let (name_line, v) = need("name")?;
    let name =
        SoundName::new(v).map_err(|_| at(name_line, format!("name {v}: not a Sound name")))?;
    let (l, v) = need("engine")?;
    let engine = [EngineType::Va, EngineType::Algo, EngineType::Modal]
        .into_iter()
        .find(|e| e.disk_ident() == v)
        .ok_or_else(|| at(l, format!("engine {v}: VA, ALGO or MODAL")))?;
    let (l, v) = need("note")?;
    let note = description(v).map_err(|m| at(l, m))?;
    let (l, v) = need("demo")?;
    let demo = match v {
        "mono" => PartMode::Mono,
        "poly" => PartMode::Poly,
        _ => return Err(at(l, format!("demo {v}: mono or poly"))),
    };
    let (l, v) = need("plays")?;
    let plays = v
        .split_whitespace()
        .map(|w| {
            Script::ALL
                .into_iter()
                .find(|s| s.word() == w)
                .ok_or_else(|| at(l, format!("plays {w}: not a script")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let (l, v) = need("home")?;
    let home = note_number(v).ok_or_else(|| at(l, format!("home {v}: not a note (C4 is 60)")))?;

    let mut s = Source {
        file: file.into(),
        id,
        id_line,
        name,
        name_line,
        engine,
        note,
        demo,
        plays,
        phrase: None,
        home,
        harmonic: engine == EngineType::Va,
        params: Vec::new(),
        mods: Vec::new(),
        taps: Vec::new(),
        trim: 0.0,
        trim_line: 0,
        from: None,
        edits: None,
        level: None,
        status: Status::New,
    };
    let mut placeholder = false;
    let mut approved = None;
    for (line, k, v) in e.iter().filter(|x| !HEAD.contains(&x.1.as_str())) {
        let line = *line;
        let unknown = || at(line, format!("unknown key {k}"));
        match k.as_str() {
            "trim" => {
                s.trim_line = line;
                s.trim = v
                    .parse::<f32>()
                    .ok()
                    .filter(|t| t.is_finite())
                    .ok_or_else(|| at(line, format!("trim {v}: dB")))?;
            }
            "phrase" => {
                s.phrase = Some(Phrase::ALL.into_iter().find(|p| p.word() == v).ok_or_else(
                    || {
                        at(
                            line,
                            format!("phrase {v}: bass, lead, pad, arp, wonky or sweep"),
                        )
                    },
                )?)
            }
            "harmonic" => {
                s.harmonic = match v.as_str() {
                    "yes" => true,
                    "no" => false,
                    _ => return Err(at(line, format!("harmonic {v}: yes or no"))),
                }
            }
            "from" => {
                s.from = Some((
                    line,
                    port_from(v).ok_or_else(|| {
                        at(
                            line,
                            format!("from {v}: \"tx81z <bank sha-256, first 8 hex> #<voice 1-32>\""),
                        )
                    })?,
                ))
            }
            "edits" => {
                s.edits = Some(Edits {
                    line,
                    text: v.clone(),
                    items: edits(v).ok_or_else(|| {
                        at(
                            line,
                            format!(
                                "edits {v}: \"<category>: <what>; …\", each category filter, fold, LFO, envelope, effects or formant"
                            ),
                        )
                    })?,
                })
            }
            "level" => {
                s.level = Some(
                    by_ear(v, line)
                        .ok_or_else(|| at(line, format!("level {v}: \"by ear <LUFS>: <why>\"")))?,
                )
            }
            "approved" => {
                let st = approval(v).ok_or_else(|| {
                    at(
                        line,
                        format!(
                            "approved {v}: \"round <n> <CRC>\", the CRC 8 uppercase hex digits"
                        ),
                    )
                })?;
                approved = Some((line, st));
            }
            "status" if v == "placeholder" => placeholder = true,
            "status" => return Err(at(line, format!("status {v}: placeholder"))),
            _ => {
                let (head, rest) = k.split_once('.').ok_or_else(unknown)?;
                let r = || routes(v, line).map_err(|m| at(line, m));
                match head {
                    "mod" => {
                        let src = ModSource::ALL
                            .into_iter()
                            .find(|m| m.disk_ident() == rest)
                            .ok_or_else(unknown)?;
                        s.mods.push((src, r()?));
                    }
                    "tap" => {
                        let op = TapOp::ALL
                            .into_iter()
                            .find(|t| t.tag() == rest)
                            .ok_or_else(unknown)?;
                        if engine != EngineType::Algo {
                            return Err(at(line, format!("{k}: taps are ALGO's")));
                        }
                        let taps = r()?
                            .into_iter()
                            .map(|r| {
                                FixedCol::of(r.dest).map(|c| (r, c)).ok_or_else(|| {
                                    at(
                                        line,
                                        format!(
                                            "{}: a tap routes to FREQ, Q, VCA or FOLD",
                                            ident(r.dest)
                                        ),
                                    )
                                })
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        s.taps.push((op, taps));
                    }
                    _ => {
                        let a = valid_addr(head, rest).ok_or_else(unknown)?;
                        s.params.push((line, a, v.clone()));
                    }
                }
            }
        }
    }
    s.status = match (placeholder, approved) {
        (true, Some((line, _))) => {
            return Err(at(line, "a placeholder has no Sound to approve".into()));
        }
        (true, None) => Status::Placeholder,
        (false, Some((_, st))) => st,
        (false, None) => Status::New,
    };
    Ok(s)
}

/// A description: `Note`'s characters and 1-40 B, and it fits the FACTORY
/// tab's line, measured as the firmware draws it (`browser::note_width`).
fn description(v: &str) -> Result<Note, String> {
    let n = Note::new(v)
        .ok_or_else(|| format!("note {v:?}: 1-40 of A-Z, 0-9, space and . , ' - + / : ( ) & %"))?;
    let w = note_width(v);
    if w > NOTE_W {
        return Err(format!("note {v:?} is {w} px wide, past {NOTE_W}"));
    }
    Ok(n)
}

/// `C4` is 60, as the firmware names notes; `C#-1` is 1.
fn note_number(v: &str) -> Option<u8> {
    let mut c = v.chars();
    let pc = match c.next()? {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest = c.as_str();
    let (sharp, oct) = rest.strip_prefix('#').map_or((0, rest), |o| (1, o));
    let n = (oct.parse::<i32>().ok()? + 1) * 12 + pc + sharp;
    u8::try_from(n).ok().filter(|&n| n <= 127)
}

/// A stored param of a Sound block, by its frozen idents.
fn valid_addr(block: &str, param: &str) -> Option<ValidAddr> {
    let b = BlockRef::ALL
        .into_iter()
        .find(|b| b.disk_ident() == Some(block))?;
    Sound::neutral(EngineType::Algo).params.block(b)?;
    ValidAddr::of_block(b).find(|a| a.spec().ident == param)
}

/// `BLOCK.PARAM`.
fn ident(a: ParamAddr) -> String {
    let p = ValidAddr::find(a.block, a.param).map_or("?", |v| v.spec().ident);
    format!("{}.{p}", a.block.disk_ident().unwrap_or("?"))
}

/// `DEST ±amount, …`: each destination a matrix column takes (a fixed
/// column or a modulatable param), amounts −127…127.
fn routes(v: &str, line: usize) -> Result<Vec<Route>, String> {
    v.split(',')
        .map(|r| {
            let r = r.trim();
            let (d, a) = r
                .rsplit_once(' ')
                .ok_or_else(|| format!("route {r:?}: DEST ±amount"))?;
            let dest = d
                .split_once('.')
                .and_then(|(b, p)| valid_addr(b, p))
                .ok_or_else(|| format!("unknown destination {d}"))?
                .addr();
            if FixedCol::of(dest).is_none() && !dest.modulatable() {
                return Err(format!("{d} is not a mod destination"));
            }
            let amount = a
                .parse::<i8>()
                .ok()
                .filter(|&a| a != i8::MIN)
                .ok_or_else(|| format!("{d} {a}: amount -127..+127"))?;
            Ok(Route { line, dest, amount })
        })
        .collect()
}

/// Ids unique and names unique across every Source, placeholders included.
pub fn check(all: &[Source]) -> Result<(), String> {
    for (i, s) in all.iter().enumerate() {
        // A VA Sound owns no amplitude of its own: without an ENV on the VCA
        // it is organ-gated, flat while held. ALGO and MODAL shape their own.
        let vca_env = s.mods.iter().any(|(src, rs)| {
            matches!(src, ModSource::Env1 | ModSource::Env2 | ModSource::Env3)
                && rs
                    .iter()
                    .any(|r| FixedCol::of(r.dest) == Some(FixedCol::Vca) && r.amount != 0)
        });
        if s.engine == EngineType::Va && !s.is_placeholder() && !vca_env {
            return Err(format!(
                "{}: a VA Sound needs an ENV routed to OUT.VCA (mod.ENV1 = \"OUT.VCA +127\")",
                s.at(s.id_line)
            ));
        }
        // The owner auditions every VA Sound over a phrase (`qa`).
        if s.engine == EngineType::Va && !s.is_placeholder() && s.phrase.is_none() {
            return Err(format!(
                "{}: a VA Sound needs a phrase (phrase = \"bass\", \"lead\", \"pad\", \"arp\", \"wonky\" or \"sweep\")",
                s.at(s.id_line)
            ));
        }
        if let Some(pin) = &s.level
            && (pin.lufs - REFERENCE_LUFS).abs() <= LOUDNESS_LU
        {
            return Err(format!(
                "{}: level by ear {}: inside {REFERENCE_LUFS} ± {LOUDNESS_LU} LU the gate passes it anyway; drop the line",
                s.at(pin.line),
                pin.lufs
            ));
        }
        if let Some(o) = all[..i].iter().find(|o| o.id == s.id) {
            return Err(format!(
                "{}: id S{} is {}'s too",
                s.at(s.id_line),
                s.id.get(),
                o.at(o.id_line)
            ));
        }
        if let Some(o) = all[..i].iter().find(|o| o.name == s.name) {
            return Err(format!(
                "{}: name {} is {}'s too",
                s.at(s.name_line),
                s.name.as_str(),
                o.at(o.name_line)
            ));
        }
    }
    Ok(())
}

/// The public set's walls (ADR 0134): no Source with a `from` or `edits`
/// line (a port's hint on the page), and no
/// id reserved for the private ports.
pub fn public(all: &[Source]) -> Result<(), String> {
    for s in all {
        if let Some((line, _)) = &s.from {
            return Err(format!(
                "{}: from: a port Source; the public build refuses it",
                s.at(*line)
            ));
        }
        if let Some(e) = &s.edits {
            return Err(format!(
                "{}: edits: a port's line; the public build refuses it",
                s.at(e.line)
            ));
        }
        if PORT_IDS.contains(&s.id.get()) {
            return Err(format!(
                "{}: id S{} is reserved for the private ports",
                s.at(s.id_line),
                s.id.get()
            ));
        }
    }
    Ok(())
}

/// `tx81z <8 hex> #<1-32>`.
fn port_from(v: &str) -> Option<PortFrom> {
    let mut w = v.split(' ');
    let (tx, sha, voice) = (w.next()?, w.next()?, w.next()?);
    if tx != "tx81z" || w.next().is_some() || sha.len() != 8 {
        return None;
    }
    let bank = sha
        .bytes()
        .all(|b| b.is_ascii_hexdigit())
        .then(|| u32::from_str_radix(sha, 16).ok())??;
    let voice = voice.strip_prefix('#')?;
    let voice = voice
        .bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| voice.parse::<u8>().ok())??;
    (1..=32)
        .contains(&voice)
        .then_some(PortFrom { bank, voice })
}

/// `<category>: <what>` items split on `;`, none empty.
fn edits(v: &str) -> Option<Vec<(Edit, String)>> {
    v.split(';')
        .map(|item| {
            let (cat, what) = item.split_once(':')?;
            let cat = cat.trim().to_ascii_lowercase();
            let edit = Edit::ALL.into_iter().find(|e| e.word() == cat)?;
            let what = what.trim();
            (!what.is_empty()).then(|| (edit, what.to_string()))
        })
        .collect()
}

/// The ports' walls (spec § 3, § 6): each has `from` and `edits`, an id in
/// `PORT_IDS`, and is neither a placeholder nor approved (ports are
/// private: the owner's rounds are for the public set).
pub fn ports(all: &[Source]) -> Result<(), String> {
    for s in all {
        let at = |m: &str| Err(format!("{}: {m}", s.at(s.id_line)));
        if !PORT_IDS.contains(&s.id.get()) {
            return at(&format!(
                "id S{} is outside the ports' S{}-S{}",
                s.id.get(),
                PORT_IDS.start(),
                PORT_IDS.end()
            ));
        }
        if s.from.is_none() {
            return at("a port needs from = \"tx81z <bank sha-256, first 8 hex> #<voice>\"");
        }
        if s.edits.is_none() {
            return at(
                "a port needs edits = \"<category>: <what>\" (filter, fold, LFO, envelope, effects or formant)",
            );
        }
        match s.status {
            Status::New => {}
            Status::Placeholder => return at("a port is never a placeholder"),
            Status::Approved { .. } => {
                return at("a port carries no approved line: ports are private");
            }
        }
    }
    Ok(())
}

/// The Sound the Source authors, before `trim`: `Sound::neutral`, each
/// param in file order, then the routes and taps.
pub fn patch(s: &Source) -> Result<Sound, String> {
    let mut snd = Sound::neutral(s.engine);
    snd.name = s.name;
    for (line, a, v) in &s.params {
        let mut blk = snd
            .params
            .block_mut(a.addr().block)
            .expect("valid_addr takes Sound blocks only");
        set(&mut *blk, *a, v).map_err(|m| format!("{}: {m}", s.at(*line)))?;
    }
    for (src, routes) in &s.mods {
        for r in routes {
            let fail = |m: &str| format!("{}: {} {m}", s.at(r.line), ident(r.dest));
            if FixedCol::of(r.dest).is_none() {
                snd.dest_registry
                    .add(r.dest, label(r.dest))
                    .map_err(|e| fail(&format!("{e:?}")))?;
            }
            if !snd.mod_state.set_route_addr(src.index(), r.dest, r.amount) {
                return Err(fail("is past the matrix's columns"));
            }
        }
    }
    for (op, routes) in &s.taps {
        for (r, c) in routes {
            snd.params.taps.set(*op, *c, r.amount);
        }
    }
    Ok(snd)
}

/// The Sound as built: `patch`, its OUT LEVEL trimmed `trim` dB.
pub fn to_sound(s: &Source) -> Result<Sound, String> {
    let mut snd = patch(s)?;
    snd.params.out.volume = trimmed(snd.params.out.volume, s.trim)
        .map_err(|m| format!("{}: {m}", s.at(s.trim_line)))?;
    Ok(snd)
}

/// OUT LEVEL `level` trimmed `trim` dB, refused (never clipped) when it
/// leaves the param's range. `fmt --level` writes a trim through it too.
pub fn trimmed(level: f32, trim: f32) -> Result<f32, String> {
    let spec = valid_addr("OUT", "LEVEL")
        .expect("OUT LEVEL is stored")
        .spec();
    let t = level * 10f32.powf(trim / 20.0);
    if (spec.min..=spec.max).contains(&t) {
        Ok(t)
    } else {
        Err(format!(
            "trim {trim:+} dB puts OUT LEVEL {level} at {t}, outside {}..={}",
            spec.min, spec.max
        ))
    }
}

/// `v` into `a`: an enum by its ident, else a number the block holds
/// exactly (never clamped or rounded: a Source says what the card stores).
pub(crate) fn set(blk: &mut dyn Block, a: ValidAddr, v: &str) -> Result<(), String> {
    let id = a.spec().id;
    let key = ident(a.addr());
    if a.coded() {
        let was = blk.enum_code(id);
        for c in 0..=u8::MAX {
            if blk.set_enum_code(id, c) && blk.enum_ident(id) == Some(v) {
                return Ok(());
            }
        }
        if let Some(c) = was {
            blk.set_enum_code(id, c);
        }
        return Err(format!("{key}: unknown value {v}"));
    }
    let x = v
        .parse::<f32>()
        .ok()
        .filter(|x| x.is_finite())
        .ok_or_else(|| format!("{key}: {v} is not a number"))?;
    blk.set(id, x);
    let got = blk.get(id);
    if got.to_bits() != x.to_bits() {
        let sp = a.spec();
        return Err(format!(
            "{key}: {v} is stored as {got} ({}..={})",
            sp.min, sp.max
        ));
    }
    Ok(())
}

/// A registry column's 8-byte label as priming spells it: `O5 LEVEL`,
/// `E2 DEC`, else the block's tag and the param's label.
fn label(a: ParamAddr) -> [u8; LABEL_LEN] {
    let spec = ValidAddr::find(a.block, a.param).map_or("", |v| v.spec().label);
    let prefix: String = match a.block {
        BlockRef::AlgoOp(op) => format!("O{} ", op.index() + 1),
        BlockRef::Env(e) => format!("E{} ", e.index() + 1),
        b => block_tag(b).chars().take(3).chain([' ']).collect(),
    };
    let mut l = [0u8; LABEL_LEN];
    for (o, c) in l.iter_mut().zip(format!("{prefix}{spec}").bytes()) {
        *o = c;
    }
    l
}

/// The Sound's param, route and tap lines, canonical: each param off
/// `Sound::neutral` in block and spec-table order, then each source's
/// routes (fixed columns, then registry columns), then each tap's.
pub fn lines(snd: &Sound) -> Vec<String> {
    let neutral = Sound::neutral(snd.engine());
    let mut out = Vec::new();
    for b in BlockRef::ALL {
        let (Some(blk), Some(base)) = (snd.params.block(b), neutral.params.block(b)) else {
            continue;
        };
        for a in ValidAddr::of_block(b) {
            let text = match (read_value(&*blk, a), read_value(&*base, a)) {
                (DiskValue::Real(x), DiskValue::Real(y)) if x.to_bits() == y.to_bits() => continue,
                (DiskValue::Code(x), DiskValue::Code(y)) if x == y => continue,
                (DiskValue::Real(x), _) => format!("{x}"),
                (DiskValue::Code(_), _) => blk.enum_ident(a.spec().id).unwrap_or("?").into(),
            };
            out.push(format!("{} = \"{text}\"", ident(a.addr())));
        }
    }
    let m = &snd.mod_state;
    for src in ModSource::ALL.into_iter().take(m.num_sources()) {
        let (i, bit) = (src.index(), 1 << src.index());
        let fixed = FixedCol::ALL
            .into_iter()
            .filter(|&c| m.fixed_present(c) & bit != 0)
            .map(|c| (c.addr(), m.fixed_amount(i, c)));
        let reg = (0..m.num_dests())
            .filter(|&d| m.present(d) & bit != 0)
            .map(|d| (m.dest(d), m.amount(i, d)));
        let r: Vec<String> = fixed
            .chain(reg)
            .map(|(d, a)| format!("{} {a:+}", ident(d)))
            .collect();
        if !r.is_empty() {
            out.push(format!("mod.{} = \"{}\"", src.disk_ident(), r.join(", ")));
        }
    }
    for op in TapOp::ALL {
        let r: Vec<String> = snd
            .params
            .taps
            .entries()
            .filter(|e| e.0 == op)
            .map(|(_, c, a)| format!("{} {a:+}", ident(c.addr())))
            .collect();
        if !r.is_empty() {
            out.push(format!("tap.{} = \"{}\"", op.tag(), r.join(", ")));
        }
    }
    out
}

/// Canonical text: each numeric param line ends `  # <display value>`, as
/// the unit's page draws it for that Sound (an LFO's RATE by its TYPE's
/// panel, `view::shown_fmt`); every other line is kept. Comments are
/// ignored on read, so this changes no Sound, and a second run changes
/// nothing. A table that doesn't build is left as written.
pub fn fmt(text: &str) -> String {
    let tables = lined_tables(text, "").unwrap_or_default();
    // Each table's header line and the page context of its patched Sound.
    let ctx: Vec<(usize, Option<SlotCtx>)> = tables
        .iter()
        .map(|t| {
            let snd = (t.kind == "sound")
                .then(|| sound("", t.line, &t.entries).and_then(|s| patch(&s)).ok())
                .flatten();
            (t.line, snd.map(|s| SlotCtx::read(&s.params, Op::A)))
        })
        .collect();
    text.lines()
        .enumerate()
        .map(|(n, l)| {
            let c = ctx.iter().rev().find(|t| t.0 <= n + 1).and_then(|t| t.1);
            c.and_then(|c| fmt_line(l, &c))
                .unwrap_or_else(|| l.to_string())
                + "\n"
        })
        .collect()
}

fn fmt_line(line: &str, ctx: &SlotCtx) -> Option<String> {
    let (k, v, _) = value_line(line.trim())?;
    let (block, param) = k.split_once('.')?;
    let a = valid_addr(block, param).filter(|a| !a.coded())?;
    let mut b = FmtBuf::new();
    let shown = shown_fmt(a.addr(), ctx)?;
    fmt_val(&mut b, page_value(a.spec(), v.parse().ok()?, shown), shown);
    Some(match b.as_str() {
        "" => format!("{k} = \"{v}\""),
        shown if shown == v => format!("{k} = \"{v}\""),
        shown => format!("{k} = \"{v}\"  # {shown}"),
    })
}

/// What `fmt_val` reads for stored `v`: the normalised value, but the
/// stored one for FORMANT, BW and VOWEL, as the renderer gives them.
pub(crate) fn page_value(spec: &ParamSpec, v: f32, fmt: ValFmt) -> f32 {
    match fmt {
        ValFmt::LogHz { .. } | ValFmt::Vowel => v,
        _ => spec.normalize(v),
    }
}

pub(crate) struct VecSink(pub Vec<u8>);

impl ByteSink for VecSink {
    fn put(&mut self, b: &[u8]) -> Result<(), StoreError> {
        self.0.extend_from_slice(b);
        Ok(())
    }
}

/// The card file `S91xxxxx.A`: a first-generation Sound file of `Engine`,
/// `Build`, `Note`, then the Sound, through the library's own encoder.
pub fn file_bytes(s: &Source, stamp: &BuildStamp) -> Result<Vec<u8>, String> {
    let snd = to_sound(s)?;
    let h = Header {
        kind: FileKind::Sound,
        generation: Generation::FIRST,
        name: Some(snd.name),
    };
    let mut sink = VecSink(Vec::new());
    write_file(&mut sink, &h, &mut |w| {
        encode_library_sound(&snd, stamp, Some(&s.note), w)
    })
    .map_err(|e| format!("{}: {e:?}", s.at(s.id_line)))?;
    Ok(sink.0)
}

/// `text` with each Source's `trim` line set to its new value (one place
/// after the point), or added after the table's last value line.
pub fn set_trims(text: &str, trims: &[(&Source, f32)]) -> String {
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let mut by_table: Vec<_> = trims.to_vec();
    // Last table first: an added line shifts only the lines after it.
    by_table.sort_by_key(|(s, _)| std::cmp::Reverse(s.id_line));
    for (s, t) in by_table {
        let l = format!("trim = \"{t:.1}\"");
        if s.trim_line > 0 {
            lines[s.trim_line - 1] = l;
            continue;
        }
        let last = lines[s.id_line..]
            .iter()
            .take_while(|x| !x.trim_start().starts_with("[["))
            .enumerate()
            .filter(|(_, x)| value_line(x.trim()).is_some())
            .map(|(i, _)| s.id_line + i)
            .last()
            .unwrap_or(s.id_line - 1);
        lines.insert(last + 1, l);
    }
    let mut out = lines.join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    out
}
