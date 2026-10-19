// SPDX-License-Identifier: GPL-3.0-only
//! The taskbar's input indicator ("ENG / US", or the input method's own
//! label such as "あ") and its menu, like Windows' input switcher: keyboard
//! layouts from the compositor, input methods from fcitx5 when it runs.
//! Win+Space moves to the next one.

use std::time::Duration;

use cosmic::app::Task;
use cosmic::iced::{Alignment, Length, Subscription, stream};
use cosmic::widget::{self, button, column, container, row, text};
use cosmic::Element;
use futures::SinkExt;

use crate::app::{Message, PopupKind, Shell, rect_key};
use crate::style;

/// An input method in the active fcitx5 group.
#[derive(Debug, Clone, PartialEq)]
pub struct Im {
    /// fcitx5's unique name ("keyboard-us", "mozc", "pinyin").
    pub name: String,
    pub label: String,
    /// Short label for the taskbar ("あ", "拼", "EN").
    pub short: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Fcitx {
    pub ims: Vec<Im>,
    pub current: String,
}

#[derive(Debug, Clone)]
pub enum InputMsg {
    Fcitx(Option<Fcitx>),
    Menu(cosmic::iced::window::Id),
    Layout(u32),
    Im(String),
    Next,
    Settings,
}

impl Shell {
    /// Whether there's more than one way to type (else no indicator).
    pub(crate) fn input_choices(&self) -> usize {
        let ims = self.fcitx.as_ref().map_or(0, |f| f.ims.len());
        if ims > 1 { ims } else { self.quick.layouts.len() }
    }

    /// Two-line indicator text.
    pub(crate) fn input_indicator(&self) -> (String, String) {
        if let Some(f) = self.fcitx.as_ref().filter(|f| f.ims.len() > 1)
            && let Some(im) = f.ims.iter().find(|i| i.name == f.current)
        {
            // Keyboard-only entries ("keyboard-us") read like layouts.
            if let Some(code) = im.name.strip_prefix("keyboard-") {
                return lapin_ux_core::quick::input_indicator(code, &im.label);
            }
            return (im.short.clone(), String::new());
        }
        let q = &self.quick;
        match q.layouts.get(q.kb_group as usize) {
            Some((code, desc)) => lapin_ux_core::quick::input_indicator(code, desc),
            None => (String::new(), String::new()),
        }
    }

    pub(crate) fn update_input(&mut self, msg: InputMsg) -> Task<Message> {
        match msg {
            InputMsg::Fcitx(f) => self.fcitx = f,
            InputMsg::Menu(bar) => return self.open_popup(bar, &rect_key(bar, "input"), PopupKind::InputMenu),
            InputMsg::Layout(g) => {
                let close = self.close_popup();
                return Task::batch([close, self.update(Message::Quick(crate::quick::QuickMsg::KeyboardLayout(g)))]);
            }
            InputMsg::Im(name) => {
                if let Some(f) = &mut self.fcitx {
                    f.current = name.clone();
                }
                let close = self.close_popup();
                return Task::batch([close, Task::future(async move {
                    set_im(&name).await;
                    cosmic::Action::App(Message::Noop)
                })]);
            }
            InputMsg::Next => {
                if let Some(f) = self.fcitx.as_mut().filter(|f| f.ims.len() > 1) {
                    let i = f.ims.iter().position(|im| im.name == f.current).map_or(0, |i| (i + 1) % f.ims.len());
                    let name = f.ims[i].name.clone();
                    f.current = name.clone();
                    return Task::future(async move {
                        set_im(&name).await;
                        cosmic::Action::App(Message::Noop)
                    });
                }
                let n = self.quick.layouts.len() as u32;
                if n > 1 {
                    let next = (self.quick.kb_group + 1) % n;
                    return self.update(Message::Quick(crate::quick::QuickMsg::KeyboardLayout(next)));
                }
            }
            InputMsg::Settings => {
                let close = self.close_popup();
                return Task::batch([close, self.open_settings("keyboard")]);
            }
        }
        Task::none()
    }
}

use cosmic::Application;

/// The indicator on the taskbar.
pub fn indicator<'a>(shell: &'a Shell, bar: cosmic::iced::window::Id) -> Option<Element<'a, Message>> {
    if shell.input_choices() < 2 {
        return None;
    }
    let (top, bottom) = shell.input_indicator();
    let small = style::small_taskbar();
    let mut col = column::with_capacity(2).align_x(Alignment::Center).push(text::caption(top).size(if small { 11 } else { 12 }));
    if !bottom.is_empty() && !small {
        col = col.push(text::caption(bottom).size(12));
    }
    let open = matches!(shell.popup.as_ref().map(|p| &p.kind), Some(PopupKind::InputMenu));
    let btn = button::custom(container(col).center_y(Length::Fill))
        .height(style::task_button())
        .padding([0, 8])
        .class(style::flat_button(open))
        .on_press(Message::Input(InputMsg::Menu(bar)));
    Some(crate::taskbar_view::track(shell, rect_key(bar, "input"), btn))
}

