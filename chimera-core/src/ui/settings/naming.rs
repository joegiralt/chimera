//! NAMING (storage spec § Naming and tags): A moves the cursor, B cycles
//! A–Z, C cycles 0–9, space and `-`, D toggles case, E deletes. F's tags
//! come with plan 3. SEQ saves, MENU cancels.

use core::fmt::Write;

use chimera_hal::{Controls, EncoderId};
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::name::{Name, ProjectName, is_name_byte};
use crate::storage::ProjectId;
use crate::ui::draw;
use crate::ui::fmt::FmtBuf;
use crate::ui::hold::{Press, Presses};
use crate::ui::theme;

/// The longest name: `ProjectName` and `SoundName` alike.
pub const NAME_MAX: usize = 16;

const LETTERS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const OTHERS: &[u8] = b"0123456789 -";

/// A name being typed: up to `NAME_MAX` name characters, the cursor on one
/// of them or just past the end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Naming {
    buf: [u8; NAME_MAX],
    len: u8,
    cursor: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamingOut {
    Save(Name<NAME_MAX>),
    Cancel,
    /// SEQ on a name of spaces: refused, NAMING stays.
    Empty,
}

impl Naming {
    /// `start`'s name characters, cut at `NAME_MAX`; the cursor at the end.
    pub fn new(start: &str) -> Self {
        let mut n = Naming {
            buf: [0; NAME_MAX],
            len: 0,
            cursor: 0,
        };
        for &c in start.as_bytes().iter().filter(|&&c| is_name_byte(c)) {
            if n.len as usize == NAME_MAX {
                break;
            }
            n.buf[n.len as usize] = c;
            n.len += 1;
        }
        n.cursor = n.last_cursor();
        n
    }

    pub fn text(&self) -> &str {
        // Name characters are ASCII.
        core::str::from_utf8(&self.buf[..self.len as usize]).unwrap_or("")
    }

    pub fn cursor(&self) -> u8 {
        self.cursor
    }

    /// Just past the end, unless the name is full.
    fn last_cursor(&self) -> u8 {
        self.len.min(NAME_MAX as u8 - 1)
    }

    /// The character under the cursor; `None` past the end.
    fn at(&self) -> Option<u8> {
        self.buf[..self.len as usize]
            .get(self.cursor as usize)
            .copied()
    }

    /// Writes `c` at the cursor, or appends it at the end.
    fn put(&mut self, c: u8) {
        let i = self.cursor as usize;
        self.buf[i] = c;
        if i == self.len as usize {
            self.len += 1;
        }
    }

    fn remove(&mut self, i: usize) {
        let len = self.len as usize;
        if i < len {
            self.buf.copy_within(i + 1..len, i);
            self.len -= 1;
            self.buf[self.len as usize] = 0;
        }
    }

    /// The next of `set` `d` steps from `c`; from outside it, +1 is the first.
    fn cycle(set: &[u8], c: Option<u8>, d: i8) -> u8 {
        let from = c
            .and_then(|c| set.iter().position(|&s| s == c))
            .map_or(if d > 0 { -1 } else { 0 }, |i| i as i32);
        set[(from + d as i32).rem_euclid(set.len() as i32) as usize]
    }

    pub fn input(&mut self, c: &impl Controls, p: &Presses) -> Option<NamingOut> {
        if p.menu == Some(Press::Tap) {
            return Some(NamingOut::Cancel);
        }
        if p.seq == Some(Press::Tap) {
            return Some(match Name::new(self.text().trim_matches(' ')) {
                Ok(n) => NamingOut::Save(n),
                Err(_) => NamingOut::Empty,
            });
        }
        let d = |e| c.encoder_delta(e);
        let a = d(EncoderId::A);
        if a != 0 {
            let to = self.cursor as i32 + a as i32;
            self.cursor = to.clamp(0, self.last_cursor() as i32) as u8;
        }
        let b = d(EncoderId::B);
        if b != 0 {
            let lower = self.at().is_some_and(|c| c.is_ascii_lowercase());
            let up = self.at().map(|c| c.to_ascii_uppercase());
            let l = Self::cycle(LETTERS, up, b);
            self.put(if lower { l.to_ascii_lowercase() } else { l });
        }
        let cc = d(EncoderId::C);
        if cc != 0 {
            let o = Self::cycle(OTHERS, self.at(), cc);
            self.put(o);
        }
        if d(EncoderId::D) != 0
            && let Some(ch) = self.at()
        {
            let i = self.cursor as usize;
            self.buf[i] = if ch.is_ascii_lowercase() {
                ch.to_ascii_uppercase()
            } else {
                ch.to_ascii_lowercase()
            };
        }
        let e = d(EncoderId::E);
        for _ in 0..e.unsigned_abs() {
            if e > 0 {
                self.remove(self.cursor as usize);
            } else if self.cursor > 0 {
                self.cursor -= 1;
                self.remove(self.cursor as usize);
            }
        }
        self.cursor = self.cursor.min(self.last_cursor());
        None
    }
}

