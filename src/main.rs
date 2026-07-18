use std::{
    fs,
    io::{self, stdout},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    },
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Terminal,
};
const DEFAULT_MESSAGE: &str =
    "j/k: move  Space: toggle  a: add  e: edit  d: delete  r: reload  q: quit";

#[derive(Debug, Clone, Copy, PartialEq)]
enum MessageKind {
    Default,
    Success,
    Warning,
    Error,
}
#[derive(Debug, Clone)]
struct Todo {
    checked: bool,
    text: String,
}

#[derive(Debug, PartialEq)]
enum InputMode {
    Normal,
    Add,
    Edit,
    ConfirmDelete,
}

struct App {
    todos: Vec<Todo>,
    selected: usize,
    todo_path: PathBuf,

    message: String,
    message_kind: MessageKind,
    message_until: Option<Instant>,

    input: String,
    input_mode: InputMode,
}

impl App {
    fn new(todo_path: PathBuf) -> Result<Self> {
    let todos = load_todos(&todo_path)?;

    Ok(Self {
        todos,
        selected: 0,
        todo_path,

        message: DEFAULT_MESSAGE.to_string(),
        message_kind: MessageKind::Default,
        message_until: None,

        input: String::new(),
        input_mode: InputMode::Normal,
    })
}

    fn move_down(&mut self) {
        if self.todos.is_empty() {
            return;
        }

        self.selected = (self.selected + 1).min(self.todos.len() - 1);
    }

    fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
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
        self.input.clear();
        self.input_mode = InputMode::Add;
        self.message = String::from("New TODO: Enter to save, Esc to cancel");
self.message_kind = MessageKind::Default;
self.message_until = None;
    }

fn start_edit(&mut self) {
    let Some(todo) = self.todos.get(self.selected) else {
        self.set_message("No TODO selected", MessageKind::Warning);
        return;
    };

    self.input = todo.text.clone();
    self.input_mode = InputMode::Edit;
    self.message = String::from("Edit TODO: Enter to save, Esc to cancel");
    self.message_kind = MessageKind::Default;
    self.message_until = None;
}

fn start_delete(&mut self) {
    if self.todos.is_empty() {
        self.set_message("No TODO selected", MessageKind::Warning);
        return;
    }

    self.input_mode = InputMode::ConfirmDelete;
    self.message = String::from("Delete selected TODO? y: yes  n/Esc: cancel");
    self.message_kind = MessageKind::Warning;
    self.message_until = None;
}

fn submit_input(&mut self) -> Result<()> {
    let text = self.input.trim().to_string();

    if text.is_empty() {
        self.set_message("TODO text cannot be empty", MessageKind::Warning);
        return Ok(());
    }

    match self.input_mode {
        InputMode::Add => {
            self.todos.push(Todo {
                checked: false,
                text,
            });

            self.selected = self.todos.len() - 1;
            self.set_message("TODO added", MessageKind::Success);
        }

        InputMode::Edit => {
            if let Some(todo) = self.todos.get_mut(self.selected) {
                todo.text = text;
                self.set_message("TODO updated", MessageKind::Success);
            }
        }

        _ => return Ok(()),
    }

    self.input.clear();
    self.input_mode = InputMode::Normal;
    self.save()?;

    Ok(())
}

fn delete_selected(&mut self) -> Result<()> {
    if self.todos.is_empty() {
        self.input_mode = InputMode::Normal;
        return Ok(());
    }

    self.todos.remove(self.selected);

    if self.selected >= self.todos.len() && self.selected > 0 {
        self.selected -= 1;
    }

    self.input_mode = InputMode::Normal;
    self.save()?;
    self.set_message("TODO deleted", MessageKind::Success);

    Ok(())
}

fn cancel_input(&mut self) {
    self.input.clear();
    self.input_mode = InputMode::Normal;
    self.set_message("Cancelled", MessageKind::Warning);
}

fn reload(&mut self) -> Result<()> {
    self.todos = load_todos(&self.todo_path)?;

    if self.todos.is_empty() {
        self.selected = 0;
    } else if self.selected >= self.todos.len() {
        self.selected = self.todos.len() - 1;
    }

    self.set_message("Reloaded TODO.md", MessageKind::Success);

    Ok(())
}

    fn save(&self) -> Result<()> {
        save_todos(&self.todo_path, &self.todos)
    }
    fn set_message(
    &mut self,
    message: impl Into<String>,
    kind: MessageKind,
) {
    self.message = message.into();
    self.message_kind = kind;
    self.message_until = Some(Instant::now() + Duration::from_secs(1));
}

fn set_default_message(&mut self) {
    self.message = DEFAULT_MESSAGE.to_string();
    self.message_kind = MessageKind::Default;
    self.message_until = None;
}

fn update_message_timeout(&mut self) {
    let Some(until) = self.message_until else {
        return;
    };

    if Instant::now() >= until {
        self.set_default_message();
    }
}
}

fn load_todos(path: &Path) -> Result<Vec<Todo>> {
    if !path.exists() {
        fs::write(path, "# TODO\n\n")
            .with_context(|| format!("failed to create {}", path.display()))?;
    }

    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    Ok(content.lines().filter_map(parse_todo_line).collect())
}

