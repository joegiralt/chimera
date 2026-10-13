//! `chimera-import tx` (showcase spec § 6; importer spec § 6's TX81Z
//! rows): a TX81Z bank's voices to private text Sources, one per voice,
//! plus the import report. Pure: `to_text` maps a voice; `import` is the
//! shell. A Source carries `from` (the bank's SHA-256 prefix and the voice
//! number), never the voice's name, and no `edits`: a raw port is refused
//! by the private build until it is mutated.
//!
//! The map: ALG n → T n on ops 1-4, ops 5 and 6 at LEVEL 0; CRS and FINE
//! index `tx.rs`'s tables; DET → DETUN; OUT → LEVEL; the envelope 1:1;
//! KVS → VEL; FBL → op 4 FDBK; OSW → W1-W8; TRPS → TRNSP; the LFO on
//! LFO 1 with routes. The filter is open; ENV 1, ENV 3, LFO 2 and LFO 3
//! are left free for the mutation. What can't be mapped logs one `Loss`.

use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use chimera_core::addr::BlockRef;
use chimera_core::dsp::algo::algorithms::ALGORITHMS;
use chimera_core::dsp::algo::tx;
use chimera_core::dsp::clampf;
use chimera_core::import::tx81z::{Bank, Voice, Warn, parse};
use chimera_core::storage::ValidAddr;
use sha2::{Digest, Sha256};

use crate::factory_sounds::build::{self, BANKS_VAR, EditsCheck};
use crate::factory_sounds::source::{self, Source};
use crate::privacy::PORT_IDS;

/// A Loss's kind: its stable tag, and whether it is approximated or
/// dropped (importer spec § 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LossKind {
    /// A fixed-frequency operator, now the nearest ratio at a key.
    FixedFreq,
    /// Its ratio was under 0.5, the grid's floor.
    FixedSubaudio,
    /// A fixed-derived ratio capped below Nyquist at note 108.
    Nyquist,
    /// Every operator fixed: the voice plays its sound at the reference
    /// key and transposes away from it.
    AllFixed,
    EgShift,
    RevRate,
    LfoDelay,
    /// LFO speed over RATE's 20 Hz.
    LfoFast,
    /// LS as a NOTE → LEVEL route, at most 7.4 dB/oct.
    KeyLevel,
    /// Ours is bipolar: the base LEVEL drops by half the depth.
    AmpMod,
    /// `tx::ratio`'s FINE steps, within about 14 c of the TX81Z's.
    Fine,
    /// Mod wheel, breath, foot, EG bias, pitch-bend range, portamento,
    /// mono: no such sources, and mono and glide are the Part's.
    Controller,
}

