//! The sim's frame loop by key script: `main`'s boot and per-frame order
//! (keys, `handle_input`, card work, SYSTEM sync, `update`) on a
//! `DirStore`, with no window. Desktop QA for what the window would show.

use crate::controls::DesktopControls;
use crate::store::DirStore;
use chimera_core::project::LoadLink;
use chimera_core::storage::{Card, SystemSettings, SystemSync};
use chimera_core::ui::UiState;
use chimera_core::ui::animation::UiTick;
use chimera_core::ui::busy::ToastStep;
use chimera_core::ui::settings::CardCx;
use chimera_hal::Ms;
use minifb::Key;
use std::path::PathBuf;

/// One frame, as `main` paces them.
pub const FRAME_MS: u32 = 33;

/// A launch of the sim on the card at `root`.
pub struct Desk {
    pub ui: Box<UiState>,
    pub store: DirStore,
    controls: DesktopControls,
    card: Card,
    sync: SystemSync,
    settings: SystemSettings,
    link: LoadLink,
    now: u32,
    /// The toast `boot` left, read before the first frame.
    pub boot_toast: Option<String>,
}

impl Desk {
    pub fn launch(root: PathBuf) -> Self {
        let mut ui = Box::new(UiState::new());
        let mut store = DirStore::new(root);
        let mut card = Card::new();
        let (sync, settings) = crate::boot(&mut ui, &mut card, &mut store);
        let boot_toast = toast(&mut ui);
        Desk {
            ui,
            store,
            controls: DesktopControls::new(),
            card,
            sync,
            settings,
            link: LoadLink::new(),
            now: 0,
            boot_toast,
        }
    }

    /// One frame with `keys` down.
    pub fn frame(&mut self, keys: &[Key]) {
        self.now += FRAME_MS;
        self.controls.update_events(keys, Ms(self.now));
        self.ui.handle_input(&self.controls);
        let link = &self.link;
        let cx = CardCx {
            card: &mut self.card,
            store: &mut self.store,
            sync: &mut self.sync,
            settings: &mut self.settings,
        };
        self.ui.card_work(cx, link, |swap, _| {
            let _ = swap.settle(link, || false);
        });
        self.ui.sync_system(
            &mut self.sync,
            &mut self.card,
            &mut self.store,
            &mut self.settings,
        );
        self.ui.update(UiTick::for_test());
    }

    pub fn tap(&mut self, k: Key) {
        self.frame(&[k]);
        self.frame(&[]);
    }

    /// Down past `HOLD_MS`, then up.
    pub fn hold(&mut self, k: Key) {
        for _ in 0..chimera_core::ui::hold::HOLD_MS / FRAME_MS + 2 {
            self.frame(&[k]);
        }
        self.frame(&[]);
    }

    pub fn toast(&mut self) -> Option<String> {
        toast(&mut self.ui)
    }
}

