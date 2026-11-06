# Chimera 0.3 MIDI Phase 1 (DIN): typed parser, NRPN address space, binding model, value echo

- **Status:** Owner answers recorded 2026-10-25 (§ 13); ready to implement
- **Date:** 2026-10-25
- **Base:** `origin/main` 4eae5d81 (worktree `chimera-midi-03`, branch `midi-03`)
- **Research:** `reports/MIDI controller and USB schemes.md` ("Recommended phased plan for 0.3", Phase 1) and its notes in `research_notes/MIDI controller and USB schemes/`
- **ADRs:** proposes [0216](../../adr/0216-every-parameter-has-a-frozen-nrpn-address-from-its-disk-identity.md) (stable NRPN address space) and [0217](../../adr/0217-controllers-bind-through-one-typed-model.md) (binding model). Builds on 0019, 0045, 0128, 0137, 0139, 0175, 0198, 0208.
- **Plan:** `docs/superpowers/plans/2026-10-25-midi-phase1.md`

Decisions marked **(my call)** are ones the report left open. Each is cheap to flip before its task starts.

## 1. Intent

0.3's scope is "efficiency, perfect MIDI and controllers, USB MIDI" (owner, scope-0-3). The report reduces "perfect" to three failures to remove:
1. The receiver guesses an encoding it should have asked about.
2. The receiver can't tell the controller its current value.
3. The receiver renumbers parameters that external files depend on.

Phase 1 removes all three over DIN only, before any USB work. It ships:
- a two-layer typed parser;
- one frozen NRPN address for every stored parameter;
- a binding model in which every input names its kind;
- per-Part value echo out of the DIN port;
- a Universal Identity Reply.

With these, generic controllers (Faderfox EC4, BCR2000, Launch Control XL 3 custom modes) and sequencers (Hapax, Cirklon, OXI) work properly.

## 2. Goals and non-goals

**Goals**
- **G1. Typed parser, two layers.** Layer 1 turns bytes into MIDI events. Layer 2 turns events into controller meaning. Covered: running status, real-time interleave, SysEx bounds, the RPN/NRPN state machine with 14-bit values, data increment/decrement, and null RPN. The parser is pure, `no_std`, `forbid(unsafe_code)`, and fuzzed and property-tested on the host.
- **G2. One parameter table, frozen NRPN numbers.** The existing `ParamSpec` tables, keyed by the frozen disk identity, are the single source of truth. A CI fixture refuses any renumbering (ADR 0216).
- **G3. A binding model.** Control → input kind → target, plus feedback. Input kinds:
  - `Absolute` with takeover `Jump`, `Pickup` or `Scale`;
  - `Relative` in one of the four byte encodings, or NRPN increment/decrement;
  - `Button`, momentary or toggle.

  The binding is stored in the Sound's map. It extends ADR 0139's learn and MIDI MAP page; it doesn't replace them.
- **G4. Learn senses the kind.** Learn infers absolute, relative (and which encoding), button or 14-bit from what the control sends. The user corrects the guess on MIDI MAP.
- **G5. Per-Part value echo out of DIN.** A bound parameter that changes for any reason other than its own binding is sent back on the Part's channel, on that binding's control. A Part change or Sound load sends a burst.
- **G6. Universal Identity Reply**, so librarians and editors can detect the unit.
- **G7. The rules the old parser skipped:**
  - CC 124–127 imply All Notes Off;
  - FF (Reset) maps to All Sound Off plus Reset All Controllers;
  - Active Sensing is honoured once seen;
  - running status is cancelled by System Common.
- **G8. Bank discipline.** Every new function lives in bank 2 behind a door (ADR 0198). Bank 1's net size goes **down**, because the old parser, the CC router, `hear_ccs` and the map page move out.
- **G9. Old projects load safely.** A Sound with the old `MidiMap` record (`0x001E`) loads as absolute-jump bindings. Garbage never crashes.

