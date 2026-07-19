use std::{
    env, fs,
    io::{self, stdout},
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Context, Result};
use chrono::{Local, NaiveDate};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const DEFAULT_MESSAGE: &str =
    "j/k: select  J/K: reorder  h/l: depth  v: descendants  a/e/d: edit  ?: help";
const MAX_DESCENDANT_DEPTH: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Priority {
    High,
    Medium,
    Low,
    None,
}

impl Priority {
    fn label(self) -> &'static str {
        match self {
            Self::High => "P1",
            Self::Medium => "P2",
            Self::Low => "P3",
            Self::None => "--",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::None => Self::High,
            Self::High => Self::Medium,
            Self::Medium => Self::Low,
            Self::Low => Self::None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Todo {
    checked: bool,
    text: String,
    depth: usize,
    priority: Priority,
    due: Option<NaiveDate>,
    saved: bool,
    category: Option<String>,
    source: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MarkdownLine {
    /// Number of TODOs that appeared before this line in the source file.
    before_todo: usize,
    text: String,
    source: Option<PathBuf>,
}

#[derive(Clone, Copy)]
struct DocumentRef<'a> {
    todos: &'a [Todo],
    markdown: &'a [MarkdownLine],
}

#[derive(Clone, Copy)]
struct TodoListView<'a> {
    selected: usize,
    active: bool,
    search_query: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    modified: Option<SystemTime>,
    len: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    Local,
    Global,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MessageKind {
    Default,
    Success,
    Warning,
}

#[derive(Debug, PartialEq, Eq)]
enum InputMode {
    Normal,
    Add,
    Edit,
    Due,
    Category,
    Search,
    ConfirmDelete,
    ConfirmBulkDelete,
    Help,
    AncestorPicker,
}

struct App {
    todos: Vec<Todo>,
    other_todos: Vec<Todo>,
    markdown: Vec<MarkdownLine>,
    other_markdown: Vec<MarkdownLine>,
    selected: usize,
    other_selected: usize,
    local_path: PathBuf,
    global_path: PathBuf,
    scope: Scope,
    message: String,
    message_kind: MessageKind,
    message_until: Option<Instant>,
    input: String,
    input_mode: InputMode,
    source_pane_id: Option<String>,
    last_cwd_check: Instant,
    pending_g: bool,
    undo_stack: Vec<Vec<Todo>>,
    other_undo_stack: Vec<Vec<Todo>>,
    last_saved_todos: Vec<Todo>,
    other_last_saved_todos: Vec<Todo>,
    search_query: Option<String>,
    other_search_query: Option<String>,
    file_stamp: Option<FileStamp>,
    other_file_stamp: Option<FileStamp>,
    last_file_check: Instant,
    local_root: PathBuf,
    ancestor_candidates: Vec<(PathBuf, bool)>,
    ancestor_selected: usize,
    descendants_expanded: bool,
}

impl App {
    fn new(local_path: PathBuf, global_path: PathBuf) -> Result<Self> {
        ensure_todo_file(&global_path)?;
        let local_root = local_path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let descendants_expanded = true;
        let (todos, markdown) = load_local_view(&local_root, &local_path, descendants_expanded)?;
        let (other_todos, other_markdown) = load_document(&global_path)?;
        let local_stamp = local_view_stamp(&local_root, &local_path, descendants_expanded);
        let global_stamp = file_stamp(&global_path);
        Ok(Self {
            last_saved_todos: todos.clone(),
            other_last_saved_todos: other_todos.clone(),
            todos,
            other_todos,
            markdown,
            other_markdown,
            selected: 0,
            other_selected: 0,
            local_path,
            global_path,
            scope: Scope::Local,
            message: DEFAULT_MESSAGE.to_string(),
            message_kind: MessageKind::Default,
            message_until: None,
            input: String::new(),
            input_mode: InputMode::Normal,
            source_pane_id: env::var("HERDR_TODO_SOURCE_PANE_ID").ok(),
            last_cwd_check: Instant::now(),
            pending_g: false,
            undo_stack: Vec::new(),
            other_undo_stack: Vec::new(),
            search_query: None,
            other_search_query: None,
            file_stamp: local_stamp,
            other_file_stamp: global_stamp,
            last_file_check: Instant::now(),
            local_root,
            ancestor_candidates: Vec::new(),
            ancestor_selected: 0,
            descendants_expanded,
        })
    }

    fn path(&self) -> &Path {
        match self.scope {
            Scope::Local => &self.local_path,
            Scope::Global => &self.global_path,
        }
    }

    fn other_path(&self) -> &Path {
        match self.scope {
            Scope::Local => &self.global_path,
            Scope::Global => &self.local_path,
        }
    }

    fn scope_name(&self) -> &'static str {
        match self.scope {
            Scope::Local => "Local",
            Scope::Global => "Global",
        }
    }

    fn move_down(&mut self) {
        if let Some(index) = ((self.selected + 1)..self.todos.len())
            .find(|index| todo_matches_search(&self.todos[*index], self.search_query.as_deref()))
        {
            self.selected = index;
        }
    }

    fn move_up(&mut self) {
        if let Some(index) = (0..self.selected)
            .rev()
            .find(|index| todo_matches_search(&self.todos[*index], self.search_query.as_deref()))
        {
            self.selected = index;
        }
    }

    fn normalize_selection(&mut self) {
        if self.todos.is_empty() {
            self.selected = 0;
            return;
        }
        if self.selected >= self.todos.len()
            || !todo_matches_search(&self.todos[self.selected], self.search_query.as_deref())
        {
            self.selected = self
                .todos
                .iter()
                .position(|todo| todo_matches_search(todo, self.search_query.as_deref()))
                .unwrap_or(0);
        }
    }

    fn reorder_selected(&mut self, down: bool) -> Result<()> {
        let mut reordered = self.todos.clone();
        let Some(new_selected) = reorder_todo_block(&mut reordered, self.selected, down) else {
            self.set_message("No sibling TODO in that direction", MessageKind::Warning);
            return Ok(());
        };
        if !sources_are_contiguous(&reordered) {
            self.set_message(
                "TODOs cannot be reordered across files",
                MessageKind::Warning,
            );
            return Ok(());
        }
        self.todos = reordered;
        self.selected = new_selected;
        self.save()?;
        self.set_message("TODO order updated", MessageKind::Success);
        Ok(())
    }

    fn toggle_scope(&mut self) -> Result<()> {
        std::mem::swap(&mut self.todos, &mut self.other_todos);
        std::mem::swap(&mut self.markdown, &mut self.other_markdown);
        std::mem::swap(&mut self.selected, &mut self.other_selected);
        std::mem::swap(&mut self.undo_stack, &mut self.other_undo_stack);
        std::mem::swap(&mut self.last_saved_todos, &mut self.other_last_saved_todos);
        std::mem::swap(&mut self.search_query, &mut self.other_search_query);
        std::mem::swap(&mut self.file_stamp, &mut self.other_file_stamp);
        self.scope = match self.scope {
            Scope::Local => Scope::Global,
            Scope::Global => Scope::Local,
        };
        self.set_message(
            format!("{} pane selected", self.scope_name()),
            MessageKind::Success,
        );
        Ok(())
    }

    fn toggle_selected(&mut self) -> Result<()> {
        let Some(todo) = self.todos.get_mut(self.selected) else {
            self.set_message("No TODO selected", MessageKind::Warning);
            return Ok(());
        };
        todo.checked = !todo.checked;
        self.save()?;
        self.set_message("TODO updated", MessageKind::Success);
        Ok(())
    }

    fn start_add(&mut self) {
        if self.scope == Scope::Local && !self.local_path.exists() {
            self.set_message(
                "Local TODO.md does not exist; press Shift+C to create it",
                MessageKind::Warning,
            );
            return;
        }
        self.input.clear();
        self.input_mode = InputMode::Add;
        self.set_persistent_message("New TODO: Enter to save, Esc to cancel");
    }

    fn start_ancestor_picker(&mut self) -> Result<()> {
        if self.scope != Scope::Local {
            self.set_message(
                "Switch to Local before configuring ancestors",
                MessageKind::Warning,
            );
            return Ok(());
        }
        let enabled = read_shared_ancestors(&self.local_root)?;
        self.ancestor_candidates = ancestor_todo_paths(&self.local_root)
            .into_iter()
            .map(|path| {
                let selected = enabled.contains(&path);
                (path, selected)
            })
            .collect();
        self.ancestor_selected = 0;
        self.input_mode = InputMode::AncestorPicker;
        self.set_persistent_message("Space: toggle  Enter: save  Esc: cancel");
        Ok(())
    }

    fn save_ancestor_picker(&mut self) -> Result<()> {
        let selected = self
            .ancestor_candidates
            .iter()
            .filter_map(|(path, selected)| selected.then_some(path.clone()))
            .collect::<Vec<_>>();
        write_shared_ancestors(&self.local_root, &selected)?;
        self.input_mode = InputMode::Normal;
        self.reload_local()?;
        self.set_message("Shared ancestor TODOs updated", MessageKind::Success);
        Ok(())
    }

    fn toggle_descendants(&mut self) -> Result<()> {
        if self.scope != Scope::Local {
            self.set_message(
                "Switch to Local before toggling descendants",
                MessageKind::Warning,
            );
            return Ok(());
        }
        self.descendants_expanded = !self.descendants_expanded;
        self.reload_local()?;
        self.file_stamp = local_view_stamp(
            &self.local_root,
            &self.local_path,
            self.descendants_expanded,
        );
        self.set_message(
            if self.descendants_expanded {
                "Descendant TODOs expanded (up to 3 levels)"
            } else {
                "Descendant TODOs collapsed"
            },
            MessageKind::Success,
        );
        Ok(())
    }

    fn start_edit(&mut self) {
        let Some(todo) = self.todos.get(self.selected) else {
            self.set_message("No TODO selected", MessageKind::Warning);
            return;
        };
        self.input = todo.text.clone();
        self.input_mode = InputMode::Edit;
        self.set_persistent_message("Edit TODO: Enter to save, Esc to cancel");
    }

    fn start_category(&mut self) {
        let Some(todo) = self.todos.get(self.selected) else {
            self.set_message("No TODO selected", MessageKind::Warning);
            return;
        };
        self.input = todo.category.clone().unwrap_or_default();
        self.input_mode = InputMode::Category;
        self.set_persistent_message("Category: Enter to save, empty to clear, Esc to cancel");
    }

    fn start_search(&mut self) {
        self.input = self.search_query.clone().unwrap_or_default();
        self.input_mode = InputMode::Search;
        self.set_persistent_message(
            "Search text/category: Enter to apply, empty to clear, Esc to cancel",
        );
    }

    fn start_due(&mut self) {
        let Some(todo) = self.todos.get(self.selected) else {
            self.set_message("No TODO selected", MessageKind::Warning);
            return;
        };
        self.input = todo.due.map(|date| date.to_string()).unwrap_or_default();
        self.input_mode = InputMode::Due;
        self.set_persistent_message("Due date: YYYY-MM-DD (empty clears it)");
    }

    fn start_delete(&mut self) {
        if self.todos.is_empty() {
            self.set_message("No TODO selected", MessageKind::Warning);
            return;
        }
        self.input_mode = InputMode::ConfirmDelete;
        self.message = "Delete selected TODO and its children? y: yes  n/Esc: cancel".into();
        self.message_kind = MessageKind::Warning;
        self.message_until = None;
    }

    fn create_local_file(&mut self) -> Result<()> {
        if self.local_path.exists() {
            self.set_message("Local TODO.md already exists", MessageKind::Warning);
            return Ok(());
        }
        ensure_todo_file(&self.local_path)?;
        let (todos, markdown) = load_local_view(
            &self.local_root,
            &self.local_path,
            self.descendants_expanded,
        )?;
        let stamp = local_view_stamp(
            &self.local_root,
            &self.local_path,
            self.descendants_expanded,
        );
        if self.scope == Scope::Local {
            self.todos = todos;
            self.markdown = markdown;
            self.last_saved_todos = self.todos.clone();
            self.file_stamp = stamp;
            self.selected = 0;
        } else {
            self.other_todos = todos;
            self.other_markdown = markdown;
            self.other_last_saved_todos = self.other_todos.clone();
            self.other_file_stamp = stamp;
            self.other_selected = 0;
        }
        self.set_message("Created Local TODO.md", MessageKind::Success);
        Ok(())
    }

    fn start_bulk_delete(&mut self) {
        let count = self.todos.iter().filter(|todo| todo.checked).count();
        if count == 0 {
            self.set_message("No completed TODOs", MessageKind::Warning);
            return;
        }
        self.input_mode = InputMode::ConfirmBulkDelete;
        self.message =
            format!("Delete {count} completed TODO(s) and their children? y: yes  n/Esc: cancel");
        self.message_kind = MessageKind::Warning;
        self.message_until = None;
    }

    fn submit_input(&mut self) -> Result<()> {
        let text = self.input.trim().to_string();
        match self.input_mode {
            InputMode::Add => {
                if text.is_empty() {
                    self.set_message("TODO text cannot be empty", MessageKind::Warning);
                    return Ok(());
                }
                let source = (self.scope == Scope::Local).then(|| self.local_path.clone());
                let insert_at = if let Some(source) = source.as_ref() {
                    self.todos
                        .iter()
                        .rposition(|todo| todo.source.as_ref() == Some(source))
                        .map(|index| index + 1)
                        .unwrap_or_else(|| {
                            self.todos
                                .iter()
                                .position(|todo| {
                                    todo.source.as_ref().is_some_and(|path| path > source)
                                })
                                .unwrap_or(self.todos.len())
                        })
                } else {
                    self.todos.len()
                };
                let depth = insert_at
                    .checked_sub(1)
                    .and_then(|index| self.todos.get(index))
                    .filter(|todo| todo.source == source)
                    .map_or(0, |todo| todo.depth);
                self.todos.insert(
                    insert_at,
                    Todo {
                        checked: false,
                        text,
                        depth,
                        priority: Priority::None,
                        due: None,
                        saved: false,
                        category: None,
                        source,
                    },
                );
                self.selected = insert_at;
            }
            InputMode::Edit => {
                if text.is_empty() {
                    self.set_message("TODO text cannot be empty", MessageKind::Warning);
                    return Ok(());
                }
                if let Some(todo) = self.todos.get_mut(self.selected) {
                    todo.text = text;
                }
            }
            InputMode::Due => {
                let due = if text.is_empty() {
                    None
                } else {
                    match NaiveDate::parse_from_str(&text, "%Y-%m-%d") {
                        Ok(date) => Some(date),
                        Err(_) => {
                            self.set_message("Use a valid date: YYYY-MM-DD", MessageKind::Warning);
                            return Ok(());
                        }
                    }
                };
                if let Some(todo) = self.todos.get_mut(self.selected) {
                    todo.due = due;
                }
            }
            InputMode::Category => {
                if text.contains(']') || text.contains(['\n', '\r']) {
                    self.set_message(
                        "Category cannot contain ] or a newline",
                        MessageKind::Warning,
                    );
                    return Ok(());
                }
                if let Some(todo) = self.todos.get_mut(self.selected) {
                    todo.category = (!text.is_empty()).then_some(text);
                }
            }
            InputMode::Search => {
                self.search_query = (!text.is_empty()).then_some(text);
                if !self
                    .todos
                    .get(self.selected)
                    .is_some_and(|todo| todo_matches_search(todo, self.search_query.as_deref()))
                {
                    self.selected = self
                        .todos
                        .iter()
                        .position(|todo| todo_matches_search(todo, self.search_query.as_deref()))
                        .unwrap_or(0);
                }
                self.input.clear();
                self.input_mode = InputMode::Normal;
                let message = self
                    .search_query
                    .as_ref()
                    .map(|query| format!("Search applied: {query}"))
                    .unwrap_or_else(|| "Search cleared".to_string());
                self.set_message(message, MessageKind::Success);
                return Ok(());
            }
            _ => return Ok(()),
        }
        self.input.clear();
        self.input_mode = InputMode::Normal;
        self.save()?;
        self.set_message("TODO saved", MessageKind::Success);
        Ok(())
    }

    fn delete_selected(&mut self) -> Result<()> {
        if self.todos.is_empty() {
            self.input_mode = InputMode::Normal;
            return Ok(());
        }
        let depth = self.todos[self.selected].depth;
        let mut end = self.selected + 1;
        let source = self.todos[self.selected].source.clone();
        while end < self.todos.len()
            && self.todos[end].source == source
            && self.todos[end].depth > depth
        {
            end += 1;
        }
        self.todos.drain(self.selected..end);
        self.selected = self.selected.min(self.todos.len().saturating_sub(1));
        self.input_mode = InputMode::Normal;
        self.save()?;
        self.set_message("TODO deleted", MessageKind::Success);
        Ok(())
    }

    fn delete_completed(&mut self) -> Result<()> {
        let before = self.todos.len();
        let mut retained = Vec::with_capacity(before);
        let mut skipped_depth = None;
        let mut skipped_source = None;
        for todo in self.todos.drain(..) {
            if skipped_source != todo.source {
                skipped_depth = None;
            }
            if skipped_depth.is_some_and(|depth| todo.depth > depth) {
                continue;
            }
            skipped_depth = None;
            if todo.checked {
                skipped_depth = Some(todo.depth);
                skipped_source = todo.source.clone();
            } else {
                retained.push(todo);
            }
        }
        self.todos = retained;
        let deleted = before - self.todos.len();
        self.selected = self.selected.min(self.todos.len().saturating_sub(1));
        self.input_mode = InputMode::Normal;
        self.save()?;
        self.set_message(
            format!("Deleted {deleted} completed TODO(s)"),
            MessageKind::Success,
        );
        Ok(())
    }

    fn change_depth(&mut self, indent: bool) -> Result<()> {
        if self.todos.is_empty() {
            return Ok(());
        }
        let new_depth = if indent {
            if self.selected == 0
                || self.todos[self.selected - 1].source != self.todos[self.selected].source
            {
                return Ok(());
            }
            let max_depth = self.todos[self.selected - 1].depth + 1;
            (self.todos[self.selected].depth + 1).min(max_depth)
        } else {
            self.todos[self.selected].depth.saturating_sub(1)
        };
        let old_depth = self.todos[self.selected].depth;
        let difference = new_depth as isize - old_depth as isize;
        let mut end = self.selected + 1;
        let source = self.todos[self.selected].source.clone();
        while end < self.todos.len()
            && self.todos[end].source == source
            && self.todos[end].depth > old_depth
        {
            end += 1;
        }
        for todo in &mut self.todos[self.selected..end] {
            todo.depth = (todo.depth as isize + difference).max(0) as usize;
        }
        self.save()?;
        self.set_message("Hierarchy updated", MessageKind::Success);
        Ok(())
    }

    fn cycle_priority(&mut self) -> Result<()> {
        let Some(todo) = self.todos.get_mut(self.selected) else {
            return Ok(());
        };
        todo.priority = todo.priority.next();
        self.save()?;
        self.set_message("Priority updated (s: sort)", MessageKind::Success);
        Ok(())
    }

    fn toggle_saved(&mut self) -> Result<()> {
        let Some(todo) = self.todos.get_mut(self.selected) else {
            return Ok(());
        };
        todo.saved = !todo.saved;
        let saved = todo.saved;
        self.save()?;
        self.set_message(
            if saved {
                "SAVE protection enabled"
            } else {
                "SAVE protection disabled"
            },
            MessageKind::Success,
        );
        Ok(())
    }

    fn sort_by_priority(&mut self) -> Result<()> {
        for_each_source(&mut self.todos, sort_siblings);
        self.selected = self.selected.min(self.todos.len().saturating_sub(1));
        self.save()?;
        self.set_message("Sorted by priority and due date", MessageKind::Success);
        Ok(())
    }

    fn group_by_category(&mut self) -> Result<()> {
        for_each_source(&mut self.todos, group_categories);
        self.selected = self.selected.min(self.todos.len().saturating_sub(1));
        self.save()?;
        self.set_message("Grouped by category priority", MessageKind::Success);
        Ok(())
    }

    fn undo(&mut self) -> Result<()> {
        let Some(previous) = self.undo_stack.pop() else {
            self.set_message("Nothing to undo", MessageKind::Warning);
            return Ok(());
        };
        self.todos = previous;
        self.last_saved_todos = self.todos.clone();
        self.normalize_selection();
        if self.scope == Scope::Local {
            save_local_documents(&self.todos, &self.markdown)?;
            self.file_stamp = local_view_stamp(
                &self.local_root,
                &self.local_path,
                self.descendants_expanded,
            );
        } else {
            save_document(self.path(), &self.todos, &self.markdown)?;
            self.file_stamp = file_stamp(self.path());
        }
        self.set_message("Undid last change", MessageKind::Success);
        Ok(())
    }

    fn cancel_input(&mut self) {
        self.input.clear();
        self.input_mode = InputMode::Normal;
        self.set_message("Cancelled", MessageKind::Warning);
    }

    fn reload(&mut self) -> Result<()> {
        if self.scope == Scope::Local {
            return self.reload_local();
        }
        let (todos, markdown) = load_document(self.path())?;
        self.todos = todos;
        self.markdown = markdown;
        self.last_saved_todos = self.todos.clone();
        self.undo_stack.clear();
        self.file_stamp = if self.scope == Scope::Local {
            local_view_stamp(
                &self.local_root,
                &self.local_path,
                self.descendants_expanded,
            )
        } else {
            file_stamp(self.path())
        };
        self.normalize_selection();
        Ok(())
    }

    fn reload_local(&mut self) -> Result<()> {
        let (todos, markdown) = load_local_view(
            &self.local_root,
            &self.local_path,
            self.descendants_expanded,
        )?;
        if self.scope == Scope::Local {
            self.todos = todos;
            self.markdown = markdown;
            self.last_saved_todos = self.todos.clone();
            self.undo_stack.clear();
            self.normalize_selection();
        } else {
            self.other_todos = todos;
            self.other_markdown = markdown;
            self.other_last_saved_todos = self.other_todos.clone();
            self.other_undo_stack.clear();
            self.other_selected = self
                .other_selected
                .min(self.other_todos.len().saturating_sub(1));
        }
        Ok(())
    }

    fn save(&mut self) -> Result<()> {
        self.normalize_selection();
        if self.todos != self.last_saved_todos {
            self.undo_stack.push(self.last_saved_todos.clone());
            if self.undo_stack.len() > 100 {
                self.undo_stack.remove(0);
            }
            self.last_saved_todos = self.todos.clone();
        }
        if self.scope == Scope::Local {
            save_local_documents(&self.todos, &self.markdown)?;
        } else {
            save_document(self.path(), &self.todos, &self.markdown)?;
        }
        self.file_stamp = if self.scope == Scope::Local {
            local_view_stamp(
                &self.local_root,
                &self.local_path,
                self.descendants_expanded,
            )
        } else {
            file_stamp(self.path())
        };
        Ok(())
    }

    fn set_message(&mut self, message: impl Into<String>, kind: MessageKind) {
        self.message = message.into();
        self.message_kind = kind;
        self.message_until = Some(Instant::now() + Duration::from_secs(2));
    }

    fn set_persistent_message(&mut self, message: impl Into<String>) {
        self.message = message.into();
        self.message_kind = MessageKind::Default;
        self.message_until = None;
    }

    fn update_message_timeout(&mut self) {
        if self
            .message_until
            .is_some_and(|until| Instant::now() >= until)
        {
            self.message = DEFAULT_MESSAGE.into();
            self.message_kind = MessageKind::Default;
            self.message_until = None;
        }
    }

    fn refresh_changed_files(&mut self) -> Result<()> {
        if self.input_mode != InputMode::Normal
            || self.last_file_check.elapsed() < Duration::from_millis(500)
        {
            return Ok(());
        }
        self.last_file_check = Instant::now();
        let mut refreshed = false;

        let current_stamp = if self.scope == Scope::Local {
            local_view_stamp(
                &self.local_root,
                &self.local_path,
                self.descendants_expanded,
            )
        } else {
            file_stamp(self.path())
        };
        if current_stamp != self.file_stamp {
            let path = self.path().to_path_buf();
            if self.scope == Scope::Local {
                (self.todos, self.markdown) = load_local_view(
                    &self.local_root,
                    &self.local_path,
                    self.descendants_expanded,
                )?;
            } else {
                (self.todos, self.markdown) = load_document(&path)?;
            }
            self.last_saved_todos = self.todos.clone();
            self.undo_stack.clear();
            self.file_stamp = if self.scope == Scope::Local {
                local_view_stamp(
                    &self.local_root,
                    &self.local_path,
                    self.descendants_expanded,
                )
            } else {
                file_stamp(&path)
            };
            self.normalize_selection();
            refreshed = true;
        }

        let other_stamp = if self.scope == Scope::Global {
            local_view_stamp(
                &self.local_root,
                &self.local_path,
                self.descendants_expanded,
            )
        } else {
            file_stamp(self.other_path())
        };
        if other_stamp != self.other_file_stamp {
            let path = self.other_path().to_path_buf();
            if self.scope == Scope::Global {
                (self.other_todos, self.other_markdown) = load_local_view(
                    &self.local_root,
                    &self.local_path,
                    self.descendants_expanded,
                )?;
            } else {
                (self.other_todos, self.other_markdown) = load_document(&path)?;
            }
            self.other_last_saved_todos = self.other_todos.clone();
            self.other_undo_stack.clear();
            self.other_file_stamp = if self.scope == Scope::Global {
                local_view_stamp(
                    &self.local_root,
                    &self.local_path,
                    self.descendants_expanded,
                )
            } else {
                file_stamp(&path)
            };
            self.other_selected = self
                .other_selected
                .min(self.other_todos.len().saturating_sub(1));
            refreshed = true;
        }

        if refreshed {
            self.set_message("TODO files updated automatically", MessageKind::Success);
        }
        Ok(())
    }

    fn update_local_cwd(&mut self) -> Result<()> {
        if self.input_mode != InputMode::Normal
            || self.last_cwd_check.elapsed() < Duration::from_secs(1)
        {
            return Ok(());
        }
        self.last_cwd_check = Instant::now();
        let Some(source_pane_id) = self.source_pane_id.as_deref() else {
            return Ok(());
        };
        let herdr = env::var_os("HERDR_BIN_PATH").unwrap_or_else(|| "herdr".into());
        let Ok(output) = Command::new(herdr)
            .args(["pane", "process-info", "--pane", source_pane_id])
            .output()
        else {
            return Ok(());
        };
        if !output.status.success() {
            return Ok(());
        }
        let Some(cwd) = parse_foreground_cwd(&output.stdout) else {
            return Ok(());
        };
        let new_path = cwd.join("TODO.md");
        if new_path == self.local_path {
            return Ok(());
        }
        let (todos, markdown) = load_local_view(&cwd, &new_path, self.descendants_expanded)?;
        let new_stamp = local_view_stamp(&cwd, &new_path, self.descendants_expanded);
        self.local_path = new_path;
        self.local_root = cwd.clone();
        if self.scope == Scope::Local {
            self.todos = todos;
            self.markdown = markdown;
            self.last_saved_todos = self.todos.clone();
            self.undo_stack.clear();
            self.file_stamp = new_stamp;
            self.selected = self.selected.min(self.todos.len().saturating_sub(1));
        } else {
            self.other_todos = todos;
            self.other_markdown = markdown;
            self.other_last_saved_todos = self.other_todos.clone();
            self.other_undo_stack.clear();
            self.other_file_stamp = new_stamp;
            self.other_selected = self
                .other_selected
                .min(self.other_todos.len().saturating_sub(1));
        }
        self.set_message(
            format!("Local TODO changed to {}", cwd.display()),
            MessageKind::Success,
        );
        Ok(())
    }
}

fn parse_foreground_cwd(output: &[u8]) -> Option<PathBuf> {
    serde_json::from_slice::<serde_json::Value>(output)
        .ok()?
        .pointer("/result/process_info/foreground_processes/0/cwd")?
        .as_str()
        .map(PathBuf::from)
}

fn ensure_todo_file(path: &Path) -> Result<()> {
    if !path.exists() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::write(path, "# TODO\n\n")
            .with_context(|| format!("failed to create {}", path.display()))?;
    }
    Ok(())
}

