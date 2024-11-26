use std::path::Path;
use std::path::PathBuf;

use crate::app::format_path;
use color_eyre::eyre::Result;
use deckard::index::FileIndex;
use ratatui::{
    buffer::Buffer,
    crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect, Rows},
    style::{Modifier, Style, Stylize},
    symbols::border,
    text::{Line, Text},
    widgets::{
        block::{Position, Title},
        Block, BorderType, Borders, Cell, HighlightSpacing, Paragraph, Row, Scrollbar,
        ScrollbarOrientation, ScrollbarState, StatefulWidget, Table, TableState, Widget,
    },
    Frame,
};

#[derive(Debug, Default)]
pub struct FileTable {
    pub table_state: TableState,
    pub table_len: usize,
    paths: Vec<PathBuf>,
    selected_path: Option<PathBuf>,
    scroll_state: ScrollbarState,
    header: Vec<&'static str>,
    // callback function that populates rows
}

impl FileTable {
    pub fn new(header: Vec<&'static str>) -> Self {
        Self {
            table_state: TableState::new(),
            table_len: 0,
            paths: Vec::new(),
            selected_path: None,
            scroll_state: ScrollbarState::new(0),
            header,
        }
    }

    pub fn update_table(&mut self, paths: &Vec<PathBuf>) {
        self.paths = paths.clone();
        self.table_len = self.paths.len();
        self.scroll_state = ScrollbarState::new(self.table_len - 1);
    }

    pub fn select_entry(&mut self, index: usize) {
        if self.table_len == 0 {
            return;
        }
        self.table_state.select(Some(index));
        self.selected_path = self.paths.get(index).map(|p| p.clone());
        self.scroll_state = self.scroll_state.position(index);
    }

    pub fn select_next(&mut self) {
        if self.table_len == 0 {
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

    pub fn render(&mut self, buf: &mut Buffer, area: Rect, focused: bool, file_index: &FileIndex) {
        let header_style = Style::default();
        let selected_style = Style::default().add_modifier(Modifier::REVERSED);

        let header = self
            .header
            .clone()
            .into_iter()
            .map(Cell::from)
            .collect::<Row>()
            .style(header_style);

        let rows = &self.paths.clone().into_iter().map(|p| {
            let path = format_path(&p, &file_index.dirs);
            let size = humansize::format_size(
                file_index.file_size(&p).unwrap_or_default(),
                humansize::DECIMAL,
            );
            let date = file_index.files[&p].modified;

            let cells = vec![
                Cell::from(Text::from(format!("{path}"))),
                Cell::from(Text::from(format!("{date}"))),
                Cell::from(Text::from(format!("{size}"))),
                Cell::from(Text::from(format!(" "))),
            ];
            cells.into_iter().collect::<Row>().style(Style::new())
        });
        let block;
        if focused {
            block = Block::bordered()
                // .title(" Clones ")
                .border_type(BorderType::Thick)
                .border_style(Style::new().green());
        } else {
            block = Block::bordered()
                .border_type(BorderType::Plain)
                .border_style(Style::new().dark_gray());
        };
        let table = Table::new(
            rows.clone(),
            [
                // + 1 is for padding.
                Constraint::Min(10),
                Constraint::Max(10),
                Constraint::Max(12),
                Constraint::Max(1),
            ],
        )
        .header(header)
        .highlight_style(selected_style)
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
