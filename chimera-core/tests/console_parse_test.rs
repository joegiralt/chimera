//! The console's command table and line parser (spec § Protocol).

use chimera_core::console::*;

fn feed(c: &mut Console, s: &[u8]) -> Vec<Result<Request, Refusal>> {
    s.iter().filter_map(|&b| c.push(b)).collect()
}

fn mixed_case(s: &str) -> String {
    s.chars()
        .enumerate()
        .map(|(i, c)| {
            if i % 2 == 1 {
                c.to_ascii_uppercase()
            } else {
                c
            }
        })
        .collect()
}

#[test]
fn every_name_parses_in_any_case() {
    for c in Command::ALL {
        for name in [
            c.name().to_string(),
            c.name().to_uppercase(),
            mixed_case(c.name()),
        ] {
            let got = feed(&mut Console::new(), format!("{name}\n").as_bytes());
            assert_eq!(got.len(), 1, "{name}");
            // `ls` and `put` need their arguments: refused, but by name.
            let named = match got[0] {
                Ok(r) => r.command(),
                Err(Refusal::Arguments(c)) => c,
                Err(e) => panic!("{name}: {e:?}"),
            };
            assert_eq!(named, c, "{name}");
        }
    }
}

#[test]
fn shot_takes_raw_or_nothing() {
    let one = |s: &str| feed(&mut Console::new(), s.as_bytes()).remove(0);
    assert_eq!(one("shot\n"), Ok(Request::Shot(Colours::Theme)));
    assert_eq!(one("SHOT RaW\n"), Ok(Request::Shot(Colours::Raw)));
    assert_eq!(one("shot  raw  \n"), Ok(Request::Shot(Colours::Raw)));
    assert_eq!(one("shot x\n"), Err(Refusal::Arguments(Command::Shot)));
    assert_eq!(
        one("shot raw raw\n"),
        Err(Refusal::Arguments(Command::Shot))
    );
    assert_eq!(Command::Shot.usage(), "raw or nothing");
}

#[test]
fn commands_without_arguments_refuse_one() {
    for c in [
        Command::Help,
        Command::Status,
        Command::Stats,
        Command::Bench,
        Command::Dfu,
    ] {
        let got = feed(
            &mut Console::new(),
            format!("{} now\n", c.name()).as_bytes(),
        );
        assert_eq!(got, vec![Err(Refusal::Arguments(c))]);
        assert_eq!(c.usage(), "no arguments");
    }
}

#[test]
fn lf_cr_and_crlf_each_end_one_request() {
    for s in ["help\n", "help\r", "help\r\n"] {
        assert_eq!(
            feed(&mut Console::new(), s.as_bytes()),
            vec![Ok(Request::Help(NoArg))],
            "{s:?}"
        );
    }
    assert_eq!(feed(&mut Console::new(), b"help\r\nstatus\r\n").len(), 2);
}

#[test]
fn empty_and_blank_lines_give_nothing() {
    assert!(feed(&mut Console::new(), b"\n\r\n   \r\n\n").is_empty());
}

#[test]
fn only_a_space_separates_words() {
    let got = feed(&mut Console::new(), b"\t\n");
    assert_eq!(got, vec![Err(Refusal::Unknown(Word::new(b"\t")))]); // shown as "?"
}

#[test]
fn edge_spaces_are_trimmed_and_repeated_spaces_split() {
    assert_eq!(
        feed(&mut Console::new(), b"   stats   \n"),
        vec![Ok(Request::Stats(NoArg))]
    );
    assert_eq!(
        feed(&mut Console::new(), b"shot     raw\n"),
        vec![Ok(Request::Shot(Colours::Raw))]
    );
}

