use super::{App, Field, Modal, Tab, Target};
use crate::detect::Category;
use crate::scan::{Exposure, Proto, human_bytes, human_duration};
use crate::truncate;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table, Wrap,
};
use std::time::Duration;

const ACCENT: Color = Color::Rgb(255, 135, 95);
const DIM: Color = Color::DarkGray;
const SEL_BG: Color = Color::Rgb(45, 45, 60);

pub fn cat_color(c: Category) -> Color {
    match c {
        Category::Web => Color::Cyan,
        Category::Dev => Color::Green,
        Category::Database => Color::Magenta,
        Category::Cache => Color::LightRed,
        Category::Queue => Color::Yellow,
        Category::Container => Color::LightBlue,
        Category::Ai => Color::LightGreen,
        Category::Network => Color::LightCyan,
        Category::App => Color::Gray,
        Category::System => Color::DarkGray,
        Category::Other => Color::White,
    }
}

fn tpl_color(cat: &str) -> Color {
    match cat {
        "web" => cat_color(Category::Web),
        "dev" => cat_color(Category::Dev),
        "database" => cat_color(Category::Database),
        "cache" => cat_color(Category::Cache),
        "queue" => cat_color(Category::Queue),
        "ai" => cat_color(Category::Ai),
        _ => Color::White,
    }
}

fn block(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(DIM))
        .title(Span::styled(
            format!(" {title} "),
            Style::new().fg(ACCENT).bold(),
        ))
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(f.area());
    draw_header(f, app, header);
    match app.tab {
        Tab::Running => draw_running(f, app, body),
        Tab::Launch => draw_launch(f, app, body),
        Tab::Managed => draw_managed(f, app, body),
    }
    draw_footer(f, app, footer);

    match &app.modal {
        Modal::None => {}
        Modal::Help => draw_help(f),
        Modal::Menu { .. } => draw_menu(f, app),
        Modal::Confirm { message, .. } => draw_confirm(f, message),
        Modal::Form(_) => draw_form(f, app),
        Modal::Logs { .. } => draw_logs(f, app),
    }
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let running = app.snap.managed.iter().filter(|m| m.alive).count();
    let tabs = [
        (
            Tab::Running,
            "1",
            format!("Running {}", app.visible_servers().len()),
        ),
        (Tab::Launch, "2", format!("Launch {}", app.templates.len())),
        (
            Tab::Managed,
            "3",
            format!("Managed {running}/{}", app.snap.managed.len()),
        ),
    ];
    let mut spans = vec![
        Span::styled(" ▲ srvr ", Style::new().fg(Color::Black).bg(ACCENT).bold()),
        Span::raw("  "),
    ];
    for (t, key, label) in tabs {
        let style = if t == app.tab {
            Style::new().fg(ACCENT).bold().underlined()
        } else {
            Style::new().fg(Color::Gray)
        };
        spans.push(Span::styled(key.to_string(), Style::new().fg(DIM)));
        spans.push(Span::styled(format!(" {label}"), style));
        spans.push(Span::raw("   "));
    }
    f.render_widget(Line::from(spans), area);
    let host = sysinfo::System::host_name().unwrap_or_default();
    f.render_widget(
        Line::from(Span::styled(
            format!("{host} · v{} ", env!("CARGO_PKG_VERSION")),
            Style::new().fg(DIM),
        ))
        .alignment(Alignment::Right),
        area,
    );
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    if let Some((msg, err, at)) = &app.status
        && at.elapsed() < Duration::from_secs(6)
    {
        let style = if *err {
            Style::new().fg(Color::LightRed)
        } else {
            Style::new().fg(Color::LightGreen)
        };
        f.render_widget(Line::from(Span::styled(format!(" {msg}"), style)), area);
        return;
    }
    let hints: &[(&str, &str)] = match app.tab {
        Tab::Running => &[
            ("↵", "actions"),
            ("o", "open"),
            ("s", "stop"),
            ("K", "kill"),
            ("h", "probe"),
            ("/", "filter"),
            ("a", "system"),
            ("u", "udp"),
            ("S", "sort"),
            ("?", "help"),
            ("q", "quit"),
        ],
        Tab::Launch => &[
            ("↵", "launch"),
            ("n", "custom command"),
            ("e", "edit templates"),
            ("tab", "switch"),
            ("?", "help"),
            ("q", "quit"),
        ],
        Tab::Managed => &[
            ("↵", "actions"),
            ("l", "logs"),
            ("R", "restart"),
            ("s", "stop"),
            ("o", "open"),
            ("d", "remove"),
            ("?", "help"),
            ("q", "quit"),
        ],
    };
    let mut spans = vec![Span::raw(" ")];
    for (k, v) in hints {
        spans.push(Span::styled(*k, Style::new().fg(ACCENT).bold()));
        spans.push(Span::styled(format!(" {v}  "), Style::new().fg(DIM)));
    }
    f.render_widget(Line::from(spans), area);
}

