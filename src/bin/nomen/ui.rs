//! Rendering. Reads [`App`], draws, and changes nothing but the viewport size.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::{
    App, Box_, Focus, HELP, HELP_KEY_WIDTH, HELP_WIDTH, HelpLine, Knob, MenuItem, Opt, Overlay,
    Snapshot, Tab,
};

/// Gap between one options cell and the next.
const CELL_GAP: usize = 2;
use crate::theme;

pub fn draw(f: &mut Frame, app: &mut App) {
    // the options grid reflows with the terminal, so its height is not known
    // until the width is
    app.set_option_columns(option_columns(f.area().width));

    let [query, options, list, status] = Layout::vertical([
        // two input lines inside one border: what the name must spell, and
        // what it should mean
        Constraint::Length(4),
        Constraint::Length(app.option_rows() as u16 + 2),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(f.area());

    draw_query(f, query, app);
    draw_options(f, options, app);
    draw_results(f, list, app);
    draw_status(f, status, app);

    // everything reached from esc floats over the boxes, so that leaving one
    // never rearranges what is underneath it
    match app.overlay {
        Overlay::None => {}
        Overlay::Menu => draw_menu(f, f.area(), app),
        Overlay::Help => draw_help(f, f.area()),
        Overlay::Weights => draw_weights(f, f.area(), app),
        Overlay::Exit => draw_exit(f, f.area(), app),
    }
}

/// The widest an option's value can ever read: `none` is longer than `[x]` and
/// longer than any length this corpus allows.
//
// measured against the widest *possible* value rather than the current one, so
// that stepping max-gaps from `none` to `8` cannot reflow the grid under the
// cursor
const VALUE_WIDTH: usize = 4;

/// Width of one options cell: every option padded to the longest, so the box
/// reads as a grid rather than as a ragged run of labels. A ragged grid is
/// harder to arrow around than a narrow one.
fn cell_width() -> usize {
    Opt::ORDER.iter().map(|o| o.label().chars().count()).max().unwrap_or(1)
        + 1
        + VALUE_WIDTH
        + CELL_GAP
}

/// How many cells fit across a terminal of this width.
fn option_columns(width: u16) -> usize {
    ((width as usize).saturating_sub(4) / cell_width()).clamp(1, Opt::ORDER.len())
}

/// The border of the box the arrow keys are in, versus every other box.
//
// with the cursor free to walk out of one box and into the next, the lit
// border is the only thing saying where it landed
fn border_style(app: &App, which: Box_) -> Style {
    if app.overlay == Overlay::None && app.focus.box_() == which {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

/// Centre a box of at most `w` x `h` inside `area`.
fn centred(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}

/// The esc menu. Everything modal hangs off it.
fn draw_menu(f: &mut Frame, area: Rect, app: &App) {
    let box_area = centred(area, 40, MenuItem::ORDER.len() as u16 + 2);
    f.render_widget(Clear, box_area);

    let lines: Vec<Line> = MenuItem::ORDER
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let picked = i == app.menu_row;
            let row = Line::from(vec![
                Span::raw(format!("  {:<10}", item.label())),
                Span::styled(item.note(), Style::default().add_modifier(Modifier::DIM)),
            ]);
            if picked {
                row.style(Style::default().bg(Color::Indexed(238)))
            } else {
                row
            }
        })
        .collect();

    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" nomen ")
                .title_bottom(Line::styled(
                    " ↑↓ · enter · esc ",
                    Style::default().add_modifier(Modifier::DIM),
                )),
        ),
        box_area,
    );
}

/// `exit?  [yes] [no]` — arrow between the buttons, enter to answer.
fn draw_exit(f: &mut Frame, area: Rect, app: &App) {
    let box_area = centred(area, 32, 5);
    f.render_widget(Clear, box_area);

    let button = |text: &'static str, picked: bool| {
        Span::styled(
            text,
            if picked {
                // reversed rather than merely coloured, so which button is
                // armed survives a terminal with no colour
                Style::default().fg(Color::Black).bg(Color::Yellow)
            } else {
                Style::default().add_modifier(Modifier::DIM)
            },
        )
    };

    f.render_widget(
        Paragraph::new(vec![
            Line::from(""),
            Line::from(vec![
                Span::raw("       "),
                button("[yes]", app.exit_yes),
                Span::raw("    "),
                button("[no]", !app.exit_yes),
            ]),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title(" exit? "),
        ),
        box_area,
    );
}

