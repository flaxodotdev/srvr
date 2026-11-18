//! Things you can do to a server: signal it, poke it over HTTP, copy its URL.

use anyhow::{Result, bail};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

#[cfg(unix)]
fn kill_raw(target: i32, force: bool) -> Result<()> {
    let sig = if force { libc::SIGKILL } else { libc::SIGTERM };
    // SAFETY: plain libc call, no memory involved.
    let rc = unsafe { libc::kill(target, sig) };
    if rc == 0 {
        return Ok(());
    }
    let err = std::io::Error::last_os_error();
    match err.raw_os_error() {
        Some(libc::EPERM) => {
            bail!("permission denied — this process belongs to another user (try `sudo srvr`)")
        }
        Some(libc::ESRCH) => bail!("process no longer exists"),
        _ => bail!("kill failed: {err}"),
    }
}

/// Stop a single process. `force` = SIGKILL / taskkill /F.
pub fn signal(pid: u32, force: bool) -> Result<()> {
    if pid == 0 {
        bail!("refusing to signal pid 0");
    }
    #[cfg(unix)]
    {
        kill_raw(pid as i32, force)
    }
    #[cfg(windows)]
    {
        taskkill(pid, force, false)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = force;
        bail!("not supported on this platform")
    }
}

/// Stop a process together with its children (process group on Unix, /T on Windows).
pub fn signal_tree(pid: u32, force: bool) -> Result<()> {
    #[cfg(unix)]
    {
        // srvr starts servers in their own process group whose id == pid.
        kill_raw(-(pid as i32), force).or_else(|_| kill_raw(pid as i32, force))
    }
    #[cfg(windows)]
    {
        // Console servers ignore a polite taskkill, so tree-stops are always forced.
        let _ = force;
        taskkill(pid, true, true)
    }
    #[cfg(not(any(unix, windows)))]
    {
        signal(pid, force)
    }
}

#[cfg(windows)]
fn taskkill(pid: u32, force: bool, tree: bool) -> Result<()> {
    let mut cmd = std::process::Command::new("taskkill");
    cmd.args(["/PID", &pid.to_string()]);
    if tree {
        cmd.arg("/T");
    }
    if force {
        cmd.arg("/F");
    }
    let out = cmd.output()?;
    if out.status.success() {
        Ok(())
    } else {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim())
    }
}

/// Which address to connect to for a server bound on `addr`.
pub fn connect_ip(addr: IpAddr) -> IpAddr {
    if addr.is_unspecified() {
        match addr {
            IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(_) => IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
        }
    } else {
        addr
    }
}

pub fn url_for(addr: IpAddr, port: u16) -> String {
    let ip = connect_ip(addr);
    if ip.is_loopback() {
        format!("http://localhost:{port}")
    } else if let IpAddr::V6(v6) = ip {
        format!("http://[{v6}]:{port}")
    } else {
        format!("http://{ip}:{port}")
    }
}

/// Send `HEAD /` and summarise the response, e.g. `200 OK · nginx/1.27`.
pub fn probe_http(addr: IpAddr, port: u16) -> String {
    let sock = SocketAddr::new(connect_ip(addr), port);
    let timeout = Duration::from_millis(1500);
    let mut stream = match TcpStream::connect_timeout(&sock, timeout) {
        Ok(s) => s,
        Err(e) => return format!("connect failed: {e}"),
    };
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    let req = format!(
        "HEAD / HTTP/1.1\r\nHost: localhost:{port}\r\nUser-Agent: srvr\r\nConnection: close\r\n\r\n"
    );
    if let Err(e) = stream.write_all(req.as_bytes()) {
        return format!("write failed: {e}");
    }
    let mut buf = [0u8; 2048];
    let n = match stream.read(&mut buf) {
        Ok(0) => return "connection closed — not HTTP".into(),
        Ok(n) => n,
        Err(_) => return "no HTTP response (not a web server?)".into(),
    };
    let text = String::from_utf8_lossy(&buf[..n]);
    let mut lines = text.lines();
    let status = lines.next().unwrap_or("");
    if !status.starts_with("HTTP/") {
        return format!(
            "not HTTP — replied {:?}",
            status.chars().take(40).collect::<String>()
        );
    }
    let status = status
        .split_once(' ')
        .map(|x| x.1)
        .unwrap_or(status)
        .to_string();
    let mut extra = Vec::new();
    for l in lines {
        let lower = l.to_ascii_lowercase();
        for h in ["server:", "x-powered-by:", "content-type:"] {
            if lower.starts_with(h) {
                extra.push(l[h.len()..].trim().to_string());
            }
        }
    }
    if extra.is_empty() {
        status
    } else {
        format!("{status} · {}", extra.join(" · "))
    }
}

/// Copy to clipboard using the OSC 52 terminal escape (works over SSH too),
/// falling back to the platform clipboard tool.
pub fn copy(text: &str) -> Result<()> {
    if native_copy(text).is_ok() {
        return Ok(());
    }
    let mut out = std::io::stdout();
    write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes()))?;
    out.flush()?;
    Ok(())
}

fn native_copy(text: &str) -> Result<()> {
    use std::process::{Command, Stdio};
    let candidates: &[&[&str]] = if cfg!(target_os = "macos") {
        &[&["pbcopy"]]
    } else if cfg!(windows) {
        &[&["clip"]]
    } else {
        &[
            &["wl-copy"],
            &["xclip", "-selection", "clipboard"],
            &["xsel", "-b", "-i"],
        ]
    };
    for c in candidates {
        if let Ok(mut child) = Command::new(c[0])
            .args(&c[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
            return Ok(());
        }
    }
    bail!("no clipboard tool")
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                s.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    #[test]
    fn b64() {
        assert_eq!(super::base64(b"hello"), "aGVsbG8=");
        assert_eq!(super::base64(b"hi!"), "aGkh");
    }
}
