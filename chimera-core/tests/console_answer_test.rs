use chimera_core::boot::{BootAction, BootSeen, UsbRegs};
use chimera_core::clock_plan::SiliconRev;
use chimera_core::console::{
    Bank2Answer, Bank2Audio, Bank2Fail, Bank2Refusal, CardAnswer, CardFail, CardSpeed, Colours,
    Command, Console, FlashPeek, FlashRegs, Frame, Hands, NoArg, Out, PerMille, QUEUE, Request,
    SHOT_HEADER, Stalled, Stats, StreamTally, SweepAsk, Unit, UsbTally, answer, check_staged,
    installed,
};
use chimera_core::perf::load::AudioStats;
use chimera_core::reset::ResetCause;
use chimera_core::ui::UiState;
use chimera_core::ui::theme_settings::{ThemeSettings, palette};
use chimera_hal::FB_SIZE;
use chimera_hal::store::{
    Doubt, Mark, StoreError, Sweep, SweepRun, SweepStats, Swept, Then, Trigger,
};
use chimera_hal::units::{Micros, Millis};

struct Sink(Vec<u8>);
impl Out for Sink {
    fn now(&self) -> chimera_hal::Ms {
        chimera_hal::Ms(0)
    }
    fn put(&mut self, b: &[u8]) -> Result<(), Stalled> {
        self.0.extend_from_slice(b);
        Ok(())
    }
}

struct Fake {
    ui: Box<UiState>,
    fb: Box<[u16; FB_SIZE]>,
    stats: Option<Stats>,
    bench: Option<String>,
    card: Option<CardAnswer>,
    stats_reads: usize,
    dfu: Option<()>,
    dfu_calls: usize,
    boot: Option<BootSeen>,
    sweep: Option<SweepRun>,
    board: Option<chimera_core::board::BoardId>,
    sweep_ask: Option<SweepAsk>,
    sweeps: usize,
    usb_regs: Option<UsbRegs>,
    flash: Option<(FlashRegs, FlashPeek)>,
    forgotten: usize,
    bank2: Option<Bank2Answer>,
    installs: usize,
    hands: Option<Hands>,
    midi: Option<chimera_core::console::MidiStats>,
}

impl Fake {
    fn new() -> Fake {
        Fake {
            ui: Box::new(UiState::new()),
            fb: Box::new([0xBEEF; FB_SIZE]),
            stats: None,
            bench: None,
            card: None,
            stats_reads: 0,
            dfu: None,
            dfu_calls: 0,
            boot: None,
            sweep: None,
            board: None,
            sweep_ask: None,
            sweeps: 0,
            usb_regs: None,
            flash: None,
            forgotten: 0,
            bank2: None,
            installs: 0,
            hands: None,
            midi: None,
        }
    }
}

impl Unit for Fake {
    fn ui(&self) -> &UiState {
        &self.ui
    }
    fn stats(&mut self) -> Option<Stats> {
        self.stats_reads += 1;
        self.stats
    }
    fn bench(&self) -> Option<&str> {
        self.bench.as_deref()
    }
    fn card(&mut self) -> Option<CardAnswer> {
        self.card
    }
    fn frame(&self) -> Frame<'_> {
        Frame {
            fb: &self.fb,
            palette: palette(&ThemeSettings::DEFAULT),
        }
    }
    fn dfu(&mut self) -> Option<()> {
        self.dfu_calls += 1;
        self.dfu
    }
    fn boot(&self) -> Option<BootSeen> {
        self.boot
    }
    fn sweep(&self) -> Option<SweepRun> {
        self.sweep
    }
    fn board(&self) -> Option<chimera_core::board::BoardId> {
        self.board
    }
    fn request_sweep(&mut self) -> Option<SweepAsk> {
        self.sweeps += 1;
        self.sweep_ask
    }
    fn usb_regs(&self) -> Option<UsbRegs> {
        self.usb_regs
    }
    fn midi(&self) -> Option<chimera_core::console::MidiStats> {
        self.midi
    }
    fn flash_regs(&self) -> Option<FlashRegs> {
        self.flash.map(|f| f.0)
    }
    fn flash_peek(&self) -> Option<FlashPeek> {
        self.flash.map(|f| f.1)
    }
    fn hands(&mut self) -> Option<&mut Hands> {
        self.hands.as_mut()
    }
    fn bank2_install(&mut self) -> Option<Bank2Answer> {
        self.installs += 1;
        self.bank2
    }
    type Files = chimera_core::console::NoCard;
    fn files(&mut self) -> Option<&mut Self::Files> {
        None
    }
    fn forget_library(&mut self) {
        self.forgotten += 1;
    }
}

fn audio() -> AudioStats {
    AudioStats::new(
        SiliconRev::V,
        chimera_core::board::BoardId::Lqfp176,
        480_000_000,
        ResetCause::Watchdog,
    )
}

fn ask(u: &mut Fake, line: &str) -> String {
    let mut c = Console::new();
    let mut s = Sink(Vec::new());
    for &b in line.as_bytes() {
        if let Some(r) = c.push(b) {
            answer(r, u, &mut s).unwrap();
        }
    }
    // Lossy: a shot's body is not text.
    String::from_utf8_lossy(&s.0).into_owned()
}

#[test]
fn help_lists_the_table_in_order() {
    let mut u = Fake::new();
    let mut want = String::from("chimera console 1\n");
    for c in Command::ALL {
        want += &format!("{:<8}{}\n", c.name(), c.about());
    }
    want += "OK\n";
    assert_eq!(ask(&mut u, "help\n"), want);
}

#[cfg(all(feature = "card-put", not(feature = "fault-inject")))]
#[test]
fn help_is_the_specs_text() {
    let mut u = Fake::new();
    assert_eq!(
        ask(&mut u, "help\n"),
        "chimera console 1\n\
         help    this list\n\
         status  firmware, project, Part and where the UI is\n\
         stats   AUDIO LOAD and the UI loop's time\n\
         bench   the bench's numbers (bench builds)\n\
         card    SD speed and streams, a chord playing; clean (bench builds)\n\
         shot    the screen in THEME's colours; shot raw: canonical\n\
         dfu     restart into the ROM loader for just flash\n\
         sweep   the lost-cluster sweep at the next boot: restarts into it\n\
         flash   flash controller and bank 2, read only\n\
         bank2   bank 2: its state; bank2 install: the staged file into it\n\
         key     B1-B6 MENU MINUS PLUS MIX EDIT SEQ PUSH1-6, A+B [hold_ms]\n\
         turn    <encoder 1-6> <delta -64..64>: detents at a hand's pace\n\
         ls      files and sizes in PROJECTS; ls tapes, ls sounds: theirs\n\
         put     a project, Sound or source: <path> <size> <crc32> [--over]\n\
         rm      a library Sound's pair: /CHIMERA/SOUNDS/S#######\n\
         get     a library Sound's file, base64: /CHIMERA/SOUNDS/S#######\n\
         OK\n"
    );
}