#[test]
fn sixty_four_bytes_parse_sixty_five_refuse_then_recover() {
    let line = |n: usize| format!("{}help\n", " ".repeat(n - 4));
    assert_eq!(
        feed(&mut Console::new(), line(64).as_bytes()),
        vec![Ok(Request::Help(NoArg))]
    );
    let mut c = Console::new();
    assert_eq!(
        feed(&mut c, line(65).as_bytes()),
        vec![Err(Refusal::TooLong)]
    );
    assert_eq!(feed(&mut c, b"status\n"), vec![Ok(Request::Status(NoArg))]);
    let mut c = Console::new();
    let long = format!("{}\n", "x".repeat(500));
    assert_eq!(
        feed(&mut c, long.as_bytes()),
        vec![Err(Refusal::TooLong)],
        "one refusal per line"
    );
}

#[test]
fn unknown_word_is_cut_to_sixteen_bytes() {
    let got = feed(&mut Console::new(), b"abcdefghijklmnopqrstuvwxyz\n");
    assert_eq!(
        got,
        vec![Err(Refusal::Unknown(Word::new(b"abcdefghijklmnop")))]
    );
    let Err(Refusal::Unknown(w)) = got[0] else {
        unreachable!()
    };
    assert_eq!(w.as_str(), "abcdefghijklmnop");
    assert_eq!(w.to_string(), "abcdefghijklmnop");
}

#[test]
fn unknown_word_answers_in_printable_ascii() {
    let got = feed(&mut Console::new(), b"\x1b[A\x00zz\n");
    let Err(Refusal::Unknown(w)) = got[0] else {
        panic!("{got:?}")
    };
    assert_eq!(w.as_str(), "?[A?zz");
    assert!(w.as_str().bytes().all(|b| (0x20..=0x7e).contains(&b)));
    assert_eq!(Word::new(&[0xff; 40]).as_str(), "????????????????");
}

#[test]
fn a_line_split_anywhere_parses_once() {
    let whole = b"  SHOT raw \r\n";
    for cut in 0..=whole.len() {
        let mut c = Console::new();
        let mut got = feed(&mut c, &whole[..cut]);
        got.extend(feed(&mut c, &whole[cut..]));
        assert_eq!(got, vec![Ok(Request::Shot(Colours::Raw))], "cut at {cut}");
    }
}

#[test]
fn the_table_is_the_one_list() {
    let names: Vec<_> = Command::ALL.iter().map(|c| c.name()).collect();
    let mut want = vec![
        "help", "status", "stats", "bench", "card", "shot", "dfu", "sweep", "flash", "bank2",
        "key", "turn",
    ];
    if cfg!(feature = "card-put") {
        want.extend(["ls", "put", "rm", "get"]);
    }
    assert_eq!(names, want);
    for c in Command::ALL {
        assert!(!c.about().is_empty() && c.about().len() <= 60, "{c:?}");
        assert!(c.name().len() <= 7, "help's column is 8 wide");
    }
}

#[test]
fn word_length_boundary() {
    let exact = [b'a'; WORD_MAX];
    assert_eq!(Word::new(&exact).as_str().len(), WORD_MAX);
    let over = [b'a'; WORD_MAX + 1];
    assert_eq!(Word::new(&over).as_str().len(), WORD_MAX);
    assert_eq!(Word::new(&over), Word::new(&exact));
}

#[test]
fn every_byte_value_is_safe() {
    let printable = |s: &str| s.bytes().all(|b| (0x20..=0x7e).contains(&b));
    for b in 0..=255u8 {
        assert!(printable(Word::new(&[b]).as_str()), "{b}");
        for line in [
            vec![b, b'\n'],
            vec![b'x', b' ', b, b'\n'],
            vec![b'h', b' ', b, b'\r'],
        ] {
            let mut c = Console::new();
            for got in feed(&mut c, &line) {
                if let Err(Refusal::Unknown(w)) = got {
                    assert!(printable(w.as_str()), "{b}");
                }
            }
        }
    }
}

