// SPDX-License-Identifier: GPL-3.0-only
//! Lapin Notes: a text and code editor. Everything stays on this computer.

mod buffer;
mod diff;
mod hex;
mod outline;
mod editor;
mod encoding;
mod lang;
mod lineops;
mod print;
mod search;
mod settings;
mod view;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use cosmic::app::{Settings as AppSettings, Task};
use cosmic::iced::keyboard::{key::Named, Event as KeyEvent, Key, Modifiers};
use cosmic::iced::{Alignment, Length, Size, Subscription};
use cosmic::widget::{self, button, column, container, row, text};
use cosmic::{ApplicationExt, Core, Element};
use lapin_ux_ui::MenuItem;
use tracing_subscriber::EnvFilter;

use buffer::{Buffer, Pos};
use editor::{Cfg, Editor, Ev};
use encoding::{Enc, Eol};
use lang::Highlighter;
use lineops::{Case, Indent, SortKind};
use search::Query;
use settings::{SessTab, Session, Settings};
use view::{cell_of, Folds, Rows};

const MAX_BYTES: u64 = 64 * 1024 * 1024;

// ---------------------------------------------------------------- files

#[derive(Debug, Clone)]
struct Loaded {
    path: PathBuf,
    text: String,
    enc: Enc,
    eol: Eol,
    readonly: bool,
    mtime: Option<SystemTime>,
}

/// Local file paths in a `text/uri-list` body (one `file://` URI per line,
/// `#` lines are comments, `%XX` escapes decoded). Other schemes and folders
/// are left out.
fn uri_list_paths(list: &str) -> Vec<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    list.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.strip_prefix("file://"))
        .map(|rest| {
            // Skip an optional host ("file://localhost/...").
            let path = &rest[rest.find('/').unwrap_or(rest.len())..];
            let b = path.as_bytes();
            let mut out = Vec::with_capacity(b.len());
            let mut i = 0;
            while i < b.len() {
                if b[i] == b'%' && i + 2 < b.len() {
                    let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
                    if let Some(v) = hex {
                        out.push(v);
                        i += 3;
                        continue;
                    }
                }
                out.push(b[i]);
                i += 1;
            }
            PathBuf::from(std::ffi::OsString::from_vec(out))
        })
        .filter(|p| p.is_file())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_list_decodes_paths_and_skips_the_rest() {
        let dir = std::env::temp_dir().join(format!("lapin-notes-uri-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("a b%c.txt");
        std::fs::write(&f, "x").unwrap();
        let enc = f.to_str().unwrap().replace('%', "%25").replace(' ', "%20");
        let list = format!("# comment\r\nfile://{enc}\r\nhttps://example.org/x\r\nfile://localhost{enc}\r\nfile://{}\r\n", dir.display());
        let got = uri_list_paths(&list);
        assert_eq!(got, vec![f.clone(), f.clone()]);
        std::fs::remove_dir_all(&dir).ok();
    }
}

fn read_file(path: &Path, force: Option<Enc>) -> Result<Loaded, String> {
    let name = path.display();
    let meta = std::fs::metadata(path).map_err(|e| format!("Could not open {name}: {e}"))?;
    if meta.len() > MAX_BYTES {
        return Err(format!("{name} is too big to open here (over 64 MB)."));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("Could not open {name}: {e}"))?;
    if force.is_none() && encoding::looks_binary(&bytes) {
        return Err(format!("{name} does not look like a text file."));
    }
    let enc = force.unwrap_or_else(|| Enc::detect(&bytes));
    let text = enc.decode(&bytes);
    let eol = Eol::detect(&text);
    let readonly = std::fs::OpenOptions::new().write(true).open(path).is_err();
    Ok(Loaded { path: path.to_path_buf(), text, enc, eol, readonly, mtime: meta.modified().ok() })
}

#[derive(Debug, Clone)]
struct SaveFail {
    msg: String,
    denied: bool,
    bytes: Vec<u8>,
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<Option<SystemTime>, SaveFail> {
    match std::fs::write(path, bytes) {
        Ok(()) => Ok(std::fs::metadata(path).and_then(|m| m.modified()).ok()),
        Err(e) => Err(SaveFail {
            msg: format!("Could not save {}: {e}", path.display()),
            denied: e.kind() == std::io::ErrorKind::PermissionDenied,
            bytes: if e.kind() == std::io::ErrorKind::PermissionDenied { bytes.to_vec() } else { Vec::new() },
        }),
    }
}

/// Writes through the system's administrator prompt (polkit). The text goes
/// through a private temporary file that is removed afterwards.
fn write_file_admin(path: &Path, bytes: &[u8]) -> Result<Option<SystemTime>, SaveFail> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let fail = |m: String| SaveFail { msg: m, denied: false, bytes: Vec::new() };
    let tmp = std::env::temp_dir().join(format!("lapin-notes-{}.tmp", std::process::id()));
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|e| fail(format!("Could not prepare the save: {e}")))?;
    f.write_all(bytes).map_err(|e| fail(format!("Could not prepare the save: {e}")))?;
    drop(f);
    let st = std::process::Command::new("pkexec")
        .args(["sh", "-c", "cat \"$1\" > \"$2\"", "sh"])
        .arg(&tmp)
        .arg(path)
        .status();
    let _ = std::fs::remove_file(&tmp);
    match st {
        Ok(s) if s.success() => Ok(std::fs::metadata(path).and_then(|m| m.modified()).ok()),
        Ok(_) => Err(fail("The administrator save was cancelled or refused.".into())),
        Err(e) => Err(fail(format!("Could not ask for administrator rights: {e}"))),
    }
}

fn eol_key(e: Eol) -> &'static str {
    match e {
        Eol::Lf => "lf",
        Eol::Crlf => "crlf",
        Eol::Cr => "cr",
    }
}

fn eol_from(k: &str) -> Eol {
    match k {
        "crlf" => Eol::Crlf,
        "cr" => Eol::Cr,
        _ => Eol::Lf,
    }
}

fn eol_short(e: Eol) -> &'static str {
    match e {
        Eol::Lf => "LF",
        Eol::Crlf => "CRLF",
        Eol::Cr => "CR",
    }
}

// ---------------------------------------------------------------- tabs

struct Tab {
    id: u64,
    buf: Buffer,
    path: Option<PathBuf>,
    enc: Enc,
    eol: Eol,
    lang: String,
    hl: RefCell<Highlighter>,
    folds: Folds,
    ranges: Vec<(usize, usize)>,
    ranges_rev: Option<u64>,
    foldable: Vec<usize>,
    rows: Rows,
    marks: Vec<(Pos, Pos)>,
    scroll_x: f32,
    scroll_y: f32,
    want: Option<usize>,
    col_anchor: Option<Pos>,
    readonly: bool,
    mtime: Option<SystemTime>,
    indent: Indent,
    untitled: usize,
    /// Extra carets as (anchor, caret); the primary one lives in `buf`.
    extra: Vec<(Pos, Pos)>,
    bookmarks: std::collections::BTreeSet<usize>,
    line_marks: std::collections::BTreeMap<usize, u8>,
}

impl Tab {
    fn new(id: u64, text: &str, path: Option<PathBuf>, enc: Enc, eol: Eol, fallback: Indent, untitled: usize) -> Tab {
        let buf = Buffer::new(text);
        let name = path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let l = lang::detect(&name, buf.line(0));
        let indent = lineops::detect_indent(&buf, fallback);
        let rows = Rows::build(&buf, &Folds::new(), None, indent.width);
        Tab {
            id,
            hl: RefCell::new(Highlighter::new(&l, cosmic::theme::is_dark())),
            buf,
            path,
            enc,
            eol,
            lang: l,
            folds: Folds::new(),
            ranges: Vec::new(),
            ranges_rev: None,
            foldable: Vec::new(),
            rows,
            marks: Vec::new(),
            scroll_x: 0.0,
            scroll_y: 0.0,
            want: None,
            col_anchor: None,
            readonly: false,
            mtime: None,
            indent,
            untitled,
            extra: Vec::new(),
            bookmarks: Default::default(),
            line_marks: Default::default(),
        }
    }

    fn name(&self) -> String {
        match &self.path {
            Some(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string()),
            None => format!("Untitled {}", self.untitled),
        }
    }

    fn set_lang(&mut self, l: &str) {
        self.lang = l.to_string();
        *self.hl.borrow_mut() = Highlighter::new(l, cosmic::theme::is_dark());
    }

    fn bytes(&self) -> (Vec<u8>, bool) {
        let text = self.buf.text().replace('\n', self.eol.as_str());
        self.enc.encode(&text)
    }
}

// ---------------------------------------------------------------- messages

#[derive(Debug, Clone, PartialEq)]
enum Cmd {
    New,
    NewWindow,
    Open,
    Save,
    SaveAs,
    Print,
    SavePdf,
    CloseTab,
    Exit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Indent,
    Outdent,
    Comment,
    Upper,
    Lower,
    Title,
    Dup,
    DelLine,
    MoveUp,
    MoveDown,
    Join,
    SplitLong,
    Sort(u8),
    Dedup,
    RemoveEmpty,
    Trim,
    Find,
    Replace,
    Goto,
    Files,
    FindNext,
    FindPrev,
    Wrap,
    Whitespace,
    Eols,
    LineNumbers,
    HlLine,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    FoldAll,
    UnfoldAll,
    SetEnc(Enc),
    ReopenEnc(Enc),
    ConvertEol(Eol),
    SetLang(String),
    UseSpaces(bool),
    TabWidth(usize),
    AutoClose,
    AutoIndent,
    NextTab,
    PrevTab,
    MoveTabLeft,
    MoveTabRight,
    Reload,
    ToggleReadonly,
    OpenRecent(String),
    ClearRecent,
    About,
    Shortcuts,
    AddNext,
    CaretAbove,
    CaretBelow,
    ClearCarets,
    BmToggle,
    BmNext,
    BmPrev,
    BmClear,
    BmDelete,
    BmCopy,
    Minimap,
}

#[derive(Debug, Clone)]
enum Message {
    Key(Key, Modifiers, Option<String>),
    Ed(Ev),
    Menu(Option<usize>),
    Cmd(Cmd),
    SelectTab(usize),
    CloseTab(usize),
    Picked(Vec<PathBuf>),
    /// Files dropped on the window (a `text/uri-list` body).
    Dropped(Vec<u8>),
    /// The pointer moved onto tab `i` while a tab is being dragged.
    DragOver(usize),
    /// The mouse button went up: the tab drag is over.
    DragEnd,
    Opened(Vec<Result<Loaded, String>>, Option<usize>),
    Reloaded(u64, Result<Loaded, String>),
    SavePicked(u64, Option<PathBuf>, bool),
    /// Printers from `lpstat`, and the default one.
    Printers(Vec<String>, Option<String>),
    PickPrinter(String),
    Copies(i32),
    PrintRun,
    PdfPicked(Option<PathBuf>),
    /// Result of printing or writing a PDF; the string says what happened.
    PrintDone(Result<String, String>),
    Saved { id: u64, path: PathBuf, close: bool, lossy: bool, result: Result<Option<SystemTime>, SaveFail> },
    Pasted(Option<String>),
    Tick,
    FindText(String),
    ReplText(String),
    GotoText(String),
    FilesText(String),
    FilesFilter(String),
    ToggleCase,
    ToggleWord,
    ToggleRegex,
    FindNext,
    FindPrev,
    ReplaceOne,
    ReplaceAll,
    GotoSubmit,
    FilesRun,
    FilesHits(Vec<search::FileHit>),
    OpenHit(usize),
    ClosePanel,
    PromptSave,
    PromptDiscard,
    PromptCancel,
    PromptAdmin,
    CloseWindow,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Panel {
    None,
    Find,
    Replace,
    Goto,
    Files,
    Print,
}

enum Prompt {
    CloseTab(u64),
    Quit,
    Reload(u64),
    Info(String, String),
    Admin { id: u64, path: PathBuf, bytes: Vec<u8>, close: bool },
}

struct App {
    core: Core,
    settings: Settings,
    tabs: Vec<Tab>,
    active: usize,
    /// Index of the tab being dragged along the strip, if any.
    drag_tab: Option<usize>,
    next_id: u64,
    next_untitled: usize,
    menu: Option<usize>,
    panel: Panel,
    q: Query,
    repl: String,
    goto: String,
    printers: Vec<String>,
    printer: Option<String>,
    copies: u32,
    files_text: String,
    files_filter: String,
    hits: Vec<search::FileHit>,
    status: String,
    prompt: Option<Prompt>,
    overwrite: bool,
    vp: (usize, usize, f32),
    lock: Option<std::fs::File>,
    last_title: String,
    last_session: Vec<(u64, u64)>,
    find_id: widget::Id,
    goto_id: widget::Id,
    files_id: widget::Id,
}

fn line_h(font: f32) -> f32 {
    (font * 1.4).round()
}

fn clip_write(s: String) -> Task<Message> {
    cosmic::iced::clipboard::write::<cosmic::Action<Message>>(s)
}

impl App {
    fn indent_default(&self) -> Indent {
        Indent { use_spaces: self.settings.use_spaces, width: self.settings.tab_width }
    }

