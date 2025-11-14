use crate::app::{Sorting, format_path};
use chrono::{DateTime, Local};
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
    collections::{BTreeMap, BTreeSet, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::SystemTime,
};
use tracing::debug;

#[derive(Debug, Default, Clone)]
pub struct DirTableEntry {
    path: Arc<PathBuf>,
    display_path: String,
    size: u64,
    date: Option<SystemTime>,
    clone_count: usize,
    is_dir: bool,
}

impl DirTableEntry {
    fn to_row(
        &self,
        mark_marked: bool,
        is_marked: bool,
        show_clone_count: bool,
        show_name: bool,
    ) -> Row<'_> {
        let size = humansize::format_size(self.size, humansize::DECIMAL);
        let date = self
            .date
            .map(|d| DateTime::<Local>::from(d).format("%d/%m/%Y").to_string())
            .unwrap_or_default();

        let path_style = if self.is_dir {
            Style::new().light_blue()
        } else if mark_marked && is_marked {
            Style::new().yellow()
        } else {
            Style::new()
        };

        let mut cells = vec![
            Cell::from(Text::from(if mark_marked && is_marked { "*" } else { " " })),
            Cell::from(Text::from(
                format!(
                    "{}{}",
                    if show_name {
                        self.path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string()
                    } else {
                        self.display_path.to_string()
                    },
                    if self.is_dir { "/" } else { "" }
                )
                .set_style(path_style),
            )),
            Cell::from(Text::from(date)),
            Cell::from(Text::from(size)),
        ];

        if show_clone_count {
            cells.push(Cell::from(Text::from(self.clone_count.to_string())));
        }

        Row::new(cells).style(Style::new())
    }
}

#[derive(Clone, Debug)]
pub struct DirView {
    pub path: Arc<PathBuf>,
    pub entries: Vec<DirTableEntry>,

    /// Count of files including subdirectories
    pub file_count: usize,
    /// Sum of sizes of files including subdirectories
    pub total_size: u64,
}

impl DirView {
    pub fn parent(&self) -> Option<&Path> {
        self.path.parent()
    }

    pub fn files(&self) -> Vec<DirTableEntry> {
        self.entries
            .iter()
            .filter_map(|f| if !f.is_dir { Some(f.clone()) } else { None })
            .collect()
    }

    pub fn directories(&self) -> Vec<DirTableEntry> {
        self.entries
            .iter()
            .filter_map(|d| if d.is_dir { Some(d.clone()) } else { None })
            .collect()
    }
}

#[derive(Debug, Default)]
pub struct DirTable<'a> {
    pub table_state: TableState,
    pub table_len: usize,
    dir_index: BTreeMap<PathBuf, DirView>,
    current_dir: Option<PathBuf>,
    current_entries: Vec<DirTableEntry>,
    selected_path: Option<Arc<PathBuf>>,
    selected_path_is_dir: bool,
    scroll_state: ScrollbarState,
    mark_marked: bool,
    show_clone_count: bool,
    flatten_dirs: bool,
    total_size: u64,
    selected_dir_history: Vec<usize>,
    // from draw
    table: Table<'a>,
    footer: Row<'a>,
}

