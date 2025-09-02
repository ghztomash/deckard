#[derive(Default)]
pub struct CommandProcessor {
    pub input: String,
    pub character_index: usize,
    pub command_history_index: Option<usize>,
    max_history_len: usize,
    history: Vec<String>,
    saved_input: Option<String>,
    command_descriptions: Vec<Command>,
}

pub struct Command {
    pub command: &'static str,
    pub alias: Option<&'static str>,
}

pub struct CommandResult {
    pub name: String,
    pub args: Vec<String>,
}

impl CommandProcessor {
    pub fn new(commands: Vec<Command>, max_history_len: usize) -> Self {
        Self {
            input: String::new(),
            history: Vec::with_capacity(max_history_len),
            character_index: 0,
            command_history_index: None,
            max_history_len,
            saved_input: None,
            command_descriptions: commands,
        }
    }

    pub fn move_cursor_left(&mut self) {
        let cursor_moved_left = self.character_index.saturating_sub(1);
        self.character_index = self.clamp_cursor(cursor_moved_left);
    }

    pub fn move_cursor_right(&mut self) {
        let cursor_moved_right = self.character_index.saturating_add(1);
        self.character_index = self.clamp_cursor(cursor_moved_right);
    }

    pub fn enter_char(&mut self, new_char: char) {
        let index = self.byte_index();
        self.input.insert(index, new_char);
        self.move_cursor_right();
        self.reset_history();
    }

    pub fn byte_index(&self) -> usize {
        self.input
            .char_indices()
            .map(|(i, _)| i)
            .nth(self.character_index)
            .unwrap_or(self.input.len())
    }

    pub fn delete_char(&mut self) {
        if self.character_index != 0 {
            let current_index = self.character_index;
            let from_left_to_current_index = current_index - 1;

            let before_char_to_delete = self.input.chars().take(from_left_to_current_index);
            let after_char_to_delete = self.input.chars().skip(current_index);

            self.input = before_char_to_delete.chain(after_char_to_delete).collect();
            self.move_cursor_left();
            self.reset_history();
        }
    }

    fn clamp_cursor(&self, new_cursor_pos: usize) -> usize {
        new_cursor_pos.clamp(0, self.input.chars().count())
    }

    fn reset_cursor(&mut self) {
        self.character_index = 0;
    }

    fn append_cursor(&mut self) {
        self.character_index = self.input.chars().count();
    }

    pub fn reset_command(&mut self) {
        self.input.clear();
        self.reset_cursor();
        self.reset_history();
    }

    fn reset_history(&mut self) {
        self.command_history_index = None;
        self.saved_input = None;
    }

    pub fn last_command(&mut self) {
        if self.history.is_empty() {
            return;
        }

        if self.command_history_index.is_none() {
            self.saved_input = Some(self.input.clone());
        }

        self.command_history_index = Some(
            self.command_history_index
                .map(|i| (i + 1).min(self.history.len() - 1))
                .unwrap_or(0),
        );

        if let Some(idx) = self.command_history_index {
            self.input = self.history[idx].clone();
            self.append_cursor();
        }
    }

    pub fn next_command(&mut self) {
        if self.history.is_empty() {
            return;
        }
        match self.command_history_index {
            Some(0) => {
                // Exit history mode and restore saved input
                self.command_history_index = None;
                self.input = self.saved_input.take().unwrap_or_default();
                self.append_cursor();
            }
            Some(i) => {
                let new_index = i - 1;
                self.command_history_index = Some(new_index);
                self.input = self.history[new_index].clone();
                self.append_cursor();
            }
            None => {}
        }
    }

    pub fn submit_command(&mut self) -> Option<CommandResult> {
        let mut result = None;

        if !self.input.trim().is_empty() {
            // store command in history
            self.history.insert(0, self.input.clone());
            if self.history.len() > self.max_history_len {
                self.history.truncate(self.max_history_len);
            }

            let mut parts = self.input.split_whitespace();
            if let Some(cmd_name) = parts.next()
                && let Some(cmd) = self
                    .command_descriptions
                    .iter()
                    .find(|c| c.command == cmd_name || c.alias == Some(cmd_name))
            {
                let args: Vec<String> = parts.map(|s| s.to_string()).collect();
                result = Some(CommandResult {
                    name: cmd.command.to_string(),
                    args,
                });
            }
        }

        self.reset_command();
        result
    }
}

