//! Discovers listening sockets and enriches them with process details.

use crate::detect::{self, Kind};
use crate::managed::{self, Managed};
use anyhow::Result;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind, Users};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum Proto {
    Tcp,
    Udp,
}

impl Proto {
    pub fn label(self) -> &'static str {
        match self {
            Proto::Tcp => "tcp",
            Proto::Udp => "udp",
        }
    }
}

/// Who can reach the server, derived from the bind addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Exposure {
    /// Loopback only (127.0.0.1 / ::1).
    Local,
    /// Bound to all interfaces — reachable from your network.
    Public,
    /// Bound to a specific interface address.
    Interface,
}

impl Exposure {
    pub fn label(self) -> &'static str {
        match self {
            Exposure::Local => "local",
            Exposure::Public => "network",
            Exposure::Interface => "iface",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Server {
    pub port: u16,
    pub proto: Proto,
    pub addrs: Vec<IpAddr>,
    pub exposure: Exposure,
    pub pid: u32,
    pub name: String,
    pub exe: String,
    pub cmd: String,
    pub cwd: Option<String>,
    pub user: Option<String>,
    pub memory: u64,
    pub cpu: f32,
    pub uptime: u64,
    pub kind: Kind,
    /// Name of the srvr-managed entry this belongs to, if any.
    pub managed: Option<String>,
}

impl Server {
    pub fn url(&self) -> String {
        crate::actions::url_for(self.primary_addr(), self.port)
    }
    pub fn primary_addr(&self) -> IpAddr {
        // Prefer IPv4 for connecting — it is what most people expect.
        self.addrs
            .iter()
            .find(|a| a.is_ipv4())
            .or(self.addrs.first())
            .copied()
            .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST))
    }
}

#[derive(Clone, Debug)]
pub struct ManagedView {
    pub entry: Managed,
    pub alive: bool,
    pub ports: Vec<u16>,
    pub memory: u64,
    pub cpu: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub servers: Vec<Server>,
    pub managed: Vec<ManagedView>,
    pub error: Option<String>,
}

pub fn new_system() -> System {
    System::new().expect("sysinfo is not supported on this platform")
}

pub struct Scanner {
    sys: System,
    users: Option<Users>,
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}

impl Scanner {
    pub fn new() -> Self {
        Self {
            sys: new_system(),
            users: Users::new_with_refreshed_list().ok(),
        }
    }

