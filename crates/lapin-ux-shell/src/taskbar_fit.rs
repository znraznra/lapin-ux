// SPDX-License-Identifier: GPL-3.0-only
//! "Combine taskbar buttons and hide labels" and "Show smaller taskbar
//! buttons": decides, after every change, whether the taskbar shows a
//! labelled button per window and whether it uses the smaller size (a 40 px
//! bar with 32 px buttons instead of 48/40).

use cosmic::app::Task;
use cosmic::iced::platform_specific::shell::commands::layer_surface::set_size;
use lapin_ux_core::taskbar::fit;

use crate::app::{Message, Shell};
use crate::config::TaskbarAlignment;
use crate::style;

/// Room the right-hand side (tray, status icons, clock) takes, roughly.
const RIGHT_SIDE: f32 = 330.0;

impl Shell {
    /// Room left for app buttons on the main taskbar.
    fn button_space(&self) -> f32 {
        let width = self.bars.first().and_then(|b| b.output_width).unwrap_or(1280) as f32;
        let mut fixed = 16.0 + 44.0 + 44.0; // padding, Start, Task view
        if self.config.show_search {
            fixed += 188.0;
        }
        // Centred, the buttons sit between equal margins as wide as the right side.
        let sides = if self.config.alignment == TaskbarAlignment::Center { 2.0 * RIGHT_SIDE } else { RIGHT_SIDE };
        (width - sides - fixed).max(0.0)
    }

    pub(crate) fn sync_taskbar_fit(&mut self) -> Task<Message> {
        let main = self.bars.first().map(|b| b.output_name.clone());
        let buttons = self.taskbar.buttons_for(main.clone().flatten().as_deref(), true, self.config.multi_display);
        let combined = buttons.len();
        let split: usize = buttons.iter().map(|b| b.windows.len().max(1)).sum();
        let f = fit(self.button_space(), combined, split, self.config.combine_buttons, self.config.small_buttons);
        if f != self.fit {
            tracing::debug!(?f, space = self.button_space(), combined, split, "taskbar fit");
        }
        self.fit = f;
        if f.small == style::small_taskbar() {
            return Task::none();
        }
        style::set_small_taskbar(f.small);
        let h = style::taskbar_height();
        let mut tasks = Vec::new();
        let bars: Vec<_> = self.bars.iter().map(|b| (b.id, b.revealed)).collect();
        for (id, revealed) in bars {
            tasks.push(set_size(id, None, Some(h)));
            tasks.push(self.resize_bar_pub(id, revealed));
        }
        // The desktop's free area changes with the bar.
        Task::batch(tasks)
    }
}
