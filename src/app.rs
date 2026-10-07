use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::widgets::TableState;

use crate::data::{Collector, Proc, Snapshot, Socket};
use crate::platform;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Procs,
    Ports,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProcSort {
    Cpu,
    Mem,
    Disk,
    Pid,
    Name,
    Net,
    User,
}

impl ProcSort {
    const ALL: [ProcSort; 7] = [
        ProcSort::Cpu,
        ProcSort::Mem,
        ProcSort::Disk,
        ProcSort::Pid,
        ProcSort::Name,
        ProcSort::Net,
        ProcSort::User,
    ];
    pub fn label(self) -> &'static str {
        match self {
            ProcSort::Cpu => "CPU",
            ProcSort::Mem => "Memory",
            ProcSort::Disk => "Disk",
            ProcSort::Pid => "PID",
            ProcSort::Name => "Name",
            ProcSort::Net => "Ports",
            ProcSort::User => "User",
        }
    }
    /// Text columns read naturally ascending, numbers descending.
    fn default_desc(self) -> bool {
        !matches!(self, ProcSort::Pid | ProcSort::Name | ProcSort::User)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PortSort {
    Port,
    Proto,
    State,
    Pid,
    Process,
}

impl PortSort {
    const ALL: [PortSort; 5] = [
        PortSort::Port,
        PortSort::Proto,
        PortSort::State,
        PortSort::Pid,
        PortSort::Process,
    ];
    pub fn label(self) -> &'static str {
        match self {
            PortSort::Port => "Port",
            PortSort::Proto => "Proto",
            PortSort::State => "State",
            PortSort::Pid => "PID",
            PortSort::Process => "Process",
        }
    }
}

pub struct Confirm {
    pub root: u32,
    pub pids: Vec<u32>,
    pub label: String,
    pub critical: Vec<String>,
    pub includes_self: bool,
}

pub struct EnvView {
    pub pid: u32,
    pub name: String,
    pub vars: Vec<String>,
    pub scroll: usize,
}

pub enum Mode {
    Normal,
    Filter,
    Confirm(Confirm),
    Help,
    Env(EnvView),
}

pub struct ProcRow {
    pub pid: u32,
    pub prefix: String,
}

pub struct App {
    pub collector: Collector,
    pub snap: Snapshot,
    pub tab: Tab,
    pub mode: Mode,
    pub running: bool,

    pub proc_filter: String,
    pub port_filter: String,
    pub proc_sort: ProcSort,
    pub proc_desc: bool,
    pub port_sort: PortSort,
    pub port_desc: bool,
    pub tree: bool,
    pub collapsed: HashSet<u32>,
    pub listen_only: bool,
    pub show_details: bool,

    pub proc_rows: Vec<ProcRow>,
    pub port_rows: Vec<usize>,
    pub proc_state: TableState,
    pub port_state: TableState,
    sel_pid: Option<u32>,
    sel_sock: Option<Socket>,

    pub paused: bool,
    pub interval: Duration,
    pub last_refresh: Instant,
    pub cpu_hist: VecDeque<u64>,
    pub mem_hist: VecDeque<u64>,
    pub status: Option<(String, bool, Instant)>,
    pub elevated: Option<bool>,
    pub self_pid: u32,
    pub table_area: Rect,
}

const HIST: usize = 300;

impl App {
    pub fn new() -> Self {
        let mut collector = Collector::new();
        // CPU usage needs two samples to mean anything.
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        let snap = collector.snapshot();
        let mut app = Self {
            collector,
            snap,
            tab: Tab::Procs,
            mode: Mode::Normal,
            running: true,
            proc_filter: String::new(),
            port_filter: String::new(),
            proc_sort: ProcSort::Cpu,
            proc_desc: true,
            port_sort: PortSort::Port,
            port_desc: false,
            tree: false,
            collapsed: HashSet::new(),
            listen_only: false,
            show_details: true,
            proc_rows: Vec::new(),
            port_rows: Vec::new(),
            proc_state: TableState::default().with_selected(0),
            port_state: TableState::default().with_selected(0),
            sel_pid: None,
            sel_sock: None,
            paused: false,
            interval: Duration::from_millis(1500),
            last_refresh: Instant::now(),
            cpu_hist: VecDeque::new(),
            mem_hist: VecDeque::new(),
            status: None,
            elevated: platform::is_elevated(),
            self_pid: std::process::id(),
            table_area: Rect::default(),
        };
        app.push_hist();
        app.rebuild();
        if app.elevated == Some(false) && cfg!(windows) {
            app.flash("Not running as Administrator: some paths are hidden and system processes can't be killed", true);
        }
        app
    }