#[cfg(feature = "card-put")]
#[test]
fn put_takes_a_project_path_a_size_and_a_crc() {
    use chimera_core::storage::ProjectId;
    let one = |s: &str| feed(&mut Console::new(), s.as_bytes()).remove(0);
    let want = Ok(Request::Put(PutArg {
        to: PutTo::Project(ProjectId::new(14).unwrap()),
        size: 131_072,
        crc: 0xDEAD_BEEF,
        resume: false,
    }));
    assert_eq!(
        one("put /CHIMERA/PROJECTS/P0000014.A 131072 deadbeef\n"),
        want
    );
    assert_eq!(
        one("PUT /chimera/projects/p0000014.b 131072 DEADBEEF\n"),
        want
    );
    for bad in [
        "put /CHIMERA/PROJECTS/AVRIL14.A 1 0",
        "put /CHIMERA/PROJECTS/P0000000.A 1 0",
        "put /CHIMERA/PROJECTS/P00000014.A 1 0",
        "put /CHIMERA/PROJECTS/P0000014 1 0",
        "put /CHIMERA/PROJECTS/P0000014.C 1 0",
        "put /CHIMERA/PROJECTS/../SYSTEM.A 1 0",
        "put /CHIMERA/PROJECTS//P0000014.A 1 0",
        "put /CHIMERA//PROJECTS/P0000014.A 1 0",
        "put /CHIMERA/PROJECTS/ 1 0",
        "put /CHIMERA 1 0",
        "put /CHIMERA/SYSTEM.A 1 0",
        "put /CHIMERA/PROJECTS/P0000014.A 4294967296 0",
        "put /CHIMERA/PROJECTS/P0000014.A 1 -1",
        "put /CHIMERA/PROJECTS/P0000014.A 1 0x10",
        "put /CHIMERA/PROJECTS/P0000014.A 1 123456789",
        "put /CHIMERA/PROJECTS/P0000014.A 1",
        "put /CHIMERA/PROJECTS/P0000014.A 1 0 more",
    ] {
        assert_eq!(
            one(&format!("{bad}\n")),
            Err(Refusal::Arguments(Command::Put)),
            "{bad}"
        );
    }
    // The longest request fits the line.
    let longest = "put /CHIMERA/PROJECTS/P0000001.A 1048576 ffffffff resume";
    assert!(longest.len() <= MAX_LINE);
}

/// A source by its stem, any case; `resume` parses, to be refused.
#[cfg(feature = "card-put")]
#[test]
fn put_takes_a_tapes_path() {
    let one = |s: &str| feed(&mut Console::new(), s.as_bytes()).remove(0);
    let want = |resume| {
        Ok(Request::Put(PutArg {
            to: PutTo::Source(chimera_core::storage::Stem::new(0x0BAD_F00D)),
            size: 600_000,
            crc: 0x1234_5678,
            resume,
        }))
    };
    assert_eq!(
        one("put /CHIMERA/TAPES/0BADF00D.SRC 600000 12345678\n"),
        want(false)
    );
    assert_eq!(
        one("put /chimera/tapes/0badf00d.src 600000 12345678\n"),
        want(false)
    );
    assert_eq!(
        one("put /CHIMERA/TAPES/0BADF00D.SRC 600000 12345678 RESUME\n"),
        want(true)
    );
    for bad in [
        "put /CHIMERA/TAPES/BADF00D.SRC 1 0",
        "put /CHIMERA/TAPES/0BADF00DD.SRC 1 0",
        "put /CHIMERA/TAPES/0BADF00G.SRC 1 0",
        "put /CHIMERA/TAPES/0BADF00D.SR 1 0",
        "put /CHIMERA/TAPES/0BADF00D 1 0",
        "put /CHIMERA/TAPES//0BADF00D.SRC 1 0",
        "put /CHIMERA/TAPES/../0BADF00D.SRC 1 0",
        "put /CHIMERA/PROJECTS/0BADF00D.SRC 1 0",
        "put /CHIMERA/TAPES/0BADF00D.SRC 1 0 again",
    ] {
        assert_eq!(
            one(&format!("{bad}\n")),
            Err(Refusal::Arguments(Command::Put)),
            "{bad}"
        );
    }
}

