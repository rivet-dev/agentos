//! Cursor state for a PTY-backed process, kept so the sidecar can answer the
//! guest's cursor-position query (`ESC [ 6 n`) the way a terminal multiplexer
//! does.
//!
//! A terminal's clients attach and detach freely and each may run its own
//! emulator. None of them can answer for the terminal: at times no client is
//! attached, and when several answer, the extra reports reach the guest as the
//! answers to its next queries. The sidecar sees every byte the guest writes,
//! in raw and cooked mode alike, so it tracks the cursor, answers each query,
//! and removes the query from the output it forwards, as tmux does. Clients
//! never see a query, so they never answer one.
//!
//! Only the cursor is tracked, not the screen: every byte costs constant work
//! and the state is a few fields plus one bit per column for tab stops, so no
//! guest output can make the sidecar do more than linear work.

use std::borrow::Cow;

use unicode_width::UnicodeWidthChar;

const CURSOR_POSITION_QUERY: &[u8] = b"\x1b[6n";

/// Guest output after the sidecar answered its cursor queries.
pub(crate) struct ObservedOutput<'a> {
    /// The output to forward to clients, without the queries.
    pub(crate) forward: Cow<'a, [u8]>,
    /// The cursor position reports to write to the terminal, in order.
    pub(crate) reports: Vec<u8>,
}

pub(crate) struct TerminalModel {
    parser: vte::Parser,
    cursor: Cursor,
    /// The start of a query at the end of the last output, not forwarded yet,
    /// so that clients never see half of a query and answer it.
    held: Vec<u8>,
}

impl TerminalModel {
    pub(crate) fn new(rows: u16, cols: u16) -> Self {
        Self {
            parser: vte::Parser::new(),
            cursor: Cursor::new(rows.max(1), cols.max(1)),
            held: Vec::new(),
        }
    }

    pub(crate) fn resize(&mut self, rows: u16, cols: u16) {
        self.cursor.resize(rows.max(1), cols.max(1));
    }

    /// Applies guest output to the cursor, answers the cursor-position queries
    /// in it, and returns the output to forward without them.
    pub(crate) fn observe_output<'a>(&mut self, output: &'a [u8]) -> ObservedOutput<'a> {
        let held = std::mem::take(&mut self.held);
        let mut performer = Performer {
            cursor: &mut self.cursor,
            query_ended: false,
        };
        let mut reports = Vec::new();
        // Byte ranges of queries, in `held` followed by `output`.
        let mut queries = Vec::new();
        let mut offset = 0;
        while offset < output.len() {
            offset += self
                .parser
                .advance_until_terminated(&mut performer, &output[offset..]);
            if std::mem::take(&mut performer.query_ended) {
                let (row, col) = performer.cursor.report();
                reports.extend_from_slice(format!("\x1b[{};{}R", row + 1, col + 1).as_bytes());
                // A CSI has no ESC inside it, so the last ESC begins the query.
                let start = output[..offset]
                    .iter()
                    .rposition(|&byte| byte == 0x1b)
                    .map(|start| held.len() + start)
                    .or_else(|| held.iter().rposition(|&byte| byte == 0x1b));
                if let Some(start) = start {
                    queries.push((start, held.len() + offset));
                }
            }
        }
        let combined_len = held.len() + output.len();
        let byte_at = |index: usize| {
            if index < held.len() {
                held[index]
            } else {
                output[index - held.len()]
            }
        };
        let forward_end = queries.last().map_or(0, |&(_, end)| end);
        let hold = (1..CURSOR_POSITION_QUERY.len())
            .rev()
            .find(|&len| {
                len <= combined_len - forward_end
                    && (0..len).all(|i| byte_at(combined_len - len + i) == CURSOR_POSITION_QUERY[i])
            })
            .unwrap_or(0);
        let kept_end = combined_len - hold;
        self.held = (kept_end..combined_len).map(byte_at).collect();
        let forward = if held.is_empty() && queries.is_empty() && hold == 0 {
            Cow::Borrowed(output)
        } else {
            let mut kept = Vec::with_capacity(kept_end);
            let mut from = 0;
            for (start, end) in queries.into_iter().chain([(kept_end, kept_end)]) {
                kept.extend((from..start).map(byte_at));
                from = end;
            }
            Cow::Owned(kept)
        };
        ObservedOutput { forward, reports }
    }
}

