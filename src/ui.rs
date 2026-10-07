use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Cell, Clear, Paragraph, Row, Sparkline, Table, Wrap};

use crate::app::{App, Mode, Tab};
use crate::platform;

const ACCENT: Color = Color::Cyan;
const DIM: Color = Color::DarkGray;

pub fn draw(f: &mut Frame, app: &mut App) {
    let [header, graphs, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(1),
    ])
    .areas(f.area());

    draw_header(f, app, header);
    draw_graphs(f, app, graphs);

    let (table_area, detail_area) = if app.show_details {
        if body.width >= 130 {
            let [a, b] = Layout::horizontal([Constraint::Min(60), Constraint::Length(56)]).areas(body);
            (a, Some(b))
        } else {
            let [a, b] = Layout::vertical([Constraint::Min(6), Constraint::Length(14)]).areas(body);
            (a, Some(b))
        }
    } else {
        (body, None)
    };
    app.table_area = table_area;

    match app.tab {
        Tab::Procs => draw_procs(f, app, table_area),
        Tab::Ports => draw_ports(f, app, table_area),
    }
    if let Some(d) = detail_area {
        draw_details(f, app, d);
    }
    draw_footer(f, app, footer);

    match &app.mode {
        Mode::Help => draw_help(f),
        Mode::Confirm(_) => draw_confirm(f, app),
        Mode::Env(_) => draw_env(f, app),
        _ => {}
    }
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let tab = |n: &str, label: &str, on: bool| {
        if on {
            Span::styled(format!(" {n} {label} "), Style::new().fg(Color::Black).bg(ACCENT).bold())
        } else {
            Span::styled(format!(" {n} {label} "), Style::new().fg(Color::Gray))
        }
    };
    let left = Line::from(vec![
        Span::styled(" deep ", Style::new().fg(Color::Black).bg(Color::Magenta).bold()),
        Span::raw(" "),
        tab("1", "Processes", app.tab == Tab::Procs),
        tab("2", "Ports", app.tab == Tab::Ports),
    ]);
    let admin = match app.elevated {
        Some(true) => Span::styled(" ADMIN ", Style::new().fg(Color::Black).bg(Color::Green)),
        Some(false) => Span::styled(" not elevated ", Style::new().fg(Color::Yellow)),
        None => Span::raw(""),
    };
    let listening = app.snap.sockets.iter().filter(|s| s.listening).count();
    let mut right = vec![
        Span::styled(
            format!(
                "{} procs  {} listening  {} sockets  up {}  ",
                app.snap.procs.len(),
                listening,
                app.snap.sockets.len(),
                fmt_dur(app.snap.uptime)
            ),
            Style::new().fg(Color::Gray),
        ),
        admin,
    ];
    if app.paused {
        right.insert(0, Span::styled(" PAUSED ", Style::new().fg(Color::Black).bg(Color::Yellow)));
    }
    f.render_widget(Paragraph::new(left), area);
    f.render_widget(Paragraph::new(Line::from(right)).alignment(Alignment::Right), area);
}

fn draw_graphs(f: &mut Frame, app: &App, area: Rect) {
    let [l, r] = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(area);
    let s = &app.snap;

    let cpu_color = load_color(s.cpu as f64);
    graph(
        f,
        l,
        Line::from(vec![
            Span::styled(" CPU ", Style::new().bold()),
            Span::styled(format!("{:5.1}%", s.cpu), Style::new().fg(cpu_color).bold()),
            Span::styled(format!("  {} cores", s.ncpu), Style::new().fg(DIM)),
        ]),
        &app.cpu_hist,
        cpu_color,
    );

    let mem_pct = if s.mem_total > 0 { s.mem_used as f64 * 100.0 / s.mem_total as f64 } else { 0.0 };
    let mut label = vec![
        Span::styled(" MEM ", Style::new().bold()),
        Span::styled(format!("{mem_pct:5.1}%"), Style::new().fg(load_color(mem_pct)).bold()),
        Span::styled(format!("  {} / {}", fmt_bytes(s.mem_used), fmt_bytes(s.mem_total)), Style::new().fg(DIM)),
    ];
    if s.swap_total > 0 {
        label.push(Span::styled(
            format!("  swap {} / {}", fmt_bytes(s.swap_used), fmt_bytes(s.swap_total)),
            Style::new().fg(DIM),
        ));
    }
    graph(f, r, Line::from(label), &app.mem_hist, Color::Magenta);
}

