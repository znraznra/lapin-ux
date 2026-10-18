// SPDX-License-Identifier: GPL-3.0-only
//! Listens for `com.canonical.Unity.LauncherEntry` `Update` signals: badge
//! counts, progress and "needs attention" for taskbar buttons.

use std::collections::HashMap;

use cosmic::iced::{self, Subscription};
use futures::{SinkExt, StreamExt};
use lapin_ux_core::badge::Prop;
use zbus::zvariant::{OwnedValue, Value};

/// (app id, properties that changed)
pub type Update = (String, Vec<Prop>);

pub fn subscription() -> Subscription<Update> {
    Subscription::run_with("lapin-ux-launcher-entry", |_| {
        iced::stream::channel(16, |mut tx: futures::channel::mpsc::Sender<Update>| async move {
            let Ok(conn) = zbus::Connection::session().await else { return futures::future::pending().await };
            let rule = zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .interface("com.canonical.Unity.LauncherEntry")
                .and_then(|b| b.member("Update"))
                .map(|b| b.build());
            let Ok(rule) = rule else { return futures::future::pending().await };
            let Ok(mut stream) = zbus::MessageStream::for_match_rule(rule, &conn, Some(64)).await else {
                tracing::warn!("launcher entry: can't listen");
                return futures::future::pending().await;
            };
            while let Some(Ok(msg)) = stream.next().await {
                let Ok((uri, props)) = msg.body().deserialize::<(String, HashMap<String, OwnedValue>)>() else { continue };
                let Some(app) = lapin_ux_core::badge::app_id(&uri) else { continue };
                let props: Vec<Prop> = props.iter().filter_map(|(k, v)| prop(k, v)).collect();
                if !props.is_empty() {
                    let _ = tx.send((app, props)).await;
                }
            }
            futures::future::pending().await
        })
    })
}

fn prop(key: &str, v: &Value<'_>) -> Option<Prop> {
    let int = || match v {
        Value::I64(x) => Some(*x),
        Value::I32(x) => Some(*x as i64),
        Value::U32(x) => Some(*x as i64),
        Value::U64(x) => Some(*x as i64),
        _ => None,
    };
    let bool = || match v {
        Value::Bool(b) => Some(*b),
        _ => None,
    };
    match key {
        "count" => int().map(Prop::Count),
        "count-visible" => bool().map(Prop::CountVisible),
        "progress" => match v {
            Value::F64(x) => Some(Prop::Progress(*x)),
            _ => None,
        },
        "progress-visible" => bool().map(Prop::ProgressVisible),
        "urgent" => bool().map(Prop::Urgent),
        _ => None,
    }
}