#[test]
fn each_refusal_is_one_err_line() {
    let mut u = Fake::new();
    assert_eq!(
        ask(&mut u, "frob\n"),
        "ERR unknown command frob, try help\n"
    );
    assert_eq!(
        ask(&mut u, "abcdefghijklmnopqrstuvwxyz\n"),
        "ERR unknown command abcdefghijklmnop, try help\n"
    );
    assert_eq!(
        ask(&mut u, "status now\n"),
        "ERR status takes no arguments\n"
    );
    assert_eq!(ask(&mut u, "shot x\n"), "ERR shot takes raw or nothing\n");
    assert_eq!(
        ask(&mut u, &format!("{}\n", "y".repeat(65))),
        "ERR line too long, 64 max\n"
    );
}

#[test]
fn none_from_the_unit_is_one_err_line() {
    let mut u = Fake::new();
    assert_eq!(ask(&mut u, "stats\n"), "ERR stats is not in this build\n");
    assert_eq!(ask(&mut u, "bench\n"), "ERR bench is not in this build\n");
    assert_eq!(ask(&mut u, "card\n"), "ERR card is not in this build\n");
}

/// Every phase measured.
fn full() -> CardSpeed {
    use chimera_core::console::{CardInfo, CardKind, Files, Latency, Load, Throughput};
    let tp = Some(Throughput {
        bytes: 1_440_000,
        us: 1_000_000,
    });
    CardSpeed {
        seed: 7,
        card: Some(CardInfo {
            kind: CardKind::Sdhc,
            bytes: 15_931_539_456,
            free_bytes: Some(15_000_000_000),
        }),
        files: Some(Files::default()),
        seq: [tp; 3],
        random: Some(Latency::default()),
        interleaved: tp,
        // A p95 round of 8 × 4 KB in 1 ms: 32.8 MB/s, 455 streams at half.
        rounds: Some(Latency {
            p95_us: 1_000,
            ..Latency::default()
        }),
        load: Some(Load::default()),
        overruns: 0,
    }
}

#[test]
fn card_fails_as_one_err_line() {
    let mut u = Fake::new();
    u.card = Some(CardAnswer::Cleaned(Err(CardFail::Store(
        chimera_hal::store::StoreError::Timeout,
    ))));
    assert_eq!(ask(&mut u, "card clean\n"), "ERR card: CARD TIMEOUT\n");
    u.card = Some(CardAnswer::Cleaned(Ok(())));
    assert_eq!(ask(&mut u, "card clean\n"), "OK\n");
}

#[test]
fn a_card_stopped_short_keeps_what_came_before() {
    let mut u = Fake::new();
    let mut s = full();
    (s.random, s.interleaved, s.rounds, s.load) = (None, None, None, None);
    u.card = Some(CardAnswer::Measured(s, Some(CardFail::Timeout)));
    let got = ask(&mut u, "card seed=7\n");
    assert!(
        got.starts_with("seed 7\ncard_type SDHC\ncard_mb 15931\nfree_mb 15000\nfile_kb 8388\n"),
        "{got}"
    );
    assert!(got.contains("seq_16k_kb_s 1440\n"), "{got}");
    assert!(!got.contains("rand_") && !got.contains("streams"), "{got}");
    assert!(got.ends_with("overruns 0\nERR card: timeout\n"), "{got}");
    u.card = Some(CardAnswer::Measured(
        CardSpeed::default(),
        Some(CardFail::NoAudio),
    ));
    assert_eq!(
        ask(&mut u, "card\n"),
        "seed 0\noverruns 0\nERR card: audio is not running\n"
    );
}

#[test]
fn card_is_key_value_lines_then_ok() {
    let mut u = Fake::new();
    u.card = Some(CardAnswer::Measured(full(), None));
    let got = ask(&mut u, "card\n");
    assert!(
        got.starts_with("seed 7\n") && got.ends_with("streams 455\nOK\n"),
        "{got}"
    );
    assert!(
        got.lines().all(|l| l == "OK" || l.split(' ').count() == 2),
        "{got}"
    );
}

#[test]
fn stats_reads_audio_load_and_the_loop() {
    let mut u = Fake::new();
    let mut a = audio();
    (
        a.load_avg,
        a.load_peak,
        a.overruns,
        a.desyncs,
        a.sources,
        a.stack_used,
    ) = (23, 41, 0, 2, 2, 12_288);
    (a.dma_errors, a.fifo_errors) = (3, 4);
    a.drops = [5, 7];
    a.record_voices(5, 60, false);
    a.record_voices(3, 47, false);
    u.stats = Some(Stats {
        audio: a,
        loop_avg_us: 812,
        loop_peak_us: 4210,
        streams: None,
        usb: None,
    });
    assert_eq!(
        ask(&mut u, "stats\n"),
        "load_pct 23\npeak_pct 41\noverruns 0\ndrops 5 7\ndesyncs 2\ndma_errors 3\nfifo_errors 4\nstack_bytes 12288\nvoices 3 5 of 8\ncost_pct 47\nloop_avg_us 812\nloop_peak_us 4210\nOK\n"
    );
    for (sources, want) in [
        (1, "\ndrops 5\n"),
        (0, "\ndrops -\n"),
        (200, "\ndrops 5 7\n"),
    ] {
        a.sources = sources;
        u.stats = Some(Stats {
            audio: a,
            loop_avg_us: 0,
            loop_peak_us: 0,
            streams: None,
            usb: None,
        });
        assert!(ask(&mut u, "stats\n").contains(want), "{sources} sources");
    }
}

