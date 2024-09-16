use std::{borrow::Cow, collections::HashSet, env, path::PathBuf};

use color_eyre::{
    eyre::{bail, Result, WrapErr},
    owo_colors::OwoColorize,
};
use config::SearchConfig;
use ratatui::{
    buffer::Buffer,
    crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
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

use deckard::index::FileIndex;
use deckard::*;

#[derive(Debug, Default)]
pub struct App {
    counter: u8,
    exit: bool,
    file_index: FileIndex,
    scroll_state: ScrollbarState,
    file_table_state: TableState,
    clone_table_state: TableState,
    selected_file: Option<PathBuf>,
}

impl App {
    pub fn new(target_paths: HashSet<PathBuf>, config: SearchConfig) -> Self {
        Self {
            counter: 0,
            exit: false,
            file_index: FileIndex::new(target_paths, config),
            scroll_state: ScrollbarState::new(0),
            file_table_state: TableState::new(),
            clone_table_state: TableState::new(),
            selected_file: None,
        }
    }

    /// runs the application's main loop until the user quits
    pub fn run(&mut self, terminal: &mut crate::tui::Tui) -> Result<()> {
        self.file_index.index_dirs();
        self.file_index.process_files();
        self.file_index.find_duplicates();

        // update
        self.scroll_state = ScrollbarState::new(self.file_index.duplicates_len() - 1);
        // self.file_table_state = TableState::default().with_selected(0);

        while !self.exit {
            terminal.draw(|frame| self.render_ui(frame.area(), frame.buffer_mut()))?;
            self.handle_events().wrap_err("handle events failed")?;
        }
        Ok(())
    }

    /// updates the application's state based on user input
    fn handle_events(&mut self) -> Result<()> {
        match event::read()? {
            // it's important to check that the event is a key press event as
            // crossterm also emits key release and repeat events on Windows.
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => self
                .handle_key_event(key_event)
                .wrap_err_with(|| format!("handling key event failed:\n{key_event:#?}")),
            _ => Ok(()),
        }
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> Result<()> {
        match key_event.code {
            KeyCode::Char('q') | KeyCode::Esc => self.exit(),
            KeyCode::Char('j') | KeyCode::Down => self.next(),
            KeyCode::Char('k') | KeyCode::Up => self.previous(),
            KeyCode::Char('l') | KeyCode::Right => self.increment_counter()?,
            KeyCode::Char('h') | KeyCode::Left => self.decrement_counter()?,
            _ => {}
        }
        Ok(())
    }

    fn exit(&mut self) {
        self.exit = true;
    }

    fn decrement_counter(&mut self) -> Result<()> {
        self.counter -= 1;
        Ok(())
    }

    fn increment_counter(&mut self) -> Result<()> {
        self.counter += 1;
        if self.counter > 2 {
            bail!("counter overflow");
        }
        Ok(())
    }

    pub fn next(&mut self) {
        let i = match self.file_table_state.selected() {
            Some(i) => {
                if i >= self.file_index.duplicates_len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.file_table_state.select(Some(i));
        self.selected_file = self
            .file_index
            .duplicates
            .keys()
            .collect::<Vec<&PathBuf>>()
            .get(i)
            .map(|&p| p.clone());
        self.scroll_state = self.scroll_state.position(i);
    }

    pub fn previous(&mut self) {
        let i = match self.file_table_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.file_index.duplicates_len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.file_table_state.select(Some(i));
        self.selected_file = self
            .file_index
            .duplicates
            .keys()
            .collect::<Vec<&PathBuf>>()
            .get(i)
            .map(|&p| p.clone());
        self.scroll_state = self.scroll_state.position(i);
    }

    fn render_table(&mut self, buf: &mut Buffer, area: Rect) {
        let header_style = Style::default().add_modifier(Modifier::BOLD);
        let selected_style = Style::default().add_modifier(Modifier::REVERSED);

        let header = ["File", "Duplicates", "Size"]
            .into_iter()
            .map(Cell::from)
            .collect::<Row>()
            .style(header_style)
            .height(1);

        let duplicates = &self.file_index.duplicates;
        let common_path = deckard::find_common_path(self.file_index.dirs.clone());
        let dirs: Vec<PathBuf> = self.file_index.dirs.clone().into_iter().collect();

        // convert paths to lines of text
        let files: Vec<String> = duplicates
            .keys()
            .map(|k| {
                if let Some(common_path) = &common_path {
                    let k = k.strip_prefix(&common_path).unwrap_or(k);
                    return k.to_string_lossy().to_string();
                }
                k.to_string_lossy().to_string()
            })
            .collect();

        let rows = duplicates.keys().into_iter().map(|k| {
            let path = if let Some(common_path) = &common_path {
                let k = k.strip_prefix(&common_path).unwrap_or(k);
                k.to_string_lossy().to_string()
            } else {
                k.to_string_lossy().to_string()
            };
            let size = self.file_index.file_size_string(k).unwrap_or_default();
            let duplicates = duplicates[k].len();

            let cells = vec![
                Cell::from(Text::from(format!("{path}"))),
                Cell::from(Text::from(format!("{duplicates}").yellow())),
                Cell::from(Text::from(format!("{size}").blue())),
            ];
            cells
                .into_iter()
                .collect::<Row>()
                .style(Style::new())
                .height(1)
        });
        let bar = " - ";
        let table = Table::new(
            rows,
            [
                // + 1 is for padding.
                Constraint::Min(10),
                Constraint::Max(10),
                Constraint::Max(12),
            ],
        )
        .header(header)
        .highlight_style(selected_style)
        .highlight_symbol(Text::from(vec![bar.into()]))
        .highlight_spacing(HighlightSpacing::Always);

        StatefulWidget::render(table, area, buf, &mut self.file_table_state);
    }

    fn render_scrollbar(&mut self, buf: &mut Buffer, area: Rect) {
        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None);
        scrollbar.render(
            area.inner(Margin {
                vertical: 1,
                horizontal: 1,
            }),
            buf,
            &mut self.scroll_state,
        );
    }

    fn render_clones_table(&mut self, buf: &mut Buffer, area: Rect) {
        let header_style = Style::default();
        let selected_style = Style::default().add_modifier(Modifier::REVERSED);

        let header = ["File", "Date", "Size"]
            .into_iter()
            .map(Cell::from)
            .collect::<Row>()
            .style(header_style)
            .height(1);

        let selected_file = self.selected_file.as_ref();
        if selected_file.is_none() {
            return ();
        }

        let duplicates = &self
            .file_index
            .duplicates
            .get(selected_file.unwrap())
            .unwrap();
        let common_path = deckard::find_common_path(self.file_index.dirs.clone());

        let rows = duplicates.into_iter().map(|k| {
            let path = if let Some(common_path) = &common_path {
                let k = k.strip_prefix(&common_path).unwrap_or(k);
                k.to_string_lossy().to_string()
            } else {
                k.to_string_lossy().to_string()
            };
            let size = self.file_index.file_size_string(k).unwrap_or_default();
            // let duplicates = duplicates[k].len();

            let cells = vec![
                Cell::from(Text::from(format!("{path}"))),
                Cell::from(Text::from(format!("0").red())),
                Cell::from(Text::from(format!("{size}").blue())),
            ];
            cells
                .into_iter()
                .collect::<Row>()
                .style(Style::new())
                .height(1)
        });
        let bar = " - ";
        let table = Table::new(
            rows,
            [
                // + 1 is for padding.
                Constraint::Min(10),
                Constraint::Max(10),
                Constraint::Max(12),
            ],
        )
        .header(header)
        .highlight_style(selected_style)
        .highlight_symbol(Text::from(vec![bar.into()]))
        .highlight_spacing(HighlightSpacing::Always);

        StatefulWidget::render(table, area, buf, &mut self.clone_table_state);
    }

    fn render_header(&self, buf: &mut Buffer, area: Rect) {
        let title = Line::from("Deckard");
        let header = Paragraph::new(title).style(Style::new()).centered().block(
            Block::bordered()
                .border_type(BorderType::Double)
                .border_style(Style::new()),
        );
        header.render(area, buf)
    }

    fn render_summary(&self, buf: &mut Buffer, area: Rect) {
        let common_path = deckard::find_common_path(self.file_index.dirs.clone());
        let dirs: Vec<PathBuf> = self.file_index.dirs.clone().into_iter().collect();

        let dir_lines: Vec<String> = dirs
            .iter()
            .map(|d| {
                if let Some(common_path) = &common_path {
                    format!(
                        "{}",
                        to_relative_path(d.clone())
                            // d.strip_prefix(&common_path)
                            // .unwrap_or(d)
                            .to_string_lossy()
                            .to_string()
                            .yellow(),
                    )
                } else {
                    format!("{}", d.to_string_lossy().to_string().yellow())
                }
            })
            .collect();

        let dir_joined = dir_lines.join(" ");

        let duplicate_lines = vec![
            Line::from(vec![
                "Total duplicate files: ".into(),
                self.file_index.duplicates_len().to_string().green(),
            ]),
            Line::from(vec!["Search Paths: ".into(), dir_joined.yellow()]),
        ];
        // duplicate_lines.extend(dir_lines);

        let duplicates_text = Text::from(duplicate_lines);

        let summary = Paragraph::new(duplicates_text).style(Style::new()).block(
            Block::bordered()
                .border_type(BorderType::Double)
                .border_style(Style::new()),
        );
        summary.render(area, buf)
    }

    fn render_footer(&self, buf: &mut Buffer, area: Rect) {
        let instructions = Line::from(vec![
            " Decrement ".into(),
            "<Left>".blue().bold(),
            " Increment ".into(),
            "<Right>".blue().bold(),
            " Quit ".into(),
            "<Q> ".blue().bold(),
        ]);
        let info_footer = Paragraph::new(instructions).style(Style::new()).block(
            Block::bordered()
                .border_type(BorderType::Double)
                .border_style(Style::new()),
        );
        info_footer.render(area, buf)
    }
}

impl App {
    fn render_ui(&mut self, area: Rect, buf: &mut Buffer) {
        let rects = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Max(5),
            Constraint::Length(3),
        ])
        .split(area);

        // let title = Title::from(" Deckard ".bold());
        // let instructions = Title::from(Line::from(vec![
        //     " Decrement ".into(),
        //     "<Left>".blue().bold(),
        //     " Increment ".into(),
        //     "<Right>".blue().bold(),
        //     " Quit ".into(),
        //     "<Q> ".blue().bold(),
        // ]));
        // let block = Block::default()
        //     .title(title.alignment(Alignment::Center))
        //     .title(
        //         instructions
        //             .alignment(Alignment::Center)
        //             .position(Position::Bottom),
        //     )
        //     .borders(Borders::ALL)
        //     .border_set(border::THICK);
        // block.render(area, buf);
        self.render_header(buf, rects[0]);

        // let duplicates = &self.file_index.duplicates;

        // convert paths to lines of text
        // let files: Vec<Line> = duplicates
        //     .keys()
        //     .map(|k| {
        //         if let Some(common_path) = &common_path {
        //             let k = k.strip_prefix(&common_path).unwrap_or(k);
        //             return Line::from(k.to_string_lossy().to_string());
        //         }
        //         Line::from(k.to_string_lossy().to_string())
        //     })
        //     .collect();

        // let files_text = Text::from(files);

        let area = area.inner(Margin {
            horizontal: 1,
            vertical: 1,
        });

        let sub_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(1)])
            .split(area);

        let main_sub_area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(rects[1]);

        self.render_table(buf, main_sub_area[0]);
        self.render_clones_table(buf, main_sub_area[1]);
        self.render_scrollbar(buf, main_sub_area[0]);

        self.render_summary(buf, rects[2]);
        self.render_footer(buf, rects[3]);

        // Paragraph::new(files_text)
        //     .block(Block::new().borders(Borders::all()))
        //     .render(main_sub_area[0], buf);

        // Paragraph::new(duplicates_text)
        //     .block(Block::new().borders(Borders::all()))
        //     .render(main_sub_area[1], buf);
    }
}