    pub fn refresh(&mut self) {
        self.snap = self.collector.snapshot();
        self.last_refresh = Instant::now();
        self.push_hist();
        self.rebuild();
    }

    fn push_hist(&mut self) {
        self.cpu_hist.push_back(self.snap.cpu.round() as u64);
        let mem = if self.snap.mem_total > 0 {
            self.snap.mem_used * 100 / self.snap.mem_total
        } else {
            0
        };
        self.mem_hist.push_back(mem);
        while self.cpu_hist.len() > HIST {
            self.cpu_hist.pop_front();
        }
        while self.mem_hist.len() > HIST {
            self.mem_hist.pop_front();
        }
    }

    pub fn flash(&mut self, msg: impl Into<String>, err: bool) {
        self.status = Some((msg.into(), err, Instant::now()));
    }

    // ---------- row building ----------

    pub fn rebuild(&mut self) {
        self.rebuild_procs();
        self.rebuild_ports();
    }

    fn proc_cmp(&self, a: &Proc, b: &Proc) -> Ordering {
        let o = match self.proc_sort {
            ProcSort::Cpu => a.cpu.partial_cmp(&b.cpu).unwrap_or(Ordering::Equal),
            ProcSort::Mem => a.mem.cmp(&b.mem),
            ProcSort::Disk => (a.read_rate + a.write_rate)
                .partial_cmp(&(b.read_rate + b.write_rate))
                .unwrap_or(Ordering::Equal),
            ProcSort::Pid => a.pid.cmp(&b.pid),
            ProcSort::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            ProcSort::Net => (a.listening, a.connections).cmp(&(b.listening, b.connections)),
            ProcSort::User => a.user.to_lowercase().cmp(&b.user.to_lowercase()),
        };
        let o = if self.proc_desc { o.reverse() } else { o };
        o.then(a.pid.cmp(&b.pid))
    }

    fn proc_matches(&self) -> Option<HashSet<u32>> {
        let f = self.proc_filter.trim().to_lowercase();
        if f.is_empty() {
            return None;
        }
        // "port:8080" -> processes that own that port
        if let Some(port) = f.strip_prefix("port:").and_then(|p| p.trim().parse::<u16>().ok()) {
            return Some(
                self.snap
                    .sockets
                    .iter()
                    .filter(|s| s.lport == port || s.remote.is_some_and(|r| r.1 == port))
                    .filter_map(|s| s.pid)
                    .collect(),
            );
        }
        // A bare number means a PID; don't also match it inside command lines and paths.
        if f.chars().all(|c| c.is_ascii_digit()) {
            return Some(
                self.snap
                    .procs
                    .values()
                    .filter(|p| p.pid.to_string() == f || p.name.to_lowercase().contains(&f))
                    .map(|p| p.pid)
                    .collect(),
            );
        }
        Some(
            self.snap
                .procs
                .values()
                .filter(|p| {
                    p.pid.to_string() == f
                        || p.name.to_lowercase().contains(&f)
                        || p.user.to_lowercase().contains(&f)
                        || p.cmd.to_lowercase().contains(&f)
                        || p.exe.as_ref().is_some_and(|e| e.to_string_lossy().to_lowercase().contains(&f))
                })
                .map(|p| p.pid)
                .collect(),
        )
    }

