use crossterm::event::{Event, KeyEvent};
use reedline::{
    EditCommand, EditMode, Emacs, Reedline, ReedlineEvent, ReedlineRawEvent, Span, Suggestion,
};
use std::{
    collections::{BTreeSet, VecDeque},
    path::Path,
};

const HISTORY_LIMIT: usize = 16;
const PATH_LIMIT: usize = 24;

/// The paths a filter command can act on.
#[derive(Clone, Copy)]
pub(crate) enum FilterScope {
    /// All files eligible for the Files table.
    Indexed,
    /// File rows in the current Files view.
    Current,
}

struct Command {
    name: &'static str,
    alias: &'static str,
    help: &'static str,
    filter_scope: Option<FilterScope>,
}

impl Command {
    const fn new(name: &'static str, alias: &'static str, help: &'static str) -> Self {
        Self {
            name,
            alias,
            help,
            filter_scope: None,
        }
    }

    const fn filter(mut self, scope: FilterScope) -> Self {
        self.filter_scope = Some(scope);
        self
    }

    fn label(&self) -> String {
        let argument = if self.filter_scope.is_some() {
            " <str>"
        } else {
            ""
        };
        format!("[{}] {}{argument}", self.alias, self.name)
    }
}

const COMMANDS: &[Command] = &[
    Command::new("filter", "f", "show only files containing <str> in path")
        .filter(FilterScope::Indexed),
    Command::new(
        "parent_filter",
        "pf",
        "filter files in the parent directory",
    ),
    Command::new("clear_filter", "cf", "remove filter"),
    Command::new("mark_filter", "mf", "mark files containing <str> in path")
        .filter(FilterScope::Current),
    Command::new(
        "mark_directory",
        "md",
        "mark files under selected directory recursively",
    ),
    Command::new("mark_all", "ma", "mark all displayed files"),
    Command::new("mark_parent", "mp", "mark files in parent directory"),
    Command::new("clear_marked", "cm", "unmark all files"),
    Command::new("flatten_dirs", "fd", "display all files at once"),
    Command::new("help", "h", "show this window"),
    Command::new("quit", "q", "quit deckard"),
];

fn find_command(name: &str) -> Option<&'static Command> {
    COMMANDS
        .iter()
        .find(|command| name == command.name || name == command.alias)
}

/// Render the command section of the About window from the command definitions.
pub(crate) fn command_help() -> String {
    COMMANDS
        .iter()
        .map(|command| format!("{} - {}\n", command.label(), command.help))
        .collect()
}

/// Resolve an alias and parse the optional first filter argument.
pub(crate) fn parse_command(input: &str) -> Result<(&'static str, Option<String>), &'static str> {
    let input = input.trim_start();
    let name_end = word_end(input);
    let command = find_command(&input[..name_end]).ok_or("Unknown command")?;
    let argument = if command.filter_scope.is_some() {
        let argument = input[name_end..].trim_start();
        if argument.is_empty() {
            None
        } else {
            let (value, end, closed) = filter_argument(argument);
            if !closed {
                return Err("Unclosed quote");
            }
            let rest = &argument[end..];
            if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
                return Err("Invalid filter");
            }
            if value.is_empty() {
                return Err("Empty filter");
            }
            Some(value)
        }
    } else {
        None
    };
    Ok((command.name, argument))
}

fn word_end(input: &str) -> usize {
    input.find(char::is_whitespace).unwrap_or(input.len())
}

// Decode the first argument and return its byte length, including any quotes.
fn filter_argument(input: &str) -> (String, usize, bool) {
    let Some(quoted) = input.strip_prefix('"') else {
        let end = word_end(input);
        return (input[..end].to_owned(), end, true);
    };
    let mut value = String::new();
    let mut chars = quoted.char_indices().peekable();
    while let Some((offset, character)) = chars.next() {
        match character {
            '"' => return (value, offset + 2, true),
            '\\' if matches!(chars.peek(), Some((_, '"' | '\\'))) => {
                if let Some((_, escaped)) = chars.next() {
                    value.push(escaped);
                }
            }
            _ => value.push(character),
        }
    }
    (value, input.len(), false)
}