/// The menu: every layout / input method, the current one marked.
pub fn menu(shell: &Shell) -> Element<'_, Message> {
    use crate::taskbar_view::panel;
    let mut items = column::with_capacity(8).spacing(2);
    let entry = |big: String, small: String, label: String, on: bool, msg: Message| -> Element<'_, Message> {
        button::custom(
            row::with_children(vec![
                column::with_children(vec![text::body(big).into(), text::caption(small).into()]).width(48).align_x(Alignment::Center).into(),
                text::body(label).width(Length::Fill).into(),
            ])
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .padding([6, 10])
        .class(style::flat_button(on))
        .on_press(msg)
        .into()
    };
    if let Some(f) = shell.fcitx.as_ref().filter(|f| f.ims.len() > 1) {
        for im in &f.ims {
            let (big, small) = match im.name.strip_prefix("keyboard-") {
                Some(code) => lapin_ux_core::quick::input_indicator(code, &im.label),
                None => (im.short.clone(), String::new()),
            };
            items = items.push(entry(big, small, im.label.clone(), im.name == f.current, Message::Input(InputMsg::Im(im.name.clone()))));
        }
    } else {
        for (i, (code, desc)) in shell.quick.layouts.iter().enumerate() {
            let (big, small) = lapin_ux_core::quick::input_indicator(code, desc);
            items = items.push(entry(big, small, desc.clone(), i as u32 == shell.quick.kb_group, Message::Input(InputMsg::Layout(i as u32))));
        }
    }
    items = items.push(widget::divider::horizontal::light());
    items = items.push(
        button::custom(text::body("More keyboard settings"))
            .width(Length::Fill)
            .padding([8, 10])
            .class(style::flat_button(false))
            .on_press(Message::Input(InputMsg::Settings)),
    );
    panel(items, 300.0)
}

// ---- fcitx5 over D-Bus --------------------------------------------------------

const FCITX: &str = "org.fcitx.Fcitx5";
const PATH: &str = "/controller";
const IFACE: &str = "org.fcitx.Fcitx.Controller1";

async fn set_im(name: &str) {
    let Ok(conn) = zbus::Connection::session().await else { return };
    let _ = conn.call_method(Some(FCITX), PATH, Some(IFACE), "SetCurrentIM", &(name,)).await;
}

async fn read_fcitx(conn: &zbus::Connection) -> Option<Fcitx> {
    let call = |m: &'static str| conn.call_method(Some(FCITX), PATH, Some(IFACE), m, &());
    let group: String = call("CurrentInputMethodGroup").await.ok()?.body().deserialize().ok()?;
    let info = conn.call_method(Some(FCITX), PATH, Some(IFACE), "InputMethodGroupInfo", &(group.as_str(),)).await.ok()?;
    let (_layout, items): (String, Vec<(String, String)>) = info.body().deserialize().ok()?;
    // (unique name, name, native name, icon, label, language, configurable)
    type Avail = Vec<(String, String, String, String, String, String, bool)>;
    let avail: Avail = call("AvailableInputMethods").await.ok()?.body().deserialize().ok()?;
    let current: String = call("CurrentInputMethod").await.ok()?.body().deserialize().ok()?;
    let ims = items
        .into_iter()
        .map(|(name, _)| {
            let a = avail.iter().find(|a| a.0 == name);
            Im {
                label: a.map_or(name.clone(), |a| a.1.clone()),
                short: a.map(|a| a.4.clone()).filter(|l| !l.is_empty()).unwrap_or_else(|| name.chars().take(2).collect::<String>().to_uppercase()),
                name,
            }
        })
        .collect();
    Some(Fcitx { ims, current })
}

/// fcitx5's input methods, refreshed every couple of seconds while it runs.
pub fn subscription() -> Subscription<InputMsg> {
    Subscription::run_with("lapin-ux-fcitx", |_| {
        stream::channel(4, |mut out: futures::channel::mpsc::Sender<InputMsg>| async move {
            let Ok(conn) = zbus::Connection::session().await else { return futures::future::pending().await };
            let mut last: Option<Fcitx> = None;
            loop {
                let now = read_fcitx(&conn).await;
                if now != last {
                    let _ = out.send(InputMsg::Fcitx(now.clone())).await;
                    last = now;
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        })
    })
}
