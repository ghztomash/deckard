use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

use deckard::find_common_path;
use deckard::index::FileIndex;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Scrollbar, ScrollbarOrientation, StatefulWidget,
};

use tracing::warn;
use tui_tree_widget::{Tree, TreeItem, TreeState};

use crate::app::{Sorting, format_path};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum TreeNode {
    File {
        display_path: PathBuf,
        path: Arc<PathBuf>,
        size: u64,
        date: Option<SystemTime>,
        clone_count: usize,
        is_marked: bool,
    },
    Directory {
        display_path: PathBuf,
        children: BTreeMap<PathBuf, TreeNode>,
        total_size: u64,
        date: Option<SystemTime>,
        num_files: usize,
    },
}

impl TreeNode {
    fn path(&self) -> PathBuf {
        match self {
            TreeNode::File { display_path, .. } => display_path.clone(),
            TreeNode::Directory { display_path, .. } => display_path.clone(),
        }
    }

    fn size(&self) -> u64 {
        match self {
            TreeNode::File { size, .. } => *size,
            TreeNode::Directory { total_size, .. } => *total_size,
        }
    }

    fn files(&self) -> usize {
        match self {
            TreeNode::File { clone_count, .. } => *clone_count,
            TreeNode::Directory { num_files, .. } => *num_files,
        }
    }

    fn date(&self) -> Option<SystemTime> {
        match self {
            TreeNode::File { date, .. } => *date,
            TreeNode::Directory { date, .. } => *date,
        }
    }

    fn new_dir(path: PathBuf) -> Self {
        TreeNode::Directory {
            display_path: path,
            children: BTreeMap::new(),
            date: None,
            total_size: 0,
            num_files: 0,
        }
    }

    fn new_file(
        path: Arc<PathBuf>,
        display_path: PathBuf,
        size: u64,
        date: Option<SystemTime>,
        clone_count: usize,
        is_marked: bool,
    ) -> Self {
        TreeNode::File {
            path,
            display_path,
            size,
            date,
            clone_count,
            is_marked,
        }
    }

    /// Insert a new file node into this tree
    fn insert(&mut self, node: TreeNode) {
        let display_path = match &node {
            TreeNode::File { display_path, .. } => display_path.clone(),
            TreeNode::Directory { .. } => {
                warn!("Inserting directories directly is not supported");
                return;
            }
        };

        let mut components = display_path.components().peekable();

        if let TreeNode::Directory {
            children,
            total_size,
            num_files,
            ..
        } = self
        {
            Self::insert_recursive(PathBuf::new(), children, &mut components, node);

            // Recompute aggregated stats
            *total_size = children.values().map(|c| c.total_size()).sum();
            *num_files = children.values().map(|c| c.num_files()).sum();
        } else {
            warn!("Cannot insert into a file node");
        }
    }

    fn insert_recursive(
        mut prefix: PathBuf,
        children: &mut BTreeMap<PathBuf, TreeNode>,
        components: &mut std::iter::Peekable<std::path::Components<'_>>,
        node: TreeNode,
    ) {
        if let Some(component) = components.next() {
            prefix.push(component); // extend prefix
            let comp_path = prefix.clone();

            if components.peek().is_none() {
                // Leaf level -> insert the file
                children.insert(comp_path, node);
            } else {
                // Intermediate directory
                let entry = children
                    .entry(comp_path.clone())
                    .or_insert_with(|| TreeNode::new_dir(comp_path.clone()));

                if let TreeNode::Directory {
                    children,
                    total_size,
                    num_files,
                    ..
                } = entry
                {
                    Self::insert_recursive(prefix, children, components, node);

                    *total_size = children.values().map(|c| c.total_size()).sum();
                    *num_files = children.values().map(|c| c.num_files()).sum();
                }
            }
        }
    }

    fn total_size(&self) -> u64 {
        match self {
            TreeNode::File { size, .. } => *size,
            TreeNode::Directory { total_size, .. } => *total_size,
        }
    }

    fn num_files(&self) -> usize {
        match self {
            TreeNode::File { .. } => 1,
            TreeNode::Directory { num_files, .. } => *num_files,
        }
    }

