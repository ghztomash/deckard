use std::{borrow::Cow, collections::HashSet, env, ops::Index, path::PathBuf};

use color_eyre::{
    eyre::{bail, Result, WrapErr},
    owo_colors::OwoColorize,
};
use deckard::config::SearchConfig;
use ratatui::{
    buffer::Buffer,
    crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Modifier, Style, Stylize},
    symbols::border,
    text::{Line, Text},
    widgets::{
        block::{title, Position, Title},
        Block, BorderType, Borders, Cell, HighlightSpacing, Paragraph, Row, Scrollbar,
        ScrollbarOrientation, ScrollbarState, StatefulWidget, Table, TableState, Widget,
    },
    Frame,
};

use deckard::index::FileIndex;

#[derive(Debug, Default)]
enum InputState {
    #[default]
    Files,
    Clones,
    Popup,
}

#[derive(Debug, Default)]
enum PopupState {
    #[default]
    None,
    Delete,
    Trash,
}

#[derive(Debug, Default)]
pub struct App {
    app_state: PopupState,
    input_state: InputState,
    exit: bool,
    file_index: FileIndex,
    scroll_state: ScrollbarState,
    file_table_state: TableState,
    file_table_len: usize,
    clone_table_state: TableState,
    clone_scroll_state: ScrollbarState,
    selected_file: Option<PathBuf>,
    selected_clone: Option<PathBuf>,
    marked_files: Vec<PathBuf>,
    show_file_clones: bool,
    show_file_info: bool,
}

impl App {
    pub fn new(target_paths: HashSet<PathBuf>, config: SearchConfig) -> Self {
        Self {
            app_state: PopupState::None,
            input_state: InputState::Files,
            exit: false,
            file_index: FileIndex::new(target_paths, config),
            scroll_state: ScrollbarState::new(0),
            file_table_state: TableState::new(),
            file_table_len: 0,
            clone_table_state: TableState::new(),
            clone_scroll_state: ScrollbarState::new(0),
            selected_file: None,
            selected_clone: None,
            marked_files: Vec::new(),
            show_file_clones: true,
            show_file_info: true,
        }
    }