impl LossKind {
    pub const fn tag(self) -> &'static str {
        match self {
            LossKind::FixedFreq => "fixed-freq",
            LossKind::FixedSubaudio => "fixed-subaudio",
            LossKind::Nyquist => "nyquist",
            LossKind::AllFixed => "all-fixed",
            LossKind::EgShift => "eg-shift",
            LossKind::RevRate => "rev-rate",
            LossKind::LfoDelay => "lfo-delay",
            LossKind::LfoFast => "lfo-fast",
            LossKind::KeyLevel => "key-level",
            LossKind::AmpMod => "amp-mod",
            LossKind::Fine => "fine",
            LossKind::Controller => "controller",
        }
    }

    /// Dropped outright, rather than approximated.
    pub const fn dropped(self) -> bool {
        matches!(
            self,
            LossKind::EgShift | LossKind::RevRate | LossKind::LfoDelay | LossKind::Controller
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Loss {
    /// 1-4, the TX81Z's operator number.
    pub op: Option<u8>,
    pub kind: LossKind,
    pub detail: String,
}

impl std::fmt::Display for Loss {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(op) = self.op {
            write!(f, "op{op} ")?;
        }
        write!(f, "{} {}", self.kind.tag(), self.detail)
    }
}

const OPS: [&str; 6] = ["OP_A", "OP_B", "OP_C", "OP_D", "OP_E", "OP_F"];

/// LFO speed, LFS 0-99 → Hz: PROVISIONAL, an exponential from 0.06 to
/// 47 Hz standing in for the TX81Z's own SPEED → Hz chart (importer spec
/// § 6.6), which replaces it once entered from the owner's manual.
pub fn lfo_hz(lfs: u8) -> f32 {
    0.06 * (47.0f32 / 0.06).powf(f32::from(lfs) / 99.0)
}

/// Pitch depth at full PMD by PMS, cents: the OPM family's (YM2151
/// application manual's PMS table; ymfm's OPM/OPZ, BSD-3-Clause, the
/// importer spec's TX81Z reference). PROVISIONAL until § 8.5's A/B.
pub const PMS_CENTS: [f32; 8] = [0.0, 5.0, 10.0, 20.0, 50.0, 100.0, 400.0, 700.0];

/// Amplitude depth at full AMD by AMS, dB: the OPM family's (same
/// sources). PROVISIONAL, and approximate anyway (logged `amp-mod`).
pub const AMS_DB: [f32; 4] = [0.0, 23.9, 47.8, 95.6];

/// One LEVEL step, dB (`tx::LEVEL_GAIN`).
const LEVEL_DB: f32 = 0.75;

fn note_hz(n: f32) -> f32 {
    440.0 * 2f32.powf((n - 69.0) / 12.0)
}

/// A fixed operator's Hz: FIXRG's octave of 16 Hz steps of CRS (top four
/// bits) plus FINE, from the owner's manual's chart (255 Hz at range 0
/// up to 32 640 Hz at 7), 8 Hz where both are 0.
fn fixed_hz(fixrg: u8, crs: u8, fine: u8) -> f32 {
    let steps = (16 * u32::from(crs >> 2) + u32::from(fine)).max(8);
    (steps << fixrg) as f32
}

/// Every grid point (CRSE, FINE) and its `tx::ratio`.
fn grid() -> impl Iterator<Item = (u8, u8, f32)> {
    (0..64u8).flat_map(|c| (0..16u8).map(move |f| (c, f, tx::ratio(c, f))))
}

/// The largest grid ratio at or under `top`.
fn largest_under(top: f32) -> (u8, u8) {
    grid()
        .filter(|g| g.2 <= top)
        .max_by(|a, b| a.2.total_cmp(&b.2))
        .map_or((0, 0), |g| (g.0, g.1))
}

/// The grid point (CRSE, FINE) whose `tx::ratio` is nearest `r` in pitch.
fn nearest(r: f32) -> (u8, u8) {
    let off = |g: &(u8, u8, f32)| (g.2 / r).log2().abs();
    grid()
        .min_by(|a, b| off(a).total_cmp(&off(b)))
        .map_or((0, 0), |g| (g.0, g.1))
}

/// `tx81z <8 hex> #<n>` → the Sound's name, `PORT 1A2B3C4D 05`.
fn port_name(from: &str) -> String {
    let mut w = from.split(' ').skip(1);
    let (sha, n) = (w.next().unwrap_or(""), w.next().unwrap_or(""));
    let n: u8 = n.trim_start_matches('#').parse().unwrap_or(0);
    format!("PORT {} {n:02}", sha.to_ascii_uppercase())
}

/// The voice as a Source's text, and what it loses.
pub fn to_text(v: &Voice, from: &str) -> (String, Vec<Loss>) {
    let mut losses = Vec::new();
    let mut lose = |op: Option<usize>, kind, detail: String| {
        losses.push(Loss {
            op: op.map(|i| i as u8 + 1),
            kind,
            detail,
        })
    };
    let alg = v.alg.get();
    let trnsp = i32::from(v.trps.get()) - 24;
    let mut t = String::new();
    let mut line = |k: &str, val: String| {
        let _ = writeln!(t, "{k} = \"{val}\"");
    };
    line("id", format!("S{}", PORT_IDS.start()));
    line("name", port_name(from));
    line("engine", "ALGO".into());
    line("note", "UNMUTATED TX81Z PORT".into());
    line("demo", "poly".into());
    line("plays", "long stab".into());
    line("home", "C4".into());
    line("from", from.into());
    let a = format!("T{}", alg + 1);
    line("ALGO.ALG_A", a.clone());
    line("ALGO.ALG_B", a);
    line("ALGO.MORPH", "0".into());
    line("ALGO.TRNSP", trnsp.to_string());

    // The fixed-frequency reference key: the loudest fixed carrier's
    // pitch after TRNSP, 24-96; note 60 when only modulators are fixed.
    let carriers = ALGORITHMS[usize::from(alg)].carriers;
    let main = (0..4)
        .filter(|&i| v.ops[i].fix && carriers & (1 << i) != 0)
        .max_by_key(|&i| v.ops[i].out.get());
    let key = main.map_or(60, |i| {
        let o = v.ops[i];
        let hz = fixed_hz(o.fixrg.get(), o.crs.get(), o.fine.get());
        let n = 69.0 + 12.0 * (hz / 440.0).log2() - trnsp as f32;
        (n.round() as i32).clamp(24, 96)
    });

    // The LFO: its depths as routes, the AM's half-depth off each LEVEL.
    let pm_st = f32::from(v.pmd.get()) / 99.0 * PMS_CENTS[usize::from(v.pms.get())] / 100.0;
    let am_db = f32::from(v.amd.get()) / 99.0 * AMS_DB[usize::from(v.ams.get())];
    let am_steps = am_db / LEVEL_DB;
    let mut lfo_routes = Vec::new();
    let mut note_routes = Vec::new();
    let mut controllers = Vec::new();

    for (i, o) in v.ops.iter().enumerate() {
        let p = OPS[i];
        let (mut crse, mut fine) = (o.crs.get(), o.fine.get());
        if o.fix {
            let hz = fixed_hz(o.fixrg.get(), crse, fine);
            let mut r = hz / note_hz((key + trnsp) as f32);
            let top = 24_000.0 / note_hz(108.0);
            let sub = r < 0.5;
            r = r.max(0.5);
            let (c, f) = if r > top {
                largest_under(top)
            } else {
                nearest(r)
            };
            lose(
                Some(i),
                LossKind::FixedFreq,
                format!("{hz:.0} Hz -> ratio {:.2} at key {key}", tx::ratio(c, f)),
            );
            if sub {
                lose(
                    Some(i),
                    LossKind::FixedSubaudio,
                    format!("{hz:.0} Hz -> 0.50"),
                );
            }
            if r > top {
                lose(Some(i), LossKind::Nyquist, format!("ratio {r:.2} capped"));
            }
            (crse, fine) = (c, f);
        } else if fine > 0 {
            lose(
                Some(i),
                LossKind::Fine,
                format!("{fine}: within about 14 c"),
            );
        }
        let am = o.ame && am_steps > 0.0;
        let half = if am { am_steps / 2.0 } else { 0.0 };
        line(&format!("{p}.WAVE"), format!("W{}", o.osw.get() + 1));
        line(&format!("{p}.CRSE"), crse.to_string());
        line(&format!("{p}.FINE"), fine.to_string());
        line(
            &format!("{p}.DETUN"),
            (i32::from(o.det.get()) - 3).to_string(),
        );
        // LS only attenuates; NOTE is (n - 60)/120, so the negative route
        // lifts LEVEL below C4. The base drops by the lift at C1, the fit
        // span's bottom, so no key rises above the source (§ 6.5).
        let ls = (o.ls.get() > 0).then(|| (f32::from(o.ls.get()) / 99.0 * 127.0).round().max(1.0));
        let lift = ls.map_or(0.0, |a| (a / 127.0 * 0.3 * 99.0).ceil());
        let level = (f32::from(o.out.get()) - half - lift).round().max(0.0);
        line(&format!("{p}.LEVEL"), level.to_string());
        line(&format!("{p}.AR"), o.ar.get().to_string());
        line(&format!("{p}.D1R"), o.d1r.get().to_string());
        line(&format!("{p}.D1L"), o.d1l.get().to_string());
        line(&format!("{p}.D2R"), o.d2r.get().to_string());
        line(&format!("{p}.RR"), o.rr.get().to_string());
        line(&format!("{p}.RS"), o.rs.get().to_string());
        line(&format!("{p}.VEL"), o.kvs.get().to_string());
        let fdbk = if i == 3 { v.fbl.get() } else { 0 };
        line(&format!("{p}.FDBK"), fdbk.to_string());
        if am {
            let amount = clampf((half / 99.0 * 127.0).round(), 1.0, 127.0);
            lfo_routes.push(format!("{p}.LEVEL +{amount}"));
            lose(Some(i), LossKind::AmpMod, format!("{am_db:.1} dB, bipolar"));
        }
        if let Some(amount) = ls {
            note_routes.push(format!("{p}.LEVEL -{amount}"));
            let fall = lift + amount / 127.0 * 0.3 * 99.0;
            lose(
                Some(i),
                LossKind::KeyLevel,
                format!(
                    "LS {} -> {:.1} dB/oct, never above the source; worst -{:.1} dB at C7",
                    o.ls.get(),
                    amount / 127.0 * 7.4,
                    fall * LEVEL_DB
                ),
            );
        }
        if o.egsft.get() > 0 {
            let db = [0, 48, 24, 12][usize::from(o.egsft.get())];
            lose(Some(i), LossKind::EgShift, format!("{db} dB floor"));
        }
        if o.ebs.get() > 0 {
            controllers.push(format!("op{} EBS {}", i + 1, o.ebs.get()));
        }
    }
    if v.ops.iter().all(|o| o.fix) {
        lose(
            None,
            LossKind::AllFixed,
            format!("every operator fixed: its own pitch at key {key} only"),
        );
    }
    for p in &OPS[4..] {
        line(&format!("{p}.LEVEL"), "0".into());
    }
    // The filter open (importer spec § 4), whatever neutral holds.
    let cutoff = ValidAddr::of_block(BlockRef::Filter)
        .find(|a| a.spec().ident == "CUTOFF")
        .map_or(20_000.0, |a| a.spec().max);
    line("FILTER.CUTOFF", format!("{cutoff}"));
    line("FOLDER.FOLD", "0".into());

    let hz = lfo_hz(v.lfs.get());
    if hz > 20.0 {
        lose(None, LossKind::LfoFast, format!("{hz:.1} Hz -> 20 Hz"));
    }
    line("LFO1.RATE", format!("{:.2}", hz.min(20.0)));
    let shape = ["SAW", "SQUARE", "TRIANGLE", "RANDOM"][usize::from(v.lfw.get())];
    line("LFO1.SHAPE", shape.into());
    line("LFO1.SYNC", if v.sync { "RETRIG" } else { "FREE" }.into());
    if pm_st > 0.0 {
        // Under a semitone on FINE (±100 c at ±127), else on PITCH (±24).
        let (dest, amount) = if pm_st <= 1.0 {
            ("PITCH.FINE", pm_st * 127.0)
        } else {
            ("PITCH.PITCH", pm_st / 24.0 * 127.0)
        };
        lfo_routes.insert(0, format!("{dest} +{}", clampf(amount.round(), 1.0, 127.0)));
    }
    if !lfo_routes.is_empty() {
        line("mod.LFO1", lfo_routes.join(", "));
    }
    if !note_routes.is_empty() {
        line("mod.NOTE", note_routes.join(", "));
    }
    if v.lfd.get() > 0 {
        lose(None, LossKind::LfoDelay, v.lfd.get().to_string());
    }
    if v.rev.get() > 0 {
        lose(None, LossKind::RevRate, v.rev.get().to_string());
    }
    let named = [
        ("MW PITCH", v.mw_pitch.get()),
        ("MW AMPL", v.mw_ampl.get()),
        ("BC PITCH", v.bc_pitch.get()),
        ("BC AMPL", v.bc_ampl.get()),
        ("BC E BIAS", v.bc_ebias.get()),
        ("FC VOL", v.fc_vol.get()),
        ("FC PITCH", v.fc_pitch.get()),
        ("FC AMPL", v.fc_ampl.get()),
        ("PBR", v.pbr.get()),
        ("PORT", v.port.get()),
    ];
    for (n, x) in named.into_iter().filter(|x| x.1 > 0) {
        controllers.push(format!("{n} {x}"));
    }
    if v.bc_pbias.get() != 50 {
        controllers.push(format!("BC P BIAS {}", v.bc_pbias.get()));
    }
    if v.mono() {
        controllers.push("MONO".into());
    }
    if !controllers.is_empty() {
        lose(None, LossKind::Controller, controllers.join(", "));
    }
    (format!("[[sound]]\n{t}"), losses)
}

/// `to_text`, parsed: always a Source (every value it writes is stored
/// exactly; `import_tx_test` pins it).
pub fn to_source(v: &Voice, from: &str) -> (Source, Vec<Loss>) {
    let (text, losses) = to_text(v, from);
    let s = source::parse(&text, "import")
        .and_then(|mut s| s.pop().ok_or_else(|| "no [[sound]]".into()))
        .unwrap_or_else(|e| panic!("the TX81Z map wrote a bad Source: {e}"));
    (s, losses)
}

/// A voice's report line: `#05  approx: …  dropped: …`, never its name.
pub fn report_line(voice: u8, losses: &[Loss], warn: Warn) -> String {
    let mut l = format!("#{voice:02}");
    let join = |dropped: bool| {
        losses
            .iter()
            .filter(|x| x.kind.dropped() == dropped)
            .map(Loss::to_string)
            .collect::<Vec<_>>()
            .join("; ")
    };
    let (approx, dropped) = (join(false), join(true));
    if !approx.is_empty() {
        let _ = write!(l, "  approx: {approx}");
    }
    if !dropped.is_empty() {
        let _ = write!(l, "  dropped: {dropped}");
    }
    if warn.adjusted > 0 {
        let _ = write!(l, "  clamped-field: {} bytes", warn.adjusted);
    }
    if warn.extra {
        l.push_str("  dx11-v50-extra: ignored");
    }
    if losses.is_empty() && warn == Warn::default() {
        l.push_str("  exact");
    }
    l
}

/// The bank's id in `from`: its SHA-256, the first 8 hex digits.
pub fn bank_id(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .take(4)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `1,4,9-12`: voices 1-32, in the order given.
pub fn voices_arg(s: &str) -> Result<Vec<u8>, String> {
    let bad = || format!("--voices {s}: 1-32, as 1,4,9-12");
    let n = |x: &str| {
        x.parse::<u8>()
            .ok()
            .filter(|n| (1..=32).contains(n))
            .ok_or_else(bad)
    };
    let mut out = Vec::new();
    for part in s.split(',') {
        let (a, b) = part.split_once('-').unwrap_or((part, part));
        let (a, b) = (n(a)?, n(b)?);
        if a > b {
            return Err(bad());
        }
        out.extend(a..=b);
    }
    Ok(out)
}

fn git(dir: &Path, args: &[&str]) -> Option<std::process::Output> {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()
}

/// `--out`'s guard: refused inside a git work tree unless git ignores it
/// (or a folder above it), so a port can't land where it could be staged.
/// The deepest folder that exists is resolved (symlinks, `..`); what is
/// left may not climb. Inside a repo, a git that can't answer refuses.
pub fn out_allowed(out: &Path) -> Result<(), String> {
    let refuse = |why: &str| {
        Err(format!(
            "{}: {why}; ports stay out of the repo",
            out.display()
        ))
    };
    let abs = std::path::absolute(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let Some(base) = abs.ancestors().find(|p| p.is_dir()) else {
        return refuse("no folder above it");
    };
    let rest = abs.strip_prefix(base).unwrap_or(Path::new(""));
    if rest.components().any(|c| c == Component::ParentDir) {
        return refuse("`..` past the folders that exist");
    }
    let base = base
        .canonicalize()
        .map_err(|e| format!("{}: {e}", base.display()))?;
    let real = base.join(rest);
    let in_repo = base.ancestors().any(|p| p.join(".git").exists());
    let tree = git(&base, &["rev-parse", "--is-inside-work-tree"]);
    let inside = tree
        .as_ref()
        .is_some_and(|o| o.status.success() && o.stdout.trim_ascii() == b"true");
    if !inside {
        return if in_repo {
            refuse("inside a git repo, and git can't say whether it is in its work tree")
        } else {
            Ok(())
        };
    }
    // Each folder from `real` up, as a folder (`dir/`): 0 ignored, 1 not.
    for p in real.ancestors() {
        let mut d = p.as_os_str().to_owned();
        d.push("/");
        match git(&base, &["check-ignore", "-q", &d.to_string_lossy()])
            .and_then(|o| o.status.code())
        {
            Some(0) => return Ok(()),
            Some(1) => {}
            _ => return refuse("git check-ignore failed inside a work tree"),
        }
    }
    refuse("inside a git work tree and not ignored (git check-ignore)")
}

/// What the report ends with: a raw port is not yet a Sound to build.
pub const RAW_PORTS: &str = "raw ports carry no edits: mutate each, add its edits line and renumber its id (S9100010-S9100025) before --private builds it";

/// `chimera-import tx`: `voices` (all when empty) of the bank at `bank`
/// into `out` as `<n>.toml`, and `import-report.txt`; an existing
/// `<n>.toml` (a port being mutated) only with `force`. Returns the report.
pub fn import(bank: &Path, voices: &[u8], out: &Path, force: bool) -> Result<String, String> {
    out_allowed(out)?;
    let bytes = std::fs::read(bank).map_err(|e| format!("{}: {e}", bank.display()))?;
    let parsed = parse(&bytes).map_err(|e| format!("{}: {e:?}", bank.display()))?;
    let id = bank_id(&bytes);
    let all: Vec<u8> = (1..=32).collect();
    let voices = if voices.is_empty() { &all } else { voices };
    let there: Vec<String> = voices
        .iter()
        .map(|n| format!("{n}.toml"))
        .filter(|f| out.join(f).exists())
        .collect();
    if !force && !there.is_empty() {
        return Err(format!(
            "{}: {} already there; --force overwrites (a mutation is lost)",
            out.display(),
            there.join(", ")
        ));
    }
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut report = format!(
        "tx81z {id}: {} voices\nLFO speed: a provisional curve until the manual's SPEED chart\n",
        voices.len()
    );
    if let Some(c) = parsed.checksum {
        let _ = writeln!(
            report,
            "warning: checksum {:02X}, the voices sum to {:02X}; imported anyway",
            c.found, c.expected
        );
    }
    for &n in voices {
        let i = usize::from(n - 1);
        let (text, losses) = to_text(&parsed.voices[i], &format!("tx81z {id} #{n}"));
        let p = out.join(format!("{n}.toml"));
        std::fs::write(&p, text).map_err(|e| format!("{}: {e}", p.display()))?;
        report.push_str(&report_line(n, &losses, parsed.warn[i]));
        report.push('\n');
    }
    report.push_str(RAW_PORTS);
    report.push('\n');
    let p = out.join("import-report.txt");
    std::fs::write(&p, &report).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(report)
}

/// Spec § 6 "Edits verified": each port against its voice re-imported
/// from the bank in `dir` (`$CHIMERA_TX_BANKS`) whose SHA-256 its `from`
/// names.
pub struct BankEdits {
    pub dir: PathBuf,
}

impl BankEdits {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// The bank whose id is `id`, parsed.
    fn bank(&self, id: &str) -> Result<Bank, String> {
        let none = |why: String| {
            format!(
                "{}: {why} (${BANKS_VAR} names the banks' folder)",
                self.dir.display()
            )
        };
        let rd = std::fs::read_dir(&self.dir).map_err(|e| none(e.to_string()))?;
        for e in rd {
            let p = e.map_err(|e| none(e.to_string()))?.path();
            if !p.is_file() {
                continue;
            }
            let bytes = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            if bank_id(&bytes) == id {
                return parse(&bytes).map_err(|e| format!("{}: {e:?}", p.display()));
            }
        }
        Err(none(format!("no bank tx81z {id}")))
    }
}

impl EditsCheck for BankEdits {
    fn verify(&self, port: &Source) -> Result<(), String> {
        let (Some((line, from)), Some(edits)) = (&port.from, &port.edits) else {
            return Err(format!(
                "{}: a port needs from and edits",
                port.at(port.id_line)
            ));
        };
        let id = format!("{:08x}", from.bank);
        let bank = self
            .bank(&id)
            .map_err(|e| format!("{}: {e}", port.at(*line)))?;
        let tag = format!("tx81z {id} #{}", from.voice);
        let base = to_source(&bank.voices[usize::from(from.voice - 1)], &tag).0;
        let (now, was) = (source::patch(port)?, source::patch(&base)?);
        for (e, _) in &edits.items {
            if build::category(&now, *e) == build::category(&was, *e) {
                return Err(format!(
                    "{}: edits names {}, but its {} is the bank voice's ({tag})",
                    port.at(edits.line),
                    e.word(),
                    e.word()
                ));
            }
        }
        Ok(())
    }
}