/// A build that streams adds the streams' and the card IRQ's lines after
/// the loop's (slice 1 § 8, § 9, § 12).
#[test]
fn stats_prints_the_streams_lines() {
    let mut u = Fake::new();
    u.stats = Some(Stats {
        audio: audio(),
        loop_avg_us: 1,
        loop_peak_us: 2,
        streams: Some(StreamTally {
            underruns: 3,
            cold: 17,
            unplanned: 1,
            card_faults: 7,
            frame_errors: 0,
            unreadable: 2,
            held_at_cap: 4,
            late_refused: 5,
            quiet_ms_max: Millis::new(38),
            quiets: 6,
            irq_us_max: Micros::new(312),
            irq_own_us_max: Micros::new(140),
            irq_share: PerMille::of(4, 1_000),
        }),
        usb: None,
    });
    let got = ask(&mut u, "stats\n");
    assert!(
        got.ends_with(
            "loop_avg_us 1\nloop_peak_us 2\n\
             stream_underruns 3\nstream_cold 17\nstream_unplanned 1\nstream_frame_errors 0\n\
             stream_unreadable 2\nstream_held_at_cap 4\nstream_late_refused 5\n\
             card_faults 7\ncard_quiet_ms_max 38\ncard_quiets 6\n\
             card_irq_us_max 312\ncard_irq_own_us_max 140\ncard_irq_pct 0.4\nOK\n"
        ),
        "{got}"
    );
}

/// A build with the USB interrupt ends on its three lines.
#[test]
fn stats_prints_the_usb_lines() {
    let mut u = Fake::new();
    u.stats = Some(Stats {
        audio: audio(),
        loop_avg_us: 1,
        loop_peak_us: 2,
        streams: None,
        usb: Some(UsbTally {
            irq_share: PerMille::of(12, 1_000),
            irq_us_max: Micros::new(41),
            storms: 3,
            midi_drops: 5,
        }),
    });
    let got = ask(&mut u, "stats\n");
    assert!(
        got.ends_with(
            "loop_peak_us 2\nusb_irq_pct 1.2\nusb_irq_us_max 41\nusb_storms 3\nusb_midi_drops 5\nOK\n"
        ),
        "{got}"
    );
}

/// The card IRQ's share, in tenths of a percent: rounded to nearest,
/// capped at the whole, nothing of nothing.
#[test]
fn irq_share_rounds_in_tenths() {
    let pct = |p: PerMille| p.to_string();
    assert_eq!(pct(PerMille::of(4, 1_000)), "0.4");
    assert_eq!(pct(PerMille::of(1_049, 100_000)), "1.0");
    assert_eq!(pct(PerMille::of(1_051, 100_000)), "1.1");
    assert_eq!(pct(PerMille::of(0, 0)), "0.0");
    assert_eq!(pct(PerMille::of(9, 1)), "100.0");
    assert_eq!(pct(PerMille::of(u64::MAX, u64::MAX)), "100.0");
}

#[test]
fn only_stats_reads_the_stats() {
    let mut u = Fake::new();
    for l in ["help\n", "status\n", "bench\n", "shot\n", "dfu\n", "nope\n"] {
        ask(&mut u, l);
    }
    assert_eq!(u.stats_reads, 0);
    ask(&mut u, "stats\n");
    assert_eq!(u.stats_reads, 1);
}

#[test]
fn dfu_is_ok_then_the_unit_is_told_once() {
    let mut u = Fake::new();
    u.dfu = Some(());
    assert_eq!(ask(&mut u, "dfu\n"), "OK\n");
    assert_eq!(u.dfu_calls, 1);
    assert_eq!(ask(&mut u, "DFU now\n"), "ERR dfu takes no arguments\n");
    assert_eq!(u.dfu_calls, 1, "a refusal arms nothing");
}

#[test]
fn dfu_without_the_chip_is_one_err_line() {
    let mut u = Fake::new(); // dfu None
    assert_eq!(ask(&mut u, "dfu\n"), "ERR dfu is not in this build\n");
}

#[test]
fn bench_is_its_text_then_ok() {
    let mut u = Fake::new();
    u.bench = Some("# VOICES\nALG 1  12 24 36\n".into());
    assert_eq!(ask(&mut u, "bench\n"), "# VOICES\nALG 1  12 24 36\nOK\n");
    u.bench = Some("# VOICES\nALG 1".into());
    assert_eq!(ask(&mut u, "bench\n"), "# VOICES\nALG 1\nOK\n");
    u.bench = Some(String::new());
    assert_eq!(ask(&mut u, "bench\n"), "OK\n");
}

#[test]
fn status_is_its_lines_then_ok() {
    let mut u = Fake::new();
    let mut want = String::new();
    chimera_core::console::write_status(&u.ui, &mut want).unwrap();
    assert_eq!(ask(&mut u, "status\n"), want + "OK\n");
}

#[test]
fn status_says_how_bank2_judged() {
    let mut u = Fake::new();
    u.ui.set_bank2(chimera_core::bank2::Bank2Status::Empty);
    let a = ask(&mut u, "status\n");
    assert!(a.contains("\nbank2 empty\n"), "{a}");
    assert!(a.ends_with("OK\n"));
}

#[test]
fn the_chips_status_ends_with_its_boot_line() {
    let mut u = Fake::new();
    let seen = BootSeen {
        marker: 0,
        readback: 0,
        action: BootAction::Synth,
        rsr: 0x00e6_0000,
        dbp: false,
        boots: 1,
        from: 0,
        jump_rsr: 0,
        last_stage: 0,
        last_usb: 0,
        last_usb_step: 0,
        last_usb_regs: Default::default(),
    };
    u.boot = Some(seen);
    let mut want = String::new();
    chimera_core::console::write_status(&u.ui, &mut want).unwrap();
    assert_eq!(ask(&mut u, "status\n"), format!("{want}{seen}\nOK\n"));
    u.usb_regs = Some(UsbRegs::from_words([1, 2, 3, 4, 5, 6]));
    assert_eq!(
        ask(&mut u, "status\n"),
        format!(
            "{want}{seen}\nusb_regs gotgctl=00000001 gccfg=00000002 dctl=00000003 \
             gintsts=00000004 dsts=00000005 pwr_cr3=00000006\nOK\n"
        )
    );
}

