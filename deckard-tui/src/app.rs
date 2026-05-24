use crate::command::{Command, CommandProcessor};
use crate::constants;
use crate::dirtable::{DirTable, DirectoryInfo};
use arboard::Clipboard;
use chrono::{DateTime, Local};
use color_eyre::eyre::{Result, WrapErr};
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use deckard::config::SearchConfig;
use deckard::file::FileEntry;
use deckard::index::FileIndex;
use futures::StreamExt;
use ratatui::widgets::Padding;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Flex, Layout, Rect},
    style::{Color, Style, Styled, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, Gauge, Paragraph, Widget, Wrap},
};
use std::sync::{Arc, RwLock};
use std::time::Instant;
use std::{
    collections::HashSet,
    env, fmt, fs,
    hash::{DefaultHasher, Hash, Hasher},
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, SystemTime},
};
use tokio::{sync::watch, task::AbortHandle};
use tracing::{debug, error, warn};

#[derive(Debug, Default)]
enum FocusedWindow {
    #[default]
    Files,
    Clones,
    Marked,
    Popup,
}

#[derive(Debug, Default)]
pub enum Mode {
    #[default]
    Normal,
    Command,
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let result = match self {
            Self::Normal => "Normal",
            Self::Command => "Command",
        };
        write!(f, "{result}")
    }
}

