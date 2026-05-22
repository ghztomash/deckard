use crate::{app::Sorting, table::format_path_with_common};
use chrono::{DateTime, Local};
use deckard::index::FileIndex;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Margin, Rect},
    style::{Color, Style},
    text::Span,
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
    display_text: String,
    name_text: String,
    size: u64,
    size_text: String,
    date: Option<SystemTime>,
    date_text: String,
    clone_count: usize,
    clone_count_text: String,
    is_dir: bool,
}

impl DirTableEntry {
    fn new(
        path: Arc<PathBuf>,
        display_path: String,
        size: u64,
        date: Option<SystemTime>,
        clone_count: usize,
        is_dir: bool,
    ) -> Self {
        let suffix = if is_dir { "/" } else { "" };
        let display_text = format!("{display_path}{suffix}");
        let name_text = format!(
            "{}{suffix}",
            path.file_name().unwrap_or_default().to_string_lossy()
        );
        let size_text = humansize::format_size(size, humansize::DECIMAL);
        let date_text = date
            .map(|d| DateTime::<Local>::from(d).format("%d/%m/%Y").to_string())
            .unwrap_or_default();
        let clone_count_text = clone_count.to_string();

        Self {
            path,
            display_path,
            display_text,
            name_text,
            size,
            size_text,
            date,
            date_text,
            clone_count,
            clone_count_text,
            is_dir,
        }
    }

    fn to_row(
        &self,
        mark_marked: bool,
        is_marked: bool,
        show_clone_count: bool,
        show_name: bool,
    ) -> Row<'_> {
        let path_style = if self.is_dir {
            Style::new().light_blue()
        } else if mark_marked && is_marked {
            Style::new().yellow()
        } else {
            Style::new()
        };

        let path_text = if show_name {
            self.name_text.as_str()
        } else {
            self.display_text.as_str()
        };

        let mut cells = vec![
            Cell::from(if mark_marked && is_marked {
                if self.is_dir { "-" } else { "*" }
            } else {
                " "
            }),
            Cell::from(Span::styled(path_text, path_style)),
            Cell::from(self.date_text.as_str()),
            Cell::from(self.size_text.as_str()),
        ];

        if show_clone_count {
            cells.push(Cell::from(self.clone_count_text.as_str()));
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
    /// Count of subdirectories including nested subdirectories
    pub subdirectory_count: usize,
    /// Sum of sizes of files including subdirectories
    pub total_size: u64,
    /// Latest modified time among files including subdirectories
    pub modified: Option<SystemTime>,
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

/// Summary details for the selected directory row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DirectoryInfo {
    /// Full or display-resolved directory path.
    pub(crate) path: PathBuf,
    /// Directory name.
    pub(crate) name: String,
    /// Recursive count of files represented by the current dirview.
    pub(crate) file_count: usize,
    /// Recursive count of subdirectories represented by the current dirview.
    pub(crate) subdirectory_count: usize,
    /// Recursive size of files represented by the current dirview.
    pub(crate) total_size: u64,
    /// Latest modified time among represented child files.
    pub(crate) modified: Option<SystemTime>,
}

#[derive(Debug, Default)]
pub struct DirTable {
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
    common_path: Option<PathBuf>,
    total_size: u64,
    selected_dir_history: Vec<usize>,
    header_labels: Vec<&'static str>,
    widths: Vec<Constraint>,
    file_count_text: String,
    total_size_text: String,
}

impl DirTable {
    pub fn new(
        header_str: Vec<&'static str>,
        mark_marked: bool,
        show_clone_count: bool,
        flatten_dirs: bool,
    ) -> Self {
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
            current_entries: Vec::new(),
            selected_path: None,
            selected_path_is_dir: false,
            scroll_state: ScrollbarState::new(0),
            mark_marked,
            show_clone_count,
            dir_index: BTreeMap::new(),
            current_dir: None,
            flatten_dirs,
            common_path: None,
            selected_dir_history: Vec::new(),
            header_labels: header_str,
            widths,
            file_count_text: String::new(),
            total_size_text: String::new(),
        }
    }