**Non-goals (later phases or never)**
- USB MIDI and the interrupt-driven USB ADR: Phase 2.
- MPE, Program Change and Bank Select actions, SysEx parameter set/get and dumps, and generated Hapax, midi.guide and editor files: Phase 3. Phase 1 parses Program Change and latches Bank Select, then drops both and counts them.
- Mackie Control, LED and text feedback, and surface profiles: Phase 4. `Feedback` stays an enum so they can be added later.
- Echo of every panel move as NRPN (Elektron's INT+EXT). Without a binding nothing is echoed. **(my call)**: it costs a hook in every edit path, and Phase 3's editor story is the place to decide it.
- Receiver-side acceleration. Controllers accelerate themselves: the EC4 has Acc0–3 and the BCR has speed bands. A relative step of |d| is taken at face value. **(my call)**
- Poly pressure, MIDI Thru, clock out, the MIDI-CI responder and a mirror of the stock PreenFM3 NRPN map.
- A DEVICE ID setting. Phase 1 answers device IDs `0x7F` and `0x00` only. **(my call)**

## 3. What exists today (origin/main 4eae5d81)

| Piece | Where | Notes |
|---|---|---|
| Byte parser | `chimera-hal/src/midi.rs` `MidiParser::feed` | Running status and real-time pass-through. No SysEx state, Program, pressure or System Common cancel. 124–127 not handled. |
| ISR | `chimera-stm32/src/midi_din.rs` `USART1` | One byte per loop: `feed` → `midi_in::route` → note queue or `CcRing`. RX only. PB6 (TX) is not configured. |
| Router | `chimera-engine/src/midi_in.rs` | `route`, `Cc` (0..=127, `is_reserved`), `CcEvent` packed in a `u32`, `CcRing` SPSC 64 × `AtomicU32`. |
| Note path | `chimera-engine/src/note_queue.rs` | `InEvent` {Note, Bend, Sustain, Mode(120/121/123), Rt}, `from_midi`. |
| Map | `chimera-engine/src/midi_map/` | `MidiMap` of 16 `MapEntry { cc, target: Mappable, min, max }` in `Sound::map`. `default_for(engine)`. Record `0x001E`, 5 B an entry. |
| Apply | `chimera-core/src/ui/cc.rs` `hear_ccs` | Per Part on its channel. Continuous targets ramp over inter-arrival time; stepped ones are binned through `edit_addr`. Capped under `MOVES`. |
| Learn | `chimera-core/src/ui/learn.rs` | MIX+SEQ arms on the focused cell. The first CC on the Part's channel is learned. |
| MIDI MAP page | `chimera-core/src/ui/settings/midi_map.rs` | A row, B CC, C MIN, D MAX, MIX+MINUS delete. |
| Frozen identity | `storage/codes.rs` `BlockRef::disk_code` | Pinned with `ParamSpec::{id, ident}` by `chimera-core/tests/fixtures/disk_codes_v1.txt` (append-only, ADR 0045). |
| Door | `chimera-engine/src/bank2.rs` `Bank2Door` | Slots: board `+0x100`, UI `+0x180`, bench `+0x200`, ORBIT UI `+0x280`. The card ISR already calls a door (ADR 0175). |

Three findings shape the design:
- **ParamIds are already frozen** per block by the disk-codes fixture, so a stable NRPN number can be *derived* from them rather than maintained as a second table (§ 5).
- **The ISR → ring → UI split is already right.** Layer 2 can run in the ISR, so a dropped event is a whole semantic event, never one CC out of an NRPN quadruple. Binding resolution stays on the UI side, where the Sound, the focus and the edit path live.
- **There is no DIN TX yet.** Echo needs PB6 (AF7), USART1 TE and a TX interrupt.

## 4. Type model

Everything in this section is a sketch of the real signatures. The plan pins each one in its task.

### 4.1 Crate `chimera-midi` (new, `no_std`, no dependencies, `#![forbid(unsafe_code)]`)

It is a leaf crate so the parser and the binding arithmetic travel to the Blackbox port and to plugins (blackbox-port, dsp-company-plan) with no PreenFM3 or UI assumptions. **(my call)**: a new crate rather than a module in `chimera-engine`. It also makes the bank-2 link globs one prefix per module.

```rust
// value.rs: bank 1 (tiny const fns the old types already had)
pub struct U7(u8);          // 0..=127
pub struct U14(u16);        // 0..=16383; U14::from_parts(msb: U7, lsb: U7)
pub struct Channel(u8);     // 0..=15
pub struct Key(u8);         // 0..=127 (today's MidiNote)
pub struct Velocity(NonZeroU8); // 1..=127: a note-on can never carry 0
pub struct Bend(i16);       // -8192..=8191
// chimera-hal re-exports these as MidiChannel, MidiNote, Velocity: one type each.
```

#### Layer 1: `stream.rs` (bank 2)

```rust
pub struct Ctrl(u8);        // 0..=119; 120..=127 are ChannelMode, never a Ctrl
pub enum Voice {
    NoteOff { key: Key, vel: U7 },
    NoteOn { key: Key, vel: Velocity },   // 9n kk 00 is returned as NoteOff
    PolyPressure { key: Key, value: U7 },
    Control { cc: Ctrl, value: U7 },
    Program(U7),
    ChannelPressure(U7),
    PitchBend(Bend),                      // LSB first, centred on 0x2000
}
pub enum ChannelMode { AllSoundOff, ResetAllControllers, LocalControl(bool),
                       AllNotesOff, OmniOff, OmniOn, MonoOn(U7), PolyOn }
pub enum Common { TimeCode(U7), SongPosition(U14), SongSelect(U7), TuneRequest }
pub enum RealTime { Clock, Start, Continue, Stop, ActiveSensing, Reset }
pub struct SysEx<'a>(&'a [u8]);           // complete body between F0 and F7
pub enum Fault {
    Orphan,          // a data byte with no running status
    Interrupted,     // a status byte cut a channel or common message short
    SysExCut,        // a non-real-time status ended a SysEx without F7
    SysExOverflow,   // longer than the buffer: the rest is ignored until F7 or a status
    Undefined,       // F4, F5 (they also cancel running status), F9, FD
}
pub enum Event<'a> { Voice(Channel, Voice), Mode(Channel, ChannelMode),
                     Common(Common), SysEx(SysEx<'a>), Fault(Fault) }
pub enum Parsed<'a> { RealTime(RealTime), Event(Event<'a>) }

pub struct Parser<const SYSEX: usize> { state: State, sysex: [u8; SYSEX], faults: FaultCounts }
enum State {                               // only legal parser states exist
    Idle,                                  // no running status
    Running { status: VoiceStatus, first: Option<U7> }, // first held only for 2-byte kinds
    Common { kind: CommonKind, first: Option<U7> },     // no running status after it
    SysEx { len: usize, over: bool },
}
impl<const N: usize> Parser<N> {
    pub const fn new() -> Self;
    pub fn push(&mut self, byte: u8) -> Option<Parsed<'_>>; // never panics, never allocates
    pub fn faults(&self) -> FaultCounts;
}
```

Rules, each a test:
- Real-time bytes F8–FF are returned at once and leave `state` untouched.
- A channel status sets running status. F0–F7 cancel it.
- A SysEx is ended by F7, or by any non-real-time status. The second case returns `Fault::SysExCut`, and the status starts its own message.
- When one byte both breaks a message and completes a one-byte message (F6 after a cut SysEx), the complete message is returned and the fault is only counted.
- Phase 1's SysEx buffer is 32 B. That is enough for the Identity Request and for MIDI-CI discovery headers later.

#### Layer 2: `ctl.rs` (bank 2): per source, per channel

```rust
pub enum Sel { Rpn(U14), Nrpn(U14) }       // the active register; None = nothing selected
pub enum DataValue { Coarse(U7), Fine(U14) } // CC 6 alone → Coarse; CC 38 → Fine(msb<<7 | lsb)
pub enum Dir { Up, Down }
/// A CC layer 2 passes through: 0..=119 minus 0, 6, 32, 38, 64 and 96..=101.
/// It is exactly today's non-reserved set, and the set learn accepts.
pub struct FreeCc(u8);
pub enum Ctl {
    Cc { cc: FreeCc, value: U7 },
    Data { sel: Sel, value: DataValue },
    Step { sel: Sel, dir: Dir },            // CC 96 / 97, one step; the data byte is ignored
    Program { bank: Option<U14>, program: U7 }, // parsed; Phase 1 drops it
    Pressure(U7),
}
pub enum Heard { Note, Ctl(Ctl), Consumed } // Note: hand it to the note path untouched
pub struct ChannelCtl {
    rpn: (U7, U7), nrpn: (U7, U7), sel: Option<Sel>, msb: Option<U7>, bank: BankLatch,
}
pub struct Controllers { ch: [ChannelCtl; 16] }
impl Controllers { pub fn hear(&mut self, ch: Channel, v: Voice) -> Heard; }
```

Rules, each a test (report § "two-layer parser"; RP-018 for increment/decrement):
- CC 99/98 select an NRPN and CC 101/100 an RPN. Whichever register was written last wins.
- RPN 127/127 sets `sel = None`, and so does NRPN 127/127 **(my call)**: stock PreenFM3 uses NRPN 127/127 as a dump request, and Chimera is not stock.
- A new selection clears `msb`. CC 6 emits `Coarse` at once and never waits for an LSB. CC 38 emits `Fine` with the held MSB, or 0 if there is none.
- CC 96/97 emit one `Step`, and the data byte is ignored (RP-018: "unused"). The research notes record that some controllers put a step count in the data byte; Phase 1 doesn't honour that.
- Data entry or a step with nothing selected is consumed and inert.
- RPN/NRPN state survives CC 121 (Reset All Controllers).
- CC 0/32 latch a bank, and either half alone is accepted. The latch is used and cleared by the next Program Change.
- CC 64 and every `Voice` that isn't a Control, Program or ChannelPressure returns `Heard::Note`.
- The state is per channel, and each source owns its own `Controllers`, as ADR 0019 does for parsers. Phase 2's USB gets its own.

#### Pure binding arithmetic (bank 2)

```rust
// rel.rs
pub enum ByteEncoding {
    TwosComplement, // +1 = 01, -1 = 7F: EC4 CCr1, BCR relative-1
    BinaryOffset,   // +1 = 41, -1 = 3F: EC4 CCr2, BCR relative-2, LCXL 3, Arturia
    SignBitDown,    // +1 = 01, -1 = 41: Mackie V-Pot, BCR relative-3
    SignBitUp,      // +1 = 41, -1 = 01: Ableton "Signed Bit"
}
impl ByteEncoding { pub const fn decode(self, v: U7) -> i8; } // 0 and 64 decode to 0 where the encoding has no zero

// take.rs
pub enum Takeover { Jump, Pickup, Scale }
pub struct Place(u16);  // 0..=16383: a param's normalised place, the unit every absolute input works in
pub enum Taken { Move(Place), Waiting { toward: Dir } } // Waiting drives the pickup hint
pub struct TakeState { last_in: Option<Place>, engaged: bool }
pub fn take(t: Takeover, s: &mut TakeState, incoming: Place, param: Place, tol: u16) -> Taken;
// Scale: moving up, p' = p + (c' - c)(1 - p)/(1 - c); moving down, p' = p - (c - c')·p/c.
// Both ends converge (Ableton's Value Scaling).

// law.rs: an incoming value as a Place, by the parameter's law (ADR 0216)
pub enum ValueLaw { Place14, Index7, Index14, Code7 }
pub fn place_of(law: ValueLaw, v: DataValue, steps: u16) -> Option<Place>;
// Place14 with Coarse(m): m<<7 | m (bit replication, so 127 → 16383); Fine refines.

// learn.rs
pub enum Guess {
    Absolute,
    Relative(ByteEncoding, Sure), // Sure::No when only one direction was seen
    Button,
    Paired14,                     // CC n then CC n+32, n in 1..=31
}
pub fn classify(seen: &Seen) -> Guess; // Seen: up to 24 (control, value) samples in arrival order

// out.rs: bytes out, running status inside a burst
pub struct Writer<'a> { buf: &'a mut [u8], len: usize, status: Option<u8> }
impl Writer<'_> {
    pub fn cc(&mut self, ch: Channel, cc: FreeCc, v: U7) -> bool;
    pub fn cc14(&mut self, ch: Channel, msb: Cc14Msb, v: U14) -> bool;
    pub fn nrpn(&mut self, ch: Channel, n: U14, v: DataValue) -> bool;
    pub fn sysex(&mut self, body: &[u8]) -> bool;
}
pub const fn din_us(bytes: usize) -> u32; // 320 µs a byte at 31,250 baud

// sysex.rs
pub enum Request { Identity }
pub fn request(s: SysEx<'_>, own: U7) -> Option<Request>; // F0 7E <7F|own> 06 01 F7
pub struct Identity { pub maker: Maker, pub family: U14, pub member: U14, pub version: [U7; 4] }
pub enum Maker { NonCommercial /* 0x7D */, One(U7), Three(U7, U7) }
pub fn identity_reply(dev: U7, id: &Identity, w: &mut Writer) -> bool;

// sense.rs: Active Sensing, opt-in after the first FE
pub enum Sensing { Off, Armed }
impl Sensing { pub fn heard(&mut self, rt: RealTime); pub fn timed_out(&mut self) -> bool; }
```

### 4.2 `chimera-engine`

```rust
// nrpn.rs (ADR 0216): the address space, derived, never a second table (bank 2)
pub struct NrpnAddr(U14);     // MSB = block disk code (< 64), LSB = ParamId (< 128)
impl NrpnAddr {
    pub const fn of(a: ParamAddr) -> Option<Self>;   // None: no disk code (PartMix, Tempo)
    pub fn addr(self) -> Option<ParamAddr>;          // None: retired, reserved or unknown
}
pub fn law(spec: &ParamSpec) -> ValueLaw;
pub fn writable(a: ParamAddr) -> Option<Mappable>;   // Phase 1: exactly Mappable

// midi_map/: the binding (ADR 0217); type definitions are bank 1 (Sound holds them)
pub struct LearnCc(FreeCc);   // FreeCc minus nothing: 64 is already out of FreeCc
pub struct Cc14Msb(u8);       // 1..=31 minus 6 (data entry); its LSB is MSB + 32
pub enum CcKind { Absolute(Takeover), Relative(ByteEncoding), Button(ButtonMode) }
pub enum NrpnKind { Absolute(Takeover), Relative(ByteEncoding), IncDec, Button(ButtonMode) }
pub enum ButtonMode { Momentary, Toggle }
/// The control and how to read it, as one type: IncDec on a CC, or relative
/// on pressure, can't be written down.
pub enum Input {
    Cc(LearnCc, CcKind),
    Cc14(Cc14Msb, Takeover),
    Nrpn(U14, NrpnKind),
    Pressure(Takeover),
}
pub enum Feedback { Off, Value }     // Phase 4 adds Led(..) and Text(..)
pub struct Span { pub min: u8, pub max: u8 } // today's places 0..=255; min > max inverts
pub struct Binding { pub input: Input, pub target: Mappable, pub span: Span, pub feedback: Feedback }
const _: () = assert!(size_of::<Binding>() <= 12);
pub struct MidiMap { entries: [Binding; 16], len: u8 } // same API: learn, set, remove, entries

// midi_in.rs, rewritten (pipe in bank 2; the ring and event types in bank 1)
pub enum CtrlEvent { Ctl(Channel, Ctl), Identity { dev: U7 }, SensingLost }
pub struct CtrlRing { slots: [[AtomicU32; 2]; 64], head, tail, dropped } // replaces CcRing
pub trait Sinks { fn note(&mut self, e: InEvent) -> bool; fn ctrl(&mut self, e: CtrlEvent) -> bool; }
pub struct DinPipe { parser: Parser<32>, ctl: Controllers, sense: Sensing, stats: PipeStats }
impl DinPipe { pub fn feed(&mut self, bytes: &[u8], out: &mut impl Sinks); pub fn idle(&mut self, out: &mut impl Sinks); }

// out_ring.rs: bytes the UI queues for the DIN TX interrupt (SPSC, 256 B, bank 1 type)
pub struct OutRing { .. } // push_slice (UI side), pop_into(&mut [u8]) (ISR side)

// door (ADR 0198 pattern)
pub struct MidiDoor {
    feed: fn(&mut DinPipe, &[u8], &mut dyn Sinks),
    idle: fn(&mut DinPipe, &mut dyn Sinks),
    decode_map: fn(&[u8], u16 /*tag*/, &mut MidiMap) -> Result<Skipped, Bounds>,
    encode_map: fn(&MidiMap, &mut dyn RecordSink),
}
// a `Bank2Door::midi` field, or a slot if the engine table has no room (Task 8 measures)
```

### 4.3 `chimera-core` (UI)

```rust
// ui/midi/: bank 2 behind MidiUiDoor at DOOR_ADDR + 0x300 (a new slot)
pub struct BindRuntime {              // per Part per entry, never stored; reset on load
    take: TakeState, msb14: Option<U7>, toggled: bool,
    echo_last: Option<u16>, quiet_until: Ms,
}
pub struct MidiUi { rt: [[BindRuntime; MAP_LEN]; MAX_PARTS], echo: EchoPlan, sensing: LearnSense }
pub enum Action { Place(Place), Detents(i16), Wait(Dir), Toggle }
pub fn resolve(b: &Binding, rt: &mut BindRuntime, c: &Ctl, now: Place) -> Option<Action>;
pub enum Learn { Off, Armed { part, note }, Sensing { part, entry: u8, seen: Seen } }
```

## 5. The NRPN address space (ADR 0216)

- **Address:** NRPN MSB = the block's frozen disk code. NRPN LSB = the `ParamId`. FILTER CUTOFF is `10:<CUTOFF's id>`, Part LEVEL is `25:<LEVEL's id>`.
  - Asserted at compile time: every disk code is < 64 and every ParamId is < 128.
  - MSB 64–126 is reserved for future global, system and FX spaces. MSB 127 is never used: 127/127 is the null.
- **No new table.** The address is derived from the identity ADR 0045 already freezes. `disk_codes_v1.txt` pins `(code, ident)` for blocks and `(id, ident)` for params.
  - A new fixture, `nrpn_v1.txt`, pins the derived chart, one line a stored param: `N <msb> <lsb> <BLOCK_IDENT> <PARAM_IDENT> <law> <W|R>`.
  - `nrpn_chart_test` fails if a line changes or disappears. Appending is allowed.
  - A retired param's address stays in the fixture marked `X`, and is never reused (`storage::RETIRED`).
  - A change of law counts as a renumber.
- **Value laws**, derived from the `ParamSpec`:

  | Law | When | Wire |
  |---|---|---|
  | `Place14` | continuous | 14-bit over the normalised span (`ParamSpec::normalize`). An MSB alone is bit-replicated (`m<<7 \| m`), so 0 → 0 and 127 → full; CC 38 refines. |
  | `Code7` | a choice with frozen enum codes | MSB = the frozen disk code; LSB ignored. Reordering a list never moves a sequencer's value. A code with no live value is ignored. A choice with a code past 7 bits (THEME BLACK's 254/255) is `Index7` (`nrpn::WIDE_CODES`). |
  | `Index7` | other discrete params, ≤ 128 values | MSB = steps from `min`; LSB ignored. |
  | `Index14` | discrete params, > 128 values (TUNE) | 14-bit steps from `min`. |

  The 7-bit laws carry the value in the MSB (CC 6), as Elektron does and as a sequencer's 7-bit NRPN sends it. **(my call)**