fn split_body(area: Rect, detail_rows: u16) -> (Rect, Rect) {
    if area.width >= 150 {
        let [a, b] = Layout::horizontal([Constraint::Percentage(64), Constraint::Percentage(36)])
            .areas(area);
        (a, b)
    } else {
        let [a, b] =
            Layout::vertical([Constraint::Min(5), Constraint::Length(detail_rows)]).areas(area);
        (a, b)
    }
}

fn header_row(cols: &[&str]) -> Row<'static> {
    Row::new(
        cols.iter()
            .map(|c| Cell::from(c.to_string()))
            .collect::<Vec<_>>(),
    )
    .style(Style::new().fg(DIM).bold())
}

fn draw_running(f: &mut Frame, app: &mut App, area: Rect) {
    let (table_area, detail_area) = split_body(area, 11);
    let servers = app.visible_servers();
    let rows: Vec<Row> = servers
        .iter()
        .map(|s| {
            let cc = cat_color(s.kind.category);
            let bind = match s.exposure {
                Exposure::Local => Span::styled("local", Style::new().fg(Color::Green)),
                Exposure::Public => Span::styled("network", Style::new().fg(Color::Yellow)),
                Exposure::Interface => Span::styled("iface", Style::new().fg(Color::LightBlue)),
            };
            let dim = s.kind.category == Category::System;
            let name_style = if dim {
                Style::new().fg(DIM)
            } else {
                Style::new()
            };
            Row::new(vec![
                Cell::from(Span::styled(
                    format!(":{}", s.port),
                    Style::new().bold().fg(if dim { DIM } else { Color::White }),
                )),
                Cell::from(Span::styled(
                    s.proto.label(),
                    Style::new().fg(if s.proto == Proto::Udp {
                        Color::Yellow
                    } else {
                        DIM
                    }),
                )),
                Cell::from(bind),
                Cell::from(Line::from(vec![
                    Span::styled("● ", Style::new().fg(cc)),
                    Span::styled(s.kind.label, Style::new().fg(cc)),
                ])),
                Cell::from(Span::styled(s.name.clone(), name_style)),
                Cell::from(Span::styled(s.pid.to_string(), Style::new().fg(DIM))),
                Cell::from(format!("{:.1}%", s.cpu)),
                Cell::from(human_bytes(s.memory)),
                Cell::from(Span::styled(human_duration(s.uptime), Style::new().fg(DIM))),
                Cell::from(match &s.managed {
                    Some(m) => Span::styled(format!("★ {m}"), Style::new().fg(ACCENT)),
                    None => Span::raw(""),
                }),
            ])
        })
        .collect();
    let empty = rows.is_empty();
    let n = servers.len();
    let mut title = format!("Listening servers ({n})");
    if !app.filter.is_empty() || app.filtering {
        title = format!(
            "{title} · filter: {}{}",
            app.filter,
            if app.filtering { "▏" } else { "" }
        );
    }
    if app.show_system {
        title.push_str(" · +system");
    }
    if app.show_udp {
        title.push_str(" · +udp");
    }
    title.push_str(&format!(" · sort: {}", app.sort.label()));
    let table = Table::new(
        rows,
        [
            Constraint::Length(7),
            Constraint::Length(4),
            Constraint::Length(8),
            Constraint::Length(24),
            Constraint::Min(14),
            Constraint::Length(7),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Length(7),
            Constraint::Min(16),
        ],
    )
    .header(header_row(&[
        "PORT", "", "BIND", "TYPE", "PROCESS", "PID", "CPU", "MEM", "UP", "SRVR",
    ]))
    .block(block(&title))
    .row_highlight_style(Style::new().bg(SEL_BG).add_modifier(Modifier::BOLD))
    .highlight_symbol("▌");
    drop(servers);
    f.render_stateful_widget(table, table_area, &mut app.server_state);

    if empty {
        let msg = if !app.loaded {
            "scanning…".to_string()
        } else if let Some(e) = &app.snap.error {
            format!("error: {e}")
        } else if !app.filter.is_empty() {
            "nothing matches the filter (Esc clears it)".into()
        } else {
            "nothing is listening right now — press 2 to launch something".into()
        };
        let inner = table_area.inner(ratatui::layout::Margin::new(2, 2));
        f.render_widget(
            Paragraph::new(msg).fg(DIM).alignment(Alignment::Center),
            inner,
        );
    }

    let detail = match app.selected_server() {
        None => Text::from(Span::styled("no server selected", Style::new().fg(DIM))),
        Some(s) => {
            let cc = cat_color(s.kind.category);
            let addrs = s
                .addrs
                .iter()
                .map(|a| a.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            let exposure = match s.exposure {
                Exposure::Local => "only this machine",
                Exposure::Public => "all interfaces — reachable from your network",
                Exposure::Interface => "a specific interface",
            };
            let mut lines = vec![
                Line::from(vec![
                    Span::styled(s.kind.label, Style::new().fg(cc).bold()),
                    Span::styled(
                        format!("  {}", s.kind.category.label()),
                        Style::new().fg(DIM),
                    ),
                    Span::styled(
                        format!("  ·  {}", s.url()),
                        Style::new().fg(Color::LightBlue).underlined(),
                    ),
                ]),
                kv(
                    "process",
                    format!(
                        "{} (pid {}){}",
                        s.name,
                        s.pid,
                        s.user
                            .as_ref()
                            .map(|u| format!(" · user {u}"))
                            .unwrap_or_default()
                    ),
                ),
                kv(
                    "listening",
                    format!("{}/{} on {addrs} ({exposure})", s.proto.label(), s.port),
                ),
                kv(
                    "command",
                    if s.cmd.is_empty() {
                        "(hidden — try sudo)".into()
                    } else {
                        s.cmd.clone()
                    },
                ),
                kv("exe", s.exe.clone()),
            ];
            if let Some(c) = &s.cwd {
                lines.push(kv("cwd", c.clone()));
            }
            lines.push(kv(
                "usage",
                format!(
                    "cpu {:.1}% · mem {} · up {}",
                    s.cpu,
                    human_bytes(s.memory),
                    human_duration(s.uptime)
                ),
            ));
            if let Some(m) = &s.managed {
                lines.push(kv("srvr", format!("managed as ★ {m} (l logs, R restart)")));
            }
            if let Some(p) = app.probes.get(&s.port) {
                lines.push(kv("http", p.clone()));
            }
            Text::from(lines)
        }
    };
    f.render_widget(
        Paragraph::new(detail)
            .block(block("Details"))
            .wrap(Wrap { trim: false }),
        detail_area,
    );
}

fn kv(k: &str, v: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{k:>10}  "), Style::new().fg(DIM)),
        Span::raw(v),
    ])
}

