use chrono::{Datelike, Days, Local, NaiveDate};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::symbols;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Axis, Bar, BarChart, BarGroup, Block, BorderType, Borders, Cell, Chart, Clear, Dataset, GraphType, Paragraph, Row,
    Table, TableState, Tabs, Wrap,
};

use crate::api::browser::Method;
use crate::api::models::{GradeKind, Justification, Lesson};
use crate::app::{App, Screen, Tab};
use crate::stats;

// ---- palette ---------------------------------------------------------------

const ACCENT: Color = Color::Rgb(122, 162, 247);
const ACCENT2: Color = Color::Rgb(187, 154, 247);
const MUTED: Color = Color::Rgb(110, 118, 155);
const SURFACE: Color = Color::Rgb(41, 46, 66);
const RED: Color = Color::Rgb(247, 118, 142);
const ORANGE: Color = Color::Rgb(255, 158, 100);
const YELLOW: Color = Color::Rgb(224, 175, 104);
const TEAL: Color = Color::Rgb(115, 218, 202);
const GREEN: Color = Color::Rgb(158, 206, 106);

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const DAYS: [&str; 7] = ["Hétfő", "Kedd", "Szerda", "Csütörtök", "Péntek", "Szombat", "Vasárnap"];
const DAYS_SHORT: [&str; 7] = ["H", "K", "Sze", "Cs", "P", "Szo", "V"];

fn grade_color(v: u8) -> Color {
    match v {
        5 => GREEN,
        4 => TEAL,
        3 => YELLOW,
        2 => ORANGE,
        1 => RED,
        _ => MUTED,
    }
}

fn avg_color(avg: Option<f64>) -> Color {
    avg.map_or(MUTED, |a| grade_color(stats::rounded(a)))
}

fn fmt_avg(avg: Option<f64>) -> String {
    avg.map_or("–".into(), |a| format!("{a:.2}"))
}

fn panel(title: &str) -> Block<'_> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(SURFACE).add_modifier(Modifier::BOLD))
        .title(Line::from(format!(" {title} ")).fg(ACCENT).bold())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

fn grade_badge(value: &str, v: u8) -> Span<'static> {
    Span::styled(format!(" {value} "), Style::new().fg(Color::Black).bg(grade_color(v)).bold())
}

fn relative_day(d: NaiveDate) -> String {
    let today = Local::now().date_naive();
    match (d - today).num_days() {
        0 => "ma".into(),
        1 => "holnap".into(),
        -1 => "tegnap".into(),
        n if n > 1 => format!("{n} nap múlva"),
        n => format!("{} napja", -n),
    }
}

fn key_hints(hints: &[(&str, &str)]) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for (k, d) in hints {
        spans.push(Span::styled(format!(" {k} "), Style::new().fg(Color::Black).bg(MUTED)));
        spans.push(Span::styled(format!(" {d}  "), Style::new().fg(MUTED)));
    }
    Line::from(spans)
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h)
}

// ---- entry -----------------------------------------------------------------

pub fn draw(f: &mut Frame, app: &mut App) {
    if let Screen::Login(_) = app.screen {
        draw_login(f, app);
        return;
    }
    let [header, tabs, body, footer] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(2), Constraint::Min(0), Constraint::Length(1)])
            .areas(f.area());
    draw_header(f, app, header);
    draw_tabs(f, app, tabs);
    match app.tab {
        Tab::Overview => draw_overview(f, app, body),
        Tab::Grades => draw_grades(f, app, body),
        Tab::Timetable => draw_timetable(f, app, body),
        Tab::Absences => draw_absences(f, app, body),
        Tab::Tasks => draw_tasks(f, app, body),
        Tab::Stats => draw_stats(f, app, body),
    }
    draw_footer(f, app, footer);
    if app.show_help {
        draw_help(f);
    }
    if app.confirm_logout {
        let area = centered(f.area(), 44, 5);
        f.render_widget(Clear, area);
        let text = vec![
            Line::raw(""),
            Line::from(vec![
                Span::raw("Biztosan kijelentkezel? "),
                Span::styled("i", Style::new().fg(GREEN).bold()),
                Span::raw(" / "),
                Span::styled("n", Style::new().fg(RED).bold()),
            ])
            .centered(),
        ];
        f.render_widget(Paragraph::new(text).block(panel("Kijelentkezés").border_style(Style::new().fg(RED))), area);
    }
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let s = &app.data.student;
    let mut left = vec![Span::styled(" ◆ KRÉTA ", Style::new().fg(Color::Black).bg(ACCENT).bold()), Span::raw(" ")];
    if !s.name.is_empty() {
        left.push(Span::styled(s.name.clone(), Style::new().bold()));
        left.push(Span::styled(format!("  {}", s.school), Style::new().fg(MUTED)));
    }
    if app.demo {
        left.push(Span::styled("  DEMÓ", Style::new().fg(ACCENT2).bold()));
    }
    f.render_widget(Paragraph::new(Line::from(left)), area);

    let mut right = Vec::new();
    if app.loading() {
        right.push(Span::styled(
            format!("{} frissítés… ", SPINNER[app.tick as usize % SPINNER.len()]),
            Style::new().fg(ACCENT),
        ));
    } else if let Some((msg, err, at)) = &app.status
        && at.elapsed().as_secs() < 6
    {
        right.push(Span::styled(format!("{msg} "), Style::new().fg(if *err { RED } else { GREEN })));
    } else if let Some(t) = app.raw.fetched_at {
        right.push(Span::styled(format!("frissítve {} ", t.format("%H:%M")), Style::new().fg(MUTED)));
    }
    let now = Local::now();
    right.push(Span::styled(
        format!("{} {} ", DAYS[now.weekday().num_days_from_monday() as usize], now.format("%m.%d. %H:%M")),
        Style::new().fg(ACCENT2),
    ));
    f.render_widget(Paragraph::new(Line::from(right)).alignment(Alignment::Right), area);
}

fn draw_tabs(f: &mut Frame, app: &App, area: Rect) {
    let titles = Tab::ALL.iter().enumerate().map(|(i, t)| Line::from(format!(" {} {} ", i + 1, t.title())));
    let tabs = Tabs::new(titles)
        .select(Tab::ALL.iter().position(|t| *t == app.tab))
        .style(Style::new().fg(MUTED))
        .highlight_style(Style::new().fg(Color::Black).bg(ACCENT).bold())
        .divider(" ")
        .padding(" ", "")
        .block(Block::new().borders(Borders::BOTTOM).border_style(Style::new().fg(SURFACE)));
    f.render_widget(tabs, area);
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let hints: &[(&str, &str)] = if app.simulating {
        &[("1-5", "jegy hozzáadása"), ("w", "súly"), ("⌫", "visszavon"), ("c", "törlés"), ("Esc", "kész")]
    } else {
        match app.tab {
            Tab::Grades => &[
                ("↑↓", "tantárgy"),
                ("→←", "jegyek"),
                ("s", "szimuláció"),
                ("r", "frissít"),
                ("?", "súgó"),
                ("q", "kilép"),
            ],
            Tab::Timetable => {
                &[("←→↑↓", "mozgás"), ("n/p", "következő/előző hét"), ("t", "ma"), ("r", "frissít"), ("?", "súgó")]
            }
            Tab::Absences | Tab::Tasks => {
                &[("↑↓", "görgetés"), ("Tab", "fül"), ("r", "frissít"), ("?", "súgó"), ("q", "kilép")]
            }
            _ => &[("Tab/1-6", "fül"), ("r", "frissít"), ("?", "súgó"), ("L", "kijelentkezés"), ("q", "kilép")],
        }
    };
    let mut line = key_hints(hints);
    if app.simulating {
        line.spans.push(Span::styled(format!("súly: {}%", app.sim_weight), Style::new().fg(ACCENT2).bold()));
    }
    f.render_widget(Paragraph::new(line), area);
}

