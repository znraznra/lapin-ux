// SPDX-License-Identifier: GPL-3.0-only
//! The text area: an iced widget that draws a monospace grid (line numbers,
//! fold marks, coloured text, selection, caret) and turns mouse input into
//! messages. Keys are handled by the app, so text fields elsewhere keep
//! theirs. The app owns the buffer; this widget only reads it.

use std::cell::RefCell;

use cosmic::iced::core::text::{self, Renderer as _};
use cosmic::iced::core::widget::{tree, Tree};
use cosmic::iced::core::{
    layout, mouse, renderer, Border, Clipboard, Color, Element, Event, Font, Layout, Length, Pixels, Point, Rectangle, Shell,
    Size, Widget,
};
use cosmic::iced::core::renderer::Renderer as _;
use cosmic::{Renderer, Theme};

use crate::buffer::{Buffer, Pos};
use crate::lang::Highlighter;
use crate::view::{cell_of, char_cells, Folds, Rows};

/// What the widget reports to the app.
#[derive(Debug, Clone)]
pub enum Ev {
    /// Mouse press at a position: `clicks` is 1, 2 or 3; `extend` for Shift.
    Press { pos: Pos, clicks: u8, extend: bool, alt: bool, ctrl: bool },
    /// Drag to a position; the flag is Alt (column selection).
    Drag(Pos, bool),
    /// A click on the bookmark cell of the gutter.
    Bookmark(usize),
    /// A click or drag on the minimap: the document row to centre.
    Minimap(usize),
    Release,
    /// Scroll by this many pixels (positive = down).
    Scroll(f32),
    /// Size of the text area in cells, and the width of one cell.
    Viewport { cols: usize, rows: usize, cell_w: f32 },
    /// A click on the fold mark of this line.
    Fold(usize),
}

#[derive(Clone)]
pub struct Cfg {
    pub font_size: f32,
    pub tab: usize,
    pub show_ws: bool,
    pub show_eol: bool,
    pub wrap: bool,
    pub line_numbers: bool,
    pub hl_line: bool,
    pub eol_text: &'static str,
}

pub struct Editor<'a, M> {
    pub buf: &'a Buffer,
    pub rows: &'a Rows,
    pub folds: &'a Folds,
    pub foldable: &'a [usize],
    pub hl: &'a RefCell<Highlighter>,
    pub cfg: Cfg,
    /// First visible pixel row (scroll offset in pixels).
    pub scroll_y: f32,
    pub scroll_x: f32,
    pub bracket: Option<(Pos, Pos)>,
    /// Matches to mark (start, end) in the visible lines.
    pub marks: &'a [(Pos, Pos)],
    /// Extra carets and selections (multi-caret and column mode).
    pub extra: &'a [(Pos, Pos)],
    pub bookmarks: &'a std::collections::BTreeSet<usize>,
    /// Diff marks per line: 1 added, 2 removed, 3 changed.
    pub line_marks: &'a std::collections::BTreeMap<usize, u8>,
    pub minimap: bool,
    pub on: Box<dyn Fn(Ev) -> M + 'a>,
}

#[derive(Default)]
struct State {
    cell_w: f32,
    size_font: f32,
    reported: (usize, usize, u32),
    dragging: bool,
    mm_drag: bool,
    last_click: Option<mouse::Click>,
    mods: cosmic::iced::core::keyboard::Modifiers,
}

const MM_W: f32 = 100.0;

fn line_h(font: f32) -> f32 {
    (font * 1.4).round()
}

impl<'a, M> Editor<'a, M> {
    /// First document row shown at the top of the minimap.
    fn mm_top(&self, b: Rectangle, lh: f32) -> usize {
        let fit = (b.height / 2.0) as usize;
        let total = self.rows.len();
        if total <= fit {
            return 0;
        }
        let vis = (b.height / lh).max(1.0);
        let max_scroll = ((total as f32 * lh) - vis * lh).max(1.0);
        ((self.scroll_y / max_scroll).clamp(0.0, 1.0) * (total - fit) as f32) as usize
    }

    /// The document row under a y position in the minimap.
    fn mm_row(&self, b: Rectangle, lh: f32, y: f32) -> usize {
        (self.mm_top(b, lh) + ((y - b.y).max(0.0) / 2.0) as usize).min(self.rows.len().saturating_sub(1))
    }

    fn gutter_cells(&self) -> usize {
        if !self.cfg.line_numbers {
            return 3;
        }
        let digits = self.buf.line_count().max(1).to_string().len().max(3);
        digits + 3
    }
}

