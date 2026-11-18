//! Ready-to-run server recipes. Users can add their own in
//! `<config dir>/srvr/templates.json` (same shape as [`Template`]).

use crate::managed::{self, LaunchSpec};
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Template {
    pub id: String,
    pub name: String,
    #[serde(default = "default_category")]
    pub category: String,
    #[serde(default)]
    pub description: String,
    /// Command line. Placeholders: `{port}`, `{dir}`, `{name}`.
    /// The program may list alternatives: `python3|python -m http.server`.
    pub cmd: String,
    /// Default port (srvr picks the next free one if it is taken).
    #[serde(default)]
    pub port: Option<u16>,
    /// Whether the template serves/uses a directory (`{dir}`).
    #[serde(default)]
    pub dir: bool,
    /// Optional command to stop it cleanly (e.g. `docker stop {name}`).
    #[serde(default)]
    pub stop: Option<String>,
    #[serde(default)]
    pub env: Vec<(String, String)>,
    /// What to show/open once it runs, e.g. `http://localhost:{port}`.
    #[serde(default)]
    pub url: Option<String>,
    #[serde(skip)]
    pub user_defined: bool,
}

fn default_category() -> String {
    "custom".into()
}

#[allow(clippy::too_many_arguments)]
fn t(
    id: &str,
    name: &str,
    category: &str,
    description: &str,
    cmd: &str,
    port: u16,
    dir: bool,
    url: Option<&str>,
) -> Template {
    Template {
        id: id.into(),
        name: name.into(),
        category: category.into(),
        description: description.into(),
        cmd: cmd.into(),
        port: Some(port),
        dir,
        stop: None,
        env: Vec::new(),
        url: url.map(Into::into),
        user_defined: false,
    }
}

const HTTP: Option<&str> = Some("http://localhost:{port}");

fn docker(
    id: &str,
    name: &str,
    category: &str,
    description: &str,
    args: &str,
    port: u16,
    url: Option<&str>,
) -> Template {
    let mut tpl = t(
        id,
        name,
        category,
        description,
        &format!("docker run --rm --name {{name}} {args}"),
        port,
        args.contains("{dir}"),
        url,
    );
    tpl.stop = Some("docker stop {name}".into());
    tpl
}

