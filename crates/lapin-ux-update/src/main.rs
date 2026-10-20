// SPDX-License-Identifier: GPL-3.0-only
//! Lapin UX Update: like Windows Update. Checks for system (pacman) and
//! Flatpak updates, and device firmware through fwupd, installs them all with one button (asking for your
//! password through polkit), shows progress and update history, and offers
//! a restart when the kernel or other core parts changed.

use cosmic::app::{Settings, Task};
use cosmic::iced::{Alignment, Length, Size, stream};
use cosmic::widget::{self, button, column, container, icon, progress_bar, row, scrollable, text};
use cosmic::{ApplicationExt, Core, Element, theme};
use lapin_ux_core::updates::{self, Update};
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Debug, Clone, PartialEq)]
enum State {
    Checking,
    UpToDate,
    Available,
    Installing,
    /// Finished: whether a restart is needed, or what went wrong.
    Done(bool),
    Failed(String),
}

#[derive(Debug, Clone)]
enum Message {
    Check,
    Checked(Vec<Update>, Option<String>),
    Install,
    Line(String),
    Installed(Result<(), String>),
    Restart,
    History(bool),
    ShowLog(bool),
}

struct App {
    core: Core,
    state: State,
    updates: Vec<Update>,
    checked_at: Option<String>,
    log: Vec<String>,
    show_log: bool,
    history: Option<Vec<String>>,
}

fn now_text() -> String {
    // Local time from `date`, avoiding a time-zone crate for one label.
    std::process::Command::new("date")
        .arg("+%-I:%M %p")
        .output()
        .ok()
        .map(|o| format!("Today, {}", String::from_utf8_lossy(&o.stdout).trim()))
        .unwrap_or_default()
}

async fn check() -> (Vec<Update>, Option<String>) {
    let mut list = Vec::new();
    let mut problem = None;
    match tokio::process::Command::new("checkupdates").output().await {
        // checkupdates exits 2 when there's nothing to update.
        Ok(o) if o.status.success() || o.status.code() == Some(2) => list.extend(updates::parse_checkupdates(&String::from_utf8_lossy(&o.stdout))),
        Ok(o) => problem = Some(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(_) => problem = Some("checkupdates isn't installed (pacman-contrib).".into()),
    }
    if let Ok(o) = tokio::process::Command::new("flatpak").args(["remote-ls", "--updates", "--columns=application,version"]).output().await
        && o.status.success()
    {
        list.extend(updates::parse_flatpak(&String::from_utf8_lossy(&o.stdout)));
    }
    // Firmware (fwupd). It exits 2 when there's nothing to do; any other
    // trouble (no fwupd, no supported devices) just means no firmware rows.
    if let Ok(o) = tokio::process::Command::new("fwupdmgr")
        .args(["get-updates", "--json", "--no-unreported-check", "--no-metadata-check"])
        .output()
        .await
    {
        list.extend(updates::parse_fwupd(&String::from_utf8_lossy(&o.stdout)));
    }
    (list, problem)
}

/// Run one command, sending each output line.
async fn run_streamed(out: &mut futures::channel::mpsc::Sender<Message>, cmd: &str, args: &[&str]) -> Result<(), String> {
    use futures::SinkExt;
    let mut child = tokio::process::Command::new(cmd)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("{cmd}: {e}"))?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let mut lines = BufReader::new(stdout).lines();
    let mut errs = BufReader::new(stderr).lines();
    let mut last_err = String::new();
    loop {
        tokio::select! {
            l = lines.next_line() => match l {
                Ok(Some(l)) => { let _ = out.send(Message::Line(l)).await; }
                _ => break,
            },
            e = errs.next_line() => if let Ok(Some(e)) = e {
                last_err = e.clone();
                let _ = out.send(Message::Line(e)).await;
            },
        }
    }
    let status = child.wait().await.map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else if status.code() == Some(126) || status.code() == Some(127) {
        Err("The update wasn't authorised.".into())
    } else {
        Err(if last_err.is_empty() { format!("{cmd} failed") } else { last_err })
    }
}

fn install(system: bool, flatpak: bool, firmware: bool) -> Task<Message> {
    let s = stream::channel(64, move |mut out: futures::channel::mpsc::Sender<Message>| async move {
        use futures::SinkExt;
        let mut result = Ok(());
        if system {
            result = run_streamed(&mut out, "pkexec", &["pacman", "-Syu", "--noconfirm"]).await;
        }
        if result.is_ok() && flatpak {
            result = run_streamed(&mut out, "flatpak", &["update", "-y", "--noninteractive"]).await;
        }
        if result.is_ok() && firmware {
            // fwupd asks polkit itself, which shows the admin prompt; the
            // firmware is applied on the next restart.
            result = run_streamed(
                &mut out,
                "fwupdmgr",
                &["update", "-y", "--no-reboot-check", "--no-unreported-check", "--no-metadata-check"],
            )
            .await;
        }
        let _ = out.send(Message::Installed(result)).await;
    });
    Task::run(s, cosmic::Action::App)
}

