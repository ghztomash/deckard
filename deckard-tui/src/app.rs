use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    env,
    ops::Index,
    path::PathBuf,
    usize,
};

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

use crate::table::FileTable;

#[derive(Debug, Default)]
enum FocusedWindow {
    #[default]
    Files,
    Clones,
    Marked,
    Help,
}

#[derive(Debug, Default)]
enum Sorting {
    #[default]
    None,
    Count,
    Size,
    Date,
}

#[derive(Debug, Default)]
pub struct App {
    focused_window: FocusedWindow,
    exit: bool,
    file_index: FileIndex,
    file_table: FileTable,
    clone_table: FileTable,
    marked_table: FileTable,
    marked_files: HashSet<PathBuf>,
    show_clones_table: bool,
    show_marked_table: bool,
    show_file_info: bool,
}

impl App {
    pub fn new(target_paths: HashSet<PathBuf>, config: SearchConfig) -> Self {
        Self {
            focused_window: FocusedWindow::Files,
            exit: false,
            file_index: FileIndex::new(target_paths, config),
            file_table: FileTable::new(vec!["File", "Date", "Size", " "]),
            clone_table: FileTable::new(vec!["Clone", "Date", "Size", " "]),
            marked_table: FileTable::new(vec!["Marked"]),
            marked_files: HashSet::new(),
            show_marked_table: true,
            show_clones_table: true,
            show_file_info: true,
        }
    }

    /// runs the application's main loop until the user quits
    pub fn run(&mut self, terminal: &mut crate::tui::Tui) -> Result<()> {
        self.file_index.index_dirs();
        self.file_index.process_files(None);
        self.file_index.find_duplicates(None);

        // update
        if self.file_index.duplicates_len() > 0 {
            self.update_file_table();
            self.update_clone_table();
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
            KeyCode::Char('c') => self.toggle_show_clones_table(),
            KeyCode::Char(' ') => self.mark(),
            KeyCode::Char('a') => self.mark_all(),
            KeyCode::Char('m') => self.toggle_show_marked_table(),
            KeyCode::Char('l') | KeyCode::Right => self.focus_clones_table(),
            KeyCode::Char('h') | KeyCode::Left => self.focus_files_table(),
            _ => {}
        }
        Ok(())
    }

    fn exit(&mut self) {
        self.exit = true;
    }

    fn mark(&mut self) {
        if let Some(path) = self.active_selected_file() {
            self.marked_files.insert(path.clone());
            let v = self.marked_files.clone().into_iter().collect();
            self.marked_table.update_table(&v);
        }
    }

    fn mark_all(&mut self) {}

    fn clear_mark(&mut self) {
        self.marked_files = HashSet::new();
    }

    fn active_selected_file(&self) -> Option<PathBuf> {
        if matches!(self.focused_window, FocusedWindow::Clones) {
            self.clone_table.selected_path()
        } else {
            self.file_table.selected_path()
        }
    }

    fn open_file(&mut self) {
        if let Some(selected_file) = self.active_selected_file() {
            _ = open::that_detached(selected_file);
        }
    }

    fn open_path(&mut self) {
        if let Some(selected_file) = self.active_selected_file() {
            if let Some(path) = selected_file.parent() {
                _ = open::that_detached(path);
            }
        }
    }

    fn delete(&mut self) {}
    fn trash(&mut self) {}

    fn focus_files_table(&mut self) {
        self.focused_window = FocusedWindow::Files;
    }

    fn focus_clones_table(&mut self) {
        if self.show_clones_table {
            self.focused_window = FocusedWindow::Clones;
        }
    }

    fn toggle_show_clones_table(&mut self) {
        self.show_clones_table = !self.show_clones_table;
    }

    fn toggle_show_marked_table(&mut self) {
        self.show_marked_table = !self.show_marked_table;
    }

    fn toggle_info(&mut self) {
        self.show_file_info = !self.show_file_info;
    }

    pub fn next(&mut self) {
        if matches!(self.focused_window, FocusedWindow::Clones) {
            self.clone_table.select_next();
        } else {
            self.file_table.select_next();
            self.update_clone_table();
        }
    }

    pub fn previous(&mut self) {
        if matches!(self.focused_window, FocusedWindow::Clones) {
            self.clone_table.select_previous();
        } else {
            self.file_table.select_previous();
            self.update_clone_table();
        }
    }