#[test]
fn status_has_the_midi_pipe_before_usb_regs() {
    let mut u = Fake::new();
    let mut want = String::new();
    chimera_core::console::write_status(&u.ui, &mut want).unwrap();
    u.midi = Some(chimera_core::console::MidiStats {
        faults: 3,
        drops: 7,
        ctrl_drops: 9,
    });
    u.usb_regs = Some(UsbRegs::from_words([1, 2, 3, 4, 5, 6]));
    let a = ask(&mut u, "status\n");
    assert!(
        a.starts_with(&format!(
            "{want}midi faults=3 drops=7 ctrl_drops=9\nusb_regs "
        )),
        "{a}"
    );
}

#[test]
fn status_names_the_board_and_its_strap_after_at() {
    use chimera_core::board::BoardId;
    let mut u = Fake::new();
    let mut want = String::new();
    chimera_core::console::write_status(&u.ui, &mut want).unwrap();
    for (b, line) in [
        (BoardId::Lqfp176, "board lqfp176 straps=0b0001\n"),
        (BoardId::Lqfp144, "board lqfp144 straps=0b0010\n"),
        (BoardId::Lqfp100, "board lqfp100 straps=0b0011\n"),
        (
            BoardId::from_straps(0b1111),
            "board unknown straps=0b1111\n",
        ),
    ] {
        u.board = Some(b);
        assert_eq!(ask(&mut u, "status\n"), format!("{want}{line}OK\n"));
    }
}

#[test]
fn shot_answers_through_the_units_frame() {
    let mut u = Fake::new();
    let mut s = Sink(Vec::new());
    answer(Ok(Request::Shot(Colours::Raw)), &mut u, &mut s).unwrap();
    assert_eq!(s.0.len(), SHOT_HEADER.len() + 153_600 + 3);
    assert_eq!(&s.0[SHOT_HEADER.len()..][..2], &[0xBE, 0xEF]);
    assert!(s.0.ends_with(b"OK\n"));
}

/// Takes nothing: every put stalls.
struct Wall;
impl Out for Wall {
    fn now(&self) -> chimera_hal::Ms {
        chimera_hal::Ms(0)
    }
    fn put(&mut self, _: &[u8]) -> Result<(), Stalled> {
        Err(Stalled)
    }
}

#[test]
fn a_stall_is_returned_from_every_answer() {
    let mut u = Fake::new();
    u.bench = Some("x\n".into());
    u.stats = Some(Stats {
        audio: audio(),
        loop_avg_us: 1,
        loop_peak_us: 2,
        streams: None,
        usb: None,
    });
    let mut c = Console::new();
    u.card = Some(CardAnswer::Measured(full(), None));
    for &b in b"help\nstatus\nstats\nbench\ncard\nshot\nfrob\n" {
        if let Some(r) = c.push(b) {
            assert_eq!(answer(r, &mut u, &mut Wall), Err(Stalled));
        }
    }
}

/// Takes the first `left` puts whole, then stalls; counts puts made after the stall.
struct Stalls {
    got: Vec<u8>,
    left: usize,
    stalled: bool,
    after: usize,
}
impl Out for Stalls {
    fn now(&self) -> chimera_hal::Ms {
        chimera_hal::Ms(0)
    }
    fn put(&mut self, b: &[u8]) -> Result<(), Stalled> {
        if self.stalled {
            self.after += 1;
            return Err(Stalled);
        }
        if self.left == 0 {
            self.stalled = true;
            return Err(Stalled);
        }
        self.left -= 1;
        self.got.extend_from_slice(b);
        Ok(())
    }
}

#[test]
fn a_stall_mid_text_ends_the_answer_there() {
    let mut u = Fake::new();
    u.bench = Some("# VOICES\nALG 1\n".into());
    u.stats = Some(Stats {
        audio: audio(),
        loop_avg_us: 1,
        loop_peak_us: 2,
        streams: None,
        usb: None,
    });
    for req in [
        Request::Help(NoArg),
        Request::Status(NoArg),
        Request::Stats(NoArg),
        Request::Bench(NoArg),
    ] {
        let mut s = Stalls {
            got: Vec::new(),
            left: 1,
            stalled: false,
            after: 0,
        };
        assert_eq!(answer(Ok(req), &mut u, &mut s), Err(Stalled), "{req:?}");
        assert!(s.stalled, "{req:?} stalled");
        assert_eq!(s.after, 0, "{req:?} put after the stall");
        let got = String::from_utf8(s.got).unwrap();
        assert!(
            !got.contains("OK") && !got.contains("ERR"),
            "{req:?}: {got:?}"
        );
    }
}

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

    /// Up to 200 bytes: 70 % command words, `raw`, spaces, CR and LF; 30 % any byte.
    fn stream(&mut self) -> Vec<u8> {
        const PIECES: [&[u8]; 12] = [
            b"help", b"status", b"stats", b"bench", b"card", b"shot", b"dfu", b"raw", b" ", b"\r",
            b"\n", b"\n",
        ];
        let len = self.below(201);
        let mut v = Vec::with_capacity(len + 8);
        while v.len() < len {
            if self.below(10) < 7 {
                v.extend_from_slice(PIECES[self.below(PIECES.len())]);
            } else {
                v.push(self.next() as u8);
            }
        }
        v.truncate(len);
        v
    }
}

/// The answer with a shot's body cut out: the header, then what follows exactly 153 600 bytes on.
fn strip_shot_body(a: &[u8]) -> String {
    let a = match a.strip_prefix(SHOT_HEADER.as_bytes()) {
        Some(rest) => [SHOT_HEADER.as_bytes(), &rest[153_600..]].concat(),
        None => a.to_vec(),
    };
    String::from_utf8(a).unwrap()
}

#[test]
fn random_streams_never_panic_and_end_each_answer_once() {
    let mut rng = Rng(0x5eed_cafe);
    let mut u = Fake::new();
    u.stats = Some(Stats {
        audio: audio(),
        loop_avg_us: 1,
        loop_peak_us: 2,
        streams: None,
        usb: None,
    });
    let mut answers = 0;
    for _ in 0..5_000 {
        let mut c = Console::new();
        for b in rng.stream() {
            let Some(r) = c.push(b) else { continue };
            let mut s = Sink(Vec::new());
            answer(r, &mut u, &mut s).unwrap();
            let text = strip_shot_body(&s.0);
            let lines: Vec<&str> = text.lines().collect();
            let last = *lines.last().expect("an answer");
            assert!(last == "OK" || last.starts_with("ERR "), "{text}");
            let terminals = lines
                .iter()
                .filter(|l| **l == "OK" || l.starts_with("ERR "))
                .count();
            assert_eq!(terminals, 1, "{text}");
            assert!(
                text.bytes()
                    .all(|b| b == b'\n' || (0x20..=0x7e).contains(&b)),
                "{text:?}"
            );
            answers += 1;
        }
    }
    assert!(answers > 5_000, "{answers} answers");
}