/// The cursor state an xterm-compatible terminal keeps, without the screen.
struct Cursor {
    rows: u16,
    cols: u16,
    row: u16,
    col: u16,
    /// The last character filled the last column; the next one wraps first.
    wrap_pending: bool,
    /// Scroll region, inclusive.
    top: u16,
    bottom: u16,
    origin_mode: bool,
    autowrap: bool,
    alternate_screen: bool,
    /// DECSC state, one per screen buffer, as in xterm. It holds the
    /// pending-wrap flag but not the autowrap mode.
    saved: [SavedCursor; 2],
    /// Width of the last printed character, for REP.
    last_width: u16,
    /// One bit per column.
    tab_stops: Vec<u64>,
}

#[derive(Clone, Copy, Default)]
struct SavedCursor {
    row: u16,
    col: u16,
    wrap_pending: bool,
    origin_mode: bool,
}

impl Cursor {
    fn new(rows: u16, cols: u16) -> Self {
        let mut cursor = Self {
            rows,
            cols,
            row: 0,
            col: 0,
            wrap_pending: false,
            top: 0,
            bottom: rows - 1,
            origin_mode: false,
            autowrap: true,
            alternate_screen: false,
            saved: [SavedCursor::default(); 2],
            last_width: 0,
            tab_stops: Vec::new(),
        };
        cursor.reset_tab_stops();
        cursor
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        self.rows = rows;
        self.cols = cols;
        self.row = self.row.min(rows - 1);
        self.col = self.col.min(cols - 1);
        self.top = 0;
        self.bottom = rows - 1;
        self.wrap_pending = false;
        self.resize_tab_stops();
    }

    /// Keeps the stops that still fit and gives new columns the default stops.
    fn resize_tab_stops(&mut self) {
        let old = std::mem::take(&mut self.tab_stops);
        let old_cols = old.len() * 64;
        self.tab_stops = vec![0; usize::from(self.cols).div_ceil(64)];
        for (word, bits) in self.tab_stops.iter_mut().zip(old) {
            *word = bits;
        }
        if let Some(last) = self.tab_stops.last_mut() {
            let used = usize::from(self.cols) % 64;
            if used != 0 {
                *last &= (1u64 << used) - 1;
            }
        }
        for col in (8..self.cols)
            .step_by(8)
            .filter(|&col| usize::from(col) >= old_cols)
        {
            self.set_tab_stop(col, true);
        }
    }

    fn reset_tab_stops(&mut self) {
        self.tab_stops = vec![0; usize::from(self.cols).div_ceil(64)];
        for col in (8..self.cols).step_by(8) {
            self.set_tab_stop(col, true);
        }
    }

    fn set_tab_stop(&mut self, col: u16, on: bool) {
        let (word, bit) = (usize::from(col) / 64, col % 64);
        if on {
            self.tab_stops[word] |= 1 << bit;
        } else {
            self.tab_stops[word] &= !(1 << bit);
        }
    }

    /// First tab stop after `col`, or the last column.
    fn next_tab_stop(&self, col: u16) -> u16 {
        let last = u32::from(self.cols - 1);
        let mut at = u32::from(col) + 1;
        while at < last {
            let word = self.tab_stops[(at / 64) as usize] >> (at % 64);
            if word != 0 {
                return (at + word.trailing_zeros()).min(last) as u16;
            }
            at = (at / 64 + 1) * 64;
        }
        last as u16
    }

    /// Last tab stop before `col`, or column 0.
    fn previous_tab_stop(&self, col: u16) -> u16 {
        let mut at = i64::from(col) - 1;
        while at > 0 {
            let word_index = (at / 64) as usize;
            let word = self.tab_stops[word_index] & (u64::MAX >> (63 - at % 64));
            if word != 0 {
                return (word_index as u32 * 64 + 63 - word.leading_zeros()) as u16;
            }
            at = (at / 64) * 64 - 1;
        }
        0
    }

    /// Line feed: down one row, staying on the bottom margin (scrolling).
    fn index(&mut self) {
        if self.row != self.bottom && self.row + 1 < self.rows {
            self.row += 1;
        }
        self.wrap_pending = false;
    }

    fn reverse_index(&mut self) {
        if self.row != self.top && self.row > 0 {
            self.row -= 1;
        }
        self.wrap_pending = false;
    }