    fn new_tab(&mut self, text: &str, path: Option<PathBuf>, enc: Enc, eol: Eol) -> usize {
        self.next_id += 1;
        let un = if path.is_none() {
            self.next_untitled += 1;
            self.next_untitled
        } else {
            0
        };
        let t = Tab::new(self.next_id, text, path, enc, eol, self.indent_default(), un);
        self.tabs.push(t);
        self.tabs.len() - 1
    }

    fn tab_index(&self, id: u64) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    fn cfg(&self, t: &Tab) -> Cfg {
        Cfg {
            font_size: self.settings.font_size,
            tab: t.indent.width,
            show_ws: self.settings.show_ws,
            show_eol: self.settings.show_eol,
            wrap: self.settings.wrap,
            line_numbers: self.settings.line_numbers,
            hl_line: self.settings.hl_line,
            eol_text: eol_short(t.eol),
        }
    }

    /// Rebuilds what the active tab shows after anything changed.
    fn refresh(&mut self) {
        let wrap = self.settings.wrap;
        let cols = self.vp.0;
        let t = &mut self.tabs[self.active];
        let rev = t.buf.revision as u64;
        if t.ranges_rev != Some(rev) {
            t.ranges = lineops::fold_ranges(&t.buf);
            t.foldable = t.ranges.iter().map(|r| r.0).collect();
            t.ranges_rev = Some(rev);
            let ranges = &t.ranges;
            t.folds.retain(|s, e| ranges.contains(&(*s, *e)));
        }
        let wrap_cells = if wrap { Some(cols.max(10)) } else { None };
        t.rows = Rows::build(&t.buf, &t.folds, wrap_cells, t.indent.width);
        t.marks.clear();
        if matches!(self.panel, Panel::Find | Panel::Replace) && !self.q.text.is_empty() {
            if let Ok(Some(re)) = self.q.compile() {
                t.marks = search::all_matches(&t.buf, &re, 3000)
                    .into_iter()
                    .map(|m| (Pos::new(m.line, m.start), Pos::new(m.line, m.end)))
                    .collect();
            }
        }
    }

    fn max_scroll(&self, t: &Tab) -> f32 {
        let lh = line_h(self.settings.font_size);
        (t.rows.len().saturating_sub(1)) as f32 * lh
    }

    fn ensure_visible(&mut self) {
        let lh = line_h(self.settings.font_size);
        let (cols, vrows, cw) = self.vp;
        let wrap = self.settings.wrap;
        let t = &mut self.tabs[self.active];
        let row = t.rows.row_of(t.buf.caret, &t.buf);
        let top = (t.scroll_y / lh).floor() as usize;
        if row < top {
            t.scroll_y = row as f32 * lh;
        } else if vrows > 0 && row >= top + vrows {
            t.scroll_y = (row + 1 - vrows) as f32 * lh;
        }
        if wrap {
            t.scroll_x = 0.0;
        } else {
            let cell = cell_of(t.buf.line(t.buf.caret.line), t.buf.caret.col, t.indent.width) as f32;
            let left = t.scroll_x / cw.max(1.0);
            if cell < left {
                t.scroll_x = cell * cw;
            } else if cell + 1.0 > left + cols as f32 {
                t.scroll_x = (cell + 1.0 - cols as f32) * cw;
            }
        }
    }

    fn selection_line_span(&self) -> bool {
        let (a, b) = self.tabs[self.active].buf.selection();
        a.line != b.line
    }

    /// Runs an edit on the active tab unless it is read-only.
    fn edit(&mut self, f: impl FnOnce(&mut Tab, &Settings)) {
        self.tabs[self.active].extra.clear();
        self.edit_keep(f);
    }

    /// Like `edit`, but leaves the extra carets alone (the closure owns them).
    fn edit_keep(&mut self, f: impl FnOnce(&mut Tab, &Settings)) {
        let t = &mut self.tabs[self.active];
        if t.readonly {
            self.status = "This file is read-only. Use File, Read-only to unlock it.".into();
            return;
        }
        let line = t.buf.selection().0.line;
        t.hl.borrow_mut().invalidate(line);
        t.want = None;
        t.buf.break_undo();
        let (lines0, start) = (t.buf.line_count(), t.buf.selection().0.line);
        f(t, &self.settings);
        t.buf.break_undo();
        let lines1 = t.buf.line_count();
        if lines1 != lines0 && !t.bookmarks.is_empty() {
            let d = lines1 as isize - lines0 as isize;
            t.bookmarks = t
                .bookmarks
                .iter()
                .filter_map(|&b| {
                    if b <= start {
                        Some(b)
                    } else if d < 0 && b as isize <= start as isize - d {
                        None
                    } else {
                        Some((b as isize + d).max(0) as usize)
                    }
                })
                .collect();
        }
    }

    fn edit_all(&mut self, f: impl FnOnce(&mut Tab, &Settings)) {
        self.edit(|t, s| {
            f(t, s);
            t.hl.borrow_mut().invalidate(0);
        });
    }

    fn type_text(&mut self, s: &str) {
        let auto = self.settings.auto_close;
        let over = self.overwrite;
        self.edit_multi(|t, _| {
            for ch in s.chars() {
                if auto && s.chars().count() == 1 && !t.buf.has_selection() {
                    if lineops::is_overtype(&t.buf, ch) {
                        t.buf.move_right(false);
                        continue;
                    }
                    if let Some(pair) = lineops::auto_close(&t.buf, ch) {
                        t.buf.insert(&pair);
                        t.buf.move_left(false);
                        continue;
                    }
                }
                if over && !t.buf.has_selection() {
                    let c = t.buf.caret;
                    if c.col < t.buf.line_len(c.line) {
                        t.buf.select_range(c, Pos::new(c.line, c.col + 1));
                    }
                }
                t.buf.insert(&ch.to_string());
            }
            t.buf.break_undo();
        });
    }

    fn enter(&mut self) {
        let ai = self.settings.auto_indent;
        self.edit_multi(|t, _| {
            if ai {
                let (txt, n) = lineops::newline_text(&t.buf, t.indent);
                let total = txt.chars().count();
                t.buf.insert(&txt);
                for _ in 0..total.saturating_sub(n) {
                    t.buf.move_left(false);
                }
            } else {
                t.buf.insert("\n");
            }
        });
    }

    fn tab_key(&mut self, back: bool) {
        let multi = self.selection_line_span() || !self.tabs[self.active].extra.is_empty() && back;
        self.edit_multi(|t, _| {
            if multi || back {
                lineops::indent_lines(&mut t.buf, t.indent, back);
            } else if t.indent.use_spaces {
                let c = t.buf.caret;
                let x = cell_of(t.buf.line(c.line), c.col, t.indent.width);
                let n = t.indent.width - x % t.indent.width;
                t.buf.insert(&" ".repeat(n));
            } else {
                t.buf.insert("\t");
            }
        });
    }

    fn all_selected_text(t: &Tab) -> String {
        let mut v: Vec<(Pos, String)> = Vec::new();
        for &(a, c) in &t.extra {
            v.push((a.min(c), t.buf.slice(a.min(c), a.max(c))));
        }
        v.push((t.buf.selection().0, t.buf.selected_text()));
        v.sort_by_key(|x| x.0);
        v.into_iter().map(|x| x.1).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n")
    }

    fn nav(&mut self, key: Named, ctrl: bool, shift: bool) -> bool {
        let page = self.vp.1.max(2) as isize - 1;
        let t = &mut self.tabs[self.active];
        let is_nav = matches!(
            key,
            Named::ArrowUp | Named::ArrowDown | Named::PageUp | Named::PageDown | Named::ArrowLeft | Named::ArrowRight | Named::Home | Named::End
        );
        if !is_nav {
            return false;
        }
        if t.extra.is_empty() {
            return Self::nav_one(t, page, key, ctrl, shift);
        }
        let mut sels = t.extra.clone();
        sels.push((t.buf.anchor, t.buf.caret));
        let mut out = Vec::with_capacity(sels.len());
        for (a, c) in sels {
            t.buf.select_range(a, c);
            t.want = None;
            Self::nav_one(t, page, key, ctrl, shift);
            out.push((t.buf.anchor, t.buf.caret));
        }
        Self::set_selections(t, out);
        true
    }

    /// Makes `sels` the selections of the tab: the last one is the primary
    /// caret, overlapping or identical ones are merged.
    fn set_selections(t: &mut Tab, mut sels: Vec<(Pos, Pos)>) {
        let prim = sels.pop().unwrap_or((t.buf.anchor, t.buf.caret));
        let mut extra: Vec<(Pos, Pos)> = Vec::new();
        let key = |s: &(Pos, Pos)| (s.0.min(s.1), s.0.max(s.1));
        let pk = key(&prim);
        sels.sort_by_key(key);
        for s in sels {
            let k = key(&s);
            let clash_prev = extra.last().map(|l| { let lk = key(l); k.0 < lk.1 || (k.0 == lk.0) }).unwrap_or(false);
            let clash_prim = k.0 < pk.1 && pk.0 < k.1 || k == pk || (k.0 == k.1 && k.0 == pk.0);
            if !clash_prev && !clash_prim {
                extra.push(s);
            }
        }
        t.buf.select_range(prim.0, prim.1);
        t.extra = extra;
    }

    /// Chars from `p` to the end of the document.
    fn dist_to_end(b: &Buffer, p: Pos) -> usize {
        let mut d = b.line_len(p.line) - p.col.min(b.line_len(p.line));
        for l in p.line + 1..b.line_count() {
            d += b.line_len(l) + 1;
        }
        d
    }

    /// The position `d` chars before the end of the document.
    fn pos_from_end(b: &Buffer, mut d: usize) -> Pos {
        let mut l = b.line_count() - 1;
        loop {
            let n = b.line_len(l);
            if d <= n {
                return Pos::new(l, n - d);
            }
            if l == 0 {
                return Pos::new(0, 0);
            }
            d -= n + 1;
            l -= 1;
        }
    }

