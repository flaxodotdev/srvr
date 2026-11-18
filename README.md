# ▲ srvr

**See every server running on your machine, and start new ones, from one terminal UI.**

`srvr` shows what's listening on your ports, identifies each one (Vite, Postgres,
Docker, Ollama, nginx…), and lets you open, probe, stop, or kill it. It also comes
with ready-made templates for static file servers, dev servers, and databases, which
you launch with one keypress. Servers you start keep running after you quit, and srvr
keeps their logs.

Written in Rust. Runs on Linux, macOS, Windows, and FreeBSD.

```
 ▲ srvr   1 Running 7   2 Launch 28   3 Managed 2/2                              my-laptop · v0.1.0
╭ Listening servers (7) · sort: port ─────────────────────────────────────────────────────────────╮
│ PORT         BIND     TYPE                  PROCESS          PID     CPU    MEM    UP     SRVR   │
│▌:3000   tcp  local    ● serve (static)      node             9512    0.1%   71M    2m     ★ site │
│ :5173   tcp  local    ● Vite                node             8167    1.0%   82M    40m           │
│ :5432   tcp  local    ● PostgreSQL          postgres         805     0.0%   12M    1d14h  ★ pg   │
│ :6379   tcp  local    ● Redis               redis-server     1203    0.2%   8.1M   3h            │
│ :8080   tcp  network  ● nginx               nginx            4410    0.0%   6.3M   2d            │
│ :11434  tcp  local    ● Ollama              ollama           1168    0.0%   6.7M   1d14h         │
╰──────────────────────────────────────────────────────────────────────────────────────────────────╯
```

## Install

**macOS / Linux / FreeBSD**

```sh
curl -fsSL https://raw.githubusercontent.com/flaxodotdev/srvr/main/install.sh | sh
```

**Windows (PowerShell)**

```powershell
irm https://raw.githubusercontent.com/flaxodotdev/srvr/main/install.ps1 | iex
```

**From source** (any platform with Rust):

```sh
cargo install --git https://github.com/flaxodotdev/srvr
```

