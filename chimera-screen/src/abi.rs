//! The wasm exports: plain `extern "C"`, one mirror and its buffers. JS
//! reads the pixels and rectangles through the pointers given out, and
//! writes an address's text through `in_ptr`.
//!
//! | export | meaning |
//! |---|---|
//! | `init(engine: u8) -> i32` | the engine's INIT Sound (`EngineType::ALL`'s order); 0, or -1 no such engine |
//! | `in_ptr() -> *mut u8` | `IN_CAP` bytes for an `ed` address's text |
//! | `set(len: u32, stored: f32) -> i32` | that address to the stored value, as `ed set`; 0 landed, 1 refused, -1 bad text |
//! | `focus(page: u16, slot: u8) -> i32` | that page's cell `slot` (knob a-f, 0-5) in the focus band, its value kept; 0, or -1 no such page or cell |
//! | `render(page: u16) -> i32` | `ed describe`'s page drawn; 0, or -1 not shown |
//! | `buf_ptr() -> *const u16` | the 240x320 RGB565 pixels, row by row |
//! | `region(r: u8) -> *const u16` | `[x, y, w, h]` of `Region` r on the page drawn; null where it has none |

use crate::{Fb, Mirror, PageNo, Region, Slot};
use chimera_core::params::EngineType;
use core::cell::RefCell;

/// The longest address text `set` reads.
pub const IN_CAP: usize = 64;

struct Shell {
    mirror: Mirror,
    fb: Fb,
    input: [u8; IN_CAP],
    rect: [u16; 4],
}

thread_local! {
    static SHELL: RefCell<Option<Shell>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Shell) -> R) -> R {
    SHELL.with(|s| {
        let mut s = s.borrow_mut();
        let s = s.get_or_insert_with(|| Shell {
            mirror: Mirror::new(),
            fb: Fb::new(),
            input: [0; IN_CAP],
            rect: [0; 4],
        });
        f(s)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn init(engine: u8) -> i32 {
    match EngineType::ALL.get(usize::from(engine)) {
        Some(&e) => {
            with(|s| s.mirror.init(e));
            0
        }
        None => -1,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn in_ptr() -> *mut u8 {
    with(|s| s.input.as_mut_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn set(len: u32, stored: f32) -> i32 {
    with(|s| {
        let text = s
            .input
            .get(..len as usize)
            .and_then(|b| str::from_utf8(b).ok());
        match text {
            None => -1,
            Some(a) => match s.mirror.set(a, stored) {
                Ok(()) => 0,
                Err(_) => 1,
            },
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn focus(page: u16, slot: u8) -> i32 {
    let Some(slot) = Slot::new(slot) else {
        return -1;
    };
    with(|s| match s.mirror.focus(PageNo(page), slot) {
        Some(()) => 0,
        None => -1,
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn render(page: u16) -> i32 {
    with(|s| match s.mirror.render(PageNo(page), &mut s.fb) {
        Some(()) => 0,
        None => -1,
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn buf_ptr() -> *const u16 {
    with(|s| s.fb.0.as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn region(r: u8) -> *const u16 {
    with(
        |s| match Region::from_u8(r).and_then(|r| s.mirror.region(r)) {
            Some(r) => {
                s.rect = [r.x, r.y, r.w, r.h];
                s.rect.as_ptr()
            }
            None => core::ptr::null(),
        },
    )
}