fn color((r, g, b): (u8, u8, u8)) -> Color {
    Color::from_rgb8(r, g, b)
}

fn measure(renderer: &Renderer, font: f32) -> f32 {
    use cosmic::iced::core::text::Paragraph;
    let p = <Renderer as text::Renderer>::Paragraph::with_text(text::Text {
        content: "0000000000",
        bounds: Size::new(f32::INFINITY, f32::INFINITY),
        size: Pixels(font),
        line_height: text::LineHeight::default(),
        font: Font::MONOSPACE,
        align_x: text::Alignment::Default,
        align_y: cosmic::iced::alignment::Vertical::Top,
        shaping: text::Shaping::Advanced,
        wrapping: text::Wrapping::None,
        ellipsize: text::Ellipsize::None,
    });
    let _ = renderer;
    (p.min_bounds().width / 10.0).max(1.0)
}

impl<'a, M> Widget<M, Theme, Renderer> for Editor<'a, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        let st = tree.state.downcast_mut::<State>();
        if st.size_font != self.cfg.font_size || st.cell_w == 0.0 {
            st.cell_w = measure(renderer, self.cfg.font_size);
            st.size_font = self.cfg.font_size;
        }
        layout::Node::new(limits.max())
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        _viewport: &Rectangle,
    ) {
        let st = tree.state.downcast_mut::<State>();
        let b = layout.bounds();
        let lh = line_h(self.cfg.font_size);
        let gutter = self.gutter_cells() as f32 * st.cell_w;
        let mm = if self.minimap { MM_W } else { 0.0 };
        let cols = (((b.width - gutter - mm) / st.cell_w).floor().max(1.0)) as usize;
        let rws = ((b.height / lh).floor().max(1.0)) as usize;
        let key = (cols, rws, st.cell_w.to_bits());
        if st.reported != key {
            st.reported = key;
            shell.publish((self.on)(Ev::Viewport { cols, rows: rws, cell_w: st.cell_w }));
        }

        let locate = |p: Point, st: &State| -> (usize, usize) {
            let row = ((p.y - b.y + self.scroll_y) / lh).floor().max(0.0) as usize;
            let x = ((p.x - b.x - gutter + self.scroll_x) / st.cell_w).round().max(0.0) as usize;
            (row, x)
        };

        match event {
            Event::Keyboard(cosmic::iced::core::keyboard::Event::ModifiersChanged(m)) => st.mods = *m,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(p) = cursor.position_in(b).map(|q| Point::new(q.x + b.x, q.y + b.y)) else { return };
                if self.minimap && p.x > b.x + b.width - MM_W {
                    st.mm_drag = true;
                    let r = self.mm_row(b, lh, p.y);
                    shell.publish((self.on)(Ev::Minimap(r)));
                    shell.capture_event();
                    return;
                }
                let (row, x) = locate(p, st);
                let row = row.min(self.rows.len().saturating_sub(1));
                if p.x - b.x < gutter {
                    let line = self.rows.rows[row].line;
                    if p.x - b.x < st.cell_w && self.rows.rows[row].first {
                        shell.publish((self.on)(Ev::Bookmark(line)));
                        shell.capture_event();
                        return;
                    }
                    // The fold mark sits in the last two cells of the gutter.
                    if p.x - b.x > gutter - 2.0 * st.cell_w {
                        let line = self.rows.rows[row].line;
                        if self.rows.rows[row].first && (self.foldable.contains(&line) || self.folds.contains_key(&line)) {
                            shell.publish((self.on)(Ev::Fold(line)));
                            shell.capture_event();
                        }
                    }
                    return;
                }
                let click = mouse::Click::new(p, mouse::Button::Left, st.last_click);
                let clicks = match click.kind() {
                    mouse::click::Kind::Single => 1,
                    mouse::click::Kind::Double => 2,
                    mouse::click::Kind::Triple => 3,
                };
                st.last_click = Some(click);
                st.dragging = true;
                let pos = self.rows.pos_at(self.buf, row, x, self.cfg.tab);
                shell.publish((self.on)(Ev::Press { pos, clicks, extend: st.mods.shift(), alt: st.mods.alt(), ctrl: st.mods.control() }));
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) if st.mm_drag => {
                let r = self.mm_row(b, lh, position.y);
                shell.publish((self.on)(Ev::Minimap(r)));
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if st.mm_drag => {
                st.mm_drag = false;
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) if st.dragging => {
                let (row, x) = locate(*position, st);
                let pos = self.rows.pos_at(self.buf, row, x, self.cfg.tab);
                shell.publish((self.on)(Ev::Drag(pos, st.mods.alt())));
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if st.dragging => {
                st.dragging = false;
                shell.publish((self.on)(Ev::Release));
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if cursor.is_over(b) => {
                let dy = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => -y * lh * 3.0,
                    mouse::ScrollDelta::Pixels { y, .. } => -y,
                };
                shell.publish((self.on)(Ev::Scroll(dy)));
                shell.capture_event();
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let st = tree.state.downcast_ref::<State>();
        let b = layout.bounds();
        match cursor.position_in(b) {
            Some(p) if p.x >= self.gutter_cells() as f32 * st.cell_w => mouse::Interaction::Text,
            _ => mouse::Interaction::None,
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let st = tree.state.downcast_ref::<State>();
        let b = layout.bounds();
        let c = theme.cosmic();
        let dark = c.is_dark;
        let mut hl = self.hl.borrow_mut();
        if hl.is_dark() != dark {
            *hl = Highlighter::new(&hl.lang().to_string(), dark);
        }
        let fg = color(hl.fg);
        let bg: Color = c.bg_color().into();
        let faint = Color { a: 0.5, ..fg };
        let sel_bg: Color = Color { a: 0.35, ..Color::from(c.accent.base) };
        let cur_bg = Color { a: 0.07, ..fg };
        let gutter_bg: Color = c.primary(false).base.into();

        let lh = line_h(self.cfg.font_size);
        let cw = st.cell_w;
        let gutter = self.gutter_cells() as f32 * cw;
        let tab = self.cfg.tab;
        let font = Font::MONOSPACE;
        let size = Pixels(self.cfg.font_size);

        renderer.fill_quad(renderer::Quad { bounds: b, border: Border::default(), ..Default::default() }, bg);

        let first = (self.scroll_y / lh).floor().max(0.0) as usize;
        let count = (b.height / lh).ceil() as usize + 1;
        let last = (first + count).min(self.rows.len());
        let caret = self.buf.caret;
        let sel = if self.buf.has_selection() { Some(self.buf.selection()) } else { None };
        let text_clip = Rectangle { x: b.x + gutter, y: b.y, width: (b.width - gutter - if self.minimap { MM_W } else { 0.0 }).max(0.0), height: b.height };
        let draw_text = |renderer: &mut Renderer, s: &str, x: f32, y: f32, col: Color, clip: Rectangle| {
            renderer.fill_text(
                text::Text {
                    content: s.to_string(),
                    bounds: Size::new(f32::INFINITY, lh),
                    size,
                    line_height: text::LineHeight::Absolute(Pixels(lh)),
                    font,
                    align_x: text::Alignment::Default,
                    align_y: cosmic::iced::alignment::Vertical::Top,
                    shaping: text::Shaping::Basic,
                    wrapping: text::Wrapping::None,
                    ellipsize: text::Ellipsize::None,
                },
                Point::new(x, y),
                col,
                clip,
            );
        };

        renderer.with_layer(b, |renderer| {
            for ri in first..last {
                let row = self.rows.rows[ri];
                let y = b.y + ri as f32 * lh - self.scroll_y;
                let line = self.buf.line(row.line);
                let base = cell_of(line, row.from, tab);
                let x0 = b.x + gutter - self.scroll_x;
                let cell_x = |col: usize| x0 + (cell_of(line, col, tab) - base) as f32 * cw;

                // Current line.
                if self.cfg.hl_line && row.line == caret.line && !self.buf.has_selection() {
                    renderer.fill_quad(
                        renderer::Quad { bounds: Rectangle { x: b.x + gutter, y, width: b.width - gutter, height: lh }, ..Default::default() },
                        cur_bg,
                    );
                }
                if let Some(m) = self.line_marks.get(&row.line) {
                    let base_c = match m {
                        1 => c.success.base,
                        2 => c.destructive.base,
                        _ => c.warning.base,
                    };
                    renderer.fill_quad(
                        renderer::Quad { bounds: Rectangle { x: b.x + gutter, y, width: b.width - gutter, height: lh }, ..Default::default() },
                        Color { a: 0.16, ..Color::from(base_c) },
                    );
                }
                // Extra carets and selections.
                for (a, e0) in self.extra {
                    let (s, e) = if (a.line, a.col) <= (e0.line, e0.col) { (*a, *e0) } else { (*e0, *a) };
                    if s != e && s.line <= row.line && e.line >= row.line {
                        let from = if s.line == row.line { s.col.max(row.from) } else { row.from };
                        let to_col = if e.line == row.line { e.col.min(row.to) } else { row.to };
                        if to_col > from {
                            let xa = cell_x(from);
                            let xb = cell_x(to_col);
                            renderer.fill_quad(
                                renderer::Quad { bounds: Rectangle { x: xa, y, width: (xb - xa).max(1.0), height: lh }, ..Default::default() },
                                sel_bg,
                            );
                        }
                    }
                    let q = *e0;
                    if q.line == row.line && q.col >= row.from && (q.col < row.to || (q.col == row.to && row.to == line.chars().count())) {
                        renderer.fill_quad(
                            renderer::Quad { bounds: Rectangle { x: cell_x(q.col), y: y + 1.0, width: 2.0, height: lh - 2.0 }, ..Default::default() },
                            fg,
                        );
                    }
                }
                // Selection, one quad per row; the line end shows as one extra cell.
                if let Some((s, e)) = sel {
                    if s.line <= row.line && e.line >= row.line {
                        let from = if s.line == row.line { s.col.max(row.from) } else { row.from };
                        let to_col = if e.line == row.line { e.col.min(row.to) } else { row.to };
                        let ends_line = e.line > row.line && row.to == line.chars().count();
                        if to_col > from || ends_line {
                            let xa = cell_x(from);
                            let mut xb = cell_x(to_col);
                            if ends_line {
                                xb += cw;
                            }
                            renderer.fill_quad(
                                renderer::Quad { bounds: Rectangle { x: xa, y, width: (xb - xa).max(1.0), height: lh }, ..Default::default() },
                                sel_bg,
                            );
                        }
                    }
                }
                // Find marks.
                for (s, e) in self.marks {
                    if s.line == row.line && e.line == row.line && s.col >= row.from && s.col < row.to.max(row.from + 1) {
                        let xa = cell_x(s.col);
                        let xb = cell_x(e.col.min(row.to));
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: Rectangle { x: xa, y: y + 1.0, width: (xb - xa).max(2.0), height: lh - 2.0 },
                                border: Border { color: Color::from(c.warning.base), width: 1.0, radius: 2.0.into() },
                                ..Default::default()
                            },
                            Color { a: 0.18, ..Color::from(c.warning.base) },
                        );
                    }
                }
                // Matching brackets.
                if let Some((p, q)) = self.bracket {
                    for p in [p, q] {
                        if p.line == row.line && p.col >= row.from && p.col < row.to {
                            let xa = cell_x(p.col);
                            renderer.fill_quad(
                                renderer::Quad {
                                    bounds: Rectangle { x: xa, y, width: cw, height: lh },
                                    border: Border { color: Color::from(c.accent.base), width: 1.0, radius: 2.0.into() },
                                    ..Default::default()
                                },
                                Color::TRANSPARENT,
                            );
                        }
                    }
                }

                // Text: coloured runs, split at tabs and spaces when whitespace shows.
                let segs = hl.line(self.buf.lines(), row.line);
                let mut col = 0usize;
                let mut cx = cell_of(line, row.from, tab);
                let mut run = String::new();
                let mut run_x = 0.0f32;
                let mut run_col = fg;
                let flush = |renderer: &mut Renderer, run: &mut String, run_x: f32, run_col: Color| {
                    if !run.is_empty() {
                        draw_text(renderer, run, run_x, y, run_col, text_clip);
                        run.clear();
                    }
                };
                for seg in &segs {
                    let sc = color(seg.color);
                    for ch in seg.text.chars() {
                        if col >= row.from && col < row.to {
                            let w = char_cells(ch, cx, tab);
                            let px = x0 + (cx - base) as f32 * cw;
                            if ch == '\t' || ch == ' ' {
                                flush(renderer, &mut run, run_x, run_col);
                                if self.cfg.show_ws {
                                    let mark = if ch == '\t' { "→" } else { "·" };
                                    draw_text(renderer, mark, px, y, faint, text_clip);
                                }
                            } else {
                                if run.is_empty() || run_col != sc {
                                    flush(renderer, &mut run, run_x, run_col);
                                    run_x = px;
                                    run_col = sc;
                                }
                                run.push(ch);
                            }
                            cx += w;
                        }
                        col += 1;
                    }
                    flush(renderer, &mut run, run_x, run_col);
                }
                if self.cfg.show_eol && row.to == line.chars().count() {
                    draw_text(renderer, self.cfg.eol_text, x0 + (cx - base) as f32 * cw, y, faint, text_clip);
                }
                // A folded line shows a marker after its text.
                if self.folds.contains_key(&row.line) && row.to == line.chars().count() {
                    draw_text(renderer, " ⋯ ", x0 + (cx - base) as f32 * cw + cw, y, faint, text_clip);
                }

                // Caret.
                if caret.line == row.line && caret.col >= row.from && (caret.col < row.to || (caret.col == row.to && (row.to == line.chars().count()))) {
                    let xa = cell_x(caret.col);
                    renderer.fill_quad(
                        renderer::Quad { bounds: Rectangle { x: xa, y: y + 1.0, width: 2.0, height: lh - 2.0 }, ..Default::default() },
                        fg,
                    );
                }
            }

            if self.minimap {
                let mx = b.x + b.width - MM_W;
                renderer.fill_quad(
                    renderer::Quad { bounds: Rectangle { x: mx, y: b.y, width: MM_W, height: b.height }, ..Default::default() },
                    bg,
                );
                let top = self.mm_top(b, lh);
                let fit = (b.height / 2.0) as usize;
                let end = (top + fit).min(self.rows.len());
                for ri in top..end {
                    let row = self.rows.rows[ri];
                    let y = b.y + (ri - top) as f32 * 2.0;
                    let mut x = 0usize;
                    let mut start: Option<usize> = None;
                    let line = self.buf.line(row.line);
                    for (i, ch) in line.chars().enumerate().skip(row.from).take(row.to.saturating_sub(row.from)) {
                        let _ = i;
                        if x >= (MM_W as usize - 6) {
                            break;
                        }
                        if ch.is_whitespace() {
                            if let Some(s0) = start.take() {
                                renderer.fill_quad(
                                    renderer::Quad { bounds: Rectangle { x: mx + 4.0 + s0 as f32, y, width: (x - s0) as f32, height: 1.5 }, ..Default::default() },
                                    Color { a: 0.45, ..fg },
                                );
                            }
                        } else if start.is_none() {
                            start = Some(x);
                        }
                        x += 1;
                    }
                    if let Some(s0) = start {
                        renderer.fill_quad(
                            renderer::Quad { bounds: Rectangle { x: mx + 4.0 + s0 as f32, y, width: (x - s0) as f32, height: 1.5 }, ..Default::default() },
                            Color { a: 0.45, ..fg },
                        );
                    }
                }
                let vis_rows = (b.height / lh).ceil();
                let vy = b.y + (first as f32 - top as f32) * 2.0;
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle { x: mx, y: vy.max(b.y), width: MM_W, height: (vis_rows * 2.0).min(b.height) },
                        border: Border { color: Color { a: 0.5, ..Color::from(c.accent.base) }, width: 1.0, radius: 0.0.into() },
                        ..Default::default()
                    },
                    Color { a: 0.08, ..fg },
                );
            }
            // Gutter drawn last so scrolled text never shows under it.
            renderer.fill_quad(
                renderer::Quad { bounds: Rectangle { x: b.x, y: b.y, width: gutter, height: b.height }, ..Default::default() },
                gutter_bg,
            );
            let gclip = Rectangle { x: b.x, y: b.y, width: gutter, height: b.height };
            for ri in first..last {
                let row = self.rows.rows[ri];
                if !row.first {
                    continue;
                }
                let y = b.y + ri as f32 * lh - self.scroll_y;
                if self.cfg.line_numbers {
                    let n = (row.line + 1).to_string();
                    let w = self.gutter_cells() - 3;
                    let x = b.x + (w - n.len().min(w) + 1) as f32 * cw;
                    let col = if row.line == caret.line { fg } else { faint };
                    draw_text(renderer, &n, x, y, col, gclip);
                }
                if self.bookmarks.contains(&row.line) {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle { x: b.x + 0.2 * cw, y: y + lh * 0.25, width: cw * 0.7, height: lh * 0.5 },
                            border: Border { color: Color::TRANSPARENT, width: 0.0, radius: (lh * 0.25).into() },
                            ..Default::default()
                        },
                        Color::from(c.accent.base),
                    );
                }
                let mark = if self.folds.contains_key(&row.line) {
                    Some("▸")
                } else if self.foldable.contains(&row.line) {
                    Some("▾")
                } else {
                    None
                };
                if let Some(m) = mark {
                    draw_text(renderer, m, b.x + gutter - 1.8 * cw, y, faint, gclip);
                }
            }
        });
    }
}

impl<'a, M: 'a> From<Editor<'a, M>> for Element<'a, M, Theme, Renderer> {
    fn from(e: Editor<'a, M>) -> Self {
        Element::new(e)
    }
}