/// The spec's example block for `cmd`: the lines between its heading's
/// fences.
fn spec_block(cmd: &str) -> Vec<&'static str> {
    const SPEC: &str =
        include_str!("../../docs/superpowers/specs/2026-10-02-usb-console-design.md");
    let head = format!("**`{cmd}`**\n\n```\n");
    let at = SPEC.find(&head).unwrap_or_else(|| panic!("no {cmd} block")) + head.len();
    let end = at + SPEC[at..].find("```").unwrap();
    SPEC[at..end].lines().collect()
}

#[test]
fn the_specs_stats_has_every_line_stats_writes() {
    let mut u = Fake::new();
    u.stats = Some(Stats {
        audio: audio(),
        loop_avg_us: 0,
        loop_peak_us: 0,
        streams: None,
        usb: None,
    });
    let key = |l: &str| l.split(' ').next().unwrap().to_owned();
    let got: Vec<_> = ask(&mut u, "stats\n").lines().map(key).collect();
    let spec: Vec<_> = spec_block("stats").into_iter().map(key).collect();
    assert_eq!(got, spec);
    let voices = spec_block("stats")
        .into_iter()
        .find(|l| l.starts_with("voices "));
    let words: Vec<_> = voices.unwrap().split(' ').collect();
    assert_eq!(words[3..], ["of", "8"], "voices <now> <peak> of MAX_VOICES");
}

#[cfg(all(feature = "card-put", not(feature = "fault-inject")))]
#[test]
fn the_specs_help_is_what_help_writes() {
    let mut u = Fake::new();
    let spec = spec_block("help").join("\n") + "\n";
    assert_eq!(ask(&mut u, "help\n"), spec);
}

#[test]
fn the_specs_card_has_every_line_card_writes() {
    let mut u = Fake::new();
    u.card = Some(CardAnswer::Measured(full(), None));
    let key = |l: &str| l.split(' ').next().unwrap().to_owned();
    let got: Vec<_> = ask(&mut u, "card\n").lines().map(key).collect();
    let spec: Vec<_> = spec_block("card").into_iter().map(key).collect();
    assert_eq!(got, spec);
}

/// The volume and phase times the sweep lines carry.
const STATS: SweepStats = SweepStats {
    spc: 64,
    clusters: 488_281,
    fat_bytes: 1_953_792,
    compare_ms: 210,
    mark_ms: 35,
    count_ms: 104,
};
const TAIL: &str = "spc 64 clusters 488281 fat 1953792 compare 210ms mark 35ms count 104ms";

/// The boot's sweep after `boot`, before `usb_regs`: skipped on a clean
/// mark, or why it ran, what it found or why it stopped, the volume, each
/// phase's time, and whether the mark was cleared.
#[test]
fn status_has_the_sweep_after_boot() {
    let mut u = Fake::new();
    u.sweep = Some(SweepRun::Ran {
        mode: Sweep::DryRun,
        why: Trigger::Owed(Mark::Missing),
        seen: Ok(Swept {
            lost: 93,
            freed: 0,
            stale: true,
            healed: false,
            stats: STATS,
        }),
        then: Then::Kept,
    });
    u.usb_regs = Some(UsbRegs::from_words([0; 6]));
    let mut want = String::new();
    chimera_core::console::write_status(&u.ui, &mut want).unwrap();
    let got = ask(&mut u, "status\n");
    let rest = got.strip_prefix(&want).unwrap();
    assert_eq!(
        rest.lines().next().unwrap(),
        format!("sweep dry-run why missing lost 93 freed 0 stale yes healed no {TAIL} then kept")
    );
    assert!(rest.lines().nth(1).unwrap().starts_with("usb_regs "));
    for (run, line) in [
        (
            SweepRun::Ran {
                mode: Sweep::DryRun,
                why: Trigger::Unread,
                seen: Err((Doubt::Card(StoreError::Timeout), STATS)),
                then: Then::Kept,
            },
            format!("sweep dry-run why unread doubt timeout {TAIL} then kept"),
        ),
        (
            SweepRun::Ran {
                mode: Sweep::Free,
                why: Trigger::Owed(Mark::Dirty),
                seen: Err((Doubt::TooMany(9_000), STATS)),
                then: Then::Kept,
            },
            format!("sweep free why dirty doubt too-many lost 9000 {TAIL} then kept"),
        ),
        (
            SweepRun::Ran {
                mode: Sweep::Free,
                why: Trigger::Owed(Mark::Dirty),
                seen: Ok(Swept {
                    lost: 6,
                    freed: 6,
                    stale: false,
                    healed: false,
                    stats: STATS,
                }),
                then: Then::Failed,
            },
            format!("sweep free why dirty lost 6 freed 6 stale no healed no {TAIL} then failed"),
        ),
        (
            SweepRun::Skipped(Mark::Clean),
            "sweep skipped mark clean".to_string(),
        ),
        (
            SweepRun::Skipped(Mark::Unused),
            "sweep skipped mark unused".to_string(),
        ),
    ] {
        u.sweep = Some(run);
        let got = ask(&mut u, "status\n");
        assert!(got.contains(&format!("\n{line}\n")), "{got}");
    }
}

/// `sweep` asks the next boot: its line, then `OK` (the shell restarts);
/// unsaved changes, a card with no `/CHIMERA` or a card fault are an `ERR`;
/// a build without it says so.
#[test]
fn sweep_asks_the_next_boot() {
    let mut u = Fake::new();
    assert_eq!(ask(&mut u, "sweep\n"), "ERR sweep is not in this build\n");
    for (got, want) in [
        (
            SweepAsk::Requested,
            "sweep asked: the next boot sweeps, restarting\nOK\n",
        ),
        (
            SweepAsk::Unsaved,
            "ERR sweep: unsaved changes, save first\n",
        ),
        (
            SweepAsk::Unused,
            "ERR sweep: the card has no /CHIMERA, nothing to sweep\n",
        ),
        (SweepAsk::Failed(StoreError::NoCard), "ERR sweep: NO CARD\n"),
    ] {
        u.sweep_ask = Some(got);
        assert_eq!(ask(&mut u, "sweep\n"), want);
    }
    assert_eq!(ask(&mut u, "sweep now\n"), "ERR sweep takes no arguments\n");
    assert_eq!(u.sweeps, 5, "a refused line never asks");
    ask(&mut u, "status\n");
    assert_eq!(u.sweeps, 5, "status never asks");
}

