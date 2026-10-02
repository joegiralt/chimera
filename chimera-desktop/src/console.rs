//! The console on a local socket: the unit's protocol, one client at a
//! time, polled at the top of the sim's frame. An answer never holds it past its `AnswerClock`.

#[cfg(test)]
use chimera_core::console::SHOT_HEADER;
use chimera_core::console::{
    AnswerClock, Console, Frame, Out, Served, Stalled, Stats, Unit, answer,
};
use chimera_core::ui::UiState;
use chimera_hal::Ms;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

pub const ADDR: &str = "127.0.0.1:7341";

/// Bytes read per frame at most: a flood without a newline can't hold the frame.
const READ_BUDGET: usize = 1024;

/// A connection and its half-read line: a new client starts a new line.
struct Client {
    stream: TcpStream,
    line: Console,
}

pub struct SocketConsole {
    listener: TcpListener,
    client: Option<Client>,
}

/// The console on `addr`, or off (and said so) when the port is taken.
pub fn bind_or_off(addr: &str) -> Option<SocketConsole> {
    SocketConsole::bind(addr)
        .inspect_err(|_| eprintln!("console: {addr} busy, console off"))
        .ok()
}

impl SocketConsole {
    pub fn bind(addr: &str) -> std::io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        Ok(SocketConsole {
            listener,
            client: None,
        })
    }

    #[cfg(test)]
    pub fn local_addr(&self) -> std::net::SocketAddr {
        self.listener.local_addr().unwrap()
    }

    /// The frame's top: take the newest waiting client (it replaces the
    /// old one), read its bytes into the line, answer at most one request.
    pub fn service(&mut self, unit: &mut impl Unit) -> Served {
        while let Ok((stream, _)) = self.listener.accept() {
            if stream.set_nonblocking(true).is_ok() {
                self.client = Some(Client {
                    stream,
                    line: Console::new(),
                });
            }
        }
        let Some(c) = self.client.as_mut() else {
            return Served::Idle;
        };
        let mut byte = [0u8; 1];
        for _ in 0..READ_BUDGET {
            match c.stream.read(&mut byte) {
                Ok(1) => {
                    if let Some(req) = c.line.push(byte[0]) {
                        let out = &mut StreamOut::new(&mut c.stream);
                        if answer(req, unit, out).is_err() {
                            self.client = None;
                        }
                        return Served::Answered;
                    }
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                // EOF or a reset: the client left.
                _ => {
                    self.client = None;
                    break;
                }
            }
        }
        Served::Idle
    }
}

/// The sim as the console sees it: no AUDIO LOAD, no bench.
pub struct DeskUnit<'a> {
    pub ui: &'a UiState,
    pub frame: Frame<'a>,
}

impl Unit for DeskUnit<'_> {
    fn ui(&self) -> &UiState {
        self.ui
    }
    fn stats(&mut self) -> Option<Stats> {
        None
    }
    fn bench(&self) -> Option<&str> {
        None
    }
    fn frame(&self) -> Frame<'_> {
        self.frame
    }
    fn dfu(&mut self) -> Option<()> {
        None
    }
    fn boot(&self) -> Option<chimera_core::boot::BootSeen> {
        None
    }
}

/// A non-blocking stream as `Out`, for one answer: `WouldBlock` retries
/// until its `AnswerClock` expires; any other failure means the client left.
struct StreamOut<'a> {
    s: &'a mut TcpStream,
    epoch: Instant,
    clock: AnswerClock,
}

impl<'a> StreamOut<'a> {
    fn new(s: &'a mut TcpStream) -> Self {
        StreamOut {
            s,
            epoch: Instant::now(),
            clock: AnswerClock::start(Ms(0)),
        }
    }

    fn now(&self) -> Ms {
        Ms(self.epoch.elapsed().as_millis() as u32)
    }
}

