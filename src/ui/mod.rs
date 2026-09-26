mod effect;
mod message;
mod model;
mod update;
mod view;

use std::io::{self, stdout, IsTerminal};
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind, MouseButton, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::engine::ScanKind;

use self::effect::{Command, Task};
use self::message::Message;
use self::view::ListView;

pub fn run(kind: ScanKind) -> Result<()> {
    if !io::stdout().is_terminal() {
        anyhow::bail!("interactive mode needs a TTY; try `mac-cleaner list`");
    }

    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let run_result = event_loop(&mut terminal, kind);

    let _ = disable_raw_mode();
    let _ = execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    );
    let _ = terminal.show_cursor();
    run_result
}

fn event_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, kind: ScanKind) -> Result<()> {
    let (mut model, command) = update::init(kind);
    let mut list = ListView::default();
    let mut tasks: Vec<Task> = Vec::new();
    if !perform(command, &mut tasks) {
        return Ok(());
    }

    loop {
        terminal.draw(|frame| view::draw(frame, &model, &mut list))?;

        let mut messages = Vec::new();
        if event::poll(Duration::from_millis(80))? {
            messages.extend(to_message(event::read()?, &list));
        }
        tasks.retain(|task| task.drain(&mut messages));

        for message in messages {
            if !perform(update::update(&mut model, message), &mut tasks) {
                return Ok(());
            }
        }
    }
}

fn to_message(event: Event, list: &ListView) -> Option<Message> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => Some(Message::Key(key)),
        Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
            list.row_at(mouse.row).map(Message::Select)
        }
        _ => None,
    }
}

/// Runs a command. Returns false when the app should exit.
fn perform(command: Command, tasks: &mut Vec<Task>) -> bool {
    match command {
        Command::None => {}
        Command::Quit => return false,
        Command::Scan(kind) => tasks.push(effect::scan(kind)),
        Command::Delete { items, bytes } => tasks.push(effect::delete(items, bytes)),
    }
    true
}
