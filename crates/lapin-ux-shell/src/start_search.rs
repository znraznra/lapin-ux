// SPDX-License-Identifier: GPL-3.0-only
//! Start search beyond apps: Settings pages, files and folders from the
//! user's libraries, "Search the web", and uninstalling apps.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cosmic::Application;
use cosmic::app::Task;
use lapin_ux_core::files::{self, FileIndex};
use lapin_ux_core::system::{self, SettingsPage, Uninstall};

use crate::app::{Message, PopupKind, Shell, rect_key};

/// One selectable entry in Start search results.
#[derive(Debug, Clone, PartialEq)]
pub enum StartItem {
    App(String),
    Setting(&'static SettingsPage),
    File { path: PathBuf, is_dir: bool },
    Web(String),
    /// A pinned tile (stored index) that isn't a plain app.
    Pin(usize),
}

/// Search results grouped like the benchmark: best match, apps, settings,
/// folders, documents, then the web.
#[derive(Debug, Default)]
pub struct Results {
    pub best: Option<StartItem>,
    pub apps: Vec<StartItem>,
    pub settings: Vec<StartItem>,
    pub folders: Vec<StartItem>,
    pub documents: Vec<StartItem>,
    pub web: Option<StartItem>,
}

impl Results {
    /// Flat order used for keyboard navigation and Enter.
    pub fn flat(&self) -> Vec<StartItem> {
        let mut v = Vec::new();
        v.extend(self.best.clone());
        v.extend(self.apps.iter().cloned());
        v.extend(self.settings.iter().cloned());
        v.extend(self.folders.iter().cloned());
        v.extend(self.documents.iter().cloned());
        v.extend(self.web.clone());
        v
    }
}

/// How long a file index is trusted before Start rebuilds it in the background.
const INDEX_MAX_AGE: Duration = Duration::from_secs(120);

impl Shell {
    pub(crate) fn search(&self, query: &str) -> Results {
        let mut r = Results::default();
        let q = query.trim();
        if q.is_empty() {
            return r;
        }
        let mut apps: Vec<StartItem> = self.catalog.search(q).into_iter().take(8).map(|a| StartItem::App(a.data.id.clone())).collect();
        let mut settings: Vec<StartItem> = system::search_settings(q).into_iter().take(4).map(StartItem::Setting).collect();
        let found = self.files.search(q, 10);
        r.folders = found.iter().filter(|f| f.is_dir).take(3).map(|f| StartItem::File { path: f.path.clone(), is_dir: true }).collect();
        r.documents = found.iter().filter(|f| !f.is_dir).take(5).map(|f| StartItem::File { path: f.path.clone(), is_dir: false }).collect();
        if !self.config.hide_web_search {
            r.web = Some(StartItem::Web(q.to_string()));
        }
        // Best match: an app if any, else a setting, else a file.
        r.best = if !apps.is_empty() {
            Some(apps.remove(0))
        } else if !settings.is_empty() {
            Some(settings.remove(0))
        } else if !r.folders.is_empty() || !r.documents.is_empty() {
            let list = if r.documents.is_empty() { &mut r.folders } else { &mut r.documents };
            Some(list.remove(0))
        } else {
            None
        };
        r.apps = apps;
        r.settings = settings;
        r
    }

    pub(crate) fn activate(&mut self, item: StartItem) -> Task<Message> {
        match item {
            StartItem::App(id) => self.launch(&id),
            StartItem::Setting(p) => {
                let close = self.close_start();
                Task::batch([close, self.open_settings(p.page)])
            }
            StartItem::File { path, .. } => self.update(Message::OpenFile(path)),
            StartItem::Pin(i) => self.update_pins(crate::start_pins::PinMsg::Open(i)),
            StartItem::Web(q) => {
                let template = if self.config.web_search.trim().is_empty() {
                    files::DEFAULT_WEB_SEARCH.to_string()
                } else {
                    self.config.web_search.clone()
                };
                let url = files::web_search_url(&template, &q);
                let close = self.close_start();
                Task::batch([close, open_with_default(url.into())])
            }
        }
    }

    /// Rebuild the file index in the background if it's missing or stale.
    pub(crate) fn refresh_file_index(&mut self) -> Task<Message> {
        if self.indexing || self.files.age().is_some_and(|a| a < INDEX_MAX_AGE) {
            return Task::none();
        }
        self.indexing = true;
        Task::future(async {
            let index = tokio::task::spawn_blocking(|| {
                let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
                let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
                let dirs = files::user_dirs(&home, &config);
                FileIndex::build(&files::default_roots(&home, &dirs), files::Limits::default())
            })
            .await
            .unwrap_or_default();
            tracing::debug!(entries = index.entries.len(), "file index built");
            cosmic::Action::App(Message::FilesIndexed(Arc::new(index)))
        })
    }