/// The help overlay: the pattern syntax, the keys, and the way out.
//
// anchored to the top of `area` rather than centred in it, so it replaces the
// toggle bar cleanly instead of slicing through it. On a terminal too short to
// hold it the box clips -- which is survivable only because the status line
// repeats `enter quit` underneath.
fn draw_help(f: &mut Frame, area: Rect) {
    let box_area = centred(area, HELP_WIDTH, HELP.len() as u16 + 2);
    f.render_widget(Clear, box_area);

    let lines: Vec<Line> = HELP
        .iter()
        .map(|l| match l {
            HelpLine::Head(t) => Line::styled(
                format!(" {t}"),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            HelpLine::Blank => Line::from(""),
            HelpLine::Row(key, what) => Line::from(vec![
                Span::styled(
                    format!("  {key:<HELP_KEY_WIDTH$}"),
                    Style::default().fg(Color::Indexed(109)),
                ),
                Span::styled(*what, Style::default().fg(Color::Indexed(245))),
            ]),
        })
        .collect();

    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" help ")
                .title_bottom(Line::styled(
                    " esc: back ",
                    Style::default().add_modifier(Modifier::DIM),
                )),
        ),
        box_area,
    );
}

/// Every option, always on screen, laid out as a grid that reflows with the
/// terminal.
//
// this used to be a panel behind ^o, which meant the state of the search was
// only visible while you were changing it. The ctrl shortcuts are no longer
// drawn beside the labels -- a `^n ` in front of every third cell broke the
// columns, and the help overlay carries them.
fn draw_options(f: &mut Frame, area: Rect, app: &App) {
    let here = app.overlay == Overlay::None && app.focus == Focus::Options;
    let cursor = app.option();
    let width = cell_width();
    let columns = app.option_columns();

    let mut lines = Vec::new();
    for row in Opt::ORDER.chunks(columns) {
        let mut spans = Vec::new();
        for opt in row {
            let on_it = here && *opt == cursor;
            let text = format!("{} {}", opt.label(), app.option_value(*opt));
            let pad = width.saturating_sub(text.chars().count() + CELL_GAP);

            let style = if on_it && app.adjusting {
                // picked up: up and down now step it rather than leaving
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else if on_it {
                Style::default().bg(Color::Indexed(238))
            } else if opt.is_toggle() && !app.option_on(*opt) {
                Style::default().add_modifier(Modifier::DIM)
            } else {
                Style::default()
            };

            spans.push(Span::raw(" ".repeat(CELL_GAP)));
            spans.push(Span::styled(text, style));
            spans.push(Span::raw(" ".repeat(pad)));
        }
        lines.push(Line::from(spans));
    }

    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style(app, Box_::Options))
                .title(" options "),
        ),
        area,
    );
}

/// The two input lines.
//
// the border carries nothing but a parse error, and only while there is one --
// so a decorated border always means something is wrong. The syntax it used to
// spell out lives in the help overlay, which has room for it.
fn draw_query(f: &mut Frame, area: Rect, app: &App) {
    let title = if app.searching { " search (searching…) " } else { " search " };
    // a panel steals the keys, so the cursor should not claim to be in a field
    let live = app.overlay == Overlay::None;

    let field = |label: &'static str, on: bool, text: &str, tail: Vec<Span<'static>>| {
        let mut spans = vec![Span::styled(
            format!(" {label:>8} "),
            if on {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().add_modifier(Modifier::DIM)
            },
        )];
        spans.push(Span::raw(text.to_string()));
        if on {
            spans.push(Span::styled("▏", Style::default().fg(Color::Cyan)));
        }
        spans.extend(tail);
        Line::from(spans)
    };

    let body = vec![
        field("pattern", live && app.focus == Focus::Pattern, &app.query, Vec::new()),
        field("steer", live && app.focus == Focus::Steer, &app.steer, Vec::new()),
    ];

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style(app, Box_::Search))
        .title(title);
    if let Some(e) = &app.parse_error {
        block = block.title_bottom(Line::styled(
            format!(" {e} — column {} ", e.column(&app.query)),
            Style::default().fg(Color::Yellow),
        ));
    }

    f.render_widget(Paragraph::new(body).block(block), area);
}

