use chimera_core::part::DacPair;
use stm32h7xx_hal::pac;

// CR1 bit 27, rev B and later only (absent from the rev-Y-based PAC).
const MCKEN: u32 = 1 << 27;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Master,
    InternalSlave,
    ExternalSlave,
}

pub fn init(new_sai: bool, mckdiv: u8) {
    // SAFETY: single-threaded init before the audio interrupt is unmasked;
    // RCC's APB2ENR, SAI1 and SAI2 are not used elsewhere yet.
    let (rcc, sai1, sai2) = unsafe { (&*pac::RCC::ptr(), &*pac::SAI1::ptr(), &*pac::SAI2::ptr()) };
    rcc.apb2enr
        .modify(|_, w| w.sai1en().enabled().sai2en().enabled());
    let _ = rcc.apb2enr.read();
    configure(sai1.cha(), Role::Master, mckdiv, new_sai);
    configure(sai1.chb(), Role::InternalSlave, mckdiv, new_sai);
    configure(sai2.cha(), Role::ExternalSlave, mckdiv, new_sai);
    // SAFETY: SYNCOUT = 01 exports block A's FS and SCK as SAI1's sync
    // output; SYNCIN = 00 makes SAI1 SAI2's sync source. Written while every
    // block is disabled, as RM0433 requires.
    unsafe {
        sai1.gcr.write(|w| w.syncout().bits(0b01));
        sai2.gcr.write(|w| w.syncin().bits(0b00));
    }
}

pub fn start() {
    // SAFETY: called once from `main` after the three DMA streams run and the
    // rings are pre-filled; only SAIEN is set. Slaves first, master last, so
    // all three start on the master's first frame.
    let (sai1, sai2) = unsafe { (&*pac::SAI1::ptr(), &*pac::SAI2::ptr()) };
    sai2.cha().cr1.modify(|_, w| w.saien().set_bit());
    sai1.chb().cr1.modify(|_, w| w.saien().set_bit());
    sai1.cha().cr1.modify(|_, w| w.saien().set_bit());
}

pub fn data_register(pair: DacPair) -> u32 {
    // SAFETY: only register addresses are taken; nothing is read or written.
    let (sai1, sai2) = unsafe { (&*pac::SAI1::ptr(), &*pac::SAI2::ptr()) };
    match pair {
        DacPair::P1 => sai1.cha().dr.as_ptr() as u32,
        DacPair::P2 => sai1.chb().dr.as_ptr() as u32,
        DacPair::P3 => sai2.cha().dr.as_ptr() as u32,
    }
}

fn configure(ch: &pac::sai1::CH, role: Role, mckdiv: u8, new_sai: bool) {
    ch.cr1.modify(|_, w| w.saien().clear_bit());
    while ch.cr1.read().saien().bit_is_set() {}
    // SAFETY: MCKDIV is a 6-bit field and `pll3_for` gives 4 or 2 (clock_plan
    // tests); every other field is set through its enumerated variants.
    ch.cr1.write(|w| {
        let w = match role {
            Role::Master => w.mode().master_tx().syncen().asynchronous(),
            Role::InternalSlave => w.mode().slave_tx().syncen().internal(),
            Role::ExternalSlave => w.mode().slave_tx().syncen().external(),
        };
        let w = w
            .prtcfg()
            .free()
            .ds()
            .bit32()
            .lsbfirst()
            .msb_first()
            // The CS4344 samples on SCK rising edges, so data must change on
            // falling ones (the ST HAL's I2S transmit setting).
            .ckstr()
            .rising_edge()
            .mono()
            .stereo()
            .nodiv()
            .master_clock()
            .dmaen()
            .enabled();
        unsafe { w.mckdiv().bits(mckdiv) }
    });
    if role == Role::Master && new_sai {
        // SAFETY: sets only MCKEN in this block's CR1; the caller checked the
        // silicon has it.
        ch.cr1.modify(|r, w| unsafe { w.bits(r.bits() | MCKEN) });
    }
    ch.cr2.write(|w| w.fth().quarter1().fflush().set_bit());
    // SAFETY: FRL 63 (64-bit frame), FSALL 31 (FS half the frame), NBSLOT 1
    // (two slots) and SLOTEN 0b11 are within their RM0433 field widths.
    ch.frcr.write(|w| {
        unsafe { w.frl().bits(63).fsall().bits(31) }
            .fsdef()
            .set_bit()
            .fspol()
            .falling_edge()
            .fsoff()
            .before_first()
    });
    ch.slotr.write(|w| {
        unsafe { w.nbslot().bits(1).sloten().bits(0b11) }
            .slotsz()
            .bit32()
    });
}