    fn rebuild_procs(&mut self) {
        let matches = self.proc_matches();
        let mut rows = Vec::new();

        if self.tree {
            // Show matches plus their ancestors so the hierarchy still makes sense.
            let visible: HashSet<u32> = match &matches {
                None => self.snap.procs.keys().copied().collect(),
                Some(m) => {
                    let mut v = m.clone();
                    for pid in m {
                        v.extend(self.snap.ancestors(*pid));
                    }
                    v
                }
            };
            let mut kids: HashMap<u32, Vec<&Proc>> = HashMap::new();
            let mut roots: Vec<&Proc> = Vec::new();
            for pid in &visible {
                let Some(p) = self.snap.procs.get(pid) else { continue };
                match self.snap.parent_of(p).filter(|pp| visible.contains(pp)) {
                    Some(pp) => kids.entry(pp).or_default().push(p),
                    None => roots.push(p),
                }
            }
            roots.sort_by(|a, b| self.proc_cmp(a, b));
            for v in kids.values_mut() {
                v.sort_by(|a, b| self.proc_cmp(a, b));
            }
            let mut seen = HashSet::new();
            for r in roots {
                self.walk(r.pid, &kids, String::new(), true, 0, &mut seen, &mut rows);
            }
        } else {
            let mut list: Vec<&Proc> = self
                .snap
                .procs
                .values()
                .filter(|p| matches.as_ref().is_none_or(|m| m.contains(&p.pid)))
                .collect();
            list.sort_by(|a, b| self.proc_cmp(a, b));
            rows = list
                .into_iter()
                .map(|p| ProcRow { pid: p.pid, prefix: String::new() })
                .collect();
        }

        self.proc_rows = rows;
        let idx = self
            .sel_pid
            .and_then(|pid| self.proc_rows.iter().position(|r| r.pid == pid));
        fix_selection(&mut self.proc_state, idx, self.proc_rows.len());
        self.sync_sel_pid();
    }

    #[allow(clippy::too_many_arguments)]
    fn walk(
        &self,
        pid: u32,
        kids: &HashMap<u32, Vec<&Proc>>,
        prefix: String,
        last: bool,
        depth: usize,
        seen: &mut HashSet<u32>,
        out: &mut Vec<ProcRow>,
    ) {
        if !seen.insert(pid) {
            return;
        }
        let children = kids.get(&pid);
        let has_kids = children.is_some_and(|c| !c.is_empty());
        let collapsed = self.collapsed.contains(&pid);
        let branch = if depth == 0 { "" } else if last { "└─" } else { "├─" };
        let marker = match (has_kids, collapsed) {
            (true, true) => "▸ ",
            (true, false) => "▾ ",
            _ => if depth == 0 { "  " } else { "─ " },
        };
        out.push(ProcRow { pid, prefix: format!("{prefix}{branch}{marker}") });
        if collapsed {
            return;
        }
        if let Some(children) = children {
            let child_prefix = if depth == 0 {
                String::new()
            } else {
                format!("{prefix}{}", if last { "  " } else { "│ " })
            };
            let n = children.len();
            for (i, c) in children.iter().enumerate() {
                self.walk(c.pid, kids, child_prefix.clone(), i + 1 == n, depth + 1, seen, out);
            }
        }
    }

    fn port_matches(&self, s: &Socket) -> bool {
        if self.listen_only && !s.listening {
            return false;
        }
        let f = self.port_filter.trim().to_lowercase();
        if f.is_empty() {
            return true;
        }
        let f = f.trim_start_matches(':');
        if let Some(pid) = f.strip_prefix("pid:").and_then(|p| p.trim().parse::<u32>().ok()) {
            return s.pid == Some(pid);
        }
        if let Ok(port) = f.parse::<u16>() {
            return s.lport == port || s.remote.is_some_and(|r| r.1 == port);
        }
        let proc = s.pid.and_then(|p| self.snap.procs.get(&p));
        s.proto.as_str().to_lowercase().contains(f)
            || s.local.to_string().contains(f)
            || s.remote.is_some_and(|r| r.0.to_string().contains(f))
            || s.state.to_lowercase().contains(f)
            || s.pid.is_some_and(|p| p.to_string() == f)
            || proc.is_some_and(|p| {
                p.name.to_lowercase().contains(f)
                    || p.exe.as_ref().is_some_and(|e| e.to_string_lossy().to_lowercase().contains(f))
            })
    }