You can also download a prebuilt binary from
[Releases](https://github.com/flaxodotdev/srvr/releases).

The installer accepts these environment variables:

| Variable            | Default                                  | Meaning                     |
| ------------------- | ---------------------------------------- | --------------------------- |
| `SRVR_VERSION`      | `latest`                                 | Tag to install, e.g. `v0.1.0` |
| `SRVR_INSTALL_DIR`  | `~/.local/bin` (Windows: `%LOCALAPPDATA%\srvr\bin`) | Where the binary goes |

## Usage

Run `srvr` to open the TUI.

### Tabs

| Tab         | What it shows                                                                                   |
| ----------- | ----------------------------------------------------------------------------------------------- |
| **Running** | Every listening TCP socket (UDP with `u`) with its type, process, PID, CPU, memory, uptime, and who can reach it (`local` or `network`). |
| **Launch**  | Ready-made server templates. ✓ means the tool it needs is installed.                             |
| **Managed** | Servers you started from srvr: running or stopped, their ports, and logs.                   |

### Keys

| Key            | Action                                                    |
| -------------- | --------------------------------------------------------- |
| `1` `2` `3` / `tab` | Switch tabs                                         |
| `↑↓` / `jk`    | Move                                                      |
| `enter`        | Open the action menu for the selected server              |
| `o`            | Open in browser                                           |
| `c`            | Copy URL or connection string (e.g. `postgres://…`)       |
| `h`            | Send an HTTP probe (`HEAD /`) and show the status and `Server` header |
| `s` / `K`      | Stop gracefully / force kill (asks first)                   |
| `R`            | Restart (srvr-launched servers)                           |
| `l`            | View live logs (srvr-launched servers)                    |
| `w` `y` `p`    | Open working directory · copy command line · copy PID     |
| `d`            | Remove a stopped entry from Managed                       |
| `/`            | Filter by port, name, type, or command                     |
| `a` / `u` / `S`| Toggle system services / UDP · cycle sort order           |
| `n`            | Launch a custom command (Launch tab)                      |
| `e`            | Edit your own templates (Launch tab)                      |
| `?`            | Help                                                      |
| `q`            | Quit. Servers you started keep running.                   |

### CLI

Everything in the TUI is also available as a command, which makes srvr scriptable:

```sh
srvr ls                         # what's listening (add --all, --udp, --json)
srvr templates                  # list templates and whether their tools are installed
srvr up static-python -d ./site # serve a folder on the first free port from 8000
srvr up postgres -p 5433        # Postgres 17 in Docker on port 5433
srvr run -p 4000 -- npm run dev # run any command as a managed server
srvr ps                         # servers started by srvr
srvr logs postgres-5433 -f      # follow logs
srvr restart postgres-5433
srvr stop 3000                  # stop whatever is on port 3000 (or a PID / name)
srvr stop 3000 --force          # SIGKILL / taskkill /F
srvr open 5173                  # open in the browser
srvr rm postgres-5433           # forget a stopped server
```

## Built-in templates

| Kind      | Templates                                                                        |
| --------- | -------------------------------------------------------------------------------- |
| Static    | Python `http.server`, `serve` (npx), miniserve, Caddy, Deno, PHP, Ruby, nginx (docker) |
| Dev       | Vite, Jupyter Lab, Hugo, MkDocs, Mailpit, Adminer, httpbin                       |
| Databases | PostgreSQL, MySQL, MariaDB, MongoDB, MinIO (S3), Meilisearch                     |
| Cache     | Redis (native or docker), Valkey, Memcached                                      |
| Queues    | RabbitMQ (with management UI), NATS (JetStream)                                  |
| AI        | Ollama                                                                           |

Docker templates run with `--rm`, so stopping one removes its container. Their
passwords are simple defaults like `srvr`, meant for local development only, and
their ports bind to `127.0.0.1`.

### Your own templates

Press `e` in the Launch tab, or create the file yourself:

- Linux: `~/.config/srvr/templates.json`
- macOS: `~/Library/Application Support/srvr/templates.json`
- Windows: `%APPDATA%\srvr\templates.json`

```json
[
  {
    "id": "my-api",
    "name": "My API",
    "category": "dev",
    "description": "Backend in dev mode",
    "cmd": "npm run dev -- --port {port}",
    "port": 4000,
    "dir": true,
    "env": [["NODE_ENV", "development"]],
    "url": "http://localhost:{port}"
  }
]
```

You can use the placeholders `{port}`, `{dir}`, and `{name}`. The program can list
fallbacks (`python3|python -m …`), and srvr uses the first one installed. Set
`"stop": "docker stop {name}"` for anything that needs its own shutdown command. A
user template with the same `id` as a built-in one replaces it.

## How it works

- **Discovery:** srvr reads the OS socket tables through
  [`listeners`](https://crates.io/crates/listeners) (`/proc` on Linux, `libproc` on
  macOS, `GetExtendedTcpTable` on Windows, `sysctl` on BSD) and gets process details
  from [`sysinfo`](https://crates.io/crates/sysinfo).
- **Detection:** srvr matches the process name, command line, executable path, and
  port against a list of known servers. See [`src/detect.rs`](src/detect.rs); pull
  requests that add servers are welcome.
- **Launching:** srvr runs each server in its own process group (Unix) or a new
  process group with no console window (Windows), sends its output to a log file, and
  records it in `state.json` in your local data directory. Stopping signals the whole
  group, so tools like `npx` don't leave child processes behind. Before signalling,
  srvr checks the process start time, so a reused PID is never killed by mistake.

Processes that belong to other users (for example system nginx or Postgres) hide some
details and can't be stopped unless you run `sudo srvr`.

## Development

```sh
cargo run              # TUI
cargo run -- ls --all  # CLI
cargo test && cargo clippy --all-targets
```

To release, push a tag: `git tag v0.2.0 && git push --tags`. GitHub Actions then
builds binaries for every platform and publishes them on the release.

## License

[MIT](LICENSE)