/// The language tabs, as the results box's own title — they select which
/// slice of the ranking the box shows, so they belong to it.
fn tab_title(app: &App) -> Line<'static> {
    let mut spans = Vec::new();
    for (i, t) in Tab::ORDER.iter().enumerate() {
        spans.push(Span::raw(if i == 0 { " " } else { "\u{2502}" }));
        let text = format!(" {} {} ", t.label(), app.count_for(i));
        spans.push(if i == app.tab {
            Span::styled(text, Style::default().fg(Color::Black).bg(Color::Cyan))
        } else {
            Span::styled(text, Style::default().add_modifier(Modifier::DIM))
        });
    }
    Line::from(spans)
}

fn draw_results(f: &mut Frame, outer: Rect, app: &mut App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style(app, Box_::Results))
        .title(tab_title(app));
    let area = block.inner(outer);
    f.render_widget(block, outer);

    app.set_viewport(area.height as usize);

    if app.view().is_empty() {
        // the hint is App's to decide -- it is the only part of an empty
        // screen worth testing, and it reads the pattern to pick its advice
        let lines: Vec<Line> = app
            .empty_hint()
            .into_iter()
            .map(|l| Line::styled(l, Style::default().add_modifier(Modifier::DIM)))
            .collect();
        f.render_widget(Paragraph::new(lines), area);
        return;
    }

    // build only the rows on screen: the ranking runs to ~77k entries, and
    // materialising all of them every frame would be hopeless
    let start = app.offset();
    let end = (start + area.height as usize).min(app.view().len());
    let selected = app.selected();

    // measured once per search, so the gloss column cannot shift as the list
    // scrolls under it
    let layout = theme::RowLayout {
        width: area.width,
        expansion: app.expansion_width(),
        lang_tag: Tab::ORDER[app.tab] == Tab::All,
    };

    let lines: Vec<Line> = (start..end)
        .filter_map(|i| {
            let (s, status) = app.row(i)?;
            let mut line = theme::row(s, status, layout);
            if i == selected {
                line = line.style(Style::default().bg(Color::Indexed(236)));
            }
            Some(line)
        })
        .collect();

    f.render_widget(Paragraph::new(lines), area);
}

/// Description and worked example for whatever is under the panel cursor.
//
// the examples matter more than the prose: "a keyword may only supply its own
// first letter" takes a moment, `Rust / rUst / ruSt` takes none
fn detail(description: &str, example: &[&str], width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from("")];
    for chunk in wrap(description, width) {
        lines.push(Line::styled(format!(" {chunk}"), Style::default().fg(Color::Indexed(245))));
    }
    if !example.is_empty() {
        lines.push(Line::from(""));
        for row in example {
            lines.push(Line::styled(
                format!("   {row}"),
                Style::default().fg(Color::Indexed(109)),
            ));
        }
    }
    lines
}