- **Writable:** in Phase 1, NRPN writes are accepted for exactly ADR 0139's `Mappable` set, on the Part's channel: a Sound's stored, non-patching params plus Part LEVEL and PAN. Every other address is reserved and inert: FX, ORBIT, settings, CH.
- **Canonical NRPNs need no binding.** An NRPN on a Part's channel whose address is writable moves that param with `Jump` takeover, through the same ramp and budget as a CC.
  - **This is not a CC chart.** ADR 0128's ban covers CCs. NRPN numbers name every param, so they can't collide with a player's CCs.
  - An explicit `Input::Nrpn` binding on the same number wins over the canonical meaning **(my call)**.
  - An address the Part's engine doesn't read is ignored, as a dead map entry is.
- **RPN 0 (bend range)** sets the Part's BEND (ADR 0128): `Coarse(m)` → `clamp(m, 0, 24)` semitones, LSB ignored, a `Step` ±1 semitone **(my call)**, for every Part on the channel. RPN 1, 2 and 5 are ignored in Phase 1. RPN 6 (MPE) belongs to Phase 3.
- **Generated chart:** `chimera-utils nrpn-chart` prints the fixture's lines with labels and ranges for the docs. The Hapax, midi.guide and Ctrlr generators are Phase 3 and read the same function.