// ---- overview --------------------------------------------------------------

fn stat_card(f: &mut Frame, area: Rect, title: &str, big: Line, sub: Line) {
    let p = Paragraph::new(vec![Line::raw(""), big.centered(), sub.centered()]).block(panel(title));
    f.render_widget(p, area);
}

fn draw_overview(f: &mut Frame, app: &App, area: Rect) {
    let d = &app.data;
    let [cards, middle, bottom] =
        Layout::vertical([Constraint::Length(5), Constraint::Min(8), Constraint::Length(10)]).areas(area);
    let c: [Rect; 4] = Layout::horizontal([Constraint::Ratio(1, 4); 4]).areas(cards);

    // Overall average + 30-day trend.
    let overall = stats::overall(&d.subjects);
    let timeline = stats::overall_timeline(&d.grades);
    let month_ago = Local::now() - chrono::Duration::days(30);
    let past = timeline.iter().rev().find(|(t, _)| *t <= month_ago).map(|p| p.1);
    let trend = match (overall, past) {
        (Some(now), Some(then)) if (now - then).abs() >= 0.005 => {
            let diff = now - then;
            Span::styled(
                format!("{} {:+.2} / 30 nap", if diff > 0.0 { "▲" } else { "▼" }, diff),
                Style::new().fg(if diff > 0.0 { GREEN } else { RED }),
            )
        }
        _ => Span::styled("stabil", Style::new().fg(MUTED)),
    };
    stat_card(
        f,
        c[0],
        "Összátlag",
        Line::styled(fmt_avg(overall), Style::new().fg(avg_color(overall)).bold()),
        Line::from(trend),
    );

    let counted = d.grades.iter().filter(|g| g.counts()).count();
    let week_ago = Local::now() - chrono::Duration::days(7);
    let recent = d.grades.iter().filter(|g| g.counts() && g.recorded >= week_ago).count();
    stat_card(
        f,
        c[1],
        "Jegyek",
        Line::styled(counted.to_string(), Style::new().fg(ACCENT).bold()),
        Line::styled(format!("+{recent} az elmúlt 7 napban"), Style::new().fg(MUTED)),
    );

    let missed = d.absences.iter().filter(|a| a.late_minutes.is_none()).count();
    let unjust = d.absences.iter().filter(|a| a.state == Justification::Unjustified).count();
    let pending = d.absences.iter().filter(|a| a.state == Justification::Pending).count();
    stat_card(
        f,
        c[2],
        "Hiányzás",
        Line::styled(format!("{missed} óra"), Style::new().fg(if missed > 0 { YELLOW } else { GREEN }).bold()),
        Line::from(vec![
            Span::styled(format!("{unjust} igazolatlan"), Style::new().fg(if unjust > 0 { RED } else { MUTED })),
            Span::styled(format!(" · {pending} igazolandó"), Style::new().fg(MUTED)),
        ]),
    );

    let today = Local::now().date_naive();
    let next_exam = d.exams.iter().find(|e| e.date.date_naive() >= today);
    match next_exam {
        Some(e) => stat_card(
            f,
            c[3],
            "Következő számonkérés",
            Line::styled(truncate(&e.subject, c[3].width as usize - 4), Style::new().fg(ACCENT2).bold()),
            Line::styled(
                format!("{} · {}", relative_day(e.date.date_naive()), truncate(&e.mode, 20)),
                Style::new().fg(if e.date.date_naive() <= today + Days::new(1) { RED } else { MUTED }),
            ),
        ),
        None => stat_card(
            f,
            c[3],
            "Következő számonkérés",
            Line::styled("nincs", Style::new().fg(GREEN).bold()),
            Line::raw(""),
        ),
    }

    let [lessons_area, grades_area] = Layout::horizontal([Constraint::Percentage(50); 2]).areas(middle);
    draw_day_lessons(f, app, lessons_area);
    draw_recent_grades(f, app, grades_area);

    let [exams_area, hw_area] = Layout::horizontal([Constraint::Percentage(50); 2]).areas(bottom);
    let exam_lines: Vec<Line> = d
        .exams
        .iter()
        .filter(|e| e.date.date_naive() >= today)
        .take(8)
        .map(|e| {
            let days = (e.date.date_naive() - today).num_days();
            Line::from(vec![
                Span::styled(
                    format!("{:>12} ", relative_day(e.date.date_naive())),
                    Style::new().fg(if days <= 1 {
                        RED
                    } else if days <= 3 {
                        YELLOW
                    } else {
                        MUTED
                    }),
                ),
                Span::styled(format!("{} ", e.subject), Style::new().bold()),
                Span::styled(format!("{} ", e.topic), Style::new().fg(MUTED)),
            ])
        })
        .collect();
    let exams_p = if exam_lines.is_empty() {
        vec![Line::styled(" Nincs bejelentett számonkérés 🎉", Style::new().fg(MUTED))]
    } else {
        exam_lines
    };
    f.render_widget(Paragraph::new(exams_p).block(panel("Közelgő számonkérések")), exams_area);

    let hw_lines: Vec<Line> = d
        .homework
        .iter()
        .filter(|h| h.deadline.date_naive() >= today)
        .take(8)
        .map(|h| {
            Line::from(vec![
                Span::styled(format!("{:>12} ", relative_day(h.deadline.date_naive())), Style::new().fg(YELLOW)),
                Span::styled(format!("{} ", h.subject), Style::new().bold()),
                Span::styled(h.text.lines().next().unwrap_or_default().to_owned(), Style::new().fg(MUTED)),
            ])
        })
        .collect();
    let hw_p = if hw_lines.is_empty() {
        vec![Line::styled(" Nincs határidős házi", Style::new().fg(MUTED))]
    } else {
        hw_lines
    };
    f.render_widget(Paragraph::new(hw_p).block(panel("Házi feladatok")), hw_area);
}

fn lesson_line(l: &Lesson, now: chrono::DateTime<Local>) -> Line<'static> {
    let current = l.start <= now && now < l.end;
    let past = l.end <= now;
    let mut subject_style = Style::new().bold();
    if l.cancelled {
        subject_style = subject_style.fg(RED).add_modifier(Modifier::CROSSED_OUT);
    } else if l.substitute.is_some() {
        subject_style = subject_style.fg(YELLOW);
    } else if past {
        subject_style = subject_style.fg(MUTED);
    }
    let mut spans = vec![
        Span::styled(if current { " ▶ " } else { "   " }, Style::new().fg(ACCENT).bold()),
        Span::styled(format!("{} ", l.start.format("%H:%M")), Style::new().fg(MUTED)),
        Span::styled(l.index.map_or("  ".into(), |i| format!("{i}.")), Style::new().fg(ACCENT2)),
        Span::raw(" "),
        Span::styled(l.subject.clone(), subject_style),
        Span::styled(format!("  {}", l.room), Style::new().fg(MUTED)),
    ];
    if let Some(s) = &l.substitute {
        spans.push(Span::styled(format!("  helyettes: {s}"), Style::new().fg(YELLOW)));
    }
    if l.has_exam {
        spans.push(Span::styled("  ● dolgozat", Style::new().fg(RED)));
    }
    if l.has_homework {
        spans.push(Span::styled("  ✎", Style::new().fg(TEAL)));
    }
    let line = Line::from(spans);
    if current { line.bg(SURFACE) } else { line }
}