    // fn select_file(&mut self, index: usize) {
    //     self.file_table_state.select(Some(index));
    //     self.selected_file = self
    //         .file_index
    //         .duplicates
    //         .keys()
    //         .collect::<Vec<&PathBuf>>()
    //         .get(index)
    //         .map(|&p| p.clone());
    //     self.scroll_state = self.scroll_state.position(index);
    //     self.update_clone_table();
    //
    //     let selected_file = self.selected_file.as_ref().unwrap();
    //     // self.clone_table_len = self
    //     //     .file_index
    //     //     .duplicates
    //     //     .get(selected_file)
    //     //     .map_or(0, |d| d.len());
    //     //
    //     // self.clone_scroll_state = ScrollbarState::new(self.clone_table_len - 1);
    //     //
    //     // self.clone_table.select(Some(0));
    //     // self.select_clone(0);
    // }

    fn update_file_table(&mut self) {
        let mut paths: Vec<PathBuf> = self.file_index.duplicates.keys().cloned().collect();

        paths.sort_by(|a, b| {
            let a_size = self.file_index.file_size(a).unwrap();
            let b_size = self.file_index.file_size(b).unwrap();
            b_size.cmp(&a_size)
        });

        self.file_table.update_table(&paths);
        self.file_table.select_first();
    }

    fn update_clone_table(&mut self) {
        if let Some(selected_file) = self.file_table.selected_path().as_ref() {
            if let Some(clone_paths) = self.file_index.duplicates.get(selected_file) {
                let paths = clone_paths.iter().cloned().collect();
                self.clone_table.update_table(&paths);
                self.clone_table.select_first();
            }
        }
    }

    // fn next_file(&mut self) {
    //     let i = match self.file_table_state.selected() {
    //         Some(i) => {
    //             if i >= self.file_table_len - 1 {
    //                 0
    //             } else {
    //                 i + 1
    //             }
    //         }
    //         None => 0,
    //     };
    //     self.select_file(i);
    // }
    //
    // fn previous_file(&mut self) {
    //     let i = match self.file_table_state.selected() {
    //         Some(i) => {
    //             if i == 0 {
    //                 self.file_table_len - 1
    //             } else {
    //                 i - 1
    //             }
    //         }
    //         None => 0,
    //     };
    //     self.select_file(i);
    // }

    // fn select_clone(&mut self, index: usize) {
    //     self.clone_table_state.select(Some(index));
    //
    //     if let Some(selected_file) = self.selected_file.as_ref() {
    //         self.selected_clone = self
    //             .file_index
    //             .duplicates
    //             .get(selected_file)
    //             .unwrap()
    //             .iter()
    //             .collect::<Vec<&PathBuf>>()
    //             .get(index)
    //             .map(|&p| p.clone());
    //     };
    //
    //     self.clone_scroll_state = self.clone_scroll_state.position(index);
    // }
    //
    // fn next_clone(&mut self) {
    //     let i = match self.clone_table_state.selected() {
    //         Some(i) => {
    //             if i >= self.clone_table_len - 1 {
    //                 0
    //             } else {
    //                 i + 1
    //             }
    //         }
    //         None => 0,
    //     };
    //     self.select_clone(i);
    // }
    //
    // fn previous_clone(&mut self) {
    //     let i = match self.clone_table_state.selected() {
    //         Some(i) => {
    //             if i == 0 {
    //                 self.clone_table_len - 1
    //             } else {
    //                 i - 1
    //             }
    //         }
    //         None => 0,
    //     };
    //     self.select_clone(i);
    // }

