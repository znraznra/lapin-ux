// SPDX-License-Identifier: GPL-3.0-only
//! Clipboard history (Super+V) and the emoji / kaomoji / symbols panel
//! (Super+.), in one panel with tabs like the benchmark. Picking a history
//! item puts it back on the clipboard and pastes it into the app you were
//! in; picking an emoji types it there.

pub mod wl;

use std::sync::{Arc, LazyLock};
use std::time::SystemTime;

use cosmic::app::Task;
use cosmic::iced::{Alignment, Length, Subscription, stream};
use cosmic::widget::{self, button, column, container, icon, row, scrollable, text, text_input};
use cosmic::{Element, theme};
use futures::{SinkExt, StreamExt};
use lapin_ux_core::clip::{ClipData, History, KAOMOJI, SYMBOLS, preview};

use crate::app::{Message, Shell};
use crate::panels::PanelKind;
use crate::style;

pub const WIDTH: u32 = 380;
const HEIGHT: f32 = 470.0;
pub(crate) static SEARCH_ID: LazyLock<widget::Id> = LazyLock::new(|| widget::Id::new("lapin-ux-emoji-search"));

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Emoji,
    Kaomoji,
    Symbols,
    Clipboard,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClipPanel {
    pub tab: Tab,
    pub query: String,
    pub group: usize,
}

#[derive(Default)]
pub struct ClipState {
    pub history: History,
    handle: Option<Arc<wl::Handle>>,
    pub unavailable: Option<String>,
    restored: bool,
}

impl std::fmt::Debug for ClipState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClipState").field("items", &self.history.items().len()).finish()
    }
}

#[derive(Clone)]
pub enum ClipMsg {
    Ready(Arc<wl::Handle>),
    Event(wl::Event),
    Open(Tab),
    Tab(Tab),
    Group(usize),
    Query(String),
    /// Paste a history item into the focused app.
    Pick(u64),
    Pin(u64),
    Delete(u64),
    ClearAll,
    /// Type this into the focused app.
    Insert(String),
}

impl std::fmt::Debug for ClipMsg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClipMsg::Ready(_) => write!(f, "Ready"),
            ClipMsg::Event(wl::Event::Copied(_)) => write!(f, "Copied"),
            ClipMsg::Event(e) => write!(f, "{e:?}"),
            ClipMsg::Open(t) => write!(f, "Open({t:?})"),
            ClipMsg::Tab(t) => write!(f, "Tab({t:?})"),
            ClipMsg::Group(g) => write!(f, "Group({g})"),
            ClipMsg::Query(_) => write!(f, "Query"),
            ClipMsg::Pick(i) => write!(f, "Pick({i})"),
            ClipMsg::Pin(i) => write!(f, "Pin({i})"),
            ClipMsg::Delete(i) => write!(f, "Delete({i})"),
            ClipMsg::ClearAll => write!(f, "ClearAll"),
            ClipMsg::Insert(_) => write!(f, "Insert"),
        }
    }
}

/// Runs the clipboard thread and forwards what it sees.
pub fn subscription() -> Subscription<ClipMsg> {
    Subscription::run_with("lapin-ux-clipboard", |_| {
        stream::channel(16, |mut out: futures::channel::mpsc::Sender<ClipMsg>| async move {
            let (tx, mut rx) = futures::channel::mpsc::unbounded();
            match wl::spawn(move |e| {
                let _ = tx.unbounded_send(e);
            }) {
                Some(h) => {
                    let _ = out.send(ClipMsg::Ready(Arc::new(h))).await;
                }
                None => {
                    let _ = out.send(ClipMsg::Event(wl::Event::Unavailable("could not start".into()))).await;
                }
            }
            while let Some(e) = rx.next().await {
                let _ = out.send(ClipMsg::Event(e)).await;
            }
            futures::future::pending::<()>().await;
        })
    })
}

fn groups() -> Vec<emojis::Group> {
    emojis::Group::iter().collect()
}