fn draw_day_lessons(f: &mut Frame, app: &App, area: Rect) {
    let now = Local::now();
    let (title, lines) = match app.data.next_school_day() {
        Some((day, lessons)) => {
            let title = match (day - now.date_naive()).num_days() {
                0 => "Mai órák".to_owned(),
                1 => "Holnapi órák".to_owned(),
                _ => format!("{} órái", DAYS[day.weekday().num_days_from_monday() as usize]),
            };
            (title, lessons.into_iter().map(|l| lesson_line(l, now)).collect())
        }
        None => {
            ("Órák".to_owned(), vec![Line::styled(" Nincs betöltött óra a következő napokra", Style::new().fg(MUTED))])
        }
    };
    f.render_widget(Paragraph::new(lines).block(panel(&title)), area);
}

fn draw_recent_grades(f: &mut Frame, app: &App, area: Rect) {
    let max = area.height.saturating_sub(2) as usize;
    let lines: Vec<Line> = app
        .data
        .grades
        .iter()
        .filter(|g| g.value > 0 || !g.text.is_empty())
        .take(max)
        .map(|g| {
            let mut spans = vec![Span::raw(" "), grade_badge(&g.display_value(), g.value), Span::raw(" ")];
            if g.weight != 100.0 {
                spans.push(Span::styled(format!("{}% ", g.weight), Style::new().fg(ACCENT2)));
            }
            spans.push(Span::styled(format!("{} ", g.subject), Style::new().bold()));
            spans.push(Span::styled(
                format!("{} · {}", g.date.format("%m.%d."), if g.topic.is_empty() { &g.mode } else { &g.topic }),
                Style::new().fg(MUTED),
            ));
            Line::from(spans)
        })
        .collect();
    f.render_widget(Paragraph::new(lines).block(panel("Legutóbbi jegyek")), area);
}

// ---- grades ----------------------------------------------------------------

