// SPDX-License-Identifier: GPL-3.0-only
//! The account button in Start: its menu (account settings, change
//! picture, lock, sign out, switch user), changing the account picture
//! through the file chooser portal and AccountsService, and switching
//! user through the display manager.

use std::collections::HashMap;
use std::path::PathBuf;

use cosmic::app::Task;
use cosmic::iced::{Alignment, Length};
use cosmic::widget::{column, container, icon, row, text};
use cosmic::Element;
use futures::StreamExt;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use crate::app::{Message, PopupKind, PowerAction, Shell, rect_key};
use crate::taskbar_view::{menu_divider, menu_item, panel};

#[derive(Debug, Clone)]
pub enum AccountMsg {
    Menu,
    Settings,
    ChangePicture,
    /// The picture was set (shown right away).
    PictureSet(PathBuf),
    SwitchUser,
    /// Whether a display manager that can switch users is running.
    SwitchAvailable(bool),
}

impl Shell {
    pub(crate) fn update_account(&mut self, msg: AccountMsg) -> Task<Message> {
        match msg {
            AccountMsg::Menu => {
                let Some(s) = &self.start else { return Task::none() };
                let parent = s.id;
                let key = rect_key(parent, "account");
                return self.open_popup(parent, &key, PopupKind::Account);
            }
            AccountMsg::Settings => {
                let close = self.close_start();
                return Task::batch([close, self.open_settings("users")]);
            }
            AccountMsg::ChangePicture => {
                let close = self.close_start();
                let pick = Task::future(async {
                    match change_picture().await {
                        Ok(Some(p)) => cosmic::Action::App(Message::Account(AccountMsg::PictureSet(p))),
                        Ok(None) => cosmic::Action::App(Message::Noop),
                        Err(e) => {
                            tracing::warn!("account picture: {e:#}");
                            cosmic::Action::App(Message::Noop)
                        }
                    }
                });
                return Task::batch([close, pick]);
            }
            AccountMsg::PictureSet(p) => self.avatar = Some(p),
            AccountMsg::SwitchUser => {
                let close = self.close_start();
                let job = Task::future(async {
                    if let Err(e) = switch_user().await {
                        tracing::warn!("switch user: {e:#}");
                    }
                    cosmic::Action::App(Message::Noop)
                });
                return Task::batch([close, job]);
            }
            AccountMsg::SwitchAvailable(on) => self.can_switch_user = on,
        }
        Task::none()
    }
}

/// Startup check: can we switch users here?
pub fn check_switch_user() -> Task<Message> {
    Task::future(async { cosmic::Action::App(Message::Account(AccountMsg::SwitchAvailable(switch_user_available().await))) })
}

async fn switch_user_available() -> bool {
    if std::env::var_os("XDG_SEAT_PATH").is_some() || which("dm-tool") {
        return true;
    }
    let Ok(conn) = zbus::Connection::system().await else { return false };
    let Ok(dbus) = zbus::fdo::DBusProxy::new(&conn).await else { return false };
    let Ok(name) = zbus::names::BusName::try_from("org.gnome.DisplayManager") else { return false };
    dbus.name_has_owner(name).await.unwrap_or(false)
}

fn which(bin: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
}

/// LightDM / SDDM (DisplayManager Seat), then GDM, then dm-tool.
async fn switch_user() -> anyhow::Result<()> {
    let conn = zbus::Connection::system().await?;
    if let Some(seat) = std::env::var_os("XDG_SEAT_PATH").and_then(|s| s.into_string().ok()) {
        let r = conn
            .call_method(Some("org.freedesktop.DisplayManager"), seat.as_str(), Some("org.freedesktop.DisplayManager.Seat"), "SwitchToGreeter", &())
            .await;
        if r.is_ok() {
            return Ok(());
        }
    }
    let r = conn
        .call_method(
            Some("org.gnome.DisplayManager"),
            "/org/gnome/DisplayManager/LocalDisplayFactory",
            Some("org.gnome.DisplayManager.LocalDisplayFactory"),
            "CreateTransientDisplay",
            &(),
        )
        .await;
    if r.is_ok() {
        return Ok(());
    }
    let status = tokio::process::Command::new("dm-tool").arg("switch-to-greeter").status().await?;
    anyhow::ensure!(status.success(), "dm-tool failed");
    Ok(())
}

