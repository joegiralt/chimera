#![cfg(feature = "bank2-spike")]
use chimera_core::console::{
    Audio, B2Answer, Bank2Sector0Addr, CCR_CLEAR, CODE, CODE_AT, Console, ERASE_TIMEOUT_MS,
    FLASH_WORD, Fail, IMAGE, IMAGE_WORDS, PATTERN_WORDS, Readback, Request, Run, SECTOR_LEN,
    SECTOR0, SECTOR0_END, code_ok, compare, erase_cr, ms_cycles, pack, program_cr, settle, word_at,
    write_b2,
};

fn parse(line: &str) -> Request {
    let mut c = Console::new();
    line.bytes()
        .chain(*b"\n")
        .filter_map(|b| c.push(b))
        .last()
        .unwrap()
        .unwrap()
}

#[test]
fn the_commands_parse() {
    assert!(matches!(parse("b2erase"), Request::B2erase(_)));
    assert!(matches!(parse("B2PROG"), Request::B2prog(_)));
    assert!(matches!(parse("b2call"), Request::B2call(_)));
    assert!(matches!(parse("b2stat"), Request::B2stat(_)));
}

#[test]
fn only_sector_0_words_are_addresses() {
    assert_eq!(
        Bank2Sector0Addr::new(SECTOR0).map(|a| a.get()),
        Some(SECTOR0)
    );
    let last = SECTOR0_END - FLASH_WORD;
    assert_eq!(Bank2Sector0Addr::new(last).map(|a| a.get()), Some(last));
    for no in [
        0x0800_0000,          // the bootloader
        0x0802_0000,          // the firmware
        SECTOR0 - FLASH_WORD, // bank 1's last word
        SECTOR0_END,          // sector 1
        0x0818_0000,          // the bootloader's slot, sector 4
        0x081c_0000,          // sector 6
        SECTOR0 + 4,          // inside a word
        u32::MAX,
        0,
    ] {
        assert_eq!(Bank2Sector0Addr::new(no), None, "{no:08x}");
    }
    assert!(std::panic::catch_unwind(|| Bank2Sector0Addr::at(SECTOR_LEN)).is_err());
    assert!(std::panic::catch_unwind(|| Bank2Sector0Addr::at(16)).is_err());
    assert!(std::panic::catch_unwind(|| Bank2Sector0Addr::at(u32::MAX)).is_err());
}

#[test]
fn the_image_sits_at_the_sector_start() {
    assert_eq!(word_at(0).get(), 0x0810_0000);
    assert_eq!(CODE_AT.get(), 0x0810_0080);
    assert_eq!(word_at(IMAGE_WORDS - 1), CODE_AT);
}

#[test]
fn the_code_is_adds_r0_42_then_bx_lr() {
    let adds = u16::from_le_bytes([CODE[0], CODE[1]]);
    assert_eq!(adds >> 11, 0b00110, "T1 ADDS Rdn, #imm8");
    assert_eq!((adds >> 8) & 7, 0, "Rdn = r0");
    assert_eq!(adds & 0xff, 42);
    assert_eq!(u16::from_le_bytes([CODE[2], CODE[3]]), 0x4770, "BX LR");
    assert_eq!(IMAGE[PATTERN_WORDS], {
        let mut w = [u32::MAX; 8];
        w[0] = 0x4770_302a;
        w
    });
}

#[test]
fn pack_is_little_endian_and_pads_erased() {
    assert_eq!(pack(&[]), [u32::MAX; 8]);
    assert_eq!(pack(&[1, 2, 3])[0], 0xff03_0201);
    let full: Vec<u8> = (0..40).collect();
    assert_eq!(pack(&full)[7], 0x1f1e_1d1c, "past 32 bytes is dropped");
}

#[test]
fn the_pattern_programs_every_bit_and_no_word_repeats() {
    let pat = &IMAGE[..PATTERN_WORDS];
    assert_eq!(pat.iter().flatten().fold(u32::MAX, |a, &x| a & x), 0);
    let mut all: Vec<u32> = pat.iter().flatten().copied().collect();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), PATTERN_WORDS * 8);
    assert!(!all.contains(&u32::MAX));
}