fn draw_launch(f: &mut Frame, app: &mut App, area: Rect) {
    let (table_area, detail_area) = split_body(area, 10);
    let mut rows = vec![Row::new(vec![
        Cell::from(Span::styled("+", Style::new().fg(ACCENT).bold())),
        Cell::from(Span::styled(
            "Custom command…",
            Style::new().fg(ACCENT).bold(),
        )),
        Cell::from(Span::styled("any", Style::new().fg(DIM))),
        Cell::from(""),
        Cell::from(Span::styled(
            "run anything as a managed server",
            Style::new().fg(DIM),
        )),
    ])];
    for (t, ok) in app.templates.iter().zip(&app.available) {
        let c = tpl_color(&t.category);
        rows.push(Row::new(vec![
            Cell::from(if *ok {
                Span::styled("✓", Style::new().fg(Color::Green))
            } else {
                Span::styled("✗", Style::new().fg(Color::Red))
            }),
            Cell::from(Span::styled(
                t.name.clone(),
                if *ok {
                    Style::new()
                } else {
                    Style::new().fg(DIM)
                },
            )),
            Cell::from(Span::styled(t.category.clone(), Style::new().fg(c))),
            Cell::from(Span::styled(
                t.port.map(|p| p.to_string()).unwrap_or_default(),
                Style::new().fg(DIM),
            )),
            Cell::from(Span::styled(
                if *ok {
                    String::new()
                } else {
                    format!("needs {}", t.program().replace('|', "/"))
                },
                Style::new().fg(Color::Red),
            )),
        ]));
    }
    let table = Table::new(
        rows,
        [
            Constraint::Length(1),
            Constraint::Min(28),
            Constraint::Length(9),
            Constraint::Length(6),
            Constraint::Min(16),
        ],
    )
    .header(header_row(&["", "TEMPLATE", "KIND", "PORT", ""]))
    .block(block("Launch a server"))
    .row_highlight_style(Style::new().bg(SEL_BG).add_modifier(Modifier::BOLD))
    .highlight_symbol("▌");
    f.render_stateful_widget(table, table_area, &mut app.launch_state);

    let text = match app.selected_template() {
        Some(0) | None => Text::from(vec![
            Line::from(Span::styled(
                "Custom command",
                Style::new().fg(ACCENT).bold(),
            )),
            Line::raw(""),
            Line::raw("Run any command (npm run dev, cargo run, ./server …) as a managed"),
            Line::raw("server: srvr keeps its logs, shows its ports, and can stop/restart it."),
            Line::raw("Use {port} in the command to have srvr substitute the chosen port."),
            Line::raw(""),
            Line::from(Span::styled(
                format!("Launch directory: {}", app.launch_dir.display()),
                Style::new().fg(DIM),
            )),
        ]),
        Some(i) => {
            let t = &app.templates[i - 1];
            let ok = app.available[i - 1];
            let preview = t
                .spec(&t.default_name(t.port), t.port, &app.launch_dir)
                .map(|s| shell_words::join(&s.argv))
                .unwrap_or_else(|e| format!("({e})"));
            let mut lines = vec![
                Line::from(vec![
                    Span::styled(
                        t.name.clone(),
                        Style::new().fg(tpl_color(&t.category)).bold(),
                    ),
                    Span::styled(
                        if t.user_defined {
                            "  (your template)"
                        } else {
                            ""
                        },
                        Style::new().fg(DIM),
                    ),
                ]),
                Line::raw(
                    t.description
                        .replace("{port}", &t.port.map(|p| p.to_string()).unwrap_or_default()),
                ),
                Line::raw(""),
                kv("runs", preview),
            ];
            if let Some(u) = &t.url {
                lines.push(kv(
                    "url",
                    u.replace("{port}", &t.port.map(|p| p.to_string()).unwrap_or_default()),
                ));
            }
            lines.push(kv(
                "requires",
                format!(
                    "{} {}",
                    t.program().replace('|', " or "),
                    if ok {
                        "✓ installed"
                    } else {
                        "✗ not found on PATH"
                    }
                ),
            ));
            if t.stop.is_some() {
                lines.push(kv("stop", t.stop.clone().unwrap_or_default()));
            }
            Text::from(lines)
        }
    };
    f.render_widget(
        Paragraph::new(text)
            .block(block("Template"))
            .wrap(Wrap { trim: false }),
        detail_area,
    );
}