fn draw_grades(f: &mut Frame, app: &App, area: Rect) {
    let d = &app.data;
    if d.subjects.is_empty() {
        f.render_widget(Paragraph::new(" Még nincs jegy.").block(panel("Jegyek")), area);
        return;
    }
    let [list_area, detail] = Layout::horizontal([Constraint::Length(36), Constraint::Min(0)]).areas(area);

    let rows = d.subjects.iter().map(|s| {
        let sim = app.sim.get(&s.name).filter(|v| !v.is_empty());
        let avg = match sim {
            Some(extra) => stats::weighted_avg(
                s.grades
                    .iter()
                    .map(|&i| &d.grades[i])
                    .filter(|g| g.counts())
                    .map(|g| (g.value as f64, g.weight))
                    .chain(extra.iter().map(|&(v, w)| (v as f64, w))),
            ),
            None => s.avg,
        };
        Row::new(vec![
            Cell::from(truncate(&s.name, 26)),
            Cell::from(
                Line::from(vec![
                    Span::styled(fmt_avg(avg), Style::new().fg(avg_color(avg)).bold()),
                    Span::styled(if sim.is_some() { "*" } else { " " }, Style::new().fg(ACCENT2)),
                ])
                .right_aligned(),
            ),
        ])
    });
    let table = Table::new(rows, [Constraint::Min(10), Constraint::Length(6)])
        .block(panel("Tantárgyak"))
        .row_highlight_style(
            Style::new().bg(if app.grades_focus_right { SURFACE } else { ACCENT }).fg(if app.grades_focus_right {
                Color::Reset
            } else {
                Color::Black
            }),
        )
        .highlight_symbol("▌");
    let mut state = TableState::default().with_selected(Some(app.subject_idx));
    f.render_stateful_widget(table, list_area, &mut state);

    let Some(s) = app.selected_subject() else { return };
    let [summary, chart_area, list] =
        Layout::vertical([Constraint::Length(7), Constraint::Length(10), Constraint::Min(4)]).areas(detail);

    // Summary + what-if.
    let sim = app.sim.get(&s.name).cloned().unwrap_or_default();
    let (sum, wsum) = sim.iter().fold((s.sum, s.weight_sum), |(a, b), &(v, w)| (a + v as f64 * w, b + w));
    let avg = (wsum > 0.0).then(|| sum / wsum);
    let mut lines = vec![Line::from(vec![
        Span::raw(" Átlag: "),
        Span::styled(fmt_avg(s.avg), Style::new().fg(avg_color(s.avg)).bold()),
        Span::styled(s.avg.map_or(String::new(), |a| format!(" (→ {})", stats::rounded(a))), Style::new().fg(MUTED)),
        Span::raw("   Félévi: "),
        s.half_year.map_or(Span::styled("–", Style::new().fg(MUTED)), |v| grade_badge(&v.to_string(), v)),
        Span::raw("   Év végi: "),
        s.end_year.map_or(Span::styled("–", Style::new().fg(MUTED)), |v| grade_badge(&v.to_string(), v)),
        Span::styled(format!("   {} jegy", s.grades.len()), Style::new().fg(MUTED)),
    ])];
    if !sim.is_empty() || app.simulating {
        let mut spans = vec![Span::styled(" Szimuláció: ", Style::new().fg(ACCENT2).bold())];
        for &(v, w) in &sim {
            spans.push(grade_badge(&if w == 100.0 { v.to_string() } else { format!("{v}·{w}%") }, v));
            spans.push(Span::raw(" "));
        }
        if sim.is_empty() {
            spans.push(Span::styled("nyomj 1–5-öt egy jegy hozzáadásához", Style::new().fg(MUTED)));
        } else {
            spans.push(Span::raw("→ "));
            spans.push(Span::styled(fmt_avg(avg), Style::new().fg(avg_color(avg)).bold()));
            if let (Some(new), Some(old)) = (avg, s.avg) {
                let diff = new - old;
                spans.push(Span::styled(
                    format!(" ({diff:+.2})"),
                    Style::new().fg(if diff >= 0.0 { GREEN } else { RED }),
                ));
            }
        }
        lines.push(Line::from(spans));
    }
    if let Some(a) = avg {
        let r = stats::rounded(a);
        if r < 5 {
            let target = r as f64 + 0.5;
            let n1 = stats::needed(sum, wsum, 5.0, 100.0, target);
            let n2 = stats::needed(sum, wsum, 5.0, 200.0, target);
            if let (Some(n1), Some(n2)) = (n1, n2) {
                lines.push(Line::from(vec![
                    Span::raw(" "),
                    Span::styled(format!("{}-eshez ({target:.2}): ", r + 1), Style::new().fg(MUTED)),
                    Span::styled(format!("{n1}× ötös"), Style::new().fg(GREEN).bold()),
                    Span::styled(format!("  vagy  {n2}× ötös dupla súllyal"), Style::new().fg(MUTED)),
                ]));
            }
        }
        if r > 1 {
            let floor = r as f64 - 0.5;
            if let Some(n) = stats::buffer_ones(sum, wsum, 100.0, floor) {
                lines.push(Line::from(vec![
                    Span::raw(" "),
                    Span::styled("Tartalék: ", Style::new().fg(MUTED)),
                    Span::styled(format!("{n}× egyes"), Style::new().fg(if n == 0 { RED } else { YELLOW }).bold()),
                    Span::styled(format!(" fér bele, mielőtt {}-esre romlik", r - 1), Style::new().fg(MUTED)),
                ]));
            }
        }
    }
    let title = if app.simulating { format!("{} · szimuláció", s.name) } else { s.name.clone() };
    let block = panel(&title).border_style(Style::new().fg(if app.simulating { ACCENT2 } else { SURFACE }));
    f.render_widget(Paragraph::new(lines).block(block), summary);

    // Running average chart (with simulated continuation).
    let tl = stats::subject_timeline(&d.grades, s);
    let pts: Vec<(f64, f64)> = tl.iter().enumerate().map(|(i, &v)| (i as f64, v)).collect();
    let mut sim_pts: Vec<(f64, f64)> = Vec::new();
    if !sim.is_empty() {
        let (mut sm, mut ws) = (s.sum, s.weight_sum);
        if let Some(last) = pts.last() {
            sim_pts.push(*last);
        }
        for (i, &(v, w)) in sim.iter().enumerate() {
            sm += v as f64 * w;
            ws += w;
            sim_pts.push(((pts.len() + i) as f64, sm / ws));
        }
    }
    let n = (pts.len() + sim.len()).max(2) as f64;
    let lo = pts.iter().chain(&sim_pts).map(|p| p.1).fold(5.0, f64::min);
    let lo = (((lo - 0.25) * 2.0).floor() / 2.0).clamp(1.0, 4.5);
    let chart = Chart::new(vec![
        Dataset::default()
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::new().fg(ACCENT))
            .data(&pts),
        Dataset::default()
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::new().fg(ACCENT2))
            .data(&sim_pts),
    ])
    .block(panel("Átlag alakulása"))
    .x_axis(Axis::default().bounds([0.0, n - 1.0]).style(Style::new().fg(MUTED)))
    .y_axis(
        Axis::default()
            .bounds([lo, 5.0])
            .labels([format!("{lo:.1}"), format!("{:.1}", (lo + 5.0) / 2.0), "5.0".into()])
            .style(Style::new().fg(MUTED)),
    );
    f.render_widget(chart, chart_area);

    // Grade list.
    let rows = s.grades.iter().map(|&i| {
        let g = &d.grades[i];
        let kind = match &g.kind {
            GradeKind::MidYear => "",
            GradeKind::HalfYear => "félévi",
            GradeKind::EndYear => "év végi",
            GradeKind::Other(o) => o.as_str(),
        };
        Row::new(vec![
            Cell::from(g.date.format("%Y.%m.%d.").to_string()).fg(MUTED),
            Cell::from(Line::from(grade_badge(&truncate(&g.display_value(), 12), g.value))),
            Cell::from(format!("{}%", g.weight)).fg(if g.weight > 100.0 { ACCENT2 } else { MUTED }),
            Cell::from(if kind.is_empty() { g.mode.clone() } else { kind.to_owned() }),
            Cell::from(g.topic.clone()),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(11),
            Constraint::Length(14),
            Constraint::Length(5),
            Constraint::Length(22),
            Constraint::Min(10),
        ],
    )
    .header(Row::new(["Dátum", "Jegy", "Súly", "Mód", "Téma"]).fg(ACCENT).bold())
    .block(panel("Jegyek").border_style(Style::new().fg(if app.grades_focus_right { ACCENT } else { SURFACE })))
    .row_highlight_style(Style::new().bg(SURFACE))
    .highlight_symbol(if app.grades_focus_right { "▌" } else { " " });
    let mut state = TableState::default().with_selected(app.grades_focus_right.then_some(app.grade_idx));
    f.render_stateful_widget(table, list, &mut state);
}

// ---- timetable -------------------------------------------------------------

fn draw_timetable(f: &mut Frame, app: &mut App, area: Rect) {
    let week = app.week;
    let days = app.week_days();
    let today = Local::now().date_naive();
    let now = Local::now();
    let [grid_area, detail_area] = Layout::vertical([Constraint::Min(6), Constraint::Length(7)]).areas(area);
    let title = format!(
        "{} – {}{}",
        week.format("%Y.%m.%d."),
        (week + Days::new(days as u64 - 1)).format("%m.%d."),
        if week == crate::app::monday_of(today) { " · ez a hét" } else { "" }
    );

    let Some(lessons) = app.data.weeks.get(&week) else {
        let msg = if app.loading() { " Betöltés…" } else { " Nincs adat erre a hétre (r: frissítés)" };
        f.render_widget(Paragraph::new(msg).block(panel(&title)), grid_area);
        return;
    };

    // Rows are lesson indices that appear anywhere this week.
    let mut indices: Vec<u32> = lessons.iter().map(|l| l.index.unwrap_or(99)).collect();
    indices.sort_unstable();
    indices.dedup();
    if indices.is_empty() {
        f.render_widget(Paragraph::new(" Ezen a héten nincs óra 🎉").block(panel(&title)), grid_area);
        return;
    }
    app.lesson_row = app.lesson_row.min(indices.len() - 1);
    let cell_at = |day: usize, idx: u32| -> Vec<&Lesson> {
        lessons
            .iter()
            .filter(|l| l.start.weekday().num_days_from_monday() as usize == day && l.index.unwrap_or(99) == idx)
            .collect()
    };

    let mut header = vec![Cell::from("")];
    for (d, short) in DAYS_SHORT.iter().enumerate().take(days) {
        let date = week + Days::new(d as u64);
        let style = if date == today {
            Style::new().fg(Color::Black).bg(ACCENT).bold()
        } else {
            Style::new().fg(ACCENT).bold()
        };
        header.push(Cell::from(format!(" {} {}", short, date.format("%m.%d."))).style(style));
    }

    let rows: Vec<Row> = indices
        .iter()
        .enumerate()
        .map(|(r, &idx)| {
            let mut cells = vec![Cell::from(Text::from(vec![Line::styled(
                if idx == 99 { "–".into() } else { format!("{idx}.") },
                Style::new().fg(ACCENT2).bold(),
            )]))];
            for d in 0..days {
                let selected = r == app.lesson_row && d == app.day_idx;
                let ls = cell_at(d, idx);
                let text = match ls.first() {
                    None => Text::raw(""),
                    Some(l) => {
                        let current = l.start <= now && now < l.end;
                        let mut st = Style::new().bold();
                        if l.cancelled {
                            st = st.fg(RED).add_modifier(Modifier::CROSSED_OUT);
                        } else if l.substitute.is_some() {
                            st = st.fg(YELLOW);
                        } else if current {
                            st = st.fg(GREEN);
                        }
                        let mut marks = String::new();
                        if l.has_exam {
                            marks.push_str(" ●");
                        }
                        if l.has_homework {
                            marks.push_str(" ✎");
                        }
                        Text::from(vec![
                            Line::from(vec![
                                Span::styled(format!(" {}", l.subject), st),
                                Span::styled(marks, Style::new().fg(RED)),
                            ]),
                            Line::styled(format!(" {} {}", l.start.format("%H:%M"), l.room), Style::new().fg(MUTED)),
                        ])
                    }
                };
                let mut cell = Cell::from(text);
                if selected {
                    cell = cell.style(Style::new().bg(SURFACE));
                }
                cells.push(cell);
            }
            Row::new(cells).height(2)
        })
        .collect();

    let mut widths = vec![Constraint::Length(4)];
    widths.extend(std::iter::repeat_n(Constraint::Ratio(1, days as u32), days));
    let table = Table::new(rows, widths).header(Row::new(header).height(1)).block(panel(&title)).column_spacing(1);
    f.render_widget(table, grid_area);

    // Details of the selected slot.
    let sel = cell_at(app.day_idx, indices[app.lesson_row]);
    let lines: Vec<Line> = match sel.first() {
        None => vec![Line::styled(" Üres óra", Style::new().fg(MUTED))],
        Some(l) => {
            let mut v = vec![
                Line::from(vec![
                    Span::styled(format!(" {}", l.subject), Style::new().bold().fg(ACCENT)),
                    Span::styled(
                        format!("   {}–{}   {}", l.start.format("%H:%M"), l.end.format("%H:%M"), l.room),
                        Style::new().fg(MUTED),
                    ),
                ]),
                Line::from(vec![Span::styled(" Tanár: ", Style::new().fg(MUTED)), Span::raw(l.teacher.clone())]),
            ];
            if let Some(s) = &l.substitute {
                v.push(Line::styled(format!(" Helyettesít: {s}"), Style::new().fg(YELLOW)));
            }
            if l.cancelled {
                v.push(Line::styled(" Elmarad", Style::new().fg(RED).bold()));
            }
            if !l.topic.is_empty() {
                v.push(Line::from(vec![Span::styled(" Téma: ", Style::new().fg(MUTED)), Span::raw(l.topic.clone())]));
            }
            if l.absent {
                v.push(Line::styled(" Hiányoztál erről az óráról", Style::new().fg(ORANGE)));
            }
            v
        }
    };
    let day_name = DAYS[app.day_idx.min(6)];
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(panel(day_name)), detail_area);
}

