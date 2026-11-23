// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! `key` and `turn`: the console's hands. A request queues a gesture;
//! `Hands::step`, once a UI loop lap, says which switches are down and
//! which detents arrived, as the HC165 scan would have read them.

use chimera_hal::{ButtonId, ENCODER_BURST_GAP_MS, EncoderId, Ms, NUM_ENCODERS, PART_BUTTONS};

use super::{Arg, Words};

/// A tap's press, and the least any hold lasts: past the 6 ms debounce.
pub const TAP_MS: u16 = 60;
/// The longest hold `key` takes.
pub const HOLD_MAX_MS: u16 = 10_000;
/// A combo's first switch goes down this long before the second, and up
/// this long after it.
pub const LEAD_MS: u32 = 60;
/// All up this long after a press, so the next one is a new edge.
pub const GAP_MS: u32 = 60;
/// A detent this often: a brisk hand, so acceleration builds.
pub const DETENT_MS: u32 = 25;
/// After a turn, past the burst gap: the next turn starts a new burst.
pub const TURN_GAP_MS: u32 = ENCODER_BURST_GAP_MS + 10;
pub const TURN_MAX: i8 = 64;
/// Gestures waiting, the running one aside.
pub const QUEUE: usize = 8;

/// A panel switch: a button, or an encoder's push.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Switch {
    Button(ButtonId),
    #[cfg(feature = "push-encoders")]
    Push(EncoderId),
}

impl Switch {
    /// Its bit in `Hand::down`: the buttons', then the pushes', as the
    /// scan's latches are laid out.
    pub const fn bit(self) -> u32 {
        match self {
            Switch::Button(b) => 1 << b as u32,
            #[cfg(feature = "push-encoders")]
            Switch::Push(e) => 1 << (chimera_hal::NUM_BUTTONS as u32 + e as u32),
        }
    }

    fn named(w: &[u8]) -> Option<Switch> {
        const NAV: [(&str, ButtonId); 6] = [
            ("MENU", ButtonId::Menu),
            ("MINUS", ButtonId::Minus),
            ("PLUS", ButtonId::Plus),
            ("MIX", ButtonId::Mix),
            ("EDIT", ButtonId::Edit),
            ("SEQ", ButtonId::Seq),
        ];
        if let Some((_, b)) = NAV
            .iter()
            .find(|(n, _)| w.eq_ignore_ascii_case(n.as_bytes()))
        {
            return Some(Switch::Button(*b));
        }
        if let [b'b' | b'B', n] = w {
            return PART_BUTTONS.get(digit(*n)?).map(|&b| Switch::Button(b));
        }
        #[cfg(feature = "push-encoders")]
        if let [p, u, s, h, n] = w
            && [*p, *u, *s, *h].eq_ignore_ascii_case(b"PUSH")
        {
            return encoder(digit(*n)?).map(Switch::Push);
        }
        None
    }
}

/// `1`..`6` as 0..5.
fn digit(b: u8) -> Option<usize> {
    (b'1'..=b'6').contains(&b).then(|| usize::from(b - b'1'))
}

fn encoder(i: usize) -> Option<EncoderId> {
    chimera_hal::ALL_ENCODERS.get(i).copied()
}

fn number<T: core::str::FromStr>(w: &[u8]) -> Option<T> {
    core::str::from_utf8(w).ok()?.parse().ok()
}

/// `key <name>[+<name>] [hold_ms]`: `with` held while `tap` is pressed for
/// `hold_ms`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyArg {
    pub with: Option<Switch>,
    pub tap: Switch,
    pub hold_ms: u16,
}

impl Arg for KeyArg {
    const USAGE: &'static str = if cfg!(feature = "push-encoders") {
        "<name>[+<name>] [hold_ms]: B1-B6 MENU MINUS PLUS MIX EDIT SEQ PUSH1-6"
    } else {
        "<name>[+<name>] [hold_ms]: B1-B6 MENU MINUS PLUS MIX EDIT SEQ"
    };
    fn parse(mut words: Words<'_>) -> Option<Self> {
        let names = words.next()?;
        let hold_ms = match words.next() {
            None => 0,
            Some(w) => number::<u16>(w).filter(|&h| h <= HOLD_MAX_MS)?,
        };
        if words.next().is_some() {
            return None;
        }
        let (with, tap) = match names.iter().position(|&b| b == b'+') {
            None => (None, Switch::named(names)?),
            Some(at) => {
                let (a, b) = (
                    Switch::named(&names[..at])?,
                    Switch::named(&names[at + 1..])?,
                );
                (a != b).then_some((Some(a), b))?
            }
        };
        Some(KeyArg { with, tap, hold_ms })
    }
}

