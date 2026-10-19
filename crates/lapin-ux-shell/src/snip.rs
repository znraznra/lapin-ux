// SPDX-License-Identifier: GPL-3.0-only
//! Snipping overlay (Win+Shift+S, Print Screen): the screen freezes and
//! dims, a toolbar at the top offers Rectangle, Window and Full screen; the
//! snip is copied to the clipboard, saved in Pictures/Screenshots and
//! announced with a notification, like Windows' Snipping Tool.
//!
//! COSMIC's own screenshot tool (cosmic-screenshot) only asks the portal for
//! a picture; this does the same job inside the shell, which already has
//! the compositor's screen-capture protocol, so the overlay is ours.

use std::path::PathBuf;
use std::sync::Arc;

use cosmic::app::Task;
use cosmic::iced::{Alignment, Background, Border, Color, Length, Point};
use cosmic::widget::{self, button, container, icon, mouse_area, row, text};
use cosmic::{Element, theme};

use crate::app::{Message, Shell};
use crate::panels::{Panel, PanelKind};
use crate::wayland::{WaylandRequest, WindowId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Rectangle,
    Window,
    Full,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SnipPanel {
    pub mode: Mode,
    pub image: widget::image::Handle,
    pub rgba: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    /// Drag in progress (logical points on the overlay).
    pub start: Option<Point>,
    pub at: Option<Point>,
}

#[derive(Clone)]
pub enum SnipMsg {
    Open,
    Shot(Option<WindowId>, u32, u32, Arc<Vec<u8>>),
    Mode(Mode),
    Press,
    Move(Point),
    Release,
    Window(WindowId),
    Saved(Option<PathBuf>),
}

impl std::fmt::Debug for SnipMsg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnipMsg::Shot(w, width, height, _) => write!(f, "Shot({w:?}, {width}x{height})"),
            SnipMsg::Move(p) => write!(f, "Move({}, {})", p.x, p.y),
            SnipMsg::Open => write!(f, "Open"),
            SnipMsg::Mode(m) => write!(f, "Mode({m:?})"),
            SnipMsg::Press => write!(f, "Press"),
            SnipMsg::Release => write!(f, "Release"),
            SnipMsg::Window(w) => write!(f, "Window({w:?})"),
            SnipMsg::Saved(p) => write!(f, "Saved({p:?})"),
        }
    }
}

impl Shell {
    fn snip(&mut self) -> Option<&mut SnipPanel> {
        match &mut self.panel {
            Some(Panel { kind: PanelKind::Snip(s), .. }) => Some(s),
            _ => None,
        }
    }

    /// Logical width of the screen the overlay covers (pixels are square,
    /// so the height follows from the picture).
    fn snip_screen(&self) -> (f32, f32) {
        let w = self.bars.first().and_then(|b| b.output_width).map(|w| w as f32).unwrap_or(1920.0);
        (w, 0.0)
    }

    pub(crate) fn update_snip(&mut self, msg: SnipMsg) -> Task<Message> {
        if !matches!(msg, SnipMsg::Shot(..)) {
            tracing::debug!(?msg, "snip");
        }
        match msg {
            SnipMsg::Open => {
                // Freeze the screen first: take the picture, then show the overlay.
                let output = self.bars.first().and_then(|b| b.output_name.clone());
                let close = Task::batch([self.close_start(), self.close_popup()]);
                self.snip_pending = true;
                self.request(WaylandRequest::Shot(output, None));
                return close;
            }
            SnipMsg::Shot(None, w, h, rgba) if self.snip_pending => {
                self.snip_pending = false;
                let image = widget::image::Handle::from_rgba(w, h, rgba.as_ref().clone());
                return self.open_panel(PanelKind::Snip(Box::new(SnipPanel { mode: Mode::Rectangle, image, rgba, width: w, height: h, start: None, at: None })));
            }
            SnipMsg::Shot(Some(_), w, h, rgba) => {
                // A window's own picture (Window mode).
                let close = self.close_panel();
                return Task::batch([close, finish(w, h, &rgba, None)]);
            }
            SnipMsg::Shot(None, ..) => {}
            SnipMsg::Mode(Mode::Full) => {
                let Some(s) = self.snip() else { return Task::none() };
                let (w, h, rgba) = (s.width, s.height, s.rgba.clone());
                let close = self.close_panel();
                return Task::batch([close, finish(w, h, &rgba, None)]);
            }
            SnipMsg::Mode(m) => {
                if m == Mode::Window {
                    for id in self.taskbar.windows().iter().filter(|w| !w.minimized).map(|w| w.handle).collect::<Vec<_>>() {
                        if !self.thumbs.contains_key(&id) {
                            self.request(WaylandRequest::Capture(id));
                        }
                    }
                }
                if let Some(s) = self.snip() {
                    s.mode = m;
                    s.start = None;
                }
            }
            SnipMsg::Press => {
                if let Some(s) = self.snip()
                    && s.mode == Mode::Rectangle
                {
                    s.start = s.at;
                }
            }
            SnipMsg::Move(p) => {
                if let Some(s) = self.snip() {
                    s.at = Some(p);
                }
            }
            SnipMsg::Release => {
                let (sw, sh) = self.snip_screen();
                let Some(s) = self.snip() else { return Task::none() };
                let (Some(a), Some(b)) = (s.start.take(), s.at) else { return Task::none() };
                let rect = to_pixels(a, b, sw, sh, s.width, s.height);
                if rect.2 < 4 || rect.3 < 4 {
                    return Task::none(); // a click, not a snip
                }
                let (w, h, rgba) = (s.width, s.height, s.rgba.clone());
                let close = self.close_panel();
                return Task::batch([close, finish(w, h, &rgba, Some(rect))]);
            }
            SnipMsg::Window(id) => {
                let close = self.close_panel();
                self.request(WaylandRequest::Shot(None, Some(id)));
                return close;
            }
            SnipMsg::Saved(path) => {
                return Task::future(async move {
                    match path {
                        Some(p) => notify_saved(&p).await,
                        None => crate::start_search::notify("Couldn't take the screenshot", "It couldn't be saved or copied.").await,
                    }
                    cosmic::Action::App(Message::Noop)
                });
            }
        }
        Task::none()
    }
}

