//! Bank 2 as the boot judged it: `status`'s line and ABOUT's label (ADR 0137).

mod screen;

use chimera_core::bank2::{Bank2Status, Version};
use chimera_core::ui::UiState;
use chimera_core::ui::about_page::viz_lines;
use chimera_core::ui::perf::PerfStats;
use chimera_image::{Header, Verdict};
use screen::*;

fn line(s: &Bank2Status) -> String {
    let mut w = String::new();
    s.line(&mut w).unwrap();
    w
}

const SEAL: [u8; 8] = [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef];

fn open() -> Bank2Status {
    Bank2Status::Open {
        version: Version::new("0.2.0 02e440e2"),
        seal: SEAL,
    }
}

#[test]
fn bank2_status_lines() {
    use Bank2Status::*;
    let cases = [
        (Unsealed, "bank2 unsealed"),
        (Empty, "bank2 empty"),
        (Torn, "bank2 torn"),
        (Foreign, "bank2 foreign"),
        (Format(7), "bank2 format 7"),
        (Corrupt, "bank2 corrupt"),
        (
            Stale {
                have: Version::new("0.1.9 aaaaaaaa"),
            },
            "bank2 stale have 0.1.9 aaaaaaaa",
        ),
        (open(), "bank2 open 0.2.0 02e440e2 seal=0123456789abcdef"),
    ];
    for (s, want) in cases {
        assert_eq!(line(&s), want);
    }
}

#[test]
fn only_open_is_ok() {
    use Bank2Status::*;
    assert_eq!(open().label(), "OK");
    for s in [Unsealed, Empty, Torn, Foreign, Format(2), Corrupt] {
        assert_eq!(s.label(), "NEEDS UPDATE");
    }
    let have = Version::new("0.1.9 aaaaaaaa");
    assert_eq!(Stale { have }.label(), "NEEDS UPDATE");
}

#[test]
fn from_verdict_keeps_what_the_judge_saw() {
    let have = Version::new("0.1.9 aaaaaaaa");
    let h = Header {
        body_len: 32,
        body_crc: 0,
        door_off: 256,
        seal: SEAL,
        version: Version::new("0.2.0 02e440e2"),
    };
    let cases = [
        (Verdict::Empty, Bank2Status::Empty),
        (Verdict::Torn, Bank2Status::Torn),
        (Verdict::Foreign, Bank2Status::Foreign),
        (Verdict::Format(3), Bank2Status::Format(3)),
        (Verdict::Corrupt, Bank2Status::Corrupt),
        // A read the flash refused is no image to trust.
        (Verdict::Unreadable, Bank2Status::Corrupt),
        (Verdict::Stale { have }, Bank2Status::Stale { have }),
        (Verdict::Good(h), open()),
    ];
    for (v, want) in cases {
        assert_eq!(Bank2Status::from_verdict(&v), want);
    }
}

#[test]
fn about_shows_bank2() {
    let label = |s: Option<Bank2Status>| viz_lines(s.as_ref())[2].as_str().to_string();
    assert_eq!(label(Some(Bank2Status::Empty)), "NEEDS UPDATE");
    assert_eq!(label(Some(Bank2Status::Unsealed)), "NEEDS UPDATE");
    assert_eq!(label(Some(open())), "OK");
    // No bank 2 on this target (the desktop): no line.
    assert_eq!(label(None), "");
}

#[test]
fn about_draws_the_label() {
    let shot = |s: Option<Bank2Status>| {
        let mut ui = UiState::new();
        if let Some(s) = s {
            ui.set_bank2(s);
        }
        to_leaf(&mut ui, &["SYSTEM", "ABOUT"]);
        settle(&mut ui);
        let mut fb = Fb::new();
        ui.render_with_audio(&mut fb, &PerfStats::zero(), None, &scope_fixture());
        fb.hash()
    };
    let (none, empty, ok) = (
        shot(None),
        shot(Some(Bank2Status::Empty)),
        shot(Some(open())),
    );
    assert_ne!(none, empty);
    assert_ne!(empty, ok);
}

#[test]
fn status_has_the_bank2_line() {
    let mut ui = UiState::new();
    let mut w = String::new();
    chimera_core::console::write_status(&ui, &mut w).unwrap();
    assert!(!w.contains("bank2"), "no bank 2 on this target, no line");
    ui.set_bank2(Bank2Status::Empty);
    let mut w = String::new();
    chimera_core::console::write_status(&ui, &mut w).unwrap();
    assert!(w.ends_with("\nbank2 empty\n"), "{w}");
}