## 6. The binding model (ADR 0217)

### 6.1 What stays from ADR 0139
- The map is in the Sound, holds 16 entries, and the Part's channel is the binding channel. There is no omni. Parts sharing a channel each apply their own map.
- Learn is MIX+SEQ on the focused cell, and the target whitelist is `Mappable`.
- The ramp over inter-arrival time (15–120 ms), the `MOVES − 1` budget, stepped binning and the dead band all stay.
- Bindings name no source. Phase 2 resolves a USB cable to a Part before channel routing (report), so a binding never names DIN or USB **(my call)**.

### 6.2 What a control does

| Input | Incoming | Becomes |
|---|---|---|
| `Cc(_, Absolute(t))` | `Ctl::Cc` value v | `take(t, Place(v<<7 \| v))` → `Action::Place`, scaled into `span` |
| `Cc14(m, t)` | CC m, then CC m+32 | MSB alone: `Place(v<<7 \| v)` at once. LSB: `Place(msb<<7 \| lsb)`. Both through `take` |
| `Cc(_, Relative(e))` | v | `Action::Detents(e.decode(v))`: one unit = one panel detent through `edit_addr`, so a relative knob turns exactly as the front-panel encoder does, budget refusals included |
| `Cc(_, Button(Momentary))` | v ≥ 64 / < 64 | `span.max` / `span.min` |
| `Cc(_, Button(Toggle))` | a rising edge (< 64 → ≥ 64) | flips between `span.min` and `span.max` |
| `Nrpn(n, Absolute(t))` | `Ctl::Data` for `Nrpn(n)` | `law::place_of(Place14, ..)` → `take` → `Place` |
| `Nrpn(n, Relative(e))` | `Data { Coarse(v) }` | `Detents(e.decode(v))` |
| `Nrpn(n, IncDec)` | `Ctl::Step` | `Detents(±1)` |
| `Pressure(t)` | `Ctl::Pressure(v)` | as an absolute CC |