fn draw_managed(f: &mut Frame, app: &mut App, area: Rect) {
    let (table_area, detail_area) = split_body(area, 9);
    let rows: Vec<Row> = app
        .snap
        .managed
        .iter()
        .map(|m| {
            let status = if m.alive {
                Span::styled("● running", Style::new().fg(Color::Green))
            } else {
                Span::styled("○ stopped", Style::new().fg(DIM))
            };
            let ports = m
                .ports
                .iter()
                .map(|p| format!(":{p}"))
                .collect::<Vec<_>>()
                .join(" ");
            let up = if m.alive {
                human_duration(crate::managed::now().saturating_sub(m.entry.started))
            } else {
                "-".into()
            };
            Row::new(vec![
                Cell::from(status),
                Cell::from(Span::styled(m.entry.name.clone(), Style::new().bold())),
                Cell::from(Span::styled(m.entry.template.clone(), Style::new().fg(DIM))),
                Cell::from(if ports.is_empty() && m.alive {
                    Span::styled(
                        m.entry
                            .port
                            .map(|p| format!("(:{p})"))
                            .unwrap_or("-".into()),
                        Style::new().fg(DIM),
                    )
                } else {
                    Span::raw(ports)
                }),
                Cell::from(m.entry.pid.to_string()),
                Cell::from(if m.alive {
                    format!("{:.1}%", m.cpu)
                } else {
                    "-".into()
                }),
                Cell::from(if m.alive {
                    human_bytes(m.memory)
                } else {
                    "-".into()
                }),
                Cell::from(up),
                Cell::from(Span::styled(
                    m.entry.url.clone().unwrap_or_default(),
                    Style::new().fg(Color::LightBlue),
                )),
            ])
        })
        .collect();
    let empty = rows.is_empty();
    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Min(18),
            Constraint::Length(14),
            Constraint::Length(12),
            Constraint::Length(7),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Length(7),
            Constraint::Min(20),
        ],
    )
    .header(header_row(&[
        "STATUS", "NAME", "TEMPLATE", "PORTS", "PID", "CPU", "MEM", "UP", "URL",
    ]))
    .block(block("Started by srvr"))
    .row_highlight_style(Style::new().bg(SEL_BG).add_modifier(Modifier::BOLD))
    .highlight_symbol("▌");
    f.render_stateful_widget(table, table_area, &mut app.managed_state);
    if empty {
        let inner = table_area.inner(ratatui::layout::Margin::new(2, 2));
        f.render_widget(
            Paragraph::new("nothing yet — press 2 and pick a template, or n for a custom command")
                .fg(DIM)
                .alignment(Alignment::Center),
            inner,
        );
    }
    let text = match app.selected_managed() {
        None => Text::from(Span::styled(
            "servers you launch from srvr show up here and keep running after you quit",
            Style::new().fg(DIM),
        )),
        Some(m) => Text::from(vec![
            kv("command", shell_words::join(&m.entry.argv)),
            kv("cwd", m.entry.cwd.display().to_string()),
            kv("log", m.entry.log.display().to_string()),
            kv("url", m.entry.url.clone().unwrap_or("-".into())),
            kv(
                "stop",
                m.entry
                    .stop_argv
                    .as_ref()
                    .map(shell_words::join)
                    .unwrap_or("signal process group".into()),
            ),
            Line::from(Span::styled(
                crate::managed::tail(&m.entry.log, 1)
                    .pop()
                    .map(|l| format!("{:>10}  {}", "last log", truncate(&l, 200)))
                    .unwrap_or_default(),
                Style::new().fg(DIM),
            )),
        ]),
    };
    f.render_widget(
        Paragraph::new(text)
            .block(block("Details"))
            .wrap(Wrap { trim: false }),
        detail_area,
    );
}