#[test]
fn readback_judges_erased_matching_or_the_first_difference() {
    assert_eq!(compare(&[[u32::MAX; 8]; IMAGE_WORDS]), Readback::Erased);
    assert_eq!(compare(&IMAGE), Readback::Matches);
    let mut torn = IMAGE;
    torn[2][5] ^= 1;
    torn[3][0] ^= 1;
    assert_eq!(
        compare(&torn),
        Readback::Differs {
            word: 2,
            index: 5,
            got: IMAGE[2][5] ^ 1,
            want: IMAGE[2][5]
        }
    );
    let mut half = [[u32::MAX; 8]; IMAGE_WORDS];
    half[0] = IMAGE[0];
    assert!(matches!(compare(&half), Readback::Differs { word: 1, .. }));
}

#[test]
fn code_ok_wants_the_whole_word() {
    assert!(code_ok(&IMAGE[PATTERN_WORDS]));
    assert!(!code_ok(&[u32::MAX; 8]));
    let mut pad = IMAGE[PATTERN_WORDS];
    pad[7] = 0;
    assert!(!code_ok(&pad));
}

#[test]
fn erase_cr_sets_ser_snb_0_and_x32_only() {
    // The unit's CR2 after the 2026-10-07 ROM DFU, unlocked: PSIZE 3.
    assert_eq!(erase_cr(0x0000_0030), 0x0000_0024);
    // PG, BER, FW, START and SNB 7 cleared; interrupt enables kept.
    assert_eq!(erase_cr(0x0001_07ca), 0x0001_0024);
}

#[test]
fn program_cr_sets_pg_and_x32_only() {
    assert_eq!(program_cr(0x0000_0030), 0x0000_0022);
    assert_eq!(program_cr(0x0001_07ec), 0x0001_0022);
}

#[test]
fn ccr_clears_every_error_and_eop() {
    assert_eq!(CCR_CLEAR, 0x07ef_0000);
}

#[test]
fn settle_puts_errors_before_the_deadline() {
    assert_eq!(settle(0, false), None);
    assert_eq!(settle(0, true), None, "done just as the deadline passed");
    assert_eq!(settle(1 << 2, false), None, "QW: still going");
    assert_eq!(settle(1 << 0, true), Some(Fail::Timeout));
    assert_eq!(settle(1 << 1, true), Some(Fail::Timeout), "WBNE");
    assert_eq!(settle(1 << 17, false), Some(Fail::Errors), "WRPERR");
    assert_eq!(settle((1 << 0) | (1 << 22), true), Some(Fail::Errors));
    assert_eq!(settle(1 << 16, false), None, "EOP is no error");
}

#[test]
fn the_erase_deadline_fits_one_counter_wrap() {
    assert_eq!(ms_cycles(ERASE_TIMEOUT_MS, 480_000_000), 2_880_000_000);
    const {
        assert!(
            ERASE_TIMEOUT_MS >= 4_000 + 1_000,
            "over Table 51's ×8 4 s max"
        )
    };
}

fn lines(a: &B2Answer) -> (String, Option<&'static str>) {
    let mut s = String::new();
    let why = write_b2(a, &mut s).unwrap();
    (s, why)
}

const AUDIO: Audio = Audio {
    overruns: [3, 3],
    blocks: [1000, 1750],
};

#[test]
fn an_erase_prints_its_time_registers_and_audio() {
    let run = Run {
        cr: 0x0000_0025,
        sr: 1 << 16,
        cycles: 528_000_000,
        fail: None,
    };
    let (s, why) = lines(&B2Answer::Erase {
        run,
        audio: AUDIO,
        cpu_hz: 480_000_000,
    });
    assert_eq!(why, None);
    assert_eq!(
        s,
        "b2erase sector 0 08100000..0811ffff psize 2 timeout_ms 6000\n\
         elapsed_ms 1100 cycles 528000000\n\
         cr2 00000025 lock 1 pg 0 ser 1 ber 0 psize 2 fw 0 start 0 snb 0\n\
         sr2 00010000 bsy 0 wbne 0 qw 0 crc_busy 0 eop 1 crcend 0 errors -\n\
         audio overruns 3 -> 3 (+0) blocks +750\n"
    );
}

#[test]
fn an_erase_timeout_ends_err_with_sr2_decoded() {
    let run = Run {
        cr: 0x0000_00a5,
        sr: 0x0000_0005,
        cycles: 2_880_000_100,
        fail: Some(Fail::Timeout),
    };
    let (s, why) = lines(&B2Answer::Erase {
        run,
        audio: AUDIO,
        cpu_hz: 480_000_000,
    });
    assert_eq!(why, Some("timeout"));
    assert!(s.contains("sr2 00000005 bsy 1 wbne 0 qw 1"), "{s}");
    assert!(s.contains("elapsed_ms 6000 "), "{s}");
}