#[cfg(test)]
mod tests_chars {
    use super::*;
    use pretty_assertions::assert_eq;

    fn new_processor() -> CommandProcessor {
        CommandProcessor::new(vec![], 10)
    }

    #[test]
    fn test_enter_char_appends_at_cursor() {
        let mut cp = new_processor();
        cp.enter_char('h');
        cp.enter_char('e');
        cp.enter_char('l');
        cp.enter_char('l');
        cp.enter_char('o');

        assert_eq!(cp.input, "hello");
        assert_eq!(cp.character_index, 5);
    }

    #[test]
    fn test_cursor_moves_left_and_right() {
        let mut cp = new_processor();
        for c in "hello".chars() {
            cp.enter_char(c);
        }

        cp.move_cursor_left();
        cp.move_cursor_left();
        assert_eq!(cp.character_index, 3);

        cp.move_cursor_right();
        assert_eq!(cp.character_index, 4);

        cp.move_cursor_right(); // should not overflow
        cp.move_cursor_right();
        assert_eq!(cp.character_index, 5);
    }

    #[test]
    fn test_enter_char_inserts_in_middle() {
        let mut cp = new_processor();
        for c in "helo".chars() {
            cp.enter_char(c);
        }

        // Move cursor to after 'l'
        cp.move_cursor_left();
        cp.move_cursor_left();
        assert_eq!(cp.character_index, 2);
        cp.enter_char('r');

        assert_eq!(cp.input, "herlo");
        assert_eq!(cp.character_index, 3);
    }

    #[test]
    fn test_delete_char_deletes_before_cursor() {
        let mut cp = new_processor();
        for c in "hello".chars() {
            cp.enter_char(c);
        }

        // Move cursor to after 'e'
        cp.move_cursor_left();
        cp.move_cursor_left();
        cp.move_cursor_left();
        assert_eq!(cp.character_index, 2);
        cp.delete_char();

        assert_eq!(cp.input, "hllo");
        assert_eq!(cp.character_index, 1);
    }

    #[test]
    fn test_delete_char_at_start_does_nothing() {
        let mut cp = new_processor();
        for c in "hi".chars() {
            cp.enter_char(c);
        }

        cp.move_cursor_left();
        cp.move_cursor_left();
        assert_eq!(cp.character_index, 0);
        cp.delete_char();

        assert_eq!(cp.input, "hi");
        assert_eq!(cp.character_index, 0);
    }

    #[test]
    fn test_reset_command_clears_input_and_cursor() {
        let mut cp = new_processor();
        cp.input = "something".into();
        cp.character_index = 5;
        cp.command_history_index = Some(1);
        cp.saved_input = Some("saved".into());

        cp.reset_command();

        assert_eq!(cp.input, "");
        assert_eq!(cp.character_index, 0);
        assert_eq!(cp.command_history_index, None);
        assert_eq!(cp.saved_input, None);
    }

    #[test]
    fn test_byte_index_multibyte_chars() {
        let mut cp = new_processor();
        cp.input = "h🙂i".to_string(); // '🙂' is 4 bytes
        cp.character_index = 1;

        // Should be the byte index of '🙂'
        let index = cp.byte_index();
        assert_eq!(index, 1); // 'h' is one byte

        cp.character_index = 2;
        let index = cp.byte_index();
        assert_eq!(index, 5); // '🙂' is 4 bytes, so total 5
    }

    #[test]
    fn enter_chars() {
        let mut cp = CommandProcessor::new(vec![], 3);
        let chars = ['a', 'b', 'c'];

        for c in chars {
            cp.enter_char(c);
        }

        let s: String = chars.into_iter().collect();
        assert_eq!(cp.input, s);
    }

