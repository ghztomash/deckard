use crate::table::FileTable;
use color_eyre::eyre::{Result, WrapErr};
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind};
use deckard::config::SearchConfig;
use deckard::index::FileIndex;
use futures::StreamExt;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Styled, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Gauge, Paragraph, Widget, Wrap},
};
use std::sync::{Arc, RwLock};
use std::{
    collections::HashSet,
    env, fmt, fs,
    hash::{DefaultHasher, Hash, Hasher},
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tokio::{
    sync::mpsc::{UnboundedSender, unbounded_channel},
    task::AbortHandle,
};

#[derive(Debug, Default)]
enum FocusedWindow {
    #[default]
    Files,
    Clones,
    Marked,
    _Help,
}

#[derive(Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Sorting {
    #[default]
    Size,
    Count,
    Date,
    Path,
}

impl Sorting {
    pub fn next(&self) -> Self {
        match self {
            Self::Size => Self::Count,
            Self::Count => Self::Date,
            Self::Date => Self::Path,
            Self::Path => Self::Size,
        }
    }
}

impl fmt::Display for Sorting {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let result = match self {
            Self::Size => "Size",
            Self::Count => "Count",
            Self::Date => "Date",
            Self::Path => "Path",
        };
        write!(f, "{result}")
    }
}

#[derive(Debug, Default)]
pub struct App {
    focused_window: FocusedWindow,
    should_exit: bool,
    dry_run: bool,
    remove_dirs: bool,
    file_index: Arc<RwLock<FileIndex>>,
    file_table: FileTable,
    clone_table: FileTable,
    marked_table: FileTable,
    marked_files: HashSet<PathBuf>,
    show_clones_table: bool,
    show_marked_table: bool,
    show_file_info: bool,
    show_more_keys: bool,
    current_state: State,
    sort_by: Sorting,
    cancel_flag: Arc<AtomicBool>,
    abort_handle: Option<AbortHandle>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum State {
    #[default]
    Idle,
    Indexing {
        done: usize,
    },
    Processing {
        done: usize,
        total: usize,
    },
    Comparing {
        done: usize,
        total: usize,
    },
    Done,
    Error(String),
}

impl State {
    fn get_color(&self) -> Color {
        match self {
            Self::Done => Color::Green,
            Self::Error(_) => Color::Red,
            _ => Color::Yellow,
        }
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let result = match self {
            Self::Idle => "Idle",
            Self::Indexing { done } => &format!("Indexing {done}"),
            Self::Processing { done, total } => &format!("Processing {done}/{total}"),
            Self::Comparing { done, total } => &format!("Comparing {done}/{total}"),
            Self::Done => "Done",
            Self::Error(e) => &format!("Error: {e}"),
        };
        write!(f, "{result}")
    }
}

impl App {
    const FRAMES_PER_SECOND: f32 = 30.0;

    pub fn new(
        target_paths: HashSet<PathBuf>,
        config: SearchConfig,
        dry_run: bool,
        remove_dirs: bool,
    ) -> Self {
        Self {
            focused_window: FocusedWindow::Files,
            should_exit: false,
            file_index: Arc::new(RwLock::new(FileIndex::new(target_paths, config))),
            file_table: FileTable::new(vec![" ", "File", "Date", "Size", "Clones"], true, true),
            clone_table: FileTable::new(vec![" ", "Clone", "Date", "Size"], true, false),
            marked_table: FileTable::new(vec![" ", "Marked"], false, false),
            marked_files: HashSet::new(),
            show_marked_table: true,
            show_clones_table: true,
            show_file_info: true,
            show_more_keys: false,
            current_state: State::Idle,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            abort_handle: None,
            sort_by: Sorting::default(),
            dry_run,
            remove_dirs,
        }
    }