fn popup(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width.saturating_sub(2));
    let h = h.min(area.height.saturating_sub(2));
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}

fn draw_menu(f: &mut Frame, app: &mut App) {
    let Modal::Menu {
        target,
        items,
        state,
    } = &mut app.modal
    else {
        return;
    };
    let title = match target {
        Target::Server(s) => format!("{} · :{}", s.kind.label, s.port),
        Target::Managed(m) => m.entry.name.clone(),
    };
    let list_items: Vec<ListItem> = items
        .iter()
        .map(|a| {
            let danger = matches!(
                a,
                super::Action::Kill | super::Action::Stop | super::Action::Remove
            );
            ListItem::new(Line::from(vec![
                Span::styled(format!(" {} ", a.key()), Style::new().fg(ACCENT).bold()),
                Span::styled(
                    a.label(),
                    if danger {
                        Style::new().fg(Color::LightRed)
                    } else {
                        Style::new()
                    },
                ),
            ]))
        })
        .collect();
    let area = popup(f.area(), 44, items.len() as u16 + 2);
    f.render_widget(Clear, area);
    let list = List::new(list_items)
        .block(block(&title))
        .highlight_style(Style::new().bg(SEL_BG).bold())
        .highlight_symbol("▌");
    f.render_stateful_widget(list, area, state);
}