    /// Selects the cells between `a` and `p` on every line in between.
    fn column_select(t: &mut Tab, a: Pos, p: Pos) {
        let tab = t.indent.width;
        let xa = cell_of(t.buf.line(a.line), a.col, tab);
        let xp = cell_of(t.buf.line(p.line), p.col, tab);
        let (lo, hi) = (xa.min(xp), xa.max(xp));
        let (l0, l1) = (a.line.min(p.line), a.line.max(p.line));
        let mut sels = Vec::new();
        for l in l0..=l1 {
            let line = t.buf.line(l);
            let c1 = view::col_at_cell(line, lo, tab);
            let c2 = view::col_at_cell(line, hi, tab);
            let (an, ca) = if xp >= xa { (c1, c2) } else { (c2, c1) };
            sels.push((l, Pos::new(l, an), Pos::new(l, ca)));
        }
        let prim_i = sels.iter().position(|s| s.0 == p.line).unwrap_or(0);
        let prim = sels.remove(prim_i);
        t.want = None;
        t.buf.select_range(prim.1, prim.2);
        t.extra = sels.into_iter().map(|s| (s.1, s.2)).collect();
    }

    fn add_caret_vertical(&mut self, d: isize) {
        let t = &mut self.tabs[self.active];
        let c = t.buf.caret;
        let nl = c.line as isize + d;
        if nl < 0 || nl as usize >= t.buf.line_count() {
            return;
        }
        let tab = t.indent.width;
        let x = cell_of(t.buf.line(c.line), c.col, tab);
        let nl = nl as usize;
        let col = view::col_at_cell(t.buf.line(nl), x, tab);
        let prim = (t.buf.anchor, t.buf.caret);
        t.extra.push(prim);
        t.buf.set_caret(Pos::new(nl, col), false);
    }

    /// Ctrl+Alt+D: select the word, then add the next match as another caret.
    fn add_next_match(&mut self) {
        let t = &mut self.tabs[self.active];
        if !t.buf.has_selection() {
            let c = t.buf.caret;
            t.buf.select_word_at(c);
            return;
        }
        let needle = t.buf.selected_text();
        if needle.is_empty() || needle.contains('\n') {
            return;
        }
        let nlen = needle.chars().count();
        let (_, end) = t.buf.selection();
        // search from the end of the latest selection, then wrap
        let from = t.extra.iter().map(|e| e.0.max(e.1)).chain([end]).max().unwrap_or(end);
        let line_find = |t: &Tab, l: usize, from_col: usize| -> Option<usize> {
            let line = t.buf.line(l);
            let chars: Vec<char> = line.chars().collect();
            let n: Vec<char> = needle.chars().collect();
            if chars.len() < n.len() {
                return None;
            }
            (from_col..=chars.len() - n.len()).find(|&i| chars[i..i + n.len()] == n[..])
        };
        let total = t.buf.line_count();
        let mut hit = None;
        for k in 0..=total {
            let l = (from.line + k) % total;
            let start = if k == 0 { from.col } else { 0 };
            if let Some(c) = line_find(t, l, start) {
                let at = Pos::new(l, c);
                // skip matches that are already carets
                let taken = t.extra.iter().chain([&(t.buf.anchor, t.buf.caret)]).any(|e| e.0.min(e.1) == at);
                if !taken {
                    hit = Some(at);
                    break;
                }
            }
        }
        if let Some(at) = hit {
            let prim = (t.buf.anchor, t.buf.caret);
            t.extra.push(prim);
            t.buf.select_range(at, Pos::new(at.line, at.col + nlen));
        }
    }

    /// Runs an edit once per caret (bottom to top so earlier carets keep
    /// their place). With a single caret it is a plain edit.
    fn edit_multi(&mut self, mut f: impl FnMut(&mut Tab, &Settings)) {
        if self.tabs[self.active].extra.is_empty() {
            return self.edit(f);
        }
        self.edit_keep(|t, s| {
            let mut sels = t.extra.clone();
            let prim_i = sels.len();
            sels.push((t.buf.anchor, t.buf.caret));
            let mut order: Vec<usize> = (0..sels.len()).collect();
            order.sort_by_key(|&i| std::cmp::Reverse(sels[i].0.min(sels[i].1)));
            t.buf.begin_join();
            let mut out = vec![(0usize, 0usize); sels.len()];
            for i in order {
                let (a, c) = sels[i];
                t.buf.select_range(a, c);
                f(t, s);
                out[i] = (Self::dist_to_end(&t.buf, t.buf.anchor), Self::dist_to_end(&t.buf, t.buf.caret));
            }
            t.buf.end_join();
            let mut res: Vec<(Pos, Pos)> = out.iter().map(|&(a, c)| (Self::pos_from_end(&t.buf, a), Self::pos_from_end(&t.buf, c))).collect();
            let prim = res.remove(prim_i);
            res.push(prim);
            Self::set_selections(t, res);
        });
    }

    fn nav_one(t: &mut Tab, page: isize, key: Named, ctrl: bool, shift: bool) -> bool {
        t.buf.break_undo();
        let vertical = |t: &mut Tab, d: isize| {
            let x = *t.want.get_or_insert_with(|| {
                let c = t.buf.caret;
                let r = t.rows.rows[t.rows.row_of(c, &t.buf)];
                let line = t.buf.line(c.line);
                cell_of(line, c.col, t.indent.width).saturating_sub(cell_of(line, r.from, t.indent.width))
            });
            if let Some(p) = t.rows.step(&t.buf, t.buf.caret, d, x, t.indent.width) {
                t.buf.set_caret(p, shift);
            } else if d < 0 {
                t.buf.move_doc_start(shift);
            } else {
                t.buf.move_doc_end(shift);
            }
        };
        match key {
            Named::ArrowUp => vertical(t, -1),
            Named::ArrowDown => vertical(t, 1),
            Named::PageUp => vertical(t, -page),
            Named::PageDown => vertical(t, page),
            Named::ArrowLeft => {
                t.want = None;
                if ctrl { t.buf.move_word(-1, shift) } else { t.buf.move_left(shift) }
            }
            Named::ArrowRight => {
                t.want = None;
                if ctrl { t.buf.move_word(1, shift) } else { t.buf.move_right(shift) }
            }
            Named::Home => {
                t.want = None;
                if ctrl { t.buf.move_doc_start(shift) } else { t.buf.move_home(shift) }
            }
            Named::End => {
                t.want = None;
                if ctrl { t.buf.move_doc_end(shift) } else { t.buf.move_end(shift) }
            }
            _ => return false,
        }
        true
    }

    // ------------------------------------------------------------ files

    fn open_dialog(&self) -> Task<Message> {
        Task::future(async move {
            let r = cosmic::dialog::file_chooser::open::Dialog::new().title("Open").open_files().await;
            let v: Vec<PathBuf> = match r {
                Ok(resp) => resp.urls().iter().filter_map(|u| u.to_file_path().ok()).collect(),
                Err(_) => Vec::new(),
            };
            cosmic::Action::App(Message::Picked(v))
        })
    }

    fn open_paths(&self, paths: Vec<PathBuf>, goto: Option<usize>) -> Task<Message> {
        Task::future(async move {
            let r = tokio::task::spawn_blocking(move || paths.iter().map(|p| read_file(p, None)).collect::<Vec<_>>())
                .await
                .unwrap_or_default();
            cosmic::Action::App(Message::Opened(r, goto))
        })
    }

    fn save_as_dialog(&self, id: u64, close: bool) -> Task<Message> {
        let name = self.tab_index(id).map(|i| self.tabs[i].name()).unwrap_or_default();
        Task::future(async move {
            let r = cosmic::dialog::file_chooser::save::Dialog::new().title("Save as".to_string()).file_name(name.to_string()).save_file().await;
            let p = r.ok().and_then(|r| r.url().and_then(|u| u.to_file_path().ok()));
            cosmic::Action::App(Message::SavePicked(id, p, close))
        })
    }

    fn pdf_dialog(&self) -> Task<Message> {
        let stem = Path::new(&self.tabs[self.active].name()).file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled".into());
        Task::future(async move {
            let r = cosmic::dialog::file_chooser::save::Dialog::new().title("Save as PDF".to_string()).file_name(format!("{stem}.pdf")).save_file().await;
            let p = r.ok().and_then(|r| r.url().and_then(|u| u.to_file_path().ok()));
            cosmic::Action::App(Message::PdfPicked(p))
        })
    }

    /// The current tab as PDF bytes plus its title, made off the UI thread.
    fn pdf_job(&self) -> (String, String, usize) {
        let t = &self.tabs[self.active];
        (t.buf.text(), t.name().to_string(), self.settings.tab_width)
    }

    fn save_tab(&self, id: u64, close: bool) -> Task<Message> {
        let Some(i) = self.tab_index(id) else { return Task::none() };
        match &self.tabs[i].path {
            Some(p) => self.write_task(id, p.clone(), close),
            None => self.save_as_dialog(id, close),
        }
    }

    fn write_task(&self, id: u64, path: PathBuf, close: bool) -> Task<Message> {
        let Some(i) = self.tab_index(id) else { return Task::none() };
        let (bytes, lossy) = self.tabs[i].bytes();
        Task::future(async move {
            let p = path.clone();
            let result = tokio::task::spawn_blocking(move || write_file(&p, &bytes))
                .await
                .unwrap_or_else(|e| Err(SaveFail { msg: e.to_string(), denied: false, bytes: Vec::new() }));
            cosmic::Action::App(Message::Saved { id, path, close, lossy, result })
        })
    }

    fn close_tab_id(&mut self, id: u64) {
        let Some(i) = self.tab_index(id) else { return };
        self.tabs.remove(i);
        if self.tabs.is_empty() {
            self.new_tab("", None, Enc::Utf8, Eol::Lf);
        }
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        } else if i < self.active {
            self.active -= 1;
        }
    }

    fn request_close(&mut self, i: usize) {
        let t = &self.tabs[i];
        if t.buf.is_modified() && t.path.is_some() {
            self.prompt = Some(Prompt::CloseTab(t.id));
        } else {
            let id = t.id;
            self.close_tab_id(id);
        }
    }

    fn session(&self) -> Session {
        Session {
            tabs: self
                .tabs
                .iter()
                .map(|t| SessTab {
                    path: t.path.as_ref().map(|p| p.to_string_lossy().into_owned()),
                    text: if t.buf.is_modified() || t.path.is_none() { Some(t.buf.text()) } else { None },
                    enc: settings::enc_key(t.enc),
                    eol: eol_key(t.eol).into(),
                    lang: t.lang.clone(),
                    line: t.buf.caret.line,
                    col: t.buf.caret.col,
                    scroll_y: t.scroll_y,
                })
                .collect(),
            active: self.active,
        }
    }

    fn save_session_if_changed(&mut self, force: bool) {
        if self.lock.is_none() {
            return;
        }
        let key: Vec<(u64, u64)> = self.tabs.iter().map(|t| (t.id, t.buf.revision as u64)).chain([(0, self.active as u64)]).collect();
        if force || key != self.last_session {
            settings::save_session(&self.session());
            self.last_session = key;
        }
    }