fn draw_weights(f: &mut Frame, area: Rect, app: &App) {
    let area = centred(area, 52, area.height.min(24));
    f.render_widget(Clear, area);
    let mut lines: Vec<Line> = Knob::ORDER
        .iter()
        .enumerate()
        .map(|(i, knob)| {
            let v = knob.get(&app.config.weights);
            // a bar makes the relative sizes readable at a glance, which is
            // the thing being tuned
            let filled = ((v / 5.0) * 12.0).round() as usize;
            let row = Line::from(vec![
                Span::raw(format!(" {:<10}", knob.label())),
                Span::styled(format!("{v:>4.2} "), Style::default().fg(Color::Cyan)),
                Span::styled(
                    "█".repeat(filled.min(12)),
                    Style::default().fg(Color::Indexed(39)),
                ),
            ]);
            if i == app.knob_row { row.style(Style::default().bg(Color::Indexed(236))) } else { row }
        })
        .collect();

    let picked = Knob::ORDER[app.knob_row.min(Knob::ORDER.len() - 1)];
    lines.extend(detail(picked.description(), picked.example(), area.width.saturating_sub(4) as usize));

    if let Some((_, s, _)) = app.current() {
        lines.push(Line::from(""));
        lines.push(Line::styled(
            format!(" {} — total {:.3}", s.name.to_uppercase(), s.total),
            Style::default().add_modifier(Modifier::BOLD),
        ));
        let c = &s.score;
        for (label, value, weight) in [
            ("name", c.name, app.config.weights.name),
            ("coverage", c.coverage, app.config.weights.coverage),
            ("usage", c.usage, app.config.weights.usage),
            ("relation", c.relation, app.config.weights.relation),
            ("niceness", c.niceness, app.config.weights.niceness),
        ] {
            lines.push(Line::styled(
                format!(" {label:<10}{value:>5.2} × {weight:<4.2} = {:>5.2}", value * weight),
                Style::default().add_modifier(Modifier::DIM),
            ));
        }
        lines.push(Line::styled(
            format!(" {:<10}{} gap(s)", "gaps", s.gaps),
            Style::default().add_modifier(Modifier::DIM),
        ));
    }

    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" weights ")
                .title_bottom(Line::styled(
                    " ↑↓ pick · ←→ tune · esc back ",
                    Style::default().add_modifier(Modifier::DIM),
                )),
        ),
        area,
    );
}

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    let snapshot = match app.snapshot {
        Snapshot::Missing => "no crates.io snapshot".to_string(),
        Snapshot::Downloading => "downloading crates.io snapshot…".to_string(),
        Snapshot::Failed => "snapshot download failed".to_string(),
        Snapshot::Ready { crates, age } => {
            format!("{} crates, {}d old", crates, age.as_secs() / 86_400)
        }
    };
    let left = if app.status.is_empty() {
        format!(
            "{} of {} results   {}",
            app.view().len(),
            app.result_count(),
            snapshot
        )
    } else {
        app.status.clone()
    };
    // the options box has no room to explain itself, so the status line does
    // it while the cursor is on one
    if app.overlay == Overlay::None && app.focus == Focus::Options && app.status.is_empty() {
        let opt = app.option();
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(" {} ", opt.label()),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    opt.description(),
                    Style::default().add_modifier(Modifier::DIM),
                ),
            ])),
            area,
        );
        return;
    }

    let keys = match app.overlay {
        // terse on purpose: the left half of this line is already 40 columns
        // of counts, and an 80-column terminal has to fit both. Everything
        // else lives behind esc, and the help overlay lists it.
        Overlay::None => "  ^w weights  ^q quit  esc menu",
        Overlay::Menu => "  ↑↓ pick  enter open  esc close",
        Overlay::Weights => "  ↑↓ pick  ←→ tune  esc back",
        Overlay::Exit => "  ←→ pick  enter answer  esc back",
        Overlay::Help => "  esc back",
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw(left),
            Span::styled(keys, Style::default().add_modifier(Modifier::DIM)),
        ])),
        area,
    );
}