fn history() -> Vec<String> {
    let log = std::fs::read_to_string("/var/log/pacman.log").unwrap_or_default();
    let mut v: Vec<String> = log
        .lines()
        .filter(|l| l.contains("[ALPM] upgraded ") || l.contains("[ALPM] installed "))
        .map(|l| {
            let date = l.get(1..11).unwrap_or_default();
            let what = l.split("[ALPM] ").nth(1).unwrap_or(l);
            format!("{date}  {what}")
        })
        .collect();
    v.reverse();
    v.truncate(200);
    v
}

fn card<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content).padding(16).width(Length::Fill).class(theme::Container::Card).into()
}

impl cosmic::Application for App {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = "dev.lapinux.Update";

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _: ()) -> (Self, Task<Message>) {
        let mut app = App { core, state: State::Checking, updates: Vec::new(), checked_at: None, log: Vec::new(), show_log: false, history: None };
        let title = match app.core.main_window_id() {
            Some(id) => app.set_window_title("Update".into(), id),
            None => Task::none(),
        };
        (app, Task::batch([title, Task::done(cosmic::Action::App(Message::Check))]))
    }

    fn header_start(&self) -> Vec<Element<'_, Message>> {
        vec![lapin_ux_ui::title_bar("system-software-update", "Update")]
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Check => {
                self.state = State::Checking;
                return Task::future(async {
                    let (list, problem) = check().await;
                    cosmic::Action::App(Message::Checked(list, problem))
                });
            }
            Message::Checked(list, problem) => {
                self.checked_at = Some(now_text());
                self.updates = list;
                self.state = match problem {
                    Some(p) if self.updates.is_empty() => State::Failed(p),
                    _ if self.updates.is_empty() => State::UpToDate,
                    _ => State::Available,
                };
            }
            Message::Install => {
                self.state = State::Installing;
                self.log.clear();
                let system = self.updates.iter().any(|u| !u.flatpak && !u.firmware);
                let flatpak = self.updates.iter().any(|u| u.flatpak);
                let firmware = self.updates.iter().any(|u| u.firmware);
                return install(system, flatpak, firmware);
            }
            Message::Line(l) => {
                self.log.push(l);
                if self.log.len() > 2000 {
                    self.log.drain(..500);
                }
            }
            Message::Installed(r) => {
                self.state = match r {
                    Ok(()) => State::Done(updates::needs_restart(&self.updates)),
                    Err(e) => State::Failed(e),
                };
                if matches!(self.state, State::Done(_)) {
                    self.updates.clear();
                }
            }
            Message::Restart => {
                let _ = std::process::Command::new("systemctl").arg("reboot").spawn();
            }
            Message::History(on) => self.history = on.then(history),
            Message::ShowLog(on) => self.show_log = on,
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let (glyph, title, sub): (&str, String, String) = match &self.state {
            State::Checking => ("view-refresh-symbolic", "Checking for updates…".into(), String::new()),
            State::UpToDate => ("emblem-ok-symbolic", "You're up to date".into(), self.checked_at.as_ref().map(|t| format!("Last checked: {t}")).unwrap_or_default()),
            State::Available => ("software-update-available-symbolic", "Updates available".into(), format!("{} updates are ready to install", self.updates.len())),
            State::Installing => ("software-update-available-symbolic", "Installing updates".into(), self.log.last().cloned().unwrap_or_else(|| "Getting things ready…".into())),
            State::Done(true) => ("system-reboot-symbolic", "Restart required".into(), "Restart your PC to finish installing updates.".into()),
            State::Done(false) => ("emblem-ok-symbolic", "You're up to date".into(), "Updates were installed.".into()),
            State::Failed(e) => ("dialog-warning-symbolic", "Updates couldn't be installed".into(), e.clone()),
        };
        let action: Element<'_, Message> = match &self.state {
            State::Checking | State::Installing => widget::Space::new().width(0).into(),
            State::Available => lapin_ux_ui::button::suggested("Download & install all").on_press(Message::Install).into(),
            State::Done(true) => lapin_ux_ui::button::suggested("Restart now").on_press(Message::Restart).into(),
            _ => lapin_ux_ui::button::suggested("Check for updates").on_press(Message::Check).into(),
        };
        let hero = row::with_children(vec![
            container(icon::from_name(glyph).size(40)).padding(8).into(),
            column::with_children(vec![text::title3(title).into(), text::body(sub).into()]).spacing(4).width(Length::Fill).into(),
            action,
        ])
        .spacing(16)
        .align_y(Alignment::Center);

        let mut page = column::with_capacity(8).spacing(16).push(text::title2("Update")).push(card(hero));
        if matches!(self.state, State::Installing | State::Checking) {
            page = page.push(progress_bar::indeterminate_linear().width(Length::Fill).girth(4));
        }
        if matches!(self.state, State::Available | State::Installing) && !self.updates.is_empty() {
            let mut list = column::with_capacity(self.updates.len()).spacing(8);
            for u in &self.updates {
                let version = if u.from.is_empty() { u.to.clone() } else { format!("{} → {}", u.from, u.to) };
                list = list.push(
                    row::with_children(vec![
                        icon::from_name(if u.firmware {
                            "firmware-symbolic"
                        } else if u.flatpak {
                            "application-x-executable-symbolic"
                        } else {
                            "package-x-generic-symbolic"
                        })
                        .fallback(Some(cosmic::widget::icon::IconFallback::Names(vec!["drive-harddisk-system-symbolic".into()])))
                        .size(16)
                        .into(),
                        text::body(u.name.clone()).width(Length::Fill).into(),
                        text::caption(version).into(),
                        text::caption(if u.firmware { "Firmware" } else if u.flatpak { "App" } else { "System" }).width(64).into(),
                    ])
                    .spacing(12)
                    .align_y(Alignment::Center),
                );
            }
            page = page.push(card(list));
        }
        if !self.log.is_empty() {
            page = page.push(lapin_ux_ui::button::text(if self.show_log { "Hide details" } else { "Show details" }).on_press(Message::ShowLog(!self.show_log)));
            if self.show_log {
                let lines: Vec<Element<'_, Message>> = self.log.iter().rev().take(200).rev().map(|l| text::caption(l.clone()).into()).collect();
                page = page.push(card(column::with_children(lines).spacing(2)));
            }
        }
        // More options.
        page = page.push(text::heading("More options"));
        page = page.push(
            button::custom(
                row::with_children(vec![
                    icon::from_name("document-open-recent-symbolic").size(20).into(),
                    column::with_children(vec![text::body("Update history").into(), text::caption("What was updated and when").into()]).width(Length::Fill).into(),
                    icon::from_name(if self.history.is_some() { "pan-down-symbolic" } else { "pan-end-symbolic" }).size(16).into(),
                ])
                .spacing(16)
                .align_y(Alignment::Center),
            )
            .padding(16)
            .width(Length::Fill)
            .class(theme::Button::MenuItem)
            .on_press(Message::History(self.history.is_none())),
        );
        if let Some(h) = &self.history {
            let lines: Vec<Element<'_, Message>> = if h.is_empty() {
                vec![text::body("No updates yet.").into()]
            } else {
                h.iter().map(|l| text::caption(l.clone()).into()).collect()
            };
            page = page.push(card(column::with_children(lines).spacing(2)));
        }
        scrollable(container(page).padding([16, 32]).max_width(900).width(Length::Fill)).height(Length::Fill).into()
    }
}

