use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, Gauge, List, ListItem, Padding, Paragraph, Wrap,
};
use ratatui::Frame;

use crate::safety::format_bytes;

use super::app::{App, Screen};

const ACCENT: Color = Color::Cyan;
const SIZE: Color = Color::LightGreen;
const DANGER: Color = Color::Red;
const MUTED: Color = Color::DarkGray;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Fill(1),
        Constraint::Length(3),
    ])
    .split(area);

    draw_header(frame, chunks[0], app);

    match app.screen {
        Screen::Scanning => draw_scanning(frame, chunks[1], app),
        Screen::Deleting => draw_deleting(frame, chunks[1], app),
        Screen::Empty => draw_empty(frame, chunks[1]),
        _ => draw_main(frame, chunks[1], app),
    }

    draw_footer(frame, chunks[2], app);

    if app.help {
        draw_help(frame, area);
    }
    if app.confirm.is_some() {
        draw_confirm(frame, area, app);
    }
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let reclaim = format_bytes(app.result.total_size());
    let mut spans = vec![
        Span::styled(
            " Mac Cleaner ",
            Style::default()
                .fg(Color::Black)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(app.mode_label(), Style::default().fg(ACCENT)),
        Span::raw("   "),
        Span::styled(
            format!(
                "{} reclaimable · {} groups",
                reclaim,
                app.result.groups.len()
            ),
            Style::default().fg(SIZE).add_modifier(Modifier::BOLD),
        ),
    ];
    if let Some(disk) = &app.result.disk {
        spans.push(Span::raw("   "));
        spans.push(Span::styled(
            format!(
                "{} free of {} ({})",
                format_bytes(disk.container_free),
                format_bytes(disk.container_bytes),
                disk.mount
            ),
            Style::default().fg(MUTED),
        ));
    }
    let title = Line::from(spans);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT));
    frame.render_widget(Paragraph::new(title).block(block), area);
}

fn draw_scanning(frame: &mut Frame, area: Rect, app: &App) {
    let ratio = if app.scan_total == 0 {
        0.0
    } else {
        ((app.scan_index + 1) as f64 / app.scan_total as f64).clamp(0.0, 1.0)
    };
    draw_progress_screen(
        frame,
        area,
        " Scanning ",
        ratio,
        format!("{} / {}", app.scan_index + 1, app.scan_total),
        &app.scan_message,
        ACCENT,
    );
}

fn draw_deleting(frame: &mut Frame, area: Rect, app: &App) {
    let ratio = if app.delete_total == 0 {
        0.0
    } else {
        (app.delete_done as f64 / app.delete_total as f64).clamp(0.0, 1.0)
    };
    let pct = (ratio * 100.0).round() as u16;
    let spinner = spinner_frame(app.delete_started.elapsed().as_millis());
    draw_progress_screen(
        frame,
        area,
        &format!(" Deleting {spinner} "),
        ratio,
        format!(
            "{pct}%  ·  {} / {}",
            format_bytes(app.delete_done),
            format_bytes(app.delete_total)
        ),
        &app.delete_message,
        DANGER,
    );
}

fn draw_progress_screen(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    ratio: f64,
    label: String,
    message: &str,
    color: Color,
) {
    let layout = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(3),
        Constraint::Length(2),
        Constraint::Fill(1),
    ])
    .split(inset(area, 8, 0));

    let gauge = Gauge::default()
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(color)),
        )
        .gauge_style(Style::default().fg(color).bg(Color::Black))
        .ratio(ratio)
        .label(label);
    frame.render_widget(gauge, layout[1]);
    frame.render_widget(
        Paragraph::new(message)
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::White)),
        layout[2],
    );
}

fn spinner_frame(elapsed_ms: u128) -> &'static str {
    const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    FRAMES[(elapsed_ms / 80) as usize % FRAMES.len()]
}

fn draw_empty(frame: &mut Frame, area: Rect) {
    let msg = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            "Nothing to clean — your Mac looks tidy.",
            Style::default().fg(SIZE).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Press r to scan again, or q to quit.",
            Style::default().fg(MUTED),
        )),
    ])
    .alignment(Alignment::Center)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(ACCENT)),
    );
    frame.render_widget(msg, area);
}