impl DirTable<'_> {
    pub fn new(
        header_str: Vec<&'static str>,
        mark_marked: bool,
        show_clone_count: bool,
        flatten_dirs: bool,
    ) -> Self {
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
            current_entries: Vec::new(),
            selected_path: None,
            selected_path_is_dir: false,
            scroll_state: ScrollbarState::new(0),
            mark_marked,
            show_clone_count,
            table,
            footer: Row::default(),
            dir_index: BTreeMap::new(),
            current_dir: None,
            flatten_dirs,
            selected_dir_history: Vec::new(),
        }
    }

    pub fn clear(&mut self) {
        self.table_state = TableState::new();
        self.table_len = 0;
        self.total_size = 0;
        self.current_entries = Vec::new();
        self.dir_index = BTreeMap::new();
        self.selected_path = None;
        self.current_dir = None;
        self.selected_path_is_dir = false;
        self.scroll_state = ScrollbarState::new(0);
        self.selected_dir_history = Vec::new();
    }

    pub fn current_paths(&self) -> Vec<Arc<PathBuf>> {
        self.current_entries
            .iter()
            .map(|e| e.path.clone())
            .collect()
    }

    fn update_dirview(&mut self, files: &Vec<DirTableEntry>, sort_by: Option<&Sorting>) {
        // Temporary accumulator per directory during build.
        #[derive(Debug, Default, Clone)]
        struct Acc {
            subdirs: BTreeSet<PathBuf>, // deterministic, deduped
            files: Vec<DirTableEntry>,  // direct files
            direct_count: usize,
            direct_size: u64,
            total_count: usize, // recursive
            total_size: u64,    // recursive
            date: Option<SystemTime>,
        }

        let mut acc_map: BTreeMap<PathBuf, Acc> = BTreeMap::new();

        for file in files {
            let path = PathBuf::from(file.display_path.clone());
            if let Some(parent) = path.parent() {
                let parent = parent.to_path_buf();
                let size = file.size;
                let date = file.date;

                // Insert file into its parent dir
                let par_acc = acc_map.entry(parent.clone()).or_default();
                par_acc.files.push(file.to_owned());
                par_acc.direct_count += 1;
                par_acc.direct_size += size;

                if let Some(file_date) = date {
                    if let Some(par_date) = par_acc.date {
                        if par_date < file_date {
                            par_acc.date = Some(file_date);
                        }
                    } else {
                        par_acc.date = Some(file_date);
                    }
                }

                // Walk up the ancestor chain to register subdir relationships
                let mut ancestors = parent.ancestors().collect::<Vec<_>>();
                ancestors.reverse(); // from root to leaf

                // dbg!(&path);
                // dbg!(&ancestors);

                for win in ancestors.windows(2) {
                    if let [a, b] = win {
                        let parent = a.to_path_buf();
                        let subdir = b.to_path_buf();

                        let par_acc = acc_map.entry(parent).or_default();
                        par_acc.subdirs.insert(subdir);
                        par_acc.total_count += 1;
                        par_acc.total_size += size;

                        if let Some(file_date) = date {
                            if let Some(par_date) = par_acc.date {
                                if par_date < file_date {
                                    par_acc.date = Some(file_date);
                                }
                            } else {
                                par_acc.date = Some(file_date);
                            }
                        }
                    }
                }
            }
        }

        debug!("acc_map: {:#?}", acc_map);

        // Convert to DirView structs
        self.dir_index = acc_map
            .clone() // TODO: fix this abomination
            .into_iter()
            .map(|(path, acc)| {
                let path_arc = Arc::new(path.clone());

                let mut entries: Vec<DirTableEntry> = acc
                    .subdirs
                    .iter()
                    .map(|d| {
                        let c = acc_map.get(d).unwrap();

                        let size = c.total_size + c.direct_size;
                        let clone_count = c.total_count + c.direct_count;
                        let date = c.date;

                        DirTableEntry {
                            path: Arc::new(d.to_owned()),
                            display_path: d
                                .file_name()
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_default(),
                            clone_count,
                            size,
                            date,
                            is_dir: true,
                        }
                    })
                    .collect();

                let size: u64 = acc.files.iter().map(|e| e.size).sum();
                assert_eq!(size, acc.direct_size);
                assert_eq!(acc.files.len(), acc.direct_count);

                entries.extend(acc.files);

                // Sort the entries
                if let Some(sort_by) = sort_by {
                    entries.sort_by(|a, b| match sort_by {
                        Sorting::Path => a.path.cmp(&b.path),
                        Sorting::Size => b.size.cmp(&a.size),
                        Sorting::Date => b.date.cmp(&a.date),
                        Sorting::Count => b.clone_count.cmp(&a.clone_count),
                    });
                }

                let view = DirView {
                    path: path_arc.clone(),
                    entries,
                    file_count: acc.direct_count + acc.total_count,
                    total_size: acc.direct_size + acc.total_size,
                };
                (path, view)
            })
            .collect();

        debug!("dir_index= {:#?}", self.dir_index);
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
                let display_path = format_path(path, &fi.dirs);
                let clone_count = fi.file_duplicates_len(path).unwrap_or_default();
                total_size_acc += size;

                entries_vec.push(DirTableEntry {
                    path: path.clone(),
                    display_path,
                    size,
                    date,
                    clone_count,
                    is_dir: false,
                });
            }

            (entries_vec, total_size_acc)
        };

        if self.flatten_dirs {
            // Sort the paths
            if let Some(sort_by) = sort_by {
                entries.sort_by(|a, b| match sort_by {
                    Sorting::Path => a.path.cmp(&b.path),
                    Sorting::Size => b.size.cmp(&a.size),
                    Sorting::Date => b.date.cmp(&a.date),
                    Sorting::Count => b.clone_count.cmp(&a.clone_count),
                });
            }

            self.current_dir = None;
            self.current_entries = entries;
            self.total_size = total_size;
        } else {
            self.update_dirview(&entries, sort_by);
            let current = self
                .dir_index
                .first_key_value()
                .map(|(k, v)| (k.to_owned(), v.entries.clone(), v.total_size));
            if let Some((path, dir, dir_size)) = current {
                self.current_dir = Some(path);
                self.current_entries = dir;
                self.total_size = dir_size;
            }
        }

        self.table_len = self.current_entries.len();
        self.scroll_state = ScrollbarState::new(self.table_len.saturating_sub(1));
        self.update_footer();
    }

    fn update_footer(&mut self) {
        // from draw
        let footer_style = Style::default().dark_gray();
        let total_size_str = humansize::format_size(self.total_size, humansize::DECIMAL);

        self.footer = Row::new(vec![
            Cell::from(Text::from("")),
            Cell::from(Text::from(format!("Files: {}", self.table_len))),
            Cell::from(Text::from("Total:")),
            Cell::from(Text::from(total_size_str)),
        ])
        .style(footer_style);
    }

    fn set_current_dir(&mut self, path: PathBuf) {
        if let Some(dir) = self.dir_index.get(&path) {
            self.current_dir = Some(path);
            self.current_entries = dir.entries.clone();
            self.total_size = dir.total_size;

            self.table_len = self.current_entries.len();
            self.scroll_state = ScrollbarState::new(self.table_len.saturating_sub(1));

            self.update_footer();
        }
    }

    pub fn enter_selected_dir(&mut self) {
        if let Some(selected) = self.selected_dir_path() {
            if let Some(index) = self.table_state.selected() {
                self.selected_dir_history.push(index);
            };
            self.set_current_dir(selected.as_path().to_owned());
            self.select_first();
        }
    }

    pub fn back_parent_dir(&mut self) {
        if let Some(selected) = self.current_dir.clone()
            && let Some(dir) = self.dir_index.get(&selected)
            && let Some(parent) = dir.parent()
        {
            self.set_current_dir(parent.to_owned());
            if let Some(index) = self.selected_dir_history.pop() {
                self.select_entry(index);
            }
        }
    }

    pub fn select_entry(&mut self, index: usize) {
        if self.table_len == 0 {
            self.select_none();
            return;
        }
        self.table_state.select(Some(index));
        if let Some((selected_path, is_dir)) = self
            .current_entries
            .get(index)
            .map(|e| (e.path.to_owned(), e.is_dir))
        {
            self.selected_path = Some(selected_path);
            self.selected_path_is_dir = is_dir;
            self.scroll_state = self.scroll_state.position(index);
        }
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
        self.selected_path_is_dir = false;
    }

    pub fn selected_path(&self) -> Option<Arc<PathBuf>> {
        self.selected_path.clone()
    }

    pub fn selected_path_is_dir(&self) -> bool {
        self.selected_path_is_dir
    }

    pub fn selected_file_path(&self) -> Option<Arc<PathBuf>> {
        if !self.selected_path_is_dir {
            self.selected_path.clone()
        } else {
            None
        }
    }

    pub fn selected_dir_path(&self) -> Option<Arc<PathBuf>> {
        if self.selected_path_is_dir {
            self.selected_path.clone()
        } else {
            None
        }
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

        let rows = self.current_entries.iter().enumerate().map(|(i, e)| {
            if i >= offset.saturating_sub(height)
                && i < offset.saturating_add(height.saturating_mul(2))
            {
                let is_marked = marked_files.contains(&e.path);
                e.to_row(
                    self.mark_marked,
                    is_marked,
                    self.show_clone_count,
                    !self.flatten_dirs,
                )
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
        }
        .title_bottom(
            self.current_dir
                .as_ref()
                .map(|d| d.display().to_string())
                .unwrap_or_default(),
        )
        .title_bottom(format!("{:?}", self.selected_dir_history));

        let selected_style = if focused {
            Style::default().fg(Color::Black).bg(Color::White)
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