- **Takeover.** `Pickup` engages when the incoming place is within `tol` of the param's place (2/127 of the span, scaled to 14 bits) or crosses it between two messages.
  - Until it engages, the focus band shows the hint (§ 8.3), and nothing moves.
  - Any other change to the param re-arms pickup: the panel, a load, another binding, an echo.
  - `Scale` converges as in § 4.1. `Jump` moves at once, and the move is still a glide, because every write goes through the ramp and the knob glide (CLAUDE.md "never snap").
- **Defaults** **(my call)**:
  - **Takeover: `Jump`** (owner, 2026-10-25). It continues ADR 0139 and matches the Elektron precedent (follow-hardware-sequencer-precedent).
  - **Echo** (owner, 2026-10-25): `Feedback::Off` by default. Learn sets `Feedback::Value` automatically when it senses a relative encoder (any `ByteEncoding`, or NRPN `IncDec`) or a 14-bit control (`Cc14`, or an NRPN absolute control that sent a CC 38 fine value). Absolute 7-bit CC, button and pressure bindings learn with echo off. MIDI MAP's F (ECHO) overrides per binding; E (INPUT) never changes F.
  - INIT's and the factory Sounds' default maps are `Cc(_, Absolute(Jump))` with `Feedback::Off`.
- **Precedence** in one pass:
  1. The hand beats a CC (ADR 0139).
  2. An explicit binding beats the canonical NRPN.
  3. Bindings to one param from several controls each act, and the last one heard wins.

### 6.3 Learn senses the kind

The report recommends asking the user to turn clockwise and then counter-clockwise, because one byte (01) means +1 in two's complement and −63 in binary offset. Learn therefore stays open after the first message:
1. **Armed.** MIX+SEQ on a mappable cell. The hint is `MIX + SEQ  LEARNS A CC` (unchanged).
2. **Sensing.** The first `Ctl::Cc`, `Data`, `Step` or `Pressure` on the Part's channel writes the entry at once (as today), as `Absolute(Jump)`.
   - The focus band then reads `CC 21 → CUTOFF` over `TURN IT RIGHT, THEN LEFT`. Nothing the sensed control sends is applied while sensing.
   - Up to 24 samples feed `classify`, and the entry's `Input` is rewritten each time the guess changes. The focus band shows the guess in words: `RELATIVE · BINARY OFFSET`, `ABSOLUTE`, `BUTTON`, `14-BIT CC`.
   - A different control heard while sensing restarts sensing on it, except CC n+32 after CC n, which is the LSB half of a 14-bit pair.
3. **Done.** MIX+SEQ, a nav key, a page change or a panel encoder turn ends sensing with the current guess. There is no timer, as ADR 0139 has none.