/// Selection in screen pixels: (x, y, w, h).
fn to_pixels(a: Point, b: Point, sw: f32, sh: f32, pw: u32, ph: u32) -> (u32, u32, u32, u32) {
    let fx = pw as f32 / sw.max(1.0);
    let fy = if sh > 0.0 { ph as f32 / sh } else { fx };
    let x0 = (a.x.min(b.x) * fx).clamp(0.0, pw as f32) as u32;
    let y0 = (a.y.min(b.y) * fy).clamp(0.0, ph as f32) as u32;
    let x1 = (a.x.max(b.x) * fx).clamp(0.0, pw as f32) as u32;
    let y1 = (a.y.max(b.y) * fy).clamp(0.0, ph as f32) as u32;
    (x0, y0, x1 - x0, y1 - y0)
}

/// Crop, save as PNG in Pictures/Screenshots, copy to the clipboard.
fn finish(w: u32, h: u32, rgba: &Arc<Vec<u8>>, rect: Option<(u32, u32, u32, u32)>) -> Task<Message> {
    let rgba = rgba.clone();
    Task::future(async move {
        let saved = tokio::task::spawn_blocking(move || -> Option<PathBuf> {
            let (x, y, cw, ch) = rect.unwrap_or((0, 0, w, h));
            let mut out = Vec::with_capacity((cw * ch * 4) as usize);
            for row in y..y + ch {
                let start = ((row * w + x) * 4) as usize;
                out.extend_from_slice(rgba.get(start..start + (cw * 4) as usize)?);
            }
            let dir = pictures_dir().join("Screenshots");
            std::fs::create_dir_all(&dir).ok()?;
            let name = chrono::Local::now().format("Screenshot %Y-%m-%d %H%M%S.png").to_string();
            let path = dir.join(name);
            let file = std::io::BufWriter::new(std::fs::File::create(&path).ok()?);
            let mut enc = png::Encoder::new(file, cw, ch);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().ok()?.write_image_data(&out).ok()?;
            Some(path)
        })
        .await
        .ok()
        .flatten();
        if let Some(p) = &saved {
            copy_png(p).await;
            lapin_ux_core::sound::play("camera-shutter");
        }
        cosmic::Action::App(Message::Snip(SnipMsg::Saved(saved)))
    })
}

fn pictures_dir() -> PathBuf {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    // XDG user dirs: PICTURES, else ~/Pictures.
    std::fs::read_to_string(home.join(".config/user-dirs.dirs"))
        .ok()
        .and_then(|s| {
            s.lines().find_map(|l| l.strip_prefix("XDG_PICTURES_DIR=")).map(|v| v.trim_matches('"').replace("$HOME", &home.to_string_lossy()))
        })
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("Pictures"))
}

/// Put the PNG on the clipboard (wl-copy stays around to serve it).
async fn copy_png(path: &std::path::Path) {
    let Ok(file) = std::fs::File::open(path) else { return };
    let _ = tokio::process::Command::new("wl-copy").args(["--type", "image/png"]).stdin(file).spawn();
}

async fn notify_saved(path: &std::path::Path) {
    let Ok(conn) = zbus::Connection::session().await else { return };
    let mut hints: std::collections::HashMap<&str, zbus::zvariant::Value> = std::collections::HashMap::new();
    let p = path.display().to_string();
    hints.insert("image-path", p.as_str().into());
    hints.insert("suppress-sound", true.into());
    let _ = conn
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(
                "Snipping Tool",
                0u32,
                "applets-screenshooter-symbolic",
                "Screenshot copied to clipboard",
                format!("Saved to {}", path.parent().map(|d| d.display().to_string()).unwrap_or_default()),
                Vec::<&str>::new(),
                hints,
                -1i32,
            ),
        )
        .await;
}