pub fn builtin() -> Vec<Template> {
    let mut v = vec![
        t(
            "static-python",
            "Static files · Python",
            "web",
            "Serve a folder over HTTP with Python's built-in http.server. Zero install on most systems.",
            "python3|python|py -m http.server {port} --bind 127.0.0.1 --directory {dir}",
            8000,
            true,
            HTTP,
        ),
        t(
            "static-node",
            "Static files · serve (Node)",
            "web",
            "Vercel's `serve`: static hosting with clean URLs and directory listings. Runs via npx, nothing to install.",
            "npx --yes serve --listen tcp://127.0.0.1:{port} {dir}",
            3000,
            true,
            HTTP,
        ),
        t(
            "static-miniserve",
            "Static files · miniserve",
            "web",
            "Fast single-binary file server with uploads and QR code (cargo install miniserve).",
            "miniserve --interfaces 127.0.0.1 --port {port} {dir}",
            8080,
            true,
            HTTP,
        ),
        t(
            "static-caddy",
            "Static files · Caddy",
            "web",
            "Caddy file-server with directory browsing.",
            "caddy file-server --root {dir} --listen 127.0.0.1:{port} --browse",
            8080,
            true,
            HTTP,
        ),
        t(
            "static-deno",
            "Static files · Deno",
            "web",
            "Deno's std file server.",
            "deno run --allow-net --allow-read --allow-sys jsr:@std/http/file-server --host 127.0.0.1 --port {port} {dir}",
            4507,
            true,
            HTTP,
        ),
        t(
            "php",
            "PHP built-in server",
            "web",
            "Run PHP scripts from a folder with `php -S`.",
            "php -S 127.0.0.1:{port} -t {dir}",
            8000,
            true,
            HTTP,
        ),
        t(
            "ruby",
            "Static files · Ruby",
            "web",
            "Ruby's un/WEBrick httpd (needs the webrick gem on Ruby 3+).",
            "ruby -run -e httpd {dir} -p {port} -b 127.0.0.1",
            8080,
            true,
            HTTP,
        ),
        t(
            "vite",
            "Vite dev server",
            "dev",
            "Start Vite in a project folder (uses the project's vite, or downloads it).",
            "npx --yes vite {dir} --port {port} --strictPort --host 127.0.0.1",
            5173,
            true,
            HTTP,
        ),
        t(
            "jupyter",
            "Jupyter Lab",
            "dev",
            "Notebook server rooted at a folder. Token is printed in the logs (press l).",
            "jupyter lab --no-browser --ip 127.0.0.1 --port {port} --notebook-dir {dir}",
            8888,
            true,
            HTTP,
        ),
        t(
            "hugo",
            "Hugo site",
            "dev",
            "Live-reloading Hugo server for a site folder.",
            "hugo server --bind 127.0.0.1 --port {port} --source {dir}",
            1313,
            true,
            HTTP,
        ),
        t(
            "mkdocs",
            "MkDocs docs",
            "dev",
            "Live-reloading MkDocs server (run in the folder with mkdocs.yml).",
            "mkdocs serve --dev-addr 127.0.0.1:{port} --config-file {dir}/mkdocs.yml",
            8001,
            true,
            HTTP,
        ),
        t(
            "redis-native",
            "Redis (native)",
            "cache",
            "redis-server from your PATH, no persistence.",
            "redis-server --port {port} --bind 127.0.0.1 --save \"\" --appendonly no",
            6379,
            false,
            Some("redis://localhost:{port}"),
        ),
        t(
            "ollama",
            "Ollama",
            "ai",
            "Local LLM API server (ollama serve).",
            "ollama serve",
            11434,
            false,
            HTTP,
        ),
    ];
    v.last_mut().unwrap().env = vec![("OLLAMA_HOST".into(), "127.0.0.1:{port}".into())];

    v.extend([
        docker("postgres", "PostgreSQL (docker)", "database",
               "PostgreSQL 17 · user postgres / password srvr.",
               "-p 127.0.0.1:{port}:5432 -e POSTGRES_PASSWORD=srvr postgres:17-alpine", 5432,
               Some("postgres://postgres:srvr@localhost:{port}/postgres")),
        docker("mysql", "MySQL (docker)", "database",
               "MySQL 8 · root password srvr, database `app`.",
               "-p 127.0.0.1:{port}:3306 -e MYSQL_ROOT_PASSWORD=srvr -e MYSQL_DATABASE=app mysql:8", 3306,
               Some("mysql://root:srvr@localhost:{port}/app")),
        docker("mariadb", "MariaDB (docker)", "database",
               "MariaDB 11 · root password srvr, database `app`.",
               "-p 127.0.0.1:{port}:3306 -e MARIADB_ROOT_PASSWORD=srvr -e MARIADB_DATABASE=app mariadb:11", 3306,
               Some("mysql://root:srvr@localhost:{port}/app")),
        docker("mongo", "MongoDB (docker)", "database",
               "MongoDB 8, no auth.",
               "-p 127.0.0.1:{port}:27017 mongo:8", 27017, Some("mongodb://localhost:{port}")),
        docker("redis", "Redis (docker)", "cache",
               "Redis 7 in a container.",
               "-p 127.0.0.1:{port}:6379 redis:7-alpine", 6379, Some("redis://localhost:{port}")),
        docker("valkey", "Valkey (docker)", "cache",
               "Valkey 8, the open-source Redis fork.",
               "-p 127.0.0.1:{port}:6379 valkey/valkey:8-alpine", 6379, Some("redis://localhost:{port}")),
        docker("memcached", "Memcached (docker)", "cache",
               "Memcached 1.6.",
               "-p 127.0.0.1:{port}:11211 memcached:1.6-alpine", 11211, None),
        docker("rabbitmq", "RabbitMQ (docker)", "queue",
               "RabbitMQ with the management UI on {port} (AMQP on 5672) · guest/guest.",
               "-p 127.0.0.1:{port}:15672 -p 127.0.0.1:5672:5672 rabbitmq:4-management-alpine", 15672, HTTP),
        docker("nats", "NATS (docker)", "queue",
               "NATS server with JetStream.",
               "-p 127.0.0.1:{port}:4222 nats:2-alpine -js", 4222, Some("nats://localhost:{port}")),
        docker("nginx", "nginx serving a folder (docker)", "web",
               "nginx serving a folder read-only.",
               "-p 127.0.0.1:{port}:80 -v {dir}:/usr/share/nginx/html:ro nginx:alpine", 8080, HTTP),
        docker("minio", "MinIO S3 (docker)", "database",
               "S3-compatible storage. Console on {port}, API on 9000 · minioadmin/minioadmin.",
               "-p 127.0.0.1:{port}:9001 -p 127.0.0.1:9000:9000 minio/minio server /data --console-address :9001", 9001, HTTP),
        docker("mailpit", "Mailpit (docker)", "dev",
               "Catch-all SMTP server for testing email. Web UI on {port}, SMTP on 1025.",
               "-p 127.0.0.1:{port}:8025 -p 127.0.0.1:1025:1025 axllent/mailpit", 8025, HTTP),
        docker("meilisearch", "Meilisearch (docker)", "database",
               "Instant search engine, no master key (dev only).",
               "-p 127.0.0.1:{port}:7700 getmeili/meilisearch:latest", 7700, HTTP),
        docker("adminer", "Adminer DB UI (docker)", "dev",
               "One-page database admin UI. Use host.docker.internal to reach local DBs.",
               "-p 127.0.0.1:{port}:8080 adminer", 8081, HTTP),
        docker("httpbin", "httpbin (docker)", "dev",
               "HTTP request & response testing service.",
               "-p 127.0.0.1:{port}:8080 mccutchen/go-httpbin", 8082, HTTP),
    ]);
    v
}

