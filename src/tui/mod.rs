//! Interactive terminal UI.

mod ui;

use crate::actions;
use crate::detect::Category;
use crate::managed::{self, Managed};
use crate::scan::{ManagedView, Proto, Scanner, Server, Snapshot};
use crate::templates::{self, Template};
use anyhow::Result;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::widgets::{ListState, TableState};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Child;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Running,
    Launch,
    Managed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortBy {
    Port,
    Name,
    Memory,
    Cpu,
}

impl SortBy {
    fn next(self) -> Self {
        match self {
            SortBy::Port => SortBy::Name,
            SortBy::Name => SortBy::Memory,
            SortBy::Memory => SortBy::Cpu,
            SortBy::Cpu => SortBy::Port,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            SortBy::Port => "port",
            SortBy::Name => "name",
            SortBy::Memory => "memory",
            SortBy::Cpu => "cpu",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Open,
    CopyUrl,
    Probe,
    Logs,
    Restart,
    Stop,
    Kill,
    OpenDir,
    CopyCmd,
    CopyPid,
    Remove,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::Open => "Open in browser",
            Action::CopyUrl => "Copy URL / connection string",
            Action::Probe => "Probe HTTP (HEAD /)",
            Action::Logs => "View logs",
            Action::Restart => "Restart / start again",
            Action::Stop => "Stop (graceful)",
            Action::Kill => "Force kill",
            Action::OpenDir => "Open working directory",
            Action::CopyCmd => "Copy command line",
            Action::CopyPid => "Copy PID",
            Action::Remove => "Remove from list",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Action::Open => "o",
            Action::CopyUrl => "c",
            Action::Probe => "h",
            Action::Logs => "l",
            Action::Restart => "R",
            Action::Stop => "s",
            Action::Kill => "K",
            Action::OpenDir => "w",
            Action::CopyCmd => "y",
            Action::CopyPid => "p",
            Action::Remove => "d",
        }
    }
}

