use chimera_fat::h7_spi::*;

fn bl(n: usize) -> BurstLen {
    BurstLen::new(n).unwrap()
}

fn off_after_suspend(seq: &[Op]) {
    for (i, op) in seq.iter().enumerate() {
        if *op == Op::Cr1(Cr1::OFF) {
            assert!(seq[..i].contains(&Op::Suspend), "{seq:?}");
        }
    }
}

#[test]
fn spe_clears_only_after_a_suspend() {
    off_after_suspend(&TAKE);
    off_after_suspend(&burst(bl(512)));
    off_after_suspend(&GIVE);
    off_after_suspend(&ABORT);
}

#[test]
fn burst_follows_rm0433() {
    let n = bl(512);
    assert_eq!(
        burst(n),
        [
            Op::Suspend,
            Op::ClearFlags,
            Op::Cr1(Cr1::OFF),
            Op::AfcntrOn,
            Op::Tsize(Some(n)),
            Op::RxDma(true),
            Op::ClearStreams,
            Op::StreamsOn(n),
            Op::TxDma(true),
            Op::Cr1(Cr1::ON),
            Op::Start,
        ]
    );
}

#[test]
fn burst_end_checks_errors_and_clears_eot() {
    assert_eq!(
        BURST_END,
        [
            Op::WaitEot,
            Op::CheckErrors,
            Op::ClearFlags,
            Op::Cr1(Cr1::OFF),
            Op::RxDma(false),
            Op::TxDma(false)
        ]
    );
}

#[test]
fn abort_stops_dma_before_spi() {
    assert_eq!(ABORT[0], Op::StreamsOff);
    assert_eq!(
        ABORT[ABORT.len() - 2..],
        [Op::RxDma(false), Op::TxDma(false)]
    );
}

/// A MODF clears CFG2.MASTER with SPE: every enable that can follow one
/// (the take's polled bytes, the abort's CMD12, the HAL's after a give)
/// comes after `MasterOn`, which comes with SPE off and MODF cleared.
#[test]
fn master_mode_comes_back_before_every_enable() {
    let with_polled = |seq: &[Op]| [seq, &POLLED[..]].concat();
    for seq in [with_polled(&TAKE), with_polled(&ABORT), GIVE.to_vec()] {
        let master = seq.iter().position(|&o| o == Op::MasterOn);
        let master = master.unwrap_or_else(|| panic!("no MasterOn: {seq:?}"));
        let first_on = seq.iter().position(|&o| o == Op::Cr1(Cr1::ON)).unwrap();
        assert!(master < first_on, "{seq:?}");
        let before = &seq[..master];
        let off = before.iter().rposition(|&o| o == Op::Cr1(Cr1::OFF));
        let cleared = before.iter().rposition(|&o| o == Op::ClearFlags);
        assert!(off.is_some() && cleared.is_some(), "{seq:?}");
    }
    assert_eq!(&ABORT[4..6], &[Op::ClearFlags, Op::MasterOn]);
    assert_eq!(TAKE[TAKE.len() - 1], Op::MasterOn);
}

#[test]
fn give_leaves_the_hal_state() {
    assert_eq!(
        GIVE[GIVE.len() - 3..],
        [Op::RestoreCfg1, Op::Tsize(None), Op::Cr1(Cr1::ON)]
    );
}

#[test]
fn mbr_picks_fastest_at_or_below() {
    let k = 100_000_000;
    assert_eq!(Mbr::fastest(k, 25_000_000).unwrap().sck_hz(k), 25_000_000);
    let k = 120_000_000;
    assert_eq!(Mbr::fastest(k, 25_000_000).unwrap().sck_hz(k), 15_000_000);
    assert_eq!(Mbr::fastest(256 * 10_000_000 + 256, 10_000_000), None);
}

#[test]
fn gate_ignores_spurious_entries() {
    let c = DmaEvent::of(true, false);
    assert_eq!(DmaEvent::of(true, true), DmaEvent::Error);
    assert_eq!(DmaEvent::of(false, false), DmaEvent::Spurious);
    assert_eq!(gate(true, c), Gate::Advance);
    assert_eq!(gate(true, DmaEvent::Spurious), Gate::Ignore);
    assert_eq!(gate(true, DmaEvent::Error), Gate::Abort);
    assert_eq!(gate(false, DmaEvent::Error), Gate::Abort);
    assert_eq!(gate(false, c), Gate::Idle);
    assert_eq!(gate(false, DmaEvent::Spurious), Gate::Idle);
}

/// SPI2's CFG1 as the ops touch it; `saved` starts at 0, as a `Held` does.
struct Cfg1Model {
    cfg1: u32,
    saved: u32,
    fail: Op,
    ran: Vec<Op>,
}

const HAL_CFG1: u32 = 0x7007_0007; // MBR /256, FTHLV, DSIZE 8 bits

impl Cfg1Model {
    fn new(fail: Op) -> Self {
        Cfg1Model { cfg1: HAL_CFG1, saved: 0, fail, ran: Vec::new() }
    }

    fn op(&mut self, o: Op) -> Result<(), RegFault> {
        self.ran.push(o);
        if o == self.fail {
            return Err(RegFault::Timeout(o));
        }
        match o {
            Op::SaveCfg1 => self.saved = self.cfg1,
            Op::SetMbr => self.cfg1 &= !(7 << 28),
            Op::RestoreCfg1 => self.cfg1 = self.saved,
            Op::RxDma(on) => self.cfg1 = self.cfg1 & !(1 << 14) | u32::from(on) << 14,
            Op::TxDma(on) => self.cfg1 = self.cfg1 & !(1 << 15) | u32::from(on) << 15,
            _ => {}
        }
        Ok(())
    }
}

/// A take whose suspend times out is given straight back: the give puts
/// back the HAL's CFG1, never a CFG1 the take never read.
#[test]
fn a_failed_take_gives_back_the_hals_cfg1() {
    // The waits and checks: the shell's only fallible ops.
    let can_fail = |o: &Op| {
        matches!(o, Op::Suspend | Op::WaitEot | Op::CheckErrors | Op::StreamsOn(_) | Op::StreamsOff)
    };
    for fail in TAKE.into_iter().filter(can_fail) {
        let mut m = Cfg1Model::new(fail);
        if run(&TAKE, |o| m.op(o)).is_ok() {
            continue;
        }
        m.fail = Op::WaitEot;
        assert_eq!(run_all(&GIVE, |o| m.op(o)), Ok(()));
        assert_eq!(m.cfg1, HAL_CFG1, "TAKE failed at {fail:?}");
    }
}

#[test]
fn run_stops_at_the_first_fault() {
    let mut m = Cfg1Model::new(Op::Suspend);
    assert_eq!(run(&burst(bl(512)), |o| m.op(o)), Err(RegFault::Timeout(Op::Suspend)));
    assert_eq!(m.ran, [Op::Suspend]);
}

/// An abort whose streams-off or suspend times out still clears every
/// enable: a polled stop after it never meets RXDMAEN.
#[test]
fn abort_runs_every_op_past_a_fault() {
    for fail in [Op::StreamsOff, Op::Suspend] {
        let mut m = Cfg1Model::new(fail);
        m.cfg1 |= 3 << 14;
        assert_eq!(run_all(&ABORT, |o| m.op(o)), Err(RegFault::Timeout(fail)));
        assert_eq!(m.ran, ABORT);
        assert_eq!(m.cfg1 & (3 << 14), 0);
    }
}
