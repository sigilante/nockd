//! Terminal dashboard (DESIGN §9) — a stand-in until the web front-end lands. Polls the
//! same control API the CLI uses; the daemon has no idea it exists.
//!
//! Keys: ↑/↓ (or j/k) select · r restart · s stop · x start · q quit.

use std::io;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap};

use crate::api::AppStatus;
use crate::client::Client;

struct State {
    apps: Vec<AppStatus>,
    selected: usize,
    logs: String,
    status_msg: String,
}

pub async fn run(host: &str, port: u16) -> Result<()> {
    let client = Client::new(host, port);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

    let result = event_loop(&mut terminal, &client).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

async fn event_loop<B: Backend>(terminal: &mut Terminal<B>, client: &Client) -> Result<()> {
    let mut state = State {
        apps: Vec::new(),
        selected: 0,
        logs: String::new(),
        status_msg: "connecting…".into(),
    };
    let mut last_poll = Instant::now()
        .checked_sub(Duration::from_secs(10))
        .unwrap_or_else(Instant::now);

    loop {
        if last_poll.elapsed() >= Duration::from_secs(1) {
            refresh(client, &mut state).await;
            last_poll = Instant::now();
        }

        terminal.draw(|f| ui(f, &state))?;

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Down | KeyCode::Char('j') => {
                        if !state.apps.is_empty() {
                            state.selected = (state.selected + 1).min(state.apps.len() - 1);
                        }
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        state.selected = state.selected.saturating_sub(1);
                    }
                    KeyCode::Char('r') => action(client, &mut state, "restart").await,
                    KeyCode::Char('s') => action(client, &mut state, "stop").await,
                    KeyCode::Char('x') => action(client, &mut state, "start").await,
                    _ => {}
                }
                if matches!(key.code, KeyCode::Char('r' | 's' | 'x')) {
                    last_poll = Instant::now()
                        .checked_sub(Duration::from_secs(10))
                        .unwrap_or_else(Instant::now);
                }
            }
        }
    }
}

async fn refresh(client: &Client, state: &mut State) {
    match client.list().await {
        Ok(apps) => {
            state.apps = apps;
            state.status_msg.clear();
        }
        Err(e) => {
            state.status_msg = format!("daemon unreachable: {e}");
            return;
        }
    }
    if state.apps.is_empty() {
        state.logs.clear();
        return;
    }
    if state.selected >= state.apps.len() {
        state.selected = state.apps.len() - 1;
    }
    let name = state.apps[state.selected].name.clone();
    state.logs = client.logs(&name, 400).await.unwrap_or_default();
}

async fn action(client: &Client, state: &mut State, verb: &str) {
    let Some(app) = state.apps.get(state.selected) else {
        return;
    };
    let name = app.name.clone();
    match client.action(&name, verb).await {
        Ok(()) => state.status_msg = format!("{verb} {name}"),
        Err(e) => state.status_msg = format!("{verb} {name} failed: {e}"),
    }
}

fn ui(f: &mut Frame, state: &State) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(6),
            Constraint::Percentage(45),
            Constraint::Length(1),
        ])
        .split(f.area());

    // Title.
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("nockd", Style::new().fg(Color::Cyan).bold()),
            Span::raw("  NockApp deployment — fleet"),
        ])),
        chunks[0],
    );

    // Fleet table.
    let header = Row::new(["app", "state", "health", "pid", "restarts", "endpoint", "metric"])
        .style(Style::new().fg(Color::DarkGray).bold());
    let rows = state.apps.iter().enumerate().map(|(i, a)| {
        let (st, health, pid, restarts) = match &a.runtime {
            Some(rt) => (
                format!("{:?}", rt.state).to_lowercase(),
                format!("{:?}", rt.health).to_lowercase(),
                rt.pid.map(|p| p.to_string()).unwrap_or_else(|| "—".into()),
                rt.restarts.to_string(),
            ),
            None => (a.desired_status.clone(), "unknown".into(), "—".into(), "0".into()),
        };
        let state_color = match st.as_str() {
            "running" => Color::Green,
            "crashed" | "backoff" => Color::Red,
            _ => Color::Gray,
        };
        let metric = a
            .runtime
            .as_ref()
            .and_then(|rt| rt.status_line.clone())
            .map(|line| {
                let label = a.status_label.as_deref().unwrap_or("").trim();
                if label.is_empty() { line } else { format!("{label} {line}") }
            })
            .unwrap_or_else(|| "—".into());
        let cells = vec![
            Cell::from(a.name.clone()),
            Cell::from(st).style(Style::new().fg(state_color)),
            Cell::from(health),
            Cell::from(pid),
            Cell::from(restarts),
            Cell::from(a.endpoint.clone().unwrap_or_else(|| "—".into())),
            Cell::from(metric),
        ];
        let row = Row::new(cells);
        if i == state.selected {
            row.style(Style::new().bg(Color::Rgb(20, 30, 42)).bold())
        } else {
            row
        }
    });
    let widths = [
        Constraint::Length(18),
        Constraint::Length(10),
        Constraint::Length(12),
        Constraint::Length(8),
        Constraint::Length(9),
        Constraint::Length(18),
        Constraint::Min(16),
    ];
    f.render_widget(
        Table::new(rows, widths)
            .header(header)
            .block(Block::default().borders(Borders::ALL).title(" apps ")),
        chunks[1],
    );

    // Logs for the selected app.
    let log_title = state
        .apps
        .get(state.selected)
        .map(|a| format!(" logs — {} ", a.name))
        .unwrap_or_else(|| " logs ".into());
    let logs = if state.logs.is_empty() {
        "(no output yet)".to_string()
    } else {
        state.logs.clone()
    };
    f.render_widget(
        Paragraph::new(logs)
            .block(Block::default().borders(Borders::ALL).title(log_title))
            .wrap(Wrap { trim: false })
            .scroll((log_scroll(&state.logs, chunks[2].height), 0)),
        chunks[2],
    );

    // Footer.
    let help = "↑/↓ select   r restart   s stop   x start   q quit";
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(help, Style::new().fg(Color::DarkGray)),
            Span::raw("   "),
            Span::styled(state.status_msg.clone(), Style::new().fg(Color::Yellow)),
        ])),
        chunks[3],
    );
}

/// Scroll so the tail of the log is visible in a pane of `height` rows.
fn log_scroll(logs: &str, height: u16) -> u16 {
    let lines = logs.lines().count() as u16;
    let visible = height.saturating_sub(2); // borders
    lines.saturating_sub(visible)
}