fn quote_filter(value: &str) -> String {
    if value.contains('"') || value.chars().any(char::is_whitespace) {
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    } else {
        value.to_owned()
    }
}

fn suggestions(
    line: &str,
    cursor: usize,
    paths: impl FnOnce(FilterScope, &str, Span) -> Vec<Suggestion>,
) -> Vec<Suggestion> {
    let input = line.trim_start();
    let name_start = line.len() - input.len();
    let name_end = name_start + word_end(input);
    if input.is_empty() || (name_start..=name_end).contains(&cursor) {
        let name_start = name_start.min(cursor);
        let Some(prefix) = line.get(name_start..cursor) else {
            return Vec::new();
        };
        let mut matches: Vec<_> = COMMANDS
            .iter()
            .filter(|command| command.name.starts_with(prefix) || command.alias.starts_with(prefix))
            .map(|command| Suggestion {
                value: command.name.into(),
                description: Some(command.help.into()),
                display_override: Some(command.label()),
                span: Span::new(name_start, if input.is_empty() { cursor } else { name_end }),
                append_whitespace: command.filter_scope.is_some() && name_end == line.len(),
                ..Suggestion::default()
            })
            .collect();
        matches.sort_unstable_by(|left, right| left.value.cmp(&right.value));
        return matches;
    }

    let Some(scope) =
        find_command(&line[name_start..name_end]).and_then(|command| command.filter_scope)
    else {
        return Vec::new();
    };
    let argument = line[name_end..].trim_start();
    let start = line.len() - argument.len();
    if argument.is_empty() && cursor > name_end && cursor <= start {
        return paths(scope, "", Span::new(cursor, cursor));
    }
    let (_, length, _) = filter_argument(argument);
    let end = start + length;
    if !(start..=end).contains(&cursor) {
        return Vec::new();
    }
    let Some(prefix) = line.get(start..cursor) else {
        return Vec::new();
    };
    let (query, _, _) = filter_argument(prefix);
    paths(scope, &query, Span::new(start, end))
}

/// Return a bounded, sorted list of path suggestions for a filter argument.
pub(crate) fn path_suggestions<'a>(
    paths: impl Iterator<Item = &'a Path>,
    query: &str,
    span: Span,
) -> Vec<Suggestion> {
    let mut matches = BTreeSet::new();
    for path in paths {
        let path = path.to_string_lossy();
        if path.contains(query) && !path.chars().any(char::is_control) {
            matches.insert(path);
            if matches.len() > PATH_LIMIT {
                matches.pop_last();
            }
        }
    }
    matches
        .into_iter()
        .map(|path| Suggestion {
            value: quote_filter(&path),
            span,
            ..Suggestion::default()
        })
        .collect()
}

/// What a key did in the command prompt.
pub(crate) enum CommandEdit {
    /// The key had no applicable action.
    Ignored,
    /// The prompt needs to be redrawn.
    Changed,
    /// The user submitted the line.
    Submit(String),
    /// The user dismissed the prompt.
    Cancel,
}

struct HistoryNavigation {
    index: usize,
    draft: String,
    cursor: usize,
}

struct CompletionCycle {
    original: String,
    suggestions: Vec<Suggestion>,
    selected: usize,
}

/// Reedline editing state and the history of submitted Deckard commands.
pub(crate) struct CommandEditor {
    editor: Reedline,
    keys: Emacs,
    history: VecDeque<String>,
    navigation: Option<HistoryNavigation>,
    completion: Option<CompletionCycle>,
}