fn file_stamp(path: &Path) -> Option<FileStamp> {
    let metadata = fs::metadata(path).ok()?;
    Some(FileStamp {
        modified: metadata.modified().ok(),
        len: metadata.len(),
    })
}

fn local_tree_stamp(root: &Path, include_descendants: bool) -> Option<FileStamp> {
    let paths = local_todo_paths(root, include_descendants).ok()?;
    let mut modified = None;
    let mut len = 0_u64;
    for path in paths {
        let Some(stamp) = file_stamp(&path) else {
            continue;
        };
        modified = match (modified, stamp.modified) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (None, value) | (value, None) => value,
        };
        len = len.saturating_add(stamp.len);
    }
    Some(FileStamp { modified, len })
}

fn local_view_stamp(root: &Path, primary: &Path, include_descendants: bool) -> Option<FileStamp> {
    let tree = local_tree_stamp(root, include_descendants);
    if primary.file_name().is_none_or(|name| name == "TODO.md") {
        return tree;
    }
    match (tree, file_stamp(primary)) {
        (Some(left), Some(right)) => Some(FileStamp {
            modified: match (left.modified, right.modified) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (None, value) | (value, None) => value,
            },
            len: left.len.saturating_add(right.len),
        }),
        (value, None) | (None, value) => value,
    }
}