fn graph(f: &mut Frame, area: Rect, label: Line, hist: &std::collections::VecDeque<u64>, color: Color) {
    let [top, spark] = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(area);
    f.render_widget(Paragraph::new(label), top);
    let w = spark.width.saturating_sub(1) as usize;
    let data: Vec<u64> = hist.iter().rev().take(w).rev().copied().collect();
    let pad = w.saturating_sub(data.len());
    let data: Vec<u64> = std::iter::repeat_n(0, pad).chain(data).collect();
    f.render_widget(
        Sparkline::default().data(&data).max(100).style(Style::new().fg(color)),
        Rect { x: spark.x + 1, width: spark.width.saturating_sub(2), ..spark },
    );
}

fn panel(title: impl Into<Line<'static>>) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(DIM))
        .title(title)
}

fn header_cell(label: &str, active: bool, desc: bool) -> Cell<'static> {
    if active {
        Cell::from(format!("{label} {}", if desc { "▼" } else { "▲" })).style(Style::new().fg(ACCENT).bold())
    } else {
        Cell::from(label.to_string())
    }
}

fn draw_procs(f: &mut Frame, app: &mut App, area: Rect) {
    use crate::app::ProcSort as S;
    let (sort, desc) = (app.proc_sort, app.proc_desc);
    let h = |label, s| header_cell(label, sort == s, desc);
    let header = Row::new(vec![
        h("PID", S::Pid),
        h("Name", S::Name),
        h("CPU%", S::Cpu),
        h("Memory", S::Mem),
        h("Disk/s", S::Disk),
        h("Ports", S::Net),
        h("User", S::User),
        Cell::from("Path"),
    ])
    .style(Style::new().fg(Color::Gray).add_modifier(Modifier::UNDERLINED));

    let rows: Vec<Row> = app
        .proc_rows
        .iter()
        .filter_map(|r| app.snap.procs.get(&r.pid).map(|p| (r, p)))
        .map(|(r, p)| {
            let disk = p.read_rate + p.write_rate;
            let ports = match (p.listening, p.connections) {
                (0, 0) => Span::raw(""),
                (l, 0) => Span::styled(format!("{l}L"), Style::new().fg(Color::Green)),
                (0, c) => Span::styled(format!("{c}C"), Style::new().fg(Color::Blue)),
                (l, c) => Span::styled(format!("{l}L {c}C"), Style::new().fg(Color::Green)),
            };
            let critical = platform::is_critical(p.pid, &p.name);
            let name_style = if p.pid == app.self_pid {
                Style::new().fg(Color::Magenta)
            } else if critical {
                Style::new().fg(Color::Red)
            } else {
                Style::new().fg(Color::White)
            };
            Row::new(vec![
                Cell::from(p.pid.to_string()).style(Style::new().fg(DIM)),
                Cell::from(Line::from(vec![
                    Span::styled(r.prefix.clone(), Style::new().fg(DIM)),
                    Span::styled(p.name.clone(), name_style),
                ])),
                Cell::from(format!("{:5.1}", p.cpu)).style(Style::new().fg(load_color(p.cpu as f64 * 2.0))),
                Cell::from(fmt_bytes(p.mem)).style(Style::new().fg(mem_color(p.mem))),
                Cell::from(if disk >= 1.0 { fmt_rate(disk) } else { String::new() }).style(Style::new().fg(Color::Yellow)),
                Cell::from(ports),
                Cell::from(short_user(&p.user)).style(Style::new().fg(DIM)),
                Cell::from(p.exe.as_ref().map(|e| e.display().to_string()).unwrap_or_default())
                    .style(Style::new().fg(DIM)),
            ])
        })
        .collect();

    let mut title = vec![Span::styled(
        if app.tree { " Process tree " } else { " Processes " },
        Style::new().fg(ACCENT).bold(),
    )];
    title.push(Span::styled(format!("{} ", app.proc_rows.len()), Style::new().fg(DIM)));
    if !app.proc_filter.is_empty() {
        title.push(Span::styled(format!("filter: {} ", app.proc_filter), Style::new().fg(Color::Yellow)));
    }

    let name_w = if app.tree { 34 } else { 24 };
    let table = Table::new(
        rows,
        [
            Constraint::Length(7),
            Constraint::Min(name_w),
            Constraint::Length(7),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(7),
            Constraint::Length(12),
            Constraint::Fill(1),
        ],
    )
    .header(header)
    .block(panel(Line::from(title)))
    .row_highlight_style(Style::new().bg(Color::Rgb(40, 50, 70)).bold())
    .highlight_symbol("▌");
    f.render_stateful_widget(table, area, &mut app.proc_state);
}

