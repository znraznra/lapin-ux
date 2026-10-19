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
/// How far below its resting place Start begins.
const START_DROP: f32 = 48.0;
const LAUNCH_HOP: Duration = Duration::from_millis(900);
/// Height of the launch hop.
const HOP: f32 = 5.0;

#[derive(Default)]
pub struct Anims {
    start: Option<(window::Id, Instant)>,
    launches: HashMap<String, Instant>,
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

impl Anims {
    pub fn active(&self) -> bool {
        self.start.is_some() || !self.launches.is_empty()
    }
}

impl Shell {
    pub(crate) fn anim_start_opened(&mut self, id: window::Id) {
        self.anims.start = Some((id, Instant::now()));
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

    pub(crate) fn anim_tick(&mut self) -> Task<Message> {
        self.anims.launches.retain(|_, t| t.elapsed() < LAUNCH_HOP);
        let Some((id, t0)) = self.anims.start else { return Task::none() };
        if self.start.as_ref().is_none_or(|s| s.id != id) {
            self.anims.start = None;
            return Task::none();
        }
        let t = (t0.elapsed().as_secs_f32() / START_SLIDE.as_secs_f32()).min(1.0);
        if t >= 1.0 {
            self.anims.start = None;
        }
        let drop = (START_DROP * (1.0 - ease_out(t))).round() as i32;
        let left = self.config.alignment == TaskbarAlignment::Left;
        set_margin(id, 0, 0, self.above_bar() - drop, if left { 12 } else { 0 })
    }
}