fn load_document(path: &Path) -> Result<(Vec<Todo>, Vec<MarkdownLine>)> {
    if !path.exists() {
        return Ok((Vec::new(), Vec::new()));
    }
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut todos = Vec::new();
    let mut markdown = Vec::new();
    for line in content.lines() {
        if let Some(todo) = parse_todo_line(line) {
            todos.push(todo);
        } else if !line.trim().is_empty()
            && let Some(todo) = todos.last_mut()
            && line.len() - line.trim_start_matches([' ', '\t']).len() >= todo.depth * 2 + 6
        {
            todo.text.push('\n');
            todo.text.push_str(line.trim());
        } else {
            markdown.push(MarkdownLine {
                before_todo: todos.len(),
                text: line.to_string(),
                source: None,
            });
        }
    }
    if remove_expired(&mut todos, Local::now().date_naive()) > 0 {
        save_document(path, &todos, &markdown)?;
    }
    Ok((todos, markdown))
}

fn config_path(root: &Path) -> PathBuf {
    root.join(".herdr-todo.toml")
}

fn read_shared_ancestors(root: &Path) -> Result<Vec<PathBuf>> {
    let path = config_path(root);
    let Ok(content) = fs::read_to_string(&path) else {
        return Ok(Vec::new());
    };
    Ok(content
        .lines()
        .filter_map(|line| {
            let value = line.trim().strip_suffix(',')?.trim();
            let value = value.strip_prefix('"')?.strip_suffix('"')?;
            Some(root.join(value))
        })
        .collect())
}

