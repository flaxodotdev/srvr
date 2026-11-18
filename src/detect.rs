//! Figures out *what* a listening process is (Vite, Postgres, Docker, ...)
//! from its name, command line, executable path and port.

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum Category {
    Web,
    Dev,
    Database,
    Cache,
    Queue,
    Container,
    Ai,
    Network,
    App,
    System,
    Other,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Category::Web => "web",
            Category::Dev => "dev",
            Category::Database => "database",
            Category::Cache => "cache",
            Category::Queue => "queue",
            Category::Container => "container",
            Category::Ai => "ai",
            Category::Network => "network",
            Category::App => "app",
            Category::System => "system",
            Category::Other => "other",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Kind {
    pub label: &'static str,
    pub category: Category,
}

const fn k(label: &'static str, category: Category) -> Kind {
    Kind { label, category }
}

use Category::*;

/// Patterns matched against the full command line for interpreter-hosted servers.
const NODE: &[(&str, Kind)] = &[
    ("vite", k("Vite", Dev)),
    ("next", k("Next.js", Dev)),
    ("nuxt", k("Nuxt", Dev)),
    ("astro", k("Astro", Dev)),
    ("svelte-kit", k("SvelteKit", Dev)),
    ("remix", k("Remix", Dev)),
    ("react-scripts", k("Create React App", Dev)),
    ("ng serve", k("Angular", Dev)),
    ("@angular", k("Angular", Dev)),
    ("webpack", k("Webpack", Dev)),
    ("storybook", k("Storybook", Dev)),
    ("gatsby", k("Gatsby", Dev)),
    ("parcel", k("Parcel", Dev)),
    ("expo", k("Expo", Dev)),
    ("docusaurus", k("Docusaurus", Dev)),
    ("json-server", k("json-server", Dev)),
    ("live-server", k("live-server", Web)),
    ("http-server", k("http-server", Web)),
    ("serve", k("serve (static)", Web)),
    ("nodemon", k("Node (nodemon)", Dev)),
    ("nest", k("NestJS", Dev)),
    ("wrangler", k("Wrangler", Dev)),
    ("tsserver", k("TS Server", Dev)),
];

const PYTHON: &[(&str, Kind)] = &[
    ("http.server", k("Python http.server", Web)),
    ("manage.py", k("Django", Dev)),
    ("django", k("Django", Dev)),
    ("flask", k("Flask", Dev)),
    ("uvicorn", k("Uvicorn (ASGI)", Dev)),
    ("gunicorn", k("Gunicorn", Web)),
    ("hypercorn", k("Hypercorn", Dev)),
    ("fastapi", k("FastAPI", Dev)),
    ("jupyter", k("Jupyter", Dev)),
    ("streamlit", k("Streamlit", Dev)),
    ("gradio", k("Gradio", Ai)),
    ("mkdocs", k("MkDocs", Dev)),
    ("celery", k("Celery", Queue)),
    ("mitmproxy", k("mitmproxy", Network)),
    ("vllm", k("vLLM", Ai)),
];

const RUBY: &[(&str, Kind)] = &[
    ("rails", k("Rails", Dev)),
    ("puma", k("Puma", Web)),
    ("jekyll", k("Jekyll", Dev)),
    ("sidekiq", k("Sidekiq", Queue)),
    ("httpd", k("Ruby httpd", Web)),
];

const JAVA: &[(&str, Kind)] = &[
    ("elasticsearch", k("Elasticsearch", Database)),
    ("opensearch", k("OpenSearch", Database)),
    ("kafka", k("Kafka", Queue)),
    ("zookeeper", k("ZooKeeper", Queue)),
    ("catalina", k("Tomcat", Web)),
    ("jetty", k("Jetty", Web)),
    ("spring", k("Spring Boot", Dev)),
    ("jenkins", k("Jenkins", Dev)),
    ("minecraft", k("Minecraft", App)),
    ("gradle", k("Gradle daemon", Dev)),
    ("idea", k("IntelliJ", App)),
    ("cassandra", k("Cassandra", Database)),
    ("neo4j", k("Neo4j", Database)),
    ("keycloak", k("Keycloak", Web)),
];

const PHP: &[(&str, Kind)] = &[
    ("artisan", k("Laravel", Dev)),
    ("php-fpm", k("PHP-FPM", Web)),
    (" -s ", k("PHP built-in", Web)),
];

/// Matched against the process name (lowercase, prefix/contains semantics noted).
const NAMES: &[(&str, Kind)] = &[
    ("postgres", k("PostgreSQL", Database)),
    ("mysqld", k("MySQL", Database)),
    ("mariadbd", k("MariaDB", Database)),
    ("mongod", k("MongoDB", Database)),
    ("mongos", k("MongoDB", Database)),
    ("redis-server", k("Redis", Cache)),
    ("valkey-server", k("Valkey", Cache)),
    ("keydb-server", k("KeyDB", Cache)),
    ("memcached", k("Memcached", Cache)),
    ("dragonfly", k("Dragonfly", Cache)),
    ("clickhouse", k("ClickHouse", Database)),
    ("influxd", k("InfluxDB", Database)),
    ("cockroach", k("CockroachDB", Database)),
    ("surreal", k("SurrealDB", Database)),
    ("meilisearch", k("Meilisearch", Database)),
    ("typesense", k("Typesense", Database)),
    ("qdrant", k("Qdrant", Database)),
    ("=etcd", k("etcd", Database)),
    ("couchdb", k("CouchDB", Database)),
    ("nginx", k("nginx", Web)),
    ("=httpd", k("Apache httpd", Web)),
    ("apache2", k("Apache httpd", Web)),
    ("caddy", k("Caddy", Web)),
    ("traefik", k("Traefik", Web)),
    ("haproxy", k("HAProxy", Web)),
    ("=envoy", k("Envoy", Web)),
    ("lighttpd", k("lighttpd", Web)),
    ("miniserve", k("miniserve", Web)),
    ("=hugo", k("Hugo", Dev)),
    ("=zola", k("Zola", Dev)),
    ("rabbitmq", k("RabbitMQ", Queue)),
    ("beam.smp", k("Erlang/BEAM", Queue)),
    ("nats-server", k("NATS", Queue)),
    ("mosquitto", k("Mosquitto MQTT", Queue)),
    ("minio", k("MinIO", Database)),
    ("grafana", k("Grafana", Web)),
    ("prometheus", k("Prometheus", Web)),
    ("=consul", k("Consul", Network)),
    ("=vault", k("Vault", Network)),
    ("mailpit", k("Mailpit", Dev)),
    ("mailhog", k("MailHog", Dev)),
    ("com.docker", k("Docker Desktop", Container)),
    ("docker-proxy", k("Docker", Container)),
    ("dockerd", k("Docker", Container)),
    ("vpnkit", k("Docker", Container)),
    ("containerd", k("containerd", Container)),
    ("orbstack", k("OrbStack", Container)),
    ("colima", k("Colima", Container)),
    ("limactl", k("Lima", Container)),
    ("gvproxy", k("Podman", Container)),
    ("podman", k("Podman", Container)),
    ("kubectl", k("kubectl port-forward", Container)),
    ("k3s", k("k3s", Container)),
    ("ollama", k("Ollama", Ai)),
    ("lm studio", k("LM Studio", Ai)),
    ("llama-server", k("llama.cpp", Ai)),
    ("comfyui", k("ComfyUI", Ai)),
    ("sshd", k("SSH server", Network)),
    ("tailscale", k("Tailscale", Network)),
    ("cloudflared", k("Cloudflare Tunnel", Network)),
    ("ngrok", k("ngrok", Network)),
    ("syncthing", k("Syncthing", Network)),
    ("dnsmasq", k("dnsmasq", Network)),
    ("=adb", k("Android Debug Bridge", Dev)),
    ("=deno", k("Deno", Dev)),
    ("=bun", k("Bun", Dev)),
    ("dotnet", k(".NET / Kestrel", Dev)),
    ("code helper", k("VS Code", App)),
    ("cursor", k("Cursor", App)),
    ("=code", k("VS Code", App)),
    ("=zed", k("Zed", App)),
    ("spotify", k("Spotify", App)),
    ("dropbox", k("Dropbox", App)),
    ("=steam", k("Steam", App)),
    ("discord", k("Discord", App)),
    ("slack", k("Slack", App)),
    ("figma", k("Figma", App)),
    ("chrome", k("Chrome", App)),
    ("firefox", k("Firefox", App)),
    ("zoom", k("Zoom", App)),
    ("raycast", k("Raycast", App)),
    ("1password", k("1Password", App)),
    // OS services
    ("controlce", k("AirPlay receiver", System)),
    ("rapportd", k("Continuity (rapportd)", System)),
    ("sharingd", k("Sharing (sharingd)", System)),
    ("mdnsresponder", k("mDNS / Bonjour", System)),
    ("cupsd", k("CUPS printing", System)),
    ("=launchd", k("launchd", System)),
    ("remoted", k("remoted", System)),
    ("systemd", k("systemd", System)),
    ("avahi", k("Avahi mDNS", System)),
    ("chronyd", k("chrony NTP", System)),
    ("rpcbind", k("rpcbind", System)),
    ("svchost", k("Windows service host", System)),
    ("lsass", k("Windows LSASS", System)),
    ("wininit", k("Windows init", System)),
    ("=services", k("Windows services", System)),
    ("spoolsv", k("Print spooler", System)),
    ("=system", k("Windows kernel", System)),
];

const PORTS: &[(u16, Kind)] = &[
    (22, k("SSH", Network)),
    (53, k("DNS", Network)),
    (80, k("HTTP", Web)),
    (443, k("HTTPS", Web)),
    (631, k("CUPS printing", System)),
    (1433, k("SQL Server", Database)),
    (3000, k("Web app", Dev)),
    (3306, k("MySQL", Database)),
    (5432, k("PostgreSQL", Database)),
    (5672, k("AMQP", Queue)),
    (6379, k("Redis", Cache)),
    (8080, k("HTTP (alt)", Web)),
    (9200, k("Elasticsearch", Database)),
    (11211, k("Memcached", Cache)),
    (11434, k("Ollama", Ai)),
    (27017, k("MongoDB", Database)),
];

/// `=foo` patterns must match exactly, everything else is a substring match.
fn first_match(hay: &str, table: &[(&str, Kind)]) -> Option<Kind> {
    table
        .iter()
        .find(|(p, _)| match p.strip_prefix('=') {
            Some(exact) => hay.trim() == exact,
            None => hay.contains(p),
        })
        .map(|(_, k)| *k)
}

/// Strip common executable suffixes and version numbers (`python3.12` → `python`).
fn base_name(name: &str) -> String {
    let n = name.to_ascii_lowercase();
    let n = n.strip_suffix(".exe").unwrap_or(&n);
    n.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.')
        .to_string()
}

fn is_system_path(exe: &str) -> bool {
    let e = exe.to_ascii_lowercase();
    [
        "/system/",
        "/usr/libexec/",
        "/usr/sbin/",
        "/sbin/",
        "/lib/systemd/",
        "/usr/lib/systemd/",
        "c:\\windows\\",
    ]
    .iter()
    .any(|p| e.starts_with(p))
}

pub fn classify(name: &str, exe: &str, cmd: &str, port: u16) -> Kind {
    let base = base_name(name);
    // Skip argv[0] so `node /x/serve.js` doesn't also match on the interpreter path.
    let args = format!(" {} ", cmd.to_ascii_lowercase());

    let interp = match base.as_str() {
        "node" | "nodejs" => Some((NODE, k("Node.js", Dev))),
        "python" | "pythonw" | "uv" => Some((PYTHON, k("Python", Dev))),
        "ruby" | "bundle" => Some((RUBY, k("Ruby", Dev))),
        "java" | "javaw" => Some((JAVA, k("Java", Dev))),
        "php" => Some((PHP, k("PHP", Web))),
        _ => None,
    };
    if let Some((table, fallback)) = interp {
        return first_match(&args, table).unwrap_or(fallback);
    }
    if (base == "bun" || base == "deno")
        && let Some(kind) = first_match(&args, NODE)
    {
        return kind;
    }
    let lname = name.to_ascii_lowercase();
    let lname = lname.strip_suffix(".exe").unwrap_or(&lname);
    if let Some(kind) = first_match(lname, NAMES) {
        return kind;
    }
    if exe.contains("go-build") {
        return k("Go app", Dev);
    }
    let lexe = exe.to_ascii_lowercase();
    let desktop = [
        ".app/contents/",
        "\\program files",
        "/snap/",
        "/flatpak/",
        "/appimage",
    ];
    if desktop.iter().any(|d| lexe.contains(d)) && !is_system_path(exe) {
        return k("Desktop app", App);
    }
    if is_system_path(exe) {
        return k("System service", System);
    }
    if let Some((_, kind)) = PORTS.iter().find(|(p, _)| *p == port) {
        return *kind;
    }
    k("Unknown", Other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_common_servers() {
        assert_eq!(
            classify("node", "", "node node_modules/.bin/vite", 5173).label,
            "Vite"
        );
        assert_eq!(
            classify("python3.12", "", "python3 -m http.server 8000", 8000).label,
            "Python http.server"
        );
        assert_eq!(
            classify("postgres", "", "postgres -D /data", 5432).label,
            "PostgreSQL"
        );
        assert_eq!(
            classify("ControlCenter", "/System/Library/x", "", 5000).category,
            System
        );
        assert_eq!(classify("weird", "", "", 6379).label, "Redis");
        assert_eq!(classify("weird", "", "", 1).label, "Unknown");
    }
}