pub fn user_templates_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(managed::data_dir)
        .join("srvr")
        .join("templates.json")
}

/// Built-ins merged with user templates (a user template with the same id replaces the built-in).
pub fn all() -> (Vec<Template>, Option<String>) {
    let mut list = builtin();
    let path = user_templates_path();
    let mut warning = None;
    if let Ok(text) = std::fs::read_to_string(&path) {
        match serde_json::from_str::<Vec<Template>>(&text) {
            Ok(user) => {
                for mut u in user {
                    u.user_defined = true;
                    if let Some(slot) = list.iter_mut().find(|t| t.id == u.id) {
                        *slot = u;
                    } else {
                        list.push(u);
                    }
                }
            }
            Err(e) => warning = Some(format!("{}: {e}", path.display())),
        }
    }
    (list, warning)
}

pub fn program(cmd: &str) -> String {
    shell_words::split(cmd)
        .ok()
        .and_then(|v| v.into_iter().next())
        .unwrap_or_default()
}

impl Template {
    pub fn program(&self) -> String {
        program(&self.cmd)
    }

    pub fn available(&self) -> bool {
        managed::resolve_program(&self.program()).is_some()
    }

    pub fn default_name(&self, port: Option<u16>) -> String {
        match port {
            Some(p) => format!("{}-{p}", self.id),
            None => self.id.clone(),
        }
    }

    pub fn spec(&self, name: &str, port: Option<u16>, dir: &Path) -> Result<LaunchSpec> {
        let dir = dir
            .canonicalize()
            .map_err(|e| anyhow!("directory {}: {e}", dir.display()))?;
        let dir_s = dir.display().to_string();
        let port_s = port.map(|p| p.to_string()).unwrap_or_default();
        let fill = |s: &str| {
            s.replace("{port}", &port_s)
                .replace("{dir}", &dir_s)
                .replace("{name}", name)
        };
        let split = |s: &str| -> Result<Vec<String>> {
            Ok(shell_words::split(s)
                .map_err(|e| anyhow!("bad command `{s}`: {e}"))?
                .iter()
                .map(|a| fill(a))
                .collect())
        };
        let mut argv = split(&self.cmd)?;
        if argv.is_empty() {
            return Err(anyhow!("empty command"));
        }
        // `python3|python` → whichever is installed (first one if none are).
        if argv[0].contains('|') {
            let alts: Vec<&str> = argv[0].split('|').collect();
            argv[0] = alts
                .iter()
                .find(|p| which::which(p).is_ok())
                .unwrap_or(&alts[0])
                .to_string();
        }
        Ok(LaunchSpec {
            name: name.to_string(),
            template: self.id.clone(),
            argv,
            cwd: dir,
            env: self.env.iter().map(|(k, v)| (k.clone(), fill(v))).collect(),
            port,
            stop_argv: self.stop.as_deref().map(split).transpose()?,
            url: self.url.as_deref().map(fill),
        })
    }
}

/// An ad-hoc template for a command typed by the user.
pub fn custom(cmd: &str, port: Option<u16>) -> Template {
    Template {
        id: "custom".into(),
        name: "Custom command".into(),
        category: "custom".into(),
        description: String::new(),
        cmd: cmd.into(),
        port,
        dir: true,
        stop: None,
        env: Vec::new(),
        url: port.map(|_| "http://localhost:{port}".into()),
        user_defined: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_parse_and_fill() {
        let dir = std::env::temp_dir();
        for t in builtin() {
            let spec = t.spec("x", t.port, &dir).unwrap();
            assert!(!spec.argv.is_empty(), "{}", t.id);
            assert!(
                spec.argv
                    .iter()
                    .all(|a| !a.contains("{port}") && !a.contains("{dir}")),
                "{}",
                t.id
            );
        }
    }
}