fn write_shared_ancestors(root: &Path, paths: &[PathBuf]) -> Result<()> {
    let mut output = String::from("# Managed by Herdr TODO\nshared_ancestors = [\n");
    for path in paths {
        let relative = path
            .strip_prefix(root)
            .map(Path::to_path_buf)
            .unwrap_or_else(|_| path.clone());
        output.push_str(&format!("  \"{}\",\n", relative.display()));
    }
    output.push_str("]\n");
    fs::write(config_path(root), output).context("failed to save .herdr-todo.toml")
}

fn ancestor_todo_paths(root: &Path) -> Vec<PathBuf> {
    root.ancestors()
        .skip(1)
        .map(|dir| dir.join("TODO.md"))
        .filter(|path| path.is_file())
        .collect()
}

fn local_todo_paths(root: &Path, include_descendants: bool) -> Result<Vec<PathBuf>> {
    let mut paths = read_shared_ancestors(root)?;
    if include_descendants {
        collect_descendant_todos(root, 0, &mut paths)?;
    } else {
        let primary = root.join("TODO.md");
        if primary.is_file() {
            paths.push(primary);
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn collect_descendant_todos(dir: &Path, depth: usize, paths: &mut Vec<PathBuf>) -> Result<()> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if !matches!(
                entry.file_name().to_str(),
                Some(".git" | "target" | "node_modules")
            ) && depth < MAX_DESCENDANT_DEPTH
            {
                collect_descendant_todos(&path, depth + 1, paths)?;
            }
        } else if file_type.is_file() && entry.file_name() == "TODO.md" {
            paths.push(path);
        }
    }
    Ok(())
}

fn load_local_documents(
    root: &Path,
    include_descendants: bool,
) -> Result<(Vec<Todo>, Vec<MarkdownLine>)> {
    let mut all_todos = Vec::new();
    let mut all_markdown = Vec::new();
    for path in local_todo_paths(root, include_descendants)? {
        append_document(&path, &mut all_todos, &mut all_markdown)?;
    }
    Ok((all_todos, all_markdown))
}

fn load_local_view(
    root: &Path,
    primary: &Path,
    include_descendants: bool,
) -> Result<(Vec<Todo>, Vec<MarkdownLine>)> {
    let (mut todos, mut markdown) = load_local_documents(root, include_descendants)?;
    if primary.file_name().is_some_and(|name| name != "TODO.md") && primary.exists() {
        append_document(primary, &mut todos, &mut markdown)?;
    }
    Ok((todos, markdown))
}

fn append_document(
    path: &Path,
    all_todos: &mut Vec<Todo>,
    all_markdown: &mut Vec<MarkdownLine>,
) -> Result<()> {
    let (mut todos, mut markdown) = load_document(path)?;
    for todo in &mut todos {
        todo.source = Some(path.to_path_buf());
    }
    for line in &mut markdown {
        line.source = Some(path.to_path_buf());
    }
    all_todos.extend(todos);
    all_markdown.extend(markdown);
    Ok(())
}

fn save_local_documents(todos: &[Todo], markdown: &[MarkdownLine]) -> Result<()> {
    let mut paths = todos
        .iter()
        .filter_map(|todo| todo.source.clone())
        .chain(markdown.iter().filter_map(|line| line.source.clone()))
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    for path in paths {
        let file_todos = todos
            .iter()
            .filter(|todo| todo.source.as_deref() == Some(path.as_path()))
            .cloned()
            .collect::<Vec<_>>();
        let file_markdown = markdown
            .iter()
            .filter(|line| line.source.as_deref() == Some(path.as_path()))
            .cloned()
            .collect::<Vec<_>>();
        save_document(&path, &file_todos, &file_markdown)?;
    }
    Ok(())
}

fn parse_todo_line(line: &str) -> Option<Todo> {
    let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
    let depth = indent / 2;
    let trimmed = line.trim_start();
    let (checked, rest) = if let Some(text) = trimmed.strip_prefix("- [ ] ") {
        (false, text)
    } else {
        let text = trimmed
            .strip_prefix("- [x] ")
            .or_else(|| trimmed.strip_prefix("- [X] "))?;
        (true, text)
    };

    let (priority, rest) = if let Some(rest) = rest.strip_prefix("[P1] ") {
        (Priority::High, rest)
    } else if let Some(rest) = rest.strip_prefix("[P2] ") {
        (Priority::Medium, rest)
    } else if let Some(rest) = rest.strip_prefix("[P3] ") {
        (Priority::Low, rest)
    } else {
        (Priority::None, rest)
    };
    let (saved, rest) = if let Some(rest) = rest.strip_prefix("[SAVE] ") {
        (true, rest)
    } else {
        (false, rest)
    };
    let (category, rest) = if let Some(category_rest) = rest.strip_prefix("[CAT:") {
        match category_rest.split_once("] ") {
            Some((category, rest)) if !category.is_empty() => (Some(category.to_string()), rest),
            _ => (None, rest),
        }
    } else {
        (None, rest)
    };
    let (text, due) = rest
        .rsplit_once(" ")
        .and_then(|(text, date)| {
            NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .ok()
                .map(|date| (text.trim_end_matches(" 📅").to_string(), Some(date)))
        })
        .unwrap_or_else(|| (rest.to_string(), None));
    Some(Todo {
        checked,
        text,
        depth,
        priority,
        due,
        saved,
        category,
        source: None,
    })
}

fn save_document(path: &Path, todos: &[Todo], markdown: &[MarkdownLine]) -> Result<()> {
    let mut output = String::new();
    for index in 0..=todos.len() {
        for line in markdown
            .iter()
            .filter(|line| line.before_todo.min(todos.len()) == index)
        {
            output.push_str(&line.text);
            output.push('\n');
        }
        let Some(todo) = todos.get(index) else {
            continue;
        };
        let mark = if todo.checked { "x" } else { " " };
        let priority = if todo.priority == Priority::None {
            String::new()
        } else {
            format!("[{}] ", todo.priority.label())
        };
        let due = todo.due.map(|date| format!(" {date}")).unwrap_or_default();
        let saved = if todo.saved { "[SAVE] " } else { "" };
        let category = todo
            .category
            .as_ref()
            .map(|category| format!("[CAT:{category}] "))
            .unwrap_or_default();
        let mut lines = todo.text.lines();
        let first_line = lines.next().unwrap_or_default();
        output.push_str(&format!(
            "{}- [{mark}] {priority}{saved}{category}{first_line}{due}\n",
            "  ".repeat(todo.depth),
        ));
        for continuation in lines {
            output.push_str(&format!(
                "{}      {continuation}\n",
                "  ".repeat(todo.depth)
            ));
        }
    }
    if output.is_empty() {
        output.push_str("# TODO\n\n");
    }
    fs::write(path, output).with_context(|| format!("failed to write {}", path.display()))
}

fn reorder_todo_block(todos: &mut [Todo], selected: usize, down: bool) -> Option<usize> {
    let depth = todos.get(selected)?.depth;
    let mut selected_end = selected + 1;
    while selected_end < todos.len() && todos[selected_end].depth > depth {
        selected_end += 1;
    }

    if down {
        if selected_end >= todos.len() || todos[selected_end].depth != depth {
            return None;
        }
        let next_start = selected_end;
        let mut next_end = next_start + 1;
        while next_end < todos.len() && todos[next_end].depth > depth {
            next_end += 1;
        }
        let next_len = next_end - next_start;
        todos[selected..next_end].rotate_left(selected_end - selected);
        Some(selected + next_len)
    } else {
        if selected == 0 {
            return None;
        }
        let mut previous_start = selected - 1;
        while todos[previous_start].depth > depth {
            if previous_start == 0 {
                return None;
            }
            previous_start -= 1;
        }
        if todos[previous_start].depth != depth {
            return None;
        }
        todos[previous_start..selected_end].rotate_right(selected_end - selected);
        Some(previous_start)
    }
}

fn sources_are_contiguous(todos: &[Todo]) -> bool {
    let mut seen = Vec::new();
    let mut previous: Option<&Option<PathBuf>> = None;
    for todo in todos {
        if previous == Some(&todo.source) {
            continue;
        }
        if seen.contains(&todo.source) {
            return false;
        }
        seen.push(todo.source.clone());
        previous = Some(&todo.source);
    }
    true
}

fn for_each_source(todos: &mut Vec<Todo>, operation: fn(&mut Vec<Todo>)) {
    let mut remaining = std::mem::take(todos).into_iter().peekable();
    while let Some(first) = remaining.next() {
        let source = first.source.clone();
        let mut group = vec![first];
        while remaining.peek().is_some_and(|todo| todo.source == source) {
            group.push(remaining.next().expect("peeked TODO must exist"));
        }
        operation(&mut group);
        todos.extend(group);
    }
}

fn group_categories(todos: &mut Vec<Todo>) {
    fn grouped_range(items: &[Todo], start: usize, depth: usize) -> (Vec<Todo>, usize) {
        let mut groups: Vec<(Option<String>, Priority, usize, Vec<Todo>)> = Vec::new();
        let mut index = start;
        let mut order = 0;
        while index < items.len() && items[index].depth == depth {
            let group_start = index;
            index += 1;
            while index < items.len() && items[index].depth > depth {
                index += 1;
            }
            let mut block = vec![items[group_start].clone()];
            if group_start + 1 < index {
                let (children, _) = grouped_range(items, group_start + 1, depth + 1);
                block.extend(children);
            }
            groups.push((
                items[group_start].category.clone(),
                items[group_start].priority,
                order,
                block,
            ));
            order += 1;
        }

        let mut categories: Vec<(Option<String>, Priority, usize)> = Vec::new();
        for (category, priority, original, _) in &groups {
            if let Some(existing) = categories.iter_mut().find(|item| item.0 == *category) {
                existing.1 = existing.1.min(*priority);
            } else {
                categories.push((category.clone(), *priority, *original));
            }
        }
        groups.sort_by_key(|(category, _, original, _)| {
            let (_, highest_priority, category_order) = categories
                .iter()
                .find(|item| item.0 == *category)
                .expect("category was collected from the same groups");
            (
                category.is_none(),
                *highest_priority,
                *category_order,
                *original,
            )
        });

        (
            groups
                .into_iter()
                .flat_map(|(_, _, _, block)| block)
                .collect(),
            index,
        )
    }

    if todos.is_empty() {
        return;
    }
    let (grouped, _) = grouped_range(todos, 0, 0);
    *todos = grouped;
}