/// The staged bank-2 file, any case, at most bank 2's 512 KiB; no other
/// name or extension (never a `.BIN` the bootloader would take).
#[cfg(feature = "card-put")]
#[test]
fn put_takes_the_staged_bank2_file() {
    let one = |s: &str| feed(&mut Console::new(), s.as_bytes()).remove(0);
    let want = |size| {
        Ok(Request::Put(PutArg {
            to: PutTo::Bank2,
            size,
            crc: 0x1A2B_3C4D,
            resume: false,
        }))
    };
    assert_eq!(
        one("put /CHIMERA/SYSTEM/BANK2.BK2 4608 1a2b3c4d\n"),
        want(4608)
    );
    assert_eq!(
        one("put /chimera/system/bank2.bk2 4608 1A2B3C4D\n"),
        want(4608)
    );
    assert_eq!(
        one("put /CHIMERA/SYSTEM/BANK2.BK2 524288 1a2b3c4d\n"),
        want(524_288)
    );
    for bad in [
        "put /CHIMERA/SYSTEM/BANK2.BIN 4608 1a2b3c4d",
        "put /CHIMERA/BANK2.BK2 4608 1a2b3c4d",
        "put /BANK2.BK2 4608 1a2b3c4d",
        "put /CHIMERA/SYSTEM/BANK2.BK2 524289 1a2b3c4d",
        "put /CHIMERA/SYSTEM/BANK3.BK2 4608 1a2b3c4d",
        "put /CHIMERA/SYSTEM//BANK2.BK2 4608 1a2b3c4d",
        "put /CHIMERA/SYSTEM/BANK2.BK2.BIN 4608 1a2b3c4d",
        "put /CHIMERA/SYSTEM/BANK2.BK2 4608 1a2b3c4d --over",
    ] {
        assert_eq!(
            one(&format!("{bad}\n")),
            Err(Refusal::Arguments(Command::Put)),
            "{bad}"
        );
    }
}

#[test]
fn bank2_takes_install_or_nothing() {
    let one = |s: &str| feed(&mut Console::new(), s.as_bytes()).remove(0);
    assert_eq!(one("bank2\n"), Ok(Request::Bank2(Bank2Arg::Status)));
    assert_eq!(
        one("BANK2 Install\n"),
        Ok(Request::Bank2(Bank2Arg::Install))
    );
    for bad in ["bank2 install now", "bank2 status", "bank2 erase now"] {
        assert_eq!(
            one(&format!("{bad}\n")),
            Err(Refusal::Arguments(Command::Bank2)),
            "{bad}"
        );
    }
    #[cfg(not(feature = "bank2-gate"))]
    {
        assert_eq!(
            one("bank2 erase\n"),
            Err(Refusal::Arguments(Command::Bank2))
        );
        assert_eq!(Command::Bank2.usage(), "install or nothing");
    }
    #[cfg(feature = "bank2-gate")]
    {
        assert_eq!(one("Bank2 ERASE\n"), Ok(Request::Bank2(Bank2Arg::Erase)));
        assert_eq!(Command::Bank2.usage(), "install, erase or nothing");
    }
}

#[cfg(feature = "card-put")]
#[test]
fn ls_takes_tapes_or_nothing() {
    let one = |s: &str| feed(&mut Console::new(), s.as_bytes()).remove(0);
    assert_eq!(one("ls\n"), Ok(Request::Ls(LsArg::Projects)));
    assert_eq!(one("ls TAPES\n"), Ok(Request::Ls(LsArg::Tapes)));
    assert_eq!(one("ls /CHIMERA\n"), Err(Refusal::Arguments(Command::Ls)));
    assert_eq!(one("ls tapes x\n"), Err(Refusal::Arguments(Command::Ls)));
}

#[cfg(feature = "card-put")]
fn parse_line(l: &[u8]) -> Result<Request, Refusal> {
    feed(&mut Console::new(), &[l, b"\n"].concat())
        .pop()
        .unwrap()
}

