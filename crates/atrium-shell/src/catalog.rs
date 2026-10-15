// SPDX-License-Identifier: GPL-3.0-only
//! Installed applications: loading .desktop files, matching windows to
//! them, and launching.

use std::collections::HashMap;

use atrium_core::search::{self, Searchable};
use cosmic::desktop::{
    self, DesktopEntryCache, DesktopEntryData, DesktopLookupContext, DesktopResolveOptions, fde,
};

pub struct App {
    pub data: DesktopEntryData,
    pub keywords: Vec<String>,
    pub generic_name: Option<String>,
}

pub struct Catalog {
    locales: Vec<String>,
    apps: Vec<App>,
    by_id: HashMap<String, usize>,
    cache: DesktopEntryCache,
    /// app_id reported by a window -> resolved entry, so lookup runs once per app.
    resolved: HashMap<String, DesktopEntryData>,
}

impl Catalog {
    pub fn load() -> Self {
        let locales = fde::get_languages_from_env();
        let desktops = current_desktops();
        let mut apps: Vec<App> = fde::Iter::new(fde::default_paths())
            .filter_map(|p| fde::DesktopEntry::from_path(p, Some(&locales)).ok())
            .filter(|de| {
                !de.hidden()
                    && !de.no_display()
                    && de.exec().is_some()
                    && de.only_show_in().is_none_or(|l| desktops.iter().any(|d| l.contains(&d.as_str())))
                    && de.not_show_in().is_none_or(|l| !desktops.iter().any(|d| l.contains(&d.as_str())))
            })
            .map(|de| {
                let keywords = de
                    .keywords(&locales)
                    .map(|k| k.into_iter().map(|s| s.to_string()).collect())
                    .unwrap_or_default();
                let generic_name =
                    de.generic_name(&locales).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
                App { data: DesktopEntryData::from_desktop_entry(&locales, de), keywords, generic_name }
            })
            .collect();

        // Earlier XDG_DATA_DIRS entries (user's own) win over system ones.
        let mut seen = std::collections::HashSet::new();
        apps.retain(|a| seen.insert(a.data.id.clone()));
        apps.sort_by_key(|a| search::normalize(&a.data.name));

        let by_id = apps.iter().enumerate().map(|(i, a)| (a.data.id.clone(), i)).collect();
        let cache = DesktopEntryCache::new(locales.clone());
        tracing::info!(count = apps.len(), "loaded applications");
        Self { locales, apps, by_id, cache, resolved: HashMap::new() }
    }

    pub fn apps(&self) -> &[App] {
        &self.apps
    }

    pub fn get(&self, id: &str) -> Option<&App> {
        self.by_id.get(id).map(|&i| &self.apps[i])
    }

    pub fn search(&self, query: &str) -> Vec<&App> {
        search::rank(query, &self.apps, |a| Searchable {
            name: &a.data.name,
            generic_name: a.generic_name.as_deref(),
            keywords: &a.keywords,
            exec: a.data.exec.as_deref(),
            id: &a.data.id,
        })
    }

    /// Desktop-entry id for a window, so windows group under the pinned
    /// button of the same app. Falls back to the raw app_id.
    pub fn resolve_window(&mut self, app_id: &str, title: &str) -> DesktopEntryData {
        if let Some(d) = self.resolved.get(app_id) {
            return d.clone();
        }
        if let Some(app) = self.get(app_id) {
            let d = app.data.clone();
            self.resolved.insert(app_id.to_string(), d.clone());
            return d;
        }
        let ctx = DesktopLookupContext::new(app_id).with_title(title);
        let entry = desktop::resolve_desktop_entry(&mut self.cache, &ctx, &DesktopResolveOptions::default());
        let d = DesktopEntryData::from_desktop_entry(&self.locales, entry);
        // Only memoise real matches; a window's title may help later.
        if d.path.as_ref().is_some_and(|p| p.exists()) {
            self.resolved.insert(app_id.to_string(), d.clone());
        }
        d
    }

    /// Entry data for an id that may be a pinned app or a running window.
    pub fn data_for(&self, id: &str) -> Option<&DesktopEntryData> {
        self.get(id).map(|a| &a.data).or_else(|| self.resolved.values().find(|d| d.id == id))
    }
}

/// `XDG_CURRENT_DESKTOP` is a colon-separated list, e.g. `Atrium:COSMIC`.
pub fn current_desktops() -> Vec<String> {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Launch an Exec= line. `token` is an xdg-activation token so the new
/// window comes to the front.
pub async fn launch(exec: String, app_id: String, terminal: bool, token: Option<String>) {
    let mut env: Vec<(&str, String)> = Vec::new();
    if let Some(t) = token {
        env.push(("XDG_ACTIVATION_TOKEN", t.clone()));
        env.push(("DESKTOP_STARTUP_ID", t));
    }
    desktop::spawn_desktop_exec(exec, env, Some(&app_id), terminal).await;
}