    /// runs the application's main loop until the user quits
    pub fn run(&mut self, terminal: &mut crate::tui::Tui) -> Result<()> {
        self.file_index.index_dirs();
        self.file_index.process_files();
        self.file_index.find_duplicates();
        self.file_table_len = self.file_index.duplicates_len();

        // update
        if self.file_table_len > 0 {
            self.scroll_state = ScrollbarState::new(self.file_table_len - 1);
            self.file_table_state = TableState::default().with_selected(0);

            self.selected_file = self
                .file_index
                .duplicates
                .keys()
                .collect::<Vec<&PathBuf>>()
                .get(0)
                .map(|&p| p.clone());
        }

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
            KeyCode::Char('i') => self.toggle_info(),
            KeyCode::Char('o') => self.open_file(),
            KeyCode::Char('p') => self.open_path(),
            KeyCode::Char('D') | KeyCode::Delete => self.delete(),
            KeyCode::Char('t') | KeyCode::Backspace => self.trash(),
            KeyCode::Char('c') => self.toggle_file_clones(),
            KeyCode::Char(' ') => self.mark(),
            KeyCode::Char('l') | KeyCode::Right => self.select_clones(),
            KeyCode::Char('h') | KeyCode::Left => self.select_files(),
            _ => {}
        }
        Ok(())
    }

    fn exit(&mut self) {
        self.exit = true;
    }

    fn mark(&mut self) {}

    fn mark_all(&mut self) {}

    fn clear_mark(&mut self) {
        self.marked_files = Vec::new();
    }

    fn open_file(&mut self) {
        let selected_file = if matches!(self.input_state, InputState::Clones) {
            self.selected_clone.as_ref()
        } else {
            self.selected_file.as_ref()
        };
        if let Some(selected_file) = selected_file {
            _ = open::that(selected_file);
        }
    }

    fn open_path(&mut self) {
        let selected_file = if matches!(self.input_state, InputState::Clones) {
            self.selected_clone.as_ref()
        } else {
            self.selected_file.as_ref()
        };
        if let Some(selected_file) = selected_file {
            if let Some(path) = selected_file.parent() {
                _ = open::that(path);
            }
        }
    }

    fn delete(&mut self) {}
    fn trash(&mut self) {}

    fn select_files(&mut self) {
        self.input_state = InputState::Files;
        self.clone_table_state.select(None);
        self.selected_clone = None
    }

    fn select_clones(&mut self) {
        if self.show_file_clones {
            self.input_state = InputState::Clones;
            self.clone_table_state.select(Some(0));
        }
    }

    fn toggle_file_clones(&mut self) {
        self.show_file_clones = !self.show_file_clones;
    }

    fn toggle_info(&mut self) {
        self.show_file_info = !self.show_file_info;
    }

    pub fn next(&mut self) {
        if matches!(self.input_state, InputState::Clones) {
            self.next_clone();
        } else {
            self.next_file();
        }
    }

    pub fn previous(&mut self) {
        if matches!(self.input_state, InputState::Clones) {
            self.previous_clone();
        } else {
            self.previous_file();
        }
    }

    // TODO: refactor to use common select_file()
    fn next_file(&mut self) {
        let i = match self.file_table_state.selected() {
            Some(i) => {
                if i >= self.file_table_len - 1 {
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

    // TODO: refactor to use common select_file()
    fn previous_file(&mut self) {
        let i = match self.file_table_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.file_table_len - 1
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

    // TODO: refactor to use common select_file()
    fn next_clone(&mut self) {
        if self.selected_file.is_none() {
            return ();
        }
        let selected_file = self.selected_file.as_ref().unwrap();
        let clones_len = self
            .file_index
            .duplicates
            .get(selected_file)
            .map_or(0, |d| d.len());

        self.clone_scroll_state = ScrollbarState::new(clones_len - 1);

        let i = match self.clone_table_state.selected() {
            Some(i) => {
                if i >= clones_len - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.clone_table_state.select(Some(i));
        self.selected_clone = self
            .file_index
            .duplicates
            .get(selected_file)
            .unwrap()
            .iter()
            .collect::<Vec<&PathBuf>>()
            .get(i)
            .map(|&p| p.clone());
        self.clone_scroll_state = self.clone_scroll_state.position(i);
    }

    // TODO: refactor to use common select_file()
    fn previous_clone(&mut self) {
        if self.selected_file.is_none() {
            return ();
        }
        let selected_file = self.selected_file.as_ref().unwrap();
        let clones_len = self
            .file_index
            .duplicates
            .get(selected_file)
            .map_or(0, |d| d.len());

        self.clone_scroll_state = ScrollbarState::new(clones_len - 1);

        let i = match self.clone_table_state.selected() {
            Some(i) => {
                if i == 0 {
                    clones_len - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.clone_table_state.select(Some(i));
        self.selected_clone = self
            .file_index
            .duplicates
            .get(selected_file)
            .unwrap()
            .iter()
            .collect::<Vec<&PathBuf>>()
            .get(i)
            .map(|&p| p.clone());
        self.clone_scroll_state = self.clone_scroll_state.position(i);
    }

    fn render_table(&mut self, buf: &mut Buffer, area: Rect) {
        let header_style = Style::default().add_modifier(Modifier::BOLD);
        let selected_style = Style::default().add_modifier(Modifier::REVERSED);

        let mut header = vec!["File", "Total"];

        if !self.show_file_clones {
            header.push("Clones");
        };
        header.push(" ");

        let header = header
            .into_iter()
            .map(Cell::from)
            .collect::<Row>()
            .style(header_style)
            .height(1);

        let duplicates = &self.file_index.duplicates;

        let rows = duplicates.keys().into_iter().map(|k| {
            let path = self.format_path(k);
            let duplicates = duplicates[k].len();
            let size = humansize::format_size(
                self.file_index.file_size(k).unwrap_or_default() * (duplicates + 1) as u64,
                humansize::DECIMAL,
            );

            let cells = if self.show_file_clones {
                vec![
                    Cell::from(Text::from(format!("{path}"))),
                    Cell::from(Text::from(format!("{size}"))),
                    Cell::from(Text::from(format!(" "))),
                ]
            } else {
                vec![
                    Cell::from(Text::from(format!("{path}"))),
                    Cell::from(Text::from(format!("{size}"))),
                    Cell::from(Text::from(format!("{duplicates}").magenta())),
                    Cell::from(Text::from(format!(" "))),
                ]
            };
            cells
                .into_iter()
                .collect::<Row>()
                .style(Style::new())
                .height(1)
        });
        let block = if matches!(self.input_state, InputState::Files) {
            Block::bordered()
                .border_type(BorderType::Thick)
                .border_style(Style::new().green())
        } else {
            Block::bordered()
                .border_type(BorderType::Plain)
                .border_style(Style::new())
        };
        let bar = "->";
        let table = Table::new(
            rows,
            if self.show_file_clones {
                vec![Constraint::Min(10), Constraint::Max(12), Constraint::Max(1)]
            } else {
                vec![
                    Constraint::Min(10),
                    Constraint::Max(12),
                    Constraint::Max(8),
                    Constraint::Max(1),
                ]
            },
        )
        .header(header)
        .highlight_style(selected_style)
        .highlight_symbol(Text::from(vec![bar.into()]))
        .highlight_spacing(HighlightSpacing::Always)
        .block(block);

        StatefulWidget::render(table, area, buf, &mut self.file_table_state);

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

    fn render_clones_table(&mut self, buf: &mut Buffer, area: Rect) {
        let header_style = Style::default();
        let selected_style = Style::default().add_modifier(Modifier::REVERSED);

        let header = [" ", "Clone", "Date", "Size"]
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

        let rows = duplicates.into_iter().map(|k| {
            let path = self.format_path(k);
            let size = humansize::format_size(
                self.file_index.file_size(k).unwrap_or_default(),
                humansize::DECIMAL,
            );
            let date = self.file_index.files[k].created;

            let cells = vec![
                Cell::from(Text::from(format!(" "))),
                Cell::from(Text::from(format!("{path}"))),
                Cell::from(Text::from(format!("{date}"))),
                Cell::from(Text::from(format!("{size}"))),
            ];
            cells
                .into_iter()
                .collect::<Row>()
                .style(Style::new())
                .height(1)
        });
        let block;
        let bar;
        if matches!(self.input_state, InputState::Clones) {
            bar = "->";
            block = Block::bordered()
                // .title(" Clones ")
                .border_type(BorderType::Thick)
                .border_style(Style::new().green());
        } else {
            bar = "  ";
            block = Block::bordered()
                .border_type(BorderType::Plain)
                .border_style(Style::new());
        };
        let table = Table::new(
            rows,
            [
                // + 1 is for padding.
                Constraint::Max(1),
                Constraint::Min(10),
                Constraint::Max(10),
                Constraint::Max(12),
            ],
        )
        .header(header)
        .highlight_style(selected_style)
        .highlight_symbol(Text::from(vec![bar.into()]))
        .highlight_spacing(HighlightSpacing::Always)
        .block(block);

        StatefulWidget::render(table, area, buf, &mut self.clone_table_state);

        let scrollbar = Scrollbar::default().orientation(ScrollbarOrientation::VerticalRight);
        scrollbar.render(
            area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            buf,
            &mut self.clone_scroll_state,
        );
    }

    fn render_header(&self, buf: &mut Buffer, area: Rect) {
        let spans = vec![
            "Deckard".bold(),
            " v".into(),
            format!("{}", env!("CARGO_PKG_VERSION")).into(),
        ];
        let title = Line::from(spans);
        let header = Paragraph::new(title)
            .style(Style::new().gray().reversed())
            .centered();
        header.render(area, buf)
    }

    fn render_file_info(&self, buf: &mut Buffer, area: Rect) {
        let selected_file = if matches!(self.input_state, InputState::Clones) {
            self.selected_clone.as_ref()
        } else {
            self.selected_file.as_ref()
        };
        if selected_file.is_none() {
            return ();
        }
        let selected_file = selected_file.unwrap();
        let file_entry = &self.file_index.files[selected_file];

        let info_lines = vec![
            Line::from(vec!["name: ".into(), file_entry.name.to_string().yellow()]),
            Line::from(vec![
                "size: ".into(),
                humansize::format_size(file_entry.size, humansize::DECIMAL)
                    .to_string()
                    .blue(),
                " (".into(),
                file_entry.size.to_string().blue(),
                ")".into(),
            ]),
            Line::from(vec![
                "created: ".into(),
                file_entry.created.to_string().red(),
            ]),
            Line::from(vec![
                "modified: ".into(),
                file_entry.created.to_string().red(),
            ]),
            Line::from(vec![
                "mime: ".into(),
                file_entry
                    .mime_type
                    .as_ref()
                    .unwrap_or(&"none".to_string())
                    .to_string()
                    .cyan(),
            ]),
            Line::from(vec![
                "hash: ".into(),
                file_entry
                    .hash
                    .as_ref()
                    .unwrap_or(&"none".to_string())
                    .to_string()
                    .cyan(),
            ]),
            Line::from(vec![
                "path: ".into(),
                deckard::to_relative_path(&file_entry.path)
                    .to_string_lossy()
                    .to_string()
                    .yellow(),
            ]),
        ];

        let file_info_text = Text::from(info_lines);

        let summary = Paragraph::new(file_info_text).style(Style::new()).block(
            Block::bordered()
                .border_type(BorderType::Plain)
                .borders(Borders::TOP)
                .border_style(Style::new()),
        );
        summary.render(area, buf)
    }

    fn render_summary(&self, buf: &mut Buffer, area: Rect) {
        let dirs: Vec<PathBuf> = self.file_index.dirs.clone().into_iter().collect();

        let dir_lines: Vec<String> = dirs
            .iter()
            .map(|d| {
                format!(
                    "{}",
                    deckard::to_relative_path(d)
                        // d.strip_prefix(&common_path)
                        // .unwrap_or(d)
                        .to_string_lossy()
                        .to_string()
                        .yellow(),
                )
            })
            .collect();

        let dir_joined = dir_lines.join(" ");

        let duplicate_lines = vec![
            Line::from(vec![
                "Clones: ".into(),
                self.file_table_len.to_string().magenta(),
                " Total: ".into(),
            ]),
            Line::from(vec!["Paths: ".into(), dir_joined.yellow()]),
        ];
        // duplicate_lines.extend(dir_lines);

        let duplicates_text = Text::from(duplicate_lines);

        let summary = Paragraph::new(duplicates_text).style(Style::new()).block(
            Block::bordered()
                .border_type(BorderType::Plain)
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
        let info_footer = Paragraph::new(instructions).style(Style::new());
        info_footer.render(area, buf)
    }
}

impl App {
    fn render_ui(&mut self, area: Rect, buf: &mut Buffer) {
        let rects = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Max(5),
            Constraint::Length(1),
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

        let main_sub_area_constrains = if self.show_file_clones || self.show_file_info {
            [Constraint::Percentage(50), Constraint::Percentage(50)]
        } else {
            [Constraint::Percentage(100), Constraint::Percentage(0)]
        };

        let main_sub_area_inner_constrains = if self.show_file_info {
            [Constraint::Percentage(60), Constraint::Percentage(40)]
        } else {
            [Constraint::Percentage(100), Constraint::Percentage(0)]
        };

        let main_sub_area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(main_sub_area_constrains)
            .split(rects[1]);

        let main_sub_area_left = Layout::default()
            .direction(Direction::Vertical)
            .constraints(main_sub_area_inner_constrains)
            .split(main_sub_area[1]);

        let main_sub_area_right = Layout::default()
            .direction(Direction::Vertical)
            .constraints(main_sub_area_inner_constrains)
            .split(main_sub_area[1]);

        self.render_table(buf, main_sub_area[0]);

        if self.show_file_clones {
            self.render_clones_table(buf, main_sub_area_right[0]);
        }

        if self.show_file_info {
            self.render_file_info(buf, main_sub_area_right[1]);
        }

        self.render_summary(buf, rects[2]);
        self.render_footer(buf, rects[3]);

        // Paragraph::new(files_text)
        //     .block(Block::new().borders(Borders::all()))
        //     .render(main_sub_area[0], buf);

        // Paragraph::new(duplicates_text)
        //     .block(Block::new().borders(Borders::all()))
        //     .render(main_sub_area[1], buf);
    }

    /// Make the path relative to the commont search parth
    fn format_path(&self, path: &PathBuf) -> String {
        let common_path = deckard::find_common_path(&self.file_index.dirs);

        let relative_path = if let Some(common_path) = &common_path {
            let path = path.strip_prefix(&common_path).unwrap_or(path);
            path
        } else {
            path
        };
        relative_path.to_string_lossy().to_string()
    }
}
