// SPDX-License-Identifier: GPL-3.0-only
// Adapted from cosmic-applets/cosmic-applet-status-area (Copyright 2023 System76, GPL-3.0-only).
//! One tray icon (`org.kde.StatusNotifierItem`) and its menu
//! (`com.canonical.dbusmenu`).

use std::collections::HashMap;
use std::hash::Hash;
use std::path::PathBuf;

use cosmic::iced::Subscription;
use futures::{FutureExt, StreamExt};
use zbus::zvariant::{self, OwnedValue};

#[derive(Clone, Debug)]
pub struct TrayItem {
    /// Bus name (and optional object path) the item registered with.
    name: String,
    is_menu: bool,
    item: StatusNotifierItemProxy<'static>,
    menu: Option<DBusMenuProxy<'static>>,
}

#[derive(Clone, Debug, zvariant::Value)]
pub struct Pixmap {
    pub width: i32,
    pub height: i32,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct IconUpdate {
    pub name: Option<String>,
    pub pixmap: Option<Vec<Pixmap>>,
    pub theme_path: Option<PathBuf>,
    pub title: Option<String>,
    /// The app's stable tray id (e.g. "discord"); bus names change every run.
    pub id: Option<String>,
}

impl TrayItem {
    pub async fn new(conn: &zbus::Connection, name: String) -> zbus::Result<Self> {
        let (dest, path) = match name.find('/') {
            Some(i) => (&name[..i], &name[i..]),
            None => (name.as_str(), "/StatusNotifierItem"),
        };
        let item = StatusNotifierItemProxy::builder(conn)
            // Many items don't emit PropertiesChanged; always read fresh.
            .cache_properties(zbus::proxy::CacheProperties::No)
            .destination(dest.to_string())?
            .path(path.to_string())?
            .build()
            .await?;
        let is_menu = item.item_is_menu().await.unwrap_or(false);
        let menu = match item.menu().await {
            Ok(p) => Some(DBusMenuProxy::builder(conn).destination(dest.to_string())?.path(p)?.build().await?),
            Err(_) => None,
        };
        Ok(Self { name, is_menu, item, menu })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The item is only a menu; it has no primary "Activate" action.
    pub fn is_menu(&self) -> bool {
        self.is_menu
    }

    pub fn has_menu(&self) -> bool {
        self.menu.is_some()
    }

    pub fn proxy(&self) -> &StatusNotifierItemProxy<'static> {
        &self.item
    }

    pub fn menu_proxy(&self) -> Option<&DBusMenuProxy<'static>> {
        self.menu.as_ref()
    }

    pub fn icon_subscription(&self) -> Subscription<IconUpdate> {
        async fn fetch(p: StatusNotifierItemProxy<'static>) -> IconUpdate {
            IconUpdate {
                name: p.icon_name().await.ok().filter(|n| !n.is_empty()),
                pixmap: p.icon_pixmap().await.ok(),
                theme_path: p.icon_theme_path().await.ok().filter(|s| !s.is_empty()).map(PathBuf::from),
                title: p.title().await.ok().filter(|t| !t.is_empty()),
                id: p.id().await.ok().filter(|t| !t.is_empty()),
            }
        }
        struct Key(StatusNotifierItemProxy<'static>, String);
        impl Hash for Key {
            fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                self.1.hash(state);
            }
        }
        Subscription::run_with(Key(self.item.clone(), format!("tray-icon-{}", self.name)), |Key(p, _)| {
            let p = p.clone();
            async move {
                let new_icon = match p.receive_new_icon().await {
                    Ok(s) => s.map(|_| ()).boxed(),
                    Err(_) => futures::stream::empty().boxed(),
                };
                futures::stream::once(async {}).chain(new_icon).then(move |()| fetch(p.clone()))
            }
            .flatten_stream()
        })
    }

    pub fn layout_subscription(&self) -> Subscription<Result<Layout, String>> {
        let Some(menu) = self.menu.clone() else { return Subscription::none() };
        struct Key(DBusMenuProxy<'static>, String);
        impl Hash for Key {
            fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                self.1.hash(state);
            }
        }
        Subscription::run_with(Key(menu, format!("tray-menu-{}", self.name)), |Key(m, _)| {
            let m = m.clone();
            async move {
                let initial = futures::stream::once(get_layout(m.clone()));
                let updates = match (m.receive_layout_updated().await, m.receive_items_properties_updated().await) {
                    (Ok(a), Ok(b)) => futures::stream::select(a.map(|_| ()), b.map(|_| ())).boxed(),
                    _ => futures::stream::empty().boxed(),
                };
                initial.chain(updates.then(move |()| get_layout(m.clone())))
            }
            .flatten_stream()
        })
    }
}

async fn get_layout(menu: DBusMenuProxy<'static>) -> Result<Layout, String> {
    menu.get_layout(0, -1, &[]).await.map(|(_, l)| l).map_err(|e| e.to_string())
}

