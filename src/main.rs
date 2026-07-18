use std::{
    env, fs,
    io::{self, stdout},
    path::{Path, PathBuf},
    time::{Duration, Instant},
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
    "j/k: move  a/e/d: add/edit/delete  D: clear done  S: save tag  C: create Local  ?: help";

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
    ConfirmDelete,
    ConfirmBulkDelete,
    Help,
}

struct App {
    todos: Vec<Todo>,
    other_todos: Vec<Todo>,
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
}

impl App {
    fn new(local_path: PathBuf, global_path: PathBuf) -> Result<Self> {
        ensure_todo_file(&global_path)?;
        let todos = load_todos(&local_path)?;
        let other_todos = load_todos(&global_path)?;
        Ok(Self {
            todos,
            other_todos,
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
        })
    }

    fn path(&self) -> &Path {
        match self.scope {
            Scope::Local => &self.local_path,
            Scope::Global => &self.global_path,
        }
    }

    fn scope_name(&self) -> &'static str {
        match self.scope {
            Scope::Local => "Local",
            Scope::Global => "Global",
        }
    }

    fn move_down(&mut self) {
        if !self.todos.is_empty() {
            self.selected = (self.selected + 1).min(self.todos.len() - 1);
        }
    }

    fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn toggle_scope(&mut self) -> Result<()> {
        std::mem::swap(&mut self.todos, &mut self.other_todos);
        std::mem::swap(&mut self.selected, &mut self.other_selected);
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

    fn start_edit(&mut self) {
        let Some(todo) = self.todos.get(self.selected) else {
            self.set_message("No TODO selected", MessageKind::Warning);
            return;
        };
        self.input = todo.text.clone();
        self.input_mode = InputMode::Edit;
        self.set_persistent_message("Edit TODO: Enter to save, Esc to cancel");
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
        if self.scope == Scope::Local {
            self.todos = load_todos(&self.local_path)?;
            self.selected = 0;
        } else {
            self.other_todos = load_todos(&self.local_path)?;
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
                let depth = self.todos.get(self.selected).map_or(0, |todo| todo.depth);
                self.todos.push(Todo {
                    checked: false,
                    text,
                    depth,
                    priority: Priority::None,
                    due: None,
                    saved: false,
                });
                self.selected = self.todos.len() - 1;
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
        while end < self.todos.len() && self.todos[end].depth > depth {
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
        for todo in self.todos.drain(..) {
            if skipped_depth.is_some_and(|depth| todo.depth > depth) {
                continue;
            }
            skipped_depth = None;
            if todo.checked {
                skipped_depth = Some(todo.depth);
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
            if self.selected == 0 {
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
        while end < self.todos.len() && self.todos[end].depth > old_depth {
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
        sort_siblings(&mut self.todos);
        self.selected = self.selected.min(self.todos.len().saturating_sub(1));
        self.save()?;
        self.set_message("Sorted by priority and due date", MessageKind::Success);
        Ok(())
    }

    fn cancel_input(&mut self) {
        self.input.clear();
        self.input_mode = InputMode::Normal;
        self.set_message("Cancelled", MessageKind::Warning);
    }

    fn reload(&mut self) -> Result<()> {
        self.todos = load_todos(self.path())?;
        self.selected = self.selected.min(self.todos.len().saturating_sub(1));
        Ok(())
    }

    fn save(&self) -> Result<()> {
        save_todos(self.path(), &self.todos)
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

fn load_todos(path: &Path) -> Result<Vec<Todo>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut todos = Vec::new();
    for line in content.lines() {
        if let Some(todo) = parse_todo_line(line) {
            todos.push(todo);
        } else if line.starts_with(char::is_whitespace)
            && !line.trim().is_empty()
            && let Some(todo) = todos.last_mut()
        {
            todo.text.push('\n');
            todo.text.push_str(line.trim());
        }
    }
    if remove_expired(&mut todos, Local::now().date_naive()) > 0 {
        save_todos(path, &todos)?;
    }
    Ok(todos)
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
    let (text, due) = match rest.rsplit_once(" 📅 ") {
        Some((text, date)) => match NaiveDate::parse_from_str(date, "%Y-%m-%d") {
            Ok(date) => (text.to_string(), Some(date)),
            Err(_) => (rest.to_string(), None),
        },
        None => (rest.to_string(), None),
    };
    Some(Todo {
        checked,
        text,
        depth,
        priority,
        due,
        saved,
    })
}

fn save_todos(path: &Path, todos: &[Todo]) -> Result<()> {
    let mut output = String::from("# TODO\n\n");
    for todo in todos {
        let mark = if todo.checked { "x" } else { " " };
        let priority = if todo.priority == Priority::None {
            String::new()
        } else {
            format!("[{}] ", todo.priority.label())
        };
        let due = todo
            .due
            .map(|date| format!(" 📅 {date}"))
            .unwrap_or_default();
        let saved = if todo.saved { "[SAVE] " } else { "" };
        let mut lines = todo.text.lines();
        let first_line = lines.next().unwrap_or_default();
        output.push_str(&format!(
            "{}- [{mark}] {priority}{saved}{first_line}{due}\n",
            "  ".repeat(todo.depth),
        ));
        for continuation in lines {
            output.push_str(&format!(
                "{}      {continuation}\n",
                "  ".repeat(todo.depth)
            ));
        }
    }
    fs::write(path, output).with_context(|| format!("failed to write {}", path.display()))
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
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut width = 0;
    for character in text.chars() {
        let character_width = character.width().unwrap_or(0);
        if width > 0 && width + character_width > max_width {
            lines.push(Line::from(std::mem::take(&mut current)));
            width = 0;
        }
        current.push(character);
        width += character_width;
    }
    lines.push(Line::from(current));
    lines
}

fn todo_style(todo: &Todo, today: NaiveDate) -> Style {
    if todo.checked {
        return Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::CROSSED_OUT);
    }
    if todo.due.is_some_and(|due| due < today) {
        return Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);
    }
    let tomorrow = today
        .checked_add_days(chrono::Days::new(1))
        .unwrap_or(NaiveDate::MAX);
    if todo.due.is_some_and(|due| due <= tomorrow) {
        return Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
    }
    Style::default()
}

fn render_todo_list(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    name: &str,
    path: &Path,
    todos: &[Todo],
    selected: usize,
    active: bool,
) {
    let available_width = area.width.saturating_sub(6).max(1) as usize;
    let today = Local::now().date_naive();
    let items: Vec<ListItem> = todos
        .iter()
        .map(|todo| {
            let mark = if todo.checked { "☑" } else { "☐" };
            let prefix = format!(
                "{}{} [{}]{} ",
                "  ".repeat(todo.depth),
                mark,
                todo.priority.label(),
                if todo.saved { " [SAVE]" } else { "" }
            );
            let due = todo
                .due
                .map(|date| format!("  📅 {date}"))
                .unwrap_or_default();
            let content = format!("{prefix}{}{due}", todo.text);
            let style = todo_style(todo, today);
            ListItem::new(wrap_display_width(&content, available_width)).style(style)
        })
        .collect();
    let marker = if active { "▶" } else { " " };
    let availability = if path.exists() {
        ""
    } else {
        " [not created: Shift+C]"
    };
    let border_style = Style::default().fg(if active { Color::Cyan } else { Color::DarkGray });
    let list = List::new(items)
        .block(
            Block::default()
                .title(format!(
                    " {marker} {name} TODO{availability} ({}) ",
                    path.display()
                ))
                .borders(Borders::ALL)
                .border_style(border_style),
        )
        .highlight_symbol("▶ ")
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));
    let mut state = ListState::default();
    if active && !todos.is_empty() {
        state.select(Some(selected));
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
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .split(frame.area());
    let (local_todos, local_selected, global_todos, global_selected) = match app.scope {
        Scope::Local => (
            &app.todos,
            app.selected,
            &app.other_todos,
            app.other_selected,
        ),
        Scope::Global => (
            &app.other_todos,
            app.other_selected,
            &app.todos,
            app.selected,
        ),
    };
    render_todo_list(
        frame,
        areas[0],
        "Local",
        &app.local_path,
        local_todos,
        local_selected,
        app.scope == Scope::Local,
    );
    render_todo_list(
        frame,
        areas[1],
        "Global",
        &app.global_path,
        global_todos,
        global_selected,
        app.scope == Scope::Global,
    );

    let input_title = match app.input_mode {
        InputMode::Add => " Add TODO ",
        InputMode::Edit => " Edit TODO ",
        InputMode::Due => " Due date ",
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
    frame.render_widget(
        Paragraph::new(input_text)
            .block(Block::default().title(input_title).borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        areas[2],
    );
    let message_style = match app.message_kind {
        MessageKind::Default => Style::default(),
        MessageKind::Success => Style::default().fg(Color::Green),
        MessageKind::Warning => Style::default().fg(Color::Yellow),
    }
    .add_modifier(Modifier::BOLD);
    frame.render_widget(
        Paragraph::new(app.message.as_str())
            .style(message_style)
            .block(Block::default().borders(Borders::ALL)),
        areas[3],
    );

    if matches!(app.input_mode, InputMode::Due) {
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
                        .border_style(Style::default().fg(Color::Cyan)),
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
                "Tab switch  j/k move  Space toggle  a/e/d edit  D clear  S save  C create Local",
            )]),
            Line::from("</> outdent/indent  p priority  s sort priority  t due date  r reload"),
            Line::from("Priority: P1 high, P2 medium, P3 low, -- unset"),
            Line::from("Enter/Esc closes this help"),
        ];
        frame.render_widget(
            Paragraph::new(help)
                .wrap(Wrap { trim: false })
                .block(Block::default().title(" Help ").borders(Borders::ALL)),
            popup,
        );
    }
}

fn handle_normal_mode(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => return Ok(true),
        KeyCode::Char('j') | KeyCode::Down => app.move_down(),
        KeyCode::Char('k') | KeyCode::Up => app.move_up(),
        KeyCode::Char(' ') | KeyCode::Enter => app.toggle_selected()?,
        KeyCode::Char('a') => app.start_add(),
        KeyCode::Char('e') => app.start_edit(),
        KeyCode::Char('d') => app.start_delete(),
        KeyCode::Char('D') => app.start_bulk_delete(),
        KeyCode::Char('S') => app.toggle_saved()?,
        KeyCode::Char('C') => app.create_local_file()?,
        KeyCode::Char('p') => app.cycle_priority()?,
        KeyCode::Char('s') => app.sort_by_priority()?,
        KeyCode::Char('t') => app.start_due(),
        KeyCode::Char('>') | KeyCode::Right => app.change_depth(true)?,
        KeyCode::Char('<') | KeyCode::Left => app.change_depth(false)?,
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
            InputMode::Add | InputMode::Edit | InputMode::Due => handle_input_mode(app, key)?,
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
        let todo = parse_todo_line("    - [x] [P1] [SAVE] ship it 📅 2026-07-18").unwrap();
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
    fn sorts_siblings_without_detaching_children() {
        let mut todos = vec![
            Todo {
                checked: false,
                text: "low".into(),
                depth: 0,
                priority: Priority::Low,
                due: None,
                saved: false,
            },
            Todo {
                checked: false,
                text: "child".into(),
                depth: 1,
                priority: Priority::High,
                due: None,
                saved: false,
            },
            Todo {
                checked: false,
                text: "high without due".into(),
                depth: 0,
                priority: Priority::High,
                due: None,
                saved: false,
            },
            Todo {
                checked: false,
                text: "high due first".into(),
                depth: 0,
                priority: Priority::High,
                due: NaiveDate::from_ymd_opt(2026, 7, 18),
                saved: false,
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
        let todos = load_todos(&path).unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!(todos[0].text, "first line\nsecond line");
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
            },
            Todo {
                checked: false,
                text: "child".into(),
                depth: 1,
                priority: Priority::None,
                due: None,
                saved: true,
            },
            Todo {
                checked: false,
                text: "protected".into(),
                depth: 0,
                priority: Priority::None,
                due: old_due,
                saved: true,
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
        assert!(load_todos(&path).unwrap().is_empty());
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
        };
        assert_eq!(
            todo_style(&todo, today),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        );
        todo.due = today.checked_sub_days(chrono::Days::new(1));
        assert_eq!(
            todo_style(&todo, today),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
        );
        todo.checked = true;
        assert_eq!(
            todo_style(&todo, today),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::CROSSED_OUT)
        );
    }
}