    fn restore(&mut self, sess: Session) {
        for st in sess.tabs {
            let enc = settings::enc_from_key(&st.enc);
            let eol = eol_from(&st.eol);
            let idx = match &st.path {
                Some(p) => {
                    let path = PathBuf::from(p);
                    match read_file(&path, None) {
                        Ok(l) => {
                            let i = self.new_tab(&l.text, Some(path), l.enc, l.eol);
                            self.tabs[i].readonly = l.readonly;
                            self.tabs[i].mtime = l.mtime;
                            if let Some(txt) = &st.text {
                                self.tabs[i].buf.replace_all(txt);
                            }
                            Some(i)
                        }
                        Err(_) => st.text.as_ref().map(|txt| {
                            let i = self.new_tab("", Some(path), enc, eol);
                            self.tabs[i].buf.replace_all(txt);
                            i
                        }),
                    }
                }
                None => Some(self.new_tab(st.text.as_deref().unwrap_or(""), None, enc, eol)),
            };
            if let Some(i) = idx {
                let t = &mut self.tabs[i];
                t.enc = if st.path.is_some() && st.text.is_none() { t.enc } else { enc };
                if !st.lang.is_empty() {
                    t.set_lang(&st.lang);
                }
                let p = t.buf.clamp(Pos::new(st.line, st.col));
                t.buf.set_caret(p, false);
                t.scroll_y = st.scroll_y.max(0.0);
            }
        }
        self.active = sess.active.min(self.tabs.len().saturating_sub(1));
    }

    // ------------------------------------------------------------ search

    fn compile(&mut self) -> Option<regex::Regex> {
        match self.q.compile() {
            Ok(Some(re)) => Some(re),
            Ok(None) => None,
            Err(e) => {
                self.status = e;
                None
            }
        }
    }

    fn find(&mut self, forward: bool, incremental: bool) {
        let Some(re) = self.compile() else { return };
        let t = &mut self.tabs[self.active];
        let (a, b) = t.buf.selection();
        let from = if forward && !incremental { b } else { a };
        match search::find(&t.buf, &re, from, forward) {
            Some(m) => {
                t.buf.select_range(Pos::new(m.line, m.start), Pos::new(m.line, m.end));
                self.status = format!("{} matches", search::count_all(&t.buf, &re));
            }
            None => self.status = "No matches".into(),
        }
    }

    fn replace_one(&mut self) {
        let Some(re) = self.compile() else { return };
        if self.tabs[self.active].readonly {
            self.status = "This file is read-only.".into();
            return;
        }
        let with = self.repl.clone();
        let regex = self.q.regex;
        let t = &mut self.tabs[self.active];
        let (a, b) = t.buf.selection();
        if let Some(m) = search::find(&t.buf, &re, a, true) {
            if Pos::new(m.line, m.start) == a && Pos::new(m.line, m.end) == b {
                let r = search::replacement_for(&re, t.buf.line(m.line), m, &with, regex);
                t.hl.borrow_mut().invalidate(a.line);
                t.buf.break_undo();
                t.buf.replace_selection(&r);
                t.buf.break_undo();
            }
        }
        self.find(true, false);
    }

    fn replace_all(&mut self) {
        let Some(re) = self.compile() else { return };
        if self.tabs[self.active].readonly {
            self.status = "This file is read-only.".into();
            return;
        }
        let with = self.repl.clone();
        let regex = self.q.regex;
        let t = &mut self.tabs[self.active];
        t.buf.break_undo();
        let n = search::replace_all(&mut t.buf, &re, &with, regex);
        t.buf.break_undo();
        t.hl.borrow_mut().invalidate(0);
        self.status = format!("Replaced {n}");
    }

    fn open_panel(&mut self, p: Panel) -> Task<Message> {
        self.panel = p;
        let id = match p {
            Panel::Find | Panel::Replace => self.find_id.clone(),
            Panel::Goto => self.goto_id.clone(),
            Panel::Files => self.files_id.clone(),
            Panel::Print | Panel::None => return Task::none(),
        };
        let sel = self.tabs[self.active].buf.selected_text();
        if matches!(p, Panel::Find | Panel::Replace) && !sel.is_empty() && !sel.contains('\n') {
            self.q.text = sel;
        }
        widget::text_input::focus::<cosmic::Action<Message>>(id)
    }

    // ------------------------------------------------------------ commands

    fn cmd(&mut self, c: Cmd) -> Task<Message> {
        self.menu = None;
        let mut tasks: Vec<Task<Message>> = Vec::new();
        match c {
            Cmd::New => {
                self.active = self.new_tab("", None, Enc::Utf8, Eol::Lf);
            }
            Cmd::NewWindow => {
                if let Ok(exe) = std::env::current_exe() {
                    let _ = std::process::Command::new(exe).spawn();
                }
            }
            Cmd::Open => tasks.push(self.open_dialog()),
            Cmd::Save => tasks.push(self.save_tab(self.tabs[self.active].id, false)),
            Cmd::SaveAs => tasks.push(self.save_as_dialog(self.tabs[self.active].id, false)),
            Cmd::Print => {
                self.panel = Panel::Print;
                tasks.push(Task::future(async {
                    let (l, d) = tokio::task::spawn_blocking(print::printers).await.unwrap_or_default();
                    cosmic::Action::App(Message::Printers(l, d))
                }));
            }
            Cmd::SavePdf => tasks.push(self.pdf_dialog()),
            Cmd::CloseTab => self.request_close(self.active),
            Cmd::Exit => tasks.push(Task::done(cosmic::Action::App(Message::CloseWindow))),
            Cmd::Undo => self.edit_all(|t, _| t.buf.undo()),
            Cmd::Redo => self.edit_all(|t, _| t.buf.redo()),
            Cmd::Cut => {
                let s = Self::all_selected_text(&self.tabs[self.active]);
                if !s.is_empty() {
                    tasks.push(clip_write(s));
                    self.edit_multi(|t, _| t.buf.replace_selection(""));
                }
            }
            Cmd::Copy => {
                let s = Self::all_selected_text(&self.tabs[self.active]);
                if !s.is_empty() {
                    tasks.push(clip_write(s));
                }
            }
            Cmd::AddNext => self.add_next_match(),
            Cmd::CaretAbove => self.add_caret_vertical(-1),
            Cmd::CaretBelow => self.add_caret_vertical(1),
            Cmd::ClearCarets => self.tabs[self.active].extra.clear(),
            Cmd::BmToggle => {
                let t = &mut self.tabs[self.active];
                let l = t.buf.caret.line;
                if !t.bookmarks.remove(&l) {
                    t.bookmarks.insert(l);
                }
            }
            Cmd::BmNext | Cmd::BmPrev => {
                let fwd = matches!(c, Cmd::BmNext);
                let t = &mut self.tabs[self.active];
                let l = t.buf.caret.line;
                let hit = if fwd {
                    t.bookmarks.range(l + 1..).next().or_else(|| t.bookmarks.iter().next())
                } else {
                    t.bookmarks.range(..l).next_back().or_else(|| t.bookmarks.iter().next_back())
                };
                if let Some(&h) = hit {
                    t.extra.clear();
                    t.buf.set_caret(Pos::new(h, 0), false);
                } else {
                    self.status = "No bookmarks. Ctrl+F2 adds one on this line.".into();
                }
            }
            Cmd::BmClear => self.tabs[self.active].bookmarks.clear(),
            Cmd::BmCopy => {
                let t = &self.tabs[self.active];
                let s: Vec<&str> = t.bookmarks.iter().filter(|&&l| l < t.buf.line_count()).map(|&l| t.buf.line(l)).collect();
                if !s.is_empty() {
                    tasks.push(clip_write(s.join("\n")));
                }
            }
            Cmd::BmDelete => {
                let marked: Vec<usize> = self.tabs[self.active].bookmarks.iter().rev().copied().collect();
                if !marked.is_empty() {
                    self.edit_all(|t, _| {
                        t.buf.begin_join();
                        for l in marked {
                            if l < t.buf.line_count() {
                                lineops::delete_line_at(&mut t.buf, l);
                            }
                        }
                        t.buf.end_join();
                        t.bookmarks.clear();
                    });
                }
            }
            Cmd::Minimap => self.settings.minimap = !self.settings.minimap,
            Cmd::Paste => tasks.push(cosmic::iced::clipboard::read().map(|t| cosmic::Action::App(Message::Pasted(t)))),
            Cmd::SelectAll => self.tabs[self.active].buf.select_all(),
            Cmd::Indent => self.tab_key(false),
            Cmd::Outdent => self.tab_key(true),
            Cmd::Comment => {
                self.edit(|t, _| {
                    let l = t.lang.clone();
                    if !lineops::toggle_comment(&mut t.buf, &l) {
                        // no comment syntax for this language: nothing to do
                    }
                });
            }
            Cmd::Upper => self.edit(|t, _| lineops::change_case(&mut t.buf, Case::Upper)),
            Cmd::Lower => self.edit(|t, _| lineops::change_case(&mut t.buf, Case::Lower)),
            Cmd::Title => self.edit(|t, _| lineops::change_case(&mut t.buf, Case::Title)),
            Cmd::Dup => self.edit(|t, _| lineops::duplicate(&mut t.buf)),
            Cmd::DelLine => self.edit(|t, _| lineops::delete_lines(&mut t.buf)),
            Cmd::MoveUp => self.edit(|t, _| {
                lineops::move_lines(&mut t.buf, -1);
            }),
            Cmd::MoveDown => self.edit(|t, _| {
                lineops::move_lines(&mut t.buf, 1);
            }),
            Cmd::Join => self.edit(|t, _| lineops::join(&mut t.buf)),
            Cmd::SplitLong => self.edit_all(|t, _| lineops::split_long(&mut t.buf, 80)),
            Cmd::Sort(k) => {
                let kind = match k {
                    0 => SortKind::Ascending,
                    1 => SortKind::Descending,
                    2 => SortKind::IgnoreCase,
                    3 => SortKind::Numeric,
                    4 => SortKind::ByLength,
                    _ => SortKind::Reverse,
                };
                self.edit_all(|t, _| lineops::sort(&mut t.buf, kind));
            }
            Cmd::Dedup => self.edit_all(|t, _| {
                lineops::remove_duplicates(&mut t.buf);
            }),
            Cmd::RemoveEmpty => self.edit_all(|t, _| {
                lineops::remove_empty_lines(&mut t.buf);
            }),
            Cmd::Trim => self.edit_all(|t, _| {
                lineops::trim_trailing(&mut t.buf);
            }),
            Cmd::Find => tasks.push(self.open_panel(Panel::Find)),
            Cmd::Replace => tasks.push(self.open_panel(Panel::Replace)),
            Cmd::Goto => tasks.push(self.open_panel(Panel::Goto)),
            Cmd::Files => tasks.push(self.open_panel(Panel::Files)),
            Cmd::FindNext => self.find(true, false),
            Cmd::FindPrev => self.find(false, false),
            Cmd::Wrap => self.settings.wrap = !self.settings.wrap,
            Cmd::Whitespace => self.settings.show_ws = !self.settings.show_ws,
            Cmd::Eols => self.settings.show_eol = !self.settings.show_eol,
            Cmd::LineNumbers => self.settings.line_numbers = !self.settings.line_numbers,
            Cmd::HlLine => self.settings.hl_line = !self.settings.hl_line,
            Cmd::ZoomIn => self.settings.font_size += 1.0,
            Cmd::ZoomOut => self.settings.font_size -= 1.0,
            Cmd::ZoomReset => self.settings.font_size = Settings::default().font_size,
            Cmd::FoldAll => {
                let t = &mut self.tabs[self.active];
                t.folds = t.ranges.iter().copied().collect();
            }
            Cmd::UnfoldAll => self.tabs[self.active].folds.clear(),
            Cmd::SetEnc(e) => self.tabs[self.active].enc = e,
            Cmd::ReopenEnc(enc) => {
                let t = &self.tabs[self.active];
                if let Some(p) = t.path.clone() {
                    let id = t.id;
                    tasks.push(Task::future(async move {
                        let r = tokio::task::spawn_blocking(move || read_file(&p, Some(enc)))
                            .await
                            .unwrap_or_else(|e| Err(e.to_string()));
                        cosmic::Action::App(Message::Reloaded(id, r))
                    }));
                }
            }
            Cmd::ConvertEol(e) => self.tabs[self.active].eol = e,
            Cmd::SetLang(l) => self.tabs[self.active].set_lang(&l),
            Cmd::UseSpaces(on) => {
                self.settings.use_spaces = on;
                self.tabs[self.active].indent.use_spaces = on;
            }
            Cmd::TabWidth(w) => {
                self.settings.tab_width = w;
                self.tabs[self.active].indent.width = w;
            }
            Cmd::AutoClose => self.settings.auto_close = !self.settings.auto_close,
            Cmd::AutoIndent => self.settings.auto_indent = !self.settings.auto_indent,
            Cmd::NextTab => self.active = (self.active + 1) % self.tabs.len(),
            Cmd::PrevTab => self.active = (self.active + self.tabs.len() - 1) % self.tabs.len(),
            Cmd::MoveTabLeft => {
                if self.active > 0 {
                    self.tabs.swap(self.active, self.active - 1);
                    self.active -= 1;
                }
            }
            Cmd::MoveTabRight => {
                if self.active + 1 < self.tabs.len() {
                    self.tabs.swap(self.active, self.active + 1);
                    self.active += 1;
                }
            }
            Cmd::Reload => {
                let t = &self.tabs[self.active];
                if let Some(p) = t.path.clone() {
                    if t.buf.is_modified() {
                        self.prompt = Some(Prompt::Reload(t.id));
                    } else {
                        let id = t.id;
                        tasks.push(Task::future(async move {
                            let r = tokio::task::spawn_blocking(move || read_file(&p, None))
                                .await
                                .unwrap_or_else(|e| Err(e.to_string()));
                            cosmic::Action::App(Message::Reloaded(id, r))
                        }));
                    }
                }
            }
            Cmd::ToggleReadonly => {
                let t = &mut self.tabs[self.active];
                t.readonly = !t.readonly;
            }
            Cmd::OpenRecent(p) => tasks.push(self.open_paths(vec![PathBuf::from(p)], None)),
            Cmd::ClearRecent => {
                self.settings.recent.clear();
            }
            Cmd::About => {
                self.prompt = Some(Prompt::Info(
                    "About Lapin Notes".into(),
                    "Notes is the text and code editor of Lapin UX. Everything stays on this computer: no network, no accounts.\n\nIts feature set takes inspiration from long-loved programmer's editors. See CREDITS.md for the libraries it uses.".into(),
                ));
            }
            Cmd::Shortcuts => {
                self.prompt = Some(Prompt::Info(
                    "Keyboard shortcuts".into(),
                    "Ctrl+N new, Ctrl+O open, Ctrl+S save, Ctrl+Shift+S save as, Ctrl+W close tab\nCtrl+F find, Ctrl+H replace, Ctrl+G go to line, Ctrl+Shift+F find in files, F3 / Shift+F3 next / previous\nCtrl+D duplicate line, Ctrl+L delete line, Ctrl+J join lines, Alt+Up / Alt+Down move lines\nCtrl+Q toggle comment, Ctrl+U lower case, Ctrl+Shift+U upper case, Tab / Shift+Tab indent\nCtrl+Plus / Ctrl+Minus / Ctrl+0 zoom, Ctrl+Tab next tab".into(),
                ));
            }
        }
        if self.settings.font_size != self.settings.font_size.clamp(8.0, 48.0) {
            self.settings.clamp();
        }
        settings::save_settings(&self.settings);
        Task::batch(tasks)
    }