impl CommandEditor {
    /// Create an empty editor and session history.
    pub(crate) fn new() -> Self {
        Self {
            editor: Reedline::create(),
            keys: Emacs::default(),
            history: VecDeque::with_capacity(HISTORY_LIMIT),
            navigation: None,
            completion: None,
        }
    }

    /// Begin a prompt with an empty buffer.
    pub(crate) fn open(&mut self) -> std::io::Result<()> {
        // Dropping the previous prompt releases its undo stack and disables raw mode.
        let raw_mode = crossterm::terminal::is_raw_mode_enabled()?;
        self.editor = Reedline::create();
        if raw_mode {
            crossterm::terminal::enable_raw_mode()?;
        }
        self.close();
        Ok(())
    }

    /// Dismiss any history navigation or completion menu.
    pub(crate) fn close(&mut self) {
        self.navigation = None;
        self.completion = None;
    }

    /// Text in the prompt.
    pub(crate) fn buffer(&self) -> &str {
        self.editor.current_buffer_contents()
    }

    /// Cursor as a byte offset in the prompt text.
    pub(crate) fn cursor(&self) -> usize {
        self.editor.current_insertion_point()
    }

    /// Candidates and the index of the one currently inserted.
    pub(crate) fn completions(&self) -> Option<(&[Suggestion], usize)> {
        self.completion
            .as_ref()
            .filter(|cycle| !cycle.suggestions.is_empty())
            .map(|cycle| (cycle.suggestions.as_slice(), cycle.selected))
    }

    /// Translate and apply one key without reading from or writing to the terminal.
    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> CommandEdit {
        let Ok(event) = ReedlineRawEvent::try_from(Event::Key(key)) else {
            return CommandEdit::Ignored;
        };
        let event = self.keys.parse_event(event);
        self.apply_event(event)
    }

    fn apply_event(&mut self, event: ReedlineEvent) -> CommandEdit {
        match event {
            ReedlineEvent::Enter => {
                let input = self.buffer().to_owned();
                if !input.trim().is_empty() {
                    if self.history.len() == HISTORY_LIMIT {
                        self.history.pop_back();
                    }
                    self.history.push_front(input.clone());
                }
                CommandEdit::Submit(input)
            }
            ReedlineEvent::CtrlC | ReedlineEvent::Esc => CommandEdit::Cancel,
            ReedlineEvent::PreviousHistory | ReedlineEvent::Up => self.recall_older(),
            ReedlineEvent::NextHistory | ReedlineEvent::Down => self.recall_newer(),
            ReedlineEvent::Left => self.edit(&[EditCommand::MoveLeft { select: false }]),
            ReedlineEvent::Right => self.edit(&[EditCommand::MoveRight { select: false }]),
            ReedlineEvent::ToStart => self.edit(&[EditCommand::MoveToStart { select: false }]),
            ReedlineEvent::ToEnd => self.edit(&[EditCommand::MoveToEnd { select: false }]),
            ReedlineEvent::CtrlD => self.edit(&[EditCommand::Delete]),
            ReedlineEvent::Edit(commands) => self.edit(&commands),
            ReedlineEvent::UntilFound(events) => events
                .into_iter()
                .map(|event| self.apply_event(event))
                .find(|result| !matches!(result, CommandEdit::Ignored))
                .unwrap_or(CommandEdit::Ignored),
            _ => CommandEdit::Ignored,
        }
    }