const WORDS: [&str; 8] = [
    "DUB", "ACID", "DRIFT", "PULSE", "GLASS", "EMBER", "TIDE", "STATIC",
];
const FALLBACK: ProjectName = match Name::new("DUB-000") {
    Ok(n) => n,
    Err(_) => panic!(),
};

/// `WORDS[id % 8]-NNN`, NNN the id's last three digits (Pre-flight 16).
pub fn proposed_name(id: ProjectId) -> ProjectName {
    let n = id.get();
    let mut b = FmtBuf::new();
    let _ = write!(b, "{}-{:03}", WORDS[n as usize % WORDS.len()], n % 1000);
    Name::new(b.as_str()).unwrap_or(FALLBACK)
}

const LABEL_Y: i32 = 66;
const BOX_X: i32 = 16;
const BOX_STEP: i32 = 13;
const BOX_TOP: i32 = 78;
const BOX_H: i32 = 26;
const CHAR_Y: i32 = 97;
const RULE_Y: i32 = 108;
const CELLS_Y: i32 = 150;

/// The name in sixteen boxes, the cursor's filled, under `title`; then
/// what each encoder does.
pub fn draw_naming<D: DrawTarget<Color = Rgb565>>(d: &mut D, n: &Naming, title: &str) {
    draw::text_tracked(
        d,
        &theme::FONT_LABEL,
        title,
        theme::LIST_TEXT_X,
        LABEL_Y,
        theme::MID,
        theme::LABEL_TRACKING,
    );
    for i in 0..NAME_MAX {
        let x = BOX_X + i as i32 * BOX_STEP;
        let on = i == n.cursor as usize;
        if on {
            draw::fill_rect(d, x - 1, BOX_TOP, BOX_STEP - 1, BOX_H, theme::ACCENT);
        }
        if let Some(&c) = n.buf[..n.len as usize].get(i) {
            let mut s = [0u8; 4];
            let s = (c as char).encode_utf8(&mut s);
            let ink = if on { theme::BG } else { theme::INK };
            draw::text_center(d, &theme::FONT_VALUE, s, x + 5, CHAR_Y, ink, 0);
        }
        let rule = if on { theme::ACCENT } else { theme::FAINT };
        draw::fill_rect(d, x, RULE_Y, BOX_STEP - 3, 2, rule);
    }
    let mut pos = FmtBuf::new();
    let _ = write!(pos, "{} / {}", n.cursor + 1, NAME_MAX);
    let case = match n.at() {
        Some(c) if c.is_ascii_lowercase() => "abc",
        _ => "ABC",
    };
    // What B and C turn: the character under the cursor, if it is theirs.
    let mut ch = [0u8; 4];
    let letter = match n.at() {
        Some(c) if c.is_ascii_alphabetic() => &*(c as char).encode_utf8(&mut ch),
        _ => "·",
    };
    let mut dg = [0u8; 4];
    let digit = match n.at() {
        Some(b' ') => "SPACE",
        Some(c) if OTHERS.contains(&c) => &*(c as char).encode_utf8(&mut dg),
        _ => "·",
    };
    let cells: [(&str, &str, bool); 6] = [
        ("CURSOR", pos.as_str(), true),
        ("LETTER", letter, true),
        ("DIGIT", digit, true),
        ("CASE", case, true),
        ("DELETE", "< >", true),
        ("TAGS", "LATER", false),
    ];
    for (i, (label, value, live)) in cells.into_iter().enumerate() {
        let x = theme::MARGIN_X + (i as i32 % 3) * theme::CELL_COL_W;
        let y = CELLS_Y + (i as i32 / 3) * theme::CELL_ROW_H;
        let (lc, vc) = if live {
            (theme::MID, theme::INK)
        } else {
            (theme::BAR_REST, theme::BAR_REST)
        };
        draw::text_tracked(
            d,
            &theme::FONT_LABEL,
            label,
            x,
            y,
            lc,
            theme::LABEL_TRACKING,
        );
        draw::text_tracked(
            d,
            &theme::FONT_VALUE,
            value,
            x,
            y + theme::CELL_VALUE_DY,
            vc,
            0,
        );
    }
}