    fn rebuild_ports(&mut self) {
        let mut idx: Vec<usize> = (0..self.snap.sockets.len())
            .filter(|&i| self.port_matches(&self.snap.sockets[i]))
            .collect();
        let name = |s: &Socket| {
            s.pid
                .and_then(|p| self.snap.procs.get(&p))
                .map(|p| p.name.to_lowercase())
                .unwrap_or_default()
        };
        idx.sort_by(|&a, &b| {
            let (a, b) = (&self.snap.sockets[a], &self.snap.sockets[b]);
            let o = match self.port_sort {
                PortSort::Port => a.lport.cmp(&b.lport),
                PortSort::Proto => a.proto.as_str().cmp(b.proto.as_str()),
                PortSort::State => a.state.cmp(&b.state),
                PortSort::Pid => a.pid.cmp(&b.pid),
                PortSort::Process => name(a).cmp(&name(b)),
            };
            let o = if self.port_desc { o.reverse() } else { o };
            o.then(a.lport.cmp(&b.lport))
                .then(a.proto.as_str().cmp(b.proto.as_str()))
                .then(a.local.cmp(&b.local))
                .then(a.remote.cmp(&b.remote))
        });
        self.port_rows = idx;
        let sel = self.sel_sock.as_ref().and_then(|want| {
            self.port_rows
                .iter()
                .position(|&i| &self.snap.sockets[i] == want)
        });
        fix_selection(&mut self.port_state, sel, self.port_rows.len());
        self.sync_sel_sock();
    }

    fn sync_sel_pid(&mut self) {
        self.sel_pid = self
            .proc_state
            .selected()
            .and_then(|i| self.proc_rows.get(i))
            .map(|r| r.pid);
    }

    fn sync_sel_sock(&mut self) {
        self.sel_sock = self
            .port_state
            .selected()
            .and_then(|i| self.port_rows.get(i))
            .map(|&i| self.snap.sockets[i].clone());
    }

    pub fn selected_socket(&self) -> Option<&Socket> {
        self.port_state
            .selected()
            .and_then(|i| self.port_rows.get(i))
            .map(|&i| &self.snap.sockets[i])
    }

    /// The process the current view is "about": the selected row, or the owner of the selected socket.
    pub fn focused_pid(&self) -> Option<u32> {
        match self.tab {
            Tab::Procs => self.proc_state.selected().and_then(|i| self.proc_rows.get(i)).map(|r| r.pid),
            Tab::Ports => self.selected_socket().and_then(|s| s.pid),
        }
    }

    pub fn focused_proc(&self) -> Option<&Proc> {
        self.focused_pid().and_then(|p| self.snap.procs.get(&p))
    }

    // ---------- navigation ----------

    fn move_sel(&mut self, delta: isize) {
        let (state, len) = match self.tab {
            Tab::Procs => (&mut self.proc_state, self.proc_rows.len()),
            Tab::Ports => (&mut self.port_state, self.port_rows.len()),
        };
        if len == 0 {
            return;
        }
        let cur = state.selected().unwrap_or(0) as isize;
        let next = (cur + delta).clamp(0, len as isize - 1) as usize;
        state.select(Some(next));
        match self.tab {
            Tab::Procs => self.sync_sel_pid(),
            Tab::Ports => self.sync_sel_sock(),
        }
    }

    fn page(&self) -> isize {
        (self.table_area.height.saturating_sub(3)).max(1) as isize
    }

    pub fn select_pid(&mut self, pid: u32) {
        self.tab = Tab::Procs;
        if !self.proc_rows.iter().any(|r| r.pid == pid) {
            self.proc_filter.clear();
            // Make sure no collapsed ancestor hides it.
            for a in self.snap.ancestors(pid) {
                self.collapsed.remove(&a);
            }
        }
        self.sel_pid = Some(pid);
        self.rebuild_procs();
    }

    // ---------- actions ----------

    fn ask_kill(&mut self, tree: bool) {
        let Some(p) = self.focused_proc() else {
            self.flash("No process selected (socket has no owning PID)", true);
            return;
        };
        let pids = if tree { self.snap.tree_pids(p.pid) } else { vec![p.pid] };
        let critical = pids
            .iter()
            .filter_map(|pid| self.snap.procs.get(pid))
            .filter(|q| platform::is_critical(q.pid, &q.name))
            .map(|q| format!("{} ({})", q.name, q.pid))
            .collect();
        let label = if tree && pids.len() > 1 {
            format!("{} ({}) and {} child process(es)", p.name, p.pid, pids.len() - 1)
        } else {
            format!("{} ({})", p.name, p.pid)
        };
        self.mode = Mode::Confirm(Confirm {
            root: p.pid,
            includes_self: pids.contains(&self.self_pid),
            pids,
            label,
            critical,
        });
    }