impl Mode {
    fn get_color(&self) -> Color {
        match self {
            Self::Normal => Color::Blue,
            Self::Command => Color::Yellow,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
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

pub struct App {
    focused_window: FocusedWindow,
    should_exit: bool,
    dry_run: bool,
    remove_dirs: bool,
    file_index: Arc<RwLock<FileIndex>>,
    file_table: DirTable,
    clone_table: DirTable,
    marked_table: DirTable,
    marked_files: HashSet<Arc<PathBuf>>,
    marked_dirs: HashSet<PathBuf>,
    disk_usage_mode: bool,
    show_clones_table: bool,
    show_marked_table: bool,
    show_file_info: bool,
    show_more_keys: bool,
    current_state: State,
    sort_by: Sorting,
    mode: Mode,
    command_processor: CommandProcessor,
    clipboard: Option<Clipboard>,
    cancel_flag: Arc<AtomicBool>,
    abort_handle: Option<AbortHandle>,
    display_filter: Option<String>,
    warning_message: Option<String>,
    flatten_dirs: bool,
    debug: bool,
    frame_count: usize,
    last_render: Instant,
    last_render_took: Duration,
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
        disk_usage: bool,
        debug: bool,
    ) -> Self {
        let clipboard = Clipboard::new().map_or_else(
            |e| {
                error!("Failed to create Clipboard {}", e);
                None
            },
            Some,
        );

        let commands = vec![
            Command {
                command: "quit",
                alias: Some("q"),
            },
            Command {
                command: "help",
                alias: Some("h"),
            },
            Command {
                command: "filter",
                alias: Some("f"),
            },
            Command {
                command: "parent_filter",
                alias: Some("pf"),
            },
            Command {
                command: "clear_filter",
                alias: Some("cf"),
            },
            Command {
                command: "mark_filter",
                alias: Some("mf"),
            },
            Command {
                command: "mark_parent",
                alias: Some("mp"),
            },
            Command {
                command: "mark_all",
                alias: Some("ma"),
            },
            Command {
                command: "clear_marked",
                alias: Some("cm"),
            },
            Command {
                command: "flatten_dirs",
                alias: Some("fd"),
            },
        ];

        // don't show clone count for disk_usage mode
        let file_table = if disk_usage {
            DirTable::new(vec![" ", "File", "Date", "Size"], true, false, false)
        } else {
            DirTable::new(
                vec![" ", "File", "Date", "Size", "Clones"],
                true,
                true,
                false,
            )
        };

        Self {
            focused_window: FocusedWindow::Files,
            should_exit: false,
            file_index: Arc::new(RwLock::new(FileIndex::new(target_paths, config))),
            file_table,
            clone_table: DirTable::new(vec![" ", "Clone", "Date", "Size"], true, false, true),
            marked_table: DirTable::new(vec![" ", "Marked"], false, false, true),
            marked_files: HashSet::new(),
            marked_dirs: HashSet::new(),
            disk_usage_mode: disk_usage,
            show_marked_table: true,
            show_clones_table: !disk_usage,
            show_file_info: true,
            show_more_keys: false,
            current_state: State::Idle,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            abort_handle: None,
            clipboard,
            sort_by: Sorting::default(),
            mode: Mode::Normal,
            command_processor: CommandProcessor::new(commands, 16),
            dry_run,
            remove_dirs,
            display_filter: None,
            warning_message: None,
            flatten_dirs: false,
            debug,
            frame_count: 0,
            last_render: Instant::now(),
            last_render_took: Duration::default(),
        }
    }

    /// runs the application's main loop until the user quits
    pub async fn run(&mut self, terminal: &mut crate::tui::Tui) -> Result<()> {
        let period = Duration::from_secs_f32(1.0 / Self::FRAMES_PER_SECOND);
        let mut interval = tokio::time::interval(period);
        let mut events = EventStream::new();

        // TODO: Handle graceful shutdown
        let (tx, mut rx) = watch::channel(State::Idle);
        let file_index = self.file_index.clone();
        let task_cancel_flag = self.cancel_flag.clone();
        let disk_usage_mode = self.disk_usage_mode;
        let task_handle = tokio::spawn(async move {
            if let Err(e) =
                index_files(file_index.clone(), tx.clone(), task_cancel_flag.clone()).await
            {
                let _ = tx.send(State::Error(format!("index_files error: {e}")));
            }
            if !disk_usage_mode {
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
            }
            let _ = tx.send(State::Done);
        });
        self.abort_handle = Some(task_handle.abort_handle());

        let mut should_redraw = true;
        while !self.should_exit {
            tokio::select! {
                // interval timer
                _ = interval.tick() => {
                    should_redraw |= self.handle_state(rx.borrow_and_update().clone());
                    if should_redraw {
                        should_redraw = false;
                        let render_start = Instant::now();
                        terminal.draw(|frame| {
                            self.render_ui(frame.area(), frame.buffer_mut());
                            self.frame_count = frame.count();
                        })?;
                        self.last_render_took = render_start.elapsed();
                        self.last_render = render_start;
                    }
                }
                // terminal events
                Some(Ok(event)) = events.next() => {
                    should_redraw |= self.handle_events(event)?;
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
    fn handle_events(&mut self, event: Event) -> Result<bool> {
        match event {
            // it's important to check that the event is a key press event as
            // crossterm also emits key release and repeat events on Windows.
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => self
                .handle_key_event(key_event)
                .wrap_err_with(|| format!("handling key event failed:\n{key_event:#?}")),
            Event::Resize(_, _) => Ok(true),
            _ => Ok(false),
        }
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> Result<bool> {
        let changed = match self.mode {
            Mode::Normal => {
                let handled = match key_event.code {
                    // page move
                    KeyCode::Char('J') | KeyCode::Down
                        if key_event.modifiers.contains(KeyModifiers::SHIFT) =>
                    {
                        self.next_file(true);
                        true
                    }
                    KeyCode::Char('K') | KeyCode::Up
                        if key_event.modifiers.contains(KeyModifiers::SHIFT) =>
                    {
                        self.previous_file(true);
                        true
                    }
                    // regular move
                    KeyCode::Char('j') | KeyCode::Down => {
                        self.next_file(false);
                        true
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        self.previous_file(false);
                        true
                    }

                    // dir navigation
                    KeyCode::Right if key_event.modifiers.contains(KeyModifiers::SHIFT) => {
                        self.enter_dir(false);
                        true
                    }
                    KeyCode::Enter => {
                        self.enter_dir(false);
                        true
                    }
                    KeyCode::Left if key_event.modifiers.contains(KeyModifiers::SHIFT) => {
                        self.enter_dir(true);
                        true
                    }

                    KeyCode::Char('q') => self.exit(),
                    KeyCode::Esc => self.handle_escape(),
                    KeyCode::Char('i') => {
                        self.toggle_info();
                        true
                    }
                    KeyCode::Char('o') => {
                        self.open_file();
                        true
                    }
                    KeyCode::Char('p') => {
                        self.open_path();
                        true
                    }
                    KeyCode::Char('D') | KeyCode::Delete => {
                        self.delete();
                        true
                    }
                    KeyCode::Char('T') | KeyCode::Backspace => {
                        self.trash();
                        true
                    }
                    KeyCode::Char('c') => {
                        self.toggle_show_clones_table();
                        true
                    }
                    KeyCode::Char(' ') => {
                        self.mark();
                        true
                    }
                    KeyCode::Char('a') => {
                        self.mark_all_clones();
                        true
                    }
                    KeyCode::Char('A') => {
                        self.clear_marked();
                        true
                    }
                    KeyCode::Char('m') => {
                        self.toggle_show_marked_table();
                        true
                    }
                    KeyCode::Char('y') => {
                        self.copy_path();
                        true
                    }
                    KeyCode::Char('.') => {
                        self.toggle_more_keys();
                        true
                    }
                    KeyCode::Char('?') => self.toggle_about(),
                    KeyCode::Char('s') => {
                        self.cycle_sort_by();
                        true
                    }
                    KeyCode::Char('l') | KeyCode::Right | KeyCode::Tab => {
                        self.focus_next_table();
                        true
                    }
                    KeyCode::Char('h') | KeyCode::Left | KeyCode::BackTab => {
                        self.focus_previus_table();
                        true
                    }
                    KeyCode::Char(':') => self.enter_command_mode(),
                    _ => false,
                };
                if handled {
                    self.clear_warning();
                }
                handled
            }
            Mode::Command => match key_event.code {
                KeyCode::Esc => {
                    self.command_processor.reset_command();
                    self.exit_command_mode();
                    true
                }
                KeyCode::Enter => {
                    self.handle_command();
                    true
                }
                KeyCode::Backspace => {
                    self.command_processor.delete_char();
                    true
                }
                KeyCode::Tab => false,
                KeyCode::Up => {
                    self.command_processor.last_command();
                    true
                }
                KeyCode::Down => {
                    self.command_processor.next_command();
                    true
                }
                KeyCode::Left => {
                    self.command_processor.move_cursor_left();
                    true
                }
                KeyCode::Right => {
                    self.command_processor.move_cursor_right();
                    true
                }
                KeyCode::Char(c) => {
                    self.command_processor.enter_char(c);
                    true
                }
                _ => false,
            },
        };
        Ok(changed)
    }

    fn toggle_flatten_dirs(&mut self) {
        self.flatten_dirs = !self.flatten_dirs;
        self.file_table.flatten_dirs(self.flatten_dirs);
        self.update_file_table();
    }

    fn toggle_about(&mut self) -> bool {
        if matches!(self.focused_window, FocusedWindow::Popup) {
            self.focused_window = FocusedWindow::Files;
        } else {
            self.focused_window = FocusedWindow::Popup;
        }
        true
    }

    fn exit(&mut self) -> bool {
        self.cancel_flag.store(true, Ordering::Relaxed);
        if let Some(abort_handle) = &self.abort_handle {
            abort_handle.abort();
        }
        self.should_exit = true;
        true
    }

    fn handle_escape(&mut self) -> bool {
        if !self.is_done() {
            return self.exit();
        }

        match self.focused_window {
            FocusedWindow::Popup => {
                self.focus_files_table();
                true
            }
            FocusedWindow::Clones | FocusedWindow::Marked => {
                self.focus_files_table();
                true
            }
            FocusedWindow::Files => {
                if self.file_table.back_parent_dir() {
                    self.update_clone_table();
                    true
                } else {
                    false
                }
            }
        }
    }

    fn enter_command_mode(&mut self) -> bool {
        if matches!(self.mode, Mode::Normal)
            && matches!(self.current_state, State::Done)
            && !matches!(self.focused_window, FocusedWindow::Popup)
        {
            self.mode = Mode::Command;
            true
        } else {
            false
        }
    }

    fn exit_command_mode(&mut self) {
        self.mode = Mode::Normal;
    }

    fn handle_command(&mut self) {
        if let Some(command) = self.command_processor.submit_command() {
            match command.name.as_str() {
                "quit" => {
                    self.exit();
                }
                "help" => {
                    self.toggle_about();
                }
                "mark_all" => {
                    self.mark_all();
                }
                "clear_marked" => {
                    self.clear_marked();
                }
                "mark_filter" => {
                    if let Some(filter) = command.args.first() {
                        self.mark_filter(filter);
                    }
                }
                "mark_parent" => {
                    if let Some(file) = self.active_selected_file()
                        && let Some(parent) = file.parent()
                    {
                        self.mark_filter(&parent.to_string_lossy());
                    }
                }
                "parent_filter" => {
                    if let Some(file) = self.active_selected_file()
                        && let Some(parent) = file.parent()
                    {
                        self.set_filter(&parent.to_string_lossy());
                    }
                }
                "clear_filter" => {
                    self.clear_filter();
                }
                "filter" => {
                    if let Some(filter) = command.args.first() {
                        self.set_filter(filter);
                    }
                }
                "flatten_dirs" => {
                    self.toggle_flatten_dirs();
                }
                _ => {
                    self.set_warning(format!("Failed to match command: {}", command.name));
                }
            }
        } else {
            self.set_warning("Unknown command".to_string());
        }
        self.exit_command_mode();
    }

    fn set_filter(&mut self, filter: &str) {
        self.display_filter = Some(filter.to_string());
        self.update_tables();
    }

    fn clear_filter(&mut self) {
        self.display_filter = None;
        self.update_tables();
    }

    fn set_warning(&mut self, message: String) {
        self.warning_message = Some(message);
    }

    fn clear_warning(&mut self) {
        self.warning_message = None;
    }

    fn enter_dir(&mut self, back: bool) {
        if matches!(self.focused_window, FocusedWindow::Files) {
            if back {
                self.file_table.back_parent_dir();
            } else {
                self.file_table.enter_selected_dir();
            }
            self.update_clone_table()
        }
    }

    fn mark(&mut self) {
        let changed = if matches!(self.focused_window, FocusedWindow::Files) {
            let dir_paths = self.file_table.selected_dir_file_paths();
            if dir_paths.is_empty() {
                self.active_selected_file()
                    .is_some_and(|path| toggle_marked_paths(&mut self.marked_files, &[path]))
            } else {
                toggle_marked_paths(&mut self.marked_files, &dir_paths)
            }
        } else {
            self.active_selected_file()
                .is_some_and(|path| toggle_marked_paths(&mut self.marked_files, &[path]))
        };

        if changed {
            self.update_marked_table();
            if matches!(self.focused_window, FocusedWindow::Marked) {
                self.marked_table.select_previous(1);
            }
        }
    }

    fn mark_all(&mut self) {
        self.marked_files
            .extend(self.file_table.current_file_paths());

        self.update_marked_table();
    }

    fn mark_all_clones(&mut self) {
        self.marked_files
            .extend(self.clone_table.current_file_paths());

        self.update_marked_table();
    }

    fn mark_filter(&mut self, filter: &str) {
        for p in self.file_table.current_file_paths() {
            if p.to_string_lossy().contains(filter) {
                self.marked_files.insert(p);
            }
        }

        self.update_marked_table();
    }

    fn clear_marked(&mut self) {
        self.marked_files = HashSet::new();
        self.update_marked_table();
        if matches!(self.focused_window, FocusedWindow::Marked) {
            self.marked_table.select_none();
        }
    }

    fn update_marked_table(&mut self) {
        let paths: Vec<Arc<PathBuf>> = self.marked_files.iter().cloned().collect();
        self.marked_table
            .update_table(&paths, &self.file_index, None);
        self.refresh_marked_dirs();
    }

    fn refresh_marked_dirs(&mut self) {
        self.marked_dirs = self.file_table.marked_directory_paths(&self.marked_files);
    }

    fn remove_marked(&mut self, remove_callback: fn(&PathBuf) -> Result<(), ()>) {
        {
            let mut index = self.file_index.write().unwrap();
            for file in &self.marked_files {
                if !self.dry_run {
                    if file.is_file() && remove_callback(file).is_err() {
                        self.warning_message = Some("Delete failed".to_string());
                    }
                    if self.remove_dirs {
                        // Delete any empty dirs
                        let mut path = file.parent();
                        loop {
                            if let Some(parent) = path
                                && parent.is_dir()
                                && parent.components().count() > 2
                            {
                                match fs::read_dir(parent) {
                                    Ok(dir) => {
                                        if dir.count() == 0 {
                                            debug!("directory empty, deleting: {parent:?}");
                                            if remove_callback(&parent.to_path_buf()).is_err() {
                                                warn!("failed deleting: {parent:?}");
                                                break;
                                            }
                                            path = parent.parent();
                                        } else {
                                            break;
                                        }
                                    }
                                    Err(e) => {
                                        warn!("failed reading parent directory: {e}");
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                index.remove_from_index(file);
            }
        }
        self.clear_marked();
        self.update_tables();
    }

    fn active_selected_file(&self) -> Option<Arc<PathBuf>> {
        match self.focused_window {
            FocusedWindow::Files => self.file_table.selected_file_path(),
            FocusedWindow::Clones => self.clone_table.selected_path(),
            FocusedWindow::Marked => self.marked_table.selected_path(),
            _ => None,
        }
    }

    fn active_selected_path(&self) -> Option<Arc<PathBuf>> {
        match self.focused_window {
            FocusedWindow::Files => self.file_table.selected_resolved_path(),
            FocusedWindow::Clones => self.clone_table.selected_path(),
            FocusedWindow::Marked => self.marked_table.selected_path(),
            FocusedWindow::Popup => None,
        }
    }

    fn active_selected_parent_path(&self) -> Option<PathBuf> {
        self.active_selected_path()
            .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
    }

    fn active_mark_target_exists(&self) -> bool {
        match self.focused_window {
            FocusedWindow::Files => self.file_table.selected_path().is_some(),
            FocusedWindow::Clones | FocusedWindow::Marked => self.active_selected_file().is_some(),
            FocusedWindow::Popup => false,
        }
    }

    fn copy_path(&mut self) {
        if let Some(selected_path) = self.active_selected_path() {
            self.clipboard.as_mut().map_or_else(
                || warn!("No Clipboard found"),
                |clipboard| {
                    debug!("Copying path to clipboard {:?}", selected_path);
                    if let Err(e) = clipboard.set_text(selected_path.to_string_lossy()) {
                        error!("Failed to set Clipboard {}", e);
                    }
                },
            );
        }
    }

    fn open_file(&mut self) {
        if let Some(selected_path) = self.active_selected_path() {
            _ = open::that_detached(selected_path.as_ref());
        }
    }

    fn open_path(&mut self) {
        if let Some(path) = self.active_selected_parent_path() {
            _ = open::that_detached(path);
        }
    }

    fn delete(&mut self) {
        self.remove_marked(|f| match fs::remove_file(f) {
            Ok(_) => Ok(()),
            Err(e) => {
                error!("Error deleting file {f:?}: {e}");
                Err(())
            }
        });
    }

    fn trash(&mut self) {
        self.remove_marked(|f| match trash::delete(f) {
            Ok(_) => Ok(()),
            Err(e) => {
                error!("Error deleting file {f:?}: {e}");
                Err(())
            }
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

    pub fn next_file(&mut self, jump: bool) {
        let step = if jump { 10 } else { 1 };

        match self.focused_window {
            FocusedWindow::Files => {
                self.file_table.select_next(step);
                self.update_clone_table();
            }
            FocusedWindow::Clones => {
                self.clone_table.select_next(step);
            }
            FocusedWindow::Marked => {
                self.marked_table.select_next(step);
            }
            _ => {}
        }
    }

    pub fn previous_file(&mut self, jump: bool) {
        let step = if jump { 10 } else { 1 };

        match self.focused_window {
            FocusedWindow::Files => {
                self.file_table.select_previous(step);
                self.update_clone_table();
            }
            FocusedWindow::Clones => {
                self.clone_table.select_previous(step);
            }
            FocusedWindow::Marked => {
                self.marked_table.select_previous(step);
            }
            _ => {}
        }
    }

    fn update_file_table(&mut self) {
        let update_start = Instant::now();
        let paths: Vec<Arc<PathBuf>> = if self.disk_usage_mode {
            // use files map for disk usage mode
            self.file_index
                .read()
                .unwrap()
                .files
                .keys()
                .filter(|k| {
                    self.display_filter
                        .as_ref()
                        .is_none_or(|filter| k.to_string_lossy().contains(filter))
                })
                .cloned()
                .collect()
        } else {
            // use duplicates map for regular mode
            self.file_index
                .read()
                .unwrap()
                .duplicates
                .keys()
                .filter(|k| {
                    self.display_filter
                        .as_ref()
                        .is_none_or(|filter| k.to_string_lossy().contains(filter))
                })
                .cloned()
                .collect()
        };

        if !paths.is_empty() {
            self.file_table
                .update_table(&paths, &self.file_index, Some(&self.sort_by));
            self.file_table.select_first();
        } else {
            self.file_table.clear();
        }
        self.refresh_marked_dirs();

        debug!(
            "update_file_table built from {} paths in {:?} (filter={:?}, flattened={})",
            paths.len(),
            update_start.elapsed(),
            self.display_filter,
            self.flatten_dirs
        );
    }

    fn update_clone_table(&mut self) {
        if let Some(selected_file) = self.file_table.selected_path().as_ref()
            && !self.file_table.selected_path_is_dir()
        {
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

        let (title_left, title_right) = if self.debug {
            let last_render_took = self.last_render_took.as_secs_f32();
            let last_render_ago = self.last_render.elapsed().as_secs_f32();

            let render_fps_line = Line::from(format!(
                " frame {}, {:.2} mS ({:.1} FPS)",
                self.frame_count,
                last_render_ago * 1000.0,
                1.0 / last_render_ago,
            ));

            let render_took_line = Line::from(format!(
                "took {:.2} mS ({:.1} FPS) ",
                last_render_took * 1000.0,
                1.0 / last_render_took
            ));

            (render_fps_line, render_took_line)
        } else {
            (Line::default(), Line::default())
        };

        let block = Block::new()
            .style(Style::new().gray().reversed())
            .title_bottom(title_left.left_aligned())
            .title_bottom(title_right.right_aligned())
            .title_bottom(title.centered());
        block.render(area, buf)
    }

    fn render_file_info(&self, buf: &mut Buffer, area: Rect) {
        let info_lines = self.selected_info_lines();

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

    fn selected_info_lines(&self) -> Vec<Line<'static>> {
        if let Some(file_entry) = self.selected_file_entry() {
            file_info_lines(&file_entry)
        } else if let Some(dir_info) = self.selected_directory_info() {
            directory_info_lines(&dir_info)
        } else {
            vec![Line::from(vec!["none".into()])]
        }
    }

    fn selected_file_entry(&self) -> Option<FileEntry> {
        let selected_file = self.active_selected_file()?;
        self.file_index
            .read()
            .unwrap()
            .files
            .get(&selected_file)
            .cloned()
    }

    fn selected_directory_info(&self) -> Option<DirectoryInfo> {
        if matches!(self.focused_window, FocusedWindow::Files) {
            self.file_table.selected_dir_info()
        } else {
            None
        }
    }

    fn render_progress_bar(&self, buf: &mut Buffer, area: Rect) {
        let popup_area = popup_area(area, 60, 30);

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

    fn render_about(&self, buf: &mut Buffer, area: Rect) {
        // take up a third of the screen vertically and half horizontally
        let popup_area = popup_area(area, 60, 60);

        let title = Line::from(" About ").centered();

        let title_block = Block::new()
            .title(title)
            .title_style(Style::new().bold().white())
            .title_bottom(
                Line::from(vec![" Hide ".into(), "<?/esc> ".blue().bold()]).right_aligned(),
            )
            .padding(Padding::horizontal(1))
            .borders(Borders::ALL)
            .border_type(BorderType::Thick)
            .border_style(Color::LightMagenta);

        Clear.render(popup_area, buf);

        let help_text = Text::raw(format!(
            "{}\n{}",
            constants::HELP_LOGO,
            constants::HELP_TEXT
        ));

        Paragraph::new(help_text)
            .block(title_block)
            // .gray()
            .left_aligned()
            // .scroll((self.scroll, 0))
            // .wrap(Wrap { trim: false })
            .render(popup_area, buf);
    }

    fn render_summary(&self, buf: &mut Buffer, area: Rect) {
        // Acquire the lock to pull needed data, then drop it.
        let dirs: Vec<PathBuf> = {
            let file_index = self.file_index.read().unwrap();
            file_index.dirs.clone().into_iter().collect()
        };

        let dir_lines: Vec<String> = dirs
            .iter()
            .map(|d| format!("./{}", deckard::to_relative_path(d).display()))
            .collect();

        let dir_joined = dir_lines.join(" ");

        let path_line = match self.mode {
            Mode::Normal => Line::from(if self.warning_message.is_none() {
                vec!["Paths: ".into(), dir_joined.yellow()]
            } else {
                vec![
                    self.warning_message
                        .as_ref()
                        .unwrap_or(&"".to_string())
                        .to_string()
                        .set_style(Style::default().fg(if self.warning_message.is_none() {
                            Color::default()
                        } else {
                            Color::Red
                        })),
                ]
            }),
            Mode::Command => Line::from(vec![
                ":".into(),
                self.command_processor.input.clone().into(),
            ]),
        };

        let summary_lines = vec![
            Line::from(vec![
                "Mode: ".into(),
                format!("{}", self.mode).set_style(Style::default().fg(self.mode.get_color())),
                " State: ".into(),
                format!("{}", self.current_state)
                    .set_style(Style::default().fg(self.current_state.get_color())),
                " Sort: ".into(),
                format!("{}", self.sort_by).blue(),
                " Filter: ".into(),
                self.display_filter
                    .as_ref()
                    .unwrap_or(&"None".to_string())
                    .to_string()
                    .set_style(Style::default().fg(if self.display_filter.is_none() {
                        Color::DarkGray
                    } else {
                        Color::LightMagenta
                    })),
            ]),
            path_line,
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
        let mark_style = if self.active_mark_target_exists() {
            Style::new().blue().bold()
        } else {
            Style::new().dark_gray().bold()
        };
        let selected_style = if self.active_selected_path().is_none() {
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
            "Mark ".into(),
            "<space>".set_style(mark_style),
            " Mark all clones ".into(),
            "<a>".blue().bold(),
            " Open ".into(),
            "<o>".set_style(selected_style),
            " Open path ".into(),
            "<p>".set_style(selected_style),
            " Trash ".into(),
            "<T/backspace>".set_style(marked_style),
            " Delete ".into(),
            "<D/delete>".set_style(marked_style),
            " Back ".into(),
            "<esc>".blue().bold(),
            " Quit ".into(),
            "<q>".blue().bold(),
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
            &self.marked_dirs,
        );
        if self.show_clones_table {
            self.clone_table.render(
                buf,
                main_sub_area_top[1], // top right
                matches!(self.focused_window, FocusedWindow::Clones),
                &self.marked_files,
                &self.marked_dirs,
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
                &self.marked_dirs,
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
            if matches!(self.focused_window, FocusedWindow::Popup) {
                self.render_about(buf, area);
            }
        } else {
            self.render_progress_bar(buf, area);
        }
    }

    fn handle_state(&mut self, state: State) -> bool {
        if self.current_state == state {
            return false;
        }

        let reached_done = state == State::Done;
        self.current_state = state;

        if reached_done {
            let update_start = Instant::now();
            self.update_tables();
            debug!(
                "state transition to Done rebuilt tables in {:?}",
                update_start.elapsed()
            );
        }

        true
    }
}

async fn index_files(
    file_index: Arc<RwLock<FileIndex>>,
    tx: watch::Sender<State>,
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
    tx: watch::Sender<State>,
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
    tx: watch::Sender<State>,
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

/// helper function to create a centered rect using up certain percentage of the available rect `r`
fn popup_area(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let vertical = Layout::vertical([Constraint::Percentage(percent_y)]).flex(Flex::Center);
    let horizontal = Layout::horizontal([Constraint::Percentage(percent_x)]).flex(Flex::Center);
    let [area] = vertical.areas(area);
    let [area] = horizontal.areas(area);
    area
}

fn file_info_lines(file_entry: &FileEntry) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(vec![
            "name: ".into(),
            file_entry
                .name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
                .yellow(),
        ]),
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
            format_system_time(file_entry.created).red(),
        ]),
        Line::from(vec![
            "modified: ".into(),
            format_system_time(file_entry.modified).red(),
        ]),
        Line::from(vec![
            "hash: ".into(),
            match &file_entry.hash {
                Some(h) => h.to_string().cyan(),
                None => "none".into(),
            },
        ]),
        Line::from(vec![
            "path: ".into(),
            deckard::to_relative_path(&file_entry.path)
                .display()
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

    if let Some(image_hash) = &file_entry.image_hash {
        let mut hasher = DefaultHasher::new();
        image_hash.hash(&mut hasher);
        lines.push(Line::from(vec![
            "image_hash: ".into(),
            format!("{:x}", hasher.finish()).to_string().cyan(),
        ]));
    }

    lines
}

fn directory_info_lines(dir_info: &DirectoryInfo) -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            "name: ".into(),
            dir_info.name.clone().yellow(),
            "/".to_string().yellow(),
        ]),
        Line::from(vec![
            "size: ".into(),
            humansize::format_size(dir_info.total_size, humansize::DECIMAL)
                .to_string()
                .blue(),
            " (".into(),
            dir_info.total_size.to_string().blue(),
            ")".into(),
        ]),
        Line::from(vec![
            "created: ".into(),
            format_system_time(dir_info.created).red(),
        ]),
        Line::from(vec![
            "modified: ".into(),
            format_system_time(dir_info.modified).red(),
        ]),
        Line::from(vec![
            "files: ".into(),
            dir_info.file_count.to_string().blue(),
        ]),
        Line::from(vec![
            "directories: ".into(),
            dir_info.subdirectory_count.to_string().blue(),
        ]),
        Line::from(vec![
            "path: ".into(),
            deckard::to_relative_path(&dir_info.path)
                .display()
                .to_string()
                .yellow(),
        ]),
    ]
}

fn format_system_time(time: Option<SystemTime>) -> String {
    time.map(|time| {
        DateTime::<Local>::from(time)
            .format("%d/%m/%Y %H:%M:%S %Z")
            .to_string()
    })
    .unwrap_or_default()
}

fn toggle_marked_paths(marked_files: &mut HashSet<Arc<PathBuf>>, paths: &[Arc<PathBuf>]) -> bool {
    if paths.is_empty() {
        return false;
    }

    if paths.iter().all(|path| marked_files.contains(path)) {
        for path in paths {
            marked_files.remove(path);
        }
    } else {
        marked_files.extend(paths.iter().cloned());
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn esc_key_event() -> KeyEvent {
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
    }

    fn path_arc(path: &str) -> Arc<PathBuf> {
        Arc::new(PathBuf::from(path))
    }

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    fn line_texts(lines: &[Line<'_>]) -> Vec<String> {
        lines.iter().map(line_text).collect()
    }

    fn app_with_nested_file_table() -> App {
        let root = PathBuf::from("/tmp/deckard-esc-test");
        let target_paths = HashSet::from([root.clone()]);
        let mut app = App::new(
            target_paths,
            SearchConfig::default(),
            true,
            false,
            true,
            false,
        );
        let paths = vec![Arc::new(root.join("deckard-tui/Cargo.toml"))];
        app.file_table
            .update_table(&paths, &app.file_index, Some(&Sorting::Path));
        app.file_table.select_first();
        app.file_table.enter_selected_dir();
        app.current_state = State::Done;
        app
    }

    fn app_with_selected_directory() -> (App, Arc<PathBuf>, Arc<PathBuf>, Arc<PathBuf>) {
        let root = PathBuf::from("/tmp/deckard-mark-test");
        let target_paths = HashSet::from([root.clone()]);
        let mut app = App::new(
            target_paths,
            SearchConfig::default(),
            true,
            false,
            true,
            false,
        );
        let direct = Arc::new(root.join("folder/direct.txt"));
        let nested = Arc::new(root.join("folder/sub/nested.txt"));
        let sibling = Arc::new(root.join("other/sibling.txt"));
        let paths = vec![direct.clone(), nested.clone(), sibling.clone()];

        app.file_table
            .update_table(&paths, &app.file_index, Some(&Sorting::Path));
        app.file_table.select_first();

        (app, direct, nested, sibling)
    }

    #[test]
    fn esc_goes_up_when_files_view_is_nested() {
        let mut app = app_with_nested_file_table();

        assert!(app.handle_key_event(esc_key_event()).unwrap());

        assert!(!app.should_exit);
    }

    #[test]
    fn esc_does_not_quit_when_files_view_is_at_top_level() {
        let mut app = app_with_nested_file_table();
        app.handle_key_event(esc_key_event()).unwrap();

        assert!(!app.handle_key_event(esc_key_event()).unwrap());

        assert!(!app.should_exit);
    }

    #[test]
    fn esc_focuses_files_when_clones_view_is_focused() {
        let mut app = app_with_nested_file_table();
        app.focused_window = FocusedWindow::Clones;

        assert!(app.handle_key_event(esc_key_event()).unwrap());

        assert!(!app.should_exit);
        assert!(matches!(app.focused_window, FocusedWindow::Files));
    }

    #[test]
    fn esc_focuses_files_when_marked_view_is_focused() {
        let mut app = app_with_nested_file_table();
        app.focused_window = FocusedWindow::Marked;

        assert!(app.handle_key_event(esc_key_event()).unwrap());

        assert!(!app.should_exit);
        assert!(matches!(app.focused_window, FocusedWindow::Files));
    }

    #[test]
    fn esc_closes_about_window() {
        let mut app = app_with_nested_file_table();
        app.focused_window = FocusedWindow::Popup;

        assert!(app.handle_key_event(esc_key_event()).unwrap());

        assert!(!app.should_exit);
        assert!(matches!(app.focused_window, FocusedWindow::Files));
    }

    #[test]
    fn esc_quits_when_working_progress_bar_is_active() {
        let mut app = app_with_nested_file_table();
        app.current_state = State::Indexing { done: 1 };

        assert!(app.handle_key_event(esc_key_event()).unwrap());

        assert!(app.should_exit);
    }

    #[test]
    fn active_selected_path_resolves_selected_directory() {
        let (app, _direct, _nested, _sibling) = app_with_selected_directory();

        assert_eq!(
            app.active_selected_path().as_deref().map(PathBuf::as_path),
            Some(PathBuf::from("/tmp/deckard-mark-test/folder").as_path())
        );
    }

    #[test]
    fn active_selected_parent_path_uses_directory_parent() {
        let (app, _direct, _nested, _sibling) = app_with_selected_directory();

        assert_eq!(
            app.active_selected_parent_path().as_deref(),
            Some(PathBuf::from("/tmp/deckard-mark-test").as_path())
        );
    }

    #[test]
    fn active_selected_path_keeps_selected_file_path() {
        let app = app_with_nested_file_table();

        assert_eq!(
            app.active_selected_path().as_deref().map(PathBuf::as_path),
            Some(PathBuf::from("/tmp/deckard-esc-test/deckard-tui/Cargo.toml").as_path())
        );
        assert_eq!(
            app.active_selected_parent_path().as_deref(),
            Some(PathBuf::from("/tmp/deckard-esc-test/deckard-tui").as_path())
        );
    }

    #[test]
    fn active_selected_path_is_none_without_selection() {
        let mut app = app_with_nested_file_table();
        app.file_table.select_none();

        assert!(app.active_selected_path().is_none());
        assert!(app.active_selected_parent_path().is_none());
    }

    #[test]
    fn toggle_marked_paths_marks_all_when_partially_marked() {
        let direct = path_arc("/tmp/folder/direct.txt");
        let nested = path_arc("/tmp/folder/sub/nested.txt");
        let outside = path_arc("/tmp/outside.txt");
        let paths = vec![direct.clone(), nested.clone()];
        let mut marked = HashSet::from([direct.clone(), outside.clone()]);

        assert!(toggle_marked_paths(&mut marked, &paths));

        assert!(marked.contains(&direct));
        assert!(marked.contains(&nested));
        assert!(marked.contains(&outside));
    }

    #[test]
    fn toggle_marked_paths_unmarks_all_when_fully_marked() {
        let direct = path_arc("/tmp/folder/direct.txt");
        let nested = path_arc("/tmp/folder/sub/nested.txt");
        let outside = path_arc("/tmp/outside.txt");
        let paths = vec![direct.clone(), nested.clone()];
        let mut marked = HashSet::from([direct.clone(), nested.clone(), outside.clone()]);

        assert!(toggle_marked_paths(&mut marked, &paths));

        assert!(!marked.contains(&direct));
        assert!(!marked.contains(&nested));
        assert!(marked.contains(&outside));
    }

    #[test]
    fn toggle_marked_paths_ignores_empty_paths() {
        let mut marked = HashSet::new();

        assert!(!toggle_marked_paths(&mut marked, &[]));
        assert!(marked.is_empty());
    }

    #[test]
    fn mark_selected_directory_marks_recursive_files_only() {
        let (mut app, direct, nested, sibling) = app_with_selected_directory();

        app.mark();

        assert!(app.marked_files.contains(&direct));
        assert!(app.marked_files.contains(&nested));
        assert!(!app.marked_files.contains(&sibling));
    }

    #[test]
    fn mark_selected_directory_unmarks_when_all_children_marked() {
        let (mut app, direct, nested, sibling) = app_with_selected_directory();
        app.marked_files
            .extend([direct.clone(), nested.clone(), sibling.clone()]);

        app.mark();

        assert!(!app.marked_files.contains(&direct));
        assert!(!app.marked_files.contains(&nested));
        assert!(app.marked_files.contains(&sibling));
    }

    #[test]
    fn mark_selected_directory_marks_all_when_some_children_unmarked() {
        let (mut app, direct, nested, sibling) = app_with_selected_directory();
        app.marked_files.extend([direct.clone(), sibling.clone()]);

        app.mark();

        assert!(app.marked_files.contains(&direct));
        assert!(app.marked_files.contains(&nested));
        assert!(app.marked_files.contains(&sibling));
    }

    #[test]
    fn selected_info_lines_shows_directory_details_for_selected_directory() {
        let (app, _direct, _nested, _sibling) = app_with_selected_directory();

        let texts = line_texts(&app.selected_info_lines());

        assert!(texts.contains(&"name: folder/".to_string()));
        assert!(texts.iter().any(|text| text.starts_with("created: ")));
        assert!(texts.iter().any(|text| text.starts_with("modified: ")));
        assert!(texts.contains(&"files: 2".to_string()));
        assert!(!texts.contains(&"none".to_string()));
    }
}