How `classify` decides (pure, table-tested):
- **Relative:** values repeat (absolute pots don't send equal neighbours) and sit in clusters near 0/127 or near 64. The cluster of the first message is clockwise. With samples from both directions, each `ByteEncoding` decodes them all, and the one that gives every clockwise sample > 0 and every counter-clockwise one < 0 with the least total |d| wins. With one direction only:
  - 01-like samples → `TwosComplement`;
  - 41-like samples → `BinaryOffset` (the more common of each pair);
  - in both cases `Sure::No`, and the hint keeps asking for the left turn.
- **Absolute:** a run of distinct neighbouring values that is monotonic, or monotonic in each direction.
- **Button:** only 0 and 127, or only two values more than 64 apart.
- **14-bit:** CC n is followed by CC n+32 (n in 1..=31, n ≠ 6) on more than half of the MSB messages.
- **NRPN:** `Step` → `IncDec`. `Data` runs are classified as above on their coarse values.

The user corrects any guess on MIDI MAP (§ 8.2).

### 6.4 Persistence

- **New Sound record `Bindings`, tag `0x001F`** (non-critical; no branch defines `0x001F` as of 2026-10-25, re-checked in Task 10). Layout:
  - Byte 0 is the format version (`1`), followed by 8 B an entry:
    `[input tag:3 | kind:5] [number hi] [number lo] [block disk code] [param id] [min] [max] [flags: bit0 feedback]`
  - Neutral is the empty map, which writes no record (ADR 0045/0087). `MAX_SOUND_COST` grows by the difference (`1 + 16 × 8` against `80` B): ≈ +49 B, rounded per `sparse_bound` in Task 10.
- **`0x001E` is read, never written.** Each old entry becomes `Cc(cc, Absolute(Jump))` with `Feedback::Off`. When both records are present, `0x001F` wins.
- **Safety:**
  - A bad length is `Bounds`. An unknown version leaves the map empty, one skip (SOME PARAMS SKIPPED, #816).
  - An entry with an unknown tag or kind, a reserved CC, an MSB outside `Cc14Msb`, an unknown address or a target that isn't `Mappable` is skipped: `LOADED · SOME PARAMS SKIPPED`.
  - A duplicate target keeps the last, as today.
  - The decoders join the existing Sound decode fuzz target.
- **Where the codec runs:** behind the door (`MidiDoor::{decode_map, encode_map}`). With bank 2 closed, neither record is decoded, so the Sound loads with an empty map and a save writes none. This is the closed-door cost of ADR 0198; § 7.3.

## 7. Data flow and placement

### 7.1 Flow

```
DIN byte ─► USART1 ISR (bank 1 stub: drain the RX FIFO into a stack [u8; 16])
              └─► door: MidiDoor::feed(&mut DinPipe, bytes, sinks)            [bank 2]
                    layer 1 Parser ─► RealTime ─► InEvent::Rt ─────────────► NoteQueue ─► audio block
                                  ─► Voice note/bend/CC 64 ─► InEvent ─────► NoteQueue
                                  ─► Mode ─► InEvent::Mode (124–127 → NotesOff)
                                  ─► FF ─► SoundOff + ResetControllers × 16
                                  ─► SysEx ─► sysex::request ─► CtrlEvent::Identity ─► CtrlRing
                    layer 2 Controllers ─► Ctl ─► CtrlEvent::Ctl ─────────────────────► CtrlRing
UI loop ─► MidiUiDoor::hear(CtrlRing.drain())                                  [bank 2]
              ├─ learn sensing (§ 6.3)
              ├─ resolve(binding) / canonical NRPN / RPN 0 ─► the existing ramp, bin and edit path (moved behind the door)
              └─ EchoPlan::pump ─► out::Writer ─► OutRing ─► USART1 TX interrupt (bank 1 stub: door fills the FIFO)
```

### 7.2 Can parsing live behind a door? Yes: one door call per ISR entry

- **Cost.** The USART1 ISR (priority MIDI, level 4, below audio) drains every byte the RX FIFO holds into a stack buffer, then calls `MidiDoor::feed` once. At DIN rate an ISR entry usually holds one byte, so in practice it is about one call per byte.
  - Worst case 3,125 calls/s × (≈ 20 cycles door overhead + ≈ 150 cycles of parse) ≈ 0.53 M cycles/s, **0.11 % of 480 MHz**. These are estimates; Task 8 measures with `MIDI_CYCLES`.
  - Batching harder (an RX FIFO threshold plus receiver timeout) would add up to a byte time (320 µs) of note latency to save cycles nobody misses. Rejected.
  - USB in Phase 2 delivers 64-byte packets, which is one call per packet.
- **Precedent.** The card DMA interrupt already runs bank-2 code through `CardDoor` (ADR 0175). `DOOR_CELL.with` is ISR-safe.
- **Rejected alternative:** parse in the audio block from a raw byte ring (one door call per block). It puts parsing on the audio thread, changes ADR 0019's "parser where the bytes arrive", and moves clock-tick arrival to block granularity.

### 7.3 Bank 1 vs bank 2

| Piece | Bank | Why |
|---|---|---|
| `chimera_midi::value` (U7, U14, Channel, Key, Velocity, Bend) | 1 | Bank-1 code (note queue, voices) already uses today's equivalents. No growth: they replace them. |
| `chimera_midi::{stream, ctl, rel, take, law, learn, out, sysex, sense}` | 2 | New logic. A `bank2.x` glob per module, listed in `check-door-placed`. |
| `chimera_engine::midi_in::{DinPipe, feed}`, `nrpn`, `midi_map::codec` | 2 | `MidiDoor`. |
| `InEvent`, `NoteQueue`, `CtrlRing`, `OutRing`, `CtrlEvent` type | 1 | Shared by the ISR stub, the audio and the UI. Types and rings only (`CtrlRing` replaces `CcRing`). |
| `Binding`, `Input`, `MidiMap` type definitions, `default_for` data | 1 | The Sound holds them, so it must load and copy with bank 2 closed. This is ADR 0198 point 1 ("a closed door's data"), measured in Task 9. |
| MIDI CONFIG › ECHO `ParamSpec` (SYSTEM block `Midi`, param 3) | 1 | Spec data (ADR 0198 point 1). |
| USART1 ISR stub: drain RX, call door, TX FIFO fill, RTO flag | 1 | It is the vector. It shrinks: it loses the parse and route. |
| PB6 AF7 pin and `TE`/`TXFTIE` bits | 1 | Board init, about 40 B: an ADR 0217 exception line. |
| `ui::midi::{hear, resolve, learn sense, echo, map page rows and input}`, plus today's `cc.rs`, `midi_map::ramp` and the map page's code | 2 | `MidiUiDoor` (new slot `+0x300`). Moving today's code out lowers bank 1. |
| Bank-1 UI stubs: `hear` (closed: drain and drop), map page (closed: `NEEDS UPDATE`), echo pump (closed: nothing) | 1 | Door stubs. |
| `chimera-hal::midi::MidiParser`, `MidiMessage`, `midi_in::route`, `from_midi` | deleted | Replaced. Bank 1 drops their bytes. |

**Closed bank 2** (a stale or missing `.bk2`, ABOUT reads `NEEDS UPDATE`):
- The ISR stub drains and drops RX, so **MIDI in is deaf, notes included**, and nothing is transmitted.
- Maps aren't decoded, and MIDI MAP and the learn hint read `NEEDS UPDATE`.
- **Today a closed unit still plays VA's plain modes from MIDI. After Phase 1 it doesn't.** Owner, 2026-10-25: accepted. There is no bank-1 fallback parser; the header's existing bank-2 warning covers it.

**Expected sizes** (estimates; each task reports its measured line per flash-counting-proportion):
- Bank 2 gains about 14–18 KB: parser and controllers 3 KB, binding and UI 6 KB, codec 1 KB, learn 1.5 KB, echo and identity 2 KB, page 2 KB.
- Bank 1 nets about −1 to −2 KB.
- RAM:
  - `CtrlRing` 512 B (replacing `CcRing`'s 256 B), `OutRing` 256 B, `Controllers` about 160 B, the SysEx buffer 32 B;
  - `BindRuntime` 6 × 16 × 8 B ≈ 768 B UI-side;
  - `Binding` grows each Sound copy by at most 16 × 4 B = 64 B.

  Task 9 measures the D2/AXI headroom (ADR 0139 moved `UiState` for 244 B).

## 8. UI

### 8.1 Learn (§ 6.3)
- **Focus band, while sensing:** the label line reads `CC 21 → CUTOFF` (or `NRPN 10:3 → CUTOFF`, `14-BIT CC 2 → CUTOFF`, `PRESSURE → CUTOFF`). The hint line reads `TURN IT RIGHT, THEN LEFT` until `classify` is sure, then the guess in full words (`RELATIVE · TWOS COMPLEMENT`), with `MIX + SEQ  DONE` after it.
- **Refusals** stay as today: `NOT MAPPABLE`, `CC <n> ON CH <c>, PART ON CH <p>`, `CC <n> RESERVED`, `MAP FULL`.

### 8.2 MIDI MAP page (SETTINGS › PART › MIDI MAP)

It is still a SETTINGS list Screen with its own visual language (settings-menu).

- **Row:** `CC 74   CUTOFF            JUMP`
  - The control is one of `CC 74`, `CC 2+34` (a 14-bit pair), `NRPN 10:3` or `PRESSURE`.
  - The target label is as today, dimmed when dead.
  - The input word is right-aligned.
  - An empty map reads `NO CCS · MIX+SEQ ON A CELL`, as today.
- **Keys** (D-pad conventions unchanged; A–F are the six encoders):

  | Key | Sets | Notes |
  |---|---|---|
  | A | the row | as today |
  | B | the control number | Steps CC numbers, skipping reserved ones, and CC14 MSBs. An NRPN or PRESSURE control is fixed: re-learn to change it, and B shows `RELEARN TO CHANGE` in the hint. |
  | C / D | MIN / MAX | as today, places `0-255` |
  | E | INPUT | Cycles only the kinds legal for the control (the `Input` type): CC → JUMP, PICKUP, SCALE, the four relatives, MOMENTARY, TOGGLE. CC14 and PRESSURE → JUMP, PICKUP, SCALE. NRPN → the CC list plus INC DEC. |
  | F | ECHO | `OFF` / `ON` |
  | MIX+MINUS | delete | as today, behind the shared confirm |

- **Input words.** Full words where they fit (full-word-labels). The row shows the short form and the focus/hint line spells it out:

  | `Input` kind | Row | Focus line |
  |---|---|---|
  | `Absolute(Jump)` | `JUMP` | `ABSOLUTE · JUMPS TO THE KNOB` |
  | `Absolute(Pickup)` | `PICKUP` | `ABSOLUTE · WAITS TO PICK UP` |
  | `Absolute(Scale)` | `SCALE` | `ABSOLUTE · SCALES TO MEET` |
  | `Relative(TwosComplement)` | `TWOS COMP` | `RELATIVE · TWOS COMPLEMENT  +1 = 1  -1 = 127` |
  | `Relative(BinaryOffset)` | `OFFSET` | `RELATIVE · BINARY OFFSET  +1 = 65  -1 = 63` |
  | `Relative(SignBitDown)` | `SIGNED` | `RELATIVE · SIGNED BIT  +1 = 1  -1 = 65` |
  | `Relative(SignBitUp)` | `SIGNED 2` | `RELATIVE · SIGNED BIT 2  +1 = 65  -1 = 1` |
  | `IncDec` | `INC DEC` | `RELATIVE · NRPN INCREMENT AND DECREMENT` |
  | `Button(Momentary)` | `MOMENTARY` | `BUTTON · HELD IS MAX` |
  | `Button(Toggle)` | `TOGGLE` | `BUTTON · EACH PRESS FLIPS` |

  The byte values in the focus line come from the controllers' manuals (EC4 CCr1/CCr2, BCR relative-1/2/3, Mackie), so a user can match the manual's wording.
- **Hint line (ADR 0086):** `B CONTROL  C MIN  D MAX  E INPUT  F ECHO`, then `MIX+MINUS DELETES`.
- **Closed bank 2:** the screen reads `NEEDS UPDATE`.

### 8.3 Pickup hint on the sound pages
When the focused cell's param has a `Pickup` binding that is waiting, the focus band's hint reads `MOVE THE KNOB UP TO PICK UP` or `MOVE THE KNOB DOWN TO PICK UP` (hidden-gestures-need-hints). It clears when the binding engages.

### 8.4 SETTINGS › MIDI CONFIG › ECHO
- A new row in SYSTEM, `ECHO` `OFF` / `ON`, default `ON`. It is the master switch for DIN OUT: off sends nothing, identity replies included.
- The per-binding `Feedback` decides what is echoed, and it defaults to off (§ 6.2), so out of the box only learned relative and 14-bit bindings echo. The master defaults to `ON` so that the owner's auto-ON rule works without a second setting.
- An older SYSTEM file without the param reads the default.

## 9. Value echo out of DIN

- **Pull, not push.** Each UI pass, `EchoPlan::pump` compares each feedback binding's param, quantised to its control's resolution, against `echo_last`. A difference marks it dirty.
  - No edit path needs a hook. Panel turns, loads, Revert, other bindings and ORBIT's writes are all caught.
  - The pass costs 6 × 16 compares.
- **No echo to the hand.** For 250 ms after a binding last received (`quiet_until`), its own echo is held, and its `echo_last` is set to what it received. The ramp's intermediate values therefore never fight the controller **(my call: 250 ms)**. Other bindings to the same param still echo.
- **Coalescing.** The queue is the dirty set, not messages, so the latest value always wins and nothing stale is ever sent. Messages are rendered into the `OutRing` only while it has room for a whole message.
- **Priority** each pass:
  1. a pending identity reply;
  2. the active Part's dirty bindings;
  3. the other Parts in Part order;
  4. burst entries.
- **Bursts.** On a Part change, a Sound load (`Loads` changed) or a project load, every feedback binding of the affected Part is marked dirty (its `echo_last` cleared). A page change doesn't burst: bindings belong to the Sound, not the page. Surfaces (Phase 4) add page bursts.
- **Channel and collisions.** The echo goes out on the Part's channel, on the binding's own control and encoding:
  - CC: the value.
  - CC14: MSB then LSB.
  - NRPN: 99, 98, 6 and 38 with running status.
  - Button: `span` min or max as 0 or 127.
  - Relative bindings with `Feedback::Value`: the absolute value on the same CC (the controller's LED ring follows). Learn turns it on (§ 6.2).

  When Parts share a channel and bind the same control, only the lowest-numbered Part echoes **(my call)**.
- **Bandwidth** at 31,250 baud (320 µs a byte; arithmetic, not measurements):

  | Message | Bytes (running status) | Time |
  |---|---|---|
  | CC | 3 (2) | 0.96 (0.64) ms |
  | 14-bit CC | 6 (4) | 1.9 (1.3) ms |
  | NRPN, 14-bit | 12 (9) | 3.8 (2.9) ms |
  | Identity reply | 15 | 4.8 ms |
  | Burst: 16 CC bindings | 33 | 10.6 ms |
  | Burst: 16 NRPN bindings | 145 | 46 ms |
  | Six Parts × 16 NRPN (project load) | 870 | 0.28 s |

  - The `OutRing` holds 256 B, about 82 ms of line. A full ring defers dirty bindings to later passes; it never drops them.
  - One continuously turned panel encoder bound to a CC costs at most one message per UI pass, about 3 B every 10–20 ms: under 10 % of the line.
  - DIN OUT carries nothing else in Phase 1. A rate cap waits for Phase 2's clock or thru output.
- **Feedback loops.** An echo that a merger loops back arrives equal to the stored value. Equal repeats are ignored (ADR 0139), so the loop stops after one round.

## 10. Identity

- **Request:** `F0 7E <7F|00> 06 01 F7`.
- **Reply:** `F0 7E 00 06 02 <maker> <family lo hi> <member lo hi> <v1 v2 v3 v4> F7`.
  - `maker` = `7D` (owner, 2026-10-25: non-commercial for now; swap for a bought MMA ID before anything is sold). `family` = `43 48` ("CH"). `member` = `01 00` (PreenFM3 hardware; the Blackbox port takes `02 00`). The version bytes are major, minor, patch, 0 from `CARGO_PKG_VERSION` (0.3.0 → `00 03 00 00`).
  - It is 15 bytes, queued first in the echo plan, and sent only when MIDI CONFIG › ECHO is on.

## 11. Stream rules the instrument now obeys

- **124–127** (Omni Off, Omni On, Mono On, Poly On) act only as their implied All Notes Off. Chimera stays Mode 3 per Part. Mono On as a mono switch is Phase 3 at the earliest. **122** (Local Control) is ignored: there is no keyboard.
- **FF** (Reset) → All Sound Off plus Reset All Controllers on all 16 channels: 32 `InEvent::Mode`. Never a reboot.
- **Active Sensing.** Once an FE has arrived, the ISR stub arms USART1's receiver timeout at 9,375 bit times (300 ms).
  - On timeout, `MidiDoor::idle` pushes All Notes Off on all 16 channels and disarms until the next FE.
  - It is All Notes Off, not Sound Off: a pulled cable releases notes with their tails **(my call)**, and Modal rings on (release-keeps-ringing).
  - The desktop doesn't arm: midir has no idle callback.
- **Faults** are counted per kind in `PipeStats` and shown on the console `status` line, beside today's `MIDI_ERRORS`.

## 12. Test strategy (host first; hardware only at ship)

- **Layer 1:**
  - Table tests, one per rule in § 4.1: `90 F8 3C F8 64` → Clock, Clock, NoteOn; bend `00 00` / `00 40` / `7F 7F` → −8192 / 0 / +8191; SysEx ended by F7, by a status and by overflow; F4/F5 cancel running status; F9/FD are ignored.
  - Proptest:
    - arbitrary bytes never panic;
    - inserting any F8–FF byte anywhere leaves the non-real-time event sequence unchanged;
    - a stream re-encoded without running status parses to the same events.
  - `cargo fuzz` target `chimera-midi/fuzz` (`stream_ctl`), excluded from the workspace like `chimera-core/fuzz`.
- **Layer 2:**
  - Interleaved 99/98 then 101/100 ends on RPN, and the null makes data entry inert.
  - CC 6 alone works, an MSB clears the LSB, and 96/97 work on RPN 0.
  - NRPN running-status streams (`B0 63 xx 62 yy 06 zz 26 ww`).
  - DAW-thinned sequences: a repeated 98 without 99 keeps the old MSB.
  - Per-channel isolation, and the bank latch with each half alone.
  - Proptest: no panic; channel n's output never depends on channel m's input.
- **Arithmetic:**
  - Every encoding at ±1, ±n and edge bytes 0/63/64/65/127.
  - `classify` against byte sequences written from the BCR, EC4 and Mackie manuals (fixtures). Real captures are added when hardware is on the bench.
  - Pickup crossing both ways, within tolerance, and re-arming.
  - Scale: monotonic, and both ends meet.
  - Law: `Coarse(127)` → 16383 for `Place14`; `Code7` with a retired code → `None`.
- **NRPN chart:** `nrpn_chart_test` (append-only fixture); `every_mappable_has_an_address`; `addresses_round_trip`; `msb_lsb_bounds`.
- **Codec:**
  - Round trips: `0x001F` → map → `0x001F` is byte-identical.
  - A `0x001E`-only Sound loads as Jump bindings.
  - Both records present: `0x001F` wins.
  - Truncated or unknown entries are skipped and counted.
  - The neutral fixtures stay byte-identical.
  - The Sound decode fuzz target gains both records.
  - Closed door: an empty map, no panic.
- **UI:**
  - `pickup_never_moves_before_engaging`.
  - `relative_turns_match_panel_detents`: the same `edit_addr` calls as encoder turns.
  - `cc14_msb_applies_without_waiting`.
  - `canonical_nrpn_moves_mappable_only`.
  - `explicit_binding_beats_canonical`.
  - `rpn0_sets_bend`.
  - `learn_senses_relative_offset` and the other kinds.
  - `learn_restarts_on_another_control`.
  - Echo: `echo_never_returns_to_the_hand`, `echo_coalesces_latest`, `echo_burst_on_load_fits_ring`, `identity_first`.
  - The existing ADR 0139 review-focus tests keep passing: `cc_flood_never_drops_note_ons`, `cc_sweep_never_snaps`, `ramps_reset_on_load`, `dead_entries_after_engine_switch`, `learn_cancels_on_nav`.
- **Door:** `midi_door_test` (open plays as before the move, closed is deaf), plus `check-door-placed` patterns, `door-check` and `check-bank1` on every set.
- **Desktop QA (qa-before-uat):**
  - The simulator takes midir in and, new, out (`--midi-out <name>`).
  - Claude drives it from a virtual port with recorded controller byte streams (a BCR relative-2 sweep, an EC4 14-bit sweep, a Hapax NRPN chart sweep) and checks the echoes with `aseqdump`.
  - The owner does UAT by ear and hand.
- **Unit (ship gate, unit-e2e-test):** flash, check the MIDI channel against the Part, play notes, sweep a learned CC, a relative encoder and an NRPN, and capture the echo and identity reply on the MOTU's MIDI in.

## 13. Owner answers (2026-10-25, binding)

1. **Identity manufacturer ID:** `0x7D` for now. Swap it for a bought MMA ID before anything is sold.
2. **Closed bank 2:** MIDI in is ignored, notes included. No bank-1 fallback parser. The header's existing bank-2 warning covers it.
3. **Takeover default:** `JUMP`.
4. **Value echo:** off by default per binding, but on automatically for bindings learned as relative encoders or 14-bit (CC14, NRPN 14-bit). MIDI MAP's per-binding ECHO overrides. The SYSTEM ECHO master stays `ON` (§ 8.4).