fn draw_ports(f: &mut Frame, app: &mut App, area: Rect) {
    use crate::app::PortSort as S;
    let (sort, desc) = (app.port_sort, app.port_desc);
    let h = |label, s| header_cell(label, sort == s, desc);
    let header = Row::new(vec![
        h("Proto", S::Proto),
        Cell::from("Local address"),
        h("Port", S::Port),
        Cell::from("Remote"),
        h("State", S::State),
        h("PID", S::Pid),
        h("Process", S::Process),
        Cell::from("Path"),
    ])
    .style(Style::new().fg(Color::Gray).add_modifier(Modifier::UNDERLINED));

    let rows: Vec<Row> = app
        .port_rows
        .iter()
        .map(|&i| {
            let s = &app.snap.sockets[i];
            let p = s.pid.and_then(|pid| app.snap.procs.get(&pid));
            let state_style = if s.listening {
                Style::new().fg(Color::Green)
            } else if s.state.eq_ignore_ascii_case("established") {
                Style::new().fg(Color::Blue)
            } else {
                Style::new().fg(DIM)
            };
            let state = if s.proto == crate::data::Proto::Udp { "(bound)".to_string() } else { s.state.clone() };
            Row::new(vec![
                Cell::from(s.proto.as_str()).style(Style::new().fg(DIM)),
                Cell::from(s.local.to_string()),
                Cell::from(s.lport.to_string()).style(Style::new().fg(Color::Yellow).bold()),
                Cell::from(s.remote.map(|(a, p)| fmt_addr(a, p)).unwrap_or_default()),
                Cell::from(state).style(state_style),
                Cell::from(s.pid.map(|p| p.to_string()).unwrap_or_else(|| "-".into())).style(Style::new().fg(DIM)),
                Cell::from(p.map(|p| p.name.clone()).unwrap_or_default()),
                Cell::from(p.and_then(|p| p.exe.as_ref()).map(|e| e.display().to_string()).unwrap_or_default())
                    .style(Style::new().fg(DIM)),
            ])
        })
        .collect();

    let mut title = vec![
        Span::styled(" Ports ", Style::new().fg(ACCENT).bold()),
        Span::styled(format!("{} ", app.port_rows.len()), Style::new().fg(DIM)),
    ];
    if app.listen_only {
        title.push(Span::styled("listening only ", Style::new().fg(Color::Green)));
    }
    if !app.port_filter.is_empty() {
        title.push(Span::styled(format!("filter: {} ", app.port_filter), Style::new().fg(Color::Yellow)));
    }
    if let Some(e) = &app.snap.sock_err {
        title.push(Span::styled(format!("error: {e} "), Style::new().fg(Color::Red)));
    }

    let table = Table::new(
        rows,
        [
            Constraint::Length(5),
            Constraint::Length(18),
            Constraint::Length(7),
            Constraint::Length(24),
            Constraint::Length(12),
            Constraint::Length(7),
            Constraint::Length(20),
            Constraint::Fill(1),
        ],
    )
    .header(header)
    .block(panel(Line::from(title)))
    .row_highlight_style(Style::new().bg(Color::Rgb(40, 50, 70)).bold())
    .highlight_symbol("▌");
    f.render_stateful_widget(table, area, &mut app.port_state);
}

fn kv(k: &str, v: impl Into<String>) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{k:<11}"), Style::new().fg(DIM)),
        Span::raw(v.into()),
    ])
}

fn section(title: &str) -> Line<'static> {
    Line::from(Span::styled(format!("─ {title} "), Style::new().fg(ACCENT).bold()))
}