fn dim() -> theme::Container<'static> {
    theme::Container::custom(|_| container::Style { background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.45))), ..Default::default() })
}

fn mode_button<'a>(ic: &'static str, label: &'static str, m: Mode, current: Mode) -> Element<'a, Message> {
    widget::tooltip(
        button::custom(icon::from_name(ic).size(18))
            .padding(8)
            .class(if m == current { theme::Button::Suggested } else { theme::Button::Icon })
            .on_press(Message::Snip(SnipMsg::Mode(m))),
        text::body(label),
        widget::tooltip::Position::Bottom,
    )
    .into()
}

pub fn view<'a>(shell: &'a Shell, s: &'a SnipPanel) -> Element<'a, Message> {
    let (sw, sh) = shell.snip_screen();
    let picture = widget::image(s.image.clone()).width(Length::Fill).height(Length::Fill).content_fit(cosmic::iced::ContentFit::Fill);

    // Dim everything but the selection.
    let shade: Element<'a, Message> = match (s.mode, s.start, s.at) {
        (Mode::Rectangle, Some(a), Some(b)) => {
            let (x0, y0) = (a.x.min(b.x).max(0.0), a.y.min(b.y).max(0.0));
            let (w, h) = ((a.x - b.x).abs(), (a.y - b.y).abs());
            let hole = container(widget::Space::new().width(w).height(h)).class(theme::Container::custom(|_| container::Style {
                border: Border { color: Color::WHITE, width: 1.0, radius: 0.0.into() },
                ..Default::default()
            }));
            widget::column::with_children(vec![
                container(widget::Space::new().width(Length::Fill).height(y0)).class(dim()).into(),
                row::with_children(vec![
                    container(widget::Space::new().width(x0).height(h)).class(dim()).into(),
                    hole.into(),
                    container(widget::Space::new().width(Length::Fill).height(h)).class(dim()).into(),
                ])
                .into(),
                container(widget::Space::new().width(Length::Fill).height(Length::Fill)).class(dim()).into(),
            ])
            .into()
        }
        _ => container(widget::Space::new().width(Length::Fill).height(Length::Fill)).class(dim()).into(),
    };
    let input = mouse_area(widget::Space::new().width(Length::Fill).height(Length::Fill))
        .on_press(Message::Snip(SnipMsg::Press))
        .on_release(Message::Snip(SnipMsg::Release))
        .on_move(|p| Message::Snip(SnipMsg::Move(p)))
        .interaction(cosmic::iced::mouse::Interaction::Crosshair);

    let toolbar = container(
        row::with_children(vec![
            mode_button("selection-mode-symbolic", "Rectangle", Mode::Rectangle, s.mode),
            mode_button("window-new-symbolic", "Window", Mode::Window, s.mode),
            mode_button("video-display-symbolic", "Full screen", Mode::Full, s.mode),
            widget::divider::vertical::default().height(24).into(),
            button::custom(icon::from_name("window-close-symbolic").size(18))
                .padding(8)
                .class(theme::Button::Icon)
                .on_press(Message::Panel(crate::panels::PanelMsg::Close))
                .into(),
        ])
        .spacing(4)
        .align_y(Alignment::Center),
    )
    .padding(6)
    .class(crate::style::panel_bg());

    let mut layers: Vec<Element<'a, Message>> = vec![picture.into(), shade, input.into()];
    if s.mode == Mode::Window {
        // Pick the window to snip.
        let cards: Vec<Element<'a, Message>> = shell
            .taskbar
            .windows()
            .iter()
            .filter(|w| !w.minimized)
            .map(|w| {
                button::custom(
                    widget::column::with_children(vec![
                        row::with_children(vec![
                            icon(crate::taskbar_view::app_icon(shell, &w.app)).size(16).into(),
                            text::caption(w.title.clone()).width(Length::Fixed(180.0)).height(18).into(),
                        ])
                        .spacing(6)
                        .into(),
                        crate::previews::picture_pub(shell, w.handle, &w.app, 200.0, 120.0),
                    ])
                    .spacing(6),
                )
                .padding(8)
                .class(crate::style::flat_button(false))
                .on_press(Message::Snip(SnipMsg::Window(w.handle)))
                .into()
            })
            .collect();
        layers.push(
            container(container(row::with_children(cards).spacing(8)).padding(8).class(crate::style::panel_bg()))
                .width(Length::Fill)
                .height(Length::Fill)
                .center(Length::Fill)
                .into(),
        );
    }
    layers.push(container(toolbar).width(Length::Fill).padding([16, 0, 0, 0]).align_x(Alignment::Center).into());
    let _ = (sw, sh);
    cosmic::iced::widget::Stack::with_children(layers).width(Length::Fill).height(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_maps_to_pixels() {
        // A 2x screen: 960x540 logical, 1920x1080 pixels; drag up-left.
        let r = to_pixels(Point::new(110.0, 60.0), Point::new(10.0, 10.0), 960.0, 540.0, 1920, 1080);
        assert_eq!(r, (20, 20, 200, 100));
    }
}