#[zbus::proxy(interface = "org.kde.StatusNotifierItem")]
pub trait StatusNotifierItem {
    #[zbus(property)]
    fn icon_name(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn icon_theme_path(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn icon_pixmap(&self) -> zbus::Result<Vec<Pixmap>>;
    #[zbus(property)]
    fn title(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn id(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn menu(&self) -> zbus::Result<zvariant::OwnedObjectPath>;
    #[zbus(property)]
    fn item_is_menu(&self) -> zbus::Result<bool>;
    #[zbus(signal)]
    fn new_icon(&self) -> zbus::Result<()>;
    fn activate(&self, x: i32, y: i32) -> zbus::Result<()>;
    fn secondary_activate(&self, x: i32, y: i32) -> zbus::Result<()>;
    fn scroll(&self, delta: i32, orientation: &str) -> zbus::Result<()>;
}

/// A dbusmenu node: id, properties, children.
#[derive(Clone, Debug)]
pub struct Layout(i32, LayoutProps, Vec<Layout>);

impl<'a> serde::Deserialize<'a> for Layout {
    fn deserialize<D: serde::Deserializer<'a>>(d: D) -> Result<Self, D::Error> {
        let (id, props, children) = <(i32, LayoutProps, Vec<(zvariant::Signature, Self)>)>::deserialize(d)?;
        Ok(Self(id, props, children.into_iter().map(|c| c.1).collect()))
    }
}

impl zvariant::Type for Layout {
    const SIGNATURE: &zvariant::Signature =
        <(i32, HashMap<String, zvariant::Value>, Vec<zvariant::Value>)>::SIGNATURE;
}

#[derive(Clone, Debug, zvariant::DeserializeDict)]
pub struct LayoutProps {
    #[zvariant(rename = "children-display")]
    children_display: Option<String>,
    label: Option<String>,
    enabled: Option<bool>,
    visible: Option<bool>,
    #[zvariant(rename = "type")]
    type_: Option<String>,
    #[zvariant(rename = "toggle-type")]
    toggle_type: Option<String>,
    #[zvariant(rename = "toggle-state")]
    toggle_state: Option<i32>,
    #[zvariant(rename = "icon-name")]
    icon_name: Option<String>,
}

impl zvariant::Type for LayoutProps {
    const SIGNATURE: &zvariant::Signature = <HashMap<String, zvariant::Value>>::SIGNATURE;
}

impl Layout {
    pub fn id(&self) -> i32 {
        self.0
    }
    pub fn children(&self) -> &[Self] {
        &self.2
    }
    /// Label with GTK-style mnemonic underscores removed ("_Quit" -> "Quit", "__" -> "_").
    pub fn label(&self) -> String {
        let raw = self.1.label.as_deref().unwrap_or("");
        let mut out = String::with_capacity(raw.len());
        let mut chars = raw.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '_' {
                if chars.peek() == Some(&'_') {
                    out.push('_');
                    chars.next();
                }
                continue;
            }
            out.push(c);
        }
        out
    }
    pub fn enabled(&self) -> bool {
        self.1.enabled.unwrap_or(true)
    }
    pub fn visible(&self) -> bool {
        self.1.visible.unwrap_or(true)
    }
    pub fn is_separator(&self) -> bool {
        self.1.type_.as_deref() == Some("separator")
    }
    pub fn is_submenu(&self) -> bool {
        self.1.children_display.as_deref() == Some("submenu") || !self.2.is_empty()
    }
    /// Some(true/false) for check and radio items.
    pub fn toggled(&self) -> Option<bool> {
        self.1.toggle_type.as_deref().filter(|t| !t.is_empty())?;
        Some(self.1.toggle_state == Some(1))
    }
    pub fn icon_name(&self) -> Option<&str> {
        self.1.icon_name.as_deref().filter(|s| !s.is_empty())
    }
    /// Follow a path of child ids from this node.
    pub fn find(&self, path: &[i32]) -> Option<&Layout> {
        let mut node = self;
        for id in path {
            node = node.2.iter().find(|c| c.0 == *id)?;
        }
        Some(node)
    }
}

#[zbus::proxy(interface = "com.canonical.dbusmenu")]
pub trait DBusMenu {
    fn get_layout(&self, parent_id: i32, recursion_depth: i32, property_names: &[&str]) -> zbus::Result<(u32, Layout)>;
    fn event(&self, id: i32, event_id: &str, data: &OwnedValue, timestamp: u32) -> zbus::Result<()>;
    fn about_to_show(&self, id: i32) -> zbus::Result<bool>;
    #[zbus(signal)]
    fn layout_updated(&self, revision: u32, parent: i32) -> zbus::Result<()>;
    #[zbus(signal)]
    fn items_properties_updated(
        &self,
        updated_props: Vec<(i32, HashMap<String, OwnedValue>)>,
        removed_props: Vec<(i32, Vec<String>)>,
    ) -> zbus::Result<()>;
}
