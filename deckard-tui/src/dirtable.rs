use crate::app::Sorting;
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
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::Instant,
    time::SystemTime,
};
use tracing::debug;

#[derive(Debug, Clone)]
struct FileRecord {
    path: Arc<PathBuf>,
    size: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
    clone_count: usize,
}

#[derive(Debug, Default, Clone)]
pub struct DirTableEntry {
    path: Arc<PathBuf>,
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
        relative_path: PathBuf,
        size: u64,
        date: Option<SystemTime>,
        clone_count: usize,
        is_dir: bool,
    ) -> Self {
        let display_path = relative_path.to_string_lossy().to_string();
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

    fn from_file_record(record: &FileRecord, relative_path: PathBuf) -> Self {
        Self::new(
            record.path.clone(),
            relative_path,
            record.size,
            record.modified,
            record.clone_count,
            false,
        )
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

#[derive(Clone, Debug, Default)]
struct DirectoryNode {
    parent: Option<PathBuf>,
    child_dirs: HashSet<PathBuf>,
    direct_files: Vec<FileRecord>,
    direct_size: u64,
    file_count: usize,
    subdirectory_count: usize,
    total_size: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
}

/// Summary details for the selected directory row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DirectoryInfo {
    /// Full or display-resolved directory path.
    pub(crate) path: PathBuf,
    /// Directory name.
    pub(crate) name: String,
    /// Recursive count of files represented by the current directory.
    pub(crate) file_count: usize,
    /// Recursive count of subdirectories represented by the current directory.
    pub(crate) subdirectory_count: usize,
    /// Recursive size of files represented by the current directory.
    pub(crate) total_size: u64,
    /// Latest modified time among represented child files.
    pub(crate) modified: Option<SystemTime>,
    pub(crate) created: Option<SystemTime>,
}

#[derive(Debug, Default)]
pub struct DirTable {
    pub table_state: TableState,
    pub table_len: usize,
    dir_nodes: HashMap<PathBuf, DirectoryNode>,
    current_dir: Option<PathBuf>,
    current_sort: Option<Sorting>,
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
            dir_nodes: HashMap::new(),
            current_dir: None,
            current_sort: None,
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
        self.dir_nodes = HashMap::new();
        self.selected_path = None;
        self.current_dir = None;
        self.current_sort = None;
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

    pub fn update_table(
        &mut self,
        paths: &Vec<Arc<PathBuf>>,
        file_index: &Arc<RwLock<FileIndex>>,
        sort_by: Option<&Sorting>,
    ) {
        let update_start = Instant::now();
        self.current_sort = sort_by.copied();

        let fi = file_index.read().unwrap();
        let common_path = deckard::find_common_path(&fi.dirs);
        self.common_path = common_path;

        if self.flatten_dirs {
            let mut entries = Vec::with_capacity(paths.len());
            let mut total_size = 0u64;

            for path in paths {
                let record = file_record_from_index(path, &fi);
                total_size += record.size;
                let relative_path =
                    relative_path_for_common(path.as_path(), self.common_path.as_deref());
                entries.push(DirTableEntry::from_file_record(&record, relative_path));
            }
            sort_entries(&mut entries, self.current_sort);

            self.current_dir = None;
            self.current_entries = entries;
            self.total_size = total_size;
            self.dir_nodes.clear();
        } else {
            let build_start = Instant::now();
            self.dir_nodes = build_directory_nodes(paths, &fi, self.common_path.as_deref());
            debug!(
                "built {} compact directory nodes from {} files in {:?}",
                self.dir_nodes.len(),
                paths.len(),
                build_start.elapsed()
            );

            if let Some(root_dir) = root_dir_path(&self.dir_nodes) {
                self.set_current_dir(root_dir);
            } else {
                self.current_dir = None;
                self.current_entries.clear();
                self.total_size = 0;
            }
        }
        drop(fi);

        self.table_len = self.current_entries.len();
        self.scroll_state = ScrollbarState::new(self.table_len);
        self.update_footer();

        debug!(
            "update_table built {} entries (flattened={}) from {} paths in {:?}",
            self.table_len,
            self.flatten_dirs,
            paths.len(),
            update_start.elapsed()
        );
    }

    fn update_footer(&mut self) {
        self.file_count_text = format!("Files: {}", self.table_len);
        self.total_size_text = humansize::format_size(self.total_size, humansize::DECIMAL);
    }

    fn materialize_dir_entries(&self, path: &Path) -> Vec<DirTableEntry> {
        let Some(node) = self.dir_nodes.get(path) else {
            return Vec::new();
        };

        let mut child_dirs = node.child_dirs.iter().collect::<Vec<_>>();
        child_dirs.sort();

        let mut entries = Vec::with_capacity(child_dirs.len() + node.direct_files.len());
        entries.extend(child_dirs.into_iter().filter_map(|child_path| {
            self.dir_nodes.get(child_path).map(|child| {
                DirTableEntry::new(
                    Arc::new(child_path.clone()),
                    child_path.clone(),
                    child.total_size,
                    child.modified,
                    child.file_count,
                    true,
                )
            })
        }));
        entries.extend(node.direct_files.iter().map(|record| {
            let relative_path =
                relative_path_for_common(record.path.as_path(), self.common_path.as_deref());
            DirTableEntry::from_file_record(record, relative_path)
        }));

        sort_entries(&mut entries, self.current_sort);
        entries
    }

    fn set_current_dir(&mut self, path: PathBuf) {
        if let Some(dir) = self.dir_nodes.get(&path) {
            let total_size = dir.total_size;
            let entries = self.materialize_dir_entries(&path);

            self.current_dir = Some(path);
            self.current_entries = entries;
            self.total_size = total_size;

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
            .and_then(|selected| self.dir_nodes.get(selected))
            .and_then(|node| node.parent.clone());

        if let Some(parent) = parent
            && self.dir_nodes.contains_key(&parent)
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

    /// Returns the selected file path or the resolved filesystem path for a selected directory.
    pub fn selected_resolved_path(&self) -> Option<Arc<PathBuf>> {
        let selected_path = self.selected_path.as_ref()?;
        if self.selected_path_is_dir {
            Some(Arc::new(self.resolved_dir_path(selected_path.as_path())))
        } else {
            Some(selected_path.clone())
        }
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

        let mut files = Vec::new();
        let mut stack = vec![selected_dir.as_ref().clone()];

        while let Some(dir_path) = stack.pop() {
            let Some(node) = self.dir_nodes.get(&dir_path) else {
                continue;
            };

            files.extend(node.direct_files.iter().map(|record| record.path.clone()));

            let mut child_dirs = node.child_dirs.iter().collect::<Vec<_>>();
            child_dirs.sort();
            stack.extend(child_dirs.into_iter().rev().cloned());
        }

        files
    }

    /// Returns summary details for the selected directory row.
    pub(crate) fn selected_dir_info(&self) -> Option<DirectoryInfo> {
        let selected_dir = self.selected_dir_path()?;
        let node = self.dir_nodes.get(selected_dir.as_path())?;
        let path = self.resolved_dir_path(selected_dir.as_path());
        let name = selected_dir
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| selected_dir.to_string_lossy().to_string());

        Some(DirectoryInfo {
            path,
            name,
            file_count: node.file_count,
            subdirectory_count: node.subdirectory_count,
            total_size: node.total_size,
            modified: node.modified,
            created: node.created,
        })
    }

    pub fn marked_directory_paths(&self, marked_files: &HashSet<Arc<PathBuf>>) -> HashSet<PathBuf> {
        marked_files
            .iter()
            .filter_map(|marked_file| {
                self.common_path
                    .as_ref()
                    .and_then(|common_path| marked_file.strip_prefix(common_path).ok())
                    .or(Some(marked_file.as_path()))
                    .map(Path::to_path_buf)
            })
            .flat_map(|relative_path| {
                relative_path
                    .ancestors()
                    .skip(1)
                    .filter(|ancestor| !ancestor.as_os_str().is_empty())
                    .map(Path::to_path_buf)
                    .collect::<Vec<_>>()
            })
            .collect()
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
        marked_dirs: &HashSet<PathBuf>,
    ) {
        let visible = self.visible_range(area);
        let rows = self.current_entries[visible.start..visible.end]
            .iter()
            .map(|e| {
                e.to_row(
                    self.mark_marked,
                    self.entry_is_marked(e, marked_files, marked_dirs),
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

    fn entry_is_marked(
        &self,
        entry: &DirTableEntry,
        marked_files: &HashSet<Arc<PathBuf>>,
        marked_dirs: &HashSet<PathBuf>,
    ) -> bool {
        if entry.is_dir {
            marked_dirs.contains(entry.path.as_path())
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

fn file_record_from_index(path: &Arc<PathBuf>, file_index: &FileIndex) -> FileRecord {
    let file = file_index.files.get(path);

    FileRecord {
        path: path.clone(),
        size: file.map(|entry| entry.size).unwrap_or_default(),
        modified: file.and_then(|entry| entry.modified),
        created: file.and_then(|entry| entry.created),
        clone_count: file_index
            .duplicates
            .get(path)
            .map(|duplicates| duplicates.len())
            .unwrap_or_default(),
    }
}

fn build_directory_nodes(
    paths: &[Arc<PathBuf>],
    file_index: &FileIndex,
    common_path: Option<&Path>,
) -> HashMap<PathBuf, DirectoryNode> {
    let mut nodes = HashMap::new();

    for path in paths {
        let relative_path = relative_path_for_common(path.as_path(), common_path);
        let parent = relative_path
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_path_buf();
        ensure_directory(&mut nodes, &parent);

        let record = file_record_from_index(path, file_index);
        let node = nodes.entry(parent).or_default();
        node.direct_size += record.size;
        node.file_count += 1;
        node.total_size += record.size;
        retain_latest_time(&mut node.modified, record.modified);
        retain_latest_time(&mut node.created, record.created);
        node.direct_files.push(record);
    }

    finalize_directory_summaries(&mut nodes);
    nodes
}

fn ensure_directory(nodes: &mut HashMap<PathBuf, DirectoryNode>, path: &Path) {
    let path = path.to_path_buf();
    if nodes.contains_key(&path) {
        return;
    }

    let parent = directory_parent(&path);
    nodes.insert(
        path.clone(),
        DirectoryNode {
            parent: parent.clone(),
            ..DirectoryNode::default()
        },
    );

    if let Some(parent) = parent {
        ensure_directory(nodes, &parent);
        if let Some(parent_node) = nodes.get_mut(&parent) {
            parent_node.child_dirs.insert(path);
        }
    }
}

fn directory_parent(path: &Path) -> Option<PathBuf> {
    if path.as_os_str().is_empty() {
        return None;
    }

    path.parent()
        .filter(|parent| *parent != path)
        .map(Path::to_path_buf)
}

fn finalize_directory_summaries(nodes: &mut HashMap<PathBuf, DirectoryNode>) {
    let mut paths = nodes.keys().cloned().collect::<Vec<_>>();
    paths.sort_by(|a, b| {
        path_depth(b.as_path())
            .cmp(&path_depth(a.as_path()))
            .then_with(|| b.cmp(a))
    });

    for path in paths {
        let Some(node) = nodes.get(&path) else {
            continue;
        };
        let Some(parent) = node.parent.clone() else {
            continue;
        };
        let file_count = node.file_count;
        let subdirectory_count = node.subdirectory_count;
        let total_size = node.total_size;
        let modified = node.modified;
        let created = node.created;

        if let Some(parent_node) = nodes.get_mut(&parent) {
            parent_node.file_count += file_count;
            parent_node.subdirectory_count += 1 + subdirectory_count;
            parent_node.total_size += total_size;
            retain_latest_time(&mut parent_node.modified, modified);
            retain_latest_time(&mut parent_node.created, created);
        }
    }
}

fn retain_latest_time(current: &mut Option<SystemTime>, candidate: Option<SystemTime>) {
    let Some(candidate) = candidate else {
        return;
    };

    if current.is_none_or(|current| current < candidate) {
        *current = Some(candidate);
    }
}

fn relative_path_for_common(path: &Path, common_path: Option<&Path>) -> PathBuf {
    common_path
        .and_then(|common_path| path.strip_prefix(common_path).ok())
        .unwrap_or(path)
        .to_path_buf()
}

fn root_dir_path(nodes: &HashMap<PathBuf, DirectoryNode>) -> Option<PathBuf> {
    if nodes.contains_key(Path::new("")) {
        return Some(PathBuf::new());
    }

    nodes
        .keys()
        .min_by(|a, b| {
            path_depth(a.as_path())
                .cmp(&path_depth(b.as_path()))
                .then_with(|| a.cmp(b))
        })
        .cloned()
}

fn path_depth(path: &Path) -> usize {
    path.components().count()
}

fn sort_entries(entries: &mut [DirTableEntry], sort_by: Option<Sorting>) {
    if let Some(sort_by) = sort_by {
        entries.sort_by(|a, b| match sort_by {
            Sorting::Path => a.path.cmp(&b.path),
            Sorting::Size => b.size.cmp(&a.size),
            Sorting::Date => b.date.cmp(&a.date),
            Sorting::Count => b.clone_count.cmp(&a.clone_count),
        });
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

    type TestFileEntry = (Arc<PathBuf>, u64, Option<SystemTime>, Option<SystemTime>);

    #[derive(Default)]
    struct NodeStats {
        file_count: usize,
        subdirectory_count: usize,
        total_size: u64,
        modified: Option<SystemTime>,
        created: Option<SystemTime>,
    }

    fn entry(index: usize) -> DirTableEntry {
        DirTableEntry::new(
            Arc::new(PathBuf::from(format!("/tmp/file-{index}"))),
            PathBuf::from(format!("file-{index}")),
            index as u64,
            None,
            index,
            false,
        )
    }

    fn file_entry(path: Arc<PathBuf>, display_path: &str) -> DirTableEntry {
        DirTableEntry::new(path, PathBuf::from(display_path), 0, None, 0, false)
    }

    fn dir_entry(path: &str) -> DirTableEntry {
        DirTableEntry::new(
            Arc::new(PathBuf::from(path)),
            PathBuf::from(path),
            0,
            None,
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

    fn marked_dirs(table: &DirTable, marked: &HashSet<Arc<PathBuf>>) -> HashSet<PathBuf> {
        table.marked_directory_paths(marked)
    }

    fn table_with_common_path(common_path: &str) -> DirTable {
        let mut table = DirTable::new(vec![" ", "File", "Date", "Size"], true, false, false);
        table.common_path = Some(PathBuf::from(common_path));
        table
    }

    fn file_record(path: Arc<PathBuf>, size: u64) -> FileRecord {
        FileRecord {
            path,
            size,
            modified: None,
            created: None,
            clone_count: 0,
        }
    }

    fn path(path: &str) -> Arc<PathBuf> {
        Arc::new(PathBuf::from(path))
    }

    fn file_index_with_entries(root: &str, entries: &[TestFileEntry]) -> Arc<RwLock<FileIndex>> {
        let mut file_index = FileIndex::new(
            HashSet::from([PathBuf::from(root)]),
            deckard::config::SearchConfig::default(),
        );

        for (path, size, modified, created) in entries {
            file_index.files.insert(
                path.clone(),
                deckard::file::FileEntry {
                    path: path.clone(),
                    size: *size,
                    created: *created,
                    modified: *modified,
                    hash: None,
                    image_hash: None,
                    audio_hash: None,
                },
            );
        }

        Arc::new(RwLock::new(file_index))
    }

    fn file_record_with_timestamps(
        path: &str,
        size: u64,
        modified: Option<SystemTime>,
        created: Option<SystemTime>,
    ) -> FileRecord {
        FileRecord {
            path: Arc::new(PathBuf::from(path)),
            size,
            modified,
            created,
            clone_count: 0,
        }
    }

    fn directory_node(
        parent: Option<&str>,
        child_dirs: &[&str],
        direct_files: Vec<FileRecord>,
        stats: NodeStats,
    ) -> DirectoryNode {
        DirectoryNode {
            parent: parent.map(PathBuf::from),
            child_dirs: child_dirs.iter().map(PathBuf::from).collect(),
            direct_size: direct_files.iter().map(|record| record.size).sum(),
            direct_files,
            file_count: stats.file_count,
            subdirectory_count: stats.subdirectory_count,
            total_size: stats.total_size,
            modified: stats.modified,
            created: stats.created,
        }
    }

    fn node_stats(file_count: usize, subdirectory_count: usize, total_size: u64) -> NodeStats {
        NodeStats {
            file_count,
            subdirectory_count,
            total_size,
            ..NodeStats::default()
        }
    }

    fn node_stats_with_timestamps(
        file_count: usize,
        subdirectory_count: usize,
        total_size: u64,
        modified: Option<SystemTime>,
        created: Option<SystemTime>,
    ) -> NodeStats {
        NodeStats {
            file_count,
            subdirectory_count,
            total_size,
            modified,
            created,
        }
    }

    #[test]
    fn back_parent_dir_moves_to_parent_when_nested() {
        let mut table = DirTable::new(vec![" ", "File", "Date", "Size"], true, false, false);
        table.dir_nodes.insert(
            PathBuf::new(),
            directory_node(None, &["deckard-tui"], vec![], node_stats(1, 1, 1)),
        );
        table.dir_nodes.insert(
            PathBuf::from("deckard-tui"),
            directory_node(
                Some(""),
                &[],
                vec![file_record(Arc::new(PathBuf::from("/tmp/file-1")), 1)],
                node_stats(1, 0, 1),
            ),
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
        table.dir_nodes.insert(
            PathBuf::new(),
            directory_node(None, &["deckard-tui"], vec![], node_stats(0, 1, 0)),
        );
        table.current_dir = Some(PathBuf::new());

        assert!(!table.back_parent_dir());
        assert_eq!(table.current_dir, Some(PathBuf::new()));
    }

    #[test]
    fn marks_only_real_ancestor_directories() {
        let table = table_with_common_path("/home/user/deckard");
        let marked = marked_files(&["/home/user/deckard/deckard-tui/Cargo.toml"]);
        let marked_dirs = marked_dirs(&table, &marked);

        assert!(table.entry_is_marked(&dir_entry("deckard-tui"), &marked, &marked_dirs));
        assert!(!table.entry_is_marked(&dir_entry("deckard"), &marked, &marked_dirs));
    }

    #[test]
    fn root_files_do_not_mark_substring_named_dirs() {
        let table = table_with_common_path("/home/user/deckard");
        let marked = marked_files(&["/home/user/deckard/.gitignore"]);
        let marked_dirs = marked_dirs(&table, &marked);

        assert!(!table.entry_is_marked(&dir_entry("deckard"), &marked, &marked_dirs));
        assert!(!table.entry_is_marked(&dir_entry(".git"), &marked, &marked_dirs));
    }

    #[test]
    fn nested_files_mark_component_ancestors() {
        let table = table_with_common_path("/home/user/deckard");
        let marked = marked_files(&["/home/user/deckard/deckard/src/lib.rs"]);
        let marked_dirs = marked_dirs(&table, &marked);

        assert!(table.entry_is_marked(&dir_entry("deckard"), &marked, &marked_dirs));
        assert!(table.entry_is_marked(&dir_entry("deckard/src"), &marked, &marked_dirs));
    }

    #[test]
    fn marked_directory_paths_excludes_file_names_and_root() {
        let table = table_with_common_path("/home/user/deckard");
        let marked = marked_files(&[
            "/home/user/deckard/deckard/src/lib.rs",
            "/home/user/deckard/deckard-tui/Cargo.toml",
        ]);

        let marked_dirs = table.marked_directory_paths(&marked);

        assert!(marked_dirs.contains(Path::new("deckard")));
        assert!(marked_dirs.contains(Path::new("deckard/src")));
        assert!(marked_dirs.contains(Path::new("deckard-tui")));
        assert!(!marked_dirs.contains(Path::new("deckard/src/lib.rs")));
        assert!(!marked_dirs.contains(Path::new("")));
    }

    #[test]
    fn current_file_paths_excludes_directory_rows() {
        let mut table = DirTable::new(vec![" ", "File", "Date", "Size"], true, false, false);
        let file_path = Arc::new(PathBuf::from("/tmp/file"));
        table.current_entries = vec![
            dir_entry("deckard"),
            DirTableEntry::new(file_path.clone(), PathBuf::from("file"), 0, None, 0, false),
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
        table.dir_nodes.insert(
            PathBuf::from("folder"),
            directory_node(
                Some(""),
                &["folder/sub"],
                vec![file_record(direct.clone(), 0)],
                node_stats(2, 1, 0),
            ),
        );
        table.dir_nodes.insert(
            PathBuf::from("folder/sub"),
            directory_node(
                Some("folder"),
                &[],
                vec![file_record(nested.clone(), 0)],
                node_stats(1, 0, 0),
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
        table.dir_nodes.insert(
            PathBuf::from("folder"),
            directory_node(
                Some(""),
                &[],
                vec![file_record(selected_file.clone(), 0)],
                node_stats(1, 0, 0),
            ),
        );
        table.dir_nodes.insert(
            PathBuf::from("folder-sibling"),
            directory_node(
                Some(""),
                &[],
                vec![file_record(sibling_file, 0)],
                node_stats(1, 0, 0),
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
        let oldest_created = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(5);
        let newest_created = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(15);
        let mut table = table_with_common_path("/tmp/root");

        table.current_entries = vec![dir_entry("folder")];
        table.table_len = table.current_entries.len();
        table.dir_nodes.insert(
            PathBuf::from("folder"),
            directory_node(
                Some(""),
                &["folder/sub"],
                vec![file_record_with_timestamps(
                    "/tmp/root/folder/direct.txt",
                    15,
                    Some(older),
                    Some(oldest_created),
                )],
                node_stats_with_timestamps(2, 1, 35, Some(newer), Some(newest_created)),
            ),
        );
        table.dir_nodes.insert(
            PathBuf::from("folder/sub"),
            directory_node(
                Some("folder"),
                &[],
                vec![file_record_with_timestamps(
                    "/tmp/root/folder/sub/nested.txt",
                    20,
                    Some(newer),
                    Some(newest_created),
                )],
                node_stats_with_timestamps(1, 0, 20, Some(newer), Some(newest_created)),
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
                created: Some(newest_created),
            })
        );
    }

    #[test]
    fn selected_dir_info_uses_cached_subdirectory_count() {
        let mut table = DirTable::new(vec![" ", "File", "Date", "Size"], true, false, false);

        table.current_entries = vec![dir_entry("folder")];
        table.table_len = table.current_entries.len();
        table.dir_nodes.insert(
            PathBuf::from("folder"),
            directory_node(Some(""), &[], vec![], node_stats(0, 7, 0)),
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
    fn root_materialization_shows_immediate_directories_and_files_only() {
        let mut table = DirTable::new(vec![" ", "File", "Date", "Size"], true, false, false);
        let root_file = path("/tmp/root/root.txt");
        let direct = path("/tmp/root/folder/direct.txt");
        let nested = path("/tmp/root/folder/sub/nested.txt");
        let paths = vec![nested.clone(), root_file.clone(), direct.clone()];
        let file_index = file_index_with_entries(
            "/tmp/root",
            &[
                (nested, 3, None, None),
                (root_file, 1, None, None),
                (direct, 2, None, None),
            ],
        );

        table.update_table(&paths, &file_index, None);

        let displayed = table
            .current_entries
            .iter()
            .map(|entry| entry.display_text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(displayed, vec!["folder/", "root.txt"]);
    }

    #[test]
    fn entering_nested_directory_materializes_direct_children() {
        let mut table = DirTable::new(vec![" ", "File", "Date", "Size"], true, false, false);
        let direct = path("/tmp/root/folder/direct.txt");
        let nested = path("/tmp/root/folder/sub/nested.txt");
        let paths = vec![direct.clone(), nested.clone()];
        let file_index = file_index_with_entries(
            "/tmp/root",
            &[(direct, 2, None, None), (nested, 3, None, None)],
        );

        table.update_table(&paths, &file_index, None);
        table.select_first();
        table.enter_selected_dir();

        assert_eq!(table.current_dir, Some(PathBuf::from("folder")));
        let displayed = table
            .current_entries
            .iter()
            .map(|entry| entry.name_text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(displayed, vec!["sub/", "direct.txt"]);
    }

    #[test]
    fn recursive_directory_summaries_match_index_metadata() {
        let older = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(10);
        let newer = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(20);
        let oldest_created = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(5);
        let newest_created = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(15);
        let direct = path("/tmp/root/folder/direct.txt");
        let nested = path("/tmp/root/folder/sub/nested.txt");
        let paths = vec![direct.clone(), nested.clone()];
        let file_index = file_index_with_entries(
            "/tmp/root",
            &[
                (direct, 15, Some(older), Some(oldest_created)),
                (nested, 20, Some(newer), Some(newest_created)),
            ],
        );
        let mut table = DirTable::new(vec![" ", "File", "Date", "Size"], true, false, false);

        table.update_table(&paths, &file_index, None);
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
                created: Some(newest_created),
            })
        );
    }

    #[test]
    fn similarly_named_siblings_do_not_affect_selection_summary() {
        let selected = path("/tmp/root/folder/file.txt");
        let sibling = path("/tmp/root/folder-sibling/file.txt");
        let paths = vec![selected.clone(), sibling.clone()];
        let file_index = file_index_with_entries(
            "/tmp/root",
            &[
                (selected.clone(), 10, None, None),
                (sibling, 20, None, None),
            ],
        );
        let mut table = DirTable::new(vec![" ", "File", "Date", "Size"], true, false, false);

        table.update_table(&paths, &file_index, Some(&Sorting::Path));
        let folder_index = table
            .current_entries
            .iter()
            .position(|entry| entry.path.as_path() == Path::new("folder"))
            .unwrap();
        table.select_entry(folder_index);

        assert_eq!(table.selected_dir_file_paths(), vec![selected]);
        assert_eq!(
            table.selected_dir_info().map(|info| (
                info.file_count,
                info.subdirectory_count,
                info.total_size
            )),
            Some((1, 0, 10))
        );
    }

    #[test]
    fn root_fallback_chooses_shallowest_lexicographic_directory() {
        let mut nodes = HashMap::new();
        nodes.insert(PathBuf::from("beta/nested"), DirectoryNode::default());
        nodes.insert(PathBuf::from("beta"), DirectoryNode::default());
        nodes.insert(PathBuf::from("alpha"), DirectoryNode::default());

        assert_eq!(root_dir_path(&nodes), Some(PathBuf::from("alpha")));

        nodes.insert(PathBuf::new(), DirectoryNode::default());
        assert_eq!(root_dir_path(&nodes), Some(PathBuf::new()));
    }

    #[test]
    fn sorting_applies_to_materialized_current_directory_only() {
        let root_file = path("/tmp/root/root.txt");
        let small = path("/tmp/root/a/small.txt");
        let large = path("/tmp/root/b/large.txt");
        let nested = path("/tmp/root/b/sub/tiny.txt");
        let paths = vec![
            root_file.clone(),
            small.clone(),
            large.clone(),
            nested.clone(),
        ];
        let file_index = file_index_with_entries(
            "/tmp/root",
            &[
                (root_file, 10, None, None),
                (small, 1, None, None),
                (large, 50, None, None),
                (nested, 1, None, None),
            ],
        );
        let mut table = DirTable::new(vec![" ", "File", "Date", "Size"], true, false, false);

        table.update_table(&paths, &file_index, Some(&Sorting::Size));

        let displayed = table
            .current_entries
            .iter()
            .map(|entry| entry.name_text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(displayed, vec!["b/", "root.txt", "a/"]);

        table.select_first();
        table.enter_selected_dir();
        let displayed = table
            .current_entries
            .iter()
            .map(|entry| entry.name_text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(displayed, vec!["large.txt", "sub/"]);
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
        table.render(&mut buf, area, true, &HashSet::new(), &HashSet::new());

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
