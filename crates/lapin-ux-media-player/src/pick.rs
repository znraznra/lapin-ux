// SPDX-License-Identifier: GPL-3.0-only
//! File dialogs through the desktop portal. In a Lapin session the portal is
//! Lapin's own file explorer; in another desktop it is that desktop's dialog.

use cosmic::dialog::file_chooser::{self, FileFilter};

/// What the dialog is for. The same dialog is never reused for two jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    File,
    Folder,
    LibraryFolder,
    Playlist,
    Stations,
    SavePlaylist,
    SaveStations,
}

/// `Ok(None)` is "cancelled". `Err` means no dialog could be shown (no portal),
/// or the chosen name cannot be used as text, and the caller falls back to
/// typing a path. A name that is not valid UTF-8 is never rewritten into a
/// different one, which could land an export on another file.
pub type Picked = Result<Option<String>, String>;

/// The portal answers with a file: address. The address type is not re-exported
/// by libcosmic, so this takes it as it comes.
macro_rules! path_of {
    ($url:expr) => {
        match $url.to_file_path() {
            Ok(p) => p.into_os_string().into_string().map(Some).map_err(|_| "the chosen name is not valid text".to_string()),
            Err(_) => Ok(None),
        }
    };
}

/// Matches an extension in either case, since the portal's globs are exact.
fn with_extensions(mut f: FileFilter, exts: &[&str]) -> FileFilter {
    for e in exts {
        // every letter in both cases, as in the portal's own `*.[iI][cC][oO]` example
        let pattern: String = e.chars().map(|c| if c.is_ascii_alphabetic() { format!("[{}{}]", c.to_ascii_lowercase(), c.to_ascii_uppercase()) } else { c.to_string() }).collect();
        f = f.glob(&format!("*.{pattern}"));
    }
    f
}

fn media_filter() -> FileFilter {
    let all: Vec<&str> = crate::library::AUDIO.iter().chain(crate::library::VIDEO).copied().collect();
    with_extensions(FileFilter::new("Music and videos"), &all)
}

fn playlist_filter() -> FileFilter {
    with_extensions(FileFilter::new("Playlists"), &["m3u", "m3u8", "pls"])
}

/// Saving always writes M3U, so PLS is only offered when opening.
fn playlist_save_filter() -> FileFilter {
    with_extensions(FileFilter::new("Playlists"), &["m3u", "m3u8"])
}

fn stations_filter() -> FileFilter {
    with_extensions(FileFilter::new("Stations"), &["json", "m3u", "m3u8"])
}

fn done<T>(r: Result<T, file_chooser::Error>, f: impl FnOnce(T) -> Picked) -> Picked {
    match r {
        Ok(v) => f(v),
        Err(file_chooser::Error::Cancelled) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub async fn pick(job: Job) -> Picked {
    use file_chooser::{open, save};
    match job {
        Job::File => done(open::Dialog::new().title("Open music or video").filter(media_filter()).open_file().await, |r| path_of!(r.url())),
        Job::Folder => done(open::Dialog::new().title("Open a folder").open_folder().await, |r| path_of!(r.url())),
        Job::LibraryFolder => done(open::Dialog::new().title("Add a folder to the library").open_folder().await, |r| path_of!(r.url())),
        Job::Playlist => done(open::Dialog::new().title("Open playlist").filter(playlist_filter()).open_file().await, |r| path_of!(r.url())),
        Job::Stations => done(open::Dialog::new().title("Import stations").filter(stations_filter()).open_file().await, |r| path_of!(r.url())),
        Job::SavePlaylist => done(
            save::Dialog::new().title("Save the queue as a playlist".to_string()).file_name("Queue.m3u".to_string()).filter(playlist_save_filter()).save_file().await,
            |r| r.url().map_or(Ok(None), |u| path_of!(u)),
        ),
        Job::SaveStations => done(
            save::Dialog::new().title("Export stations".to_string()).file_name("Stations.json".to_string()).filter(stations_filter()).save_file().await,
            |r| r.url().map_or(Ok(None), |u| path_of!(u)),
        ),
    }
}