    /// `count` line feeds.
    fn index_by(&mut self, count: u32) {
        let limit = if self.row <= self.bottom {
            self.bottom
        } else {
            self.rows - 1
        };
        self.row = (u32::from(self.row) + count).min(u32::from(limit)) as u16;
        self.wrap_pending = false;
    }

    fn print(&mut self, width: u16) {
        if width == 0 {
            return;
        }
        if self.wrap_pending && self.autowrap {
            self.col = 0;
            self.index();
        }
        if width == 2 && self.col + 1 >= self.cols {
            if !self.autowrap || self.cols == 1 {
                return;
            }
            self.col = 0;
            self.index();
        }
        self.advance_after_print(u32::from(self.col), 1, width);
    }

    /// Places the cursor after `count` characters of `width` printed from
    /// column `from` without wrapping.
    fn advance_after_print(&mut self, from: u32, count: u32, width: u16) {
        let end = from + count * u32::from(width);
        if end >= u32::from(self.cols) {
            self.col = self.cols - 1;
            self.wrap_pending = self.autowrap;
        } else {
            self.col = end as u16;
            self.wrap_pending = false;
        }
    }

    /// REP: `count` more copies of the last character, in constant time.
    fn repeat(&mut self, count: u32) {
        let width = self.last_width;
        if width == 0 || count == 0 {
            return;
        }
        let cols = u32::from(self.cols);
        if !self.autowrap || (width == 2 && cols == 1) {
            // Without wrapping the cursor stops at the last column.
            for _ in 0..count.min(cols + 1) {
                self.print(width);
            }
            return;
        }
        let per_row = cols / u32::from(width);
        let start = if self.wrap_pending {
            cols
        } else {
            u32::from(self.col)
        };
        let fit = cols.saturating_sub(start) / u32::from(width);
        if count <= fit {
            self.advance_after_print(start, count, width);
            return;
        }
        let rest = count - fit;
        self.col = 0;
        self.index_by(1 + (rest - 1) / per_row);
        self.advance_after_print(0, (rest - 1) % per_row + 1, width);
    }

    fn home(&mut self) {
        self.row = if self.origin_mode { self.top } else { 0 };
        self.col = 0;
        self.wrap_pending = false;
    }

    fn save(&mut self) {
        self.saved[usize::from(self.alternate_screen)] = SavedCursor {
            row: self.row,
            col: self.col,
            wrap_pending: self.wrap_pending,
            origin_mode: self.origin_mode,
        };
    }

    fn restore(&mut self) {
        let saved = self.saved[usize::from(self.alternate_screen)];
        self.row = saved.row.min(self.rows - 1);
        self.col = saved.col.min(self.cols - 1);
        self.wrap_pending = saved.wrap_pending && self.autowrap;
        self.origin_mode = saved.origin_mode;
    }

    fn up(&mut self, count: u16) {
        let limit = if self.row >= self.top { self.top } else { 0 };
        self.row = self.row.saturating_sub(count).max(limit);
        self.wrap_pending = false;
    }

    fn down(&mut self, count: u16) {
        let limit = if self.row <= self.bottom {
            self.bottom
        } else {
            self.rows - 1
        };
        self.row = self.row.saturating_add(count).min(limit);
        self.wrap_pending = false;
    }

    fn set_col(&mut self, col: u16) {
        self.col = col.min(self.cols - 1);
        self.wrap_pending = false;
    }

    fn set_row(&mut self, row: u16) {
        self.row = if self.origin_mode {
            self.top.saturating_add(row).min(self.bottom)
        } else {
            row.min(self.rows - 1)
        };
        self.wrap_pending = false;
    }

    /// The position a cursor position report carries, zero-based.
    fn report(&self) -> (u16, u16) {
        let row = if self.origin_mode {
            self.row.saturating_sub(self.top)
        } else {
            self.row
        };
        (row, self.col)
    }

    fn reset(&mut self) {
        *self = Self::new(self.rows, self.cols);
    }
}

struct Performer<'a> {
    cursor: &'a mut Cursor,
    query_ended: bool,
}