/// What an action applies to.
#[derive(Clone)]
pub enum Target {
    Server(Box<Server>),
    Managed(Box<ManagedView>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FieldKey {
    Command,
    Name,
    Port,
    Dir,
}

pub struct Field {
    pub key: FieldKey,
    pub label: &'static str,
    pub value: String,
    pub hint: &'static str,
}

pub struct Form {
    pub tpl: Template,
    pub fields: Vec<Field>,
    pub focus: usize,
    pub error: Option<String>,
}

pub enum Modal {
    None,
    Menu {
        target: Target,
        items: Vec<Action>,
        state: ListState,
    },
    Confirm {
        target: Target,
        action: Action,
        message: String,
    },
    Form(Form),
    Logs {
        name: String,
        path: PathBuf,
        offset: usize,
        follow: bool,
    },
    Help,
}

pub enum Msg {
    Snapshot(Snapshot),
    Probe(u16, String),
    Status(String, bool),
    Child(Child),
}

pub struct App {
    pub tab: Tab,
    pub snap: Snapshot,
    pub loaded: bool,
    pub templates: Vec<Template>,
    pub available: Vec<bool>,
    pub server_state: TableState,
    pub launch_state: TableState,
    pub managed_state: TableState,
    pub modal: Modal,
    pub show_system: bool,
    pub show_udp: bool,
    pub sort: SortBy,
    pub filter: String,
    pub filtering: bool,
    pub probes: HashMap<u16, String>,
    pub status: Option<(String, bool, Instant)>,
    pub launch_dir: PathBuf,
    /// Managed entry to select once it appears in a snapshot.
    pending_select: Option<String>,
    quit: bool,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    scan_now: Sender<()>,
    children: Vec<Child>,
}

pub fn run() -> Result<()> {
    let mut terminal = ratatui::init();
    let res = App::new().run(&mut terminal);
    ratatui::restore();
    res
}

impl App {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let (scan_now, scan_rx) = mpsc::channel::<()>();
        let stx = tx.clone();
        std::thread::spawn(move || {
            let mut scanner = Scanner::new();
            loop {
                if stx.send(Msg::Snapshot(scanner.snapshot())).is_err() {
                    break;
                }
                match scan_rx.recv_timeout(Duration::from_secs(2)) {
                    Ok(()) | Err(RecvTimeoutError::Timeout) => while scan_rx.try_recv().is_ok() {},
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        let (templates, warning) = templates::all();
        let available = templates.iter().map(|t| t.available()).collect();
        let mut app = App {
            tab: Tab::Running,
            snap: Snapshot::default(),
            loaded: false,
            templates,
            available,
            server_state: TableState::default().with_selected(0),
            launch_state: TableState::default().with_selected(0),
            managed_state: TableState::default().with_selected(0),
            modal: Modal::None,
            show_system: false,
            show_udp: false,
            sort: SortBy::Port,
            filter: String::new(),
            filtering: false,
            probes: HashMap::new(),
            status: None,
            launch_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            pending_select: None,
            quit: false,
            tx,
            rx,
            scan_now,
            children: Vec::new(),
        };
        if let Some(w) = warning {
            app.set_status(format!("templates.json: {w}"), true);
        }
        app
    }

    fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.quit {
            self.drain();
            self.children
                .retain_mut(|c| matches!(c.try_wait(), Ok(None)));
            terminal.draw(|f| ui::draw(f, &mut self))?;
            if event::poll(Duration::from_millis(200))? {
                match event::read()? {
                    Event::Key(k) if k.kind == KeyEventKind::Press => self.on_key(k),
                    _ => {}
                }
            }
        }
        // Detached servers keep running after srvr exits; nothing to clean up.
        Ok(())
    }

    fn drain(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Snapshot(s) => self.apply_snapshot(s),
                Msg::Probe(port, r) => {
                    self.set_status(format!(":{port} → {r}"), false);
                    self.probes.insert(port, r);
                }
                Msg::Status(s, err) => {
                    self.set_status(s, err);
                    self.rescan();
                }
                Msg::Child(c) => {
                    self.children.push(c);
                    self.rescan();
                }
            }
        }
    }

    fn apply_snapshot(&mut self, snap: Snapshot) {
        // Keep the same row selected across refreshes.
        let sel_server = self.selected_server().map(|s| (s.port, s.proto, s.pid));
        let sel_managed = self.selected_managed().map(|m| m.entry.name.clone());
        self.snap = snap;
        // A freshly launched server wins over the previous selection once it shows up.
        let sel_managed = match self.pending_select.take() {
            Some(p) if self.snap.managed.iter().any(|m| m.entry.name == p) => Some(p),
            Some(p) => {
                self.pending_select = Some(p);
                sel_managed
            }
            None => sel_managed,
        };
        self.loaded = true;
        if let Some(key) = sel_server
            && let Some(i) = self
                .visible_servers()
                .iter()
                .position(|s| (s.port, s.proto, s.pid) == key)
        {
            self.server_state.select(Some(i));
        }
        if let Some(name) = sel_managed
            && let Some(i) = self.snap.managed.iter().position(|m| m.entry.name == name)
        {
            self.managed_state.select(Some(i));
        }
        let n = self.visible_servers().len();
        clamp(&mut self.server_state, n);
        clamp(&mut self.managed_state, self.snap.managed.len());
    }

    pub fn set_status(&mut self, s: impl Into<String>, err: bool) {
        self.status = Some((s.into(), err, Instant::now()));
    }

    fn rescan(&self) {
        let _ = self.scan_now.send(());
    }

    pub fn visible_servers(&self) -> Vec<&Server> {
        let f = self.filter.to_ascii_lowercase();
        let mut v: Vec<&Server> = self
            .snap
            .servers
            .iter()
            .filter(|s| self.show_system || s.kind.category != Category::System)
            .filter(|s| self.show_udp || s.proto == Proto::Tcp)
            .filter(|s| {
                f.is_empty()
                    || s.port.to_string().contains(&f)
                    || s.name.to_ascii_lowercase().contains(&f)
                    || s.kind.label.to_ascii_lowercase().contains(&f)
                    || s.kind.category.label().contains(&f)
                    || s.cmd.to_ascii_lowercase().contains(&f)
                    || s.managed.as_deref().is_some_and(|m| m.contains(&f))
            })
            .collect();
        match self.sort {
            SortBy::Port => v.sort_by_key(|s| (s.port, s.proto, s.pid)),
            SortBy::Name => v.sort_by_key(|s| (s.name.to_ascii_lowercase(), s.port)),
            SortBy::Memory => v.sort_by(|a, b| b.memory.cmp(&a.memory).then(a.port.cmp(&b.port))),
            SortBy::Cpu => v.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(a.port.cmp(&b.port))),
        }
        v
    }

    pub fn selected_server(&self) -> Option<&Server> {
        self.server_state
            .selected()
            .and_then(|i| self.visible_servers().get(i).copied())
    }

    pub fn selected_managed(&self) -> Option<&ManagedView> {
        self.managed_state
            .selected()
            .and_then(|i| self.snap.managed.get(i))
    }

    /// Template list rows: index 0 is "custom command", then templates.
    pub fn selected_template(&self) -> Option<usize> {
        self.launch_state.selected()
    }

    fn current_target(&self) -> Option<Target> {
        match self.tab {
            Tab::Running => self
                .selected_server()
                .cloned()
                .map(|s| Target::Server(Box::new(s))),
            Tab::Managed => self
                .selected_managed()
                .cloned()
                .map(|m| Target::Managed(Box::new(m))),
            Tab::Launch => None,
        }
    }

    fn managed_for(&self, t: &Target) -> Option<ManagedView> {
        match t {
            Target::Managed(m) => Some((**m).clone()),
            Target::Server(s) => s
                .managed
                .as_ref()
                .and_then(|n| self.snap.managed.iter().find(|m| &m.entry.name == n))
                .cloned(),
        }
    }

    pub fn actions_for(&self, t: &Target) -> Vec<Action> {
        use Action::*;
        let m = self.managed_for(t);
        let mut v = Vec::new();
        match t {
            Target::Server(s) => {
                v.extend([Open, CopyUrl, Probe]);
                if m.is_some() {
                    v.extend([Logs, Restart]);
                }
                v.extend([Stop, Kill]);
                if s.cwd.is_some() {
                    v.push(OpenDir);
                }
                v.extend([CopyCmd, CopyPid]);
            }
            Target::Managed(mv) => {
                if mv.entry.url.is_some() || !mv.ports.is_empty() {
                    v.extend([Open, CopyUrl]);
                }
                if !mv.ports.is_empty() {
                    v.push(Probe);
                }
                v.extend([Logs, Restart]);
                if mv.alive {
                    v.extend([Stop, Kill]);
                } else {
                    v.push(Remove);
                }
                v.extend([OpenDir, CopyCmd, CopyPid]);
            }
        }
        v
    }

    fn on_key(&mut self, k: KeyEvent) {
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        match std::mem::replace(&mut self.modal, Modal::None) {
            Modal::None => self.on_key_main(k),
            Modal::Help => {}
            Modal::Menu {
                target,
                items,
                mut state,
            } => {
                let mut keep = true;
                match k.code {
                    KeyCode::Esc | KeyCode::Char('q') => keep = false,
                    KeyCode::Up | KeyCode::Char('k') => state.select_previous(),
                    KeyCode::Down | KeyCode::Char('j') => state.select_next(),
                    KeyCode::Enter => {
                        keep = false;
                        if let Some(a) = state.selected().and_then(|i| items.get(i)) {
                            self.request(target.clone(), *a);
                        }
                    }
                    KeyCode::Char(c) => {
                        if let Some(a) = items.iter().find(|a| a.key() == c.to_string()) {
                            keep = false;
                            self.request(target.clone(), *a);
                        }
                    }
                    _ => {}
                }
                if keep {
                    let n = items.len();
                    if let Some(i) = state.selected() {
                        state.select(Some(i.min(n.saturating_sub(1))));
                    }
                    self.modal = Modal::Menu {
                        target,
                        items,
                        state,
                    };
                }
            }
            Modal::Confirm {
                target,
                action,
                message,
            } => match k.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    self.perform(target, action)
                }
                KeyCode::Char('n') | KeyCode::Esc | KeyCode::Char('q') => {}
                _ => {
                    self.modal = Modal::Confirm {
                        target,
                        action,
                        message,
                    }
                }
            },
            Modal::Form(form) => self.on_key_form(form, k),
            Modal::Logs {
                name,
                path,
                mut offset,
                mut follow,
            } => {
                let mut keep = true;
                match k.code {
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('l') => keep = false,
                    KeyCode::Up | KeyCode::Char('k') => {
                        offset += 1;
                        follow = false;
                    }
                    KeyCode::Down | KeyCode::Char('j') => offset = offset.saturating_sub(1),
                    KeyCode::PageUp => {
                        offset += 20;
                        follow = false;
                    }
                    KeyCode::PageDown => offset = offset.saturating_sub(20),
                    KeyCode::Char('g') | KeyCode::Home => {
                        offset = usize::MAX / 2;
                        follow = false;
                    }
                    KeyCode::Char('G') | KeyCode::End | KeyCode::Char('f') => {
                        offset = 0;
                        follow = true;
                    }
                    KeyCode::Char('o') => {
                        let _ = open::that_detached(&path);
                    }
                    _ => {}
                }
                if offset == 0 {
                    follow = true;
                }
                if keep {
                    self.modal = Modal::Logs {
                        name,
                        path,
                        offset,
                        follow,
                    };
                }
            }
        }
    }

    fn on_key_main(&mut self, k: KeyEvent) {
        if self.filtering {
            match k.code {
                KeyCode::Esc => {
                    self.filter.clear();
                    self.filtering = false;
                }
                KeyCode::Enter | KeyCode::Down | KeyCode::Up => self.filtering = false,
                KeyCode::Backspace => {
                    self.filter.pop();
                }
                KeyCode::Char(c) => self.filter.push(c),
                _ => {}
            }
            self.server_state.select(Some(0));
            return;
        }

        let len = match self.tab {
            Tab::Running => self.visible_servers().len(),
            Tab::Launch => self.templates.len() + 1,
            Tab::Managed => self.snap.managed.len(),
        };
        let state = match self.tab {
            Tab::Running => &mut self.server_state,
            Tab::Launch => &mut self.launch_state,
            Tab::Managed => &mut self.managed_state,
        };
        match k.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.modal = Modal::Help,
            KeyCode::Tab | KeyCode::Right => self.tab = next_tab(self.tab, true),
            KeyCode::BackTab | KeyCode::Left => self.tab = next_tab(self.tab, false),
            KeyCode::Char('1') => self.tab = Tab::Running,
            KeyCode::Char('2') => self.tab = Tab::Launch,
            KeyCode::Char('3') => self.tab = Tab::Managed,
            KeyCode::Down | KeyCode::Char('j') => move_sel(state, len, 1),
            KeyCode::Up | KeyCode::Char('k') => move_sel(state, len, -1),
            KeyCode::PageDown => move_sel(state, len, 10),
            KeyCode::PageUp => move_sel(state, len, -10),
            KeyCode::Home | KeyCode::Char('g') => state.select(Some(0)),
            KeyCode::End | KeyCode::Char('G') => state.select(Some(len.saturating_sub(1))),
            KeyCode::Char('r') => {
                self.rescan();
                self.set_status("refreshing…", false);
            }
            _ => match self.tab {
                Tab::Launch => self.on_key_launch(k),
                _ => self.on_key_list(k),
            },
        }
    }

    fn on_key_list(&mut self, k: KeyEvent) {
        if self.tab == Tab::Running {
            match k.code {
                KeyCode::Char('/') => {
                    self.filtering = true;
                    return;
                }
                KeyCode::Char('a') => {
                    self.show_system = !self.show_system;
                    self.set_status(
                        if self.show_system {
                            "showing system services"
                        } else {
                            "hiding system services"
                        },
                        false,
                    );
                    return;
                }
                KeyCode::Char('u') => {
                    self.show_udp = !self.show_udp;
                    self.set_status(
                        if self.show_udp {
                            "showing UDP"
                        } else {
                            "TCP only"
                        },
                        false,
                    );
                    return;
                }
                KeyCode::Char('S') => {
                    self.sort = self.sort.next();
                    self.set_status(format!("sorted by {}", self.sort.label()), false);
                    return;
                }
                KeyCode::Esc if !self.filter.is_empty() => {
                    self.filter.clear();
                    return;
                }
                _ => {}
            }
        }
        let Some(target) = self.current_target() else {
            return;
        };
        let items = self.actions_for(&target);
        match k.code {
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.modal = Modal::Menu {
                    target,
                    items,
                    state: ListState::default().with_selected(Some(0)),
                }
            }
            KeyCode::Char(c) => {
                if let Some(a) = items.iter().find(|a| a.key() == c.to_string()) {
                    self.request(target, *a);
                }
            }
            _ => {}
        }
    }

    fn on_key_launch(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Enter | KeyCode::Char(' ') => match self.selected_template() {
                Some(0) => self.open_form(None),
                Some(i) => self.open_form(Some(i - 1)),
                None => {}
            },
            KeyCode::Char('n') => self.open_form(None),
            KeyCode::Char('e') => self.edit_user_templates(),
            _ => {}
        }
    }

    fn edit_user_templates(&mut self) {
        let path = templates::user_templates_path();
        if !path.exists() {
            let example = vec![Template {
                id: "my-api".into(),
                name: "My API (example)".into(),
                category: "custom".into(),
                description: "Edit me! Placeholders: {port} {dir} {name}".into(),
                cmd: "npm run dev -- --port {port}".into(),
                port: Some(4000),
                dir: true,
                stop: None,
                env: vec![("NODE_ENV".into(), "development".into())],
                url: Some("http://localhost:{port}".into()),
                user_defined: true,
            }];
            let write = || -> Result<()> {
                std::fs::create_dir_all(path.parent().unwrap())?;
                std::fs::write(&path, serde_json::to_string_pretty(&example)?)?;
                Ok(())
            };
            if let Err(e) = write() {
                self.set_status(format!("could not create {}: {e}", path.display()), true);
                return;
            }
        }
        match open::that_detached(&path) {
            Ok(()) => self.set_status(
                format!("opened {} — restart srvr to reload", path.display()),
                false,
            ),
            Err(e) => self.set_status(format!("{}: {e}", path.display()), true),
        }
    }

    fn open_form(&mut self, idx: Option<usize>) {
        let tpl = match idx {
            Some(i) => self.templates[i].clone(),
            None => templates::custom("", None),
        };
        let port = tpl.port.map(managed::free_port);
        let mut fields = Vec::new();
        if idx.is_none() {
            fields.push(Field {
                key: FieldKey::Command,
                label: "Command",
                value: String::new(),
                hint: "e.g. npm run dev -- --port {port}",
            });
        }
        fields.push(Field {
            key: FieldKey::Port,
            label: "Port",
            value: port.map(|p| p.to_string()).unwrap_or_default(),
            hint: "empty = none",
        });
        if tpl.dir || idx.is_none() {
            fields.push(Field {
                key: FieldKey::Dir,
                label: "Directory",
                value: self.launch_dir.display().to_string(),
                hint: "folder to serve / run in",
            });
        }
        fields.push(Field {
            key: FieldKey::Name,
            label: "Name",
            value: if idx.is_some() {
                tpl.default_name(port)
            } else {
                String::new()
            },
            hint: "empty = auto",
        });
        self.modal = Modal::Form(Form {
            tpl,
            fields,
            focus: 0,
            error: None,
        });
    }

    fn on_key_form(&mut self, mut form: Form, k: KeyEvent) {
        let n = form.fields.len();
        match k.code {
            KeyCode::Esc => return,
            KeyCode::Tab | KeyCode::Down => form.focus = (form.focus + 1) % n,
            KeyCode::BackTab | KeyCode::Up => form.focus = (form.focus + n - 1) % n,
            KeyCode::Backspace => {
                form.fields[form.focus].value.pop();
            }
            KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                form.fields[form.focus].value.clear();
            }
            KeyCode::Char(c) => {
                let f = &mut form.fields[form.focus];
                if f.key != FieldKey::Port || c.is_ascii_digit() {
                    f.value.push(c);
                }
                // Keep the auto-generated name in sync with the port.
                if f.key == FieldKey::Port {
                    self.sync_name(&mut form);
                }
            }
            KeyCode::Enter => match self.submit(&form) {
                Ok(()) => return,
                Err(e) => form.error = Some(format!("{e:#}")),
            },
            _ => {}
        }
        if matches!(k.code, KeyCode::Backspace) && form.fields[form.focus].key == FieldKey::Port {
            self.sync_name(&mut form);
        }
        self.modal = Modal::Form(form);
    }

    fn sync_name(&self, form: &mut Form) {
        if form.tpl.id == "custom" {
            return;
        }
        let port = form.get(FieldKey::Port).parse().ok();
        let auto = form.tpl.default_name(port);
        if let Some(f) = form.fields.iter_mut().find(|f| f.key == FieldKey::Name)
            && f.value.starts_with(&form.tpl.id)
        {
            f.value = auto;
        }
    }

    fn submit(&mut self, form: &Form) -> Result<()> {
        let port_s = form.get(FieldKey::Port);
        let port: Option<u16> = if port_s.trim().is_empty() {
            None
        } else {
            Some(
                port_s
                    .trim()
                    .parse()
                    .map_err(|_| anyhow::anyhow!("port must be 1-65535"))?,
            )
        };
        if let Some(p) = port
            && !managed::port_free(p)
        {
            anyhow::bail!(
                "port {p} is in use — next free is {}",
                managed::free_port(p)
            );
        }
        let mut tpl = form.tpl.clone();
        if tpl.id == "custom" {
            let cmd = form.get(FieldKey::Command).trim().to_string();
            if cmd.is_empty() {
                anyhow::bail!("enter a command");
            }
            tpl = templates::custom(&cmd, port);
        }
        let dir_s = form.get(FieldKey::Dir);
        let dir = if dir_s.trim().is_empty() {
            self.launch_dir.clone()
        } else {
            expand_tilde(dir_s.trim())
        };
        let name = form.get(FieldKey::Name).trim().to_string();
        let name = if name.is_empty() {
            if tpl.id == "custom" {
                PathBuf::from(templates::program(&tpl.cmd))
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "custom".into())
            } else {
                tpl.default_name(port)
            }
        } else {
            name.replace(['/', '\\', ' '], "-")
        };
        let name = managed::unique_name(&name);
        if managed::resolve_program(&tpl.program()).is_none() {
            anyhow::bail!(
                "`{}` is not installed / not on PATH",
                tpl.program().replace('|', "` or `")
            );
        }
        let spec = tpl.spec(&name, port, &dir)?;
        let (m, child) = managed::launch(spec)?;
        self.children.push(child);
        self.set_status(
            format!(
                "started {} (pid {}){}",
                m.name,
                m.pid,
                m.url
                    .as_ref()
                    .map(|u| format!(" → {u}"))
                    .unwrap_or_default()
            ),
            false,
        );
        self.tab = Tab::Managed;
        self.pending_select = Some(m.name.clone());
        self.rescan();
        Ok(())
    }

    /// Ask for confirmation for destructive actions, run the rest directly.
    fn request(&mut self, target: Target, action: Action) {
        let what = match &target {
            Target::Server(s) => format!("{} (pid {}, :{})", s.kind.label, s.pid, s.port),
            Target::Managed(m) => format!("{} (pid {})", m.entry.name, m.entry.pid),
        };
        let message = match action {
            Action::Stop => format!("Stop {what}?"),
            Action::Kill => format!("Force kill {what}? It won't get a chance to clean up."),
            Action::Restart => format!("Restart {what}?"),
            _ => return self.perform(target, action),
        };
        self.modal = Modal::Confirm {
            target,
            action,
            message,
        };
    }

    fn perform(&mut self, target: Target, action: Action) {
        let managed = self.managed_for(&target);
        let entry: Option<Managed> = managed.as_ref().map(|m| m.entry.clone());
        let (url, pid, cmd, cwd, port, addr) = match &target {
            Target::Server(s) => {
                let url = entry
                    .as_ref()
                    .and_then(|m| m.url.clone())
                    .unwrap_or_else(|| s.url());
                (
                    Some(url),
                    s.pid,
                    s.cmd.clone(),
                    s.cwd.clone().map(PathBuf::from),
                    Some(s.port),
                    Some(s.primary_addr()),
                )
            }
            Target::Managed(m) => {
                let port = m.ports.first().copied().or(m.entry.port);
                let url = m
                    .entry
                    .url
                    .clone()
                    .or(port.map(|p| format!("http://localhost:{p}")));
                (
                    url,
                    m.entry.pid,
                    shell_words::join(&m.entry.argv),
                    Some(m.entry.cwd.clone()),
                    port,
                    None,
                )
            }
        };
        match action {
            Action::Open => match url {
                Some(u) if u.starts_with("http") => match open::that_detached(&u) {
                    Ok(()) => self.set_status(format!("opened {u}"), false),
                    Err(e) => self.set_status(format!("could not open browser: {e}"), true),
                },
                Some(u) => self.set_status(format!("not a web URL: {u} (press c to copy)"), true),
                None => self.set_status("no URL for this server", true),
            },
            Action::CopyUrl => self.copy(url.unwrap_or_default()),
            Action::CopyCmd => self.copy(cmd),
            Action::CopyPid => self.copy(pid.to_string()),
            Action::OpenDir => match cwd {
                Some(d) => match open::that_detached(&d) {
                    Ok(()) => self.set_status(format!("opened {}", d.display()), false),
                    Err(e) => self.set_status(format!("{e}"), true),
                },
                None => self.set_status("working directory unknown", true),
            },
            Action::Probe => {
                let Some(port) = port else {
                    return self.set_status("no port to probe", true);
                };
                let addr = addr.unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
                self.set_status(format!("probing :{port}…"), false);
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(Msg::Probe(port, actions::probe_http(addr, port)));
                });
            }
            Action::Logs => match entry {
                Some(m) => {
                    self.modal = Modal::Logs {
                        name: m.name,
                        path: m.log,
                        offset: 0,
                        follow: true,
                    }
                }
                None => {
                    self.set_status("logs are only available for servers started by srvr", true)
                }
            },
            Action::Remove => {
                if let Some(m) = entry {
                    match managed::remove(&m.name) {
                        Ok(()) => self.set_status(format!("removed {}", m.name), false),
                        Err(e) => self.set_status(format!("{e}"), true),
                    }
                    self.rescan();
                }
            }
            Action::Stop | Action::Kill => {
                let force = action == Action::Kill;
                let tx = self.tx.clone();
                let label = match &target {
                    Target::Server(s) => format!("{} (pid {})", s.name, s.pid),
                    Target::Managed(m) => m.entry.name.clone(),
                };
                self.set_status(format!("stopping {label}…"), false);
                std::thread::spawn(move || {
                    let res = match &entry {
                        Some(m) => managed::stop(m, force),
                        None => actions::signal(pid, force),
                    };
                    let msg = match res {
                        Ok(()) => Msg::Status(
                            format!("{} {label}", if force { "killed" } else { "stopped" }),
                            false,
                        ),
                        Err(e) => Msg::Status(format!("could not stop {label}: {e:#}"), true),
                    };
                    let _ = tx.send(msg);
                });
            }
            Action::Restart => {
                let Some(m) = entry else {
                    return self.set_status("only servers started by srvr can be restarted", true);
                };
                let tx = self.tx.clone();
                self.set_status(format!("restarting {}…", m.name), false);
                std::thread::spawn(move || {
                    let alive = m.alive(&managed::system_for(m.pid));
                    let res = if alive {
                        managed::restart(&m)
                    } else {
                        managed::launch(managed::LaunchSpec {
                            name: m.name.clone(),
                            template: m.template.clone(),
                            argv: m.argv.clone(),
                            cwd: m.cwd.clone(),
                            env: m.env.clone(),
                            port: m.port,
                            stop_argv: m.stop_argv.clone(),
                            url: m.url.clone(),
                        })
                    };
                    match res {
                        Ok((m, child)) => {
                            let _ = tx.send(Msg::Child(child));
                            let _ = tx.send(Msg::Status(
                                format!("{} running (pid {})", m.name, m.pid),
                                false,
                            ));
                        }
                        Err(e) => {
                            let _ = tx.send(Msg::Status(format!("restart failed: {e:#}"), true));
                        }
                    }
                });
            }
        }
    }

    fn copy(&mut self, text: String) {
        if text.is_empty() {
            return self.set_status("nothing to copy", true);
        }
        match actions::copy(&text) {
            Ok(()) => self.set_status(format!("copied {text}"), false),
            Err(e) => self.set_status(format!("copy failed: {e}"), true),
        }
    }
}

impl Form {
    pub fn get(&self, key: FieldKey) -> &str {
        self.fields
            .iter()
            .find(|f| f.key == key)
            .map(|f| f.value.as_str())
            .unwrap_or("")
    }
}

fn expand_tilde(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest.trim_start_matches(['/', '\\']));
    }
    PathBuf::from(p)
}

fn next_tab(t: Tab, fwd: bool) -> Tab {
    use Tab::*;
    match (t, fwd) {
        (Running, true) | (Managed, false) => Launch,
        (Launch, true) | (Running, false) => Managed,
        (Managed, true) | (Launch, false) => Running,
    }
}

fn move_sel(state: &mut TableState, len: usize, delta: isize) {
    if len == 0 {
        state.select(None);
        return;
    }
    let cur = state.selected().unwrap_or(0) as isize;
    state.select(Some((cur + delta).clamp(0, len as isize - 1) as usize));
}

fn clamp(state: &mut TableState, len: usize) {
    match (state.selected(), len) {
        (_, 0) => state.select(None),
        (None, _) => state.select(Some(0)),
        (Some(i), n) if i >= n => state.select(Some(n - 1)),
        _ => {}
    }
}