    pub fn flatten_dirs(&mut self, enabled: bool) {
        self.flatten_dirs = enabled;
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
        self.common_path = None;
        self.scroll_state = ScrollbarState::new(0);
        self.selected_dir_history = Vec::new();
        self.file_count_text = String::new();
        self.total_size_text = String::new();
    }

    /// Returns paths for currently visible file rows only.
    pub fn current_file_paths(&self) -> Vec<Arc<PathBuf>> {
        self.current_entries
            .iter()
            .filter(|e| !e.is_dir)
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

        fn count_subdirectories(
            path: &Path,
            acc_map: &BTreeMap<PathBuf, Acc>,
            cache: &mut BTreeMap<PathBuf, usize>,
        ) -> usize {
            if let Some(count) = cache.get(path) {
                return *count;
            }

            let count = acc_map
                .get(path)
                .map(|acc| {
                    acc.subdirs
                        .iter()
                        .map(|subdir| 1 + count_subdirectories(subdir, acc_map, cache))
                        .sum()
                })
                .unwrap_or_default();
            cache.insert(path.to_path_buf(), count);
            count
        }

        debug!("acc_map: {:#?}", acc_map);

        let mut subdirectory_counts = BTreeMap::new();

        // Convert to DirView structs
        self.dir_index = acc_map
            .clone() // TODO: fix this abomination
            .into_iter()
            .map(|(path, acc)| {
                let path_arc = Arc::new(path.clone());
                let subdirectory_count =
                    count_subdirectories(&path, &acc_map, &mut subdirectory_counts);

                let mut entries: Vec<DirTableEntry> = acc
                    .subdirs
                    .iter()
                    .map(|d| {
                        let c = acc_map.get(d).unwrap();

                        let size = c.total_size + c.direct_size;
                        let clone_count = c.total_count + c.direct_count;
                        let date = c.date;

                        DirTableEntry::new(
                            Arc::new(d.to_owned()),
                            d.file_name()
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_default(),
                            size,
                            date,
                            clone_count,
                            true,
                        )
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
                    subdirectory_count,
                    total_size: acc.direct_size + acc.total_size,
                    modified: acc.date,
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
        let (mut entries, total_size, common_path) = {
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
                let clone_count = fi.file_duplicates_len(path).unwrap_or_default();
                total_size_acc += size;

                entries_vec.push(DirTableEntry::new(
                    path.clone(),
                    display_path,
                    size,
                    date,
                    clone_count,
                    false,
                ));
            }

            (entries_vec, total_size_acc, common_path)
        };
        self.common_path = common_path;

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
        self.scroll_state = ScrollbarState::new(self.table_len);
        self.update_footer();
    }

    fn update_footer(&mut self) {
        self.file_count_text = format!("Files: {}", self.table_len);
        self.total_size_text = humansize::format_size(self.total_size, humansize::DECIMAL);
    }

    fn set_current_dir(&mut self, path: PathBuf) {
        if let Some(dir) = self.dir_index.get(&path) {
            self.current_dir = Some(path);
            self.current_entries = dir.entries.clone();
            self.total_size = dir.total_size;

            self.table_len = self.current_entries.len();
            self.scroll_state = ScrollbarState::new(self.table_len);

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

    /// Moves to the parent directory if the current view has one.
    pub fn back_parent_dir(&mut self) -> bool {
        let parent = self
            .current_dir
            .as_ref()
            .and_then(|selected| self.dir_index.get(selected))
            .and_then(DirView::parent)
            .map(Path::to_path_buf);

        if let Some(parent) = parent
            && self.dir_index.contains_key(&parent)
        {
            self.set_current_dir(parent);
            if let Some(index) = self.selected_dir_history.pop() {
                self.select_entry(index);
            }
            true
        } else {
            false
        }
    }

    pub fn select_entry(&mut self, index: usize) {
        if self.table_len == 0 {
            self.select_none();
            return;
        }
        let index = index.min(self.table_len.saturating_sub(1));
        self.table_state.select(Some(index));
        if let Some((selected_path, is_dir)) = self
            .current_entries
            .get(index)
            .map(|e| (e.path.to_owned(), e.is_dir))
        {
            self.selected_path = Some(selected_path);
            self.selected_path_is_dir = is_dir;
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

    /// Returns file paths below the selected directory row.
    pub fn selected_dir_file_paths(&self) -> Vec<Arc<PathBuf>> {
        let Some(selected_dir) = self.selected_dir_path() else {
            return Vec::new();
        };

        self.dir_index
            .iter()
            .filter(|(dir_path, _)| dir_path.starts_with(selected_dir.as_path()))
            .flat_map(|(_, view)| {
                view.entries
                    .iter()
                    .filter(|entry| !entry.is_dir)
                    .map(|entry| entry.path.clone())
            })
            .collect()
    }

    /// Returns summary details for the selected directory row.
    pub(crate) fn selected_dir_info(&self) -> Option<DirectoryInfo> {
        let selected_dir = self.selected_dir_path()?;
        let view = self.dir_index.get(selected_dir.as_path())?;
        let path = self.resolved_dir_path(selected_dir.as_path());
        let name = selected_dir
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| selected_dir.to_string_lossy().to_string());

        Some(DirectoryInfo {
            path,
            name,
            file_count: view.file_count,
            subdirectory_count: view.subdirectory_count,
            total_size: view.total_size,
            modified: view.modified,
        })
    }

    fn resolved_dir_path(&self, selected_dir: &Path) -> PathBuf {
        self.common_path
            .as_ref()
            .map(|common_path| common_path.join(selected_dir))
            .unwrap_or_else(|| selected_dir.to_path_buf())
    }

    pub fn render(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        focused: bool,
        marked_files: &HashSet<Arc<PathBuf>>,
    ) {
        let visible = self.visible_range(area);
        let rows = self.current_entries[visible.start..visible.end]
            .iter()
            .map(|e| {
                e.to_row(
                    self.mark_marked,
                    self.entry_is_marked(e, marked_files),
                    self.show_clone_count,
                    !self.flatten_dirs,
                )
            });

        let block = self.block(focused);

        let selected_style = if focused {
            Style::default().fg(Color::Black).bg(Color::White)
        } else {
            Style::default().fg(Color::Black).bg(Color::DarkGray)
        };

        let table = Table::default()
            .widths(self.widths.iter().copied())
            .header(self.header())
            .rows(rows)
            .footer(self.footer())
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

    fn entry_is_marked(&self, entry: &DirTableEntry, marked_files: &HashSet<Arc<PathBuf>>) -> bool {
        if entry.is_dir {
            marked_files.iter().any(|marked_file| {
                let marked_file = self
                    .common_path
                    .as_ref()
                    .and_then(|common_path| marked_file.strip_prefix(common_path).ok())
                    .unwrap_or(marked_file.as_path());

                marked_file.starts_with(entry.path.as_path())
            })
        } else {
            marked_files.contains(&entry.path)
        }
    }

    fn block(&self, focused: bool) -> Block<'_> {
        if focused {
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
        .title_bottom(format!(
            "{:?} {:?}",
            self.selected_dir_history, self.selected_path
        ))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(index: usize) -> DirTableEntry {
        DirTableEntry::new(
            Arc::new(PathBuf::from(format!("/tmp/file-{index}"))),
            format!("file-{index}"),
            index as u64,
            None,
            index,
            false,
        )
    }

    fn file_entry(path: Arc<PathBuf>, display_path: &str) -> DirTableEntry {
        DirTableEntry::new(path, display_path.to_string(), 0, None, 0, false)
    }

    fn file_entry_with_metadata(
        path: &str,
        display_path: &str,
        size: u64,
        date: Option<SystemTime>,
    ) -> DirTableEntry {
        DirTableEntry::new(
            Arc::new(PathBuf::from(path)),
            display_path.to_string(),
            size,
            date,
            0,
            false,
        )
    }

    fn dir_entry(path: &str) -> DirTableEntry {
        DirTableEntry::new(
            Arc::new(PathBuf::from(path)),
            path.to_string(),
            0,
            None,
            0,
            true,
        )
    }

    fn dir_entry_with_date(path: &str, date: Option<SystemTime>) -> DirTableEntry {
        DirTableEntry::new(
            Arc::new(PathBuf::from(path)),
            path.to_string(),
            0,
            date,
            0,
            true,
        )
    }

    fn marked_files(paths: &[&str]) -> HashSet<Arc<PathBuf>> {
        paths
            .iter()
            .map(|path| Arc::new(PathBuf::from(path)))
            .collect()
    }

    fn table_with_common_path(common_path: &str) -> DirTable {
        let mut table = DirTable::new(vec![" ", "File", "Date", "Size"], true, false, false);
        table.common_path = Some(PathBuf::from(common_path));
        table
    }

    fn dir_view(path: &str, entries: Vec<DirTableEntry>) -> DirView {
        dir_view_with_stats(path, entries, 0, 0)
    }

    fn dir_view_with_stats(
        path: &str,
        entries: Vec<DirTableEntry>,
        file_count: usize,
        total_size: u64,
    ) -> DirView {
        dir_view_with_cached_info(path, entries, file_count, 0, total_size, None)
    }

    fn dir_view_with_cached_info(
        path: &str,
        entries: Vec<DirTableEntry>,
        file_count: usize,
        subdirectory_count: usize,
        total_size: u64,
        modified: Option<SystemTime>,
    ) -> DirView {
        DirView {
            path: Arc::new(PathBuf::from(path)),
            entries,
            file_count,
            subdirectory_count,
            total_size,
            modified,
        }
    }

    #[test]
    fn back_parent_dir_moves_to_parent_when_nested() {
        let mut table = DirTable::new(vec![" ", "File", "Date", "Size"], true, false, false);
        table
            .dir_index
            .insert(PathBuf::new(), dir_view("", vec![dir_entry("deckard-tui")]));
        table.dir_index.insert(
            PathBuf::from("deckard-tui"),
            dir_view("deckard-tui", vec![entry(1)]),
        );
        table.current_dir = Some(PathBuf::from("deckard-tui"));
        table.selected_dir_history.push(0);

        assert!(table.back_parent_dir());
        assert_eq!(table.current_dir, Some(PathBuf::new()));
        assert_eq!(table.table_state.selected(), Some(0));
        assert!(table.selected_dir_history.is_empty());
    }

    #[test]
    fn back_parent_dir_returns_false_at_top_level() {
        let mut table = DirTable::new(vec![" ", "File", "Date", "Size"], true, false, false);
        table
            .dir_index
            .insert(PathBuf::new(), dir_view("", vec![dir_entry("deckard-tui")]));
        table.current_dir = Some(PathBuf::new());

        assert!(!table.back_parent_dir());
        assert_eq!(table.current_dir, Some(PathBuf::new()));
    }

    #[test]
    fn marks_only_real_ancestor_directories() {
        let table = table_with_common_path("/home/user/deckard");
        let marked = marked_files(&["/home/user/deckard/deckard-tui/Cargo.toml"]);

        assert!(table.entry_is_marked(&dir_entry("deckard-tui"), &marked));
        assert!(!table.entry_is_marked(&dir_entry("deckard"), &marked));
    }

    #[test]
    fn root_files_do_not_mark_substring_named_dirs() {
        let table = table_with_common_path("/home/user/deckard");
        let marked = marked_files(&["/home/user/deckard/.gitignore"]);

        assert!(!table.entry_is_marked(&dir_entry("deckard"), &marked));
        assert!(!table.entry_is_marked(&dir_entry(".git"), &marked));
    }

    #[test]
    fn nested_files_mark_component_ancestors() {
        let table = table_with_common_path("/home/user/deckard");
        let marked = marked_files(&["/home/user/deckard/deckard/src/lib.rs"]);

        assert!(table.entry_is_marked(&dir_entry("deckard"), &marked));
        assert!(table.entry_is_marked(&dir_entry("deckard/src"), &marked));
    }

    #[test]
    fn current_file_paths_excludes_directory_rows() {
        let mut table = DirTable::new(vec![" ", "File", "Date", "Size"], true, false, false);
        let file_path = Arc::new(PathBuf::from("/tmp/file"));
        table.current_entries = vec![
            dir_entry("deckard"),
            DirTableEntry::new(file_path.clone(), "file".to_string(), 0, None, 0, false),
        ];

        assert_eq!(table.current_file_paths(), vec![file_path]);
    }

    #[test]
    fn selected_dir_file_paths_includes_direct_and_nested_files() {
        let mut table = DirTable::new(vec![" ", "File", "Date", "Size"], true, false, false);
        let direct = Arc::new(PathBuf::from("/tmp/root/folder/direct.txt"));
        let nested = Arc::new(PathBuf::from("/tmp/root/folder/sub/nested.txt"));

        table.current_entries = vec![dir_entry("folder")];
        table.table_len = table.current_entries.len();
        table.dir_index.insert(
            PathBuf::from("folder"),
            dir_view(
                "folder",
                vec![
                    file_entry(direct.clone(), "folder/direct.txt"),
                    dir_entry("folder/sub"),
                ],
            ),
        );
        table.dir_index.insert(
            PathBuf::from("folder/sub"),
            dir_view(
                "folder/sub",
                vec![file_entry(nested.clone(), "folder/sub/nested.txt")],
            ),
        );
        table.select_first();

        assert_eq!(table.selected_dir_file_paths(), vec![direct, nested]);
    }

    #[test]
    fn selected_dir_file_paths_excludes_similarly_named_siblings() {
        let mut table = DirTable::new(vec![" ", "File", "Date", "Size"], true, false, false);
        let selected_file = Arc::new(PathBuf::from("/tmp/root/folder/file.txt"));
        let sibling_file = Arc::new(PathBuf::from("/tmp/root/folder-sibling/file.txt"));

        table.current_entries = vec![dir_entry("folder")];
        table.table_len = table.current_entries.len();
        table.dir_index.insert(
            PathBuf::from("folder"),
            dir_view(
                "folder",
                vec![file_entry(selected_file.clone(), "folder/file.txt")],
            ),
        );
        table.dir_index.insert(
            PathBuf::from("folder-sibling"),
            dir_view(
                "folder-sibling",
                vec![file_entry(sibling_file, "folder-sibling/file.txt")],
            ),
        );
        table.select_first();

        assert_eq!(table.selected_dir_file_paths(), vec![selected_file]);
    }

    #[test]
    fn selected_dir_file_paths_is_empty_without_selected_dir() {
        let mut table = DirTable::new(vec![" ", "File", "Date", "Size"], true, false, false);
        let file_path = Arc::new(PathBuf::from("/tmp/root/file.txt"));
        table.current_entries = vec![file_entry(file_path, "file.txt")];
        table.table_len = table.current_entries.len();
        table.select_first();

        assert!(table.selected_dir_file_paths().is_empty());
    }

    #[test]
    fn selected_dir_info_returns_stats_for_selected_directory() {
        let older = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(10);
        let newer = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(20);
        let mut table = table_with_common_path("/tmp/root");

        table.current_entries = vec![dir_entry("folder")];
        table.table_len = table.current_entries.len();
        table.dir_index.insert(
            PathBuf::from("folder"),
            dir_view_with_cached_info(
                "folder",
                vec![
                    file_entry_with_metadata(
                        "/tmp/root/folder/direct.txt",
                        "folder/direct.txt",
                        15,
                        Some(older),
                    ),
                    dir_entry_with_date("folder/sub", Some(newer)),
                ],
                2,
                1,
                35,
                Some(newer),
            ),
        );
        table.dir_index.insert(
            PathBuf::from("folder/sub"),
            dir_view_with_cached_info(
                "folder/sub",
                vec![file_entry_with_metadata(
                    "/tmp/root/folder/sub/nested.txt",
                    "folder/sub/nested.txt",
                    20,
                    Some(newer),
                )],
                1,
                0,
                20,
                Some(newer),
            ),
        );
        table.select_first();

        assert_eq!(
            table.selected_dir_info(),
            Some(DirectoryInfo {
                path: PathBuf::from("/tmp/root/folder"),
                name: "folder".to_string(),
                file_count: 2,
                subdirectory_count: 1,
                total_size: 35,
                modified: Some(newer),
            })
        );
    }

    #[test]
    fn selected_dir_info_uses_cached_subdirectory_count() {
        let mut table = DirTable::new(vec![" ", "File", "Date", "Size"], true, false, false);

        table.current_entries = vec![dir_entry("folder")];
        table.table_len = table.current_entries.len();
        table.dir_index.insert(
            PathBuf::from("folder"),
            dir_view_with_cached_info("folder", vec![], 0, 7, 0, None),
        );
        table.select_first();

        let info = table.selected_dir_info().unwrap();

        assert_eq!(info.subdirectory_count, 7);
    }

    #[test]
    fn update_table_caches_subdirectory_count_component_safely() {
        let mut table = DirTable::new(vec![" ", "File", "Date", "Size"], true, false, false);
        let file_index = Arc::new(RwLock::new(FileIndex::new(
            HashSet::from([PathBuf::from("/tmp/root")]),
            deckard::config::SearchConfig::default(),
        )));
        let paths = vec![
            Arc::new(PathBuf::from("/tmp/root/folder/direct.txt")),
            Arc::new(PathBuf::from("/tmp/root/folder/sub/nested.txt")),
            Arc::new(PathBuf::from("/tmp/root/folder-sibling/file.txt")),
        ];

        table.update_table(&paths, &file_index, Some(&Sorting::Path));
        table.select_first();

        assert_eq!(
            table
                .selected_dir_path()
                .as_ref()
                .map(|path| path.as_path()),
            Some(Path::new("folder"))
        );
        assert_eq!(
            table
                .selected_dir_info()
                .map(|info| (info.file_count, info.subdirectory_count)),
            Some((2, 1))
        );
    }

    #[test]
    fn selected_dir_info_is_none_without_selected_directory() {
        let mut table = DirTable::new(vec![" ", "File", "Date", "Size"], true, false, false);
        let file_path = Arc::new(PathBuf::from("/tmp/root/file.txt"));
        table.current_entries = vec![file_entry(file_path, "file.txt")];
        table.table_len = table.current_entries.len();

        assert!(table.selected_dir_info().is_none());

        table.select_first();

        assert!(table.selected_dir_info().is_none());
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
        let mut table = DirTable::new(vec![" ", "File", "Date", "Size"], true, false, false);
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
        let mut table = DirTable::new(vec![" ", "File", "Date", "Size"], true, false, false);
        table.table_len = 20;
        table.table_state.select(Some(19));

        let visible = table.visible_range(Rect::new(0, 0, 80, 8));

        assert_eq!(visible.start, 16);
        assert_eq!(visible.selected, Some(3));
        assert_eq!(visible.scroll_position, 19);
    }

    #[test]
    fn render_large_table_uses_visible_slice() {
        let mut table = DirTable::new(vec![" ", "File", "Date", "Size"], true, false, false);
        table.current_entries = (0..10_000).map(entry).collect();
        table.table_len = table.current_entries.len();
        table.file_count_text = format!("Files: {}", table.table_len);
        table.total_size_text = humansize::format_size(0u64, humansize::DECIMAL);
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
