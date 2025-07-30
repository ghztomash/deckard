use crate::app::format_path;
use deckard::index::FileIndex;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Margin, Rect},
    style::{Color, Style, Styled, Stylize},
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
};

#[derive(Debug, Default)]
pub struct FileTable {
    pub table_state: TableState,
    pub table_len: usize,
    paths: Vec<PathBuf>,
    selected_path: Option<PathBuf>,
    scroll_state: ScrollbarState,
    header: Vec<&'static str>,
    mark_marked: bool,
    show_clone_count: bool,
    // callback function that populates rows
}

impl FileTable {
    pub fn new(header: Vec<&'static str>, mark_marked: bool, show_clone_count: bool) -> Self {
        Self {
            table_state: TableState::new(),
            table_len: 0,
            paths: Vec::new(),
            selected_path: None,
            scroll_state: ScrollbarState::new(0),
            header,
            mark_marked,
            show_clone_count,
        }
    }

    pub fn clear(&mut self) {
        self.table_state = TableState::new();
        self.table_len = 0;
        self.paths = Vec::new();
        self.selected_path = None;
        self.scroll_state = ScrollbarState::new(0);
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        self.paths.clone()
    }

    pub fn update_table(&mut self, paths: &Vec<PathBuf>) {
        self.paths = paths.to_owned();
        self.table_len = self.paths.len();
        self.scroll_state = ScrollbarState::new(self.table_len.saturating_sub(1));
    }

    pub fn select_entry(&mut self, index: usize) {
        if self.table_len == 0 {
            self.select_none();
            return;
        }
        self.table_state.select(Some(index));
        self.selected_path = self.paths.get(index).cloned();
        self.scroll_state = self.scroll_state.position(index);
    }

    pub fn select_next(&mut self) {
        if self.table_len == 0 {
            self.select_none();
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i >= self.table_len - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.select_entry(i);
    }

    pub fn select_previous(&mut self) {
        if self.table_len == 0 {
            self.select_none();
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.table_len - 1
                } else {
                    i - 1
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

    pub fn selected_path(&self) -> Option<PathBuf> {
        self.selected_path.clone()
    }

    pub fn render(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        focused: bool,
        file_index: &Arc<RwLock<FileIndex>>,
        marked_files: &HashSet<PathBuf>,
    ) {
        let header_style = Style::default().dark_gray();
        let selected_style = if focused {
            Style::default().fg(Color::Black).bg(Color::White)
        } else {
            Style::default().fg(Color::Black).bg(Color::DarkGray)
        };
        let footer_style = Style::default().dark_gray();

        let header = self
            .header
            .clone()
            .into_iter()
            .map(Cell::from)
            .collect::<Row>()
            .style(header_style);

        let count = self.paths.len();

        // Lock the FileIndex only once, then copy out the data we need:
        let (dirs, meta_for_paths, total_size_raw) = {
            let fi = file_index.read().unwrap();

            // Pre-calculate file metadata for each path we display,
            // including size & date. Also track a sum to show total size.
            let dirs = fi.dirs.clone();
            let mut total_size_acc = 0u64;
            let mut meta_vec = Vec::with_capacity(count);
            for path in &self.paths {
                let size = fi.file_size(path).unwrap_or_default();
                let date = fi.file_date_modified(path).unwrap_or_default(); // or created
                let clone_count = fi.file_duplicates_len(path).unwrap_or_default();
                total_size_acc += size;

                meta_vec.push((path.clone(), size, date, clone_count));
            }
            (dirs, meta_vec, total_size_acc)
        };

        let total_size = humansize::format_size(total_size_raw, humansize::DECIMAL);

        let rows = meta_for_paths
            .into_iter()
            .map(|(p, size, date, clone_count)| {
                let path = format_path(&p, &dirs);
                let size = humansize::format_size(size, humansize::DECIMAL);
                let date = date.format("%d/%m/%Y");
                let is_marked = marked_files.contains(&p);

                let path_style = if self.mark_marked && is_marked {
                    Style::new().yellow()
                } else {
                    Style::new()
                };

                let mut cells = vec![
                    Cell::from(Text::from(if self.mark_marked && is_marked {
                        "*"
                    } else {
                        " "
                    })),
                    Cell::from(Text::from(path.set_style(path_style))),
                    Cell::from(Text::from(format!("{date}"))),
                    Cell::from(Text::from(size)),
                ];
                if self.show_clone_count {
                    cells.push(Cell::from(Text::from(clone_count.to_string())));
                }
                cells.into_iter().collect::<Row>().style(Style::new())
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

        let footer = Row::new(vec![
            Cell::from(Text::from("")),
            Cell::from(Text::from(format!("Files: {count}"))),
            Cell::from(Text::from("Total:")),
            Cell::from(Text::from(total_size.to_string())),
        ])
        .style(footer_style);

        let mut widths = vec![
            // + 1 is for padding.
            Constraint::Max(1),
            Constraint::Min(10),
            Constraint::Max(11),
            Constraint::Max(11),
        ];
        if self.show_clone_count {
            widths.push(Constraint::Max(7));
        }

        let table = Table::new(rows.clone(), widths)
            .header(header)
            .footer(footer)
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