impl Out for StreamOut<'_> {
    fn put(&mut self, mut bytes: &[u8]) -> Result<(), Stalled> {
        while !bytes.is_empty() {
            match self.s.write(bytes) {
                Ok(0) => return Err(Stalled),
                Ok(n) => {
                    bytes = &bytes[n..];
                    self.clock.progress(self.now());
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) if e.kind() == ErrorKind::WouldBlock && !self.clock.expired(self.now()) => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(_) => return Err(Stalled),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_core::ui::theme;
    use chimera_core::ui::theme_settings::{Accent, Palette, ThemeSettings};
    use chimera_hal::FB_SIZE;
    use embedded_graphics_core::pixelcolor::raw::{RawData, RawU16};
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::{Duration, Instant};

    fn client(con: &SocketConsole) -> TcpStream {
        let c = TcpStream::connect(con.local_addr()).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        c
    }

    /// A fresh UI, a framebuffer of known pixels that the theme recolours,
    /// and a palette that isn't the identity.
    fn fb_unit() -> (Box<UiState>, Box<[u16; FB_SIZE]>, Palette) {
        let pal = ThemeSettings {
            accent: Accent::Amber,
            ..ThemeSettings::DEFAULT
        }
        .palette();
        assert_ne!(pal, Palette::IDENTITY);
        let mut fb: Box<[u16; FB_SIZE]> = vec![0u16; FB_SIZE].try_into().unwrap();
        for (i, px) in fb.iter_mut().enumerate() {
            *px = (i as u16).wrapping_mul(7919);
        }
        for (i, c) in [theme::ACCENT, theme::ACCENT_SOFT, theme::BG]
            .into_iter()
            .enumerate()
        {
            fb[i * 1000] = RawU16::from(c).into_inner();
        }
        (Box::new(UiState::new()), fb, pal)
    }

    fn serve_until_answered(con: &mut SocketConsole, unit: &mut impl Unit) -> bool {
        for _ in 0..100 {
            if con.service(unit) == Served::Answered {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    fn read_line(c: &mut TcpStream) -> String {
        let mut line = Vec::new();
        let mut b = [0u8; 1];
        while line.last() != Some(&b'\n') {
            c.read_exact(&mut b).unwrap();
            line.push(b[0]);
        }
        String::from_utf8(line).unwrap()
    }

    /// Lines up to and including `OK` or `ERR …`.
    fn read_until_terminal(c: &mut TcpStream) -> String {
        let mut text = String::new();
        loop {
            let line = read_line(c);
            text.push_str(&line);
            if line == "OK\n" || line.starts_with("ERR ") {
                return text;
            }
        }
    }

    fn read_shot(c: &mut TcpStream) -> Vec<u8> {
        assert_eq!(read_line(c), SHOT_HEADER);
        let mut body = vec![0u8; FB_SIZE * 2];
        c.read_exact(&mut body).unwrap();
        assert_eq!(read_line(c), "OK\n");
        body
    }

    fn shot_is(request: &[u8], expect: impl Fn(u16) -> u16) {
        let (ui, fb, pal) = fb_unit();
        let mut con = SocketConsole::bind("127.0.0.1:0").unwrap();
        let mut c = client(&con);
        c.write_all(request).unwrap();
        let unit = &mut DeskUnit {
            ui: &ui,
            frame: Frame {
                fb: &fb,
                palette: pal,
            },
        };
        assert!(serve_until_answered(&mut con, unit));
        let body = read_shot(&mut c);
        for (i, px) in body.chunks(2).enumerate() {
            assert_eq!(
                u16::from_be_bytes([px[0], px[1]]),
                expect(fb[i]),
                "pixel {i}"
            );
        }
    }

    #[test]
    fn status_after_a_key_walk_names_the_place() {
        let mut desk = crate::qa::Desk::launch(crate::store::tests::unique_root());
        desk.tap(minifb::Key::M); // MENU in the sim's key map; opens SETTINGS on release
        let fb: Box<[u16; FB_SIZE]> = vec![0u16; FB_SIZE].try_into().unwrap();
        let mut con = SocketConsole::bind("127.0.0.1:0").unwrap();
        let mut c = client(&con);
        c.write_all(b"status\n").unwrap();
        let unit = &mut DeskUnit {
            ui: &desk.ui,
            frame: Frame {
                fb: &fb,
                palette: Palette::IDENTITY,
            },
        };
        assert!(serve_until_answered(&mut con, unit));
        let text = read_until_terminal(&mut c);
        assert!(text.contains("\nat SETTINGS\n"), "{text}");
        assert!(text.ends_with("OK\n"));
    }

    #[test]
    fn shot_matches_the_framebuffer_through_the_palette() {
        let pal = fb_unit().2;
        shot_is(b"shot\n", |px| pal.map_raw(px));
    }

    #[test]
    fn shot_raw_matches_the_framebuffer() {
        shot_is(b"shot raw\n", |px| px);
    }

    #[test]
    fn stats_and_bench_are_not_in_this_build() {
        let (ui, fb, pal) = fb_unit();
        let mut con = SocketConsole::bind("127.0.0.1:0").unwrap();
        let mut c = client(&con);
        c.write_all(b"stats\nbench\n").unwrap();
        let mut unit = DeskUnit {
            ui: &ui,
            frame: Frame {
                fb: &fb,
                palette: pal,
            },
        };
        for cmd in ["stats", "bench"] {
            assert!(serve_until_answered(&mut con, &mut unit));
            assert_eq!(
                read_until_terminal(&mut c),
                format!("ERR {cmd} is not in this build\n")
            );
        }
    }

    #[test]
    fn dfu_on_the_sim_is_not_in_this_build() {
        let (ui, fb, pal) = fb_unit();
        let mut con = SocketConsole::bind("127.0.0.1:0").unwrap();
        let mut c = client(&con);
        c.write_all(b"dfu\n").unwrap();
        let mut unit = DeskUnit {
            ui: &ui,
            frame: Frame {
                fb: &fb,
                palette: pal,
            },
        };
        assert!(serve_until_answered(&mut con, &mut unit));
        assert_eq!(
            read_until_terminal(&mut c),
            "ERR dfu is not in this build\n"
        );
    }

    #[test]
    fn two_requests_in_one_write_answer_on_two_frames() {
        let (ui, fb, pal) = fb_unit();
        let mut con = SocketConsole::bind("127.0.0.1:0").unwrap();
        let mut c = client(&con);
        c.write_all(b"help\nstatus\n").unwrap();
        let mut unit = DeskUnit {
            ui: &ui,
            frame: Frame {
                fb: &fb,
                palette: pal,
            },
        };
        assert!(serve_until_answered(&mut con, &mut unit));
        let first = read_until_terminal(&mut c);
        assert!(first.starts_with("chimera console 1\n"), "{first}");
        assert_eq!(
            con.service(&mut unit),
            Served::Answered,
            "the second request waited, and the next frame answers it"
        );
        assert!(read_until_terminal(&mut c).starts_with("firmware "));
    }

    #[test]
    fn a_new_client_replaces_the_old() {
        let (ui, fb, pal) = fb_unit();
        let mut con = SocketConsole::bind("127.0.0.1:0").unwrap();
        let _old = client(&con);
        let mut new = client(&con);
        new.write_all(b"help\n").unwrap();
        let unit = &mut DeskUnit {
            ui: &ui,
            frame: Frame {
                fb: &fb,
                palette: pal,
            },
        };
        assert!(serve_until_answered(&mut con, unit));
        assert!(read_until_terminal(&mut new).ends_with("OK\n"));
    }

    #[test]
    fn a_client_that_stops_reading_stalls_a_shot_in_250_ms() {
        // The client never reads. Loopback buffers hold some shots; ask for
        // shots until one answer can't go out, then time that one.
        let (ui, fb, pal) = fb_unit();
        let mut con = SocketConsole::bind("127.0.0.1:0").unwrap();
        let mut c = client(&con);
        let mut unit = DeskUnit {
            ui: &ui,
            frame: Frame {
                fb: &fb,
                palette: pal,
            },
        };
        let mut stalled = None;
        for _ in 0..200 {
            c.write_all(b"shot\n").unwrap();
            let t = Instant::now();
            serve_until_answered(&mut con, &mut unit);
            if t.elapsed() >= Duration::from_millis(250) {
                stalled = Some(t.elapsed());
                break;
            }
        }
        let took = stalled.expect("a reader that never reads stalls a shot");
        assert!(took < Duration::from_millis(1500), "{took:?}");
        c.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        assert!(
            c.read_to_end(&mut Vec::new()).is_ok(),
            "the shell closed the stalled client"
        );
    }

    #[test]
    fn a_taken_port_leaves_the_console_off() {
        let first = SocketConsole::bind("127.0.0.1:0").unwrap();
        assert!(bind_or_off(&first.local_addr().to_string()).is_none());
    }

    #[test]
    fn a_client_that_leaves_is_forgotten() {
        let (ui, fb, pal) = fb_unit();
        let mut con = SocketConsole::bind("127.0.0.1:0").unwrap();
        drop(client(&con));
        let mut unit = DeskUnit {
            ui: &ui,
            frame: Frame {
                fb: &fb,
                palette: pal,
            },
        };
        assert!(!serve_until_answered(&mut con, &mut unit));
        let mut c = client(&con);
        c.write_all(b"help\n").unwrap();
        assert!(serve_until_answered(&mut con, &mut unit));
        assert!(read_until_terminal(&mut c).ends_with("OK\n"));
    }

    #[test]
    fn a_flood_without_a_newline_is_read_a_budget_a_frame() {
        let (ui, fb, pal) = fb_unit();
        let mut con = SocketConsole::bind("127.0.0.1:0").unwrap();
        let mut c = client(&con);
        c.write_all(&[b'a'; 8 * READ_BUDGET]).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        let unit = &mut DeskUnit {
            ui: &ui,
            frame: Frame {
                fb: &fb,
                palette: pal,
            },
        };
        assert_eq!(con.service(unit), Served::Idle);
        let left = con.client.as_ref().unwrap().stream.peek(&mut [0u8; 1]);
        assert_eq!(left.unwrap(), 1, "the rest waits for later frames");
    }

    #[test]
    fn a_new_client_starts_a_fresh_line() {
        let (ui, fb, pal) = fb_unit();
        let mut con = SocketConsole::bind("127.0.0.1:0").unwrap();
        let mut old = client(&con);
        let mut unit = DeskUnit {
            ui: &ui,
            frame: Frame {
                fb: &fb,
                palette: pal,
            },
        };
        old.write_all(b"hel").unwrap();
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(con.service(&mut unit), Served::Idle);
        let mut new = client(&con);
        new.write_all(b"help\n").unwrap();
        assert!(serve_until_answered(&mut con, &mut unit));
        let text = read_until_terminal(&mut new);
        assert!(text.starts_with("chimera console 1\n"), "{text}");
    }
}