/// Doubting boots counted, and the warning once the boot stops sweeping.
#[test]
fn status_warns_once_the_sweep_stopped() {
    let mut u = Fake::new();
    let doubt = |n| SweepRun::Ran {
        mode: Sweep::Free,
        why: Trigger::Owed(if n == 1 {
            Mark::Dirty
        } else {
            Mark::Doubted(n - 1)
        }),
        seen: Err((Doubt::CrossLinked, STATS)),
        then: Then::Doubted(n),
    };
    const WARN: &str = "warn sweep stopped: 3 doubting boots in a row; check the card on a computer, then run sweep";
    u.sweep = Some(doubt(2));
    let got = ask(&mut u, "status\n");
    assert!(
        got.contains(&format!(
            "\nsweep free why doubted 1 doubt cross-linked {TAIL} then doubted 2\n"
        )),
        "{got}"
    );
    assert!(!got.contains("warn "), "{got}");
    u.sweep = Some(doubt(3));
    let got = ask(&mut u, "status\n");
    assert!(got.contains(&format!("then doubted 3\n{WARN}\n")), "{got}");
    u.sweep = Some(SweepRun::Skipped(Mark::Doubted(3)));
    let got = ask(&mut u, "status\n");
    assert!(
        got.contains(&format!("\nsweep skipped mark doubted 3\n{WARN}\n")),
        "{got}"
    );
}

/// The boot's toast when the sweep stopped, and none otherwise.
#[test]
fn a_stopped_sweep_toasts_at_boot() {
    use chimera_core::ui::SWEEP_STOPPED;
    use chimera_core::ui::busy::{ToastStep, toast_fits};
    assert!(toast_fits(SWEEP_STOPPED));
    let mut ui = UiState::new();
    ui.note_sweep(&SweepRun::Skipped(Mark::Clean));
    assert_eq!(ui.step_toast(0), ToastStep::Idle);
    ui.note_sweep(&SweepRun::Skipped(Mark::Doubted(3)));
    assert!(matches!(ui.step_toast(0), ToastStep::Show(l) if l.as_str() == SWEEP_STOPPED));
}

/// Card verbs wait while a library load holds the card's work: no card
/// touched, and the page kept.
#[cfg(feature = "card-put")]
#[test]
fn card_verbs_answer_busy_while_a_library_load_is_pending() {
    let mut u = Fake::new();
    u.ui.start_pending_library_load_for_test();
    assert!(u.ui.card_pending());
    for line in [
        "ls sounds",
        "put /CHIMERA/SOUNDS/S0000002.A 10 0",
        "rm /CHIMERA/SOUNDS/S0000001",
        "get /CHIMERA/SOUNDS/S0000001",
        "ls",
        "put /CHIMERA/PROJECTS/P0000001.A 10 0",
    ] {
        assert_eq!(ask(&mut u, &format!("{line}\n")), "ERR busy\n", "{line}");
    }
    assert_eq!(u.forgotten, 0);
    assert_eq!(ask(&mut u, "status\n").lines().last(), Some("OK"));
}

/// No card in the build: nothing reached it, so nothing is forgotten.
#[cfg(feature = "card-put")]
#[test]
fn rm_without_a_card_forgets_nothing() {
    let mut u = Fake::new();
    assert_eq!(
        ask(&mut u, "rm /CHIMERA/SOUNDS/S0000001\n"),
        "ERR rm is not in this build\n"
    );
    assert_eq!(u.forgotten, 0);
}

#[test]
fn flash_is_absent_off_the_chip() {
    assert_eq!(
        ask(&mut Fake::new(), "flash\n"),
        "ERR flash is not in this build\n"
    );
}

#[test]
fn flash_registers_then_peek_then_ok() {
    let mut u = Fake::new();
    u.flash = Some((FlashRegs::default(), FlashPeek::default()));
    let a = ask(&mut u, "flash\n");
    let keys: Vec<_> = a.lines().map(|l| l.split(' ').next().unwrap()).collect();
    assert_eq!(keys.first(), Some(&"acr"));
    assert!(keys.iter().position(|k| *k == "scar_cur2") < keys.iter().position(|k| *k == "peek"));
    assert_eq!(&keys[keys.len() - 3..], ["sr1_after", "sr2_after", "OK"]);
}

use chimera_core::bank2::{Bank2Status, Version};
use chimera_image::{Bank2Sector, Bank2Word, FlashAt, FlashFail, Header, InstallFail, Verdict};

fn v020() -> Version {
    Version::new("0.2.0 02e440e2")
}

const AUDIO: Bank2Audio = Bank2Audio {
    overruns: [3, 3],
    blocks: [10, 1660],
};

#[test]
fn bank2_is_the_boot_line_then_ok() {
    let mut u = Fake::new();
    assert_eq!(ask(&mut u, "bank2\n"), "ERR bank2 is not in this build\n");
    u.ui.set_bank2(Bank2Status::Stale { have: v020() });
    assert_eq!(
        ask(&mut u, "bank2\n"),
        "bank2 stale have 0.2.0 02e440e2\nOK\n"
    );
    assert_eq!(u.installs, 0, "bank2 alone writes nothing");
}