// ---- absences --------------------------------------------------------------

fn draw_absences(f: &mut Frame, app: &App, area: Rect) {
    let d = &app.data;
    let [table_area, side] = Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)]).areas(area);

    let rows = d.absences.iter().map(|a| {
        let (state, color) = match a.state {
            Justification::Justified => ("igazolt", GREEN),
            Justification::Pending => ("igazolandó", YELLOW),
            Justification::Unjustified => ("igazolatlan", RED),
        };
        let kind = match a.late_minutes {
            Some(m) => format!("késés {m}p"),
            None => "hiányzás".into(),
        };
        Row::new(vec![
            Cell::from(a.date.format("%m.%d.").to_string()).fg(MUTED),
            Cell::from(a.lesson.map_or("–".into(), |l| format!("{l}."))).fg(ACCENT2),
            Cell::from(a.subject.clone()),
            Cell::from(kind),
            Cell::from(state).fg(color),
            Cell::from(a.justification_type.clone()).fg(MUTED),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(6),
            Constraint::Length(3),
            Constraint::Min(12),
            Constraint::Length(10),
            Constraint::Length(11),
            Constraint::Length(16),
        ],
    )
    .header(Row::new(["Dátum", "Óra", "Tantárgy", "Típus", "Állapot", "Igazolás"]).fg(ACCENT).bold())
    .block(panel("Mulasztások"))
    .row_highlight_style(Style::new().bg(SURFACE))
    .highlight_symbol("▌");
    let mut state = TableState::default().with_selected((!d.absences.is_empty()).then_some(app.absence_idx));
    f.render_stateful_widget(table, table_area, &mut state);

    let [summary_area, chart_area] = Layout::vertical([Constraint::Length(8), Constraint::Min(5)]).areas(side);
    let count = |s: Justification| d.absences.iter().filter(|a| a.state == s && a.late_minutes.is_none()).count();
    let lates: Vec<u32> = d.absences.iter().filter_map(|a| a.late_minutes).collect();
    let days: std::collections::BTreeSet<NaiveDate> =
        d.absences.iter().filter(|a| a.late_minutes.is_none()).map(|a| a.date.date_naive()).collect();
    let summary = vec![
        Line::from(vec![
            Span::styled(" Igazolt:      ", Style::new().fg(MUTED)),
            Span::styled(count(Justification::Justified).to_string(), Style::new().fg(GREEN).bold()),
        ]),
        Line::from(vec![
            Span::styled(" Igazolandó:   ", Style::new().fg(MUTED)),
            Span::styled(count(Justification::Pending).to_string(), Style::new().fg(YELLOW).bold()),
        ]),
        Line::from(vec![
            Span::styled(" Igazolatlan:  ", Style::new().fg(MUTED)),
            Span::styled(count(Justification::Unjustified).to_string(), Style::new().fg(RED).bold()),
        ]),
        Line::from(vec![
            Span::styled(" Érintett nap: ", Style::new().fg(MUTED)),
            Span::styled(days.len().to_string(), Style::new().bold()),
        ]),
        Line::from(vec![
            Span::styled(" Késések:      ", Style::new().fg(MUTED)),
            Span::styled(format!("{} db, {} perc", lates.len(), lates.iter().sum::<u32>()), Style::new().bold()),
        ]),
    ];
    f.render_widget(Paragraph::new(summary).block(panel("Összesítés")), summary_area);

    let mut per_subject: std::collections::BTreeMap<&str, u64> = std::collections::BTreeMap::new();
    for a in d.absences.iter().filter(|a| a.late_minutes.is_none()) {
        *per_subject.entry(&a.subject).or_default() += 1;
    }
    let mut ranked: Vec<(&str, u64)> = per_subject.into_iter().collect();
    ranked.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    let max_bars = (chart_area.height.saturating_sub(2) as usize).div_ceil(2);
    let bars: Vec<Bar> = ranked
        .iter()
        .take(max_bars)
        .map(|(s, n)| {
            Bar::default()
                .label(Line::from(truncate(s, 14)))
                .value(*n)
                .style(Style::new().fg(ORANGE))
                .value_style(Style::new().fg(Color::Black).bg(ORANGE))
        })
        .collect();
    let chart = BarChart::default()
        .direction(Direction::Horizontal)
        .data(BarGroup::default().bars(&bars))
        .bar_width(1)
        .bar_gap(1)
        .block(panel("Tantárgyanként (óra)"));
    f.render_widget(chart, chart_area);
}

// ---- tasks -----------------------------------------------------------------

fn draw_tasks(f: &mut Frame, app: &App, area: Rect) {
    let d = &app.data;
    let today = Local::now().date_naive();
    let mut lines: Vec<Line> = Vec::new();
    let section = |lines: &mut Vec<Line>, title: &str| {
        if !lines.is_empty() {
            lines.push(Line::raw(""));
        }
        lines
            .push(Line::styled(format!(" {title}"), Style::new().fg(ACCENT).bold().add_modifier(Modifier::UNDERLINED)));
    };

    section(&mut lines, "Bejelentett számonkérések");
    let upcoming: Vec<_> = d.exams.iter().filter(|e| e.date.date_naive() >= today).collect();
    if upcoming.is_empty() {
        lines.push(Line::styled("   nincs", Style::new().fg(MUTED)));
    }
    for e in upcoming {
        let days = (e.date.date_naive() - today).num_days();
        lines.push(Line::from(vec![
            Span::styled(
                format!("   {:>11} ", relative_day(e.date.date_naive())),
                Style::new()
                    .fg(if days <= 1 {
                        RED
                    } else if days <= 3 {
                        YELLOW
                    } else {
                        MUTED
                    })
                    .bold(),
            ),
            Span::styled(format!("{} ", e.date.format("%m.%d.")), Style::new().fg(MUTED)),
            Span::styled(e.lesson.map_or(String::new(), |l| format!("{l}. óra ")), Style::new().fg(ACCENT2)),
            Span::styled(format!("{} ", e.subject), Style::new().bold()),
            Span::styled(format!("– {}", e.mode), Style::new().fg(MUTED)),
            Span::styled(
                if e.teacher.is_empty() { String::new() } else { format!(" ({})", e.teacher) },
                Style::new().fg(MUTED),
            ),
        ]));
        if !e.topic.is_empty() {
            lines.push(Line::styled(format!("               {}", e.topic), Style::new().fg(Color::Reset)));
        }
    }

    section(&mut lines, "Házi feladatok");
    if d.homework.is_empty() {
        lines.push(Line::styled("   nincs", Style::new().fg(MUTED)));
    }
    let (upcoming_hw, past_hw): (Vec<_>, Vec<_>) = d.homework.iter().partition(|h| h.deadline.date_naive() >= today);
    for h in upcoming_hw.into_iter().chain(past_hw.into_iter().rev()) {
        let overdue = h.deadline.date_naive() < today;
        lines.push(Line::from(vec![
            Span::styled(
                format!("   {:>11} ", relative_day(h.deadline.date_naive())),
                Style::new().fg(if overdue { MUTED } else { YELLOW }).bold(),
            ),
            Span::styled(format!("{} ", h.subject), Style::new().bold()),
            Span::styled(format!("({})", h.teacher), Style::new().fg(MUTED)),
        ]));
        for l in h.text.lines().filter(|l| !l.trim().is_empty()) {
            lines.push(Line::styled(
                format!("               {}", l.trim()),
                Style::new().fg(if overdue { MUTED } else { Color::Reset }),
            ));
        }
    }

    section(&mut lines, "Feljegyzések");
    if d.notes.is_empty() {
        lines.push(Line::styled("   nincs", Style::new().fg(MUTED)));
    }
    for n in &d.notes {
        lines.push(Line::from(vec![
            Span::styled(format!("   {} ", n.date.format("%Y.%m.%d.")), Style::new().fg(MUTED)),
            Span::styled(format!("{} ", n.title), Style::new().bold()),
            Span::styled(format!("({})", n.teacher), Style::new().fg(MUTED)),
        ]));
        for l in n.content.lines().filter(|l| !l.trim().is_empty()) {
            lines.push(Line::raw(format!("               {}", l.trim())));
        }
    }

    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).scroll((app.tasks_scroll, 0)).block(panel("Teendők")),
        area,
    );
}

