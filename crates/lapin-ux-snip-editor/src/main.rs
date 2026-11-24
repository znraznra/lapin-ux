// SPDX-License-Identifier: GPL-3.0-only
//! Snip editor (#78): a small app to mark up a screenshot with a pen and a
//! highlighter, crop it, then copy or save it. It opens the picture named on
//! the command line; the snipping tool's notification and Photos offer "Edit".
//! Nothing leaves the computer. It writes no shared desktop setting.

mod model;

use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::sync::Arc;

use cosmic::app::{Settings, Task};
use cosmic::iced::keyboard::{Event as KeyEvent, Key};
use cosmic::iced::{Alignment, Length, Point, Size, Subscription};
use cosmic::widget::image::Handle;
use cosmic::widget::{self, button, column, container, mouse_area, row, text};
use cosmic::{ApplicationExt, Core, Element, theme};
use image::RgbaImage;

use model::{Crop, Grip, History, Ink, Stroke};

const APP_ID: &str = "dev.lapinux.SnipEditor";
/// The preview is drawn at most this wide or tall; the saved picture is full size.
const PREVIEW_MAX: u32 = 1600;
/// How close to a crop edge counts as grabbing it, in screen pixels.
const GRAB: f32 = 12.0;
const COLORS: [(&str, [u8; 3]); 6] = [
    ("Red", [230, 40, 40]),
    ("Yellow", [255, 214, 0]),
    ("Green", [40, 180, 70]),
    ("Blue", [30, 110, 230]),
    ("Black", [20, 20, 20]),
    ("White", [255, 255, 255]),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tool {
    Pen,
    Highlighter,
    Crop,
}

#[derive(Debug, Clone)]
enum Message {
    Loaded(Result<Arc<RgbaImage>, String>),
    Tool(Tool),
    Color([u8; 3]),
    Size(u8),
    /// The pointer moved over the picture: position in picture pixels, and how
    /// many picture pixels one screen pixel is.
    Move((f32, f32), f32),
    Press,
    Release,
    Undo,
    Redo,
    ApplyCrop,
    Copy,
    Save,
    Done(String),
}

/// What undo goes back to.
#[derive(Clone)]
struct Snap {
    base: Arc<RgbaImage>,
    strokes: Vec<Stroke>,
}

struct App {
    core: Core,
    base: Option<Arc<RgbaImage>>,
    /// The base scaled down for the screen.
    small: Option<RgbaImage>,
    strokes: Vec<Stroke>,
    drawing: Option<Stroke>,
    tool: Tool,
    color: [u8; 3],
    size: u8,
    crop: Crop,
    /// A crop handle being dragged: which one, the rectangle and the pointer when it was grabbed.
    drag: Option<(Grip, Crop, (f32, f32))>,
    pointer: (f32, f32),
    /// Picture pixels per screen pixel, as last seen.
    k: f32,
    history: History<Snap>,
    preview: Option<Handle>,
    notice: Option<String>,
    failed: Option<String>,
}

impl App {
    fn dims(&self) -> (u32, u32) {
        self.base.as_ref().map(|b| b.dimensions()).unwrap_or((0, 0))
    }

    fn snap(&self) -> Option<Snap> {
        Some(Snap { base: self.base.clone()?, strokes: self.strokes.clone() })
    }

    fn set_base(&mut self, base: Arc<RgbaImage>) {
        let (w, h) = base.dimensions();
        let s = (PREVIEW_MAX as f32 / w.max(h) as f32).min(1.0);
        self.small = Some(if s < 1.0 {
            image::imageops::resize(&*base, ((w as f32 * s) as u32).max(1), ((h as f32 * s) as u32).max(1), image::imageops::FilterType::Triangle)
        } else {
            (*base).clone()
        });
        self.crop = Crop::full(w, h);
        self.base = Some(base);
    }

    /// Redraws the preview: the picture, every stroke, and the crop frame while cropping.
    fn refresh(&mut self) {
        let (Some(small), Some(base)) = (&self.small, &self.base) else { return };
        let scale = (small.width() as f32 / base.width() as f32, small.height() as f32 / base.height() as f32);
        let mut img = small.clone();
        for s in self.strokes.iter().chain(self.drawing.iter()) {
            model::draw_stroke(&mut img, s, scale);
        }
        if self.tool == Tool::Crop {
            model::draw_crop_frame(&mut img, &self.crop, scale);
        }
        self.preview = Some(Handle::from_rgba(img.width(), img.height(), img.into_raw()));
    }

    /// The finished picture at full size, with strokes and the crop applied.
    fn finished(&self) -> Option<RgbaImage> {
        let base = self.base.as_ref()?;
        let mut img = (**base).clone();
        for s in &self.strokes {
            model::draw_stroke(&mut img, s, (1.0, 1.0));
        }
        let (w, h) = img.dimensions();
        if self.crop.is_full(w, h) {
            return Some(img);
        }
        let (x, y, cw, ch) = self.crop.pixels(w, h);
        Some(image::imageops::crop_imm(&img, x, y, cw, ch).to_image())
    }

    fn undo_or_redo(&mut self, undo: bool) {
        let Some(now) = self.snap() else { return };
        let step = if undo { self.history.undo(now) } else { self.history.redo(self.snap().unwrap_or_else(|| unreachable!())) };
        if let Some(s) = step {
            self.strokes = s.strokes;
            self.drawing = None;
            let same = self.base.as_ref().is_some_and(|b| Arc::ptr_eq(b, &s.base));
            if same {
                // Only strokes changed; the crop frame stays where it is.
            } else {
                self.set_base(s.base);
            }
            self.refresh();
        }
    }

    fn ink(&self) -> Option<Ink> {
        match self.tool {
            Tool::Pen => Some(Ink::Pen),
            Tool::Highlighter => Some(Ink::Highlighter),
            Tool::Crop => None,
        }
    }
}

fn load(path: PathBuf) -> Task<Message> {
    Task::future(async move {
        let result = tokio::task::spawn_blocking(move || {
            use image::ImageDecoder;
            let err = |e: image::ImageError| e.to_string();
            let reader = image::ImageReader::open(&path).map_err(|e| e.to_string())?.with_guessed_format().map_err(|e| e.to_string())?;
            let mut decoder = reader.into_decoder().map_err(err)?;
            let orientation = decoder.orientation().ok();
            let mut img = image::DynamicImage::from_decoder(decoder).map_err(err)?;
            if let Some(o) = orientation {
                img.apply_orientation(o);
            }
            Ok::<_, String>(Arc::new(img.to_rgba8()))
        })
        .await
        .unwrap_or_else(|e| Err(e.to_string()));
        cosmic::Action::App(Message::Loaded(result))
    })
}

fn encode_png(img: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

fn pictures_dir() -> PathBuf {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    // XDG user dirs: PICTURES, else ~/Pictures (same lookup as the snipping tool).
    std::fs::read_to_string(home.join(".config/user-dirs.dirs"))
        .ok()
        .and_then(|s| {
            s.lines().find_map(|l| l.strip_prefix("XDG_PICTURES_DIR=")).map(|v| v.trim_matches('"').replace("$HOME", &home.to_string_lossy()))
        })
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("Pictures"))
}

/// Creates a new file in Pictures/Screenshots. The name is claimed with
/// `create_new`, so two saves at once never share a file.
fn create_new_file() -> std::io::Result<(PathBuf, std::fs::File)> {
    let dir = pictures_dir().join("Screenshots");
    std::fs::create_dir_all(&dir)?;
    let stamp = chrono::Local::now().format("Screenshot %Y-%m-%d %H%M%S edited");
    let mut path = dir.join(format!("{stamp}.png"));
    let mut n = 2;
    loop {
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(f) => return Ok((path, f)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                path = dir.join(format!("{stamp} {n}.png"));
                n += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

fn save(img: RgbaImage) -> Task<Message> {
    Task::future(async move {
        let result = tokio::task::spawn_blocking(move || -> Result<PathBuf, String> {
            let bytes = encode_png(&img)?;
            let (path, mut file) = create_new_file().map_err(|e| e.to_string())?;
            if let Err(e) = file.write_all(&bytes).and_then(|()| file.flush()) {
                drop(file);
                let _ = std::fs::remove_file(&path);
                return Err(e.to_string());
            }
            Ok(path)
        })
        .await
        .unwrap_or_else(|e| Err(e.to_string()));
        cosmic::Action::App(Message::Done(match result {
            Ok(p) => format!("Saved to {}", p.parent().map(|d| d.display().to_string()).unwrap_or_default()),
            Err(e) => format!("Could not save: {e}"),
        }))
    })
}

fn copy(img: RgbaImage) -> Task<Message> {
    Task::future(async move {
        let result = tokio::task::spawn_blocking(move || -> Result<(), String> {
            let bytes = encode_png(&img)?;
            let mut child = std::process::Command::new("wl-copy")
                .args(["--type", "image/png"])
                .env_remove("WAYLAND_DEBUG")
                .stdin(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| e.to_string())?;
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(&bytes).map_err(|e| e.to_string())?;
            }
            // wl-copy hands the picture to a background copy of itself and returns.
            let status = child.wait().map_err(|e| e.to_string())?;
            if !status.success() {
                return Err(format!("wl-copy stopped with {status}"));
            }
            Ok(())
        })
        .await
        .unwrap_or_else(|e| Err(e.to_string()));
        cosmic::Action::App(Message::Done(match result {
            Ok(()) => "Copied to the clipboard".to_string(),
            Err(e) => format!("Could not copy: {e}"),
        }))
    })
}

impl cosmic::Application for App {
    type Executor = cosmic::executor::Default;
    type Flags = Option<PathBuf>;
    type Message = Message;
    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, path: Option<PathBuf>) -> (Self, Task<Message>) {
        let mut app = App {
            core,
            base: None,
            small: None,
            strokes: Vec::new(),
            drawing: None,
            tool: Tool::Pen,
            color: COLORS[0].1,
            size: 3,
            crop: Crop::full(0, 0),
            drag: None,
            pointer: (0.0, 0.0),
            k: 1.0,
            history: History::default(),
            preview: None,
            notice: None,
            failed: if path.is_none() { Some("Open a picture with this editor, or choose Edit after taking a snip.".into()) } else { None },
        };
        let title = match app.core.main_window_id() {
            Some(id) => app.set_window_title("Snip editor".into(), id),
            None => Task::none(),
        };
        let loading = path.map(load).unwrap_or_else(Task::none);
        (app, Task::batch([title, loading]))
    }

    fn header_start(&self) -> Vec<Element<'_, Message>> {
        vec![lapin_ux_ui::title_bar("applets-screenshooter-symbolic", "Snip editor")]
    }

    fn subscription(&self) -> Subscription<Message> {
        cosmic::iced::event::listen_with(|event, status, _| {
            if status != cosmic::iced::event::Status::Ignored {
                return None;
            }
            match event {
                cosmic::iced::Event::Keyboard(KeyEvent::KeyPressed { key: Key::Character(c), modifiers, .. }) if modifiers.control() => {
                    match (c.as_str(), modifiers.shift()) {
                        ("z", false) | ("Z", false) => Some(Message::Undo),
                        ("z", true) | ("Z", true) | ("y", _) => Some(Message::Redo),
                        ("s", false) => Some(Message::Save),
                        ("c", false) => Some(Message::Copy),
                        _ => None,
                    }
                }
                _ => None,
            }
        })
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Loaded(Ok(img)) => {
                self.failed = None;
                self.set_base(img);
                self.refresh();
            }
            Message::Loaded(Err(e)) => self.failed = Some(format!("This picture could not be opened: {e}")),
            Message::Tool(t) => {
                // A crop that was set but not applied is applied now, so the
                // preview and the saved picture always agree.
                if self.tool == Tool::Crop && t != Tool::Crop {
                    if let Some((w, h)) = self.base.as_ref().map(|b| b.dimensions()) {
                        if !self.crop.is_full(w, h) {
                            if let (Some(img), Some(snap)) = (self.finished(), self.snap()) {
                                self.history.record(snap);
                                self.strokes.clear();
                                self.set_base(Arc::new(img));
                            }
                        }
                    }
                }
                self.tool = t;
                self.drawing = None;
                self.drag = None;
                self.refresh();
            }
            Message::Color(c) => self.color = c,
            Message::Size(s) => self.size = s,
            Message::Move(p, k) => {
                self.pointer = p;
                self.k = k;
                let (w, h) = self.dims();
                if let Some((grip, start, from)) = self.drag {
                    self.crop = model::drag_crop(start, grip, from, p, w, h);
                    self.refresh();
                } else if let Some(s) = &mut self.drawing {
                    s.points.push(p);
                    self.refresh();
                }
            }
            Message::Press => {
                let (w, h) = self.dims();
                if let Some(ink) = self.ink() {
                    if let Some(snap) = self.snap() {
                        self.history.record(snap);
                        let width = model::stroke_width(self.size as f32, ink, w, h);
                        self.drawing = Some(Stroke { ink, color: self.color, width, points: vec![self.pointer] });
                        self.refresh();
                    }
                } else if self.base.is_some()
                    && let Some(grip) = model::grip_at(&self.crop, self.pointer, GRAB * self.k)
                {
                    self.drag = Some((grip, self.crop, self.pointer));
                }
            }
            Message::Release => {
                self.drag = None;
                if let Some(s) = self.drawing.take() {
                    self.strokes.push(s);
                    self.refresh();
                }
            }
            Message::Undo => self.undo_or_redo(true),
            Message::Redo => self.undo_or_redo(false),
            Message::ApplyCrop => {
                if let (Some(img), Some(snap)) = (self.finished(), self.snap()) {
                    self.history.record(snap);
                    self.strokes.clear();
                    self.set_base(Arc::new(img));
                    self.refresh();
                }
            }
            Message::Copy => {
                if let Some(img) = self.finished() {
                    return copy(img);
                }
            }
            Message::Save => {
                if let Some(img) = self.finished() {
                    return save(img);
                }
            }
            Message::Done(n) => self.notice = Some(n),
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        lapin_ux_ui::a11y::group("Snip editor", self.content())
    }
}

impl App {
    fn content(&self) -> Element<'_, Message> {
        let Some(preview) = self.preview.clone() else {
            let msg = self.failed.clone().unwrap_or_else(|| "Opening...".into());
            return container(text::body(msg)).center(Length::Fill).into();
        };
        let (w, h) = self.dims();
        let picture = cosmic::iced::widget::responsive(move |avail| {
            let shown = model::fit(avail.width, avail.height, w, h);
            let k = if shown.0 > 0.0 { w as f32 / shown.0 } else { 1.0 };
            let image = widget::image(preview.clone()).width(Length::Fixed(shown.0)).height(Length::Fixed(shown.1));
            let area = mouse_area(image)
                .on_press(Message::Press)
                .on_release(Message::Release)
                .on_move(move |p: Point| Message::Move(model::to_picture((p.x, p.y), shown, w, h), k))
                .interaction(cosmic::iced::mouse::Interaction::Crosshair);
            container(area).center(Length::Fill).into()
        });
        column::with_capacity(2)
            .push(self.toolbar())
            .push(container(picture).padding(8).width(Length::Fill).height(Length::Fill))
            .into()
    }

    fn toolbar(&self) -> Element<'_, Message> {
        let tool = |label: &'static str, t: Tool| {
            button::custom(text::body(label))
                .padding([6, 12])
                .class(if self.tool == t { lapin_ux_ui::suggested() } else { lapin_ux_ui::icon() })
                .name(label)
                .on_press(Message::Tool(t))
        };
        let mut bar = row::with_capacity(16)
            .spacing(6)
            .align_y(Alignment::Center)
            .push(tool("Pen", Tool::Pen))
            .push(tool("Highlighter", Tool::Highlighter))
            .push(tool("Crop", Tool::Crop));
        if self.tool == Tool::Crop {
            let (w, h) = self.dims();
            bar = bar.push(button::standard("Apply crop").on_press_maybe((!self.crop.is_full(w, h)).then_some(Message::ApplyCrop)));
        } else {
            for (name, rgb) in COLORS {
                let [r, g, b] = rgb;
                let swatch = container(widget::Space::new().width(18).height(18)).class(theme::Container::custom(move |_| container::Style {
                    background: Some(cosmic::iced::Background::Color(cosmic::iced::Color::from_rgb8(r, g, b))),
                    border: cosmic::iced::Border { radius: 9.0.into(), width: 1.0, color: cosmic::iced::Color::from_rgba(0.5, 0.5, 0.5, 0.8) },
                    ..Default::default()
                }));
                bar = bar.push(
                    button::custom(swatch)
                        .padding(5)
                        .class(if self.color == rgb { lapin_ux_ui::suggested() } else { lapin_ux_ui::icon() })
                        .name(name)
                        .on_press(Message::Color(rgb)),
                );
            }
            bar = bar.push(text::caption("Size")).push(widget::slider(1..=12u8, self.size, Message::Size).width(Length::Fixed(110.0)));
        }
        bar = bar
            .push(widget::Space::new().width(Length::Fill))
            .push(button::icon(widget::icon::from_name("edit-undo-symbolic")).name("Undo").on_press_maybe(self.history.can_undo().then_some(Message::Undo)))
            .push(button::icon(widget::icon::from_name("edit-redo-symbolic")).name("Redo").on_press_maybe(self.history.can_redo().then_some(Message::Redo)));
        if let Some(n) = &self.notice {
            bar = bar.push(text::caption(n.clone()));
        }
        bar = bar.push(button::standard("Copy").on_press(Message::Copy)).push(button::suggested("Save").on_press(Message::Save));
        container(bar).padding([6, 10]).width(Length::Fill).class(theme::Container::Card).into()
    }
}

fn main() -> cosmic::iced::Result {
    lapin_ux_core::crash::install("lapin-ux-snip-editor");
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()))
        .log_internal_errors(false) // #174: a full disk must not end the program
        .init();
    let start = std::env::args_os().nth(1).map(PathBuf::from);
    cosmic::app::run::<App>(
        Settings::default()
            .size(Size::new(1000.0, 700.0))
            .size_limits(cosmic::iced::Limits::NONE.min_width(640.0).min_height(420.0)),
        start,
    )
}