#[test]
fn bank2_install_answers() {
    let fail = |why| Bank2Answer::Failed { why, audio: AUDIO };
    let refused = Bank2Answer::Refused;
    let word = Bank2Word::new(0x0810_0120).unwrap();
    let cases = [
        (
            Bank2Answer::Open {
                version: v020(),
                audio: AUDIO,
            },
            "audio overruns 3 -> 3 blocks +1650\nOK bank2 open 0.2.0 02e440e2\n",
        ),
        (Bank2Answer::Already, "OK already installed\n"),
        (
            refused(Bank2Refusal::Stale(Version::new("0.3.0"))),
            "ERR bank2: staged: needs firmware 0.3.0\n",
        ),
        (
            refused(Bank2Refusal::Nothing),
            "ERR bank2: nothing staged\n",
        ),
        (
            refused(Bank2Refusal::Staged(Bank2Status::Foreign)),
            "ERR bank2: staged: foreign\n",
        ),
        (
            refused(Bank2Refusal::Card(StoreError::NoCard)),
            "ERR bank2: NO CARD\n",
        ),
        (
            refused(Bank2Refusal::Unsealed),
            "ERR bank2: firmware unsealed\n",
        ),
        (refused(Bank2Refusal::Busy), "ERR bank2: busy\n"),
        (
            fail(Bank2Fail::Flash(
                FlashFail::Timeout,
                FlashAt::Sector(Bank2Sector::S0),
            )),
            "audio overruns 3 -> 3 blocks +1650\nERR bank2: flash timeout at S0\n",
        ),
        (
            fail(Bank2Fail::Flash(
                FlashFail::Errors(1 << 17),
                FlashAt::Word(word),
            )),
            "audio overruns 3 -> 3 blocks +1650\nERR bank2: flash errors 00020000 at 08100120\n",
        ),
        (
            fail(Bank2Fail::Flash(
                FlashFail::NotWritable,
                FlashAt::Word(word),
            )),
            "audio overruns 3 -> 3 blocks +1650\nERR bank2: flash not writable at 08100120\n",
        ),
        (
            fail(Bank2Fail::Readback(Bank2Status::Torn)),
            "audio overruns 3 -> 3 blocks +1650\nERR bank2: readback torn\n",
        ),
        (
            fail(Bank2Fail::StagedRead),
            "audio overruns 3 -> 3 blocks +1650\nERR bank2: staged file unreadable\n",
        ),
        (
            fail(Bank2Fail::Door),
            "audio overruns 3 -> 3 blocks +1650\nERR bank2: door check failed\n",
        ),
    ];
    for (a, want) in cases {
        let mut u = Fake::new();
        u.bank2 = Some(a);
        assert_eq!(ask(&mut u, "bank2 install\n"), want, "{a:?}");
        assert_eq!(u.installs, 1);
    }
    let mut u = Fake::new();
    assert_eq!(
        ask(&mut u, "bank2 install\n"),
        "ERR bank2 is not in this build\n"
    );
}

/// `bank2 erase`: the board's outcome, no audio line.
#[cfg(feature = "bank2-gate")]
#[test]
fn bank2_erase_answers() {
    for (r, want) in [
        (Ok(()), "OK bank2 erased\n"),
        (Err(FlashFail::Timeout), "ERR bank2: flash timeout\n"),
        (
            Err(FlashFail::Errors(1 << 17)),
            "ERR bank2: flash errors 00020000\n",
        ),
    ] {
        let mut u = Fake::new();
        u.bank2 = Some(Bank2Answer::Erased(r));
        assert_eq!(ask(&mut u, "bank2 erase\n"), want, "{r:?}");
    }
    let mut u = Fake::new();
    assert_eq!(
        ask(&mut u, "bank2 erase\n"),
        "ERR bank2 is not in this build\n"
    );
}

#[test]
fn every_flash_failure_has_its_words() {
    for (f, want) in [
        (FlashFail::Busy, "busy"),
        (FlashFail::Locked, "locked"),
        (FlashFail::NotWritable, "not writable"),
        (FlashFail::Errors(0x0004_0000), "errors 00040000"),
        (FlashFail::Timeout, "timeout"),
        (FlashFail::NotErased, "not erased"),
        (FlashFail::NoClock, "no cycle counter"),
    ] {
        let mut u = Fake::new();
        u.bank2 = Some(Bank2Answer::Failed {
            why: Bank2Fail::Flash(f, FlashAt::Sector(Bank2Sector::S3)),
            audio: AUDIO,
        });
        let a = ask(&mut u, "bank2 install\n");
        assert!(a.ends_with(&format!("flash {want} at S3\n")), "{a}");
    }
}

fn header() -> Header {
    Header {
        body_len: 4352,
        body_crc: 0x1234_5678,
        door_off: 0x100,
        seal: *b"SEAL0001",
        version: v020(),
    }
}

/// What the staged file's verdict allows, before anything is written.
#[test]
fn the_staged_file_decides_first() {
    let h = header();
    assert_eq!(check_staged(false, &Verdict::Good(h), None), Ok(h));
    assert_eq!(
        check_staged(true, &Verdict::Good(h), None),
        Err(Bank2Answer::Already)
    );
    let refused = |r| Err(Bank2Answer::Refused(r));
    assert_eq!(
        check_staged(false, &Verdict::Unreadable, Some(StoreError::NotFound)),
        refused(Bank2Refusal::Nothing)
    );
    assert_eq!(
        check_staged(true, &Verdict::Unreadable, Some(StoreError::Timeout)),
        refused(Bank2Refusal::Card(StoreError::Timeout))
    );
    assert_eq!(
        check_staged(false, &Verdict::Stale { have: v020() }, None),
        refused(Bank2Refusal::Stale(v020()))
    );
    for (v, s) in [
        (Verdict::Empty, Bank2Status::Empty),
        (Verdict::Torn, Bank2Status::Torn),
        (Verdict::Foreign, Bank2Status::Foreign),
        (Verdict::Format(2), Bank2Status::Format(2)),
        (Verdict::Corrupt, Bank2Status::Corrupt),
        (Verdict::Unreadable, Bank2Status::Corrupt),
    ] {
        assert_eq!(
            check_staged(false, &v, None),
            refused(Bank2Refusal::Staged(s)),
            "{v:?}"
        );
    }
}