fn toast(ui: &mut UiState) -> Option<String> {
    match ui.step_toast(0) {
        ToastStep::Show(t) => Some(t.as_str().to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::unique_root;
    use chimera_core::project::ProjectStatus;
    use chimera_core::project::test_support::same;

    /// SAVE AS, a quick save, quit, relaunch: the same project, SAVED.
    /// Then `CHIMERA_CARD` swapped: each card boots its own toast, and the
    /// first card still holds its project.
    #[test]
    fn save_relaunch_and_swap_cards() {
        let (a, b) = (unique_root(), unique_root());
        let mut d = Desk::launch(a.clone());
        assert_eq!(d.boot_toast.as_deref(), Some("NEW PROJECT"));
        // MENU held on NEW opens SAVE AS; SEQ (Up) takes the name.
        d.hold(Key::M);
        assert_eq!(d.ui.naming().unwrap().text(), "ACID-001");
        d.tap(Key::Up);
        assert_eq!(d.toast().as_deref(), Some("SAVED"));
        while d.ui.in_settings() {
            d.tap(Key::M);
        }
        // An edit (encoder A on Part 1's home), then a quick save.
        d.frame(&[Key::Q]);
        d.frame(&[]);
        assert_eq!(d.ui.project_status(), ProjectStatus::Modified);
        d.hold(Key::M);
        assert_eq!(d.toast().as_deref(), Some("SAVED"));
        assert_eq!(d.ui.project_status(), ProjectStatus::Saved);
        // Quit: `d` stays only as the project to compare against.
        let kept = d.ui.project();

        let mut again = Desk::launch(a.clone());
        assert_eq!(again.boot_toast, None);
        same(again.ui.project(), kept);
        again.frame(&[]);
        assert_eq!(again.ui.project_status(), ProjectStatus::Saved);

        let fresh = Desk::launch(b.clone());
        assert_eq!(fresh.boot_toast.as_deref(), Some("NEW PROJECT"));
        let none = Desk::launch(b.join("absent"));
        assert_eq!(none.boot_toast.as_deref(), Some("NO CARD"));
        let back = Desk::launch(a.clone());
        assert_eq!(back.boot_toast, None);
        same(back.ui.project(), kept);
        for r in [a, b] {
            let _ = std::fs::remove_dir_all(r);
        }
    }

    /// A framebuffer for `render`, as the window's.
    struct Fb(Vec<u16>);

    impl embedded_graphics_core::geometry::OriginDimensions for Fb {
        fn size(&self) -> embedded_graphics_core::geometry::Size {
            embedded_graphics_core::geometry::Size::new(
                chimera_hal::SCREEN_WIDTH as u32,
                chimera_hal::SCREEN_HEIGHT as u32,
            )
        }
    }

    impl embedded_graphics_core::draw_target::DrawTarget for Fb {
        type Color = embedded_graphics_core::pixelcolor::Rgb565;
        type Error = core::convert::Infallible;
        fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
        where
            I: IntoIterator<Item = embedded_graphics_core::Pixel<Self::Color>>,
        {
            use embedded_graphics_core::pixelcolor::raw::{RawData, RawU16};
            let w = chimera_hal::SCREEN_WIDTH as i32;
            for embedded_graphics_core::Pixel(p, c) in pixels {
                if (0..w).contains(&p.x)
                    && p.y >= 0
                    && let Some(px) = self.0.get_mut((p.y * w + p.x) as usize)
                {
                    *px = RawU16::from(c).into_inner();
                }
            }
            Ok(())
        }
    }

    /// Host frame time while turning a SETTINGS › MIDI CONFIG › CHANNELS
    /// cell every frame: keys, `handle_input`, card work, `update` (the
    /// footer's status rehashed once per edited frame) and `render`.
    /// `full()` saved and booted (the costliest hash), then NEW.
    /// `cargo test -p chimera-desktop --release -- --ignored --nocapture
    /// channels_frame_time`
    #[test]
    #[ignore]
    fn channels_frame_time() {
        use chimera_core::project::test_support::full;
        use chimera_core::project::{SaveTo, new_project_id};
        use chimera_core::scope::SCOPE_LEN;
        use chimera_core::ui::perf::PerfStats;
        use chimera_core::ui::settings::rows;
        use std::time::Instant;

        let saved = unique_root();
        {
            let mut store = DirStore::new(saved.clone());
            let mut card = Card::new();
            let (mut sync, mut set, _) = SystemSync::boot(&mut card, &mut store);
            let mut ui = Box::new(UiState::new());
            *ui.project_mut() = *full().0;
            let file = new_project_id(&mut card, &mut store).out.unwrap();
            ui.save_project(
                &mut card,
                &mut store,
                &mut sync,
                &mut set,
                SaveTo::Fresh(file),
            );
        }
        let fresh = unique_root();
        for (what, root) in [("full()", &saved), ("NEW", &fresh)] {
            let mut d = Desk::launch(root.clone());
            d.tap(Key::M);
            let mut path = Vec::new();
            for l in ["MIDI CONFIG", "CHANNELS"] {
                let i = rows(&path).iter().position(|r| r.label == l).unwrap();
                for _ in 0..i {
                    d.tap(Key::Right);
                }
                d.tap(Key::Down);
                path.push(i as u8);
            }
            assert_eq!(
                d.ui.location(),
                chimera_core::ui::nav::Location::settings_at(&path, 0)
            );
            let mut fb = Fb(vec![0; chimera_hal::FB_SIZE]);
            let scope = [0.0; SCOPE_LEN];
            let stats = PerfStats::zero();
            const N: usize = 2000;
            let hashes = d.ui.status_hashes_for_test();
            let mut us = Vec::with_capacity(N);
            for i in 0..N {
                // Encoder C up, then down: CHANNEL 3 ↔ 4, an edit each frame.
                let keys: &[Key] = if i % 2 == 0 {
                    &[Key::E]
                } else {
                    &[Key::LeftShift, Key::E]
                };
                let t = Instant::now();
                d.frame(keys);
                d.ui.render_with_scope(&mut fb, &stats, &scope);
                us.push(t.elapsed().as_secs_f64() * 1e6);
            }
            let rehashed = d.ui.status_hashes_for_test() - hashes;
            assert_eq!(rehashed as usize, N, "one rehash per edited frame");
            us.sort_by(|a, b| a.partial_cmp(b).unwrap());
            println!(
                "{what}: {N} frames, median {:.1} us, p99 {:.1} us, max {:.1} us, {rehashed} rehashes",
                us[N / 2],
                us[N * 99 / 100],
                us[N - 1]
            );
        }
        for r in [saved, fresh] {
            let _ = std::fs::remove_dir_all(r);
        }
    }
}