fn remove_expired(todos: &mut Vec<Todo>, today: NaiveDate) -> usize {
    let cutoff = today
        .checked_sub_days(chrono::Days::new(7))
        .unwrap_or(NaiveDate::MIN);
    let before = todos.len();
    let mut retained = Vec::with_capacity(before);
    let mut skipped_depth = None;
    for todo in todos.drain(..) {
        if skipped_depth.is_some_and(|depth| todo.depth > depth) {
            continue;
        }
        skipped_depth = None;
        if !todo.saved && todo.due.is_some_and(|due| due <= cutoff) {
            skipped_depth = Some(todo.depth);
        } else {
            retained.push(todo);
        }
    }
    *todos = retained;
    before - todos.len()
}

fn sort_siblings(todos: &mut Vec<Todo>) {
    fn sorted_range(items: &[Todo], start: usize, depth: usize) -> (Vec<Todo>, usize) {
        let mut groups: Vec<(Priority, Option<NaiveDate>, usize, Vec<Todo>)> = Vec::new();
        let mut index = start;
        let mut order = 0;
        while index < items.len() && items[index].depth == depth {
            let group_start = index;
            index += 1;
            while index < items.len() && items[index].depth > depth {
                index += 1;
            }
            let mut group = vec![items[group_start].clone()];
            if group_start + 1 < index {
                let (children, _) = sorted_range(items, group_start + 1, depth + 1);
                group.extend(children);
            }
            groups.push((
                items[group_start].priority,
                items[group_start].due,
                order,
                group,
            ));
            order += 1;
        }
        groups.sort_by_key(|(priority, due, original, _)| {
            (*priority, due.unwrap_or(NaiveDate::MAX), *original)
        });
        (
            groups
                .into_iter()
                .flat_map(|(_, _, _, group)| group)
                .collect(),
            index,
        )
    }
    if !todos.is_empty() {
        let (sorted, _) = sorted_range(todos, 0, 0);
        *todos = sorted;
    }
}

fn wrap_display_width(text: &str, max_width: usize) -> Vec<Line<'static>> {
    wrap_strings(text, max_width)
        .into_iter()
        .map(Line::from)
        .collect()
}

fn wrap_strings(text: &str, max_width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut width = 0;
    for character in text.chars() {
        if character == '\n' {
            lines.push(std::mem::take(&mut current));
            width = 0;
            continue;
        }
        let character_width = character.width().unwrap_or(0);
        if width > 0 && width + character_width > max_width {
            lines.push(std::mem::take(&mut current));
            width = 0;
        }
        current.push(character);
        width += character_width;
    }
    lines.push(current);
    lines
}

fn table_cells(line: &str) -> Option<Vec<&str>> {
    let trimmed = line.trim();
    if !trimmed.contains('|') {
        return None;
    }
    let cells: Vec<_> = trimmed
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .collect();
    (cells.len() >= 2).then_some(cells)
}

fn is_table_separator(line: &str) -> bool {
    table_cells(line).is_some_and(|cells| {
        cells.iter().all(|cell| {
            cell.contains('-')
                && cell
                    .chars()
                    .all(|character| matches!(character, '-' | ':' | ' '))
        })
    })
}

fn fit_cell(text: &str, width: usize) -> String {
    let mut fitted = String::new();
    let mut used = 0;
    for character in text.chars() {
        let character_width = character.width().unwrap_or(0);
        if used + character_width > width {
            break;
        }
        fitted.push(character);
        used += character_width;
    }
    fitted.push_str(&" ".repeat(width.saturating_sub(used)));
    fitted
}