// ---- stats -----------------------------------------------------------------

fn draw_stats(f: &mut Frame, app: &App, area: Rect) {
    let d = &app.data;
    let [top, bottom] = Layout::vertical([Constraint::Percentage(50); 2]).areas(area);
    let [line_area, dist_area] =
        Layout::horizontal([Constraint::Percentage(65), Constraint::Percentage(35)]).areas(top);
    let [subj_area, facts_area] =
        Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)]).areas(bottom);

    // Overall average over time.
    let tl = stats::overall_timeline(&d.grades);
    if let (Some(first), Some(last)) = (tl.first(), tl.last()) {
        let t0 = first.0;
        let pts: Vec<(f64, f64)> = tl.iter().map(|(t, v)| ((*t - t0).num_hours() as f64 / 24.0, *v)).collect();
        let span = pts.last().map_or(1.0, |p| p.0.max(1.0));
        let lo = tl.iter().map(|p| p.1).fold(5.0, f64::min);
        let lo = ((lo - 0.25) * 2.0).floor() / 2.0;
        let lo = lo.clamp(1.0, 4.5);
        let mid = t0 + chrono::Duration::hours((span * 12.0) as i64);
        let chart = Chart::new(vec![
            Dataset::default()
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::new().fg(ACCENT))
                .data(&pts),
        ])
        .block(panel("Összátlag alakulása"))
        .x_axis(Axis::default().bounds([0.0, span]).style(Style::new().fg(MUTED)).labels([
            t0.format("%m.%d.").to_string(),
            mid.format("%m.%d.").to_string(),
            last.0.format("%m.%d.").to_string(),
        ]))
        .y_axis(Axis::default().bounds([lo, 5.0]).style(Style::new().fg(MUTED)).labels([
            format!("{lo:.1}"),
            format!("{:.1}", (lo + 5.0) / 2.0),
            "5.0".into(),
        ]));
        f.render_widget(chart, line_area);
    } else {
        f.render_widget(Paragraph::new(" Nincs elég adat").block(panel("Összátlag alakulása")), line_area);
    }

    // Grade distribution.
    let dist = stats::distribution(&d.grades);
    let inner_w = dist_area.width.saturating_sub(2);
    let bar_w = ((inner_w.saturating_sub(4)) / 5).clamp(1, 9);
    let bars: Vec<Bar> = dist
        .iter()
        .enumerate()
        .map(|(i, &n)| {
            let c = grade_color(i as u8 + 1);
            Bar::default()
                .label(Line::from(format!("{}", i + 1)).centered())
                .value(n as u64)
                .style(Style::new().fg(c))
                .value_style(Style::new().fg(Color::Black).bg(c).bold())
        })
        .collect();
    f.render_widget(
        BarChart::default()
            .data(BarGroup::default().bars(&bars))
            .bar_width(bar_w)
            .bar_gap(1)
            .block(panel("Jegyeloszlás")),
        dist_area,
    );

    // Subject averages, best first.
    let mut ranked: Vec<_> = d.subjects.iter().filter_map(|s| s.avg.map(|a| (s, a))).collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    let max_bars = (subj_area.height.saturating_sub(2) as usize).div_ceil(2);
    let bars: Vec<Bar> = ranked
        .iter()
        .take(max_bars)
        .map(|(s, a)| {
            let c = avg_color(Some(*a));
            Bar::default()
                .label(Line::from(truncate(&s.name, 16)))
                .value(((a - 1.0) * 100.0) as u64)
                .text_value(format!("{a:.2}"))
                .style(Style::new().fg(c))
                .value_style(Style::new().fg(Color::Black).bg(c).bold())
        })
        .collect();
    f.render_widget(
        BarChart::default()
            .direction(Direction::Horizontal)
            .data(BarGroup::default().bars(&bars))
            .max(400)
            .bar_width(1)
            .bar_gap(1)
            .block(panel("Tantárgyi átlagok")),
        subj_area,
    );

    // Fun facts.
    let counted: Vec<_> = d.grades.iter().filter(|g| g.counts()).collect();
    let total = counted.len().max(1) as f64;
    let mut facts: Vec<Line> = Vec::new();
    let fact = |label: &str, value: Span<'static>| {
        Line::from(vec![Span::styled(format!(" {label:<25}"), Style::new().fg(MUTED)), value])
    };
    if let Some((s, a)) = ranked.first() {
        facts.push(fact("Legerősebb:", Span::styled(format!("{} ({a:.2})", s.name), Style::new().fg(GREEN).bold())));
    }
    if let Some((s, a)) = ranked.last().filter(|_| ranked.len() > 1) {
        facts.push(fact("Leggyengébb:", Span::styled(format!("{} ({a:.2})", s.name), Style::new().fg(RED).bold())));
    }
    facts.push(fact(
        "Ötösök aránya:",
        Span::styled(format!("{:.0}%", dist[4] as f64 / total * 100.0), Style::new().fg(GREEN).bold()),
    ));
    let fails = dist[0];
    facts.push(fact(
        "Egyesek:",
        Span::styled(fails.to_string(), Style::new().fg(if fails > 0 { RED } else { GREEN }).bold()),
    ));
    let at_risk: Vec<&str> =
        d.subjects.iter().filter(|s| s.avg.is_some_and(|a| a < 2.0)).map(|s| s.name.as_str()).collect();
    if !at_risk.is_empty() {
        facts.push(fact("Bukásveszély:", Span::styled(at_risk.join(", "), Style::new().fg(RED).bold())));
    }
    let close: Vec<String> = d
        .subjects
        .iter()
        .filter_map(|s| {
            let a = s.avg?;
            let frac = a - a.floor();
            (0.35..0.5).contains(&frac).then(|| format!("{} ({a:.2})", s.name))
        })
        .collect();
    if !close.is_empty() {
        facts.push(fact("Majdnem felfelé kerekít:", Span::styled(close.join(", "), Style::new().fg(YELLOW))));
    }
    let mut weekdays = [0u32; 7];
    for g in &counted {
        weekdays[g.date.weekday().num_days_from_monday() as usize] += 1;
    }
    if let Some((i, _)) = weekdays.iter().enumerate().max_by_key(|(_, n)| **n).filter(|(_, n)| **n > 0) {
        facts.push(fact("Legtöbb jegy napja:", Span::styled(DAYS[i].to_owned(), Style::new().fg(ACCENT2).bold())));
    }
    let mut teachers: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    for g in &counted {
        *teachers.entry(&g.teacher).or_default() += 1;
    }
    if let Some((t, n)) = teachers.into_iter().filter(|(t, _)| !t.is_empty()).max_by_key(|(_, n)| *n) {
        facts.push(fact("Legtöbbet osztályoz:", Span::styled(format!("{t} ({n})"), Style::new().bold())));
    }
    let weighted = counted.iter().filter(|g| g.weight > 100.0).count();
    facts.push(fact("Dupla súlyú jegyek:", Span::styled(weighted.to_string(), Style::new().fg(ACCENT2).bold())));
    f.render_widget(Paragraph::new(facts).wrap(Wrap { trim: false }).block(panel("Érdekességek")), facts_area);
}

