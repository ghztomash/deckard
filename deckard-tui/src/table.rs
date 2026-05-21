use crate::app::Sorting;
use chrono::{DateTime, Local};
use deckard::index::FileIndex;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Margin, Rect},
    style::{Color, Style, Stylize},
    text::Span,
    widgets::{
        Block, BorderType, Cell, Row, Scrollbar, ScrollbarOrientation, ScrollbarState,
        StatefulWidget, Table, TableState,
    },
};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, RwLock},
    time::SystemTime,
};

#[derive(Debug, Default, Clone)]
pub struct FileTableEntry {
    path: Arc<PathBuf>,
    display_path: String,
    size: u64,
    size_text: String,
    date: Option<SystemTime>,
    date_text: String,
    clone_count: usize,
    clone_count_text: String,
}

impl FileTableEntry {
    fn to_row(&self, mark_marked: bool, is_marked: bool, show_clone_count: bool) -> Row<'_> {
        let path_style = if mark_marked && is_marked {
            Style::new().yellow()
        } else {
            Style::new()
        };

        let mut cells = vec![
            Cell::from(if mark_marked && is_marked { "*" } else { " " }),
            Cell::from(Span::styled(self.display_path.as_str(), path_style)),
            Cell::from(self.date_text.as_str()),
            Cell::from(self.size_text.as_str()),
        ];

        if show_clone_count {
            cells.push(Cell::from(self.clone_count_text.as_str()));
        }

        Row::new(cells).style(Style::new())
    }
}

#[derive(Debug, Default)]
pub struct FileTable {
    pub table_state: TableState,
    pub table_len: usize,
    entries: Vec<FileTableEntry>,
    selected_path: Option<Arc<PathBuf>>,
    scroll_state: ScrollbarState,
    mark_marked: bool,
    show_clone_count: bool,
    total_size: u64,
    header_labels: Vec<&'static str>,
    widths: Vec<Constraint>,
    file_count_text: String,
    total_size_text: String,
}

impl FileTable {
    pub fn new(header_str: Vec<&'static str>, mark_marked: bool, show_clone_count: bool) -> Self {
        let mut widths = vec![
            // + 1 is for padding.
            Constraint::Max(1),
            Constraint::Min(10),
            Constraint::Max(11),
            Constraint::Max(11),
        ];
        if show_clone_count {
            widths.push(Constraint::Max(7));
        }

        Self {
            table_state: TableState::new(),
            table_len: 0,
            total_size: 0,
            entries: Vec::new(),
            selected_path: None,
            scroll_state: ScrollbarState::new(0),
            mark_marked,
            show_clone_count,
            header_labels: header_str,
            widths,
            file_count_text: String::new(),
            total_size_text: String::new(),
        }
    }

    pub fn clear(&mut self) {
        self.table_state = TableState::new();
        self.table_len = 0;
        self.entries = Vec::new();
        self.selected_path = None;
        self.scroll_state = ScrollbarState::new(0);
    }

    pub fn paths(&self) -> Vec<Arc<PathBuf>> {
        self.entries.iter().map(|e| e.path.clone()).collect()
    }

    pub fn update_table(
        &mut self,
        paths: &Vec<Arc<PathBuf>>,
        file_index: &Arc<RwLock<FileIndex>>,
        sort_by: Option<&Sorting>,
    ) {
        // Lock the FileIndex only once, then copy out the data we need:
        let (mut entries, total_size) = {
            let fi = file_index.read().unwrap();
            let common_path = deckard::find_common_path(&fi.dirs);

            // Pre-calculate file metadata for each path we display,
            // including size & date. Also track a sum to show total size.
            let mut total_size_acc = 0u64;
            let mut entries_vec = Vec::with_capacity(paths.len());
            for path in paths {
                let size = fi.file_size(path).unwrap_or_default();
                let date = fi.file_date_modified(path); // or created
                let display_path = format_path_with_common(path, common_path.as_ref());
                let size_text = humansize::format_size(size, humansize::DECIMAL);
                let date_text = date
                    .map(|d| DateTime::<Local>::from(d).format("%d/%m/%Y").to_string())
                    .unwrap_or_default();
                let clone_count = fi.file_duplicates_len(path).unwrap_or_default();
                let clone_count_text = clone_count.to_string();
                total_size_acc += size;

                entries_vec.push(FileTableEntry {
                    path: path.clone(),
                    display_path,
                    size,
                    size_text,
                    date,
                    date_text,
                    clone_count,
                    clone_count_text,
                });
            }

            (entries_vec, total_size_acc)
        };

        // Sort the paths
        if let Some(sort_by) = sort_by {
            entries.sort_by(|a, b| match sort_by {
                Sorting::Path => a.path.cmp(&b.path),
                Sorting::Size => b.size.cmp(&a.size),
                Sorting::Date => b.date.cmp(&a.date),
                Sorting::Count => b.clone_count.cmp(&a.clone_count),
            });
        }

        self.entries = entries;
        self.table_len = self.entries.len();
        self.total_size = total_size;
        self.scroll_state = ScrollbarState::new(self.table_len);

        self.file_count_text = format!("Files: {}", self.table_len);
        self.total_size_text = humansize::format_size(total_size, humansize::DECIMAL);
    }