fn draw_main(frame: &mut Frame, area: Rect, app: &mut App) {
    let show_detail = area.width >= 88;
    let body = if show_detail {
        Layout::horizontal([Constraint::Fill(3), Constraint::Fill(2)]).split(area)
    } else {
        Layout::horizontal([Constraint::Fill(1)]).split(area)
    };

    app.list_area = body[0];
    let items = list_items(app);
    let title = list_title(app);
    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(ACCENT))
                .padding(Padding::horizontal(1)),
        )
        .highlight_style(
            Style::default()
                .bg(ACCENT)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    frame.render_stateful_widget(list, body[0], &mut app.list_state);

    if show_detail {
        let (heading, body_text) = app.selected_detail();
        let detail = Paragraph::new(vec![
            Line::from(Span::styled(
                heading,
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(body_text),
            Line::from(""),
            Line::from(Span::styled(
                if app.status.is_empty() {
                    String::new()
                } else {
                    app.status.clone()
                },
                Style::default().fg(SIZE),
            )),
        ])
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .title(" Details ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(MUTED))
                .padding(Padding::horizontal(1)),
        );
        frame.render_widget(detail, body[1]);
    }
}

fn list_title(app: &App) -> String {
    match app.screen {
        Screen::Categories => " Categories ".into(),
        Screen::Groups { category } => format!(" {} ", category.label()),
        Screen::Files { ref group_key, .. } => app
            .result
            .group(group_key)
            .map(|g| format!(" {} ", g.title))
            .unwrap_or_else(|| " Files ".into()),
        _ => " Mac Cleaner ".into(),
    }
}

fn list_items(app: &App) -> Vec<ListItem<'static>> {
    match app.screen {
        Screen::Categories => {
            let cats = app.result.categories_sorted();
            let max = cats
                .iter()
                .map(|c| {
                    app.result
                        .groups_in(*c)
                        .iter()
                        .map(|g| g.size())
                        .sum::<u64>()
                })
                .max()
                .unwrap_or(1)
                .max(1);
            cats.into_iter()
                .map(|cat| {
                    let groups = app.result.groups_in(cat);
                    let size: u64 = groups.iter().map(|g| g.size()).sum();
                    let files: usize = groups.iter().map(|g| g.count()).sum();
                    let bar = spark(size, max, 10);
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{bar}  "), Style::default().fg(SIZE)),
                        Span::raw(format!("{:<24}  ", cat.label())),
                        Span::styled(
                            format!("{:>3} groups  {:>4} items  ", groups.len(), files),
                            Style::default().fg(MUTED),
                        ),
                        Span::styled(format_bytes(size), Style::default().fg(SIZE)),
                    ]))
                })
                .collect()
        }
        Screen::Groups { category } => app
            .result
            .groups_in(category)
            .into_iter()
            .map(|group| {
                ListItem::new(Line::from(vec![
                    Span::raw(format!("{:<42}  ", truncate(&group.title, 42))),
                    Span::styled(
                        format!("{:>4}  ", group.count()),
                        Style::default().fg(MUTED),
                    ),
                    Span::styled(format_bytes(group.size()), Style::default().fg(SIZE)),
                ]))
            })
            .collect(),
        Screen::Files { ref group_key, .. } => {
            let Some(group) = app.result.group(group_key) else {
                return Vec::new();
            };
            group
                .items
                .iter()
                .enumerate()
                .map(|(idx, item)| {
                    let mark = if app.marked.contains(&idx) {
                        Span::styled(
                            " × ",
                            Style::default().fg(DANGER).add_modifier(Modifier::BOLD),
                        )
                    } else {
                        Span::raw("   ")
                    };
                    let path = item.path.display().to_string();
                    ListItem::new(Line::from(vec![
                        mark,
                        Span::raw(format!("{}  ", truncate(&path, 56))),
                        Span::styled(format_bytes(item.size), Style::default().fg(SIZE)),
                    ]))
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let keys = match app.screen {
        Screen::Scanning => "  scanning…   ctrl+q abort".to_string(),
        Screen::Deleting => "  deleting… please wait".to_string(),
        Screen::Empty => "  r rescan   q quit   ? help".to_string(),
        Screen::Categories => "  ↑↓/jk move   ⏎ open   r rescan   ? help   q quit".to_string(),
        Screen::Groups { .. } => {
            "  ↑↓/jk move   ⏎ files   d delete group   a delete all   esc back   q quit".to_string()
        }
        Screen::Files { .. } => {
            let marked = if app.marked.is_empty() {
                String::new()
            } else {
                format!("   {} marked", app.marked.len())
            };
            format!(
                "  ↑↓/jk move   space mark   ⏎ delete{}   K keep this   d delete group   esc back",
                marked
            )
        }
    };
    let status = if app.status.is_empty() {
        keys
    } else {
        format!("  {}  ·{}", app.status, keys)
    };
    let para = Paragraph::new(status)
        .style(Style::default().fg(MUTED))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(MUTED)),
        );
    frame.render_widget(para, area);
}