/// A representative emoji for each category button.
fn group_icon(g: emojis::Group) -> &'static str {
    use emojis::Group::*;
    match g {
        SmileysAndEmotion => "\u{1F600}",
        PeopleAndBody => "\u{1F44B}",
        AnimalsAndNature => "\u{1F43B}",
        FoodAndDrink => "\u{1F354}",
        TravelAndPlaces => "\u{1F697}",
        Activities => "\u{26BD}",
        Objects => "\u{1F4A1}",
        Symbols => "\u{2764}\u{FE0F}",
        Flags => "\u{1F3C1}",
    }
}

impl Shell {
    fn clip_panel(&mut self) -> Option<&mut ClipPanel> {
        match &mut self.panel {
            Some(crate::panels::Panel { kind: PanelKind::Clip(c), .. }) => Some(c),
            _ => None,
        }
    }

    pub(crate) fn update_clip(&mut self, msg: ClipMsg) -> Task<Message> {
        tracing::debug!(?msg, "clipboard panel");
        match msg {
            ClipMsg::Ready(h) => {
                self.clip.handle = Some(h);
                if !self.clip.restored {
                    self.clip.restored = true;
                    let pinned = self.state.pinned_clips.clone();
                    self.clip.history.restore_pinned(&pinned);
                }
            }
            ClipMsg::Event(wl::Event::Copied(data)) => {
                self.clip.history.push(data, SystemTime::now());
            }
            ClipMsg::Event(wl::Event::Unavailable(e)) => {
                self.clip.unavailable = Some(e);
            }
            ClipMsg::Open(tab) => {
                if matches!(&self.panel, Some(p) if matches!(&p.kind, PanelKind::Clip(c) if c.tab == tab)) {
                    return self.close_panel();
                }
                let open = self.open_panel(PanelKind::Clip(ClipPanel { tab, ..Default::default() }));
                let close_centre = Task::future(async {
                    crate::notify_client::close_center().await;
                    cosmic::Action::App(Message::Noop)
                });
                return Task::batch([open, close_centre, text_input::focus(SEARCH_ID.clone())]);
            }
            ClipMsg::Tab(tab) => {
                if let Some(c) = self.clip_panel() {
                    c.tab = tab;
                    c.query.clear();
                }
                return text_input::focus(SEARCH_ID.clone());
            }
            ClipMsg::Group(g) => {
                if let Some(c) = self.clip_panel() {
                    c.group = g;
                    c.query.clear();
                }
            }
            ClipMsg::Query(q) => {
                if let Some(c) = self.clip_panel() {
                    c.query = q;
                    if c.tab == Tab::Clipboard {
                        c.tab = Tab::Emoji;
                    }
                }
            }
            ClipMsg::Pick(id) => {
                let Some(item) = self.clip.history.get(id).cloned() else { return Task::none() };
                let close = self.close_panel();
                if let Some(h) = &self.clip.handle {
                    h.send(wl::Command::SetAndPaste(item.data));
                }
                return close;
            }
            ClipMsg::Pin(id) => {
                self.clip.history.toggle_pin(id);
                self.save_pinned();
            }
            ClipMsg::Delete(id) => {
                self.clip.history.remove(id);
                self.save_pinned();
            }
            ClipMsg::ClearAll => {
                self.clip.history.clear();
            }
            ClipMsg::Insert(s) => {
                let close = self.close_panel();
                if emojis::get(&s).is_some() {
                    self.state.recent_emoji.retain(|e| e != &s);
                    self.state.recent_emoji.insert(0, s.clone());
                    self.state.recent_emoji.truncate(24);
                    self.persist_state();
                }
                if let Some(h) = &self.clip.handle {
                    h.send(wl::Command::Type(s));
                }
                return close;
            }
        }
        Task::none()
    }

    fn save_pinned(&mut self) {
        let pinned = self.clip.history.pinned_texts();
        if pinned != self.state.pinned_clips {
            self.state.pinned_clips = pinned;
            self.persist_state();
        }
    }
}

// ---- view ------------------------------------------------------------------------

fn tab_button<'a>(label: &'static str, tab: Tab, current: Tab) -> Element<'a, Message> {
    button::custom(container(text::body(label)).center_x(Length::Fill))
        .width(Length::Fill)
        .padding([6, 4])
        .class(style::flat_button(tab == current))
        .on_press(Message::Clip(ClipMsg::Tab(tab)))
        .into()
}