/// SOUNDS paths, any case; nothing outside `/CHIMERA/SOUNDS/` parses.
#[cfg(feature = "card-put")]
#[test]
fn sound_paths_parse_and_others_refuse() {
    use chimera_core::storage::LibId;
    let id = |n| LibId::new(n).unwrap();
    assert_eq!(
        parse_line(b"put /chimera/sounds/s0000012.a 120 deadbeef"),
        Ok(Request::Put(PutArg {
            to: PutTo::Sound {
                id: id(12),
                over: false
            },
            size: 120,
            crc: 0xDEAD_BEEF,
            resume: false,
        }))
    );
    assert!(matches!(
        parse_line(b"put /CHIMERA/SOUNDS/S0000012.B 120 deadbeef --over"),
        Ok(Request::Put(PutArg {
            to: PutTo::Sound { over: true, .. },
            ..
        }))
    ));
    assert!(matches!(
        parse_line(b"put /CHIMERA/SOUNDS/S9000001.A 120 deadbeef"),
        Ok(Request::Put(PutArg {
            to: PutTo::Sound { .. },
            ..
        }))
    ));
    assert_eq!(
        parse_line(b"rm /chimera/sounds/s0000001"),
        Ok(Request::Rm(SoundArg(id(1))))
    );
    assert_eq!(
        parse_line(b"GET /CHIMERA/SOUNDS/S9000001"),
        Ok(Request::Get(SoundArg(id(9_000_001))))
    );
    for bad in [
        &b"rm /CHIMERA/SOUNDS/../PROJECTS/P0000001"[..],
        b"rm //CHIMERA/SOUNDS/S0000001",
        b"rm /CHIMERA//SOUNDS/S0000001",
        b"rm /CHIMERA/PROJECTS/P0000001",
        b"rm SOUNDS/S0000001",
        b"rm /CHIMERA/SOUNDS/S000001",
        b"rm /CHIMERA/SOUNDS/S0000000",
        b"rm /CHIMERA/SOUNDS/S9000000",
        b"rm /CHIMERA/SOUNDS/S0000001 x",
        b"rm",
        b"get /CHIMERA/SOUNDS/S0000001.A",
        b"put /CHIMERA/SOUNDS/S0000001 1 0",
        b"put /CHIMERA/SOUNDS/S0000001.C 1 0",
        b"put /CHIMERA/SOUNDS/../S0000001.A 1 0",
        b"put /CHIMERA/SOUNDS/S0000001.A 1 0 --over --over",
        b"put /CHIMERA/PROJECTS/P0000001.A 1 0 --over",
        b"put /CHIMERA/SOUNDS/S0000001.A 1 0 --OVERS",
    ] {
        assert!(
            matches!(parse_line(bad), Err(Refusal::Arguments(_))),
            "{}",
            String::from_utf8_lossy(bad)
        );
    }
    assert_eq!(parse_line(b"ls sounds"), Ok(Request::Ls(LsArg::Sounds)));
    assert_eq!(parse_line(b"LS SOUNDS"), Ok(Request::Ls(LsArg::Sounds)));
    let longest = "put /CHIMERA/SOUNDS/S0000001.A 1048576 ffffffff --over";
    assert!(longest.len() <= MAX_LINE);
}

#[cfg(not(feature = "card-put"))]
#[test]
fn without_card_put_ls_and_put_are_unknown() {
    let got = feed(&mut Console::new(), b"put x 1 0\nls\n");
    assert!(matches!(got[0], Err(Refusal::Unknown(_))));
    assert!(matches!(got[1], Err(Refusal::Unknown(_))));
}

fn one(s: &str) -> Result<Request, Refusal> {
    feed(&mut Console::new(), s.as_bytes()).remove(0)
}

fn key(with: Option<Switch>, tap: Switch, hold_ms: u16) -> Result<Request, Refusal> {
    Ok(Request::Key(KeyArg { with, tap, hold_ms }))
}