fn draw_details(f: &mut Frame, app: &App, area: Rect) {
    let block = panel(Line::from(Span::styled(" Details ", Style::new().fg(ACCENT).bold())));
    let Some(p) = app.focused_proc() else {
        let msg = if app.tab == Tab::Ports && app.selected_socket().is_some() {
            "This socket has no owning process\n(system socket or TIME_WAIT)."
        } else {
            "Nothing selected"
        };
        f.render_widget(Paragraph::new(msg).fg(DIM).block(block), area);
        return;
    };
    let s = &app.snap;
    let denied = || Span::styled("access denied", Style::new().fg(DIM).italic());
    let path_line = |label: &str, v: &Option<std::path::PathBuf>| match v {
        Some(x) => kv(label, x.display().to_string()),
        None => Line::from(vec![Span::styled(format!("{label:<11}"), Style::new().fg(DIM)), denied()]),
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled(p.name.clone(), Style::new().fg(Color::White).bold()),
            Span::styled(format!("  pid {}", p.pid), Style::new().fg(DIM)),
            if platform::is_critical(p.pid, &p.name) {
                Span::styled("  CRITICAL", Style::new().fg(Color::Red).bold())
            } else {
                Span::raw("")
            },
        ]),
        kv("Status", p.status.clone()),
        kv("User", if p.user.is_empty() { "-".into() } else { p.user.clone() }),
        kv("Running", fmt_dur(p.run_time)),
        kv("CPU", format!("{:.1}%", p.cpu)),
        kv("Memory", format!("{}  (virtual {})", fmt_bytes(p.mem), fmt_bytes(p.vmem))),
        kv(
            "Disk",
            format!("R {}  W {}", fmt_rate(p.read_rate), fmt_rate(p.write_rate)),
        ),
        kv(
            "Disk total",
            format!("R {}  W {}", fmt_bytes(p.total_read), fmt_bytes(p.total_written)),
        ),
        section("Location"),
        path_line("Exe", &p.exe),
        path_line("Cwd", &p.cwd),
        Line::from(Span::styled("Command line", Style::new().fg(DIM))),
        if p.cmd.is_empty() { Line::from(denied()) } else { Line::from(p.cmd.clone()).fg(Color::Gray) },
    ];

    lines.push(section("Lineage"));
    let chain = s.ancestors(p.pid);
    if chain.is_empty() {
        let note = match p.ppid {
            Some(pp) if pp != p.pid => format!("parent {pp} (exited)"),
            _ => "no parent".into(),
        };
        lines.push(Line::from(Span::styled(note, Style::new().fg(DIM))));
    }
    for (depth, pid) in chain.iter().enumerate() {
        let name = s.procs.get(pid).map(|q| q.name.as_str()).unwrap_or("?");
        lines.push(Line::from(vec![
            Span::styled(format!("{}{}", "  ".repeat(depth), if depth > 0 { "└ " } else { "" }), Style::new().fg(DIM)),
            Span::raw(name.to_string()),
            Span::styled(format!(" {pid}"), Style::new().fg(DIM)),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled(
            format!("{}{}", "  ".repeat(chain.len()), if chain.is_empty() { "" } else { "└ " }),
            Style::new().fg(DIM),
        ),
        Span::styled(p.name.clone(), Style::new().fg(ACCENT).bold()),
        Span::styled(format!(" {}", p.pid), Style::new().fg(DIM)),
    ]));
    let tree = s.tree_pids(p.pid);
    let kids = s.children_map().get(&p.pid).map(|v| v.len()).unwrap_or(0);
    if tree.len() > 1 {
        let tree_mem: u64 = tree.iter().filter_map(|x| s.procs.get(x)).map(|q| q.mem).sum();
        let tree_cpu: f32 = tree.iter().filter_map(|x| s.procs.get(x)).map(|q| q.cpu).sum();
        lines.push(kv(
            "Children",
            format!("{kids} direct, {} total  (tree: {:.1}% CPU, {})", tree.len() - 1, tree_cpu, fmt_bytes(tree_mem)),
        ));
    } else {
        lines.push(kv("Children", "none"));
    }

    let socks: Vec<_> = s.sockets.iter().filter(|x| x.pid == Some(p.pid)).collect();
    lines.push(section(&format!("Network ({} sockets)", socks.len())));
    let mut listen: Vec<String> = socks
        .iter()
        .filter(|x| x.listening)
        .map(|x| format!("{} {}", x.proto.as_str(), fmt_addr(x.local, x.lport)))
        .collect();
    listen.sort();
    listen.dedup();
    for l in listen.iter().take(8) {
        lines.push(Line::from(vec![Span::styled("listen ", Style::new().fg(Color::Green)), Span::raw(l.clone())]));
    }
    if listen.len() > 8 {
        lines.push(Line::from(Span::styled(format!("… {} more (press p)", listen.len() - 8), Style::new().fg(DIM))));
    }
    let conns: Vec<_> = socks.iter().filter(|x| x.remote.is_some() && !x.listening).collect();
    for c in conns.iter().take(6) {
        let (ra, rp) = c.remote.unwrap();
        lines.push(Line::from(vec![
            Span::styled("conn   ", Style::new().fg(Color::Blue)),
            Span::raw(format!(":{} → {}", c.lport, fmt_addr(ra, rp))),
            Span::styled(format!(" {}", c.state), Style::new().fg(DIM)),
        ]));
    }
    if conns.len() > 6 {
        lines.push(Line::from(Span::styled(format!("… {} more (press p)", conns.len() - 6), Style::new().fg(DIM))));
    }

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(block), area);
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    if matches!(app.mode, Mode::Filter) {
        let filter = match app.tab {
            Tab::Procs => &app.proc_filter,
            Tab::Ports => &app.port_filter,
        };
        let hint = match app.tab {
            Tab::Procs => "  name, pid, path, user, or port:8080",
            Tab::Ports => "  port number, address, state, process, or pid:1234",
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" / ", Style::new().fg(Color::Black).bg(Color::Yellow)),
                Span::raw(format!(" {filter}")),
                Span::styled("█", Style::new().fg(Color::Yellow)),
                Span::styled(hint, Style::new().fg(DIM)),
            ])),
            area,
        );
        return;
    }
    if let Some((msg, err, _)) = &app.status {
        let style = if *err { Style::new().fg(Color::Red) } else { Style::new().fg(Color::Green) };
        f.render_widget(Paragraph::new(Span::styled(format!(" {msg}"), style)), area);
        return;
    }
    let mut keys: Vec<(&str, &str)> = vec![("k", "kill"), ("K", "kill tree"), ("/", "filter")];
    match app.tab {
        Tab::Procs => keys.extend([("t", "tree"), ("p", "ports"), ("u", "parent")]),
        Tab::Ports => keys.extend([("l", "listening"), ("⏎", "go to process")]),
    }
    keys.extend([("o", "open location"), ("c", "copy path"), ("e", "env"), ("s", "sort"), ("?", "help"), ("q", "quit")]);
    let spans: Vec<Span> = keys
        .into_iter()
        .flat_map(|(k, v)| {
            [
                Span::styled(format!(" {k} "), Style::new().fg(Color::Black).bg(Color::Gray)),
                Span::styled(format!(" {v} "), Style::new().fg(Color::Gray)),
            ]
        })
        .collect();
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width.saturating_sub(2));
    let h = h.min(area.height.saturating_sub(2));
    Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    }
}