    /// Convert a `TreeNode` into a `TreeItem` for rendering
    fn to_tree_item(&self, sort_by: Option<Sorting>) -> TreeItem<'static, TreeNode> {
        match self {
            TreeNode::File {
                size,
                clone_count,
                display_path,
                ..
            } => TreeItem::new_leaf(
                self.clone(),
                Line::from(vec![
                    Span::raw(format!(
                        "{} ",
                        display_path.file_name().unwrap_or_default().display(),
                    )),
                    Span::styled(
                        format!(
                            "- clones: {}, size: {}",
                            clone_count,
                            humansize::format_size(*size, humansize::DECIMAL),
                        ),
                        Style::default().dark_gray(),
                    ),
                ]),
            ),
            TreeNode::Directory {
                display_path,
                children,
                total_size,
                num_files,
                ..
            } => {
                let label = Line::from(vec![
                    Span::raw(format!(
                        "{} ",
                        display_path.file_name().unwrap_or_default().display(),
                    )),
                    Span::styled(
                        format!(
                            "- files: {}, total: {}",
                            num_files,
                            humansize::format_size(*total_size, humansize::DECIMAL),
                        ),
                        Style::default().dark_gray(),
                    ),
                ]);

                let child_items: Vec<TreeItem<TreeNode>> = children
                    .values()
                    .map(|child_node| child_node.to_tree_item(sort_by))
                    .collect();

                TreeItem::new(self.clone(), label, child_items).expect("reason")
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct FileTree<'a> {
    tree_state: TreeState<TreeNode>,
    pub table_len: usize,
    selected_path: Option<Arc<PathBuf>>,
    entries: Vec<TreeItem<'a, TreeNode>>,
    common_path: Option<PathBuf>,
    sort_by: Option<Sorting>,
}

impl FileTree<'_> {
    pub fn update_tree(
        &mut self,
        paths: &Vec<Arc<PathBuf>>,
        file_index: &Arc<RwLock<FileIndex>>,
        sort_by: Option<&Sorting>,
    ) {
        // Lock the FileIndex only once, then copy out the data we need:
        let (mut entries, common_path) = {
            let fi = file_index.read().unwrap();

            // Pre-calculate file metadata for each path we display,
            // including size & date.
            let common_path = deckard::find_common_path(&fi.dirs);
            let mut entries_vec = Vec::with_capacity(paths.len());
            for path in paths {
                let size = fi.file_size(path).unwrap_or_default();
                let clone_count = fi.file_duplicates_len(path).unwrap_or_default();
                let date = fi.file_date_modified(path); // or created
                let display_path = format_path(path, &fi.dirs);

                entries_vec.push(TreeNode::new_file(
                    path.clone(),
                    display_path,
                    size,
                    date,
                    clone_count,
                    false,
                ));
            }

            (entries_vec, common_path)
        };

        // Sort the paths
        if let Some(sort_by) = sort_by {
            entries.sort_by(|a, b| match sort_by {
                Sorting::Path => a.path().cmp(&b.path()),
                Sorting::Size => b.size().cmp(&a.size()),
                Sorting::Date => b.date().cmp(&a.date()),
                Sorting::Count => b.files().cmp(&a.files()),
            });
        }

        let common_display = common_path
            .clone()
            .map(|p| PathBuf::from(p.file_name().unwrap_or_default()))
            .unwrap_or_default();

        let mut root = TreeNode::new_dir(common_display.clone());
        for entry in entries {
            root.insert(entry);
        }

        let items = vec![root.to_tree_item(sort_by.cloned())];

        self.entries = items;
        self.common_path = common_path;
        self.sort_by = sort_by.cloned();

        // open the first level
        self.tree_state
            .open(vec![TreeNode::new_dir(common_display.clone())]);
    }

    pub fn render(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        focused: bool,
        marked_files: &HashSet<Arc<PathBuf>>,
    ) {
        let block = if focused {
            Block::bordered()
                .border_type(BorderType::Thick)
                .border_style(Style::new().light_magenta())
        } else {
            Block::bordered()
                .border_type(BorderType::Plain)
                .border_style(Style::new().dark_gray())
        };

        let selected_style = if focused {
            Style::default()
                .fg(Color::Black)
                .bg(Color::LightGreen)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Black).bg(Color::DarkGray)
        };

        let widget = Tree::new(&self.entries)
            .expect("all item identifiers are unique")
            // .node_open_symbol("📂")
            // .node_closed_symbol("📁")
            // .node_no_children_symbol("📄")
            .block(block.title_bottom(format!("{:?}", self.tree_state.selected())))
            .experimental_scrollbar(Some(
                Scrollbar::default().orientation(ScrollbarOrientation::VerticalRight),
            ))
            .highlight_style(selected_style);

        StatefulWidget::render(widget, area, buf, &mut self.tree_state);
    }

    pub fn select_none(&mut self) {
        self.tree_state.select(vec![]);
        self.selected_path = None;
    }

    pub fn select_first(&mut self) {
        self.tree_state.select_first();
        self.selected_path = None;
    }

    pub fn select_next(&mut self) {
        self.tree_state.key_down();
    }

    pub fn select_previous(&mut self) {
        self.tree_state.key_up();
    }

    pub fn key_right(&mut self) {
        self.tree_state.key_right();
    }

    pub fn key_left(&mut self) {
        self.tree_state.key_left();
    }

    pub fn key_enter(&mut self) {
        self.tree_state.toggle_selected();
    }

    pub fn selected_path(&self) -> Option<Arc<PathBuf>> {
        if let Some(selected) = self.tree_state.selected().last()
            && let TreeNode::File { path, .. } = selected
        {
            return Some(path.clone());
        }
        None
    }
}