    fn do_kill(&mut self, c: Confirm) {
        let mut killed = 0;
        let mut denied = Vec::new();
        let mut gone = 0;
        // Parent first, so a supervisor can't respawn the children we're about to kill.
        for pid in &c.pids {
            if *pid == self.self_pid {
                continue;
            }
            match self.collector.kill(*pid) {
                Ok(true) => killed += 1,
                Ok(false) => denied.push(*pid),
                Err(()) => gone += 1,
            }
        }
        let name = self.snap.procs.get(&c.root).map(|p| p.name.clone()).unwrap_or_default();
        if denied.is_empty() {
            let extra = if gone > 0 { format!(" ({gone} already exited)") } else { String::new() };
            self.flash(format!("Killed {killed} process(es): {name}{extra}"), false);
        } else {
            let hint = if self.elevated == Some(false) { " - try running as Administrator" } else { "" };
            self.flash(
                format!("Killed {killed}, access denied for {} (pid {:?}){hint}", denied.len(), denied),
                true,
            );
        }
        // Give the OS a moment to tear them down before re-reading.
        std::thread::sleep(Duration::from_millis(150));
        self.refresh();
    }

    fn reveal(&mut self) {
        let Some(p) = self.focused_proc() else { return };
        match p.exe.clone() {
            Some(exe) => match platform::reveal(&exe) {
                Ok(()) => self.flash(format!("Opened {}", exe.display()), false),
                Err(e) => self.flash(format!("Couldn't open file location: {e}"), true),
            },
            None => self.flash(format!("Path for {} is not accessible", p.name), true),
        }
    }

    fn copy(&mut self, what: &str) {
        let Some(p) = self.focused_proc() else { return };
        let text = match what {
            "path" => p.exe.as_ref().map(|e| e.display().to_string()),
            "cmd" => Some(p.cmd.clone()).filter(|c| !c.is_empty()),
            _ => Some(p.pid.to_string()),
        };
        match text {
            Some(t) => match platform::copy(&t) {
                Ok(()) => self.flash(format!("Copied: {t}"), false),
                Err(e) => self.flash(format!("Clipboard error: {e}"), true),
            },
            None => self.flash(format!("No {what} available for {}", p.name), true),
        }
    }

    fn show_env(&mut self) {
        let Some((pid, name)) = self.focused_proc().map(|p| (p.pid, p.name.clone())) else { return };
        match self.collector.environ(pid) {
            Some(vars) if !vars.is_empty() => self.mode = Mode::Env(EnvView { pid, name, vars, scroll: 0 }),
            _ => self.flash(format!("Can't read environment of {name} (access denied or exited)"), true),
        }
    }

    fn cycle_sort(&mut self) {
        match self.tab {
            Tab::Procs => {
                let i = ProcSort::ALL.iter().position(|s| *s == self.proc_sort).unwrap_or(0);
                self.proc_sort = ProcSort::ALL[(i + 1) % ProcSort::ALL.len()];
                self.proc_desc = self.proc_sort.default_desc();
            }
            Tab::Ports => {
                let i = PortSort::ALL.iter().position(|s| *s == self.port_sort).unwrap_or(0);
                self.port_sort = PortSort::ALL[(i + 1) % PortSort::ALL.len()];
                self.port_desc = false;
            }
        }
        let label = match self.tab {
            Tab::Procs => self.proc_sort.label(),
            Tab::Ports => self.port_sort.label(),
        };
        self.flash(format!("Sorted by {label}"), false);
        self.rebuild();
    }

    fn filter_mut(&mut self) -> &mut String {
        match self.tab {
            Tab::Procs => &mut self.proc_filter,
            Tab::Ports => &mut self.port_filter,
        }
    }

    // ---------- input ----------

