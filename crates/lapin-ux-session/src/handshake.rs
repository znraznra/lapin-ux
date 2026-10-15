// SPDX-License-Identifier: GPL-3.0-only
//! Compositor start-up handshake.
//!
//! cosmic-comp reads a file descriptor number from `COSMIC_SESSION_SOCK`.
//! Once its Wayland socket exists it writes one message to that fd:
//! a native-endian `u16` length followed by JSON
//! `{"message":"set_env","variables":{"WAYLAND_DISPLAY":"wayland-1",...}}`.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::HashMap;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::net::UnixStream as StdUnixStream;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::net::UnixStream;
use tokio::process::{Child, Command};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", tag = "message")]
enum Message {
    SetEnv { variables: HashMap<String, String> },
}

/// Read one length-prefixed message from the compositor.
pub async fn read_message<R: tokio::io::AsyncRead + Unpin>(rx: &mut R) -> Result<HashMap<String, String>> {
    let mut len = [0u8; 2];
    rx.read_exact(&mut len).await.context("reading message length")?;
    let len = u16::from_ne_bytes(len) as usize;
    let mut buf = vec![0u8; len];
    rx.read_exact(&mut buf).await.context("reading message body")?;
    match serde_json::from_slice::<Message>(&buf).context("decoding compositor message")? {
        Message::SetEnv { variables } => Ok(variables),
    }
}

pub async fn start_compositor(exec: &str) -> Result<(Child, HashMap<String, String>)> {
    let (ours, theirs) = StdUnixStream::pair().context("creating session socket pair")?;
    let theirs: OwnedFd = theirs.into();
    let fd = theirs.as_raw_fd();

    let mut cmd = Command::new(exec);
    cmd.env("COSMIC_SESSION_SOCK", fd.to_string()).kill_on_drop(true);
    // The socket pair is created close-on-exec; let this one fd through.
    unsafe {
        cmd.pre_exec(move || {
            let fd = std::os::fd::BorrowedFd::borrow_raw(fd);
            rustix::io::fcntl_setfd(fd, rustix::io::FdFlags::empty())?;
            Ok(())
        });
    }
    let child = cmd.spawn().with_context(|| format!("starting {exec}"))?;
    drop(theirs);

    ours.set_nonblocking(true)?;
    let mut ours = UnixStream::from_std(ours)?;
    let env = match tokio::time::timeout(Duration::from_secs(30), read_message(&mut ours)).await {
        Ok(Ok(env)) => env,
        Ok(Err(e)) => return Err(e.context("compositor did not complete the handshake")),
        Err(_) => bail!("compositor did not report its display within 30 s"),
    };
    if !env.contains_key("WAYLAND_DISPLAY") {
        bail!("compositor did not report WAYLAND_DISPLAY");
    }
    // Keep our end open for the compositor's lifetime; it treats EOF as an error.
    std::mem::forget(ours);
    Ok((child, env))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    async fn roundtrip(json: &str) -> Result<HashMap<String, String>> {
        let (mut a, mut b) = UnixStream::pair().unwrap();
        let bytes = json.as_bytes().to_vec();
        tokio::spawn(async move {
            a.write_all(&(bytes.len() as u16).to_ne_bytes()).await.unwrap();
            a.write_all(&bytes).await.unwrap();
        });
        read_message(&mut b).await
    }

    #[tokio::test]
    async fn parses_set_env() {
        let env = roundtrip(r#"{"message":"set_env","variables":{"WAYLAND_DISPLAY":"wayland-1","DISPLAY":":0"}}"#)
            .await
            .unwrap();
        assert_eq!(env["WAYLAND_DISPLAY"], "wayland-1");
        assert_eq!(env["DISPLAY"], ":0");
    }

    #[tokio::test]
    async fn rejects_unknown_message() {
        assert!(roundtrip(r#"{"message":"something_else"}"#).await.is_err());
    }
}