    pub(crate) fn uninstall_ask(&mut self, app: String) -> Task<Message> {
        let Some(s) = &self.start else { return Task::none() };
        let parent = s.id;
        let key = rect_key(parent, &app);
        let close = self.close_popup();
        Task::batch([close, self.open_popup(parent, &key, PopupKind::ConfirmUninstall(app))])
    }

    pub(crate) fn uninstall(&mut self, app: String) -> Task<Message> {
        let close = Task::batch([self.close_popup(), self.close_start()]);
        let Some(entry) = self.catalog.get(&app) else { return close };
        let name = entry.data.name.clone();
        let Some(path) = entry.data.path.clone() else { return close };
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let method = system::uninstall_method(&path, &app, &home);
        let job = Task::future(async move {
            let (title, body) = match run_uninstall(method).await {
                Ok(()) => (format!("{name} was uninstalled"), String::new()),
                Err(e) => (format!("Couldn't uninstall {name}"), e),
            };
            notify(&title, &body).await;
            cosmic::Action::App(Message::ReloadApps)
        });
        Task::batch([close, job])
    }
}

/// Whether the Start context menu should offer "Uninstall" for this app.
pub fn can_uninstall(shell: &Shell, app: &str) -> bool {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    shell
        .catalog
        .get(app)
        .and_then(|a| a.data.path.as_deref())
        .is_some_and(|p| system::uninstall_method(p, app, &home) != Uninstall::Unsupported)
}

async fn output(cmd: &[String]) -> Result<String, String> {
    let out = tokio::process::Command::new(&cmd[0])
        .args(&cmd[1..])
        .output()
        .await
        .map_err(|e| format!("{}: {e}", cmd[0]))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        // pkexec exit codes: 126 = dialog dismissed, 127 = not authorized.
        Err(match out.status.code() {
            Some(126) => "Cancelled.".into(),
            Some(127) => "Not authorized.".into(),
            _ if err.is_empty() => format!("{} failed", cmd[0]),
            _ => err.lines().last().unwrap_or_default().to_string(),
        })
    }
}

async fn run_uninstall(method: Uninstall) -> Result<(), String> {
    match method {
        Uninstall::Flatpak { app_id, user } => output(&system::flatpak_remove_command(&app_id, user)).await.map(drop),
        Uninstall::Package { desktop_file } => {
            let owner = output(&["pacman".into(), "-Qqo".into(), desktop_file]).await?;
            let pkg = owner.trim().to_string();
            if pkg.is_empty() {
                return Err("No package owns this app.".into());
            }
            if system::is_protected_package(&pkg) {
                return Err(format!("{pkg} is part of the desktop and can't be removed here."));
            }
            output(&system::pacman_remove_command(&pkg)).await.map(drop)
        }
        Uninstall::Unsupported => Err("This app wasn't installed by a package manager.".into()),
    }
}

/// A plain notification through our own notification daemon (or any other).
pub async fn notify(summary: &str, body: &str) {
    let Ok(conn) = zbus::Connection::session().await else { return };
    let hints: std::collections::HashMap<&str, zbus::zvariant::Value> = std::collections::HashMap::new();
    let _ = conn
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &("Lapin UX", 0u32, "system-software-install-symbolic", summary, body, Vec::<&str>::new(), hints, -1i32),
        )
        .await;
}

pub(crate) fn open_with_default(target: std::ffi::OsString) -> Task<Message> {
    Task::future(async move {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(target);
        if cosmic::process::spawn(c).await.is_none() {
            tracing::warn!("xdg-open failed");
        }
        cosmic::Action::App(Message::Noop)
    })
}

/// Show a file selected in the file manager (FileManager1), or open its folder.
pub fn show_in_folder(path: PathBuf) -> Task<Message> {
    Task::future(async move {
        let uri = format!("file://{}", path.display());
        let shown = async {
            let conn = zbus::Connection::session().await?;
            conn.call_method(
                Some("org.freedesktop.FileManager1"),
                "/org/freedesktop/FileManager1",
                Some("org.freedesktop.FileManager1"),
                "ShowItems",
                &(vec![uri.as_str()], ""),
            )
            .await?;
            zbus::Result::Ok(())
        }
        .await;
        if shown.is_err() {
            let dir = path.parent().map(Path::to_path_buf).unwrap_or(path);
            let mut c = std::process::Command::new("xdg-open");
            c.arg(dir);
            let _ = cosmic::process::spawn(c).await;
        }
        cosmic::Action::App(Message::Noop)
    })
}