/// The install's outcome and bank 2 as the door then judged it.
#[test]
fn the_install_and_the_door_decide_the_answer() {
    let h = header();
    let open = Bank2Status::Open {
        version: v020(),
        seal: h.seal,
    };
    let fail = |why| Bank2Answer::Failed { why, audio: AUDIO };
    assert_eq!(
        installed(Ok(h), &open, AUDIO),
        Bank2Answer::Open {
            version: v020(),
            audio: AUDIO
        }
    );
    assert_eq!(
        installed(Ok(h), &Bank2Status::Corrupt, AUDIO),
        fail(Bank2Fail::Door)
    );
    let at = FlashAt::Sector(Bank2Sector::S1);
    for (r, why) in [
        (
            InstallFail::Flash(FlashFail::Timeout, at),
            Bank2Fail::Flash(FlashFail::Timeout, at),
        ),
        (InstallFail::Io, Bank2Fail::StagedRead),
        (
            InstallFail::Staged(Verdict::Corrupt),
            Bank2Fail::Staged(Bank2Status::Corrupt),
        ),
    ] {
        // Even if the door somehow judged open, a failed install says so.
        assert_eq!(installed(Err(r), &open, AUDIO), fail(why), "{r:?}");
    }
    // The install's last judge failed, but the door's own judge, a moment
    // later, opened bank 2: it is open, and the answer says so.
    let r = Err(InstallFail::Readback(Verdict::Unreadable));
    assert_eq!(
        installed(r, &open, AUDIO),
        Bank2Answer::Open {
            version: v020(),
            audio: AUDIO
        }
    );
    assert_eq!(
        installed(r, &Bank2Status::Torn, AUDIO),
        fail(Bank2Fail::Readback(Bank2Status::Corrupt))
    );
}

#[test]
fn key_and_turn_queue_then_ok() {
    let mut u = Fake::new();
    assert_eq!(ask(&mut u, "key MENU\n"), "ERR key is not in this build\n");
    assert_eq!(ask(&mut u, "turn 1 3\n"), "ERR turn is not in this build\n");
    u.hands = Some(Hands::new());
    assert_eq!(ask(&mut u, "key MIX+SEQ\n"), "OK\n");
    assert_eq!(ask(&mut u, "turn 6 -64\n"), "OK\n");
    for _ in 2..QUEUE {
        assert_eq!(ask(&mut u, "key b1 500\n"), "OK\n");
    }
    assert_eq!(ask(&mut u, "key b1\n"), "ERR busy\n");
    assert!(
        ask(&mut u, "key FOO\n").starts_with("ERR key takes <name>[+<name>] [hold_ms]: B1-B6 ")
    );
    assert_eq!(
        ask(&mut u, "turn 7 1\n"),
        "ERR turn takes <encoder 1-6> <delta -64..64, not 0>\n"
    );
}

mod hands {
    use chimera_core::console::{Gesture, Hand, Hands, KeyArg, Switch, TurnArg};
    use chimera_hal::{ButtonId, EncoderId, Ms};

    fn key(with: Option<ButtonId>, tap: ButtonId, hold_ms: u16) -> Gesture {
        Gesture::Key(KeyArg {
            with: with.map(Switch::Button),
            tap: Switch::Button(tap),
            hold_ms,
        })
    }

    fn held(h: Hand) -> Vec<ButtonId> {
        chimera_hal::ALL_BUTTONS
            .into_iter()
            .filter(|&b| h.holds(Switch::Button(b)))
            .collect()
    }

    #[test]
    fn a_tap_is_down_60_ms_then_up_60() {
        let mut h = Hands::new();
        h.queue(key(None, ButtonId::Menu, 0)).unwrap();
        assert_eq!(held(h.step(Ms(1000))), [ButtonId::Menu]);
        assert_eq!(held(h.step(Ms(1059))), [ButtonId::Menu]);
        assert_eq!(held(h.step(Ms(1060))), []);
        assert!(!h.idle());
        assert_eq!(h.step(Ms(1120)), Hand::default());
        assert!(h.idle());
    }

    #[test]
    fn a_combo_holds_the_first_around_the_second() {
        let mut h = Hands::new();
        h.queue(key(Some(ButtonId::Mix), ButtonId::Seq, 100))
            .unwrap();
        h.queue(key(None, ButtonId::B2, 0)).unwrap();
        use ButtonId::*;
        assert_eq!(held(h.step(Ms(0))), [Mix]);
        assert_eq!(held(h.step(Ms(60))), [Mix, Seq]);
        assert_eq!(held(h.step(Ms(159))), [Mix, Seq]);
        assert_eq!(held(h.step(Ms(160))), [Mix]);
        assert_eq!(held(h.step(Ms(220))), []);
        // The gap ends at 280: the next gesture starts on that lap.
        assert_eq!(held(h.step(Ms(279))), []);
        assert_eq!(held(h.step(Ms(280))), [B2]);
    }

    #[test]
    fn a_turn_spreads_its_detents_and_catches_up_after_a_stall() {
        let mut h = Hands::new();
        let turn = |delta| {
            Gesture::Turn(TurnArg {
                enc: EncoderId::C,
                delta,
            })
        };
        h.queue(turn(-3)).unwrap();
        h.queue(turn(5)).unwrap();
        let c = |h: Hand| h.detents[EncoderId::C as usize];
        assert_eq!(h.step(Ms(0)).detents, [0, 0, -1, 0, 0, 0]);
        assert_eq!(c(h.step(Ms(10))), 0);
        assert_eq!(c(h.step(Ms(25))), -1);
        assert_eq!(c(h.step(Ms(90))), -1);
        assert_eq!(c(h.step(Ms(200))), 0);
        // 50 + 160: the next turn's first detent.
        assert_eq!(c(h.step(Ms(210))), 1);
        assert_eq!(c(h.step(Ms(400))), 4);
        assert_eq!(c(h.step(Ms(1000))), 0);
        assert!(h.idle());
    }
}

/// The card IRQ's u32 cycles wrap every 8.9 s of its time at 480 MHz:
/// folded every UI pass into 64 bits, a 10-minute gap at 2 % reads 2 %
/// (#596 review), not the wrapped remainder.
#[test]
fn irq_cycles_fold_past_the_wrap() {
    use chimera_core::stream::stats::IrqCycles;
    let (hz, pass_ms, share) = (480_000_000u64, 33u64, 0.02);
    let per_pass = (hz as f64 * share * pass_ms as f64 / 1e3) as u64;
    let passes = 10 * 60 * 1_000 / pass_ms;
    let (mut raw, mut f) = (0u32, IrqCycles::new(0));
    let start = f.fold(raw);
    for _ in 0..passes {
        raw = raw.wrapping_add(per_pass as u32);
        f.fold(raw);
    }
    let own = f.total() - start;
    assert_eq!(own, per_pass * passes);
    assert!(own > u64::from(u32::MAX), "the raw counter wrapped");
    let wall = passes * pass_ms * hz / 1_000;
    assert_eq!(PerMille::of(own, wall).to_string(), "2.0");
}
