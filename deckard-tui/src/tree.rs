use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use deckard::index::FileIndex;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Scrollbar, ScrollbarOrientation, StatefulWidget};

use tracing::warn;
use tui_tree_widget::{Tree, TreeItem, TreeState};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum TreeNode {
    File {
        path: Arc<PathBuf>,
        size: u64,
        clone_count: usize,
    },
    Directory {
        path: Arc<PathBuf>,
        children: BTreeMap<Arc<PathBuf>, TreeNode>,
        total_size: u64,
        num_files: usize,
    },
}

impl TreeNode {
    fn new_root() -> Self {
        TreeNode::Directory {
            path: Arc::new(PathBuf::new()),
            children: BTreeMap::new(),
            total_size: 0,
            num_files: 0,
        }
    }

    fn new_dir(path: Arc<PathBuf>) -> Self {
        TreeNode::Directory {
            path,
            children: BTreeMap::new(),
            total_size: 0,
            num_files: 0,
        }
    }

    fn new_file(path: Arc<PathBuf>, size: u64, clone_count: usize) -> Self {
        TreeNode::File {
            path,
            size,
            clone_count,
        }
    }

    /// Insert a new file node into this tree
    fn insert(&mut self, node: TreeNode) {
        let path = match &node {
            TreeNode::File { path, .. } => Arc::clone(path),
            TreeNode::Directory { .. } => {
                warn!("Inserting directories directly is not supported");
                return;
            }
        };

        let mut components = path.components().peekable();

        if let TreeNode::Directory {
            path,
            children,
            total_size,
            num_files,
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
        children: &mut BTreeMap<Arc<PathBuf>, TreeNode>,
        components: &mut std::iter::Peekable<std::path::Components<'_>>,
        node: TreeNode,
    ) {
        if let Some(component) = components.next() {
            prefix.push(component); // extend prefix
            let comp_path = Arc::new(prefix.clone());

            if components.peek().is_none() {
                // Leaf level → insert the file
                children.insert(comp_path, node);
            } else {
                // Intermediate directory
                let entry = children
                    .entry(comp_path.clone())
                    .or_insert_with(|| TreeNode::new_dir(comp_path.clone()));

                if let TreeNode::Directory {
                    path: _,
                    children,
                    total_size,
                    num_files,
                } = entry
                {
                    Self::insert_recursive(prefix, children, components, node);

                    *total_size = children.values().map(|c| c.total_size()).sum();
                    *num_files = children.values().map(|c| c.num_files()).sum();
                }
            }
        }
    }

    // fn insert_recursive(
    //     prefix: Arc<PathBuf>,
    //     children: &mut BTreeMap<Arc<PathBuf>, TreeNode>,
    //     components: &mut std::iter::Peekable<std::path::Components<'_>>,
    //     node: TreeNode,
    // ) {
    //     if let Some(component) = components.next() {
    //         let comp_path = Arc::new(PathBuf::from(component.as_os_str()));
    //
    //         if components.peek().is_none() {
    //             // Leaf level → insert the file
    //             children.insert(comp_path, node);
    //         } else {
    //             // Intermediate directory
    //             let entry = children
    //                 .entry(comp_path.clone())
    //                 .or_insert_with(TreeNode::new_root);
    //             if let TreeNode::Directory {
    //                 path,
    //                 children,
    //                 total_size,
    //                 num_files,
    //             } = entry
    //             {
    //                 Self::insert_recursive(prefix, children, components, node);
    //
    //                 *total_size = children.values().map(|c| c.total_size()).sum();
    //                 *num_files = children.values().map(|c| c.num_files()).sum();
    //             }
    //         }
    //     }
    // }

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
    fn to_tree_item(&self) -> TreeItem<'static, Arc<PathBuf>> {
        match self {
            TreeNode::File {
                size,
                clone_count,
                path,
            } => TreeItem::new_leaf(
                path.clone(),
                Line::from(vec![Span::raw(format!(
                    "{} [{} bytes, clones: {}]",
                    path.to_string_lossy(),
                    size,
                    clone_count
                ))]),
            ),
            TreeNode::Directory {
                path,
                children,
                total_size,
                num_files,
            } => {
                let label = Line::from(vec![Span::styled(
                    format!(
                        "{} [📁 {} files, {} bytes]",
                        path.to_string_lossy(),
                        num_files,
                        total_size
                    ),
                    Style::default().add_modifier(Modifier::BOLD),
                )]);
                let child_items: Vec<TreeItem<Arc<PathBuf>>> = children
                    .iter()
                    .map(|(child_name, child_node)| child_node.to_tree_item())
                    .collect();
                TreeItem::new(path.clone(), label, child_items).expect("reason")
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct FileTree<'a> {
    tree_state: TreeState<Arc<PathBuf>>,
    pub table_len: usize,
    selected_path: Option<Arc<PathBuf>>,
    entries: Vec<TreeItem<'a, Arc<PathBuf>>>,
}

impl FileTree<'_> {
    pub fn update_tree(&mut self, paths: &Vec<Arc<PathBuf>>, file_index: &Arc<RwLock<FileIndex>>) {
        // Lock the FileIndex only once, then copy out the data we need:
        let (entries, total_size) = {
            let fi = file_index.read().unwrap();

            // Pre-calculate file metadata for each path we display,
            // including size & date. Also track a sum to show total size.
            let mut total_size_acc = 0u64;
            let mut entries = TreeNode::new_root();
            for path in paths {
                let size = fi.file_size(path).unwrap_or_default();
                let clone_count = fi.file_duplicates_len(path).unwrap_or_default();
                total_size_acc += size;

                entries.insert(TreeNode::new_file(path.clone(), size, clone_count));
            }

            (entries, total_size_acc)
        };

        let items = vec![entries.to_tree_item()];

        self.entries = items;
        // self.tree_state.close_all();

        let mut vecs = vec![Arc::new(PathBuf::new())];
        self.tree_state.open(vecs);
    }

    pub fn render(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        focused: bool,
        marked_files: &HashSet<Arc<PathBuf>>,
    ) {
        let widget = Tree::new(&self.entries)
            .expect("all item identifiers are unique")
            .block(
                Block::bordered()
                    .title("Tree Widget")
                    .title_bottom(format!("{:?}", self.tree_state)),
            )
            .experimental_scrollbar(Some(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(None)
                    .track_symbol(None)
                    .end_symbol(None),
            ))
            .highlight_style(
                Style::new()
                    .fg(Color::Black)
                    .bg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol(">> ");

        StatefulWidget::render(widget, area, buf, &mut self.tree_state);
    }

    pub fn select_none(&mut self) {
        self.tree_state.select(vec![]);
        self.selected_path = None;
    }

    pub fn select_next(&mut self) {
        self.tree_state.key_down();
    }

    pub fn select_previous(&mut self) {
        self.tree_state.key_up();
    }

    pub fn select_enter(&mut self) {
        self.tree_state.key_right();
    }
}