#[test]
fn key_takes_a_name_a_combo_and_a_hold() {
    use chimera_hal::ButtonId::*;
    let b = Switch::Button;
    assert_eq!(one("key MENU\n"), key(None, b(Menu), 0));
    assert_eq!(one("key menu 0\n"), key(None, b(Menu), 0));
    assert_eq!(one("KEY b6 250\n"), key(None, b(B6), 250));
    assert_eq!(one("key MIX+SEQ\n"), key(Some(b(Mix)), b(Seq), 0));
    assert_eq!(one("key mix+b3 800\n"), key(Some(b(Mix)), b(B3), 800));
    assert_eq!(one("key minus\n"), key(None, b(Minus), 0));
    assert_eq!(one("key PLUS 10000\n"), key(None, b(Plus), 10_000));
    assert_eq!(one("key EDIT\n"), key(None, b(Edit), 0));
    #[cfg(feature = "push-encoders")]
    assert_eq!(
        one("key MIX+push4\n"),
        key(Some(b(Mix)), Switch::Push(chimera_hal::EncoderId::D), 0)
    );
    for bad in [
        "key\n",
        "key FOO\n",
        "key B0\n",
        "key B7\n",
        "key PUSH7\n",
        "key MIX+\n",
        "key +SEQ\n",
        "key MIX+MIX\n",
        "key MIX+SEQ+B1\n",
        "key MENU -1\n",
        "key MENU 10001\n",
        "key MENU 1x\n",
        "key MENU 5 5\n",
    ] {
        assert_eq!(one(bad), Err(Refusal::Arguments(Command::Key)), "{bad}");
    }
}

#[test]
fn turn_takes_an_encoder_and_a_delta() {
    use chimera_hal::EncoderId::*;
    let t = |enc, delta| Ok(Request::Turn(TurnArg { enc, delta }));
    assert_eq!(one("turn 1 1\n"), t(A, 1));
    assert_eq!(one("TURN 6 -64\n"), t(F, -64));
    assert_eq!(one("turn 3 +64\n"), t(C, 64));
    for bad in [
        "turn\n",
        "turn 1\n",
        "turn 0 1\n",
        "turn 7 1\n",
        "turn 12 1\n",
        "turn A 1\n",
        "turn 1 0\n",
        "turn 1 65\n",
        "turn 1 -65\n",
        "turn 1 200\n",
        "turn 1 x\n",
        "turn 1 2 3\n",
    ] {
        assert_eq!(one(bad), Err(Refusal::Arguments(Command::Turn)), "{bad}");
    }
}

#[test]
fn card_dma_takes_a_bench_and_its_stream_count() {
    use chimera_hal::units::StreamCount;
    let one = |s: &str| feed(&mut Console::new(), format!("{s}\n").as_bytes()).remove(0);
    let n = |k| StreamCount::new(k).unwrap();
    for (line, verb) in [
        ("card dma chunk", DmaVerb::Chunk),
        ("card dma burst", DmaVerb::Burst),
        ("card dma sustain 8", DmaVerb::Sustain(n(8))),
        ("card dma stall 4", DmaVerb::Stall(n(4))),
        ("card dma ui 8", DmaVerb::Ui(n(8))),
        ("card dma back", DmaVerb::Back),
        ("card dma listen 8", DmaVerb::Listen(n(8))),
        ("card dma wake 2", DmaVerb::Wake(n(2))),
        ("card dma inject abort", DmaVerb::Inject(Inject::Abort)),
        ("card dma inject te", DmaVerb::Inject(Inject::Te)),
        ("card dma inject modf", DmaVerb::Inject(Inject::Modf)),
        ("card dma report", DmaVerb::Report),
        ("card dma stop", DmaVerb::Stop),
        ("CARD DMA Sustain 1", DmaVerb::Sustain(n(1))),
    ] {
        assert_eq!(one(line), Ok(Request::Card(CardArg::Dma(verb))), "{line}");
    }
    for bad in [
        "card dma",
        "card dma sustain 0",
        "card dma sustain 9",
        "card dma sustain 9x",
        "card dma sustain",
        "card dma inject",
        "card dma inject x",
        "card dma burst 2",
        "card dma report now",
        "card dma wake 2 2",
    ] {
        assert_eq!(one(bad), Err(Refusal::Arguments(Command::Card)), "{bad}");
    }
    assert!(Command::Card.usage().contains("dma"));
}