    // fn render_table(&mut self, buf: &mut Buffer, area: Rect) {
    //     let header_style = Style::default().add_modifier(Modifier::BOLD);
    //     let selected_style = Style::default().add_modifier(Modifier::REVERSED);
    //
    //     let mut header = vec!["File", "Total"];
    //
    //     if !self.show_clones_table {
    //         header.push("Clones");
    //     };
    //     header.push(" ");
    //
    //     let header = header
    //         .into_iter()
    //         .map(Cell::from)
    //         .collect::<Row>()
    //         .style(header_style)
    //         .height(1);
    //
    //     let duplicates = &self.file_index.duplicates;
    //
    //     let rows = duplicates.keys().into_iter().map(|k| {
    //         let path = format_path(k, &self.file_index.dirs);
    //         let duplicates = duplicates[k].len();
    //         let size = humansize::format_size(
    //             self.file_index.file_size(k).unwrap_or_default() * (duplicates + 1) as u64,
    //             humansize::DECIMAL,
    //         );
    //
    //         let cells = if self.show_clones_table {
    //             vec![
    //                 Cell::from(Text::from(format!("{path}"))),
    //                 Cell::from(Text::from(format!("{size}"))),
    //                 Cell::from(Text::from(format!(" "))),
    //             ]
    //         } else {
    //             vec![
    //                 Cell::from(Text::from(format!("{path}"))),
    //                 Cell::from(Text::from(format!("{size}"))),
    //                 Cell::from(Text::from(format!("{duplicates}").magenta())),
    //                 Cell::from(Text::from(format!(" "))),
    //             ]
    //         };
    //         cells
    //             .into_iter()
    //             .collect::<Row>()
    //             .style(Style::new())
    //             .height(1)
    //     });
    //     let block;
    //     let bar;
    //     if matches!(self.focused_window, FocusedWindow::Files) {
    //         bar = "->";
    //         block = Block::bordered()
    //             // .title(" Clones ")
    //             .border_type(BorderType::Thick)
    //             .border_style(Style::new().green());
    //     } else {
    //         bar = "  ";
    //         block = Block::bordered()
    //             .border_type(BorderType::Plain)
    //             .border_style(Style::new());
    //     };
    //
    //     let table = Table::new(
    //         rows,
    //         if self.show_clones_table {
    //             vec![Constraint::Min(10), Constraint::Max(12), Constraint::Max(1)]
    //         } else {
    //             vec![
    //                 Constraint::Min(10),
    //                 Constraint::Max(12),
    //                 Constraint::Max(8),
    //                 Constraint::Max(1),
    //             ]
    //         },
    //     )
    //     .header(header)
    //     .highlight_style(selected_style)
    //     .highlight_symbol(Text::from(vec![bar.into()]))
    //     .highlight_spacing(HighlightSpacing::Always)
    //     .block(block);
    //
    //     StatefulWidget::render(table, area, buf, &mut self.file_table_state);
    //
    //     let scrollbar = Scrollbar::default().orientation(ScrollbarOrientation::VerticalRight);
    //     scrollbar.render(
    //         area.inner(Margin {
    //             vertical: 1,
    //             horizontal: 0,
    //         }),
    //         buf,
    //         &mut self.scroll_state,
    //     );
    // }

    // fn render_clones_table(&mut self, buf: &mut Buffer, area: Rect) {
    //     let header_style = Style::default();
    //     let selected_style = Style::default().add_modifier(Modifier::REVERSED);
    //
    //     let header = vec!["Clone", "Date", "Size", " "]
    //         .into_iter()
    //         .map(Cell::from)
    //         .collect::<Row>()
    //         .style(header_style)
    //         .height(1);
    //
    //     let selected_file = self.selected_file.as_ref();
    //     if selected_file.is_none() {
    //         return ();
    //     }
    //
    //     let duplicates = &self
    //         .file_index
    //         .duplicates
    //         .get(selected_file.unwrap())
    //         .unwrap();
    //
    //     let rows = duplicates.into_iter().map(|k| {
    //         let path = format_path(k, &self.file_index.dirs);
    //         let size = humansize::format_size(
    //             self.file_index.file_size(k).unwrap_or_default(),
    //             humansize::DECIMAL,
    //         );
    //         let date = self.file_index.files[k].created;
    //
    //         let cells = vec![
    //             Cell::from(Text::from(format!("{path}"))),
    //             Cell::from(Text::from(format!("{date}"))),
    //             Cell::from(Text::from(format!("{size}"))),
    //             Cell::from(Text::from(format!(" "))),
    //         ];
    //         cells
    //             .into_iter()
    //             .collect::<Row>()
    //             .style(Style::new())
    //             .height(1)
    //     });
    //     let block;
    //     let bar;
    //     if matches!(self.focused_window, FocusedWindow::Clones) {
    //         bar = "->";
    //         block = Block::bordered()
    //             // .title(" Clones ")
    //             .border_type(BorderType::Thick)
    //             .border_style(Style::new().green());
    //     } else {
    //         bar = "  ";
    //         block = Block::bordered()
    //             .border_type(BorderType::Plain)
    //             .border_style(Style::new());
    //     };
    //     let table = Table::new(
    //         rows,
    //         [
    //             // + 1 is for padding.
    //             Constraint::Min(10),
    //             Constraint::Max(10),
    //             Constraint::Max(12),
    //             Constraint::Max(1),
    //         ],
    //     )
    //     .header(header)
    //     .highlight_style(selected_style)
    //     .highlight_symbol(Text::from(vec![bar.into()]))
    //     .highlight_spacing(HighlightSpacing::Always)
    //     .block(block);
    //
    //     StatefulWidget::render(table, area, buf, &mut self.clone_table_state);
    //
    //     let scrollbar = Scrollbar::default().orientation(ScrollbarOrientation::VerticalRight);
    //     scrollbar.render(
    //         area.inner(Margin {
    //             vertical: 1,
    //             horizontal: 0,
    //         }),
    //         buf,
    //         &mut self.clone_scroll_state,
    //     );
    // }

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
        let info_lines = if let Some(selected_file) = self.active_selected_file() {
            let file_entry = &self.file_index.files[&selected_file];

            vec![
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
            ]
        } else {
            vec![Line::from(vec!["none".into()])]
        };