    fn key(&mut self, key: Key, mods: Modifiers, text: Option<String>) -> Task<Message> {
        let ctrl = mods.control();
        let shift = mods.shift();
        let alt = mods.alt();
        if ctrl {
            if let Key::Character(c) = &key {
                let c = c.to_lowercase();
                let cmd = match (c.as_str(), shift) {
                    ("s", false) => Some(Cmd::Save),
                    ("s", true) => Some(Cmd::SaveAs),
                    ("p", _) => Some(Cmd::Print),
                    ("o", _) => Some(Cmd::Open),
                    ("n", false) => Some(Cmd::New),
                    ("n", true) => Some(Cmd::NewWindow),
                    ("w", _) => Some(Cmd::CloseTab),
                    ("z", false) => Some(Cmd::Undo),
                    ("z", true) | ("y", _) => Some(Cmd::Redo),
                    ("x", _) => Some(Cmd::Cut),
                    ("c", _) => Some(Cmd::Copy),
                    ("v", _) => Some(Cmd::Paste),
                    ("a", _) => Some(Cmd::SelectAll),
                    ("f", false) => Some(Cmd::Find),
                    ("f", true) => Some(Cmd::Files),
                    ("h", _) => Some(Cmd::Replace),
                    ("g", _) => Some(Cmd::Goto),
                    ("d", _) if alt => Some(Cmd::AddNext),
                    ("d", _) => Some(Cmd::Dup),
                    ("l", _) => Some(Cmd::DelLine),
                    ("j", _) => Some(Cmd::Join),
                    ("u", false) => Some(Cmd::Lower),
                    ("u", true) => Some(Cmd::Upper),
                    ("q", _) => Some(Cmd::Comment),
                    ("=", _) | ("+", _) => Some(Cmd::ZoomIn),
                    ("-", _) => Some(Cmd::ZoomOut),
                    ("0", _) => Some(Cmd::ZoomReset),
                    _ => None,
                };
                if let Some(cmd) = cmd {
                    return self.cmd(cmd);
                }
            }
        }
        if let Key::Named(n) = &key {
            match n {
                Named::Tab if ctrl => return self.cmd(if shift { Cmd::PrevTab } else { Cmd::NextTab }),
                Named::PageDown if ctrl => return self.cmd(Cmd::NextTab),
                Named::PageUp if ctrl => return self.cmd(Cmd::PrevTab),
                Named::F3 => return self.cmd(if shift { Cmd::FindPrev } else { Cmd::FindNext }),
                Named::ArrowUp if ctrl && alt => return self.cmd(Cmd::CaretAbove),
                Named::ArrowDown if ctrl && alt => return self.cmd(Cmd::CaretBelow),
                Named::ArrowUp | Named::ArrowDown | Named::ArrowLeft | Named::ArrowRight if alt && shift && !ctrl => {
                    let t = &mut self.tabs[self.active];
                    let c = t.buf.caret;
                    let a = t.col_anchor.unwrap_or(c);
                    let tab = t.indent.width;
                    let x = cell_of(t.buf.line(c.line), c.col, tab);
                    let p = match n {
                        Named::ArrowLeft => Pos::new(c.line, c.col.saturating_sub(1)),
                        Named::ArrowRight => Pos::new(c.line, (c.col + 1).min(t.buf.line_len(c.line))),
                        Named::ArrowUp if c.line > 0 => Pos::new(c.line - 1, view::col_at_cell(t.buf.line(c.line - 1), x, tab)),
                        Named::ArrowDown if c.line + 1 < t.buf.line_count() => Pos::new(c.line + 1, view::col_at_cell(t.buf.line(c.line + 1), x, tab)),
                        _ => c,
                    };
                    t.col_anchor = Some(a);
                    Self::column_select(t, a, p);
                    return Task::none();
                }
                Named::F2 if ctrl => return self.cmd(Cmd::BmToggle),
                Named::F2 => return self.cmd(if shift { Cmd::BmPrev } else { Cmd::BmNext }),
                Named::ArrowUp if alt || (ctrl && shift) => return self.cmd(Cmd::MoveUp),
                Named::ArrowDown if alt || (ctrl && shift) => return self.cmd(Cmd::MoveDown),
                Named::Escape => {
                    self.menu = None;
                    let t = &mut self.tabs[self.active];
                    if !t.extra.is_empty() && self.panel == Panel::None {
                        t.extra.clear();
                    }
                    self.panel = Panel::None;
                    return Task::none();
                }
                Named::Insert => {
                    self.overwrite = !self.overwrite;
                    return Task::none();
                }
                Named::Enter => {
                    self.enter();
                    return Task::none();
                }
                Named::Tab => {
                    self.tab_key(shift);
                    return Task::none();
                }
                Named::Backspace => {
                    let w = ctrl;
                    self.edit_multi(|t, _| if w { t.buf.delete_word(-1) } else { t.buf.backspace() });
                    return Task::none();
                }
                Named::Delete => {
                    let w = ctrl;
                    self.edit_multi(|t, _| if w { t.buf.delete_word(1) } else { t.buf.delete() });
                    return Task::none();
                }
                _ => {
                    if self.nav(*n, ctrl, shift) {
                        return Task::none();
                    }
                }
            }
        }
        if let Some(t) = text {
            let typed = (!ctrl || alt) && !t.is_empty() && t.chars().all(|c| !c.is_control());
            if typed {
                self.type_text(&t);
            }
        }
        Task::none()
    }

    fn title(&self) -> String {
        let t = &self.tabs[self.active];
        format!("{}{} - Notes", if t.buf.is_modified() { "* " } else { "" }, t.name())
    }
}

// ---------------------------------------------------------------- app