fn render_table(lines: &[&MarkdownLine], available_width: usize) -> Vec<(String, Style)> {
    let rows: Vec<Vec<&str>> = lines
        .iter()
        .filter(|line| !is_table_separator(&line.text))
        .filter_map(|line| table_cells(&line.text))
        .collect();
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    if columns == 0 {
        return Vec::new();
    }
    let max_cell_width = available_width
        .saturating_sub(columns + 1)
        .checked_div(columns)
        .unwrap_or(1)
        .saturating_sub(2)
        .max(1);
    let widths: Vec<usize> = (0..columns)
        .map(|column| {
            rows.iter()
                .filter_map(|row| row.get(column))
                .map(|cell| cell.width())
                .max()
                .unwrap_or(1)
                .min(max_cell_width)
                .max(1)
        })
        .collect();
    let border = |left: char, middle: char, right: char| {
        format!(
            "{left}{}{right}",
            widths
                .iter()
                .map(|width| "─".repeat(width + 2))
                .collect::<Vec<_>>()
                .join(&middle.to_string())
        )
    };
    let border_style = Style::default().fg(Color::Gray);
    let mut rendered = vec![(border('┌', '┬', '┐'), border_style)];
    for (row_index, row) in rows.iter().enumerate() {
        let content = (0..columns)
            .map(|column| fit_cell(row.get(column).copied().unwrap_or(""), widths[column]))
            .collect::<Vec<_>>()
            .join(" │ ");
        let style = if row_index == 0 {
            Style::default()
                .fg(Color::LightCyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        rendered.push((format!("│ {content} │"), style));
        if row_index == 0 && rows.len() > 1 {
            rendered.push((border('├', '┼', '┤'), border_style));
        }
    }
    rendered.push((border('└', '┴', '┘'), border_style));
    rendered
}

fn todo_style(todo: &Todo, today: NaiveDate) -> Style {
    if todo.checked {
        return Style::default()
            .fg(Color::Gray)
            .add_modifier(Modifier::CROSSED_OUT);
    }
    let _ = today;
    match todo.priority {
        Priority::High => Style::default().fg(Color::Rgb(255, 105, 180)),
        Priority::Medium => Style::default().fg(Color::LightBlue),
        Priority::Low => Style::default().fg(Color::Rgb(173, 255, 47)),
        Priority::None => Style::default().fg(Color::White),
    }
}

fn priority_style(todo: &Todo) -> Style {
    if todo.checked {
        return Style::default()
            .fg(Color::Gray)
            .add_modifier(Modifier::CROSSED_OUT);
    }
    match todo.priority {
        Priority::High => Style::default()
            .fg(Color::Rgb(255, 105, 180))
            .add_modifier(Modifier::BOLD),
        Priority::Medium => Style::default()
            .fg(Color::LightBlue)
            .add_modifier(Modifier::BOLD),
        Priority::Low => Style::default()
            .fg(Color::Rgb(173, 255, 47))
            .add_modifier(Modifier::BOLD),
        Priority::None => Style::default().fg(Color::White),
    }
}

fn due_style(todo: &Todo, today: NaiveDate) -> Style {
    if todo.checked {
        return Style::default()
            .fg(Color::Gray)
            .add_modifier(Modifier::CROSSED_OUT);
    }
    if todo.due.is_some_and(|due| due < today) {
        return Style::default()
            .fg(Color::LightRed)
            .add_modifier(Modifier::BOLD);
    }
    let tomorrow = today
        .checked_add_days(chrono::Days::new(1))
        .unwrap_or(NaiveDate::MAX);
    if todo.due.is_some_and(|due| due <= tomorrow) {
        return Style::default()
            .fg(Color::LightYellow)
            .add_modifier(Modifier::BOLD);
    }
    Style::default().fg(Color::Gray)
}

fn todo_list_item(todo: &Todo, available_width: usize, today: NaiveDate) -> ListItem<'static> {
    let hierarchy = if todo.depth == 0 {
        String::new()
    } else {
        format!("{}|-- ", "|  ".repeat(todo.depth.saturating_sub(1)))
    };
    let mark = if todo.checked { "☑ " } else { "☐ " };
    let priority = format!("{:>2}", todo.priority.label());
    let saved = if todo.saved { "  SAVE" } else { "" };
    let category = todo
        .category
        .as_ref()
        .map(|category| format!("  #{category}"))
        .unwrap_or_default();
    let due = todo
        .due
        .map(|date| format!("  due:{date}"))
        .unwrap_or_default();
    let location = todo
        .source
        .as_ref()
        .and_then(|path| path.parent())
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .map(|name| format!("[{name}] "))
        .unwrap_or_default();
    let prefix_width = location.width()
        + hierarchy.width()
        + mark.width()
        + priority.width()
        + saved.width()
        + category.width()
        + 2;
    let text_width = available_width
        .saturating_sub(prefix_width + due.width())
        .max(1);
    let wrapped = wrap_strings(&todo.text, text_width);
    let body_style = todo_style(todo, today);
    let saved_style = if todo.checked {
        body_style
    } else {
        Style::default().fg(Color::LightMagenta)
    };
    let category_style = if todo.checked {
        body_style
    } else {
        Style::default().fg(Color::LightCyan)
    };
    let mut lines = Vec::with_capacity(wrapped.len());
    let first = wrapped.first().cloned().unwrap_or_default();
    lines.push(Line::from(vec![
        Span::styled(location, Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{hierarchy}{mark}"), body_style),
        Span::styled(priority, priority_style(todo)),
        Span::styled(saved, saved_style),
        Span::styled(category, category_style),
        Span::styled("  ", body_style),
        Span::styled(first, body_style),
        Span::styled(due, due_style(todo, today)),
    ]));
    let continuation = " ".repeat(prefix_width);
    for line in wrapped.into_iter().skip(1) {
        lines.push(Line::from(vec![
            Span::raw(continuation.clone()),
            Span::styled(line, body_style),
        ]));
    }
    ListItem::new(lines)
}

fn todo_matches_search(todo: &Todo, query: Option<&str>) -> bool {
    let Some(query) = query else {
        return true;
    };
    let query = query.to_lowercase();
    todo.text.to_lowercase().contains(&query)
        || todo
            .category
            .as_ref()
            .is_some_and(|category| category.to_lowercase().contains(&query))
}

fn render_todo_list(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    name: &str,
    path: &Path,
    cwd: Option<&Path>,
    document: DocumentRef<'_>,
    view: TodoListView<'_>,
) {
    let TodoListView {
        selected,
        active,
        search_query,
    } = view;
    let todos = document.todos;
    let markdown = document.markdown;
    let available_width = area.width.saturating_sub(6).max(1) as usize;
    let today = Local::now().date_naive();
    let mut items = Vec::new();
    let mut selected_row = 0;
    for index in 0..=todos.len() {
        let markdown_lines: Vec<_> = if search_query.is_none() {
            markdown
                .iter()
                .filter(|line| {
                    let offset = line
                        .source
                        .as_ref()
                        .and_then(|source| {
                            todos
                                .iter()
                                .position(|todo| todo.source.as_ref() == Some(source))
                        })
                        .unwrap_or(0);
                    (offset + line.before_todo).min(todos.len()) == index
                })
                .collect()
        } else {
            Vec::new()
        };
        let mut markdown_index = 0;
        while markdown_index < markdown_lines.len() {
            if markdown_index + 1 < markdown_lines.len()
                && table_cells(&markdown_lines[markdown_index].text).is_some()
                && is_table_separator(&markdown_lines[markdown_index + 1].text)
            {
                let mut table_end = markdown_index + 2;
                while table_end < markdown_lines.len()
                    && table_cells(&markdown_lines[table_end].text).is_some()
                {
                    table_end += 1;
                }
                for (line, style) in
                    render_table(&markdown_lines[markdown_index..table_end], available_width)
                {
                    items.push(ListItem::new(line).style(style));
                }
                markdown_index = table_end;
                continue;
            }
            let markdown_line = markdown_lines[markdown_index];
            let trimmed = markdown_line.text.trim();
            let heading_level = trimmed.chars().take_while(|char| *char == '#').count();
            let (display, style) = if (1..=6).contains(&heading_level)
                && trimmed.as_bytes().get(heading_level) == Some(&b' ')
            {
                let heading = &trimmed[heading_level + 1..];
                let modifier = if heading_level == 1 {
                    Modifier::BOLD | Modifier::UNDERLINED
                } else {
                    Modifier::BOLD
                };
                (
                    format!("{} {heading}", "━".repeat(heading_level.saturating_sub(1))),
                    Style::default().fg(Color::LightCyan).add_modifier(modifier),
                )
            } else if let Some(quote) = trimmed.strip_prefix("> ") {
                (
                    format!("│ {quote}"),
                    Style::default()
                        .fg(Color::Gray)
                        .add_modifier(Modifier::ITALIC),
                )
            } else {
                (
                    markdown_line.text.clone(),
                    Style::default().fg(Color::White),
                )
            };
            items.push(ListItem::new(wrap_display_width(&display, available_width)).style(style));
            markdown_index += 1;
        }
        if index == selected {
            selected_row = items.len();
        }
        if let Some(todo) = todos
            .get(index)
            .filter(|todo| todo_matches_search(todo, search_query))
        {
            items.push(todo_list_item(todo, available_width, today));
        }
    }
    let marker = if active { ">" } else { "-" };
    let availability = if path.exists() {
        ""
    } else {
        " [TODO.md not found. Create it with Shift+C]"
    };
    let border_style = Style::default().fg(if active {
        Color::LightCyan
    } else {
        Color::Gray
    });
    let completed = todos.iter().filter(|todo| todo.checked).count();
    let search = search_query
        .map(|query| {
            let matches = todos
                .iter()
                .filter(|todo| todo_matches_search(todo, Some(query)))
                .count();
            format!("  /{query} ({matches})")
        })
        .unwrap_or_default();
    let cwd = cwd
        .map(|cwd| format!(" · {}", cwd.display()))
        .unwrap_or_default();
    let list = List::new(items)
        .block(
            Block::default()
                .title(format!(
                    " {marker} {name} · {} TODO{cwd} · {completed} done{search}{availability} ",
                    todos.len()
                ))
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .highlight_symbol("> ")
        .highlight_style(
            Style::default()
                .fg(Color::White)
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );
    let mut state = ListState::default();
    if active && !todos.is_empty() {
        state.select(Some(selected_row));
    }
    frame.render_stateful_widget(list, area, &mut state);
}

fn input_popup_area(area: ratatui::layout::Rect) -> ratatui::layout::Rect {
    let width = (area.width * 3 / 4)
        .max(20)
        .min(area.width.saturating_sub(2));
    let height = 14.min(area.height.saturating_sub(2)).max(5);
    ratatui::layout::Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

fn input_cursor(input: &str, width: u16) -> (u16, u16) {
    let width = width.max(1) as usize;
    let mut row = 0;
    let mut column = 0;
    for (line_index, line) in input.split('\n').enumerate() {
        if line_index > 0 {
            row += 1;
        }
        let line_width = line.width();
        row += line_width / width;
        column = line_width % width;
    }
    (column as u16, row as u16)
}

fn draw(frame: &mut ratatui::Frame, app: &mut App) {
    let show_inline_input = matches!(
        app.input_mode,
        InputMode::Due
            | InputMode::Category
            | InputMode::Search
            | InputMode::ConfirmDelete
            | InputMode::ConfirmBulkDelete
    );
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Fill(1),
            Constraint::Length(if show_inline_input { 3 } else { 0 }),
            Constraint::Length(1),
        ])
        .split(frame.area());
    let (
        DocumentRef {
            todos: local_todos,
            markdown: local_markdown,
        },
        local_selected,
        DocumentRef {
            todos: global_todos,
            markdown: global_markdown,
        },
        global_selected,
    ) = match app.scope {
        Scope::Local => (
            DocumentRef {
                todos: &app.todos,
                markdown: &app.markdown,
            },
            app.selected,
            DocumentRef {
                todos: &app.other_todos,
                markdown: &app.other_markdown,
            },
            app.other_selected,
        ),
        Scope::Global => (
            DocumentRef {
                todos: &app.other_todos,
                markdown: &app.other_markdown,
            },
            app.other_selected,
            DocumentRef {
                todos: &app.todos,
                markdown: &app.markdown,
            },
            app.selected,
        ),
    };
    let local_name = if app.descendants_expanded {
        "Local ▾ descendants (3 levels)"
    } else {
        "Local ▸ descendants"
    };
    render_todo_list(
        frame,
        areas[0],
        local_name,
        &app.local_path,
        Some(&app.local_root),
        DocumentRef {
            todos: local_todos,
            markdown: local_markdown,
        },
        TodoListView {
            selected: local_selected,
            active: app.scope == Scope::Local,
            search_query: if app.scope == Scope::Local {
                app.search_query.as_deref()
            } else {
                app.other_search_query.as_deref()
            },
        },
    );
    render_todo_list(
        frame,
        areas[1],
        "Global",
        &app.global_path,
        None,
        DocumentRef {
            todos: global_todos,
            markdown: global_markdown,
        },
        TodoListView {
            selected: global_selected,
            active: app.scope == Scope::Global,
            search_query: if app.scope == Scope::Global {
                app.search_query.as_deref()
            } else {
                app.other_search_query.as_deref()
            },
        },
    );

    let input_title = match app.input_mode {
        InputMode::Add => " Add TODO ",
        InputMode::Edit => " Edit TODO ",
        InputMode::Due => " Due date ",
        InputMode::Category => " Category (empty: clear) ",
        InputMode::Search => " Search text/category (empty: clear) ",
        InputMode::ConfirmDelete => " Delete confirmation ",
        InputMode::ConfirmBulkDelete => " Bulk delete confirmation ",
        _ => " Input ",
    };
    let input_text = if matches!(
        app.input_mode,
        InputMode::ConfirmDelete | InputMode::ConfirmBulkDelete
    ) {
        "Press y to delete, n or Esc to cancel"
    } else if matches!(app.input_mode, InputMode::Add | InputMode::Edit) {
        "Editing in popup: Shift/Alt+Enter newline, Enter save"
    } else {
        &app.input
    };
    if show_inline_input {
        frame.render_widget(
            Paragraph::new(input_text)
                .style(Style::default().fg(Color::White))
                .block(
                    Block::default()
                        .title(input_title)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::LightCyan)),
                )
                .wrap(Wrap { trim: false }),
            areas[2],
        );
    }
    let message_style = match app.message_kind {
        MessageKind::Default => Style::default(),
        MessageKind::Success => Style::default().fg(Color::LightGreen),
        MessageKind::Warning => Style::default().fg(Color::LightYellow),
    }
    .add_modifier(Modifier::BOLD);
    let scope = format!(" {} ", app.scope_name().to_uppercase());
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                scope,
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(app.message.as_str(), message_style),
        ])),
        areas[3],
    );

    if matches!(
        app.input_mode,
        InputMode::Due | InputMode::Category | InputMode::Search
    ) {
        let inner_width = areas[2].width.saturating_sub(2).max(1);
        let cursor_width = app.input.as_str().width() as u16;
        frame.set_cursor_position((
            areas[2].x + 1 + cursor_width % inner_width,
            areas[2].y + 1 + cursor_width / inner_width,
        ));
    }

    if matches!(app.input_mode, InputMode::Add | InputMode::Edit) {
        let popup = input_popup_area(frame.area());
        let inner_width = popup.width.saturating_sub(2).max(1);
        let visible_height = popup.height.saturating_sub(2).max(1);
        let (cursor_x, cursor_row) = input_cursor(&app.input, inner_width);
        let scroll = cursor_row.saturating_sub(visible_height - 1);
        let title = if app.input_mode == InputMode::Add {
            " Add TODO (Enter: save, Shift/Alt+Enter: newline, Esc: cancel) "
        } else {
            " Edit TODO (Enter: save, Shift/Alt+Enter: newline, Esc: cancel) "
        };
        frame.render_widget(ratatui::widgets::Clear, popup);
        frame.render_widget(
            Paragraph::new(app.input.as_str())
                .wrap(Wrap { trim: false })
                .scroll((scroll, 0))
                .block(
                    Block::default()
                        .title(title)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::LightCyan)),
                ),
            popup,
        );
        frame.set_cursor_position((
            popup.x + 1 + cursor_x,
            popup.y + 1 + cursor_row.saturating_sub(scroll),
        ));
    }

    if app.input_mode == InputMode::Help {
        let popup = ratatui::layout::Rect {
            x: frame.area().width / 8,
            y: frame.area().height / 5,
            width: frame.area().width * 3 / 4,
            height: 11,
        };
        frame.render_widget(ratatui::widgets::Clear, popup);
        let help = vec![
            Line::from(vec![Span::raw(
                "Tab switch  j/k select  J/K reorder  Space toggle  a/e/d edit  u undo",
            )]),
            Line::from("/ search text/category  c category  f group category  u undo"),
            Line::from("p priority  s sort  t due  S save  C create"),
            Line::from("h/l or </> outdent/indent  gg/G first/last"),
            Line::from("v expand/collapse Local descendants (up to 3 levels)"),
            Line::from("Priority: P1 high, P2 medium, P3 low, -- unset"),
            Line::from("Cmd+Shift+Q quit (q/Esc stay open)"),
            Line::from("Enter/Esc closes this help"),
        ];
        frame.render_widget(
            Paragraph::new(help)
                .wrap(Wrap { trim: false })
                .block(Block::default().title(" Help ").borders(Borders::ALL)),
            popup,
        );
    }

    if app.input_mode == InputMode::AncestorPicker {
        let area = input_popup_area(frame.area());
        frame.render_widget(ratatui::widgets::Clear, area);
        let items = if app.ancestor_candidates.is_empty() {
            vec![ListItem::new("No ancestor TODO.md found")]
        } else {
            app.ancestor_candidates
                .iter()
                .map(|(path, selected)| {
                    let marker = if *selected { "[x]" } else { "[ ]" };
                    ListItem::new(format!("{marker} {}", path.display()))
                })
                .collect()
        };
        let mut state = ListState::default();
        if !app.ancestor_candidates.is_empty() {
            state.select(Some(app.ancestor_selected));
        }
        frame.render_stateful_widget(
            List::new(items).block(
                Block::default()
                    .title(" Shared ancestor TODOs (Space toggle, Enter save) ")
                    .borders(Borders::ALL),
            ),
            area,
            &mut state,
        );
    }
}