        let file_info_text = Text::from(info_lines);

        let summary = Paragraph::new(file_info_text).style(Style::new()).block(
            Block::bordered()
                .border_type(BorderType::Plain)
                .borders(Borders::ALL)
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
        let total: u64 = self.file_index.files.iter().map(|(_, f)| f.size).sum();
        let total_size = humansize::format_size(total, humansize::DECIMAL);

        let duplicate_lines = vec![
            Line::from(vec![
                "Clones: ".into(),
                self.file_index.files_len().to_string().magenta(),
                " Total: ".into(),
                total_size.blue(),
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
            " File ".into(),
            "<h/left>".blue().bold(),
            " Clones ".into(),
            "<l/right>".blue().bold(),
            " Mark ".into(),
            "<space>".blue().bold(),
            " Mark All ".into(),
            "<a>".blue().bold(),
            " Show clones ".into(),
            "<c>".blue().bold(),
            " Info ".into(),
            "<i>".blue().bold(),
            " Open file ".into(),
            "<o>".blue().bold(),
            " Open path ".into(),
            "<p>".blue().bold(),
            " Trash ".into(),
            "<t/backspace>".blue().bold(),
            " Delete ".into(),
            "<D/delete>".blue().bold(),
            " Quit ".into(),
            "<q/esc>".blue().bold(),
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

        let main_sub_area_constrains = if self.show_clones_table || self.show_file_info {
            [Constraint::Percentage(50), Constraint::Percentage(50)]
        } else {
            [Constraint::Percentage(100), Constraint::Percentage(0)]
        };

        let main_sub_area_inner_constrains = if self.show_file_info || self.show_marked_table {
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
            .split(main_sub_area[0]);

        let main_sub_area_right = Layout::default()
            .direction(Direction::Vertical)
            .constraints(main_sub_area_inner_constrains)
            .split(main_sub_area[1]);

        self.file_table.render(
            buf,
            main_sub_area_left[0],
            matches!(self.focused_window, FocusedWindow::Files),
            &self.file_index,
        );

        if self.show_marked_table {
            self.marked_table
                .render(buf, main_sub_area_left[1], false, &self.file_index);
        }

        if self.show_clones_table {
            self.clone_table.render(
                buf,
                main_sub_area_right[0],
                matches!(self.focused_window, FocusedWindow::Clones),
                &self.file_index,
            );
        }

        if self.show_file_info {
            let area = if self.show_clones_table { 1 } else { 0 };
            self.render_file_info(buf, main_sub_area_right[area]);
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
}

/// Make the path relative to the commont search parth
pub fn format_path(path: &PathBuf, target_paths: &HashSet<PathBuf>) -> String {
    let common_path = deckard::find_common_path(target_paths);

    let relative_path = if let Some(common_path) = &common_path {
        let path = path.strip_prefix(&common_path).unwrap_or(path);
        path
    } else {
        path
    };
    relative_path.to_string_lossy().to_string()
}
