//! The console tests' fake unit, its sink and `ask`.
#![allow(dead_code)]

use chimera_core::boot::{BootSeen, UsbRegs};
use chimera_core::console::{
    Bank2Answer, CardAnswer, Console, FlashPeek, FlashRegs, Frame, Hands, Out, Stalled, Stats,
    SweepAsk, Unit, answer,
};
use chimera_core::ui::UiState;
use chimera_core::ui::theme_settings::{ThemeSettings, palette};
use chimera_hal::FB_SIZE;
use chimera_hal::store::SweepRun;

pub struct Sink(pub Vec<u8>);
impl Out for Sink {
    fn now(&self) -> chimera_hal::Ms {
        chimera_hal::Ms(0)
    }
    fn put(&mut self, b: &[u8]) -> Result<(), Stalled> {
        self.0.extend_from_slice(b);
        Ok(())
    }
}

pub struct Fake {
    pub ui: Box<UiState>,
    pub fb: Box<[u16; FB_SIZE]>,
    pub stats: Option<Stats>,
    pub bench: Option<String>,
    pub card: Option<CardAnswer>,
    pub stats_reads: usize,
    pub dfu: Option<()>,
    pub dfu_calls: usize,
    pub boot: Option<BootSeen>,
    pub sweep: Option<SweepRun>,
    pub board: Option<chimera_core::board::BoardId>,
    pub sweep_ask: Option<SweepAsk>,
    pub sweeps: usize,
    pub usb_regs: Option<UsbRegs>,
    pub flash: Option<(FlashRegs, FlashPeek)>,
    pub forgotten: usize,
    pub bank2: Option<Bank2Answer>,
    pub installs: usize,
    pub hands: Option<Hands>,
    pub ed: Option<chimera_core::console::EdQueue>,
    pub midi: Option<chimera_core::console::MidiStats>,
}

impl Fake {
    pub fn new() -> Fake {
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
            ed: Some(chimera_core::console::EdQueue::new()),
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
    fn ed(&mut self) -> (&UiState, Option<&mut chimera_core::console::EdQueue>) {
        (&self.ui, self.ed.as_mut())
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

pub fn ask(u: &mut Fake, line: &str) -> String {
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