fn glyph_button<'a>(s: String, size: u16, width: f32) -> Element<'a, Message> {
    button::custom(container(text(s.clone()).size(size)).center(Length::Fill))
        .width(width)
        .height(width)
        .padding(0)
        .class(style::flat_button(false))
        .on_press(Message::Clip(ClipMsg::Insert(s)))
        .into()
}

fn grid<'a>(items: Vec<String>, per_row: usize, size: u16, cell: f32) -> Element<'a, Message> {
    let mut col = column::with_capacity(items.len() / per_row + 1).spacing(2);
    for chunk in items.chunks(per_row) {
        col = col.push(row::with_children(chunk.iter().map(|s| glyph_button(s.clone(), size, cell)).collect::<Vec<_>>()).spacing(2));
    }
    col.into()
}

fn emoji_tab<'a>(shell: &'a Shell, p: &'a ClipPanel) -> Element<'a, Message> {
    let q = p.query.trim().to_lowercase();
    if !q.is_empty() {
        let hits: Vec<String> = emojis::iter()
            .filter(|e| e.name().contains(q.as_str()) || e.shortcodes().any(|s| s.contains(q.as_str())))
            .take(160)
            .map(|e| e.as_str().to_string())
            .collect();
        if hits.is_empty() {
            return container(text::body("No emoji found").class(style::dim_text())).padding(16).into();
        }
        return scrollable(grid(hits, 8, 24, 42.0)).height(Length::Fill).into();
    }
    let all = groups();
    let g = all.get(p.group).copied().unwrap_or(emojis::Group::SmileysAndEmotion);
    let mut body = column::with_capacity(4).spacing(6);
    if p.group == 0 && !shell.state.recent_emoji.is_empty() {
        body = body
            .push(text::caption("Most recently used").class(style::dim_text()))
            .push(grid(shell.state.recent_emoji.iter().take(16).cloned().collect(), 8, 24, 42.0));
    }
    // Default (yellow) skin tone only; tone variants would multiply the list.
    let list: Vec<String> = g.emojis().filter(|e| e.skin_tone().is_none_or(|t| t == emojis::SkinTone::Default)).map(|e| e.as_str().to_string()).collect();
    body = body.push(grid(list, 8, 24, 42.0));
    let cats = row::with_children(
        all.iter()
            .enumerate()
            .map(|(i, gr)| {
                button::custom(container(text(group_icon(*gr)).size(16)).center(Length::Fill))
                    .width(Length::Fill)
                    .height(32)
                    .padding(0)
                    .class(style::flat_button(i == p.group))
                    .on_press(Message::Clip(ClipMsg::Group(i)))
                    .into()
            })
            .collect::<Vec<_>>(),
    )
    .spacing(2);
    column::with_children(vec![scrollable(body).height(Length::Fill).into(), cats.into()]).spacing(6).into()
}

