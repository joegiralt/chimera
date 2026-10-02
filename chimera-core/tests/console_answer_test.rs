use chimera_core::clock_plan::SiliconRev;
use chimera_core::console::{
    Colours, Command, Console, Frame, NoArg, Out, Request, SHOT_HEADER, Stalled, Stats, Unit,
    answer,
};
use chimera_core::perf::load::AudioStats;
use chimera_core::reset::ResetCause;
use chimera_core::ui::UiState;
use chimera_core::ui::theme_settings::ThemeSettings;
use chimera_hal::FB_SIZE;

struct Sink(Vec<u8>);
impl Out for Sink {
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
    stats_reads: usize,
    dfu: Option<()>,
    dfu_calls: usize,
}

impl Fake {
    fn new() -> Fake {
        Fake {
            ui: Box::new(UiState::new()),
            fb: Box::new([0xBEEF; FB_SIZE]),
            stats: None,
            bench: None,
            stats_reads: 0,
            dfu: None,
            dfu_calls: 0,
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
    fn frame(&self) -> Frame<'_> {
        Frame {
            fb: &self.fb,
            palette: ThemeSettings::DEFAULT.palette(),
        }
    }
    fn dfu(&mut self) -> Option<()> {
        self.dfu_calls += 1;
        self.dfu
    }
}

fn audio() -> AudioStats {
    AudioStats::new(SiliconRev::V, 480_000_000, ResetCause::Watchdog)
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
         shot    the screen in THEME's colours; shot raw: canonical\n\
         dfu     restart into the ROM loader for just flash\n\
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
    a.drops = [5, 7];
    u.stats = Some(Stats {
        audio: a,
        loop_avg_us: 812,
        loop_peak_us: 4210,
    });
    assert_eq!(
        ask(&mut u, "stats\n"),
        "load_pct 23\npeak_pct 41\noverruns 0\ndrops 5 7\ndesyncs 2\nstack_bytes 12288\nloop_avg_us 812\nloop_peak_us 4210\nOK\n"
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
        });
        assert!(ask(&mut u, "stats\n").contains(want), "{sources} sources");
    }
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
    });
    let mut c = Console::new();
    for &b in b"help\nstatus\nstats\nbench\nshot\nfrob\n" {
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
        const PIECES: [&[u8]; 11] = [
            b"help", b"status", b"stats", b"bench", b"shot", b"dfu", b"raw", b" ", b"\r", b"\n",
            b"\n",
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