fn draw_help(f: &mut Frame) {
    let rows: &[(&str, &str)] = &[
        ("", "Anywhere"),
        ("1 / 2 / Tab", "switch Processes / Ports"),
        ("↑ ↓ PgUp PgDn Home End", "move (mouse wheel + click work too)"),
        ("/", "filter as you type (Esc clears)"),
        ("k / Del", "kill selected process"),
        ("K / Shift+Del", "kill process and its whole tree"),
        ("o", "open file location in Explorer"),
        ("c / C / i", "copy exe path / command line / PID"),
        ("e", "view environment variables"),
        ("u", "jump to parent process"),
        ("d", "toggle details pane"),
        ("s / r", "cycle sort column / reverse"),
        ("Space", "pause refreshing"),
        ("+ / -", "slower / faster refresh"),
        ("F5", "refresh now"),
        ("q / Ctrl+C", "quit"),
        ("", "Processes"),
        ("t", "toggle tree view"),
        ("← → (h l)", "collapse / expand in tree"),
        ("p / Enter", "show this process's ports"),
        ("filter port:8080", "processes using port 8080"),
        ("", "Ports"),
        ("l", "listening sockets only"),
        ("Enter / p", "jump to owning process"),
        ("filter 443", "everything on port 443"),
        ("filter pid:1234", "sockets of one process"),
    ];
    let lines: Vec<Line> = rows
        .iter()
        .map(|(k, v)| {
            if k.is_empty() {
                Line::from(Span::styled(format!(" {v}"), Style::new().fg(ACCENT).bold()))
            } else {
                Line::from(vec![
                    Span::styled(format!("  {k:<24}"), Style::new().fg(Color::Yellow)),
                    Span::raw(v.to_string()),
                ])
            }
        })
        .collect();
    let area = centered(f.area(), 70, lines.len() as u16 + 2);
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(lines).block(panel(" Keys — any key to close ").border_style(Style::new().fg(ACCENT))),
        area,
    );
}