/// `--check`: look for updates without a window; if there are any, say
/// so in a notification and open Update when it's clicked.
async fn check_and_notify() {
    use futures::StreamExt;
    let (list, _) = check().await;
    if list.is_empty() {
        return;
    }
    let Ok(conn) = zbus::Connection::session().await else { return };
    let mut hints: std::collections::HashMap<&str, zbus::zvariant::Value> = std::collections::HashMap::new();
    hints.insert("desktop-entry", "dev.lapinux.Update".into());
    let body = if list.len() == 1 { "1 update is ready to install.".to_string() } else { format!("{} updates are ready to install.", list.len()) };
    // Listen before sending so the click can't be missed.
    let Ok(proxy) = zbus::Proxy::new(&conn, "org.freedesktop.Notifications", "/org/freedesktop/Notifications", "org.freedesktop.Notifications").await else { return };
    let Ok(mut signals) = proxy.receive_all_signals().await else { return };
    let reply = conn
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &("Update", 0u32, "software-update-available-symbolic", "Updates are available", body.as_str(), vec!["default", "Open Update", "open", "Open Update"], hints, -1i32),
        )
        .await;
    let Ok(id) = reply.and_then(|r| r.body().deserialize::<u32>()) else { return };
    let deadline = tokio::time::sleep(std::time::Duration::from_secs(6 * 3600));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => return,
            s = signals.next() => {
                let Some(msg) = s else { return };
                let header = msg.header();
                let member = header.member().map(|m| m.to_string()).unwrap_or_default();
                if member == "ActionInvoked" && let Ok((nid, _key)) = msg.body().deserialize::<(u32, String)>() && nid == id {
                    let me = std::env::current_exe().unwrap_or_else(|_| "lapin-ux-update".into());
                    let _ = std::process::Command::new(me).spawn();
                    return;
                }
                if member == "NotificationClosed" && let Ok((nid, _)) = msg.body().deserialize::<(u32, u32)>() && nid == id {
                    // Kept in the notification centre: stay to open it later.
                    continue;
                }
            }
        }
    }
}

fn main() -> cosmic::iced::Result {
    if std::env::args().any(|a| a == "--check") {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime");
        rt.block_on(check_and_notify());
        return Ok(());
    }
    cosmic::app::run::<App>(Settings::default().size(Size::new(860.0, 640.0)), ())
}
