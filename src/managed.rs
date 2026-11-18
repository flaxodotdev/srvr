//! Servers launched by srvr: spawning, persistence, stop/restart and logs.

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Managed {
    /// Unique, human-friendly name (also the docker container name for docker templates).
    pub name: String,
    pub template: String,
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    #[serde(default)]
    pub env: Vec<(String, String)>,
    pub port: Option<u16>,
    pub pid: u32,
    /// Process start time (unix secs) used to guard against PID reuse.
    #[serde(default)]
    pub proc_start: u64,
    pub started: u64,
    pub log: PathBuf,
    #[serde(default)]
    pub stop_argv: Option<Vec<String>>,
    #[serde(default)]
    pub url: Option<String>,
}

/// What to launch. Produced from a template + user input.
#[derive(Clone, Debug)]
pub struct LaunchSpec {
    pub name: String,
    pub template: String,
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    pub port: Option<u16>,
    pub stop_argv: Option<Vec<String>>,
    pub url: Option<String>,
}

pub fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("srvr")
}

fn state_path() -> PathBuf {
    data_dir().join("state.json")
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn load() -> Vec<Managed> {
    fs::read_to_string(state_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(list: &[Managed]) -> Result<()> {
    let path = state_path();
    fs::create_dir_all(path.parent().unwrap())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(list)?)?;
    fs::rename(tmp, path)?;
    Ok(())
}

fn update<F: FnOnce(&mut Vec<Managed>) -> Result<R>, R>(f: F) -> Result<R> {
    let mut list = load();
    let r = f(&mut list)?;
    save(&list)?;
    Ok(r)
}

pub fn find(query: &str) -> Option<Managed> {
    let list = load();
    list.iter()
        .find(|m| m.name == query)
        .or_else(|| {
            list.iter()
                .find(|m| m.port.map(|p| p.to_string()).as_deref() == Some(query))
        })
        .or_else(|| list.iter().find(|m| m.name.starts_with(query)))
        .cloned()
}

/// `base`, or `base-2`, `base-3`… if a *running* server already uses it.
/// A stopped entry with the same name is simply replaced on launch.
pub fn unique_name(base: &str) -> String {
    let list = load();
    let taken = |n: &str| {
        list.iter()
            .any(|m| m.name == n && m.alive(&system_for(m.pid)))
    };
    if !taken(base) {
        return base.to_string();
    }
    (2..)
        .map(|i| format!("{base}-{i}"))
        .find(|n| !taken(n))
        .unwrap()
}

/// Resolve a program, supporting `a|b|c` alternatives (first one found on PATH wins).
pub fn resolve_program(spec: &str) -> Option<PathBuf> {
    spec.split('|').find_map(|p| which::which(p).ok())
}

impl Managed {
    pub fn alive(&self, sys: &System) -> bool {
        is_alive(sys, self.pid, self.proc_start)
    }
}

pub fn is_alive(sys: &System, pid: u32, proc_start: u64) -> bool {
    match sys.process(Pid::from_u32(pid)) {
        Some(p) => {
            !matches!(p.status(), ProcessStatus::Zombie | ProcessStatus::Dead)
                && (proc_start == 0 || p.start_time().abs_diff(proc_start) <= 2)
        }
        None => false,
    }
}

pub fn system_for(pid: u32) -> System {
    let mut sys = crate::scan::new_system();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
        true,
        ProcessRefreshKind::nothing(),
    );
    sys
}

/// Spawn the server detached from the terminal, logging to a file.
/// Returns the `Child` so an interactive caller can reap it.
pub fn launch(spec: LaunchSpec) -> Result<(Managed, Child)> {
    let program = spec.argv.first().ok_or_else(|| anyhow!("empty command"))?;
    let resolved = resolve_program(program)
        .ok_or_else(|| anyhow!("`{}` not found on PATH", program.replace('|', "` / `")))?;

    let logs = data_dir().join("logs");
    fs::create_dir_all(&logs)?;
    let log = logs.join(format!("{}.log", spec.name));
    let mut f = OpenOptions::new().create(true).append(true).open(&log)?;
    writeln!(
        f,
        "\n--- srvr: started {} in {} ---\n$ {}",
        format_utc(now()),
        spec.cwd.display(),
        shell_words::join(&spec.argv)
    )?;

    let mut cmd = Command::new(&resolved);
    cmd.args(&spec.argv[1..])
        .current_dir(&spec.cwd)
        // Make interpreters flush output so logs show up live.
        .env("PYTHONUNBUFFERED", "1")
        .env("FORCE_COLOR", "0")
        .envs(spec.env.iter().cloned())
        .stdin(Stdio::null())
        .stdout(f.try_clone()?)
        .stderr(f);
    detach(&mut cmd);
    let child = cmd
        .spawn()
        .with_context(|| format!("failed to start {}", resolved.display()))?;

    let pid = child.id();
    let proc_start = system_for(pid)
        .process(Pid::from_u32(pid))
        .map(|p| p.start_time())
        .unwrap_or(0);

    let m = Managed {
        name: spec.name,
        template: spec.template,
        argv: spec.argv,
        cwd: spec.cwd,
        env: spec.env,
        port: spec.port,
        pid,
        proc_start,
        started: now(),
        log,
        stop_argv: spec.stop_argv,
        url: spec.url,
    };
    let m2 = m.clone();
    update(move |list| {
        list.retain(|x| x.name != m2.name);
        list.push(m2);
        Ok(())
    })?;
    Ok((m, child))
}

#[cfg(unix)]
fn detach(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    // New process group: survives the TUI closing and lets us signal the whole tree.
    cmd.process_group(0);
}

#[cfg(windows)]
fn detach(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
}

#[cfg(not(any(unix, windows)))]
fn detach(_cmd: &mut Command) {}

/// Stop a managed server (runs its stop command, then signals its process group).
pub fn stop(m: &Managed, force: bool) -> Result<()> {
    if let Some(stop) = &m.stop_argv
        && let Some(prog) = stop.first().and_then(|p| resolve_program(p))
    {
        let _ = Command::new(prog)
            .args(&stop[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let sys = system_for(m.pid);
    if m.alive(&sys) {
        crate::actions::signal_tree(m.pid, force)?;
    }
    Ok(())
}

pub fn wait_dead(pid: u32, proc_start: u64, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if !is_alive(&system_for(pid), pid, proc_start) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    false
}

pub fn restart(m: &Managed) -> Result<(Managed, Child)> {
    stop(m, false)?;
    if !wait_dead(m.pid, m.proc_start, Duration::from_secs(6)) {
        stop(m, true)?;
        wait_dead(m.pid, m.proc_start, Duration::from_secs(3));
    }
    launch(LaunchSpec {
        name: m.name.clone(),
        template: m.template.clone(),
        argv: m.argv.clone(),
        cwd: m.cwd.clone(),
        env: m.env.clone(),
        port: m.port,
        stop_argv: m.stop_argv.clone(),
        url: m.url.clone(),
    })
}

pub fn remove(name: &str) -> Result<()> {
    update(|list| {
        let before = list.len();
        list.retain(|m| m.name != name);
        if list.len() == before {
            bail!("no managed server named `{name}`");
        }
        Ok(())
    })
}

/// Last `max_lines` lines of a log file.
pub fn tail(path: &Path, max_lines: usize) -> Vec<String> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = File::open(path) else {
        return vec!["(no log file yet)".into()];
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(256 * 1024);
    let _ = f.seek(SeekFrom::Start(start));
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<String> = text.lines().map(strip_ansi).collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0); // partial line
    }
    let skip = lines.len().saturating_sub(max_lines);
    lines.split_off(skip)
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else if c == '\r' || c == '\t' {
            out.push(' ');
        } else if !c.is_control() {
            out.push(c);
        }
    }
    out
}

/// `2026-11-18 14:03:22 UTC` without pulling in a date crate.
pub fn format_utc(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// First free TCP port at or after `start`.
pub fn free_port(start: u16) -> u16 {
    (start..=u16::MAX).find(|p| port_free(*p)).unwrap_or(start)
}

pub fn port_free(port: u16) -> bool {
    use std::net::TcpListener;
    TcpListener::bind(("127.0.0.1", port)).is_ok() && TcpListener::bind(("0.0.0.0", port)).is_ok()
}