    pub fn on_key(&mut self, k: KeyEvent) {
        if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
            self.running = false;
            return;
        }
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Normal => self.on_normal(k),
            Mode::Filter => self.on_filter(k),
            Mode::Help => {}
            Mode::Confirm(c) => {
                let ok = match k.code {
                    // Critical processes need a deliberate capital Y.
                    KeyCode::Char('Y') => true,
                    KeyCode::Char('y') | KeyCode::Enter => c.critical.is_empty(),
                    _ => false,
                };
                if ok {
                    self.do_kill(c);
                } else if matches!(k.code, KeyCode::Char('y') | KeyCode::Enter) {
                    self.mode = Mode::Confirm(c);
                } else {
                    self.flash("Cancelled", false);
                }
            }
            Mode::Env(mut e) => match k.code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('e') => {}
                KeyCode::Down => {
                    e.scroll = (e.scroll + 1).min(e.vars.len().saturating_sub(1));
                    self.mode = Mode::Env(e);
                }
                KeyCode::Up => {
                    e.scroll = e.scroll.saturating_sub(1);
                    self.mode = Mode::Env(e);
                }
                KeyCode::PageDown => {
                    e.scroll = (e.scroll + 20).min(e.vars.len().saturating_sub(1));
                    self.mode = Mode::Env(e);
                }
                KeyCode::PageUp => {
                    e.scroll = e.scroll.saturating_sub(20);
                    self.mode = Mode::Env(e);
                }
                KeyCode::Char('c') => {
                    let text = e.vars.join("\n");
                    let pid = e.pid;
                    self.mode = Mode::Env(e);
                    match platform::copy(&text) {
                        Ok(()) => self.flash(format!("Copied environment of pid {pid}"), false),
                        Err(err) => self.flash(format!("Clipboard error: {err}"), true),
                    }
                }
                _ => self.mode = Mode::Env(e),
            },
        }
    }

    fn on_filter(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Esc => {
                self.filter_mut().clear();
                self.rebuild();
                return;
            }
            KeyCode::Enter => return,
            KeyCode::Backspace | KeyCode::Char(_) => {
                match k.code {
                    KeyCode::Char(ch) => self.filter_mut().push(ch),
                    _ => {
                        self.filter_mut().pop();
                    }
                }
                // Jump to the best match as the filter changes.
                match self.tab {
                    Tab::Procs => {
                        self.sel_pid = None;
                        self.proc_state.select(Some(0));
                    }
                    Tab::Ports => {
                        self.sel_sock = None;
                        self.port_state.select(Some(0));
                    }
                }
            }
            KeyCode::Up => self.move_sel(-1),
            KeyCode::Down => self.move_sel(1),
            _ => {}
        }
        self.mode = Mode::Filter;
        self.rebuild();
    }

    fn on_normal(&mut self, k: KeyEvent) {
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        match k.code {
            KeyCode::Char('q') => self.running = false,
            KeyCode::Char('?') | KeyCode::F(1) => self.mode = Mode::Help,
            KeyCode::Tab | KeyCode::BackTab => {
                self.tab = if self.tab == Tab::Procs { Tab::Ports } else { Tab::Procs }
            }
            KeyCode::Char('1') => self.tab = Tab::Procs,
            KeyCode::Char('2') => self.tab = Tab::Ports,

            KeyCode::Up => self.move_sel(-1),
            KeyCode::Down => self.move_sel(1),
            KeyCode::PageUp => self.move_sel(-self.page()),
            KeyCode::PageDown => self.move_sel(self.page()),
            KeyCode::Home => self.move_sel(isize::MIN / 2),
            KeyCode::End => self.move_sel(isize::MAX / 2),

            KeyCode::Char('/') => self.mode = Mode::Filter,
            KeyCode::Esc => {
                self.filter_mut().clear();
                self.rebuild();
            }

            KeyCode::Delete if shift => self.ask_kill(true),
            KeyCode::Char('k') | KeyCode::Delete => self.ask_kill(false),
            KeyCode::Char('K') => self.ask_kill(true),

            KeyCode::Char('o') => self.reveal(),
            KeyCode::Char('c') => self.copy("path"),
            KeyCode::Char('C') => self.copy("cmd"),
            KeyCode::Char('i') => self.copy("pid"),
            KeyCode::Char('e') => self.show_env(),
            KeyCode::Char('d') => self.show_details = !self.show_details,
            KeyCode::Char('s') => self.cycle_sort(),
            KeyCode::Char('r') => {
                match self.tab {
                    Tab::Procs => self.proc_desc = !self.proc_desc,
                    Tab::Ports => self.port_desc = !self.port_desc,
                }
                self.rebuild();
            }
            KeyCode::Char(' ') => {
                self.paused = !self.paused;
                self.flash(if self.paused { "Paused" } else { "Resumed" }, false);
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.interval = (self.interval + Duration::from_millis(500)).min(Duration::from_secs(10));
                self.flash(format!("Refresh every {:.1}s", self.interval.as_secs_f32()), false);
            }
            KeyCode::Char('-') => {
                self.interval = self
                    .interval
                    .saturating_sub(Duration::from_millis(500))
                    .max(Duration::from_millis(500));
                self.flash(format!("Refresh every {:.1}s", self.interval.as_secs_f32()), false);
            }
            KeyCode::F(5) => self.refresh(),
            KeyCode::Char('u') => {
                let parent = self.focused_proc().and_then(|p| self.snap.parent_of(p));
                match parent {
                    Some(pp) => self.select_pid(pp),
                    None => self.flash("No parent process", true),
                }
            }
            _ => match self.tab {
                Tab::Procs => self.on_procs_key(k),
                Tab::Ports => self.on_ports_key(k),
            },
        }
    }

    fn on_procs_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Char('t') => {
                self.tree = !self.tree;
                self.rebuild();
            }
            KeyCode::Left | KeyCode::Char('h') if self.tree => {
                if let Some(pid) = self.sel_pid {
                    if self.snap.children_map().contains_key(&pid) && !self.collapsed.contains(&pid) {
                        self.collapsed.insert(pid);
                    } else if let Some(pp) = self.snap.procs.get(&pid).and_then(|p| self.snap.parent_of(p)) {
                        self.sel_pid = Some(pp);
                    }
                    self.rebuild();
                }
            }
            KeyCode::Right | KeyCode::Char('l') if self.tree => {
                if let Some(pid) = self.sel_pid {
                    self.collapsed.remove(&pid);
                    self.rebuild();
                }
            }
            KeyCode::Char('p') | KeyCode::Enter => {
                if let Some(p) = self.focused_proc() {
                    let (pid, n) = (p.pid, p.listening + p.connections);
                    if n == 0 && k.code == KeyCode::Char('p') {
                        self.flash(format!("{} has no open ports", p.name), false);
                        return;
                    }
                    if n > 0 {
                        self.port_filter = format!("pid:{pid}");
                        self.listen_only = false;
                        self.tab = Tab::Ports;
                        self.rebuild();
                    }
                }
            }
            _ => {}
        }
    }

    fn on_ports_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Char('l') => {
                self.listen_only = !self.listen_only;
                self.rebuild();
            }
            KeyCode::Enter | KeyCode::Char('p') => match self.selected_socket().and_then(|s| s.pid) {
                Some(pid) => self.select_pid(pid),
                None => self.flash("Socket has no owning process", true),
            },
            _ => {}
        }
    }

    pub fn on_mouse(&mut self, m: MouseEvent) {
        if !matches!(self.mode, Mode::Normal | Mode::Filter) {
            return;
        }
        match m.kind {
            MouseEventKind::ScrollDown => self.move_sel(3),
            MouseEventKind::ScrollUp => self.move_sel(-3),
            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                let a = self.table_area;
                // +2: top border and header row
                if m.column >= a.x && m.column < a.x + a.width && m.row >= a.y + 2 && m.row < a.y + a.height {
                    let offset = match self.tab {
                        Tab::Procs => self.proc_state.offset(),
                        Tab::Ports => self.port_state.offset(),
                    };
                    let target = offset + (m.row - a.y - 2) as usize;
                    let cur = match self.tab {
                        Tab::Procs => self.proc_state.selected(),
                        Tab::Ports => self.port_state.selected(),
                    }
                    .unwrap_or(0);
                    self.move_sel(target as isize - cur as isize);
                }
            }
            _ => {}
        }
    }

    pub fn tick(&mut self) {
        if !self.paused && self.last_refresh.elapsed() >= self.interval {
            self.refresh();
        }
        if let Some((_, _, t)) = &self.status
            && t.elapsed() > Duration::from_secs(5) {
                self.status = None;
            }
    }
}

fn fix_selection(state: &mut TableState, idx: Option<usize>, len: usize) {
    if len == 0 {
        state.select(None);
        return;
    }
    let i = idx.unwrap_or_else(|| state.selected().unwrap_or(0).min(len - 1));
    state.select(Some(i));
}