fn handle_normal_mode(app: &mut App, key: KeyEvent) -> Result<bool> {
    if matches!(key.code, KeyCode::Char('q' | 'Q'))
        && key
            .modifiers
            .contains(KeyModifiers::SUPER | KeyModifiers::SHIFT)
    {
        return Ok(true);
    }

    if app.pending_g {
        app.pending_g = false;
        if key.code == KeyCode::Char('g') {
            app.selected = app
                .todos
                .iter()
                .position(|todo| todo_matches_search(todo, app.search_query.as_deref()))
                .unwrap_or(0);
            return Ok(false);
        }
    }

    match key.code {
        KeyCode::Char('j') | KeyCode::Down => app.move_down(),
        KeyCode::Char('k') | KeyCode::Up => app.move_up(),
        KeyCode::Char('J') => app.reorder_selected(true)?,
        KeyCode::Char('K') => app.reorder_selected(false)?,
        KeyCode::Char('g') => app.pending_g = true,
        KeyCode::Char('G') => {
            app.selected = app
                .todos
                .iter()
                .rposition(|todo| todo_matches_search(todo, app.search_query.as_deref()))
                .unwrap_or(0)
        }
        KeyCode::Char(' ') | KeyCode::Enter => app.toggle_selected()?,
        KeyCode::Char('a') => app.start_add(),
        KeyCode::Char('e') => app.start_edit(),
        KeyCode::Char('d') => app.start_delete(),
        KeyCode::Char('D') => app.start_bulk_delete(),
        KeyCode::Char('S') => app.toggle_saved()?,
        KeyCode::Char('C') => app.create_local_file()?,
        KeyCode::Char('p') => app.cycle_priority()?,
        KeyCode::Char('s') => app.sort_by_priority()?,
        KeyCode::Char('c') => app.start_category(),
        KeyCode::Char('f') => app.group_by_category()?,
        KeyCode::Char('t') => app.start_due(),
        KeyCode::Char('u') => app.undo()?,
        KeyCode::Char('/') => app.start_search(),
        KeyCode::Char('-') => app.start_ancestor_picker()?,
        KeyCode::Char('v') => app.toggle_descendants()?,
        KeyCode::Char('l') | KeyCode::Char('>') | KeyCode::Right => app.change_depth(true)?,
        KeyCode::Char('h') | KeyCode::Char('<') | KeyCode::Left => app.change_depth(false)?,
        KeyCode::Tab => app.toggle_scope()?,
        KeyCode::Char('r') => {
            app.reload()?;
            app.set_message("Reloaded", MessageKind::Success);
        }
        KeyCode::Char('?') => app.input_mode = InputMode::Help,
        _ => {}
    }
    Ok(false)
}

fn handle_input_mode(app: &mut App, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => app.cancel_input(),
        KeyCode::Enter
            if matches!(app.input_mode, InputMode::Add | InputMode::Edit)
                && key
                    .modifiers
                    .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) =>
        {
            app.input.push('\n');
        }
        KeyCode::Enter => app.submit_input()?,
        KeyCode::Backspace => {
            app.input.pop();
        }
        KeyCode::Char(character) => app.input.push(character),
        _ => {}
    }
    Ok(())
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> Result<()> {
    loop {
        app.update_message_timeout();
        app.update_local_cwd()?;
        app.refresh_changed_files()?;
        terminal.draw(|frame| draw(frame, app))?;
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match app.input_mode {
            InputMode::Normal => {
                if handle_normal_mode(app, key)? {
                    return Ok(());
                }
            }
            InputMode::Add
            | InputMode::Edit
            | InputMode::Due
            | InputMode::Category
            | InputMode::Search => handle_input_mode(app, key)?,
            InputMode::ConfirmDelete | InputMode::ConfirmBulkDelete => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    if app.input_mode == InputMode::ConfirmDelete {
                        app.delete_selected()?;
                    } else {
                        app.delete_completed()?;
                    }
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => app.cancel_input(),
                _ => {}
            },
            InputMode::Help => {
                if matches!(key.code, KeyCode::Enter | KeyCode::Esc | KeyCode::Char('?')) {
                    app.input_mode = InputMode::Normal;
                }
            }
            InputMode::AncestorPicker => match key.code {
                KeyCode::Esc => app.cancel_input(),
                KeyCode::Up | KeyCode::Char('k') => {
                    app.ancestor_selected = app.ancestor_selected.saturating_sub(1)
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    app.ancestor_selected = (app.ancestor_selected + 1)
                        .min(app.ancestor_candidates.len().saturating_sub(1))
                }
                KeyCode::Char(' ') => {
                    if let Some((_, selected)) =
                        app.ancestor_candidates.get_mut(app.ancestor_selected)
                    {
                        *selected = !*selected;
                    }
                }
                KeyCode::Enter => app.save_ancestor_picker()?,
                _ => {}
            },
        }
    }
}

fn global_todo_path() -> Result<PathBuf> {
    if let Some(path) = env::var_os("HERDR_TODO_GLOBAL_PATH") {
        return Ok(PathBuf::from(path));
    }
    let home = env::var_os("HOME").context("HOME is not set; set HERDR_TODO_GLOBAL_PATH")?;
    Ok(PathBuf::from(home).join(".herdr").join("TODO.md"))
}

