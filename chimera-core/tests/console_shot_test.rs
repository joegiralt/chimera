use chimera_core::console::{Colours, Frame, Out, SHOT_HEADER, Stalled, write_shot};
use chimera_core::ui::theme;
use chimera_core::ui::theme_settings::{Accent, Palette, ThemeSettings};
use chimera_hal::FB_SIZE;
use embedded_graphics::pixelcolor::raw::{RawData, RawU16};

struct Sink(Vec<u8>);
impl Out for Sink {
    fn put(&mut self, b: &[u8]) -> Result<(), Stalled> {
        self.0.extend_from_slice(b);
        Ok(())
    }
}
/// Takes `left` bytes, then stalls; counts puts made after the stall.
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
        let n = b.len().min(self.left);
        self.got.extend_from_slice(&b[..n]);
        self.left -= n;
        if n < b.len() {
            self.stalled = true;
            Err(Stalled)
        } else {
            Ok(())
        }
    }
}
fn raw(c: embedded_graphics::pixelcolor::Rgb565) -> u16 {
    RawU16::from(c).into_inner()
}
fn fb() -> Box<[u16; FB_SIZE]> {
    let mut fb = Box::new([0u16; FB_SIZE]);
    for (i, p) in fb.iter_mut().enumerate() {
        *p = (i as u16).wrapping_mul(2654) ^ 0x5a5a;
    }
    fb[0] = raw(theme::ACCENT);
    fb[1] = raw(theme::BG);
    fb[FB_SIZE - 1] = 0x1234;
    fb
}
fn amber() -> Palette {
    ThemeSettings {
        accent: Accent::Amber,
        ..ThemeSettings::DEFAULT
    }
    .palette()
}

#[test]
fn shot_is_header_body_ok() {
    let fb = fb();
    let mut s = Sink(Vec::new());
    write_shot(
        Frame {
            fb: &fb,
            palette: Palette::IDENTITY,
        },
        Colours::Theme,
        &mut s,
    )
    .unwrap();
    assert_eq!(
        SHOT_HEADER,
        format!(
            "SHOT {} {} rgb565be {}\n",
            chimera_hal::SCREEN_WIDTH,
            chimera_hal::SCREEN_HEIGHT,
            FB_SIZE * 2
        )
    );
    assert!(s.0.starts_with(SHOT_HEADER.as_bytes()));
    assert_eq!(s.0.len(), SHOT_HEADER.len() + 153_600 + 3);
    assert!(s.0.ends_with(b"OK\n"));
}

#[test]
fn pixels_go_big_endian_through_the_palette() {
    let fb = fb();
    let pal = amber();
    assert_ne!(pal, Palette::IDENTITY);
    let mut s = Sink(Vec::new());
    write_shot(
        Frame {
            fb: &fb,
            palette: pal,
        },
        Colours::Theme,
        &mut s,
    )
    .unwrap();
    let body = &s.0[SHOT_HEADER.len()..SHOT_HEADER.len() + 153_600];
    for (i, px) in body.chunks(2).enumerate() {
        assert_eq!(
            u16::from_be_bytes([px[0], px[1]]),
            pal.map_raw(fb[i]),
            "pixel {i}"
        );
    }
    assert_eq!(u16::from_be_bytes([body[0], body[1]]), raw(pal.accent));
}

#[test]
fn shot_raw_ignores_the_palette() {
    let fb = fb();
    let mut s = Sink(Vec::new());
    write_shot(
        Frame {
            fb: &fb,
            palette: amber(),
        },
        Colours::Raw,
        &mut s,
    )
    .unwrap();
    let body = &s.0[SHOT_HEADER.len()..SHOT_HEADER.len() + 153_600];
    for (i, px) in body.chunks(2).enumerate() {
        assert_eq!(u16::from_be_bytes([px[0], px[1]]), fb[i], "pixel {i}");
    }
}

#[test]
fn a_stall_mid_shot_stops_without_a_terminal_line() {
    let fb = fb();
    for left in [
        0,
        10,
        SHOT_HEADER.len(),
        1000,
        153_600,
        SHOT_HEADER.len() + 153_600 + 1,
    ] {
        let mut s = Stalls {
            got: Vec::new(),
            left,
            stalled: false,
            after: 0,
        };
        let r = write_shot(
            Frame {
                fb: &fb,
                palette: Palette::IDENTITY,
            },
            Colours::Theme,
            &mut s,
        );
        assert_eq!(r, Err(Stalled), "left {left}");
        assert!(!s.got.ends_with(b"OK\n"), "left {left}");
        assert_eq!(s.after, 0, "nothing is put after a stall (left {left})");
    }
}