impl cosmic::Application for App {
    type Executor = cosmic::executor::Default;
    type Flags = Vec<PathBuf>;
    type Message = Message;
    const APP_ID: &'static str = settings::APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }
    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, flags: Self::Flags) -> (Self, Task<Message>) {
        let id = core.main_window_id();
        let lock = settings::lock_session();
        let mut app = App {
            core,
            settings: settings::load_settings(),
            tabs: Vec::new(),
            active: 0,
            next_id: 0,
            next_untitled: 0,
            menu: None,
            drag_tab: None,
            panel: Panel::None,
            q: Query::default(),
            repl: String::new(),
            goto: String::new(),
            printers: Vec::new(),
            printer: None,
            copies: 1,
            files_text: String::new(),
            files_filter: String::new(),
            hits: Vec::new(),
            status: String::new(),
            prompt: None,
            overwrite: false,
            vp: (80, 30, 8.0),
            lock,
            last_title: String::new(),
            last_session: Vec::new(),
            find_id: widget::Id::unique(),
            goto_id: widget::Id::unique(),
            files_id: widget::Id::unique(),
        };
        if app.lock.is_some() {
            let s = settings::load_session();
            app.restore(s);
        }
        for p in flags {
            match read_file(&p, None) {
                Ok(l) => {
                    let i = app.new_tab(&l.text, Some(l.path.clone()), l.enc, l.eol);
                    app.tabs[i].readonly = l.readonly;
                    app.tabs[i].mtime = l.mtime;
                    app.active = i;
                }
                Err(e) => app.prompt = Some(Prompt::Info("Could not open".into(), e)),
            }
        }
        if app.tabs.is_empty() {
            app.new_tab("", None, Enc::Utf8, Eol::Lf);
        }
        app.refresh();
        let title = app.title();
        app.last_title = title.clone();
        let t = match id {
            Some(id) => app.set_window_title(title, id),
            None => Task::none(),
        };
        (app, t)
    }

    fn header_start(&self) -> Vec<Element<'_, Message>> {
        vec![lapin_ux_ui::title_bar("accessories-text-editor", "Notes")]
    }

    fn on_close_requested(&self, _id: cosmic::iced::window::Id) -> Option<Message> {
        Some(Message::CloseWindow)
    }

    fn subscription(&self) -> Subscription<Message> {
        let keys = cosmic::iced::event::listen_with(|event, status, _| {
            // The end of a tab drag counts even when a widget took the event.
            if let cosmic::iced::Event::Mouse(cosmic::iced::mouse::Event::ButtonReleased(cosmic::iced::mouse::Button::Left)) = event {
                return Some(Message::DragEnd);
            }
            if status != cosmic::iced::event::Status::Ignored {
                return None;
            }
            match event {
                cosmic::iced::Event::Keyboard(KeyEvent::KeyPressed { key, modifiers, text, .. }) => {
                    Some(Message::Key(key, modifiers, text.map(|t| t.to_string())))
                }
                _ => None,
            }
        });
        let tick = cosmic::iced::time::every(Duration::from_secs(2)).map(|_| Message::Tick);
        Subscription::batch([keys, tick])
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        let mut tasks: Vec<Task<Message>> = Vec::new();
        let mut follow = false;
        let mut redraw = true;
        match message {
            Message::Key(k, m, t) => {
                tasks.push(self.key(k, m, t));
                follow = true;
            }
            Message::Cmd(c) => {
                tasks.push(self.cmd(c));
                follow = true;
            }
            Message::Menu(m) => self.menu = m,
            Message::Ed(ev) => {
                self.menu = None;
                match ev {
                    Ev::Press { pos, clicks, extend, alt, ctrl } => {
                        let t = &mut self.tabs[self.active];
                        t.want = None;
                        t.buf.break_undo();
                        if ctrl && clicks == 1 && !alt {
                            // Ctrl+click adds a caret (or removes the one clicked).
                            let prim = (t.buf.anchor, t.buf.caret);
                            if let Some(i) = t.extra.iter().position(|e| e.1 == pos) {
                                t.extra.remove(i);
                            } else {
                                t.extra.push(prim);
                                t.buf.set_caret(pos, false);
                            }
                            t.col_anchor = None;
                        } else {
                            t.extra.clear();
                            t.col_anchor = Some(pos);
                            match clicks {
                                1 => t.buf.set_caret(pos, extend),
                                2 => t.buf.select_word_at(pos),
                                _ => t.buf.select_line_at(pos.line),
                            }
                        }
                    }
                    Ev::Drag(p, alt) => {
                        let t = &mut self.tabs[self.active];
                        if alt {
                            let a = t.col_anchor.unwrap_or(t.buf.anchor);
                            Self::column_select(t, a, p);
                        } else if t.extra.is_empty() || t.col_anchor.is_none() {
                            t.buf.set_caret(p, true);
                        } else {
                            let a = t.col_anchor.unwrap_or(t.buf.anchor);
                            Self::column_select(t, a, p);
                        }
                    }
                    Ev::Bookmark(line) => {
                        let t = &mut self.tabs[self.active];
                        if !t.bookmarks.remove(&line) {
                            t.bookmarks.insert(line);
                        }
                    }
                    Ev::Minimap(row) => {
                        let lh = line_h(self.settings.font_size);
                        let vrows = self.vp.1;
                        let max = self.max_scroll(&self.tabs[self.active]);
                        let t = &mut self.tabs[self.active];
                        let top = row.saturating_sub(vrows / 2);
                        t.scroll_y = (top as f32 * lh).clamp(0.0, max);
                    }
                    Ev::Release => {}
                    Ev::Scroll(dy) => {
                        let max = self.max_scroll(&self.tabs[self.active]);
                        let t = &mut self.tabs[self.active];
                        t.scroll_y = (t.scroll_y + dy).clamp(0.0, max);
                    }
                    Ev::Viewport { cols, rows, cell_w } => {
                        if self.vp != (cols, rows, cell_w) {
                            self.vp = (cols, rows, cell_w);
                        }
                    }
                    Ev::Fold(line) => {
                        let t = &mut self.tabs[self.active];
                        if t.folds.remove(&line).is_none() {
                            if let Some(r) = t.ranges.iter().find(|r| r.0 == line) {
                                t.folds.insert(r.0, r.1);
                            }
                        }
                    }
                }
            }
            Message::SelectTab(i) => {
                if i < self.tabs.len() {
                    self.active = i;
                    self.drag_tab = Some(i);
                }
            }
            Message::DragOver(j) => {
                if let Some(i) = self.drag_tab {
                    if i != j && j < self.tabs.len() && i < self.tabs.len() {
                        let t = self.tabs.remove(i);
                        self.tabs.insert(j, t);
                        self.active = j;
                        self.drag_tab = Some(j);
                    }
                }
            }
            Message::DragEnd => self.drag_tab = None,
            Message::CloseTab(i) => {
                if i < self.tabs.len() {
                    self.request_close(i);
                }
            }
            Message::Picked(v) => {
                if !v.is_empty() {
                    tasks.push(self.open_paths(v, None));
                }
            }
            Message::Dropped(data) => {
                let v = uri_list_paths(&String::from_utf8_lossy(&data));
                if !v.is_empty() {
                    tasks.push(self.open_paths(v, None));
                }
            }
            Message::Opened(results, goto) => {
                for r in results {
                    match r {
                        Ok(l) => {
                            if let Some(i) = self.tabs.iter().position(|t| t.path.as_ref() == Some(&l.path)) {
                                self.active = i;
                            } else {
                                let cur = &self.tabs[self.active];
                                if cur.path.is_none() && !cur.buf.is_modified() && cur.buf.is_empty() {
                                    self.tabs.remove(self.active);
                                    let i = self.new_tab(&l.text, Some(l.path.clone()), l.enc, l.eol);
                                    let last = self.tabs.len() - 1;
                                    self.tabs.swap(i, self.active.min(last));
                                    self.active = self.active.min(last);
                                } else {
                                    self.active = self.new_tab(&l.text, Some(l.path.clone()), l.enc, l.eol);
                                }
                                let t = &mut self.tabs[self.active];
                                t.readonly = l.readonly;
                                t.mtime = l.mtime;
                            }
                            self.settings.add_recent(&l.path);
                            if let Some(line) = goto {
                                let t = &mut self.tabs[self.active];
                                let p = t.buf.clamp(Pos::new(line.saturating_sub(1), 0));
                                t.buf.set_caret(p, false);
                                follow = true;
                            }
                        }
                        Err(e) => self.prompt = Some(Prompt::Info("Could not open".into(), e)),
                    }
                }
                settings::save_settings(&self.settings);
            }
            Message::Reloaded(id, r) => {
                if let Some(i) = self.tab_index(id) {
                    match r {
                        Ok(l) => {
                            let t = &mut self.tabs[i];
                            t.buf.replace_all(&l.text);
                            t.buf.mark_saved();
                            t.enc = l.enc;
                            t.eol = l.eol;
                            t.readonly = l.readonly;
                            t.mtime = l.mtime;
                            t.hl.borrow_mut().invalidate(0);
                            let c = t.buf.clamp(t.buf.caret);
                            t.buf.set_caret(c, false);
                        }
                        Err(e) => self.prompt = Some(Prompt::Info("Could not reload".into(), e)),
                    }
                }
            }
            Message::Printers(l, d) => {
                if self.printer.as_ref().is_none_or(|p| !l.contains(p)) {
                    self.printer = d.or_else(|| l.first().cloned());
                }
                self.printers = l;
            }
            Message::PickPrinter(p) => self.printer = Some(p),
            Message::Copies(d) => self.copies = (self.copies as i32 + d).clamp(1, 99) as u32,
            Message::PrintRun => {
                if let Some(printer) = self.printer.clone() {
                    let (text, title, tab) = self.pdf_job();
                    let copies = self.copies;
                    self.panel = Panel::None;
                    self.status = format!("Printing {title}...");
                    tasks.push(Task::future(async move {
                        let r = tokio::task::spawn_blocking(move || {
                            print::send(&print::pdf(&text, &title, tab), &printer, copies, &title).map(|_| format!("Sent {title} to {printer}"))
                        })
                        .await
                        .unwrap_or_else(|e| Err(e.to_string()));
                        cosmic::Action::App(Message::PrintDone(r))
                    }));
                }
            }
            Message::PdfPicked(p) => {
                if let Some(path) = p {
                    let (text, title, tab) = self.pdf_job();
                    self.panel = Panel::None;
                    tasks.push(Task::future(async move {
                        let r = tokio::task::spawn_blocking(move || {
                            std::fs::write(&path, print::pdf(&text, &title, tab))
                                .map(|_| format!("Saved {}", path.display()))
                                .map_err(|e| format!("{}: {e}", path.display()))
                        })
                        .await
                        .unwrap_or_else(|e| Err(e.to_string()));
                        cosmic::Action::App(Message::PrintDone(r))
                    }));
                }
            }
            Message::PrintDone(r) => match r {
                Ok(s) => self.status = s,
                Err(e) => {
                    self.status.clear();
                    self.prompt = Some(Prompt::Info("Could not print".into(), e));
                }
            },
            Message::SavePicked(id, p, close) => {
                if let Some(p) = p {
                    tasks.push(self.write_task(id, p, close));
                }
            }
            Message::Saved { id, path, close, lossy, result } => match result {
                Ok(mtime) => {
                    if let Some(i) = self.tab_index(id) {
                        let t = &mut self.tabs[i];
                        let first_path = t.path.is_none();
                        t.buf.mark_saved();
                        t.mtime = mtime;
                        t.readonly = false;
                        t.path = Some(path.clone());
                        if first_path && t.lang == lang::PLAIN {
                            let l = lang::detect(&t.name(), t.buf.line(0));
                            t.set_lang(&l);
                        }
                        self.status = format!("Saved {}", t.name());
                    }
                    self.settings.add_recent(&path);
                    settings::save_settings(&self.settings);
                    if lossy {
                        self.prompt = Some(Prompt::Info(
                            "Some characters were replaced".into(),
                            "The chosen encoding cannot store every character in this file, so those were replaced. Choose UTF-8 in the Encoding menu and save again to keep them.".into(),
                        ));
                    }
                    if close {
                        self.close_tab_id(id);
                    }
                }
                Err(f) => {
                    if f.denied {
                        self.prompt = Some(Prompt::Admin { id, path, bytes: f.bytes, close });
                    } else {
                        self.prompt = Some(Prompt::Info("Could not save".into(), f.msg));
                    }
                }
            },
            Message::Pasted(t) => {
                if let Some(t) = t {
                    let t = t.replace("\r\n", "\n").replace('\r', "\n");
                    let n = self.tabs[self.active].extra.len() + 1;
                    let parts: Vec<String> = t.split('\n').map(String::from).collect();
                    if n > 1 && parts.len() == n {
                        let mut k = n;
                        self.edit_multi(|tab, _| {
                            k -= 1;
                            tab.buf.insert(&parts[k]);
                        });
                    } else {
                        self.edit_multi(|tab, _| tab.buf.insert(&t));
                    }
                    follow = true;
                }
            }
            Message::Tick => {
                redraw = false;
                self.save_session_if_changed(false);
                let t = &mut self.tabs[self.active];
                if let Some(p) = t.path.clone() {
                    let m = std::fs::metadata(&p).and_then(|m| m.modified()).ok();
                    if m.is_some() && m != t.mtime {
                        t.mtime = m;
                        if t.buf.is_modified() {
                            self.prompt = Some(Prompt::Reload(t.id));
                        } else {
                            let id = t.id;
                            tasks.push(Task::future(async move {
                                let r = tokio::task::spawn_blocking(move || read_file(&p, None))
                                    .await
                                    .unwrap_or_else(|e| Err(e.to_string()));
                                cosmic::Action::App(Message::Reloaded(id, r))
                            }));
                        }
                    }
                }
            }
            Message::FindText(s) => {
                self.q.text = s;
                self.find(true, true);
                follow = true;
            }
            Message::ReplText(s) => self.repl = s,
            Message::GotoText(s) => self.goto = s,
            Message::FilesText(s) => self.files_text = s,
            Message::FilesFilter(s) => self.files_filter = s,
            Message::ToggleCase => {
                self.q.match_case = !self.q.match_case;
                self.find(true, true);
            }
            Message::ToggleWord => {
                self.q.whole_word = !self.q.whole_word;
                self.find(true, true);
            }
            Message::ToggleRegex => {
                self.q.regex = !self.q.regex;
                self.find(true, true);
            }
            Message::FindNext => {
                self.find(true, false);
                follow = true;
            }
            Message::FindPrev => {
                self.find(false, false);
                follow = true;
            }
            Message::ReplaceOne => {
                self.replace_one();
                follow = true;
            }
            Message::ReplaceAll => self.replace_all(),
            Message::GotoSubmit => {
                let mut it = self.goto.trim().split(':');
                let line: usize = it.next().and_then(|s| s.trim().parse().ok()).unwrap_or(1);
                let col: usize = it.next().and_then(|s| s.trim().parse().ok()).unwrap_or(1);
                let t = &mut self.tabs[self.active];
                let p = t.buf.clamp(Pos::new(line.saturating_sub(1), col.saturating_sub(1)));
                t.buf.set_caret(p, false);
                self.panel = Panel::None;
                follow = true;
            }
            Message::FilesRun => {
                let dir = self.tabs[self.active]
                    .path
                    .as_ref()
                    .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                    .or_else(dirs::home_dir)
                    .unwrap_or_default();
                if let Some(re) = self.compile() {
                    let filter = self.files_filter.clone();
                    self.status = "Searching...".into();
                    tasks.push(Task::future(async move {
                        let hits = tokio::task::spawn_blocking(move || search::find_in_files(&dir, &re, &filter, 500))
                            .await
                            .unwrap_or_default();
                        cosmic::Action::App(Message::FilesHits(hits))
                    }));
                }
            }
            Message::FilesHits(h) => {
                self.status = format!("{} results", h.len());
                self.hits = h;
            }
            Message::OpenHit(i) => {
                if let Some(h) = self.hits.get(i) {
                    tasks.push(self.open_paths(vec![h.path.clone()], Some(h.line + 1)));
                }
            }
            Message::ClosePanel => self.panel = Panel::None,
            Message::PromptSave => {
                if let Some(Prompt::CloseTab(id)) = self.prompt.take() {
                    tasks.push(self.save_tab(id, true));
                }
            }
            Message::PromptDiscard => match self.prompt.take() {
                Some(Prompt::CloseTab(id)) => self.close_tab_id(id),
                Some(Prompt::Quit) => std::process::exit(0),
                Some(Prompt::Reload(id)) => {
                    if let Some(i) = self.tab_index(id) {
                        if let Some(p) = self.tabs[i].path.clone() {
                            tasks.push(Task::future(async move {
                                let r = tokio::task::spawn_blocking(move || read_file(&p, None))
                                    .await
                                    .unwrap_or_else(|e| Err(e.to_string()));
                                cosmic::Action::App(Message::Reloaded(id, r))
                            }));
                        }
                    }
                }
                _ => {}
            },
            Message::PromptCancel => self.prompt = None,
            Message::PromptAdmin => {
                if let Some(Prompt::Admin { id, path, bytes, close }) = self.prompt.take() {
                    tasks.push(Task::future(async move {
                        let p = path.clone();
                        let result = tokio::task::spawn_blocking(move || write_file_admin(&p, &bytes))
                            .await
                            .unwrap_or_else(|e| Err(SaveFail { msg: e.to_string(), denied: false, bytes: Vec::new() }));
                        cosmic::Action::App(Message::Saved { id, path, close, lossy: false, result })
                    }));
                }
            }
            Message::CloseWindow => {
                if self.lock.is_some() {
                    self.save_session_if_changed(true);
                    std::process::exit(0);
                }
                if self.tabs.iter().any(|t| t.buf.is_modified()) {
                    self.prompt = Some(Prompt::Quit);
                } else {
                    std::process::exit(0);
                }
            }
        }
        if redraw {
            self.refresh();
            if follow {
                self.ensure_visible();
            }
            let title = self.title();
            if title != self.last_title {
                self.last_title = title.clone();
                let id = self.core.main_window_id();
                if let Some(id) = id {
                    tasks.push(self.set_window_title(title, id));
                }
            }
        }
        Task::batch(tasks)
    }

    fn dialog(&self) -> Option<Element<'_, Message>> {
        let p = self.prompt.as_ref()?;
        let name_of = |id: u64| self.tab_index(id).map(|i| self.tabs[i].name()).unwrap_or_default();
        let d = widget::dialog();
        Some(match p {
            Prompt::CloseTab(id) => d
                .title(format!("Save changes to {}?", name_of(*id)))
                .body("Your changes are lost if you don't save them.")
                .primary_action(button::suggested("Save").on_press(Message::PromptSave))
                .secondary_action(button::destructive("Don't save").on_press(Message::PromptDiscard))
                .tertiary_action(button::standard("Cancel").on_press(Message::PromptCancel))
                .into(),
            Prompt::Quit => d
                .title("Close without saving?")
                .body("Some tabs have changes that are not saved.")
                .primary_action(button::destructive("Close anyway").on_press(Message::PromptDiscard))
                .secondary_action(button::standard("Cancel").on_press(Message::PromptCancel))
                .into(),
            Prompt::Reload(id) => d
                .title(format!("{} changed on disk", name_of(*id)))
                .body("Reload it and lose the changes made here, or keep what you have?")
                .primary_action(button::standard("Keep mine").on_press(Message::PromptCancel))
                .secondary_action(button::destructive("Reload").on_press(Message::PromptDiscard))
                .into(),
            Prompt::Info(t, b) => d
                .title(t.clone())
                .body(b.clone())
                .primary_action(button::suggested("OK").on_press(Message::PromptCancel))
                .into(),
            Prompt::Admin { path, .. } => d
                .title("Save as administrator?")
                .body(format!("You do not have permission to write {}. Save it with administrator rights instead?", path.display()))
                .primary_action(button::suggested("Save as administrator").on_press(Message::PromptAdmin))
                .secondary_action(button::standard("Cancel").on_press(Message::PromptCancel))
                .into(),
        })
    }

    fn view(&self) -> Element<'_, Message> {
        let t = &self.tabs[self.active];
        let bracket = lineops::matching_bracket(&t.buf, 20000);
        let ed: Element<Message> = Editor {
            buf: &t.buf,
            rows: &t.rows,
            folds: &t.folds,
            foldable: &t.foldable,
            hl: &t.hl,
            cfg: self.cfg(t),
            scroll_y: t.scroll_y,
            scroll_x: t.scroll_x,
            bracket,
            marks: &t.marks,
            extra: &t.extra,
            bookmarks: &t.bookmarks,
            line_marks: &t.line_marks,
            minimap: self.settings.minimap,
            on: Box::new(Message::Ed),
        }
        .into();
        let mut col = column::with_capacity(6);
        col = col.push(self.menu_bar());
        col = col.push(self.tab_strip());
        if let Some(p) = self.panel_view() {
            col = col.push(p);
        }
        col = col.push(lapin_ux_ui::a11y::group("Text editor", ed));
        col = col.push(self.status_bar());
        let body = container(col).width(Length::Fill).height(Length::Fill);
        // Files dragged in from the file manager open in new tabs.
        widget::dnd_destination(body, vec!["text/uri-list".into()])
            .on_finish(|_mime, data, _action, _x, _y| Message::Dropped(data))
            .into()
    }
}