    #[test]
    fn delete_chars() {
        let mut cp = CommandProcessor::new(vec![], 3);
        let chars = ['a', 'b', 'c'];

        for c in chars {
            cp.enter_char(c);
        }

        for _ in 0..chars.len() {
            cp.delete_char();
        }

        assert!(cp.input.is_empty());

        cp.enter_char('x');
        assert_eq!(cp.input, "x".to_string());

        cp.delete_char();
        assert!(cp.input.is_empty());
    }
}

#[cfg(test)]
mod tests_command_history {
    use super::*;
    use pretty_assertions::assert_eq;

    fn create_processor_with_history(commands: &[&str]) -> CommandProcessor {
        let mut processor = CommandProcessor::new(vec![], 10);
        processor.history = commands.iter().map(|s| s.to_string()).collect();
        processor
    }

    #[test]
    fn entering_history_saves_input() {
        let mut processor = create_processor_with_history(&["cmd1", "cmd2"]);
        processor.input = "draft command".into();
        processor.character_index = processor.input.chars().count();

        processor.last_command(); // Enter history mode

        assert_eq!(processor.command_history_index, Some(0));
        assert_eq!(processor.saved_input, Some("draft command".to_string()));
        assert_eq!(processor.input, "cmd1"); // First history entry (most recent)
    }

    #[test]
    fn navigating_back_and_forth_restores_input() {
        let mut processor = create_processor_with_history(&["cmd1", "cmd2"]);
        processor.input = "temp input".into();

        // Enter history mode
        processor.last_command(); // cmd1
        processor.last_command(); // cmd2

        assert_eq!(processor.command_history_index, Some(1));
        assert_eq!(processor.input, "cmd2");

        // Exit history mode
        processor.next_command(); // back to cmd1
        assert_eq!(processor.input, "cmd1");
        processor.next_command(); // back to live input
        assert_eq!(processor.input, "temp input");
        assert_eq!(processor.command_history_index, None);
        assert_eq!(processor.saved_input, None);
    }

    #[test]
    fn submit_command_clears_saved_input_and_index() {
        let mut processor = create_processor_with_history(&[]);

        processor.input = "hello world".into();
        processor.saved_input = Some("should be cleared".into());
        processor.command_history_index = Some(0);

        processor.submit_command();

        assert_eq!(processor.input, "");
        assert_eq!(processor.command_history_index, None);
        assert_eq!(processor.saved_input, None);
        assert_eq!(processor.history.len(), 1);
        assert_eq!(processor.history[0], "hello world");
    }

    #[test]
    fn next_command_does_not_crash_without_saved_input() {
        let mut processor = create_processor_with_history(&["cmd1"]);
        processor.command_history_index = Some(0);
        processor.saved_input = None;

        processor.next_command(); // Should exit history and reset input
        assert_eq!(processor.command_history_index, None);
        assert_eq!(processor.input, "");
    }

    #[test]
    fn history_truncates_at_max_size() {
        let mut processor = CommandProcessor::new(vec![], 3);

        processor.input = "one".into();
        processor.submit_command();
        processor.input = "two".into();
        processor.submit_command();
        processor.input = "three".into();
        processor.submit_command();
        processor.input = "four".into();
        processor.submit_command();

        assert_eq!(processor.history.len(), 3);
        assert_eq!(processor.history, vec!["four", "three", "two"]);
    }

    #[test]
    fn command_history_filled() {
        let mut cp = CommandProcessor::new(vec![], 3);
        let commands = ["aaa", "bbb", "ccc"];

        for command in commands {
            for c in command.chars() {
                cp.enter_char(c);
            }
            cp.submit_command();
        }
        assert_eq!(cp.history.len(), commands.len());

        for (index, command) in commands.iter().rev().enumerate() {
            assert_eq!(cp.history[index], command.to_string());
        }
    }

    #[test]
    fn command_history_limit() {
        let max_limit = 2;
        let mut cp = CommandProcessor::new(vec![], max_limit);
        let mut commands = ["aaa", "bbb", "ccc", "ddd"];

        for command in commands {
            for c in command.chars() {
                cp.enter_char(c);
            }
            cp.submit_command();
        }
        assert_eq!(cp.history.len(), max_limit);

        commands.reverse();
        let commands = &commands[..max_limit];
        for (index, command) in commands.iter().enumerate() {
            assert_eq!(cp.history[index], command.to_string());
        }
    }