impl vte::Perform for Performer<'_> {
    /// Stops the parser after a query, so the caller knows where it ends.
    fn terminated(&self) -> bool {
        self.query_ended
    }

    fn print(&mut self, character: char) {
        let width = character.width().unwrap_or(0) as u16;
        if width > 0 {
            self.cursor.last_width = width;
        }
        self.cursor.print(width);
    }

    fn execute(&mut self, byte: u8) {
        let cursor = &mut *self.cursor;
        match byte {
            0x08 => {
                cursor.col = cursor.col.saturating_sub(1);
                cursor.wrap_pending = false;
            }
            0x09 => cursor.col = cursor.next_tab_stop(cursor.col),
            0x0a..=0x0c => cursor.index(),
            0x0d => {
                cursor.col = 0;
                cursor.wrap_pending = false;
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        let cursor = &mut *self.cursor;
        match (intermediates, byte) {
            ([], b'D') => cursor.index(),
            ([], b'E') => {
                cursor.col = 0;
                cursor.index();
            }
            ([], b'M') => cursor.reverse_index(),
            ([], b'7') => cursor.save(),
            ([], b'8') => cursor.restore(),
            ([], b'c') => cursor.reset(),
            ([], b'H') => cursor.set_tab_stop(cursor.col, true),
            ([b'#'], b'8') => {
                cursor.top = 0;
                cursor.bottom = cursor.rows - 1;
                cursor.origin_mode = false;
                cursor.home();
            }
            _ => {}
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        ignore: bool,
        action: char,
    ) {
        if ignore {
            return;
        }
        let cursor = &mut *self.cursor;
        let mut values = params.iter().map(|param| param[0]);
        let first = values.next().unwrap_or(0);
        let second = values.next().unwrap_or(0);
        let count = first.max(1);
        match (intermediates, action) {
            ([], 'A') => cursor.up(count),
            ([], 'B' | 'e') => cursor.down(count),
            ([], 'C' | 'a') => cursor.set_col(cursor.col.saturating_add(count)),
            ([], 'D') => cursor.set_col(cursor.col.saturating_sub(count)),
            ([], 'E') => {
                cursor.down(count);
                cursor.col = 0;
            }
            ([], 'F') => {
                cursor.up(count);
                cursor.col = 0;
            }
            ([], 'G' | '`') => cursor.set_col(count - 1),
            ([], 'd') => cursor.set_row(count - 1),
            ([], 'H' | 'f') => {
                cursor.set_row(count - 1);
                cursor.set_col(second.max(1) - 1);
            }
            ([], 'I') => {
                for _ in 0..count {
                    let next = cursor.next_tab_stop(cursor.col);
                    if next == cursor.col {
                        break;
                    }
                    cursor.col = next;
                }
            }
            ([], 'Z') => {
                for _ in 0..count {
                    let previous = cursor.previous_tab_stop(cursor.col);
                    if previous == cursor.col {
                        break;
                    }
                    cursor.col = previous;
                }
                cursor.wrap_pending = false;
            }
            ([], 'b') => cursor.repeat(u32::from(count)),
            ([], 'r') => {
                let top = first.max(1) - 1;
                let bottom = if second == 0 {
                    cursor.rows - 1
                } else {
                    (second - 1).min(cursor.rows - 1)
                };
                if top < bottom {
                    cursor.top = top;
                    cursor.bottom = bottom;
                    cursor.home();
                }
            }
            ([], 's') => cursor.save(),
            ([], 'u') => cursor.restore(),
            ([], 'L' | 'M') => {
                if cursor.row >= cursor.top && cursor.row <= cursor.bottom {
                    cursor.col = 0;
                    cursor.wrap_pending = false;
                }
            }
            ([], 'g') => match first {
                0 => cursor.set_tab_stop(cursor.col, false),
                3 => cursor.tab_stops.iter_mut().for_each(|word| *word = 0),
                _ => {}
            },
            ([], 'n') if first == 6 => self.query_ended = true,
            ([b'?'], 'h' | 'l') => {
                let on = action == 'h';
                for param in params.iter() {
                    match param[0] {
                        6 => {
                            cursor.origin_mode = on;
                            cursor.home();
                        }
                        7 => {
                            cursor.autowrap = on;
                            if !on {
                                cursor.wrap_pending = false;
                            }
                        }
                        47 | 1047 => cursor.alternate_screen = on,
                        1048 if on => cursor.save(),
                        1048 => cursor.restore(),
                        1049 if on => {
                            cursor.save();
                            cursor.alternate_screen = true;
                        }
                        1049 => {
                            cursor.alternate_screen = false;
                            cursor.restore();
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}
