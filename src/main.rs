mod actions;
mod detect;
mod managed;
mod scan;
mod templates;
mod tui;

use anyhow::{Result, anyhow, bail};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "srvr", version, about = "See, manage and launch the servers running on your machine.", long_about = None)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Open the interactive TUI (default)
    Tui,
    /// List servers listening on this machine
    #[command(alias = "list")]
    Ls {
        /// Include OS/system services
        #[arg(short, long)]
        all: bool,
        /// Include UDP sockets
        #[arg(short, long)]
        udp: bool,
        /// Output JSON
        #[arg(long)]
        json: bool,
    },
    /// List the ready-made server templates
    #[command(alias = "t")]
    Templates,
    /// Launch a server from a template, e.g. `srvr up static-python -d ./site`
    #[command(alias = "start")]
    Up {
        template: String,
        #[arg(short, long)]
        port: Option<u16>,
        /// Directory to serve / run in (default: current directory)
        #[arg(short, long)]
        dir: Option<PathBuf>,
        #[arg(short, long)]
        name: Option<String>,
    },
    /// Launch any command as a managed server: `srvr run -p 4000 -- bun run dev`
    Run {
        #[arg(short, long)]
        port: Option<u16>,
        #[arg(short, long)]
        name: Option<String>,
        #[arg(short, long)]
        dir: Option<PathBuf>,
        #[arg(trailing_var_arg = true, required = true)]
        command: Vec<String>,
    },
    /// Show servers launched by srvr
    Ps,
    /// Stop a server by port, PID or managed name
    #[command(alias = "kill")]
    Stop {
        target: String,
        /// Force kill (SIGKILL / taskkill /F)
        #[arg(short, long)]
        force: bool,
    },
    /// Restart a managed server
    Restart { name: String },
    /// Show logs of a managed server
    Logs {
        name: String,
        #[arg(short = 'n', long, default_value_t = 100)]
        lines: usize,
        /// Keep printing new output
        #[arg(short, long)]
        follow: bool,
    },
    /// Open a server in the browser by port or managed name
    Open { target: String },
    /// Forget a stopped managed server
    Rm { name: String },
}

fn main() {
    // Behave like a normal Unix CLI when piped into `head` etc.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    if let Err(e) = run() {
        eprintln!("srvr: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd.unwrap_or(Cmd::Tui) {
        Cmd::Tui => tui::run(),
        Cmd::Ls { all, udp, json } => ls(all, udp, json),
        Cmd::Templates => list_templates(),
        Cmd::Up {
            template,
            port,
            dir,
            name,
        } => {
            let (all, _) = templates::all();
            let tpl = all
                .iter()
                .find(|t| t.id == template)
                .ok_or_else(|| anyhow!("unknown template `{template}` — see `srvr templates`"))?;
            up(tpl, port, dir, name)
        }
        Cmd::Run {
            port,
            name,
            dir,
            command,
        } => {
            let tpl = templates::custom(&shell_words::join(&command), port);
            let name = name.unwrap_or_else(|| {
                std::path::Path::new(&command[0])
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "custom".into())
            });
            up(&tpl, port, dir, Some(name))
        }
        Cmd::Ps => ps(),
        Cmd::Stop { target, force } => stop(&target, force),
        Cmd::Restart { name } => {
            let m = managed::find(&name).ok_or_else(|| anyhow!("no managed server `{name}`"))?;
            let (m, _child) = managed::restart(&m)?;
            println!("restarted {} (pid {})", m.name, m.pid);
            Ok(())
        }
        Cmd::Logs {
            name,
            lines,
            follow,
        } => logs(&name, lines, follow),
        Cmd::Open { target } => {
            let url = if let Some(m) = managed::find(&target).filter(|m| m.url.is_some()) {
                m.url.unwrap()
            } else {
                let port: u16 = target
                    .parse()
                    .map_err(|_| anyhow!("`{target}` is not a port or managed server"))?;
                format!("http://localhost:{port}")
            };
            println!("opening {url}");
            open::that_detached(&url)?;
            Ok(())
        }
        Cmd::Rm { name } => {
            let m = managed::find(&name).ok_or_else(|| anyhow!("no managed server `{name}`"))?;
            if m.alive(&managed::system_for(m.pid)) {
                bail!("{} is still running — `srvr stop {}` first", m.name, m.name);
            }
            managed::remove(&m.name)?;
            println!("removed {}", m.name);
            Ok(())
        }
    }
}

fn up(
    tpl: &templates::Template,
    port: Option<u16>,
    dir: Option<PathBuf>,
    name: Option<String>,
) -> Result<()> {
    let port = port.or(tpl.port.map(managed::free_port));
    let dir = dir.unwrap_or(std::env::current_dir()?);
    let name = managed::unique_name(&name.unwrap_or_else(|| tpl.default_name(port)));
    if let Some(p) = port
        && !managed::port_free(p)
    {
        bail!("port {p} is already in use");
    }
    let spec = tpl.spec(&name, port, &dir)?;
    let (m, _child) = managed::launch(spec)?;
    println!("started {} (pid {})", m.name, m.pid);
    if let Some(url) = &m.url {
        println!("  → {url}");
    }
    println!("  logs: srvr logs {}   stop: srvr stop {}", m.name, m.name);
    Ok(())
}

