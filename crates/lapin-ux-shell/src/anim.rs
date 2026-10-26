// SPDX-License-Identifier: GPL-3.0-only
//! Small motion touches: Start slides up as it opens, and an app's taskbar
//! icon hops while the app starts. Frames are only requested while
//! something is moving.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use cosmic::app::Task;
use cosmic::iced::platform_specific::shell::commands::layer_surface::set_margin;
use cosmic::iced::window;

use crate::app::{Message, Shell};
use crate::config::TaskbarAlignment;

const START_SLIDE: Duration = Duration::from_millis(220);
/// How far below its resting place Start begins. Start rests 12 px above
/// the taskbar, as in Windows 11, and opens 4 px above it: it never touches
/// or covers the taskbar and nothing of it is cut off.
const START_DROP: f32 = 8.0;
const LAUNCH_HOP: Duration = Duration::from_millis(900);
/// Start's slide between Pinned and All apps.
const PAGE_SLIDE: Duration = Duration::from_millis(260);
/// Height of the launch hop.
const HOP: f32 = 5.0;

#[derive(Default)]
pub struct Anims {
    start: Option<(window::Id, Instant)>,
    /// The slide clock was restarted when Start first got focus.
    start_shown: bool,
    launches: HashMap<String, Instant>,
    /// Start sliding to this page (All › / ‹ Back), since.
    page: Option<(crate::app::StartPage, Instant)>,
}

/// `LAPIN_UX_ANIM_SCALE=10` plays animations ten times slower (for checking
/// them frame by frame).
fn slow(d: Duration) -> Duration {
    static SCALE: std::sync::LazyLock<f32> = std::sync::LazyLock::new(|| {
        std::env::var("LAPIN_UX_ANIM_SCALE").ok().and_then(|v| v.parse().ok()).filter(|v: &f32| *v > 0.0).unwrap_or(1.0)
    });
    d.mul_f32(*SCALE)
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

impl Anims {
    pub fn active(&self) -> bool {
        self.start.is_some() || !self.launches.is_empty() || self.page.is_some()
    }
}

impl Shell {
    pub(crate) fn anim_start_opened(&mut self, id: window::Id) {
        self.anims.start = Some((id, Instant::now()));
        self.anims.start_shown = false;
    }

    pub(crate) fn anim_start_shown(&mut self, id: window::Id) {
        if !self.anims.start_shown && self.anims.start.is_some_and(|(s, _)| s == id) {
            self.anims.start = Some((id, Instant::now()));
            self.anims.start_shown = true;
        }
    }

    pub(crate) fn anim_launched(&mut self, app: &str) {
        self.anims.launches.insert(app.to_string(), Instant::now());
    }

    /// Pixels an app's taskbar icon is lifted right now.
    pub(crate) fn launch_lift(&self, app: &str) -> f32 {
        let Some(t0) = self.anims.launches.get(app) else { return 0.0 };
        let t = (t0.elapsed().as_secs_f32() / LAUNCH_HOP.as_secs_f32()).min(1.0);
        // Two hops, the second smaller.
        (std::f32::consts::PI * 2.0 * t).sin().abs() * HOP * (1.0 - t)
    }

    /// Start's page changed with All › / ‹ Back (or Esc): slide to it.
    pub(crate) fn anim_page(&mut self, to: crate::app::StartPage) {
        self.anims.page = Some((to, Instant::now()));
    }

    /// The page slide in progress: the page it goes to and how far (0..1,
    /// eased). None when Start isn't sliding between pages.
    pub(crate) fn page_slide(&self) -> Option<(crate::app::StartPage, f32)> {
        let (to, t0) = self.anims.page?;
        let t = (t0.elapsed().as_secs_f32() / slow(PAGE_SLIDE).as_secs_f32()).min(1.0);
        // Windows' page slide: quick start, long gentle stop.
        (t < 1.0).then(|| (to, 1.0 - (1.0 - t).powi(4)))
    }

    pub(crate) fn anim_tick(&mut self) -> Task<Message> {
        self.anims.launches.retain(|_, t| t.elapsed() < LAUNCH_HOP);
        if self.anims.page.is_some() && (self.page_slide().is_none() || self.start.is_none()) {
            self.anims.page = None;
        }
        let Some((id, t0)) = self.anims.start else { return Task::none() };
        if self.start.as_ref().is_none_or(|s| s.id != id) {
            self.anims.start = None;
            return Task::none();
        }
        let t = (t0.elapsed().as_secs_f32() / slow(START_SLIDE).as_secs_f32()).min(1.0);
        // Keep the clock until Start has shown (its clock restarts then), but
        // give up after a few seconds in case focus never arrives.
        if t >= 1.0 && (self.anims.start_shown || t0.elapsed() > slow(Duration::from_secs(3))) {
            self.anims.start = None;
        }
        let left = self.config.alignment == TaskbarAlignment::Left;
        set_margin(id, 0, 0, self.above_bar() - self.start_drop() as i32, if left { 12 } else { 0 })
    }

    /// How far below its resting place Start is right now.
    pub(crate) fn start_drop(&self) -> f32 {
        let Some((id, t0)) = self.anims.start else { return 0.0 };
        if self.start.as_ref().is_none_or(|s| s.id != id) {
            return 0.0;
        }
        let t = (t0.elapsed().as_secs_f32() / slow(START_SLIDE).as_secs_f32()).min(1.0);
        (START_DROP * (1.0 - ease_out(t))).round()
    }
}