#[test]
fn a_flash_error_names_its_flags() {
    let run = Run {
        cr: 0x0000_0025,
        sr: (1 << 17) | (1 << 18),
        cycles: 10,
        fail: Some(Fail::Errors),
    };
    let (s, why) = lines(&B2Answer::Erase {
        run,
        audio: AUDIO,
        cpu_hz: 480_000_000,
    });
    assert_eq!(why, Some("flash error"));
    assert!(s.contains("errors wrperr pgserr\n"), "{s}");
}

#[test]
fn a_program_passes_only_on_a_matching_readback() {
    let run = Run {
        cr: 0x0000_0023,
        sr: 0,
        cycles: 48_000,
        fail: None,
    };
    let prog = |readback| B2Answer::Prog {
        run,
        audio: AUDIO,
        cpu_hz: 480_000_000,
        readback,
    };
    let (s, why) = lines(&prog(Readback::Matches));
    assert_eq!(why, None);
    assert!(
        s.starts_with(
            "b2prog words 5 08100000..0810009f code at 08100080 psize 2 word_timeout_ms 5\n\
             elapsed_ms 0 cycles 48000\n"
        ),
        "{s}"
    );
    assert!(s.contains("readback matches\n"));
    let (s, why) = lines(&prog(Readback::Differs {
        word: 4,
        index: 1,
        got: 0,
        want: u32::MAX,
    }));
    assert_eq!(why, Some("readback differs from the image"));
    assert!(s.contains("readback differs word 4 index 1 at 08100084 got 00000000 want ffffffff\n"));
    assert_eq!(
        lines(&prog(Readback::Erased)).1,
        Some("readback differs from the image")
    );
}

#[test]
fn a_refused_program_says_why() {
    let run = Run {
        cr: 0x0000_0031,
        sr: 0,
        cycles: 0,
        fail: Some(Fail::NotErased),
    };
    let (_, why) = lines(&B2Answer::Prog {
        run,
        audio: AUDIO,
        cpu_hz: 480_000_000,
        readback: Readback::Matches,
    });
    assert_eq!(why, Some("sector 0 not erased, b2erase first"));
}

#[test]
fn a_call_reports_its_answer_or_why_it_did_not_call() {
    let (s, why) = lines(&B2Answer::Call(Ok(43)));
    assert_eq!(
        (s.as_str(), why),
        ("b2call 08100081 f(1) = 43 want 43\n", None)
    );
    assert_eq!(lines(&B2Answer::Call(Ok(44))).1, Some("wrong return value"));
    let (s, why) = lines(&B2Answer::Call(Err(Readback::Erased)));
    assert_eq!(why, Some("the code's word differs, not called"));
    assert_eq!(s, "b2call code at 08100080: not called\nreadback erased\n");
}

#[test]
fn stat_peeks_every_image_word() {
    let (s, why) = lines(&B2Answer::Stat {
        cr: 0x0000_0031,
        sr: 0,
        words: IMAGE,
    });
    assert_eq!(why, None);
    assert!(s.starts_with("cr2 00000031 lock 1 "));
    assert_eq!(
        s.lines().filter(|l| l.starts_with("peek ")).count(),
        IMAGE_WORDS
    );
    assert!(s.contains("peek 08100080 4770302a ffffffff"));
    assert!(s.ends_with("readback matches\n"));
}

/// The shell names only bank 2's own registers: never the option bytes,
/// bank 1's key or control register, bank erase or mass erase.
#[test]
fn the_shell_never_names_bank_1_or_the_option_bytes() {
    let src = include_str!("../../chimera-board/src/bank2.rs").to_ascii_lowercase();
    for no in [
        "optkeyr",
        "optcr",
        "optsr",
        "_prg",
        "bank1",
        ".ber(",
        ".mer(",
        "keyr1",
        "cr1",
        "0x5200_2004",
        "0x5200_200c",
        "0x52002004",
        "0x5200200c",
    ] {
        assert!(!src.contains(no), "chimera-board/src/bank2.rs names {no}");
    }
    assert!(src.contains("bank2()"));
}
