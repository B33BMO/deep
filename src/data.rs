use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::time::Instant;

use netstat2::{AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo, TcpState, get_sockets_info};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind, Users};

#[derive(Clone, Debug)]
pub struct Proc {
    pub pid: u32,
    pub ppid: Option<u32>,
    pub name: String,
    pub exe: Option<PathBuf>,
    pub cwd: Option<PathBuf>,
    pub cmd: String,
    pub user: String,
    pub status: String,
    /// Normalized to 0-100 across all cores, like Task Manager.
    pub cpu: f32,
    pub mem: u64,
    pub vmem: u64,
    pub read_rate: f64,
    pub write_rate: f64,
    pub total_read: u64,
    pub total_written: u64,
    pub start_time: u64,
    pub run_time: u64,
    pub listening: usize,
    pub connections: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Proto {
    Tcp,
    Udp,
}

impl Proto {
    pub fn as_str(self) -> &'static str {
        match self {
            Proto::Tcp => "TCP",
            Proto::Udp => "UDP",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Socket {
    pub proto: Proto,
    pub local: IpAddr,
    pub lport: u16,
    pub remote: Option<(IpAddr, u16)>,
    pub state: String,
    pub listening: bool,
    pub pid: Option<u32>,
}

#[derive(Default)]
pub struct Snapshot {
    pub procs: HashMap<u32, Proc>,
    pub sockets: Vec<Socket>,
    pub cpu: f32,
    pub ncpu: usize,
    pub mem_used: u64,
    pub mem_total: u64,
    pub swap_used: u64,
    pub swap_total: u64,
    pub uptime: u64,
    pub sock_err: Option<String>,
}

impl Snapshot {
    /// Valid parent: exists, isn't itself, and started before the child
    /// (Windows reuses PIDs, so a stale PPID can point at an unrelated newer process).
    pub fn parent_of(&self, p: &Proc) -> Option<u32> {
        let pp = p.ppid?;
        if pp == p.pid {
            return None;
        }
        let parent = self.procs.get(&pp)?;
        if parent.start_time > p.start_time {
            return None;
        }
        Some(pp)
    }

    pub fn children_map(&self) -> HashMap<u32, Vec<u32>> {
        let mut map: HashMap<u32, Vec<u32>> = HashMap::new();
        for p in self.procs.values() {
            if let Some(pp) = self.parent_of(p) {
                map.entry(pp).or_default().push(p.pid);
            }
        }
        map
    }

    /// Root first, then breadth-first through every descendant.
    pub fn tree_pids(&self, root: u32) -> Vec<u32> {
        let kids = self.children_map();
        let mut out = vec![root];
        let mut i = 0;
        while i < out.len() {
            if let Some(cs) = kids.get(&out[i]) {
                for c in cs {
                    if !out.contains(c) {
                        out.push(*c);
                    }
                }
            }
            i += 1;
        }
        out
    }

    /// Parent chain from the top-most ancestor down to (not including) pid.
    pub fn ancestors(&self, pid: u32) -> Vec<u32> {
        let mut chain = Vec::new();
        let mut cur = pid;
        while let Some(p) = self.procs.get(&cur) {
            match self.parent_of(p) {
                Some(pp) if !chain.contains(&pp) && pp != pid => {
                    chain.push(pp);
                    cur = pp;
                }
                _ => break,
            }
        }
        chain.reverse();
        chain
    }
}

pub struct Collector {
    sys: System,
    users: Users,
    last: Instant,
    user_cache: HashMap<String, String>,
}

impl Collector {
    pub fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, Self::kind());
        Self {
            sys,
            users: Users::new_with_refreshed_list(),
            last: Instant::now(),
            user_cache: HashMap::new(),
        }
    }

    fn kind() -> ProcessRefreshKind {
        ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .with_disk_usage()
            .with_exe(UpdateKind::OnlyIfNotSet)
            .with_cmd(UpdateKind::OnlyIfNotSet)
            .with_cwd(UpdateKind::OnlyIfNotSet)
            .with_user(UpdateKind::OnlyIfNotSet)
            .without_tasks()
    }