fn draw_help(frame: &mut Frame, area: Rect) {
    let popup = centered(area, 70, 70);
    let text = vec![
        Line::from(Span::styled(
            "Keyboard",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  ↑ ↓   j k     move highlight"),
        Line::from("  g / G         first / last"),
        Line::from("  PgUp PgDn     jump"),
        Line::from("  Enter         open · delete marked/current files"),
        Line::from("  Space         mark / unmark a file"),
        Line::from("  c             clear marks"),
        Line::from("  d             delete group (or marked files)"),
        Line::from("  a             delete every group in this category"),
        Line::from("  K             keep highlighted file, delete the rest"),
        Line::from("  r             scan again"),
        Line::from("  Esc / b       back"),
        Line::from("  q             quit"),
        Line::from(""),
        Line::from("Click a row to highlight it. Nothing is deleted until you confirm."),
        Line::from(""),
        Line::from(Span::styled(
            "Press Esc to close",
            Style::default().fg(MUTED),
        )),
    ];
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(text).block(
            Block::default()
                .title(" Help ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(ACCENT))
                .padding(Padding::uniform(1)),
        ),
        popup,
    );
}

fn draw_confirm(frame: &mut Frame, area: Rect, app: &App) {
    let Some(confirm) = &app.confirm else {
        return;
    };
    let popup = centered(area, 64, 50);
    let yes_style = if confirm.yes {
        Style::default()
            .fg(Color::Black)
            .bg(DANGER)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    let no_style = if !confirm.yes {
        Style::default()
            .fg(Color::Black)
            .bg(ACCENT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };

    let mut lines = vec![
        Line::from(Span::styled(
            confirm.title.clone(),
            Style::default().fg(DANGER).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];
    for row in &confirm.body {
        lines.push(Line::from(Span::styled(
            row.clone(),
            Style::default().fg(Color::White),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::raw("    "),
        Span::styled("  Yes  ", yes_style),
        Span::raw("     "),
        Span::styled("  No  ", no_style),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "← → switch    y/n    Enter confirm    Esc cancel",
        Style::default().fg(MUTED),
    )));

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .title(" Confirm ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(DANGER))
                .padding(Padding::uniform(1)),
        ),
        popup,
    );
}

fn spark(size: u64, max: u64, width: usize) -> String {
    if width == 0 || max == 0 {
        return String::new();
    }
    let filled = ((size as f64 / max as f64) * width as f64).round() as usize;
    let filled = filled.min(width);
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return format!("{s:<max$}");
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn centered(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let popup = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup[1])[1]
}

fn inset(area: Rect, margin_x: u16, margin_y: u16) -> Rect {
    let h = Layout::horizontal([
        Constraint::Length(margin_x),
        Constraint::Fill(1),
        Constraint::Length(margin_x),
    ])
    .split(area);
    Layout::vertical([
        Constraint::Length(margin_y),
        Constraint::Fill(1),
        Constraint::Length(margin_y),
    ])
    .split(h[1])[1]
}