/// `turn <1-6> <delta>`: `delta` detents, never 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TurnArg {
    pub enc: EncoderId,
    pub delta: i8,
}

impl Arg for TurnArg {
    const USAGE: &'static str = "<encoder 1-6> <delta -64..64, not 0>";
    fn parse(mut words: Words<'_>) -> Option<Self> {
        let enc = match words.next()? {
            [n] => encoder(digit(*n)?)?,
            _ => return None,
        };
        let delta = number::<i8>(words.next()?)
            .filter(|d| *d != 0 && (-TURN_MAX..=TURN_MAX).contains(d))?;
        words.next().is_none().then_some(TurnArg { enc, delta })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    Key(KeyArg),
    Turn(TurnArg),
}

/// One lap's hand: the switches it holds down, and the detents since the
/// last lap.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hand {
    pub down: u32,
    pub detents: [i8; NUM_ENCODERS],
}

impl Hand {
    pub const fn holds(&self, s: Switch) -> bool {
        self.down & s.bit() != 0
    }
}

#[derive(Clone, Copy, Debug)]
struct Running {
    gesture: Gesture,
    since: Ms,
    sent: u8,
}

/// The queue is full: nothing queued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Busy;

/// The gestures, queued and running.
#[derive(Clone, Copy, Debug)]
pub struct Hands {
    queue: [Option<Gesture>; QUEUE],
    head: u8,
    len: u8,
    running: Option<Running>,
}

impl Default for Hands {
    fn default() -> Self {
        Self::new()
    }
}

impl Hands {
    pub const fn new() -> Self {
        Hands {
            queue: [None; QUEUE],
            head: 0,
            len: 0,
            running: None,
        }
    }

    pub fn queue(&mut self, g: Gesture) -> Result<(), Busy> {
        if usize::from(self.len) >= QUEUE {
            return Err(Busy);
        }
        let at = (usize::from(self.head) + usize::from(self.len)) % QUEUE;
        if let Some(slot) = self.queue.get_mut(at) {
            *slot = Some(g);
            self.len += 1;
        }
        Ok(())
    }

    /// Nothing running or queued.
    pub fn idle(&self) -> bool {
        self.running.is_none() && self.len == 0
    }

    fn pop(&mut self) -> Option<Gesture> {
        if self.len == 0 {
            return None;
        }
        let g = self.queue.get_mut(usize::from(self.head))?.take();
        self.head = ((usize::from(self.head) + 1) % QUEUE) as u8;
        self.len -= 1;
        g
    }

    /// The hand at `now`. A gesture starts at the first lap that sees it,
    /// and the next one at the lap its own last gap ends.
    pub fn step(&mut self, now: Ms) -> Hand {
        loop {
            let r = match &mut self.running {
                Some(r) => r,
                None => match self.pop() {
                    Some(gesture) => self.running.insert(Running {
                        gesture,
                        since: now,
                        sent: 0,
                    }),
                    None => return Hand::default(),
                },
            };
            if let Some(hand) = at(r, now.since(r.since)) {
                return hand;
            }
            self.running = None;
        }
    }
}

/// The running gesture `t` ms in, or `None` once it and its gap are over.
fn at(r: &mut Running, t: u32) -> Option<Hand> {
    let mut hand = Hand::default();
    match r.gesture {
        Gesture::Key(k) => {
            let hold = u32::from(k.hold_ms.max(TAP_MS));
            let lead = if k.with.is_some() { LEAD_MS } else { 0 };
            let end = lead + hold + lead;
            if t >= end + GAP_MS {
                return None;
            }
            if let Some(w) = k.with
                && t < end
            {
                hand.down |= w.bit();
            }
            if (lead..lead + hold).contains(&t) {
                hand.down |= k.tap.bit();
            }
        }
        Gesture::Turn(tn) => {
            let n = tn.delta.unsigned_abs();
            if r.sent >= n && t >= u32::from(n - 1) * DETENT_MS + TURN_GAP_MS {
                return None;
            }
            let due = (t / DETENT_MS + 1).min(u32::from(n)) as u8;
            let new = due.saturating_sub(r.sent);
            r.sent = r.sent.max(due);
            if let Some(d) = hand.detents.get_mut(tn.enc as usize) {
                *d = new as i8 * tn.delta.signum();
            }
        }
    }
    Some(hand)
}