fn clipboard_tab(shell: &Shell) -> Element<'_, Message> {
    if let Some(e) = &shell.clip.unavailable {
        return container(text::body(format!("Clipboard history isn't available: {e}")).class(style::dim_text())).padding(16).into();
    }
    let items = shell.clip.history.items();
    if items.is_empty() {
        return container(
            column::with_children(vec![
                text::heading("You'll see your clipboard history here").into(),
                text::body("Copy something to get started.").class(style::dim_text()).into(),
            ])
            .spacing(6),
        )
        .padding(16)
        .into();
    }
    let mut list = column::with_capacity(items.len() + 1).spacing(6);
    list = list.push(row::with_children(vec![
        widget::Space::new().width(Length::Fill).into(),
        button::text("Clear all").on_press(Message::Clip(ClipMsg::ClearAll)).into(),
    ]));
    for it in items {
        let content: Element<'_, Message> = match &it.data {
            ClipData::Text(t) => text::body(preview(t, 220)).width(Length::Fill).into(),
            ClipData::Image { png, width, height } => {
                let h = (*height as f32).min(120.0);
                let w = if *height > 0 { *width as f32 * h / *height as f32 } else { 120.0 };
                widget::image(widget::image::Handle::from_bytes(png.as_ref().clone())).width(w.min(280.0)).height(h).into()
            }
        };
        let tools = column::with_children(vec![
            button::custom(icon::from_name(if it.pinned { "view-pin-symbolic" } else { "pin-symbolic" }).size(14))
                .padding(4)
                .class(style::flat_button(it.pinned))
                .on_press(Message::Clip(ClipMsg::Pin(it.id)))
                .into(),
            button::custom(icon::from_name("user-trash-symbolic").size(14))
                .padding(4)
                .class(style::flat_button(false))
                .on_press(Message::Clip(ClipMsg::Delete(it.id)))
                .into(),
        ])
        .spacing(2);
        let card = button::custom(row::with_children(vec![content, tools.into()]).spacing(8).align_y(Alignment::Start))
            .width(Length::Fill)
            .padding([10, 12])
            .class(theme::Button::Custom {
                active: Box::new(|_, t| card_look(t, 0.0)),
                disabled: Box::new(|t| card_look(t, 0.0)),
                hovered: Box::new(|_, t| card_look(t, 0.06)),
                pressed: Box::new(|_, t| card_look(t, 0.1)),
            })
            .on_press(Message::Clip(ClipMsg::Pick(it.id)));
        list = list.push(card);
    }
    scrollable(list).height(Length::Fill).into()
}

fn card_look(t: &cosmic::Theme, lift: f32) -> button::Style {
    let c = t.cosmic();
    let mut bg: cosmic::iced::Color = c.background(true).component.base.into();
    bg.r = (bg.r + lift).min(1.0);
    bg.g = (bg.g + lift).min(1.0);
    bg.b = (bg.b + lift).min(1.0);
    button::Style {
        background: Some(cosmic::iced::Background::Color(bg)),
        border_radius: style::CONTROL_RADIUS.into(),
        text_color: Some(c.on_bg_color().into()),
        icon_color: Some(c.on_bg_color().into()),
        ..Default::default()
    }
}

pub fn view<'a>(shell: &'a Shell, p: &'a ClipPanel) -> Element<'a, Message> {
    let search = text_input::search_input("Search emoji", &p.query)
        .id(SEARCH_ID.clone())
        .on_input(|q| Message::Clip(ClipMsg::Query(q)))
        .width(Length::Fill);
    let tabs = row::with_children(vec![
        tab_button("\u{1F600}", Tab::Emoji, p.tab),
        tab_button(";-)", Tab::Kaomoji, p.tab),
        tab_button("\u{03A9}", Tab::Symbols, p.tab),
        tab_button("Clipboard", Tab::Clipboard, p.tab),
    ])
    .spacing(2);
    let body: Element<'_, Message> = match p.tab {
        Tab::Emoji => emoji_tab(shell, p),
        Tab::Kaomoji => scrollable(column::with_children(
            KAOMOJI
                .chunks(2)
                .map(|pair| {
                    row::with_children(
                        pair.iter()
                            .map(|k| {
                                button::custom(container(text::body(*k)).center_x(Length::Fill))
                                    .width(Length::Fill)
                                    .padding([8, 4])
                                    .class(style::flat_button(false))
                                    .on_press(Message::Clip(ClipMsg::Insert((*k).to_string())))
                                    .into()
                            })
                            .collect::<Vec<_>>(),
                    )
                    .spacing(2)
                    .into()
                })
                .collect::<Vec<_>>(),
        ))
        .height(Length::Fill)
        .into(),
        Tab::Symbols => {
            let mut col = column::with_capacity(SYMBOLS.len() * 2).spacing(6);
            for (name, chars) in SYMBOLS {
                col = col.push(text::caption(*name).class(style::dim_text()));
                col = col.push(grid(chars.chars().map(String::from).collect(), 8, 18, 42.0));
            }
            scrollable(col).height(Length::Fill).into()
        }
        Tab::Clipboard => clipboard_tab(shell),
    };
    column::with_children(vec![search.into(), tabs.into(), body])
        .spacing(10)
        .padding(12)
        .height(Length::Fixed(HEIGHT))
        .into()
}