    pub fn snapshot(&mut self) -> Snapshot {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.sys
            .refresh_processes_specifics(ProcessesToUpdate::All, true, Self::kind());
        let elapsed = self.last.elapsed().as_secs_f64().max(0.001);
        self.last = Instant::now();

        let ncpu = self.sys.cpus().len().max(1);
        let mut procs = HashMap::with_capacity(self.sys.processes().len());
        for (pid, p) in self.sys.processes() {
            if p.thread_kind().is_some() {
                continue;
            }
            let user = match p.user_id() {
                Some(uid) => {
                    let key = uid.to_string();
                    if let Some(name) = self.user_cache.get(&key) {
                        name.clone()
                    } else {
                        let name = self
                            .users
                            .get_user_by_id(uid)
                            .map(|u| u.name().to_string())
                            .unwrap_or_else(|| key.clone());
                        self.user_cache.insert(key, name.clone());
                        name
                    }
                }
                None => String::new(),
            };
            let du = p.disk_usage();
            let cmd = p
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ");
            procs.insert(
                pid.as_u32(),
                Proc {
                    pid: pid.as_u32(),
                    ppid: p.parent().map(|x| x.as_u32()),
                    name: p.name().to_string_lossy().into_owned(),
                    exe: p.exe().map(|x| x.to_path_buf()).filter(|x| !x.as_os_str().is_empty()),
                    cwd: p.cwd().map(|x| x.to_path_buf()).filter(|x| !x.as_os_str().is_empty()),
                    cmd,
                    user,
                    status: p.status().to_string(),
                    cpu: p.cpu_usage() / ncpu as f32,
                    mem: p.memory(),
                    vmem: p.virtual_memory(),
                    read_rate: du.read_bytes as f64 / elapsed,
                    write_rate: du.written_bytes as f64 / elapsed,
                    total_read: du.total_read_bytes,
                    total_written: du.total_written_bytes,
                    start_time: p.start_time(),
                    run_time: p.run_time(),
                    listening: 0,
                    connections: 0,
                },
            );
        }

        let (sockets, sock_err) = match read_sockets() {
            Ok(s) => (s, None),
            Err(e) => (Vec::new(), Some(e.to_string())),
        };
        for s in &sockets {
            if let Some(p) = s.pid.and_then(|pid| procs.get_mut(&pid)) {
                if s.listening {
                    p.listening += 1;
                } else if s.remote.is_some() {
                    p.connections += 1;
                }
            }
        }

        Snapshot {
            procs,
            sockets,
            cpu: self.sys.global_cpu_usage(),
            ncpu,
            mem_used: self.sys.used_memory(),
            mem_total: self.sys.total_memory(),
            swap_used: self.sys.used_swap(),
            swap_total: self.sys.total_swap(),
            uptime: System::uptime(),
            sock_err,
        }
    }

    pub fn environ(&mut self, pid: u32) -> Option<Vec<String>> {
        let p = Pid::from_u32(pid);
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[p]),
            false,
            ProcessRefreshKind::nothing().with_environ(UpdateKind::Always),
        );
        let mut env: Vec<String> = self
            .sys
            .process(p)?
            .environ()
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        env.sort_by_key(|s| s.to_lowercase());
        Some(env)
    }

    /// Ok(true) killed, Ok(false) the OS refused, Err if it's already gone.
    pub fn kill(&self, pid: u32) -> Result<bool, ()> {
        self.sys.process(Pid::from_u32(pid)).map(|p| p.kill()).ok_or(())
    }
}

fn read_sockets() -> anyhow::Result<Vec<Socket>> {
    let af = AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6;
    let pf = ProtocolFlags::TCP | ProtocolFlags::UDP;
    let mut out = Vec::new();
    for si in get_sockets_info(af, pf)? {
        let pids: Vec<Option<u32>> = if si.associated_pids.is_empty() {
            vec![None]
        } else {
            si.associated_pids.iter().map(|p| Some(*p)).collect()
        };
        for pid in pids {
            let s = match &si.protocol_socket_info {
                ProtocolSocketInfo::Tcp(t) => {
                    let listening = t.state == TcpState::Listen;
                    Socket {
                        proto: Proto::Tcp,
                        local: t.local_addr,
                        lport: t.local_port,
                        remote: (!listening).then_some((t.remote_addr, t.remote_port)),
                        state: t.state.to_string(),
                        listening,
                        pid,
                    }
                }
                // UDP has no connection state; a bound UDP socket is effectively "listening".
                ProtocolSocketInfo::Udp(u) => Socket {
                    proto: Proto::Udp,
                    local: u.local_addr,
                    lport: u.local_port,
                    remote: None,
                    state: String::new(),
                    listening: true,
                    pid,
                },
            };
            out.push(s);
        }
    }
    Ok(out)
}
