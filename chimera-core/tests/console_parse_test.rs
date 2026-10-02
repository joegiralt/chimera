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
            assert_eq!(got[0].map(|r| r.command()), Ok(c), "{name}");
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
    assert_eq!(names, ["help", "status", "stats", "bench", "shot", "dfu"]);
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