fn main() -> Result<()> {
    let local_path = env::current_dir()
        .context("failed to determine current directory")?
        .join("TODO.md");
    let mut app = App::new(local_path, global_todo_path()?)?;
    enable_raw_mode()?;
    let mut output = stdout();
    execute!(output, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(output);
    let mut terminal = Terminal::new(backend)?;
    let result = run_app(&mut terminal, &mut app);
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hierarchy_priority_and_due_date() {
        let todo = parse_todo_line("    - [x] [P1] [SAVE] ship it 2026-07-18").unwrap();
        assert!(todo.checked);
        assert_eq!(todo.depth, 2);
        assert_eq!(todo.priority, Priority::High);
        assert_eq!(todo.text, "ship it");
        assert_eq!(todo.due.unwrap().to_string(), "2026-07-18");
        assert!(todo.saved);
    }

    #[test]
    fn parses_legacy_todos() {
        let todo = parse_todo_line("- [ ] plain task").unwrap();
        assert_eq!(todo.priority, Priority::None);
        assert_eq!(todo.text, "plain task");
    }

    #[test]
    fn parses_and_saves_category() {
        let todo = parse_todo_line("- [ ] [P2] [SAVE] [CAT:仕事] ship it").unwrap();
        assert_eq!(todo.category.as_deref(), Some("仕事"));
        assert_eq!(todo.text, "ship it");

        let path = std::env::temp_dir().join(format!(
            "herdr-todo-category-{}-{}.md",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        save_document(&path, &[todo], &[]).unwrap();
        assert!(fs::read_to_string(&path).unwrap().contains("[CAT:仕事]"));
        assert_eq!(
            load_document(&path).unwrap().0[0].category.as_deref(),
            Some("仕事")
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn groups_categories_by_their_highest_priority() {
        let make_todo = |text: &str, depth, priority, category: Option<&str>| Todo {
            checked: false,
            text: text.into(),
            depth,
            priority,
            due: None,
            saved: false,
            category: category.map(str::to_string),
            source: None,
        };
        let mut todos = vec![
            make_todo("work low", 0, Priority::Low, Some("work")),
            make_todo("personal", 0, Priority::Medium, Some("personal")),
            make_todo("work high", 0, Priority::High, Some("work")),
            make_todo("work child", 1, Priority::None, Some("child category")),
            make_todo("uncategorized high", 0, Priority::High, None),
        ];

        group_categories(&mut todos);
        assert_eq!(
            todos
                .iter()
                .map(|todo| todo.text.as_str())
                .collect::<Vec<_>>(),
            vec![
                "work low",
                "work high",
                "work child",
                "personal",
                "uncategorized high"
            ]
        );
    }

    #[test]
    fn searches_todo_text_and_category_case_insensitively() {
        let todo = Todo {
            checked: false,
            text: "Deploy API".into(),
            depth: 0,
            priority: Priority::None,
            due: None,
            saved: false,
            category: Some("仕事".into()),
            source: None,
        };
        assert!(todo_matches_search(&todo, Some("api")));
        assert!(todo_matches_search(&todo, Some("DEPLOY")));
        assert!(todo_matches_search(&todo, Some("仕事")));
        assert!(!todo_matches_search(&todo, Some("個人")));
        assert!(todo_matches_search(&todo, None));
    }

    #[test]
    fn undo_restores_the_previous_saved_state() {
        let unique = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let local = std::env::temp_dir().join(format!("herdr-todo-undo-local-{unique}.md"));
        let global = std::env::temp_dir().join(format!("herdr-todo-undo-global-{unique}.md"));
        fs::write(&local, "- [ ] task\n").unwrap();
        let mut app = App::new(local.clone(), global.clone()).unwrap();

        app.toggle_selected().unwrap();
        assert!(app.todos[0].checked);
        app.undo().unwrap();
        assert!(!app.todos[0].checked);
        assert!(app.undo_stack.is_empty());
        assert!(fs::read_to_string(&local).unwrap().contains("- [ ] task"));

        fs::remove_file(local).unwrap();
        fs::remove_file(global).unwrap();
    }

    #[test]
    fn automatically_refreshes_externally_changed_todo_files() {
        let unique = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let local = std::env::temp_dir().join(format!("herdr-todo-watch-local-{unique}.md"));
        let global = std::env::temp_dir().join(format!("herdr-todo-watch-global-{unique}.md"));
        fs::write(&local, "- [ ] before\n").unwrap();
        let mut app = App::new(local.clone(), global.clone()).unwrap();
        app.undo_stack.push(Vec::new());

        fs::write(&local, "- [ ] changed externally\n- [ ] second task\n").unwrap();
        app.last_file_check = Instant::now() - Duration::from_secs(1);
        app.refresh_changed_files().unwrap();

        assert_eq!(app.todos.len(), 2);
        assert_eq!(app.todos[0].text, "changed externally");
        assert!(app.undo_stack.is_empty());
        assert_eq!(app.file_stamp, file_stamp(&local));

        fs::remove_file(local).unwrap();
        fs::remove_file(global).unwrap();
    }

    #[test]
    fn loads_descendant_and_selected_ancestor_todos_and_saves_to_sources() {
        let unique = format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let base = std::env::temp_dir().join(format!("herdr-todo-tree-{unique}"));
        let root = base.join("root");
        let current = root.join("current");
        let child = current.join("child");
        fs::create_dir_all(&child).unwrap();
        fs::write(root.join("TODO.md"), "- [ ] ancestor\n").unwrap();
        fs::write(child.join("TODO.md"), "- [ ] descendant\n").unwrap();

        write_shared_ancestors(&current, &[root.join("TODO.md")]).unwrap();
        let (mut todos, markdown) = load_local_documents(&current, true).unwrap();
        assert!(todos.iter().any(|todo| todo.text == "ancestor"));
        assert!(todos.iter().any(|todo| todo.text == "descendant"));

        let ancestor = todos
            .iter_mut()
            .find(|todo| todo.text == "ancestor")
            .unwrap();
        ancestor.checked = true;
        save_local_documents(&todos, &markdown).unwrap();
        assert!(
            fs::read_to_string(root.join("TODO.md"))
                .unwrap()
                .contains("- [x] ancestor")
        );
        assert!(
            fs::read_to_string(child.join("TODO.md"))
                .unwrap()
                .contains("- [ ] descendant")
        );

        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn descendant_todos_are_limited_to_three_levels_and_can_be_collapsed() {
        let unique = format!(
            "{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let base = std::env::temp_dir().join(format!("herdr-todo-depth-{unique}"));
        let root = base.join("root");
        let level1 = root.join("one");
        let level2 = level1.join("two");
        let level3 = level2.join("three");
        let level4 = level3.join("four");
        fs::create_dir_all(&level4).unwrap();
        for (dir, text) in [
            (&root, "root"),
            (&level1, "level 1"),
            (&level2, "level 2"),
            (&level3, "level 3"),
            (&level4, "level 4"),
        ] {
            fs::write(dir.join("TODO.md"), format!("- [ ] {text}\n")).unwrap();
        }

        let (expanded, _) = load_local_documents(&root, true).unwrap();
        assert_eq!(
            expanded
                .iter()
                .map(|todo| todo.text.as_str())
                .collect::<Vec<_>>(),
            vec!["root", "level 1", "level 2", "level 3"]
        );

        let (collapsed, _) = load_local_documents(&root, false).unwrap();
        assert_eq!(
            collapsed
                .iter()
                .map(|todo| todo.text.as_str())
                .collect::<Vec<_>>(),
            vec!["root"]
        );

        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn sorts_siblings_without_detaching_children() {
        let mut todos = vec![
            Todo {
                checked: false,
                text: "low".into(),
                depth: 0,
                priority: Priority::Low,
                due: None,
                saved: false,
                category: None,
                source: None,
            },
            Todo {
                checked: false,
                text: "child".into(),
                depth: 1,
                priority: Priority::High,
                due: None,
                saved: false,
                category: None,
                source: None,
            },
            Todo {
                checked: false,
                text: "high without due".into(),
                depth: 0,
                priority: Priority::High,
                due: None,
                saved: false,
                category: None,
                source: None,
            },
            Todo {
                checked: false,
                text: "high due first".into(),
                depth: 0,
                priority: Priority::High,
                due: NaiveDate::from_ymd_opt(2026, 7, 18),
                saved: false,
                category: None,
                source: None,
            },
        ];
        sort_siblings(&mut todos);
        assert_eq!(
            todos
                .iter()
                .map(|todo| todo.text.as_str())
                .collect::<Vec<_>>(),
            vec!["high due first", "high without due", "low", "child"]
        );
    }

    #[test]
    fn reorders_sibling_blocks_without_detaching_children() {
        let make_todo = |text: &str, depth| Todo {
            checked: false,
            text: text.into(),
            depth,
            priority: Priority::None,
            due: None,
            saved: false,
            category: None,
            source: None,
        };
        let mut todos = vec![
            make_todo("first", 0),
            make_todo("first child", 1),
            make_todo("second", 0),
            make_todo("second child", 1),
            make_todo("third", 0),
        ];

        assert_eq!(reorder_todo_block(&mut todos, 0, true), Some(2));
        assert_eq!(
            todos
                .iter()
                .map(|todo| todo.text.as_str())
                .collect::<Vec<_>>(),
            vec!["second", "second child", "first", "first child", "third"]
        );
        assert_eq!(reorder_todo_block(&mut todos, 2, false), Some(0));
        assert_eq!(todos[0].text, "first");
        assert_eq!(todos[1].text, "first child");
        assert_eq!(reorder_todo_block(&mut todos, 0, false), None);
    }

    #[test]
    fn wraps_using_full_width_character_widths() {
        let lines = wrap_display_width("日本語abc", 5);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].width(), 4);
        assert_eq!(lines[1].width(), 5);
    }

    #[test]
    fn reads_markdown_continuation_as_multiline_text() {
        let path = std::env::temp_dir().join(format!("herdr-todo-{}.md", std::process::id()));
        fs::write(&path, "# TODO\n\n- [ ] first line\n      second line\n").unwrap();
        let todos = load_document(&path).unwrap().0;
        fs::remove_file(path).unwrap();
        assert_eq!(todos[0].text, "first line\nsecond line");
    }

    #[test]
    fn preserves_and_positions_markdown_around_todos() {
        let path = std::env::temp_dir().join(format!(
            "herdr-todo-markdown-{}-{}.md",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let source = "# Project\n\nIntro text\n\n- [ ] parent\n  - [ ] child\n\n## Later\n\n> memo\n- [x] done\n";
        fs::write(&path, source).unwrap();
        let (todos, markdown) = load_document(&path).unwrap();
        assert_eq!(todos.len(), 3);
        assert_eq!(todos[1].depth, 1);
        assert!(markdown.iter().any(|line| line.text == "# Project"));
        assert!(markdown.iter().any(|line| line.text == "## Later"));

        save_document(&path, &todos, &markdown).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), source);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn renders_markdown_tables_with_aligned_borders() {
        let markdown = [
            MarkdownLine {
                before_todo: 0,
                text: "| Name | Status |".into(),
                source: None,
            },
            MarkdownLine {
                before_todo: 0,
                text: "| --- | --- |".into(),
                source: None,
            },
            MarkdownLine {
                before_todo: 0,
                text: "| 日本語 | done |".into(),
                source: None,
            },
        ];
        let rendered = render_table(&markdown.iter().collect::<Vec<_>>(), 40);
        assert_eq!(rendered.len(), 5);
        assert!(rendered[0].0.starts_with('┌'));
        assert!(rendered[1].0.contains("Name"));
        assert!(rendered[2].0.contains('┼'));
        assert!(rendered[3].0.contains("日本語"));
        assert_eq!(rendered[0].0.width(), rendered[3].0.width());
    }

    #[test]
    fn removes_week_old_overdue_todos_but_keeps_saved_todos() {
        let today = NaiveDate::from_ymd_opt(2026, 7, 18).unwrap();
        let old_due = NaiveDate::from_ymd_opt(2026, 7, 11);
        let mut todos = vec![
            Todo {
                checked: false,
                text: "expired parent".into(),
                depth: 0,
                priority: Priority::None,
                due: old_due,
                saved: false,
                category: None,
                source: None,
            },
            Todo {
                checked: false,
                text: "child".into(),
                depth: 1,
                priority: Priority::None,
                due: None,
                saved: true,
                category: None,
                source: None,
            },
            Todo {
                checked: false,
                text: "protected".into(),
                depth: 0,
                priority: Priority::None,
                due: old_due,
                saved: true,
                category: None,
                source: None,
            },
        ];
        assert_eq!(remove_expired(&mut todos, today), 2);
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].text, "protected");
    }

    #[test]
    fn missing_todo_file_is_not_created_when_loaded() {
        let path = std::env::temp_dir().join(format!(
            "herdr-todo-missing-{}-{}.md",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        assert!(load_document(&path).unwrap().0.is_empty());
        assert!(!path.exists());
        ensure_todo_file(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "# TODO\n\n");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn highlights_urgent_and_overdue_todos() {
        let today = NaiveDate::from_ymd_opt(2026, 7, 18).unwrap();
        let mut todo = Todo {
            checked: false,
            text: "urgent".into(),
            depth: 0,
            priority: Priority::None,
            due: Some(today),
            saved: false,
            category: None,
            source: None,
        };
        assert_eq!(
            due_style(&todo, today),
            Style::default()
                .fg(Color::LightYellow)
                .add_modifier(Modifier::BOLD)
        );
        todo.due = today.checked_sub_days(chrono::Days::new(1));
        assert_eq!(
            due_style(&todo, today),
            Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD)
        );
        todo.due = None;
        assert_eq!(priority_style(&todo), Style::default().fg(Color::White));
        todo.priority = Priority::High;
        assert_eq!(
            priority_style(&todo),
            Style::default()
                .fg(Color::Rgb(255, 105, 180))
                .add_modifier(Modifier::BOLD)
        );
        todo.priority = Priority::Medium;
        assert_eq!(
            priority_style(&todo),
            Style::default()
                .fg(Color::LightBlue)
                .add_modifier(Modifier::BOLD)
        );
        todo.priority = Priority::Low;
        assert_eq!(
            priority_style(&todo),
            Style::default()
                .fg(Color::Rgb(173, 255, 47))
                .add_modifier(Modifier::BOLD)
        );
        assert_eq!(
            todo_style(&todo, today),
            Style::default().fg(Color::Rgb(173, 255, 47))
        );
        todo.checked = true;
        assert_eq!(
            todo_style(&todo, today),
            Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::CROSSED_OUT)
        );
        assert_eq!(priority_style(&todo), todo_style(&todo, today));
        assert_eq!(due_style(&todo, today), todo_style(&todo, today));
    }

    #[test]
    fn parses_source_pane_foreground_cwd() {
        let output =
            br#"{"result":{"process_info":{"foreground_processes":[{"cwd":"/tmp/project"}]}}}"#;
        assert_eq!(
            parse_foreground_cwd(output),
            Some(PathBuf::from("/tmp/project"))
        );
    }
}