fn ls(all: bool, udp: bool, json: bool) -> Result<()> {
    let mut sc = scan::Scanner::new();
    let snap = sc.snapshot();
    if let Some(e) = snap.error {
        bail!(e);
    }
    let servers: Vec<_> = snap
        .servers
        .into_iter()
        .filter(|s| all || s.kind.category != detect::Category::System)
        .filter(|s| udp || s.proto == scan::Proto::Tcp)
        .collect();
    if json {
        println!("{}", serde_json::to_string_pretty(&servers)?);
        return Ok(());
    }
    println!(
        "{:<7} {:<5} {:<8} {:<22} {:<10} {:<8} {:>7}  PROCESS",
        "PORT", "PROTO", "BIND", "TYPE", "CATEGORY", "PID", "MEM"
    );
    for s in &servers {
        println!(
            "{:<7} {:<5} {:<8} {:<22} {:<10} {:<8} {:>7}  {}{}",
            s.port,
            s.proto.label(),
            s.exposure.label(),
            truncate(s.kind.label, 22),
            s.kind.category.label(),
            s.pid,
            scan::human_bytes(s.memory),
            s.name,
            s.managed
                .as_ref()
                .map(|m| format!("  [srvr:{m}]"))
                .unwrap_or_default()
        );
    }
    if servers.is_empty() {
        println!(
            "(nothing listening{})",
            if all { "" } else { " — try --all" }
        );
    }
    Ok(())
}

fn list_templates() -> Result<()> {
    let (all, warning) = templates::all();
    if let Some(w) = warning {
        eprintln!("warning: {w}");
    }
    println!(
        "{:<18} {:<34} {:<9} {:<6} READY",
        "ID", "NAME", "CATEGORY", "PORT"
    );
    for t in &all {
        println!(
            "{:<18} {:<34} {:<9} {:<6} {}",
            t.id,
            truncate(&t.name, 34),
            t.category,
            t.port.map(|p| p.to_string()).unwrap_or("-".into()),
            if t.available() {
                "yes".to_string()
            } else {
                format!("needs {}", t.program().replace('|', "/"))
            }
        );
    }
    println!(
        "\nAdd your own in {}",
        templates::user_templates_path().display()
    );
    Ok(())
}

fn ps() -> Result<()> {
    let snap = scan::Scanner::new().snapshot();
    if snap.managed.is_empty() {
        println!("no servers launched by srvr yet — try `srvr templates` and `srvr up <id>`");
        return Ok(());
    }
    println!(
        "{:<24} {:<9} {:<8} {:<12} {:>7}  URL",
        "NAME", "STATUS", "PID", "PORTS", "MEM"
    );
    for m in &snap.managed {
        let ports = m
            .ports
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{:<24} {:<9} {:<8} {:<12} {:>7}  {}",
            truncate(&m.entry.name, 24),
            if m.alive { "running" } else { "stopped" },
            m.entry.pid,
            if ports.is_empty() { "-".into() } else { ports },
            scan::human_bytes(m.memory),
            m.entry.url.clone().unwrap_or_default()
        );
    }
    Ok(())
}

fn stop(target: &str, force: bool) -> Result<()> {
    if let Some(m) = managed::find(target) {
        managed::stop(&m, force)?;
        println!("stopped {}", m.name);
        return Ok(());
    }
    let n: u32 = target
        .parse()
        .map_err(|_| anyhow!("`{target}` is not a port, PID or managed server"))?;
    let snap = scan::Scanner::new().snapshot();
    let by_port: Vec<_> = snap.servers.iter().filter(|s| s.port as u32 == n).collect();
    if !by_port.is_empty() {
        let mut pids: Vec<u32> = by_port.iter().map(|s| s.pid).collect();
        pids.dedup();
        for pid in pids {
            let s = by_port.iter().find(|s| s.pid == pid).unwrap();
            if let Some(m) = s.managed.as_deref().and_then(managed::find) {
                managed::stop(&m, force)?;
            } else {
                actions::signal(pid, force)?;
            }
            println!("stopped {} (pid {pid}) on port {n}", s.name);
        }
        return Ok(());
    }
    actions::signal(n, force)?;
    println!("signalled pid {n}");
    Ok(())
}

fn logs(name: &str, lines: usize, follow: bool) -> Result<()> {
    let m = managed::find(name).ok_or_else(|| anyhow!("no managed server `{name}`"))?;
    for l in managed::tail(&m.log, lines) {
        println!("{l}");
    }
    if follow {
        use std::io::{Read, Seek, SeekFrom, Write};
        let mut f = std::fs::File::open(&m.log)?;
        let mut pos = f.seek(SeekFrom::End(0))?;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(300));
            let len = f.metadata()?.len();
            if len < pos {
                pos = 0;
            }
            if len > pos {
                f.seek(SeekFrom::Start(pos))?;
                let mut buf = Vec::new();
                f.read_to_end(&mut buf)?;
                pos += buf.len() as u64;
                std::io::stdout().write_all(&buf)?;
                std::io::stdout().flush()?;
            }
        }
    }
    Ok(())
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}