// ---------------------------------------------------------------- view parts

impl App {
    fn menu_bar(&self) -> Element<'_, Message> {
        use Cmd::*;
        let it = |l: &str, a: &str, c: Cmd| MenuItem::new(l, a, Message::Cmd(c));
        let ck = |l: &str, on: bool, c: Cmd| MenuItem::check(l, on, Message::Cmd(c));
        let sep = MenuItem::separator;
        let t = &self.tabs[self.active];
        let s = &self.settings;

        let mut file = vec![
            it("New", "Ctrl+N", New),
            it("New window", "Ctrl+Shift+N", NewWindow),
            it("Open...", "Ctrl+O", Open),
            it("Save", "Ctrl+S", Save),
            it("Save as...", "Ctrl+Shift+S", SaveAs),
            sep(),
            it("Print...", "Ctrl+P", Print),
            it("Save as PDF...", "", SavePdf),
            sep(),
            it("Reload from disk", "", Reload),
            MenuItem::check("Read-only", t.readonly, Message::Cmd(ToggleReadonly)),
            it("Close tab", "Ctrl+W", CloseTab),
        ];
        if !s.recent.is_empty() {
            file.push(sep());
            for p in s.recent.iter().take(8) {
                let name = Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.clone());
                file.push(it(&name, "", OpenRecent(p.clone())));
            }
            file.push(it("Clear recent files", "", ClearRecent));
        }
        file.push(sep());
        file.push(it("Exit", "", Exit));

        let edit = vec![
            it("Undo", "Ctrl+Z", Undo),
            it("Redo", "Ctrl+Y", Redo),
            sep(),
            it("Cut", "Ctrl+X", Cut),
            it("Copy", "Ctrl+C", Copy),
            it("Paste", "Ctrl+V", Paste),
            it("Select all", "Ctrl+A", SelectAll),
            sep(),
            it("Indent", "Tab", Indent),
            it("Outdent", "Shift+Tab", Outdent),
            it("Toggle comment", "Ctrl+Q", Comment),
            it("Upper case", "Ctrl+Shift+U", Upper),
            it("Lower case", "Ctrl+U", Lower),
            it("Title case", "", Title),
            sep(),
            it("Duplicate line", "Ctrl+D", Dup),
            it("Delete line", "Ctrl+L", DelLine),
            it("Move line up", "Alt+Up", MoveUp),
            it("Move line down", "Alt+Down", MoveDown),
            it("Join lines", "Ctrl+J", Join),
        ];
        let search = vec![
            it("Find...", "Ctrl+F", Find),
            it("Find next", "F3", FindNext),
            it("Find previous", "Shift+F3", FindPrev),
            it("Replace...", "Ctrl+H", Replace),
            it("Find in files...", "Ctrl+Shift+F", Files),
            it("Go to line...", "Ctrl+G", Goto),
        ];
        let view = vec![
            ck("Word wrap", s.wrap, Wrap),
            ck("Show spaces and tabs", s.show_ws, Whitespace),
            ck("Show line endings", s.show_eol, Eols),
            ck("Line numbers", s.line_numbers, LineNumbers),
            ck("Highlight current line", s.hl_line, HlLine),
            sep(),
            it("Zoom in", "Ctrl++", ZoomIn),
            it("Zoom out", "Ctrl+-", ZoomOut),
            it("Reset zoom", "Ctrl+0", ZoomReset),
            sep(),
            it("Fold all", "", FoldAll),
            it("Unfold all", "", UnfoldAll),
        ];
        let mut enc = vec![
            ck("UTF-8", t.enc == Enc::Utf8, SetEnc(Enc::Utf8)),
            ck("UTF-8 with BOM", t.enc == Enc::Utf8Bom, SetEnc(Enc::Utf8Bom)),
            ck("UTF-16 LE", t.enc == Enc::Utf16Le, SetEnc(Enc::Utf16Le)),
            ck("UTF-16 BE", t.enc == Enc::Utf16Be, SetEnc(Enc::Utf16Be)),
        ];
        for (name, e) in encoding::legacy_choices() {
            enc.push(ck(name, t.enc == Enc::Legacy(e), SetEnc(Enc::Legacy(e))));
        }
        enc.push(sep());
        enc.push(ck("Windows (CRLF)", t.eol == Eol::Crlf, ConvertEol(Eol::Crlf)));
        enc.push(ck("Unix (LF)", t.eol == Eol::Lf, ConvertEol(Eol::Lf)));
        enc.push(ck("Old Mac (CR)", t.eol == Eol::Cr, ConvertEol(Eol::Cr)));
        enc.push(sep());
        // Reopen the file from disk, reading it with another encoding (for a
        // file that shows the wrong letters).
        for (name, e) in [
            ("UTF-8", Enc::Utf8),
            ("UTF-16 LE", Enc::Utf16Le),
            ("UTF-16 BE", Enc::Utf16Be),
        ] {
            enc.push(it(&format!("Reopen with {name}"), "", ReopenEnc(e)));
        }
        for (name, e) in encoding::legacy_choices() {
            enc.push(it(&format!("Reopen with {name}"), "", ReopenEnc(Enc::Legacy(e))));
        }

        let langs: Vec<MenuItem<Message>> = lang::names()
            .into_iter()
            .map(|n| {
                let on = t.lang == n;
                ck(&n, on, SetLang(n.clone()))
            })
            .collect();

        let settings_menu = vec![
            ck("Indent with spaces", s.use_spaces, UseSpaces(true)),
            ck("Indent with tabs", !s.use_spaces, UseSpaces(false)),
            sep(),
            ck("Tab width 2", s.tab_width == 2, TabWidth(2)),
            ck("Tab width 4", s.tab_width == 4, TabWidth(4)),
            ck("Tab width 8", s.tab_width == 8, TabWidth(8)),
            sep(),
            ck("Auto-close brackets and quotes", s.auto_close, AutoClose),
            ck("Auto-indent", s.auto_indent, AutoIndent),
        ];
        let tools = vec![
            it("Sort lines ascending", "", Sort(0)),
            it("Sort lines descending", "", Sort(1)),
            it("Sort ignoring case", "", Sort(2)),
            it("Sort numerically", "", Sort(3)),
            it("Sort by length", "", Sort(4)),
            it("Reverse line order", "", Sort(5)),
            sep(),
            it("Remove duplicate lines", "", Dedup),
            it("Remove empty lines", "", RemoveEmpty),
            it("Trim trailing spaces", "", Trim),
            it("Split long lines at 80", "", SplitLong),
        ];
        let mut window = vec![
            it("Next tab", "Ctrl+Tab", NextTab),
            it("Previous tab", "Ctrl+Shift+Tab", PrevTab),
            it("Move tab left", "", MoveTabLeft),
            it("Move tab right", "", MoveTabRight),
            sep(),
        ];
        for (i, tab) in self.tabs.iter().enumerate() {
            window.push(MenuItem::check(tab.name(), i == self.active, Message::SelectTab(i)));
        }
        let help = vec![it("Keyboard shortcuts", "", Shortcuts), it("About Lapin Notes", "", About)];

        lapin_ux_ui::menu_bar(
            vec![
                ("File".into(), file),
                ("Edit".into(), edit),
                ("Search".into(), search),
                ("View".into(), view),
                ("Encoding".into(), enc),
                ("Language".into(), langs),
                ("Settings".into(), settings_menu),
                ("Tools".into(), tools),
                ("Window".into(), window),
                ("Help".into(), help),
            ],
            self.menu,
            Message::Menu,
        )
    }

    fn tab_strip(&self) -> Element<'_, Message> {
        let mut r = row::with_capacity(self.tabs.len()).spacing(2).align_y(Alignment::Center).padding([2, 8]);
        for (i, t) in self.tabs.iter().enumerate() {
            let dot = if t.buf.is_modified() { "\u{25CF} " } else { "" };
            let label = row::with_capacity(2)
                .spacing(6)
                .align_y(Alignment::Center)
                .push(text::body(format!("{dot}{}", t.name())))
                .push(
                    button::icon(widget::icon::from_name("window-close-symbolic").size(12))
                        .name("Close tab")
                        .padding(2)
                        .on_press(Message::CloseTab(i)),
                );
            let b = button::custom(label).padding([4, 8]).class(lapin_ux_ui::nav_item(i == self.active)).on_press(Message::SelectTab(i));
            let mut area = widget::mouse_area(b).on_middle_press(Message::CloseTab(i));
            if self.drag_tab.is_some() {
                area = area.on_enter(Message::DragOver(i));
            }
            r = r.push(area);
        }
        lapin_ux_ui::a11y::group("Open files", r)
    }

    fn toggle(label: &'static str, name: &'static str, on: bool, m: Message) -> Element<'static, Message> {
        button::custom(text::body(label)).name(name).padding([4, 8]).class(lapin_ux_ui::nav_item(on)).on_press(m).into()
    }

    fn panel_view(&self) -> Option<Element<'_, Message>> {
        let mut c = column::with_capacity(3).spacing(4).padding([4, 8]);
        match self.panel {
            Panel::None => return None,
            Panel::Print => {}
            Panel::Find | Panel::Replace => {
                let find = widget::text_input("Find", &self.q.text)
                    .id(self.find_id.clone())
                    .on_input(Message::FindText)
                    .on_submit(|_| Message::FindNext)
                    .width(Length::Fill);
                let top = row::with_capacity(8)
                    .spacing(4)
                    .align_y(Alignment::Center)
                    .push(find)
                    .push(button::standard("Previous").on_press(Message::FindPrev))
                    .push(button::standard("Next").on_press(Message::FindNext))
                    .push(Self::toggle("Aa", "Match case", self.q.match_case, Message::ToggleCase))
                    .push(Self::toggle("W", "Whole word", self.q.whole_word, Message::ToggleWord))
                    .push(Self::toggle(".*", "Regular expression", self.q.regex, Message::ToggleRegex))
                    .push(button::icon(widget::icon::from_name("window-close-symbolic").size(14)).name("Close search").on_press(Message::ClosePanel));
                c = c.push(top);
                if self.panel == Panel::Replace {
                    c = c.push(
                        row::with_capacity(3)
                            .spacing(4)
                            .align_y(Alignment::Center)
                            .push(widget::text_input("Replace with", &self.repl).on_input(Message::ReplText).on_submit(|_| Message::ReplaceOne).width(Length::Fill))
                            .push(button::standard("Replace").on_press(Message::ReplaceOne))
                            .push(button::standard("Replace all").on_press(Message::ReplaceAll)),
                    );
                }
            }
            Panel::Goto => {
                c = c.push(
                    row::with_capacity(3)
                        .spacing(4)
                        .align_y(Alignment::Center)
                        .push(
                            widget::text_input("Line or line:column", &self.goto)
                                .id(self.goto_id.clone())
                                .on_input(Message::GotoText)
                                .on_submit(|_| Message::GotoSubmit)
                                .width(Length::Fill),
                        )
                        .push(button::suggested("Go").on_press(Message::GotoSubmit))
                        .push(button::icon(widget::icon::from_name("window-close-symbolic").size(14)).name("Close").on_press(Message::ClosePanel)),
                );
            }
            Panel::Files => {
                c = c.push(
                    row::with_capacity(5)
                        .spacing(4)
                        .align_y(Alignment::Center)
                        .push(
                            widget::text_input("Find in the folder of this file", &self.q.text)
                                .id(self.files_id.clone())
                                .on_input(Message::FindText)
                                .on_submit(|_| Message::FilesRun)
                                .width(Length::Fill),
                        )
                        .push(widget::text_input("Files, e.g. *.rs", &self.files_filter).on_input(Message::FilesFilter).on_submit(|_| Message::FilesRun).width(160))
                        .push(Self::toggle("Aa", "Match case", self.q.match_case, Message::ToggleCase))
                        .push(Self::toggle(".*", "Regular expression", self.q.regex, Message::ToggleRegex))
                        .push(button::suggested("Search").on_press(Message::FilesRun))
                        .push(button::icon(widget::icon::from_name("window-close-symbolic").size(14)).name("Close").on_press(Message::ClosePanel)),
                );
                if !self.hits.is_empty() {
                    let mut list = column::with_capacity(self.hits.len()).spacing(1);
                    for (i, h) in self.hits.iter().enumerate().take(200) {
                        let name = h.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        list = list.push(
                            button::custom(text::body(format!("{name}:{}  {}", h.line + 1, h.text.trim())))
                                .width(Length::Fill)
                                .padding([2, 8])
                                .class(lapin_ux_ui::nav_item(false))
                                .on_press(Message::OpenHit(i)),
                        );
                    }
                    c = c.push(widget::scrollable(list).height(Length::Fixed(160.0)));
                }
            }
        }
        if self.panel == Panel::Print {
            let mut r = row::with_capacity(self.printers.len() + 6).spacing(4).align_y(Alignment::Center);
            if self.printers.is_empty() {
                r = r.push(text::body("No printer found. You can still save a PDF."));
            } else {
                r = r.push(text::body("Printer"));
                for p in &self.printers {
                    let on = self.printer.as_deref() == Some(p.as_str());
                    r = r.push(
                        button::custom(text::body(p.clone())).padding([4, 8]).class(lapin_ux_ui::nav_item(on)).on_press(Message::PickPrinter(p.clone())),
                    );
                }
                r = r
                    .push(text::body("Copies"))
                    .push(button::standard("-").name("Fewer copies").on_press(Message::Copies(-1)))
                    .push(text::body(self.copies.to_string()))
                    .push(button::standard("+").name("More copies").on_press(Message::Copies(1)))
                    .push(button::suggested("Print").on_press(Message::PrintRun));
            }
            r = r
                .push(widget::Space::new().width(Length::Fill))
                .push(button::standard("Save as PDF...").on_press(Message::Cmd(Cmd::SavePdf)))
                .push(button::icon(widget::icon::from_name("window-close-symbolic").size(14)).name("Close").on_press(Message::ClosePanel));
            c = c.push(r);
        }
        Some(lapin_ux_ui::a11y::group("Search", c))
    }

    fn status_bar(&self) -> Element<'_, Message> {
        let t = &self.tabs[self.active];
        let c = t.buf.caret;
        let mut left = format!("Ln {}, Col {}", c.line + 1, c.col + 1);
        if t.buf.has_selection() {
            left.push_str(&format!("   Sel {}", t.buf.selected_chars()));
        }
        left.push_str(&format!("   {} chars, {} lines", t.buf.len_chars(), t.buf.line_count()));
        let mut right = format!("{}   {}   {}   {}", eol_short(t.eol), t.enc.label(), t.lang, if self.overwrite { "OVR" } else { "INS" });
        if t.readonly {
            right = format!("Read-only   {right}");
        }
        let r = row::with_capacity(3)
            .spacing(16)
            .padding([3, 10])
            .align_y(Alignment::Center)
            .push(text::caption(left))
            .push(container(text::caption(self.status.clone())).width(Length::Fill))
            .push(text::caption(right));
        lapin_ux_ui::a11y::group("Status bar", r)
    }
}

fn main() -> cosmic::iced::Result {
    lapin_ux_core::crash::install("lapin-ux-notes");
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()))
        .log_internal_errors(false)
        .init();
    let files: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    let settings = AppSettings::default()
        .size(Size::new(1000.0, 700.0))
        .size_limits(cosmic::iced::Limits::NONE.min_width(520.0).min_height(320.0));
    cosmic::app::run::<App>(settings, files)
}