    fn refresh(&mut self) {
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_memory()
                .with_cpu()
                .with_cmd(UpdateKind::OnlyIfNotSet)
                .with_exe(UpdateKind::OnlyIfNotSet)
                .with_cwd(UpdateKind::OnlyIfNotSet)
                .with_user(UpdateKind::OnlyIfNotSet),
        );
    }

    pub fn snapshot(&mut self) -> Snapshot {
        self.refresh();
        let managed_list = managed::load();
        let (servers, error) = match self.servers(&managed_list) {
            Ok(s) => (s, None),
            Err(e) => (Vec::new(), Some(e.to_string())),
        };
        let mut managed = Vec::new();
        for m in managed_list {
            let alive = m.alive(&self.sys);
            let ports = servers
                .iter()
                .filter(|s| s.managed.as_deref() == Some(&m.name))
                .map(|s| s.port)
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            let (memory, cpu) = if alive {
                self.tree_usage(m.pid)
            } else {
                (0, 0.0)
            };
            managed.push(ManagedView {
                entry: m,
                alive,
                ports,
                memory,
                cpu,
            });
        }
        Snapshot {
            servers,
            managed,
            error,
        }
    }

    /// Memory/CPU of a process plus all its descendants.
    fn tree_usage(&self, root: u32) -> (u64, f32) {
        let root = Pid::from_u32(root);
        let mut mem = 0;
        let mut cpu = 0.0;
        for (pid, p) in self.sys.processes() {
            if *pid == root || self.descends_from(*pid, root) {
                mem += p.memory();
                cpu += p.cpu_usage();
            }
        }
        (mem, cpu)
    }

    fn descends_from(&self, mut pid: Pid, ancestor: Pid) -> bool {
        for _ in 0..64 {
            match self.sys.process(pid).and_then(|p| p.parent()) {
                Some(parent) if parent == ancestor => return true,
                Some(parent) if parent != pid => pid = parent,
                _ => return false,
            }
        }
        false
    }

    fn servers(&self, managed_list: &[Managed]) -> Result<Vec<Server>> {
        let listeners = listeners::get_all().map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut grouped: BTreeMap<(u16, Proto, u32), (Vec<IpAddr>, listeners::Process)> =
            BTreeMap::new();
        for l in listeners {
            let proto = match l.protocol {
                listeners::Protocol::TCP if l.state == listeners::SocketState::Listen => Proto::Tcp,
                listeners::Protocol::UDP if l.socket.port() != 0 => Proto::Udp,
                _ => continue,
            };
            let e = grouped
                .entry((l.socket.port(), proto, l.process.pid))
                .or_insert_with(|| (Vec::new(), l.process.clone()));
            if !e.0.contains(&l.socket.ip()) {
                e.0.push(l.socket.ip());
            }
        }

        let alive_managed: Vec<&Managed> =
            managed_list.iter().filter(|m| m.alive(&self.sys)).collect();
        let mut by_pid: HashMap<u32, String> = HashMap::new();

        let mut out = Vec::with_capacity(grouped.len());
        for ((port, proto, pid), (mut addrs, lp)) in grouped {
            addrs.sort();
            let proc = self.sys.process(Pid::from_u32(pid));
            let name = if lp.name.is_empty() {
                proc.map(|p| p.name().to_string_lossy().into_owned())
                    .unwrap_or_default()
            } else {
                lp.name.clone()
            };
            let exe = proc
                .and_then(|p| p.exe())
                .map(|e| e.display().to_string())
                .unwrap_or(lp.path.clone());
            let cmd = proc
                .map(|p| {
                    p.cmd()
                        .iter()
                        .map(|a| a.to_string_lossy())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            let kind = detect::classify(&name, &exe, &cmd, port);

            let managed = by_pid.get(&pid).cloned().or_else(|| {
                alive_managed
                    .iter()
                    .find(|m| {
                        m.pid == pid || self.descends_from(Pid::from_u32(pid), Pid::from_u32(m.pid))
                    })
                    // Docker-style templates: the listener is the container runtime, match on port.
                    .or_else(|| {
                        alive_managed
                            .iter()
                            .find(|m| m.port == Some(port) && m.stop_argv.is_some())
                    })
                    .map(|m| m.name.clone())
            });
            if let Some(m) = &managed {
                by_pid.entry(pid).or_insert_with(|| m.clone());
            }

            let exposure = if addrs.iter().any(|a| a.is_unspecified()) {
                Exposure::Public
            } else if addrs.iter().all(|a| a.is_loopback()) {
                Exposure::Local
            } else {
                Exposure::Interface
            };

            out.push(Server {
                port,
                proto,
                exposure,
                pid,
                name,
                exe,
                cmd,
                cwd: proc.and_then(|p| p.cwd()).map(|c| c.display().to_string()),
                user: proc.and_then(|p| p.user_id()).and_then(|uid| {
                    self.users
                        .as_ref()
                        .and_then(|u| u.get_user_by_id(uid))
                        .map(|u| u.name().to_string())
                }),
                memory: proc.map(|p| p.memory()).unwrap_or(0),
                cpu: proc.map(|p| p.cpu_usage()).unwrap_or(0.0),
                uptime: proc.map(|p| p.run_time()).unwrap_or(0),
                kind,
                managed,
                addrs,
            });
        }
        Ok(out)
    }
}

pub fn human_bytes(b: u64) -> String {
    const U: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{b}B")
    } else if v < 10.0 {
        format!("{v:.1}{}", U[i])
    } else {
        format!("{v:.0}{}", U[i])
    }
}

pub fn human_duration(secs: u64) -> String {
    match secs {
        0..60 => format!("{secs}s"),
        60..3600 => format!("{}m", secs / 60),
        3600..86400 => format!("{}h{}m", secs / 3600, secs % 3600 / 60),
        _ => format!("{}d{}h", secs / 86400, secs % 86400 / 3600),
    }
}