    /// runs the application's main loop until the user quits
    pub async fn run(&mut self, terminal: &mut crate::tui::Tui) -> Result<()> {
        let period = Duration::from_secs_f32(1.0 / Self::FRAMES_PER_SECOND);
        let mut interval = tokio::time::interval(period);
        let mut events = EventStream::new();

        // TODO: Handle graceful shutdown
        let (tx, mut rx) = unbounded_channel::<State>();
        let file_index = self.file_index.clone();
        let task_cancel_flag = self.cancel_flag.clone();
        let task_handle = tokio::spawn(async move {
            if let Err(e) =
                index_files(file_index.clone(), tx.clone(), task_cancel_flag.clone()).await
            {
                let _ = tx.send(State::Error(format!("index_files error: {e}")));
            }
            if let Err(e) =
                process_files(file_index.clone(), tx.clone(), task_cancel_flag.clone()).await
            {
                let _ = tx.send(State::Error(format!("process_files error: {e}")));
            }
            if let Err(e) =
                find_duplicates(file_index.clone(), tx.clone(), task_cancel_flag.clone()).await
            {
                let _ = tx.send(State::Error(format!("find_duplicates error: {e}")));
            }
            let _ = tx.send(State::Done);
        });
        self.abort_handle = Some(task_handle.abort_handle());

        while !self.should_exit {
            tokio::select! {
                _ = interval.tick() => {
                    terminal.draw(|frame| self.render_ui(frame.area(), frame.buffer_mut()))?;
                },
                Some(Ok(event)) = events.next() => self.handle_events(event)?,
                Some(state) = rx.recv() => {
                    self.handle_state(state);
                },
                else => break,
            }
        }
        task_handle.await?;
        Ok(())
    }

    fn is_done(&self) -> bool {
        self.current_state == State::Done
    }

    fn update_tables(&mut self) {
        // update
        self.update_file_table();
        self.update_clone_table();
    }

    /// updates the application's state based on user input
    fn handle_events(&mut self, event: Event) -> Result<()> {
        match event {
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
            KeyCode::Char('j') | KeyCode::Down => self.next_file(),
            KeyCode::Char('k') | KeyCode::Up => self.previous_file(),
            KeyCode::Char('i') => self.toggle_info(),
            KeyCode::Char('o') => self.open_file(),
            KeyCode::Char('p') => self.open_path(),
            KeyCode::Char('D') | KeyCode::Delete => self.delete(),
            KeyCode::Char('T') | KeyCode::Backspace => self.trash(),
            KeyCode::Char('c') => self.toggle_show_clones_table(),
            KeyCode::Char(' ') => self.mark(),
            KeyCode::Char('a') => self.mark_all(),
            KeyCode::Char('A') => self.clear_marked(),
            KeyCode::Char('m') => self.toggle_show_marked_table(),
            KeyCode::Char('y') => unimplemented!(), // TODO: copy path
            KeyCode::Char('.') => self.toggle_more_keys(),
            KeyCode::Char('?') => unimplemented!(), // TODO: popup help
            KeyCode::Char('s') => self.cycle_sort_by(),
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Tab => self.focus_next_table(),
            KeyCode::Char('h') | KeyCode::Left | KeyCode::BackTab => self.focus_previus_table(),
            _ => {}
        }
        Ok(())
    }

    fn exit(&mut self) {
        self.cancel_flag.store(true, Ordering::Relaxed);
        if let Some(abort_handle) = &self.abort_handle {
            abort_handle.abort();
        }
        self.should_exit = true;
    }

    fn mark(&mut self) {
        if let Some(path) = self.active_selected_file() {
            if !self.marked_files.insert(path.clone()) {
                self.marked_files.remove(&path);
            }
            let v = self.marked_files.clone().into_iter().collect();
            self.marked_table.update_table(&v, &self.file_index, None);
            if matches!(self.focused_window, FocusedWindow::Marked) {
                self.marked_table.select_previous();
            }
        }
    }

    fn mark_all(&mut self) {
        for p in self.clone_table.paths() {
            self.marked_files.insert(p);
            let v = self.marked_files.clone().into_iter().collect();
            self.marked_table.update_table(&v, &self.file_index, None);
        }
    }

    fn clear_marked(&mut self) {
        self.marked_files = HashSet::new();
        let v = self.marked_files.clone().into_iter().collect();
        self.marked_table.update_table(&v, &self.file_index, None);
        if matches!(self.focused_window, FocusedWindow::Marked) {
            self.marked_table.select_none();
        }
    }

    fn remove_marked(&mut self, remove_callback: fn(&PathBuf)) {
        {
            let mut index = self.file_index.write().unwrap();
            for file in &self.marked_files {
                if !self.dry_run {
                    remove_callback(file);
                    if self.remove_dirs {
                        // TODO: Delete any empty dirs
                    }
                }
                index.remove_from_index(file);
            }
        }
        self.clear_marked();
        self.update_tables();
    }

