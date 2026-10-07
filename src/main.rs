mod app;
mod data;
mod platform;
mod ui;

use std::io;
use std::time::Duration;

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind};
use crossterm::execute;

fn main() -> anyhow::Result<()> {
    eprintln!("deep: reading processes...");
    let mut app = app::App::new();

    let mut terminal = ratatui::init();
    execute!(io::stdout(), EnableMouseCapture)?;
    let res = run(&mut terminal, &mut app);
    execute!(io::stdout(), DisableMouseCapture)?;
    ratatui::restore();
    res
}

fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut app::App) -> anyhow::Result<()> {
    while app.running {
        terminal.draw(|f| ui::draw(f, app))?;
        let wait = app
            .interval
            .saturating_sub(app.last_refresh.elapsed())
            .clamp(Duration::from_millis(50), Duration::from_millis(500));
        if event::poll(wait)? {
            match event::read()? {
                // Windows also reports key releases; only act on presses.
                Event::Key(k) if k.kind != KeyEventKind::Release => app.on_key(k),
                Event::Mouse(m) => app.on_mouse(m),
                _ => {}
            }
        }
        app.tick();
    }
    Ok(())
}
