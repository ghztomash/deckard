use crate::app::{Sorting, format_path};
use chrono::{DateTime, Local};
use deckard::index::FileIndex;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Margin, Rect},
    style::{Color, Modifier, Style, Styled, Stylize},
    text::Text,
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
    date: Option<SystemTime>,
    clone_count: usize,
}

impl FileTableEntry {
    fn to_row(&self, mark_marked: bool, is_marked: bool, show_clone_count: bool) -> Row<'_> {
        let size = humansize::format_size(self.size, humansize::DECIMAL);
        let date = self
            .date
            .map(|d| DateTime::<Local>::from(d).format("%d/%m/%Y").to_string())
            .unwrap_or_default();

        let path_style = if mark_marked && is_marked {
            Style::new().yellow()
        } else {
            Style::new()
        };

        let mut cells = vec![
            Cell::from(Text::from(if mark_marked && is_marked { "*" } else { " " })),
            Cell::from(Text::from(self.display_path.clone().set_style(path_style))),
            Cell::from(Text::from(date)),
            Cell::from(Text::from(size)),
        ];

        if show_clone_count {
            cells.push(Cell::from(Text::from(self.clone_count.to_string())));
        }

        Row::new(cells).style(Style::new())
    }
}

#[derive(Debug, Default)]
pub struct FileTable<'a> {
    pub table_state: TableState,
    pub table_len: usize,
    entries: Vec<FileTableEntry>,
    selected_path: Option<Arc<PathBuf>>,
    scroll_state: ScrollbarState,
    mark_marked: bool,
    show_clone_count: bool,
    total_size: u64,
    // from draw
    table: Table<'a>,
    footer: Row<'a>,
}

impl FileTable<'_> {
    pub fn new(header_str: Vec<&'static str>, mark_marked: bool, show_clone_count: bool) -> Self {
        let header_style = Style::default().dark_gray();
        let header = header_str
            .into_iter()
            .map(Cell::from)
            .collect::<Row>()
            .style(header_style);

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

        let table = Table::default().widths(widths).header(header);

        Self {
            table_state: TableState::new(),
            table_len: 0,
            total_size: 0,
            entries: Vec::new(),
            selected_path: None,
            scroll_state: ScrollbarState::new(0),
            mark_marked,
            show_clone_count,
            table,
            footer: Row::default(),
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

            // Pre-calculate file metadata for each path we display,
            // including size & date. Also track a sum to show total size.
            let mut total_size_acc = 0u64;
            let mut entries_vec = Vec::with_capacity(paths.len());
            for path in paths {
                let size = fi.file_size(path).unwrap_or_default();
                let date = fi.file_date_modified(path); // or created
                let display_path = format_path(path, &fi.dirs).display().to_string();
                let clone_count = fi.file_duplicates_len(path).unwrap_or_default();
                total_size_acc += size;

                entries_vec.push(FileTableEntry {
                    path: path.clone(),
                    display_path,
                    size,
                    date,
                    clone_count,
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
        self.scroll_state = ScrollbarState::new(self.table_len.saturating_sub(1));

        // from draw
        let footer_style = Style::default().dark_gray();
        let total_size_str = humansize::format_size(total_size, humansize::DECIMAL);

        self.footer = Row::new(vec![
            Cell::from(Text::from("")),
            Cell::from(Text::from(format!("Files: {}", self.table_len))),
            Cell::from(Text::from("Total:")),
            Cell::from(Text::from(total_size_str)),
        ])
        .style(footer_style);
    }

    pub fn select_entry(&mut self, index: usize) {
        if self.table_len == 0 {
            self.select_none();
            return;
        }
        self.table_state.select(Some(index));
        self.selected_path = self.entries.get(index).map(|e| e.path.to_owned());
        self.scroll_state = self.scroll_state.position(index);
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
        let height = area.height.saturating_sub(3) as usize;
        let offset = self.table_state.offset();

        let rows = self.entries.iter().enumerate().map(|(i, e)| {
            if i >= offset.saturating_sub(height)
                && i < offset.saturating_add(height.saturating_mul(2))
            {
                let is_marked = marked_files.contains(&e.path);
                e.to_row(self.mark_marked, is_marked, self.show_clone_count)
            } else {
                Row::new::<Vec<Cell>>(vec![]).style(Style::new())
            }
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
            Style::default().fg(Color::Black).bg(Color::LightGreen).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Black).bg(Color::DarkGray)
        };

        let table = self
            .table
            .clone()
            .rows(rows)
            .footer(self.footer.clone())
            .row_highlight_style(selected_style)
            .block(block);
        StatefulWidget::render(table, area, buf, &mut self.table_state);

        let scrollbar = Scrollbar::default().orientation(ScrollbarOrientation::VerticalRight);
        scrollbar.render(
            area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            buf,
            &mut self.scroll_state,
        );
    }
}