// ---- overlays --------------------------------------------------------------

fn draw_help(f: &mut Frame) {
    let area = centered(f.area(), 62, 22);
    f.render_widget(Clear, area);
    let rows: &[(&str, &str)] = &[
        ("Tab / Shift+Tab", "fülek között"),
        ("1 – 6", "fül kiválasztása"),
        ("r", "adatok frissítése"),
        ("q / Ctrl+C", "kilépés"),
        ("L", "kijelentkezés"),
        ("", ""),
        ("Jegyek", ""),
        ("↑↓ / j k", "tantárgy választás"),
        ("→ / Enter", "jegylista"),
        ("s", "szimuláció: 1–5 jegy, w súly, ⌫ vissza"),
        ("", ""),
        ("Órarend", ""),
        ("←→↑↓ / hjkl", "óra kiválasztása"),
        ("n / p", "következő / előző hét"),
        ("t", "vissza a mai naphoz"),
    ];
    let lines: Vec<Line> = rows
        .iter()
        .map(|(k, d)| {
            if d.is_empty() {
                Line::styled(format!(" {k}"), Style::new().fg(ACCENT2).bold())
            } else {
                Line::from(vec![Span::styled(format!("   {k:<16}"), Style::new().fg(ACCENT).bold()), Span::raw(*d)])
            }
        })
        .collect();
    f.render_widget(Paragraph::new(lines).block(panel("Súgó · bármely gomb bezárja")), area);
}

const LOGO: [&str; 3] = ["╦╔═ ╦═╗ ╔═╗ ╔╦╗ ╔═╗", "╠╩╗ ╠╦╝ ║╣   ║  ╠═╣", "╩ ╩ ╩╚═ ╚═╝  ╩  ╩ ╩"];