fn draw_confirm(f: &mut Frame, app: &App) {
    let Mode::Confirm(c) = &app.mode else { return };
    let mut lines = vec![
        Line::from(""),
        Line::from(vec![Span::raw(" Kill "), Span::styled(c.label.clone(), Style::new().bold())]),
    ];
    if c.pids.len() > 1 {
        let names: Vec<String> = c
            .pids
            .iter()
            .skip(1)
            .take(6)
            .filter_map(|p| app.snap.procs.get(p))
            .map(|p| format!("{} ({})", p.name, p.pid))
            .collect();
        let more = if c.pids.len() > 7 { format!(", … +{}", c.pids.len() - 7) } else { String::new() };
        lines.push(Line::from(Span::styled(format!(" incl. {}{more}", names.join(", ")), Style::new().fg(DIM))));
    }
    if !c.critical.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!(" WARNING: critical system process: {}", c.critical.join(", ")),
            Style::new().fg(Color::Red).bold(),
        )));
        lines.push(Line::from(Span::styled(
            " Killing it can crash or log out the system.",
            Style::new().fg(Color::Red),
        )));
    }
    if c.includes_self {
        lines.push(Line::from(Span::styled(
            " This tree includes deep itself, so deep will exit too.",
            Style::new().fg(Color::Yellow),
        )));
    }
    lines.push(Line::from(""));
    lines.push(if c.critical.is_empty() {
        Line::from(vec![
            Span::styled(" y ", Style::new().fg(Color::Black).bg(Color::Red)),
            Span::raw(" kill   "),
            Span::styled(" any other key ", Style::new().fg(Color::Black).bg(Color::Gray)),
            Span::raw(" cancel"),
        ])
    } else {
        Line::from(vec![
            Span::styled(" Shift+Y ", Style::new().fg(Color::Black).bg(Color::Red)),
            Span::raw(" kill anyway   "),
            Span::styled(" any other key ", Style::new().fg(Color::Black).bg(Color::Gray)),
            Span::raw(" cancel"),
        ])
    });
    let area = centered(f.area(), 72, lines.len() as u16 + 3);
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(panel(" Confirm kill ").border_style(Style::new().fg(Color::Red))),
        area,
    );
}

fn draw_env(f: &mut Frame, app: &App) {
    let Mode::Env(e) = &app.mode else { return };
    let area = centered(f.area(), f.area().width.saturating_sub(8), f.area().height.saturating_sub(4));
    let lines: Vec<Line> = e
        .vars
        .iter()
        .skip(e.scroll)
        .map(|v| match v.split_once('=') {
            Some((k, val)) => Line::from(vec![
                Span::styled(k.to_string(), Style::new().fg(Color::Yellow)),
                Span::styled("=", Style::new().fg(DIM)),
                Span::raw(val.to_string()),
            ]),
            None => Line::from(v.clone()),
        })
        .collect();
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            panel(format!(
                " Environment: {} ({}), {} vars   ↑↓ scroll  c copy all  Esc close ",
                e.name,
                e.pid,
                e.vars.len()
            ))
            .border_style(Style::new().fg(ACCENT)),
        ),
        area,
    );
}

// ---------- formatting ----------

pub fn fmt_bytes(b: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{b} B") } else { format!("{v:.1} {}", U[i]) }
}

fn fmt_rate(r: f64) -> String {
    format!("{}/s", fmt_bytes(r as u64))
}

pub fn fmt_dur(s: u64) -> String {
    let (d, h, m, sec) = (s / 86400, s / 3600 % 24, s / 60 % 60, s % 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {sec}s")
    } else {
        format!("{sec}s")
    }
}

fn fmt_addr(a: std::net::IpAddr, p: u16) -> String {
    match a {
        std::net::IpAddr::V6(_) => format!("[{a}]:{p}"),
        _ => format!("{a}:{p}"),
    }
}

/// "DESKTOP-123\\bob" -> "bob"
fn short_user(u: &str) -> String {
    u.rsplit('\\').next().unwrap_or(u).to_string()
}

fn load_color(pct: f64) -> Color {
    if pct >= 80.0 {
        Color::Red
    } else if pct >= 40.0 {
        Color::Yellow
    } else {
        Color::Green
    }
}

fn mem_color(b: u64) -> Color {
    const GB: u64 = 1 << 30;
    if b >= 2 * GB {
        Color::Red
    } else if b >= 500 << 20 {
        Color::Yellow
    } else {
        Color::Gray
    }
}