/// Break text onto lines of at most `width` columns, on word boundaries.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use nomen::{Availability, Config, ExpandedSlot, Lang, Score, Suggestion};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn sample() -> Vec<Suggestion> {
        vec![
            Suggestion {
                name: "logos".into(),
                display: "λόγος".into(),
                gloss: "that which is said".into(),
                lemma: "λόγος".into(),
                lang: Lang::Greek,
                expansion: vec![
                    ExpandedSlot::Gap,
                    ExpandedSlot::Block { word: "code".into(), term: 0, group: 0, letter: 3 },
                    ExpandedSlot::Gap,
                    ExpandedSlot::Block { word: "source".into(), term: 1, group: 0, letter: 0 },
                    ExpandedSlot::Gap,
                ],
                gaps: 3,
                score: Score::default(),
                total: 2.0,
            },
            Suggestion {
                name: "user".into(),
                display: "user".into(),
                gloss: String::new(),
                lemma: "user".into(),
                lang: Lang::English,
                expansion: vec![ExpandedSlot::Gap; 4],
                gaps: 4,
                score: Score::default(),
                total: 1.0,
            },
        ]
    }

    fn app() -> App {
        let mut app = App::new(Config::default());
        // these fixtures deliberately include a taken row, so show everything
        app.only_available = false;
        app.query = "source code".into();
        app.set_snapshot(Snapshot::Ready { crates: 327_000, age: std::time::Duration::ZERO });
        app.accept(0, sample());
        app.apply_availability(vec![Availability::Taken, Availability::Available]);
        app
    }

    fn render(width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut app = app();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect::<String>()
    }

    #[test]
    fn draws_at_a_normal_terminal_size() {
        let out = render(100, 24);
        assert!(out.contains("LOGOS"), "the acronym is shown");
        assert!(out.contains("source code"), "the query is echoed");
        assert!(out.contains("english"), "the tab bar is drawn");
        assert!(out.contains("λόγος"), "the original spelling is annotated");
    }

    /// Where each gloss starts, and whether it sets its headword in italic.
    fn glosses(app: &mut App, width: u16, height: u16) -> Vec<(usize, bool)> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let b = terminal.backend().buffer();
        (0..height)
            .filter_map(|y| {
                let row: String =
                    (0..width).map(|x| b[(x, y)].symbol()).collect::<Vec<_>>().concat();
                // a gloss row is one carrying the opening quote; `[` alone
                // would also match the options box's own tick boxes
                if !row.contains('\u{201c}') {
                    return None;
                }
                let at = (0..width).find(|x| b[(*x, y)].symbol() == "[")?;
                let italic =
                    (0..width).any(|x| b[(x, y)].modifier.contains(Modifier::ITALIC));
                Some((at as usize, italic))
            })
            .collect()
    }

    #[test]
    fn every_gloss_starts_in_the_same_column_with_an_italic_headword() {
        let mut app = app();
        let found = glosses(&mut app, 100, 24);
        assert!(!found.is_empty(), "no gloss was drawn");
        let first = found[0].0;
        for (at, italic) in &found {
            assert_eq!(*at, first, "the glosses do not line up");
            // a foreign word set inside English, which is what italic means
            assert!(italic, "the headword is not italic");
        }
    }

    #[test]
    fn only_the_all_tab_names_the_language() {
        let mut app = app();

        app.tab = Tab::ORDER.iter().position(|t| *t == Tab::All).unwrap();
        let mixed = render_app(&mut app, 100, 24);
        assert!(mixed.contains("[grc"), "the all tab mixes languages, so it has to say");

        app.tab = Tab::ORDER.iter().position(|t| *t == Tab::Greek).unwrap();
        let single = render_app(&mut app, 100, 24);
        assert!(single.contains("λόγος"), "the original spelling is kept either way");
        assert!(!single.contains("[grc"), "but the tab has already said which language");
    }

    #[test]
    fn a_narrow_terminal_drops_the_gloss_rather_than_wrapping() {
        // the annotation is the first thing to go; the acronym must survive
        let out = render(44, 12);
        assert!(out.contains("LOGOS"));
        assert!(!out.contains("that which is said"), "the gloss was dropped, not wrapped");
    }

    fn render_app(app: &mut App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn both_input_lines_are_drawn() {
        let mut app = app();
        app.query = "~?(source|code) graph".into();
        app.steer = "theory".into();
        let out = render_app(&mut app, 100, 24);

        assert!(out.contains("~?(source|code) graph"), "the pattern line");
        assert!(out.contains("theory"), "the steering line");
    }

    /// The colour of the top-left corner of each box, which is what says
    /// where the arrow keys are.
    fn corner_colours(app: &mut App) -> Vec<Color> {
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let b = terminal.backend().buffer();
        // search, options, results: the three box tops, in screen order
        [0u16, 4, 8].iter().map(|y| b[(0, *y)].fg).collect()
    }

    #[test]
    fn only_the_box_holding_the_cursor_has_a_lit_border() {
        let mut app = app();
        for (focus, lit) in
            [(Focus::Pattern, 0), (Focus::Steer, 0), (Focus::Options, 1), (Focus::Results, 2)]
        {
            app.focus = focus;
            let borders = corner_colours(&mut app);
            for (i, c) in borders.iter().enumerate() {
                let want = if i == lit { Color::Cyan } else { Color::DarkGray };
                assert_eq!(*c, want, "{focus:?}: box {i} is the wrong colour");
            }
        }
    }

    #[test]
    fn an_overlay_dims_every_border_because_the_arrows_are_elsewhere() {
        let mut app = app();
        app.focus = Focus::Results;
        app.overlay = Overlay::Weights;
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let b = terminal.backend().buffer();
        assert_eq!(b[(0, 0)].fg, Color::DarkGray);
        assert_eq!(b[(0, 4)].fg, Color::DarkGray);
    }

    #[test]
    fn the_hovered_option_is_explained_in_the_status_line() {
        let mut app = app();
        app.focus = Focus::Options;
        app.option_row = Opt::ORDER.iter().position(|o| *o == Opt::MaxGaps).unwrap();
        let out = render_app(&mut app, 100, 24);
        // the box has no room to explain itself, so the status line does
        assert!(out.contains("unaccounted for"), "{out}");
    }

    #[test]
    fn the_options_box_shows_every_switch_and_number_at_once() {
        let mut app = app();
        app.inherit = true;
        app.config.allow_recursion = false;
        let out = render_app(&mut app, 100, 24);

        assert!(out.contains("inherit [x]"), "on reads as ticked");
        assert!(out.contains("recursive [ ]"), "off reads as empty");
        assert!(out.contains("only available"));
        // the numbers used to be behind ^o, so the state of the search was
        // only visible while you were changing it
        assert!(out.contains("min length 3"));
        assert!(out.contains("max gaps none"));
        // the ctrl shortcuts still fire but are no longer drawn: a `^n ` in
        // front of every third cell broke the grid columns
        assert!(!out.contains("^n"), "the shortcut prefixes are gone from the box");
    }

    #[test]
    fn the_language_tabs_title_the_results_box() {
        let mut app = app();
        let out = render_app(&mut app, 100, 24);
        assert!(out.contains("english"), "the tabs are the box's own title");
        assert!(out.contains("all 2"));
    }

    #[test]
    fn the_search_border_stays_bare_until_the_pattern_breaks() {
        let mut app = app();
        app.query = "source code".into();
        app.take_query();
        let clean = render_app(&mut app, 100, 24);
        assert!(!clean.contains("column"), "nothing on the border while all is well");

        app.query = "(source|code".into();
        app.take_query();
        let broken = render_app(&mut app, 100, 24);
        assert!(broken.contains("unclosed"), "the error takes the border");
    }

    #[test]
    fn esc_opens_help_that_carries_the_syntax_and_the_way_out() {
        let mut app = app();
        app.overlay = Overlay::Help;
        let out = render_app(&mut app, 100, 24);

        // the syntax that used to live on the search border has to survive
        // somewhere, and this is the only place left
        assert!(out.contains("~block"), "the syntax reference");
        assert!(out.contains("a-b"));
        assert!(out.contains("esc: back"));
    }

    #[test]
    fn esc_draws_a_menu_of_everything_behind_it() {
        let mut app = app();
        app.overlay = Overlay::Menu;
        let out = render_app(&mut app, 100, 24);
        for item in MenuItem::ORDER {
            assert!(out.contains(item.label()), "{item:?} is missing from the menu");
        }
    }

    #[test]
    fn the_exit_dialog_offers_two_buttons() {
        let mut app = app();
        app.overlay = Overlay::Exit;
        let out = render_app(&mut app, 100, 24);
        assert!(out.contains("exit?"));
        assert!(out.contains("yes") && out.contains("no"));
    }

    #[test]
    fn an_empty_result_set_explains_itself() {
        let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
        let mut app = App::new(Config::default());
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let out: String =
            terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect();
        assert!(out.contains("type keywords"));
    }
}