fn draw_confirm(f: &mut Frame, message: &str) {
    let area = popup(f.area(), 60, 6);
    f.render_widget(Clear, area);
    let text = Text::from(vec![
        Line::raw(message.to_string()),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                " y ",
                Style::new().fg(Color::Black).bg(Color::LightRed).bold(),
            ),
            Span::raw(" yes    "),
            Span::styled(" n ", Style::new().fg(Color::Black).bg(Color::Gray).bold()),
            Span::raw(" cancel"),
        ]),
    ]);
    f.render_widget(
        Paragraph::new(text)
            .block(block("Confirm"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_form(f: &mut Frame, app: &App) {
    let Modal::Form(form) = &app.modal else {
        return;
    };
    let h = form.fields.len() as u16 * 2 + 6 + u16::from(form.error.is_some()) * 2;
    let area = popup(f.area(), 78, h);
    f.render_widget(Clear, area);
    let mut lines = vec![Line::from(Span::styled(
        if form.tpl.description.is_empty() {
            "Run a command as a managed server".to_string()
        } else {
            truncate(&form.tpl.description, 74)
        },
        Style::new().fg(DIM),
    ))];
    lines.push(Line::raw(""));
    for (
        i,
        Field {
            label, value, hint, ..
        },
    ) in form.fields.iter().enumerate()
    {
        let focused = i == form.focus;
        lines.push(Line::from(vec![
            Span::styled(
                format!("{label:>10}  "),
                if focused {
                    Style::new().fg(ACCENT).bold()
                } else {
                    Style::new().fg(DIM)
                },
            ),
            Span::styled(
                value.clone(),
                if focused {
                    Style::new().bg(SEL_BG)
                } else {
                    Style::new()
                },
            ),
            Span::styled(if focused { "▏" } else { "" }, Style::new().fg(ACCENT)),
        ]));
        lines.push(Line::from(Span::styled(
            format!("{:>12}{hint}", ""),
            Style::new().fg(DIM).italic(),
        )));
    }
    if let Some(e) = &form.error {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            format!("  ✗ {e}"),
            Style::new().fg(Color::LightRed),
        )));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled(" ↵ ", Style::new().fg(Color::Black).bg(ACCENT).bold()),
        Span::raw(" launch   "),
        Span::styled("tab", Style::new().fg(ACCENT)),
        Span::raw(" next field   "),
        Span::styled("ctrl-u", Style::new().fg(ACCENT)),
        Span::raw(" clear   "),
        Span::styled("esc", Style::new().fg(ACCENT)),
        Span::raw(" cancel"),
    ]));
    f.render_widget(
        Paragraph::new(lines)
            .block(block(&format!("Launch · {}", form.tpl.name)))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_logs(f: &mut Frame, app: &App) {
    let Modal::Logs {
        name,
        path,
        offset,
        follow,
    } = &app.modal
    else {
        return;
    };
    let area = f.area().inner(ratatui::layout::Margin::new(2, 1));
    f.render_widget(Clear, area);
    let height = area.height.saturating_sub(2) as usize;
    let lines = crate::managed::tail(path, 5000);
    let max_off = lines.len().saturating_sub(height);
    let off = (*offset).min(max_off);
    let end = lines.len() - off;
    let start = end.saturating_sub(height);
    let text: Vec<Line> = lines[start..end]
        .iter()
        .map(|l| {
            let lower = l.to_ascii_lowercase();
            let style = if l.starts_with("--- srvr:") || l.starts_with("$ ") {
                Style::new().fg(ACCENT)
            } else if lower.contains("error") || lower.contains("fatal") || lower.contains("panic")
            {
                Style::new().fg(Color::LightRed)
            } else if lower.contains("warn") {
                Style::new().fg(Color::Yellow)
            } else {
                Style::new()
            };
            Line::from(Span::styled(l.clone(), style))
        })
        .collect();
    let title = format!(
        "logs · {name} · {}",
        if *follow { "following" } else { "paused" }
    );
    let hints = Line::from(Span::styled(
        format!(
            " ↑↓/PgUp/PgDn scroll · g top · f follow · o open file · esc close · {} ",
            path.display()
        ),
        Style::new().fg(DIM),
    ));
    f.render_widget(
        Paragraph::new(text).block(block(&title).title_bottom(hints)),
        area,
    );
}

fn draw_help(f: &mut Frame) {
    let area = popup(f.area(), 70, 30);
    f.render_widget(Clear, area);
    let sec = |s: &str| Line::from(Span::styled(s.to_string(), Style::new().fg(ACCENT).bold()));
    let k = |key: &str, desc: &str| {
        Line::from(vec![
            Span::styled(format!("  {key:<14}"), Style::new().fg(Color::White).bold()),
            Span::styled(desc.to_string(), Style::new().fg(Color::Gray)),
        ])
    };
    let text = vec![
        sec("Everywhere"),
        k("1 2 3 / tab", "switch tabs: Running · Launch · Managed"),
        k("↑↓ jk g G", "move · PgUp/PgDn jump"),
        k("r", "refresh now (auto every 2s)"),
        k("q  ctrl-c", "quit (servers you launched keep running)"),
        sec("Running & Managed"),
        k("enter", "action menu for the selected server"),
        k("o / c", "open in browser / copy URL"),
        k("h", "probe HTTP: status + Server header"),
        k("s / K", "stop gracefully / force kill"),
        k("R / l", "restart / logs (srvr-launched only)"),
        k("w y p", "open cwd · copy command · copy PID"),
        k("d", "remove a stopped entry (Managed)"),
        k("/", "filter by port, name, type, command"),
        k("a / u / S", "toggle system services / UDP · cycle sort"),
        sec("Launch"),
        k("enter", "configure port, folder & name, then start"),
        k("n", "run a custom command as a managed server"),
        k("e", "edit your own templates (templates.json)"),
        Line::raw(""),
        Line::from(Span::styled(
            "  ★ = started by srvr · BIND: local = this machine only, network = LAN",
            Style::new().fg(DIM),
        )),
        Line::from(Span::styled(
            "  press any key to close",
            Style::new().fg(DIM),
        )),
    ];
    f.render_widget(Paragraph::new(text).block(block("Help")), area);
}