fn draw_login(f: &mut Frame, app: &App) {
    let Screen::Login(form) = &app.screen else { return };
    let area = centered(f.area(), 74, if form.browser.is_some() { 23 } else { 26 });
    f.render_widget(Clear, area);
    let outer = Block::bordered().border_type(BorderType::Rounded).border_style(Style::new().fg(ACCENT));
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let mut constraints = vec![Constraint::Length(1), Constraint::Length(3), Constraint::Length(2)];
    if form.browser.is_some() {
        constraints.extend([Constraint::Length(8), Constraint::Length(3)]);
    } else {
        constraints.extend([
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
        ]);
    }
    constraints.extend([Constraint::Length(2), Constraint::Min(1)]);
    let chunks = Layout::vertical(constraints).split(inner);

    let colors = [ACCENT, Color::Rgb(155, 158, 247), ACCENT2];
    let logo: Vec<Line> =
        LOGO.iter().zip(colors).map(|(l, c)| Line::styled(*l, Style::new().fg(c).bold()).centered()).collect();
    f.render_widget(Paragraph::new(logo), chunks[1]);
    f.render_widget(
        Paragraph::new(Line::styled("terminál kliens az e-KRÉTÁhoz", Style::new().fg(MUTED)).centered()),
        chunks[2],
    );

    let input = |title: Line<'static>, value: Line<'static>, focused: bool| {
        let mut value = value;
        if focused {
            value.spans.push(Span::styled("▏", Style::new().fg(ACCENT)));
        }
        Paragraph::new(value).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(if focused { ACCENT } else { SURFACE }))
                .title(title.style(Style::new().fg(if focused { ACCENT } else { MUTED }))),
        )
    };

    let mut dropdown_anchor = None;
    let next = if let Some(b) = &form.browser {
        let mut text = vec![Line::styled(" Böngészős belépés", Style::new().fg(ACCENT2).bold())];
        let explain: &[&str] = match b.method {
            Method::Chromium => &[
                " Megnyílt egy külön böngészőablak a KRÉTA belépéssel.",
                " Lépj be ott – utána az ablak magától bezárul, és már bent is vagy.",
                "",
            ],
            Method::Clipboard => &[
                " Megnyitottam a KRÉTA belépést a böngésződben. Belépés után a",
                " mobil.e-kreta.hu/…/oauthredirect?code=… lapra jutsz (üres is lehet):",
                " másold ki a címét (Ctrl+L, Ctrl+C) – a program magától felismeri.",
            ],
        };
        text.extend(explain.iter().map(|l| Line::styled(*l, Style::new().fg(MUTED))));
        text.push(Line::styled(" Ha nem nyílt meg semmi, ezt a címet nyisd meg:", Style::new().fg(MUTED)));
        text.push(Line::styled(format!(" {}", truncate(&b.url, inner.width as usize - 2)), Style::new().fg(SURFACE)));
        f.render_widget(Paragraph::new(text), chunks[3]);
        f.render_widget(
            input(
                Line::raw(" …vagy illeszd be ide az átirányított címet "),
                Line::raw(truncate(&b.input, inner.width as usize - 6)),
                true,
            ),
            chunks[4],
        );
        5
    } else {
        let focused = form.focus == 0;
        let button = Paragraph::new(Line::from(" ▶  Belépés böngészővel ").centered().style(if focused {
            Style::new().fg(Color::Black).bg(ACCENT).bold()
        } else {
            Style::new().fg(ACCENT).bold()
        }))
        .block(
            Block::bordered().border_type(BorderType::Rounded).border_style(Style::new().fg(if focused {
                ACCENT
            } else {
                SURFACE
            })),
        );
        f.render_widget(button, centered(chunks[3], 34, 3));
        f.render_widget(
            Paragraph::new(Line::styled("─── vagy jelszóval ───", Style::new().fg(MUTED)).centered()),
            chunks[4],
        );

        let query = form.fields[0].trim();
        let school_title = if form.searching {
            format!(" Iskola – keresés {} ", SPINNER[app.tick as usize % SPINNER.len()])
        } else if form.school.is_none()
            && form.results.is_empty()
            && query.chars().count() >= 3
            && form.query_sent == query
        {
            " Iskola – nincs találat ".to_owned()
        } else {
            " Iskola (OM azonosító vagy név) ".to_owned()
        };
        let school_value = match &form.school {
            Some(s) => Line::from(vec![
                Span::styled(truncate(&s.name, inner.width as usize - 20), Style::new().fg(GREEN)),
                Span::styled(format!("  OM {}", s.om), Style::new().fg(MUTED)),
            ]),
            None => Line::raw(form.fields[0].clone()),
        };
        f.render_widget(input(Line::raw(school_title), school_value, form.focus == 1), chunks[5]);
        f.render_widget(
            input(
                Line::raw(" Felhasználónév (oktatási azonosító) "),
                Line::raw(form.fields[1].clone()),
                form.focus == 2,
            ),
            chunks[6],
        );
        let masked = "•".repeat(form.fields[2].chars().count());
        f.render_widget(input(Line::raw(" Jelszó "), Line::raw(masked), form.focus == 3), chunks[7]);
        if form.focus == 1 && form.school.is_none() && !form.results.is_empty() {
            dropdown_anchor = Some(chunks[5]);
        }
        8
    };

    let status = if form.busy {
        Line::styled(format!(" {} Bejelentkezés…", SPINNER[app.tick as usize % SPINNER.len()]), Style::new().fg(ACCENT))
    } else if let Some(e) = &form.error {
        Line::styled(format!(" ✗ {e}"), Style::new().fg(RED))
    } else if form.browser.is_some() {
        Line::styled(
            format!(" {} Várakozás a böngészős belépésre…", SPINNER[app.tick as usize % SPINNER.len()]),
            Style::new().fg(ACCENT),
        )
    } else {
        Line::raw("")
    };
    f.render_widget(Paragraph::new(status).wrap(Wrap { trim: true }), chunks[next]);
    let hints: &[(&str, &str)] = if form.browser.is_some() {
        &[("Enter", "beillesztett cím"), ("Esc", "vissza")]
    } else if form.focus == 0 {
        &[("Enter", "belépés böngészővel"), ("↓", "jelszavas belépés"), ("Esc", "kilép")]
    } else if dropdown_anchor.is_some() {
        &[("↑↓", "iskola"), ("Enter", "kiválaszt"), ("Esc", "bezár")]
    } else {
        &[("Enter", "tovább"), ("Tab", "mező"), ("Ctrl+B", "böngésző"), ("Esc", "kilép")]
    };
    f.render_widget(Paragraph::new(key_hints(hints)), chunks[next + 1]);

    // School search results, drawn over the fields below the search box.
    if let Some(anchor) = dropdown_anchor {
        const VISIBLE: usize = 6;
        let n = form.results.len();
        let h = (n.min(VISIBLE) + 2) as u16;
        let rect = Rect::new(anchor.x, anchor.y + anchor.height - 1, anchor.width, h).intersection(f.area());
        let first = form.result_idx.saturating_sub(VISIBLE - 1).min(n.saturating_sub(VISIBLE));
        let lines: Vec<Line> = form
            .results
            .iter()
            .enumerate()
            .skip(first)
            .take(VISIBLE)
            .map(|(i, s)| {
                let selected = i == form.result_idx;
                let line = Line::from(vec![
                    Span::styled(if selected { "▌" } else { " " }, Style::new().fg(ACCENT)),
                    Span::styled(truncate(&s.name, rect.width as usize - 16), Style::new().bold()),
                    Span::styled(format!("  {}", s.om), Style::new().fg(MUTED)),
                ]);
                if selected { line.bg(SURFACE) } else { line }
            })
            .collect();
        f.render_widget(Clear, rect);
        f.render_widget(
            Paragraph::new(lines).block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(Style::new().fg(ACCENT))
                    .title(Line::styled(format!(" {n} találat "), Style::new().fg(MUTED))),
            ),
            rect,
        );
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    /// Renders every tab with demo data; run with `--nocapture` to eyeball the output.
    #[test]
    fn renders_all_tabs() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(tx, true);
        let mut term = Terminal::new(TestBackend::new(130, 40)).unwrap();
        for tab in Tab::ALL {
            app.tab = tab;
            if tab == Tab::Grades {
                app.sim.insert(app.data.subjects[0].name.clone(), vec![(5, 100.0), (5, 200.0)]);
            }
            term.draw(|f| draw(f, &mut app)).unwrap();
            if std::env::var("SHOW").is_ok() {
                let buf = term.backend().buffer();
                let text: Vec<String> =
                    (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()).collect();
                println!("==== {} ====\n{}", tab.title(), text.join("\n"));
            }
        }
        app.show_help = true;
        term.draw(|f| draw(f, &mut app)).unwrap();
    }

    #[test]
    fn renders_login_with_school_dropdown() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(tx, true);
        let mut form = App::empty_login();
        form.focus = 1;
        form.school = None;
        form.fields[0] = "Fazekas".into();
        form.results = (0..8)
            .map(|i| crate::api::auth::School {
                code: format!("klik{i}"),
                name: format!("Fazekas Iskola {i}"),
                om: format!("03527{i}"),
            })
            .collect();
        form.result_idx = 7;
        app.screen = Screen::Login(Box::new(form));
        let mut term = Terminal::new(TestBackend::new(100, 34)).unwrap();
        term.draw(|f| draw(f, &mut app)).unwrap();
        let buf = term.backend().buffer();
        let text: Vec<String> =
            (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()).collect();
        let text = text.join("\n");
        if std::env::var("SHOW").is_ok() {
            println!("{text}");
        }
        assert!(text.contains("Fazekas Iskola 7") && text.contains("8 találat"));
    }
}