    #[test]
    fn command_history_limit_zero() {
        let mut cp = CommandProcessor::new(vec![], 0);
        let commands = ["aaa", "bbb", "ccc", "ddd"];

        for command in commands {
            for c in command.chars() {
                cp.enter_char(c);
            }
            cp.submit_command();
        }
        assert!(cp.history.is_empty());
    }

    #[test]
    fn command_history_last_restore() {
        let commands = ["a", "bb", "ccc", "dddd"];
        let mut cp = CommandProcessor::new(vec![], commands.len());

        for command in commands {
            for c in command.chars() {
                cp.enter_char(c);
            }
            cp.submit_command();
        }
        assert_eq!(cp.history.len(), commands.len());

        for command in commands.iter().rev() {
            cp.last_command();
            assert_eq!(cp.input, command.to_string());
        }

        cp.last_command();
        assert_eq!(cp.input, commands.first().unwrap().to_string());
    }

    #[test]
    fn command_history_next_restore() {
        let commands = ["a", "bb", "ccc", "dddd"];
        let mut cp = CommandProcessor::new(vec![], commands.len());

        for command in commands {
            for c in command.chars() {
                cp.enter_char(c);
            }
            cp.submit_command();
        }
        assert_eq!(cp.history.len(), commands.len());

        for _ in 0..commands.len() {
            cp.last_command();
        }
        assert_eq!(cp.input, commands.first().unwrap().to_string());
        cp.last_command();
        assert_eq!(cp.input, commands.first().unwrap().to_string());

        for command in &commands[1..] {
            cp.next_command();
            assert_eq!(cp.input, command.to_string());
        }

        cp.next_command();
        assert_eq!(cp.input, "".to_string());
    }
}

#[cfg(test)]
mod tests_command_processing {
    use super::*;
    use pretty_assertions::assert_eq;

    fn create_processor_with_commands() -> CommandProcessor {
        let commands = vec![
            Command {
                command: "test",
                alias: None,
            },
            Command {
                command: "run",
                alias: None,
            },
            Command {
                command: "quit",
                alias: Some("q"),
            },
        ];
        CommandProcessor::new(commands, 5)
    }

    #[test]
    fn test_submit_command() {
        let mut cp = create_processor_with_commands();

        cp.input = "test arg1 arg2".into();
        let result = cp.submit_command().unwrap();

        // Command should be in history
        assert_eq!(cp.history.len(), 1);
        assert_eq!(cp.history[0], "test arg1 arg2");

        // Input should be cleared
        assert_eq!(cp.input, "");

        // Cursor should be reset
        assert_eq!(cp.character_index, 0);

        assert_eq!(result.name, "test");
        assert_eq!(result.args, vec!["arg1", "arg2"]);
    }

    #[test]
    fn test_submit_ignores_unknown_command() {
        let mut cp = create_processor_with_commands();

        cp.input = "unknown cmd".into();
        let result = cp.submit_command();

        // History still stores the input
        assert_eq!(cp.history.len(), 1);
        assert_eq!(cp.history[0], "unknown cmd");

        assert!(result.is_none());
    }

    #[test]
    fn test_submit_empty_input_does_nothing() {
        let mut cp = create_processor_with_commands();

        cp.input = "   ".into();
        let result = cp.submit_command();

        assert!(cp.history.is_empty());
        assert!(result.is_none());
    }

    #[test]
    fn test_submit_multiple_commands() {
        let mut cp = create_processor_with_commands();

        cp.input = "test arg1 arg2".into();
        let res1 = cp.submit_command().unwrap();
        cp.input = "quit".into();
        let res2 = cp.submit_command().unwrap();

        assert_eq!(res1.name, "test");
        assert_eq!(res1.args, vec!["arg1", "arg2"]);
        assert_eq!(res2.name, "quit");
        assert!(res2.args.is_empty());
    }

    #[test]
    fn test_submit_alias_command() {
        let mut cp = create_processor_with_commands();

        cp.input = "q".into();
        let res = cp.submit_command().unwrap();

        assert_eq!(res.name, "quit");
        assert!(res.args.is_empty());
    }
}