    fn active_selected_file(&self) -> Option<PathBuf> {
        let active_table = match self.focused_window {
            FocusedWindow::Files => &self.file_table,
            FocusedWindow::Clones => &self.clone_table,
            FocusedWindow::Marked => &self.marked_table,
            _ => return None,
        };
        active_table.selected_path()
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

    fn delete(&mut self) {
        self.remove_marked(|f| {
            fs::remove_file(f).unwrap();
        });
    }

    fn trash(&mut self) {
        self.remove_marked(|f| {
            trash::delete(f).unwrap();
        });
    }

    fn focus_next_table(&mut self) {
        match self.focused_window {
            FocusedWindow::Files => {
                if self.show_clones_table {
                    self.focus_clones_table();
                } else {
                    self.focus_marked_table();
                }
            }
            FocusedWindow::Clones => {
                if self.show_marked_table {
                    self.focus_marked_table();
                } else {
                    self.focus_files_table();
                }
            }
            FocusedWindow::Marked => {
                self.focus_files_table();
                self.marked_table.select_none();
            }
            _ => {}
        }
    }

    fn focus_previus_table(&mut self) {
        match self.focused_window {
            FocusedWindow::Files => {
                if self.show_marked_table {
                    self.focus_marked_table();
                } else {
                    self.focus_clones_table();
                }
            }
            FocusedWindow::Clones => self.focus_files_table(),
            FocusedWindow::Marked => {
                if self.show_clones_table {
                    self.focus_clones_table();
                } else {
                    self.focus_files_table();
                }
                self.marked_table.select_none();
            }
            _ => {}
        }
    }

    fn focus_marked_table(&mut self) {
        if self.show_marked_table {
            self.focused_window = FocusedWindow::Marked;
            if self.marked_table.selected_path().is_none() {
                self.marked_table.select_first();
            }
        }
    }

    fn focus_files_table(&mut self) {
        self.focused_window = FocusedWindow::Files;
    }

    fn focus_clones_table(&mut self) {
        if self.show_clones_table {
            self.focused_window = FocusedWindow::Clones;
            if self.clone_table.selected_path().is_none() {
                self.clone_table.select_first();
            }
        }
    }

    fn toggle_show_clones_table(&mut self) {
        self.show_clones_table = !self.show_clones_table;
        if !self.show_clones_table && matches!(self.focused_window, FocusedWindow::Clones) {
            self.focus_files_table();
        }
    }

    fn toggle_show_marked_table(&mut self) {
        self.show_marked_table = !self.show_marked_table;
        if !self.show_marked_table && matches!(self.focused_window, FocusedWindow::Marked) {
            self.focus_files_table();
        }
    }

    fn toggle_info(&mut self) {
        self.show_file_info = !self.show_file_info;
    }

    fn toggle_more_keys(&mut self) {
        self.show_more_keys = !self.show_more_keys;
    }

    fn cycle_sort_by(&mut self) {
        self.sort_by = self.sort_by.next();
        self.update_file_table();
    }

    pub fn next_file(&mut self) {
        match self.focused_window {
            FocusedWindow::Files => {
                self.file_table.select_next();
                self.update_clone_table();
            }
            FocusedWindow::Clones => {
                self.clone_table.select_next();
            }
            FocusedWindow::Marked => {
                self.marked_table.select_next();
            }
            _ => {}
        }
    }

    pub fn previous_file(&mut self) {
        match self.focused_window {
            FocusedWindow::Files => {
                self.file_table.select_previous();
                self.update_clone_table();
            }
            FocusedWindow::Clones => {
                self.clone_table.select_previous();
            }
            FocusedWindow::Marked => {
                self.marked_table.select_previous();
            }
            _ => {}
        }
    }

    fn update_file_table(&mut self) {
        let paths: Vec<PathBuf> = self
            .file_index
            .read()
            .unwrap()
            .duplicates
            .keys()
            .cloned()
            .collect();

        if !paths.is_empty() {
            self.file_table
                .update_table(&paths, &self.file_index, Some(&self.sort_by));
            self.file_table.select_first();
        } else {
            self.file_table.clear();
        }
    }

    fn update_clone_table(&mut self) {
        if let Some(selected_file) = self.file_table.selected_path().as_ref() {
            if let Some(clone_paths) = self
                .file_index
                .read()
                .unwrap()
                .duplicates
                .get(selected_file)
            {
                let paths = clone_paths.iter().cloned().collect();
                self.clone_table
                    .update_table(&paths, &self.file_index, Some(&Sorting::Path));
                self.clone_table.select_none();
            }
        } else {
            // Empty the table
            self.clone_table.clear();
        }
    }

    fn render_header(&self, buf: &mut Buffer, area: Rect) {
        let spans = vec![
            "Deckard".bold(),
            " v".into(),
            env!("CARGO_PKG_VERSION").into(),
        ];
        let title = Line::from(spans);
        let header = Paragraph::new(title)
            .style(Style::new().gray().reversed())
            .centered();
        header.render(area, buf)
    }

    fn render_file_info(&self, buf: &mut Buffer, area: Rect) {
        let selected_file = self.active_selected_file();
        let maybe_entry = {
            if let Some(ref path) = selected_file {
                let fi = self.file_index.read().unwrap();
                fi.files.get(path).cloned()
            } else {
                None
            }
        };

        let info_lines = if let Some(file_entry) = maybe_entry {
            let mut lines = vec![
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
                    file_entry
                        .created
                        .format("%d/%m/%Y %H:%M:%S %Z")
                        .to_string()
                        .red(),
                ]),
                Line::from(vec![
                    "modified: ".into(),
                    file_entry
                        .modified
                        .format("%d/%m/%Y %H:%M:%S %Z")
                        .to_string()
                        .red(),
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

            if let Some(audio_hash) = &file_entry.audio_hash {
                let mut hasher = DefaultHasher::new();
                audio_hash.hash(&mut hasher);
                lines.push(Line::from(vec![
                    "audio_hash: ".into(),
                    format!("{:x}", hasher.finish()).to_string().cyan(),
                ]));
            }

            if let Some(audio_tags) = &file_entry.audio_tags {
                let mut tag_lines = vec![];
                if let Some(v) = &audio_tags.title {
                    tag_lines.push(Line::from(vec!["title: ".into(), v.clone().yellow()]));
                }
                if let Some(v) = &audio_tags.artist {
                    tag_lines.push(Line::from(vec!["artist: ".into(), v.clone().yellow()]));
                }
                if let Some(v) = &audio_tags.album {
                    tag_lines.push(Line::from(vec!["album: ".into(), v.clone().yellow()]));
                }
                if let Some(v) = &audio_tags.genre {
                    tag_lines.push(Line::from(vec!["genre: ".into(), v.clone().yellow()]));
                }
                if let Some(v) = &audio_tags.rating {
                    tag_lines.push(Line::from(vec!["rating: ".into(), v.clone().yellow()]));
                }
                if let Some(v) = &audio_tags.bpm {
                    tag_lines.push(Line::from(vec!["bpm: ".into(), v.clone().yellow()]));
                }
                if let Some(v) = &audio_tags.duration {
                    tag_lines.push(Line::from(vec![
                        "duration: ".into(),
                        v.to_string().yellow(),
                    ]));
                }
                if let Some(v) = &audio_tags.bitrate {
                    tag_lines.push(Line::from(vec!["bitrate: ".into(), v.clone().yellow()]));
                }
                if let Some(v) = &audio_tags.sample_rate {
                    tag_lines.push(Line::from(vec!["sample_rate: ".into(), v.clone().yellow()]));
                }
                if let Some(v) = &audio_tags.comment {
                    tag_lines.push(Line::from(vec![
                        "comment: ".into(),
                        v.clone()
                            .chars()
                            .filter(|c| !c.is_whitespace() || *c == ' ')
                            .collect::<String>()
                            .yellow(),
                    ]));
                }
                lines.extend(tag_lines);
            }

            lines
        } else {
            vec![Line::from(vec!["none".into()])]
        };

        let file_info_text = Text::from(info_lines);

        let summary = Paragraph::new(file_info_text)
            .wrap(Wrap { trim: true })
            .style(Style::new())
            .block(
                Block::bordered()
                    .border_type(BorderType::Plain)
                    .borders(Borders::ALL)
                    .border_style(Style::new()),
            );
        summary.render(area, buf)
    }

    fn render_progress_bar(&self, buf: &mut Buffer, area: Rect) {
        // take up a third of the screen vertically and half horizontally
        let popup_area = Rect {
            x: area.width / 4,
            y: area.height / 3,
            width: area.width / 2,
            height: area.height / 6,
        };

        let title = Line::from(" Working ").centered();
        let label = Span::styled(format!("{} files", self.current_state), Style::new().bold());

        let ratio = match self.current_state {
            State::Processing { done, total } | State::Comparing { done, total } => {
                done as f64 / total as f64
            }
            _ => 0.0,
        };

        let title_block = Block::new()
            .title(title)
            .title_style(Style::new().bold().white())
            .title_bottom(
                Line::from(vec![" Quit ".into(), "<q/esc> ".blue().bold()]).right_aligned(),
            )
            .borders(Borders::ALL)
            .border_type(BorderType::Thick)
            .border_style(Color::Green);

        let gauge = Gauge::default()
            .block(title_block)
            .ratio(ratio)
            .label(label);

        gauge.render(popup_area, buf);
    }

    fn render_summary(&self, buf: &mut Buffer, area: Rect) {
        // Acquire the lock to pull needed data, then drop it.
        let dirs: Vec<PathBuf> = {
            let file_index = self.file_index.read().unwrap();
            file_index.dirs.clone().into_iter().collect()
        };

        let dir_lines: Vec<String> = dirs
            .iter()
            .map(|d| {
                format!(
                    "./{}",
                    deckard::to_relative_path(d)
                        .to_string_lossy()
                        .to_string()
                        .yellow(),
                )
            })
            .collect();

        let dir_joined = dir_lines.join(" ");

        let summary_lines = vec![
            Line::from(vec![
                "Sort by: ".into(),
                format!("{:?}", self.sort_by).blue(),
                " State: ".into(),
                format!("{}", self.current_state)
                    .set_style(Style::default().fg(self.current_state.get_color())),
            ]),
            Line::from(vec!["Paths: ".into(), dir_joined.yellow()]),
        ];

        let summary_text = Text::from(summary_lines);

        let summary = Paragraph::new(summary_text).style(Style::new()).block(
            Block::bordered()
                .border_type(BorderType::Plain)
                .border_style(Style::new()),
        );
        summary.render(area, buf)
    }

    fn render_footer(&self, buf: &mut Buffer, area: Rect) {
        let more = if self.show_more_keys {
            " less "
        } else {
            " more "
        };
        let selected_style = if self.active_selected_file().is_none() {
            Style::new().dark_gray().bold()
        } else {
            Style::new().blue().bold()
        };
        let marked_style = if self.marked_files.is_empty() {
            Style::new().dark_gray().bold()
        } else {
            Style::new().blue().bold()
        };
        let instructions_text = vec![
            "Mark file ".into(),
            "<space>".set_style(selected_style),
            " Mark all clones ".into(),
            "<a>".blue().bold(),
            " Open file ".into(),
            "<o>".set_style(selected_style),
            " Open path ".into(),
            "<p>".set_style(selected_style),
            " Trash ".into(),
            "<T/backspace>".set_style(marked_style),
            " Delete ".into(),
            "<D/delete>".set_style(marked_style),
            " Quit ".into(),
            "<q/esc>".blue().bold(),
            more.into(),
            "<.>".blue().bold(),
        ];

        let more_instructions_text = if self.show_more_keys {
            vec![
                " Focus left ".into(),
                "<h/left>".blue().bold(),
                " Focus right ".into(),
                "<l/right>".blue().bold(),
                " Copy path ".into(),
                "<y>".set_style(selected_style),
                " Sort by ".into(),
                "<s>".blue().bold(),
                " Clear marked ".into(),
                "<A>".set_style(marked_style),
                " Show marked ".into(),
                "<m>".blue().bold(),
                " Show clones ".into(),
                "<c>".blue().bold(),
                " Show info ".into(),
                "<i>".blue().bold(),
                " About ".into(),
                "<?>".blue().bold(),
            ]
        } else {
            vec![]
        };

        let instructions = vec![
            Line::from(instructions_text),
            Line::from(more_instructions_text),
        ];
        let info_footer = Paragraph::new(instructions).style(Style::new());
        info_footer.render(area, buf)
    }

    fn render_main(&mut self, buf: &mut Buffer, area: Rect) {
        // count shown panes
        let window_count = [
            true,
            self.show_file_info,
            self.show_clones_table,
            self.show_marked_table,
        ]
        .iter()
        .filter(|&&enabled| enabled)
        .count();

        let (
            main_vertical_constrains,
            main_horiozntal_top_constrains,
            main_horiozntal_bottom_constrains,
        ) = match window_count {
            1 => (
                [Constraint::Percentage(100), Constraint::Percentage(0)],
                [Constraint::Percentage(100), Constraint::Percentage(0)],
                [Constraint::Percentage(100), Constraint::Percentage(0)],
            ),
            2 => (
                [Constraint::Percentage(100), Constraint::Percentage(0)],
                [Constraint::Percentage(50), Constraint::Percentage(50)],
                [Constraint::Percentage(100), Constraint::Percentage(0)],
            ),
            3 => (
                [Constraint::Percentage(60), Constraint::Percentage(40)],
                [Constraint::Percentage(50), Constraint::Percentage(50)],
                [Constraint::Percentage(100), Constraint::Percentage(0)],
            ),
            _ => (
                [Constraint::Percentage(60), Constraint::Percentage(40)],
                [Constraint::Percentage(50), Constraint::Percentage(50)],
                [Constraint::Percentage(50), Constraint::Percentage(50)],
            ),
        };

        let main_sub_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints(main_vertical_constrains)
            .split(area);

        let main_sub_area_top = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(main_horiozntal_top_constrains)
            .split(main_sub_area[0]);

        let main_sub_area_bottom = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(main_horiozntal_bottom_constrains)
            .split(main_sub_area[1]);

        self.file_table.render(
            buf,
            main_sub_area_top[0], // top left
            matches!(self.focused_window, FocusedWindow::Files),
            &self.marked_files,
        );
        if self.show_clones_table {
            self.clone_table.render(
                buf,
                main_sub_area_top[1], // top right
                matches!(self.focused_window, FocusedWindow::Clones),
                &self.marked_files,
            );
        }
        if self.show_marked_table {
            let rect_area = if window_count == 2 {
                main_sub_area_top[1] // top right
            } else {
                main_sub_area_bottom[0] // bottom left
            };
            self.marked_table.render(
                buf,
                rect_area,
                matches!(self.focused_window, FocusedWindow::Marked),
                &self.marked_files,
            );
        }
        if self.show_file_info {
            let rect_area = match window_count {
                2 => main_sub_area_top[1],                           // top right
                3 if self.show_marked_table => main_sub_area_top[1], // top right
                3 => main_sub_area_bottom[0],                        // bottom left
                _ => main_sub_area_bottom[1],                        // bottom right
            };
            self.render_file_info(buf, rect_area);
        }
    }

    fn render_ui(&mut self, area: Rect, buf: &mut Buffer) {
        let footer_height = if self.show_more_keys { 2 } else { 1 };

        let rects = Layout::vertical([
            Constraint::Length(1),             // header
            Constraint::Min(5),                // main content
            Constraint::Max(4),                // summary
            Constraint::Length(footer_height), // footer
        ])
        .split(area);

        self.render_header(buf, rects[0]);

        if self.is_done() {
            self.render_main(buf, rects[1]);
            self.render_summary(buf, rects[2]);
            self.render_footer(buf, rects[3]);
        } else {
            self.render_progress_bar(buf, area);
        }
    }

    fn handle_state(&mut self, state: State) {
        self.current_state = state;

        if self.current_state == State::Done {
            self.update_tables();
        }
    }
}

async fn index_files(
    file_index: Arc<RwLock<FileIndex>>,
    tx: UnboundedSender<State>,
    cancel_flag: Arc<AtomicBool>,
) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let mut fi = file_index.write().unwrap();

        let progress_callback = Arc::new(move |done: usize| {
            let _ = tx.send(State::Indexing { done });
        });

        fi.index_dirs(Some(progress_callback), Some(cancel_flag));
    })
    .await?;
    Ok(())
}

