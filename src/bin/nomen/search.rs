//! The search worker.
//!
//! A search costs anywhere from ~10ms for a pattern of required blocks to
//! ~350ms for three `~` blocks, which is fast enough to run on every keystroke
//! but far too slow to run on the UI thread. One worker owns the lexicon and
//! answers requests; replies carry the generation they were asked for, so a
//! slow search cannot land on top of a newer one.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use nomen::{Config, Lexicon, Pattern, Query, Steering, Suggestion, generate};

pub struct Request {
    pub generation: u64,
    pub pattern: Pattern,
    pub steer: Steering,
    pub config: Config,
}

pub struct Reply {
    pub generation: u64,
    pub results: Vec<Suggestion>,
    pub elapsed: Duration,
}

pub struct Search {
    tx: Sender<Request>,
    pub rx: Receiver<Reply>,
}

impl Search {
    pub fn spawn() -> Search {
        let (req_tx, req_rx) = mpsc::channel::<Request>();
        let (rep_tx, rep_rx) = mpsc::channel::<Reply>();

        thread::spawn(move || {
            // ~5 MB of embedded corpora and vectors; loaded once, here, so the
            // UI thread never waits for it
            let lexicon = Lexicon::embedded();
            while let Ok(mut req) = req_rx.recv() {
                // if the user typed while we were busy, skip straight to what
                // they last asked for
                while let Ok(newer) = req_rx.try_recv() {
                    req = newer;
                }
                let started = Instant::now();
                let query = Query { pattern: req.pattern, steer: req.steer };
                let results = generate(&query, &lexicon, &req.config).unwrap_or_default();
                let reply = Reply {
                    generation: req.generation,
                    results,
                    elapsed: started.elapsed(),
                };
                if rep_tx.send(reply).is_err() {
                    break;
                }
            }
        });

        Search { tx: req_tx, rx: rep_rx }
    }

    pub fn submit(&self, request: Request) {
        let _ = self.tx.send(request);
    }
}