/// Ask for a picture with the file chooser portal, then hand it to
/// AccountsService. None if the user cancelled.
async fn change_picture() -> anyhow::Result<Option<PathBuf>> {
    let session = zbus::Connection::session().await?;
    let token = format!("lapinux{}", std::process::id());
    let sender = session.unique_name().map(|n| n.trim_start_matches(':').replace('.', "_")).unwrap_or_default();
    let request_path = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
    // Listen before asking, so a fast answer isn't missed.
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.portal.Request")?
        .member("Response")?
        .path(request_path.as_str())?
        .build();
    let mut responses = zbus::MessageStream::for_match_rule(rule, &session, Some(4)).await?;

    let filter: (String, Vec<(u32, String)>) = (
        "Images".into(),
        ["image/png", "image/jpeg", "image/webp", "image/gif", "image/bmp"].iter().map(|m| (1u32, m.to_string())).collect(),
    );
    let mut options: HashMap<&str, Value<'_>> = HashMap::new();
    options.insert("handle_token", Value::from(token.as_str()));
    options.insert("modal", Value::from(true));
    options.insert("filters", Value::from(vec![filter]));
    let _: OwnedObjectPath = session
        .call_method(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            Some("org.freedesktop.portal.FileChooser"),
            "OpenFile",
            &("", "Choose a picture", options),
        )
        .await?
        .body()
        .deserialize()?;

    let Some(Ok(msg)) = responses.next().await else { return Ok(None) };
    let (code, results): (u32, HashMap<String, OwnedValue>) = msg.body().deserialize()?;
    if code != 0 {
        return Ok(None);
    }
    let uris: Vec<String> = results.get("uris").and_then(|v| Vec::<String>::try_from(v.clone()).ok()).unwrap_or_default();
    let Some(path) = uris.first().and_then(|u| lapin_ux_core::recent::file_uri_to_path(u)) else { return Ok(None) };

    let system = zbus::Connection::system().await?;
    let uid = {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata("/proc/self")?.uid() as i64
    };
    let user: OwnedObjectPath = system
        .call_method(Some("org.freedesktop.Accounts"), "/org/freedesktop/Accounts", Some("org.freedesktop.Accounts"), "FindUserById", &(uid,))
        .await?
        .body()
        .deserialize()?;
    system
        .call_method(Some("org.freedesktop.Accounts"), user.as_str(), Some("org.freedesktop.Accounts.User"), "SetIconFile", &(path.to_string_lossy().as_ref(),))
        .await?;
    Ok(Some(path))
}

/// The account picture: one just chosen, else AccountsService's copy.
pub fn avatar(shell: &Shell, size: u16) -> Element<'static, Message> {
    let path = shell.avatar.clone().or_else(|| crate::start_view::account_avatar());
    match path {
        Some(p) => icon(icon::from_path(p)).size(size).into(),
        None => icon::from_name("avatar-default-symbolic").size(size * 5 / 7).into(),
    }
}

/// The account button's menu.
pub fn menu(shell: &Shell) -> Element<'_, Message> {
    let header = container(
        row::with_children(vec![
            avatar(shell, 40),
            text::heading(crate::start_view::account_name()).into(),
        ])
        .spacing(12)
        .align_y(Alignment::Center),
    )
    .padding([8, 12])
    .width(Length::Fill);
    let mut items: Vec<Element<'_, Message>> = vec![
        header.into(),
        menu_item("system-users-symbolic", "Change account settings", Message::Account(AccountMsg::Settings)),
        menu_item("camera-photo-symbolic", "Change account picture", Message::Account(AccountMsg::ChangePicture)),
        menu_divider(),
        menu_item("system-lock-screen-symbolic", "Lock", Message::Power(PowerAction::Lock)),
        menu_item("system-log-out-symbolic", "Sign out", Message::Power(PowerAction::SignOut)),
    ];
    if shell.can_switch_user {
        items.push(menu_item("system-switch-user-symbolic", "Switch user", Message::Account(AccountMsg::SwitchUser)));
    }
    panel(column::with_children(items).spacing(2), 280.0)
}