    pub fn select_entry(&mut self, index: usize) {
        if self.table_len == 0 {
            self.select_none();
            return;
        }
        let index = index.min(self.table_len.saturating_sub(1));
        self.table_state.select(Some(index));
        self.selected_path = self.entries.get(index).map(|e| e.path.to_owned());
    }

    // Select step entries down
    pub fn select_next(&mut self, step: usize) {
        if self.table_len == 0 {
            self.select_none();
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i >= self.table_len.saturating_sub(1) {
                    0
                } else if i >= self.table_len.saturating_sub(step) {
                    self.table_len.saturating_sub(1)
                } else {
                    i.saturating_add(step)
                }
            }
            None => 0,
        };
        self.select_entry(i);
    }

    /// Select step entries up
    pub fn select_previous(&mut self, step: usize) {
        if self.table_len == 0 {
            self.select_none();
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.table_len.saturating_sub(1)
                } else {
                    i.saturating_sub(step)
                }
            }
            None => 0,
        };
        self.select_entry(i);
    }

    pub fn select_first(&mut self) {
        self.select_entry(0);
    }

    pub fn select_none(&mut self) {
        self.table_state.select(None);
        self.selected_path = None;
    }

    pub fn selected_path(&self) -> Option<Arc<PathBuf>> {
        self.selected_path.clone()
    }

    pub fn render(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        focused: bool,
        marked_files: &HashSet<Arc<PathBuf>>,
    ) {
        let visible = self.visible_range(area);
        let rows = self.entries[visible.start..visible.end].iter().map(|e| {
            let is_marked = marked_files.contains(&e.path);
            e.to_row(self.mark_marked, is_marked, self.show_clone_count)
        });

        let block = if focused {
            Block::bordered()
                .border_type(BorderType::Thick)
                .border_style(Style::new().light_magenta())
        } else {
            Block::bordered()
                .border_type(BorderType::Plain)
                .border_style(Style::new().dark_gray())
        };

        let selected_style = if focused {
            Style::default().fg(Color::Black).bg(Color::White)
        } else {
            Style::default().fg(Color::Black).bg(Color::DarkGray)
        };

        let header = self.header();
        let footer = self.footer();
        let table = Table::default()
            .widths(self.widths.iter().copied())
            .header(header)
            .rows(rows)
            .footer(footer)
            .row_highlight_style(selected_style)
            .block(block);
        let mut visible_state = TableState::new()
            .with_selected(visible.selected)
            .with_offset(0);
        StatefulWidget::render(table, area, buf, &mut visible_state);

        let scrollbar = Scrollbar::default().orientation(ScrollbarOrientation::VerticalRight);
        self.scroll_state = self
            .scroll_state
            .content_length(self.table_len)
            .viewport_content_length(visible.row_capacity)
            .position(visible.scroll_position);
        scrollbar.render(
            area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            buf,
            &mut self.scroll_state,
        );
    }

    fn header(&self) -> Row<'_> {
        self.header_labels
            .iter()
            .copied()
            .map(Cell::from)
            .collect::<Row>()
            .style(Style::default().dark_gray())
    }

    fn footer(&self) -> Row<'_> {
        Row::new(vec![
            Cell::from(""),
            Cell::from(self.file_count_text.as_str()),
            Cell::from("Total:"),
            Cell::from(self.total_size_text.as_str()),
        ])
        .style(Style::default().dark_gray())
    }

    fn visible_range(&mut self, area: Rect) -> VisibleRange {
        let row_capacity = row_capacity(area);
        if self.table_len == 0 || row_capacity == 0 {
            *self.table_state.offset_mut() = 0;
            return VisibleRange {
                start: 0,
                end: 0,
                selected: None,
                row_capacity,
                scroll_position: 0,
            };
        }

        let global_selected = self
            .table_state
            .selected()
            .map(|i| i.min(self.table_len.saturating_sub(1)));
        self.table_state.select(global_selected);

        let start = clamp_offset(
            self.table_state.offset(),
            global_selected,
            row_capacity,
            self.table_len,
        );
        *self.table_state.offset_mut() = start;

        let end = start.saturating_add(row_capacity).min(self.table_len);
        let selected = global_selected.and_then(|index| {
            if (start..end).contains(&index) {
                Some(index - start)
            } else {
                None
            }
        });

        VisibleRange {
            start,
            end,
            selected,
            row_capacity,
            scroll_position: global_selected.unwrap_or(start),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VisibleRange {
    start: usize,
    end: usize,
    selected: Option<usize>,
    row_capacity: usize,
    scroll_position: usize,
}

fn row_capacity(area: Rect) -> usize {
    area.height.saturating_sub(4) as usize
}

fn clamp_offset(
    offset: usize,
    selected: Option<usize>,
    row_capacity: usize,
    table_len: usize,
) -> usize {
    if table_len == 0 || row_capacity == 0 {
        return 0;
    }

    let max_start = table_len.saturating_sub(row_capacity);
    let mut start = offset.min(max_start);

    if let Some(selected) = selected {
        let selected = selected.min(table_len.saturating_sub(1));
        if selected < start {
            start = selected;
        } else if selected >= start.saturating_add(row_capacity) {
            start = selected.saturating_add(1).saturating_sub(row_capacity);
        }
    }

    start.min(max_start)
}

fn format_path_with_common(path: &PathBuf, common_path: Option<&PathBuf>) -> String {
    let relative_path = if let Some(common_path) = common_path {
        path.strip_prefix(common_path).unwrap_or(path)
    } else {
        path
    };
    relative_path.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(index: usize) -> FileTableEntry {
        FileTableEntry {
            path: Arc::new(PathBuf::from(format!("/tmp/file-{index}"))),
            display_path: format!("file-{index}"),
            size: index as u64,
            size_text: format!("{index} B"),
            date: None,
            date_text: String::new(),
            clone_count: index,
            clone_count_text: index.to_string(),
        }
    }

    #[test]
    fn row_capacity_accounts_for_table_chrome() {
        assert_eq!(row_capacity(Rect::new(0, 0, 80, 8)), 4);
        assert_eq!(row_capacity(Rect::new(0, 0, 80, 3)), 0);
    }

    #[test]
    fn clamp_offset_keeps_selection_visible_below_viewport() {
        assert_eq!(clamp_offset(0, Some(9), 4, 20), 6);
    }

    #[test]
    fn clamp_offset_keeps_selection_visible_above_viewport() {
        assert_eq!(clamp_offset(10, Some(3), 4, 20), 3);
    }

    #[test]
    fn clamp_offset_limits_start_near_end() {
        assert_eq!(clamp_offset(99, Some(99), 8, 100), 92);
    }

    #[test]
    fn visible_range_uses_relative_selection() {
        let mut table = FileTable::new(vec![" ", "File", "Date", "Size"], true, false);
        table.table_len = 20;
        table.table_state.select(Some(9));

        let visible = table.visible_range(Rect::new(0, 0, 80, 8));

        assert_eq!(
            visible,
            VisibleRange {
                start: 6,
                end: 10,
                selected: Some(3),
                row_capacity: 4,
                scroll_position: 9,
            }
        );
        assert_eq!(table.table_state.offset(), 6);
        assert_eq!(table.table_state.selected(), Some(9));
    }

    #[test]
    fn visible_range_scroll_position_tracks_selected_row() {
        let mut table = FileTable::new(vec![" ", "File", "Date", "Size"], true, false);
        table.table_len = 20;
        table.table_state.select(Some(19));

        let visible = table.visible_range(Rect::new(0, 0, 80, 8));

        assert_eq!(visible.start, 16);
        assert_eq!(visible.selected, Some(3));
        assert_eq!(visible.scroll_position, 19);
    }

    #[test]
    fn render_large_table_uses_visible_slice() {
        let mut table = FileTable::new(vec![" ", "File", "Date", "Size"], true, false);
        table.entries = (0..10_000).map(entry).collect();
        table.table_len = table.entries.len();
        table.select_entry(5_000);

        let area = Rect::new(0, 0, 80, 8);
        let mut buf = Buffer::empty(area);
        table.render(&mut buf, area, true, &HashSet::new());

        let output = buf
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(output.contains("file-5000"));
        assert!(!output.contains("file-0"));
        assert_eq!(table.table_state.offset(), 4_997);
    }
}