    fn edit(&mut self, commands: &[EditCommand]) -> CommandEdit {
        if commands.iter().any(|command| match command {
            // The single-line prompt has no selection highlight.
            EditCommand::SelectAll
            | EditCommand::MoveLeft { select: true }
            | EditCommand::MoveRight { select: true }
            | EditCommand::MoveWordLeft { select: true }
            | EditCommand::MoveWordRight { select: true }
            | EditCommand::MoveLineUp { select: true }
            | EditCommand::MoveLineDown { select: true }
            | EditCommand::MoveToLineStart { select: true }
            | EditCommand::MoveToLineEnd { select: true }
            | EditCommand::MoveToStart { select: true }
            | EditCommand::MoveToEnd { select: true } => true,
            EditCommand::InsertNewline
            | EditCommand::InsertNewlineAbove
            | EditCommand::InsertNewlineBelow => true,
            EditCommand::InsertChar(character) => character.is_control(),
            _ => false,
        }) {
            return CommandEdit::Ignored;
        }

        self.completion = None;
        self.editor.run_edit_commands(commands);
        if let Some(navigation) = &self.navigation
            && self.buffer() != self.history[navigation.index]
        {
            self.navigation = None;
        }
        CommandEdit::Changed
    }

    fn recall_older(&mut self) -> CommandEdit {
        let cleared = self.completion.take().is_some();
        if self.history.is_empty() {
            return if cleared {
                CommandEdit::Changed
            } else {
                CommandEdit::Ignored
            };
        }
        let navigation = match &mut self.navigation {
            Some(navigation) => {
                navigation.index = (navigation.index + 1).min(self.history.len() - 1);
                navigation
            }
            None => self.navigation.insert(HistoryNavigation {
                index: 0,
                draft: self.editor.current_buffer_contents().to_owned(),
                cursor: self.editor.current_insertion_point(),
            }),
        };
        let input = self.history[navigation.index].clone();
        let cursor = input.len();
        self.replace_buffer(input, cursor);
        CommandEdit::Changed
    }

    fn recall_newer(&mut self) -> CommandEdit {
        let cleared = self.completion.take().is_some();
        let Some(navigation) = &mut self.navigation else {
            return if cleared {
                CommandEdit::Changed
            } else {
                CommandEdit::Ignored
            };
        };
        if navigation.index == 0 {
            if let Some(draft) = self.navigation.take() {
                self.replace_buffer(draft.draft, draft.cursor);
            }
        } else {
            navigation.index -= 1;
            let input = self.history[navigation.index].clone();
            let cursor = input.len();
            self.replace_buffer(input, cursor);
        }
        CommandEdit::Changed
    }

    /// Insert or cycle a completion, consulting the path source only at the start of a cycle.
    pub(crate) fn complete(
        &mut self,
        backwards: bool,
        paths: impl FnOnce(FilterScope, &str, Span) -> Vec<Suggestion>,
    ) -> bool {
        if let Some(cycle) = &mut self.completion {
            let count = cycle.suggestions.len();
            if count == 0 {
                return false;
            }
            cycle.selected = if backwards {
                (cycle.selected + count - 1) % count
            } else {
                (cycle.selected + 1) % count
            };
        } else {
            let suggestions = suggestions(self.buffer(), self.cursor(), paths);
            let selected = if backwards {
                suggestions.len().saturating_sub(1)
            } else {
                0
            };
            self.completion = Some(CompletionCycle {
                original: self.buffer().to_owned(),
                suggestions,
                selected,
            });
        }

        if let Some(cycle) = &self.completion
            && let Some(candidate) = cycle.suggestions.get(cycle.selected)
        {
            let finished_command = cycle.suggestions.len() == 1 && candidate.append_whitespace;
            let mut line = cycle.original.clone();
            let span = candidate.span;
            line.replace_range(span.start..span.end, &candidate.value);
            let mut cursor = span.start + candidate.value.len();
            if candidate.append_whitespace {
                line.insert(cursor, ' ');
                cursor += 1;
            }
            self.navigation = None;
            self.replace_buffer(line, cursor);
            if finished_command {
                // A unique command has been accepted. The next Tab can complete its argument.
                self.completion = None;
            }
            return true;
        }
        false
    }

