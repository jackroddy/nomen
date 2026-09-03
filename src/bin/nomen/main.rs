//! nomen's frontend.
//!
//! The engine knows nothing about terminals: this binary only feeds it
//! patterns and draws what comes back. Search runs on a worker thread,
//! availability on two more, and the UI thread does nothing but handle keys
//! and render.

mod app;
mod availability;
mod search;
mod settings;
mod theme;
mod ui;

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use nomen::{Availability, Config};

use app::{App, Change, Overlay, Snapshot, Typed};
use search::{Request, Search};

/// How long to wait after the last keystroke before searching.
//
// a debug-build search runs from ~10ms for a strict pattern to ~350ms for
// three `~` blocks over the whole corpus, so this is short enough to feel live
// and long enough that a fast typist does not queue one per character
const DEBOUNCE: Duration = Duration::from_millis(150);

fn main() -> std::io::Result<()> {
    // ratatui::init installs a panic hook that leaves raw mode and the
    // alternate screen, so a crash cannot wreck the user's terminal
    let mut terminal = ratatui::init();
    let result = run(&mut terminal);
    ratatui::restore();
    result
}

fn run(terminal: &mut ratatui::DefaultTerminal) -> std::io::Result<()> {
    // the whole ranking, not a page of it -- the TUI scrolls, so there is no
    // reason to truncate. A loose pattern reaches ~40k results and still sorts
    // in well under the debounce.
    let config = Config {
        top_k: usize::MAX,
        weights: settings::load().unwrap_or_default(),
        ..Config::default()
    };
    let mut app = App::new(config);
    let searcher = Search::spawn();
    let mut snapshot_rx = Some(availability::spawn_snapshot(false));
    let mut recheck_rx: Option<Receiver<(usize, Availability)>> = None;
    let mut snapshot: Option<nomen::Snapshot> = None;

    let mut typed_at: Option<Instant> = None;
    let mut weights_dirty = false;

    loop {
        terminal.draw(|f| ui::draw(f, &mut app))?;

        if event::poll(Duration::from_millis(50))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

            // an overlay takes every key while it is up
            if app.overlay != Overlay::None {
                match (key.code, ctrl) {
                    (KeyCode::Char('c'), true) => app.should_quit = true,
                    (KeyCode::Char('q'), true) => app.open_exit(),
                    (KeyCode::Esc, _) => {
                        if app.overlay == Overlay::Weights {
                            save_weights(&mut app, &mut weights_dirty);
                        }
                        app.close_overlay();
                    }
                    (KeyCode::Char('w'), true) => {
                        save_weights(&mut app, &mut weights_dirty);
                        app.close_overlay();
                    }
                    (KeyCode::Up, _) => match app.overlay {
                        Overlay::Menu => app.menu_move(-1),
                        Overlay::Weights => app.knob_row = app.knob_row.saturating_sub(1),
                        _ => {}
                    },
                    (KeyCode::Down, _) => match app.overlay {
                        Overlay::Menu => app.menu_move(1),
                        Overlay::Weights => {
                            app.knob_row = (app.knob_row + 1).min(app::Knob::ORDER.len() - 1)
                        }
                        _ => {}
                    },
                    (KeyCode::Left, _) | (KeyCode::Right, _) => {
                        let delta = if key.code == KeyCode::Right { 1 } else { -1 };
                        match app.overlay {
                            Overlay::Exit => app.exit_move(delta),
                            Overlay::Weights => {
                                if app.adjust_knob(delta as f32 * 0.05) == Change::Resort {
                                    app.resort();
                                    weights_dirty = true;
                                }
                            }
                            _ => {}
                        }
                    }
                    (KeyCode::Enter, _) => match app.overlay {
                        Overlay::Menu => app.menu_activate(),
                        // the only way out, and it takes three deliberate keys
                        // to reach: esc, enter on `exit`, enter on `yes`
                        Overlay::Exit if app.exit_yes => app.should_quit = true,
                        Overlay::Exit => app.close_overlay(),
                        _ => {}
                    },
                    _ => {}
                }
                continue;
            }

            // the arrow keys walk the whole screen, so every command that is
            // not a movement needs a modifier or a key of its own -- otherwise
            // `q` would quit instead of typing the letter q
            let mut change = Change::None;
            match (key.code, ctrl) {
                (KeyCode::Char('c'), true) => app.should_quit = true,
                // raw mode clears IXON, so ^q reaches us rather than being
                // eaten by the terminal's flow control
                (KeyCode::Char('q'), true) => app.open_exit(),
                (KeyCode::Esc, _) => app.open_menu(),
                (KeyCode::Char('w'), true) => app.open_direct(Overlay::Weights),
                (KeyCode::Char('r'), true) => app.request_recheck(),
                (KeyCode::Char('f'), true) => {
                    snapshot_rx = Some(availability::spawn_snapshot(true));
                    app.set_snapshot(Snapshot::Downloading);
                }
                // the switch shortcuts, looked up rather than listed, so the
                // key and the option it flips are declared in one place
                (KeyCode::Char(c), true) => {
                    if let Some(opt) = app::Opt::from_key(c) {
                        change = app.toggle(opt);
                    }
                }
                (KeyCode::Char(c), false) => {
                    if app.type_char(c) {
                        change = Change::Search;
                    }
                }
                (KeyCode::Backspace, _) => {
                    if app.backspace() {
                        change = Change::Search;
                    }
                }
                (KeyCode::Enter, _) => change = app.activate(),
                (KeyCode::Tab, _) => app.next_box(),
                (KeyCode::BackTab, _) => app.prev_box(),
                (KeyCode::Up, _) => change = app.focus_up(),
                (KeyCode::Down, _) => change = app.focus_down(),
                (KeyCode::Left, _) => app.move_across(-1),
                (KeyCode::Right, _) => app.move_across(1),
                (KeyCode::PageDown, _) => app.move_selection(app.viewport as isize),
                (KeyCode::PageUp, _) => app.move_selection(-(app.viewport as isize)),
                (KeyCode::Home, _) => app.select_first(),
                (KeyCode::End, _) => app.select_last(),
                _ => {}
            }
            if change == Change::Search {
                // reuse the typing debounce so holding a key down does not
                // queue one search per repeat
                typed_at = Some(Instant::now());
            }
        }

        if app.should_quit {
            save_weights(&mut app, &mut weights_dirty);
            return Ok(());
        }

        // debounce: search once typing pauses
        if let Some(at) = typed_at
            && at.elapsed() >= DEBOUNCE
        {
            typed_at = None;
            match app.take_query() {
                // a half-typed bracket is not a mistake; leave the last good
                // results up and wait for the rest of it
                Typed::Invalid => {}
                Typed::Clear => {
                    app.accept(app.generation, Vec::new());
                }
                Typed::Search(pattern, steer) => {
                    app.generation += 1;
                    app.searching = true;
                    searcher.submit(Request {
                        generation: app.generation,
                        pattern,
                        steer,
                        config: app.config.clone(),
                    });
                }
            }
        }

        while let Ok(reply) = searcher.rx.try_recv() {
            let generation = reply.generation;
            let elapsed = reply.elapsed;
            if app.accept(generation, reply.results) {
                app.status.clear();
                if let Some(snap) = &snapshot {
                    apply_snapshot(&mut app, snap);
                }
                let _ = elapsed;
            }
        }

        if let Some(rx) = &snapshot_rx {
            match rx.try_recv() {
                Ok(availability::SnapshotMsg::Downloading) => {
                    app.set_snapshot(Snapshot::Downloading);
                }
                Ok(availability::SnapshotMsg::Ready(snap)) => {
                    app.set_snapshot(Snapshot::Ready { crates: snap.len(), age: snap.age() });
                    apply_snapshot(&mut app, &snap);
                    snapshot = Some(snap);
                }
                Ok(availability::SnapshotMsg::Failed(e)) => {
                    app.set_snapshot(Snapshot::Failed);
                    app.status = format!("snapshot: {e}");
                    snapshot_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => snapshot_rx = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }

        if let Some(rx) = &recheck_rx
            && let Ok((row, status)) = rx.try_recv()
        {
            app.set_row_availability(row, status);
            app.status = String::new();
            recheck_rx = None;
        }

        // 'r' rechecks the selected row; handled here so it can see `current`
        if app.take_recheck_request() {
            // copy out before mutating status, which borrows app
            let target = app.current().map(|(i, s, _)| (i, s.name.clone()));
            if let Some((index, name)) = target {
                app.status = format!("re-checking {name} on crates.io…");
                recheck_rx = Some(availability::spawn_recheck(index, name));
            }
        }
    }
}

/// Persist tuned weights, if any were touched.
//
// written once here rather than at every exit, so a weight nudge cannot be
// lost by leaving through a path that forgot to save
fn save_weights(app: &mut App, dirty: &mut bool) {
    if *dirty {
        settings::save(&app.config.weights);
        *dirty = false;
    }
}

/// Fill in availability for every result from the snapshot.
fn apply_snapshot(app: &mut App, snapshot: &nomen::Snapshot) {
    app.apply_availability_with(|name| snapshot.status(name));
}