async fn process_files(
    file_index: Arc<RwLock<FileIndex>>,
    tx: UnboundedSender<State>,
    cancel_flag: Arc<AtomicBool>,
) -> Result<()> {
    // If the I/O or hashing is heavy, prefer spawn_blocking:
    tokio::task::spawn_blocking(move || {
        let mut fi = file_index.write().unwrap();

        // Provide a callback to `process_files` that sends progress:
        let progress_callback = Arc::new(move |done: usize, total: usize| {
            let _ = tx.send(State::Processing { done, total });
        });

        fi.process_files(Some(progress_callback), Some(cancel_flag));
    })
    .await?;
    Ok(())
}

async fn find_duplicates(
    file_index: Arc<RwLock<FileIndex>>,
    tx: UnboundedSender<State>,
    cancel_flag: Arc<AtomicBool>,
) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let mut fi = file_index.write().unwrap();

        let progress_callback = Arc::new(move |done: usize, total: usize| {
            let _ = tx.send(State::Comparing { done, total });
        });

        fi.find_duplicates(Some(progress_callback), Some(cancel_flag));
        fi.cleanup_index();
    })
    .await?;
    Ok(())
}

/// Make the path relative to the commont search parth
pub fn format_path(path: &PathBuf, target_paths: &HashSet<PathBuf>) -> String {
    let common_path = deckard::find_common_path(target_paths);

    let relative_path = if let Some(common_path) = &common_path {
        path.strip_prefix(common_path).unwrap_or(path)
    } else {
        path
    };
    relative_path.to_string_lossy().to_string()
}