    fn replace_buffer(&mut self, input: String, cursor: usize) {
        self.editor.run_edit_commands(&[
            EditCommand::Clear,
            EditCommand::InsertString(input),
            EditCommand::MoveToPosition {
                position: cursor,
                select: false,
            },
        ]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};
    use std::path::PathBuf;

    #[test]
    fn command_completion_matches_alias_once_and_cycles_both_ways() {
        let mut editor = CommandEditor::new();
        for character in "ma".chars() {
            editor.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        assert!(editor.complete(false, |_, _, _| unreachable!()));
        let (matches, selected) = editor.completions().unwrap();
        assert_eq!(selected, 0);
        assert_eq!(
            matches
                .iter()
                .filter(|item| item.value == "mark_all")
                .count(),
            1
        );
        let last = matches.len() - 1;
        assert!(editor.complete(true, |_, _, _| unreachable!()));
        assert_eq!(editor.completions().unwrap().1, last);
        assert!(editor.complete(false, |_, _, _| unreachable!()));
        assert_eq!(editor.completions().unwrap().1, 0);

        editor.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert!(editor.completions().is_none());
    }

    #[test]
    fn filter_completion_reuses_an_empty_lookup_until_edit() {
        let mut editor = CommandEditor::new();
        for character in "f absent".chars() {
            editor.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        let mut lookups = 0;
        let mut empty_lookup = |_, _: &str, _| {
            lookups += 1;
            Vec::new()
        };
        assert!(!editor.complete(false, &mut empty_lookup));
        assert!(!editor.complete(true, &mut empty_lookup));
        editor.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert!(!editor.complete(false, &mut empty_lookup));
        assert_eq!(lookups, 2);
    }

    #[test]
    fn command_mode_undo_cannot_restore_a_previous_prompt() {
        let mut editor = CommandEditor::new();
        editor.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        editor.open().unwrap();
        editor.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        editor.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
        assert!(editor.buffer().is_empty());
        editor.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
        assert!(editor.buffer().is_empty());
    }

    #[test]
    fn command_completion_decodes_a_partial_quote_and_preserves_following_text() {
        let line = r#"mf "reports 2026" trailing"#;
        let cursor = line.find("2026").unwrap();
        let matches = suggestions(line, cursor, |scope, query, span| {
            assert!(matches!(scope, FilterScope::Current));
            assert_eq!(query, "reports ");
            let path = Path::new("reports 2025");
            path_suggestions(std::iter::once(path), query, span)
        });
        let candidate = &matches[0];
        let mut completed = line.to_owned();
        completed.replace_range(candidate.span.start..candidate.span.end, &candidate.value);
        assert_eq!(
            parse_command(&completed),
            Ok(("mark_filter", Some("reports 2025".to_owned())))
        );
        assert_eq!(
            &completed[candidate.span.start + candidate.value.len()..],
            " trailing"
        );
    }

    #[test]
    fn command_completion_filter_quoting_round_trips() {
        let value = r#"one "two"\three"#;
        let encoded = quote_filter(value);
        assert_eq!(
            parse_command(&format!("f {encoded}")),
            Ok(("filter", Some(value.to_owned())))
        );
        assert_eq!(
            parse_command("f reports extra"),
            Ok(("filter", Some("reports".to_owned())))
        );
        assert!(parse_command("mf \"").is_err());
        assert!(parse_command("mf \"\"").is_err());
    }

    #[test]
    fn filter_completion_caps_unique_matches_in_lexical_order() {
        let mut paths: Vec<PathBuf> = (0..30)
            .rev()
            .map(|index| format!("report-{index:02}").into())
            .collect();
        paths.extend([
            "report-00".into(),
            "report-\u{1b}".into(),
            "Report-00".into(),
        ]);
        let span = Span::new(0, 0);
        let suggestions = path_suggestions(paths.iter().map(PathBuf::as_path), "report", span);
        assert_eq!(suggestions.len(), PATH_LIMIT);
        for (index, item) in suggestions.iter().enumerate() {
            assert_eq!(
                Path::new(&item.value),
                Path::new(&format!("report-{index:02}"))
            );
        }
    }
}