fn parse_todo_line(line: &str) -> Option<Todo> {
    let trimmed = line.trim();

    if let Some(text) = trimmed.strip_prefix("- [ ] ") {
        return Some(Todo {
            checked: false,
            text: text.to_string(),
        });
    }

    if let Some(text) = trimmed
        .strip_prefix("- [x] ")
        .or_else(|| trimmed.strip_prefix("- [X] "))
    {
        return Some(Todo {
            checked: true,
            text: text.to_string(),
        });
    }

    None
}

fn save_todos(path: &Path, todos: &[Todo]) -> Result<()> {
    let mut output = String::from("# TODO\n\n");

    for todo in todos {
        let mark = if todo.checked { "x" } else { " " };
        output.push_str(&format!("- [{mark}] {}\n", todo.text));
    }

    fs::write(path, output)
        .with_context(|| format!("failed to write {}", path.display()))?;

    Ok(())
}

fn draw(frame: &mut ratatui::Frame, app: &mut App) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .split(frame.area());

    let items: Vec<ListItem> = app
        .todos
        .iter()
        .map(|todo| {
            let mark = if todo.checked { "☑" } else { "☐" };

            let style = if todo.checked {
                Style::default().add_modifier(Modifier::CROSSED_OUT)
            } else {
                Style::default()
            };

            ListItem::new(Line::from(format!("{mark} {}", todo.text))).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(" 📋 TODO ")
                .borders(Borders::ALL),
        )
        .highlight_symbol("▶ ")
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    let mut state = ListState::default();

    if !app.todos.is_empty() {
        state.select(Some(app.selected));
    }

    frame.render_stateful_widget(list, areas[0], &mut state);

    let input_title = match app.input_mode {
        InputMode::Add => " Add TODO ",
        InputMode::Edit => " Edit TODO ",
        InputMode::ConfirmDelete => " Delete confirmation ",
        InputMode::Normal => " Input ",
    };

    let input_text = match app.input_mode {
        InputMode::ConfirmDelete => "Press y to delete, n or Esc to cancel",
        _ => app.input.as_str(),
    };

    let input = Paragraph::new(input_text)
        .block(Block::default().title(input_title).borders(Borders::ALL));

    frame.render_widget(input, areas[1]);

    let message_style = match app.message_kind {
    MessageKind::Default => Style::default(),
    MessageKind::Success => Style::default()
        .fg(Color::Green)
        .add_modifier(Modifier::BOLD),
    MessageKind::Warning => Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD),
    MessageKind::Error => Style::default()
        .fg(Color::Red)
        .add_modifier(Modifier::BOLD),
};

let status = Paragraph::new(app.message.as_str())
    .style(message_style)
    .block(Block::default().borders(Borders::ALL));

frame.render_widget(status, areas[2]);

    if matches!(app.input_mode, InputMode::Add | InputMode::Edit) {
        frame.set_cursor_position((
            areas[1].x + app.input.chars().count() as u16 + 1,
            areas[1].y + 1,
        ));
    }
}

fn handle_normal_mode(app: &mut App, code: KeyCode) -> Result<bool> {
    match code {
        KeyCode::Char('q') | KeyCode::Esc => return Ok(true),

        KeyCode::Char('j') | KeyCode::Down => app.move_down(),
        KeyCode::Char('k') | KeyCode::Up => app.move_up(),

        KeyCode::Char(' ') | KeyCode::Enter => {
            app.toggle_selected()?;
        }

        KeyCode::Char('a') => app.start_add(),
        KeyCode::Char('e') => app.start_edit(),
        KeyCode::Char('d') => app.start_delete(),

        KeyCode::Char('r') => {
            app.reload()?;
        }

        _ => {}
    }

    Ok(false)
}

fn handle_input_mode(app: &mut App, code: KeyCode) -> Result<()> {
    match code {
        KeyCode::Esc => app.cancel_input(),

        KeyCode::Enter => {
            app.submit_input()?;
        }

        KeyCode::Backspace => {
            app.input.pop();
        }

        KeyCode::Char(character) => {
            app.input.push(character);
        }

        _ => {}
    }

    Ok(())
}

fn handle_delete_mode(app: &mut App, code: KeyCode) -> Result<()> {
    match code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            app.delete_selected()?;
        }

        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.cancel_input();
        }

        _ => {}
    }

    Ok(())
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
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
                if handle_normal_mode(app, key.code)? {
                    return Ok(());
                }
            }

            InputMode::Add | InputMode::Edit => {
                handle_input_mode(app, key.code)?;
            }

            InputMode::ConfirmDelete => {
                handle_delete_mode(app, key.code)?;
            }
        }
    }
}

fn restore_terminal(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}

fn main() -> Result<()> {
    let todo_path = std::env::current_dir()
        .context("failed to determine current directory")?
        .join("TODO.md");

    let mut app = App::new(todo_path)?;

    enable_raw_mode()?;

    let mut output = stdout();
    execute!(output, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(output);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, &mut app);
    let restore_result = restore_terminal(&mut terminal);

    result?;
    restore_result?;

    Ok(())
}
