//! Nomen Offers Maybe-Excellent Names.
//!
//! Build software names that work as acronyms for a set of keywords.
//!
//! Give [`generate`] a [`Pattern`] and it returns ranked [`Suggestion`]s: a
//! name drawn from the word list, an expansion whose letters the pattern's
//! blocks account for, and the [`Score`] components explaining the ranking.
//!
//! ```no_run
//! use nomen::{Config, Lexicon, Query, Steering, generate, pattern};
//!
//! let lexicon = Lexicon::embedded();
//! let query = Query {
//!     pattern: pattern::parse("~?(source|code) (graph|map)").unwrap(),
//!     steer: Steering::default(),
//! };
//! for s in generate(&query, &lexicon, &Config::default()).unwrap() {
//!     println!("{}", s.name);
//! }
//! ```
//!
//! Nothing here formats output. Every suggestion is data — including the score
//! breakdown — so that a frontend owns all presentation.

pub mod config;
pub mod lexicon;
pub mod pattern;
pub mod query;
pub mod registry;
pub mod semantics;
pub mod snapshot;
pub mod score;
pub mod solve;

pub use config::{Config, Weights};
pub use lexicon::{Lang, Lexicon, WordId};
pub use pattern::{Group, ParseError, Pattern, Term};
pub use query::{MAX_TERMS, Query, Steering};
pub use registry::{Availability, Registry};
pub use semantics::Semantics;
pub use snapshot::Snapshot;
pub use score::Score;
pub use solve::Slot;

use query::TermScores;

/// Why a search could not run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    EmptyPattern,
    TooManyBlocks { got: usize, max: usize },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::EmptyPattern => write!(f, "no keywords given"),
            Error::TooManyBlocks { got, max } => {
                write!(f, "{got} blocks given, at most {max} are supported")
            }
        }
    }
}

impl std::error::Error for Error {}

/// What accounts for one letter of a name.
///
/// Slot `i` always corresponds to letter `i` of [`Suggestion::name`], so a
/// [`Gap`](ExpandedSlot::Gap) needs no payload: the letter it stands for is
/// recoverable from the name.
#[derive(Clone, Debug, PartialEq)]
pub enum ExpandedSlot {
    /// Block `term` of the pattern, at its `group`th letter, resolved to the
    /// alternative that won — `word` — using that word's letter at `letter`
    /// (0 = its own first letter, anything higher means the block was `~`).
    Block { word: String, term: usize, group: usize, letter: u8 },
    /// The name referring to itself, as in `GNU: Gnu Not Unix`.
    SelfRef { word: String },
    /// No block supplies this letter.
    Gap,
}

/// A candidate name and the expansion that spells it.
#[derive(Clone, Debug)]
pub struct Suggestion {
    /// The acronym itself: folded ASCII, lowercase. This is what the
    /// expansion spells and what a package registry is asked about.
    pub name: String,
    /// The entry's original spelling — `aquila`, `νίκη`. Equals `name` for
    /// English, and for Greek differs in script, not just in accents.
    pub display: String,
    /// The dictionary form this name inflects.
    pub lemma: String,
    /// English gloss, empty for English entries.
    pub gloss: String,
    pub lang: Lang,
    pub expansion: Vec<ExpandedSlot>,
    /// How many letters no block accounts for.
    pub gaps: usize,
    pub score: Score,
    /// `score.total(&config.weights)`, precomputed for ranking.
    pub total: f32,
}

/// Rank names for `query`, best first.
pub fn generate(
    query: &Query,
    lex: &Lexicon,
    cfg: &Config,
) -> Result<Vec<Suggestion>, Error> {
    let blocks = &query.pattern.terms;
    if blocks.is_empty() {
        return Err(Error::EmptyPattern);
    }
    if blocks.len() > MAX_TERMS {
        return Err(Error::TooManyBlocks { got: blocks.len(), max: MAX_TERMS });
    }

    let terms: Vec<TermScores> = blocks.iter().map(TermScores::for_term).collect();

    let mut required_mask = 0u32;
    for (i, t) in terms.iter().enumerate() {
        if !t.optional {
            required_mask |= 1 << i;
        }
    }
    let bonus_total = terms.iter().filter(|t| t.optional).count();

    // letters each group of each required block could supply, for the
    // prefilter below. One mask per group rather than per block, since every
    // group of a required block must land somewhere.
    let required_letters: Vec<u32> = terms
        .iter()
        .filter(|t| !t.optional)
        .flat_map(|t| t.groups.iter())
        .map(|g| g.letter_mask())
        .collect();

    // one centroid for the whole query; every candidate is scored against it
    //
    // **note: an alternation contributes all of its words, not the one a given
    // name happened to take. Making this per-assignment would make `relation`
    // depend on the solved assignment, and the single-pass DP rests on every
    // component being independent of it.
    let steering = query.steering_words().join(" ");
    let centroid = lex.semantics().and_then(|s| s.centroid(&steering));

    let mut out: Vec<Suggestion> = Vec::new();

    for id in 0..lex.name_count() as WordId {
        if !cfg.langs.contains(&lex.lang(id)) {
            continue;
        }
        let name = lex.name(id);
        let len = name.len();
        if len < cfg.min_len || len > cfg.max_len {
            continue;
        }

        // a name can only host a required block if it contains a letter each
        // of that block's groups can supply -- necessary, not sufficient, but
        // it rejects most of the list before the DP runs
        if !required_letters.is_empty() {
            let mut name_letters = 0u32;
            for b in name.bytes() {
                if let Some(i) = lexicon::letter_index(b) {
                    name_letters |= 1 << i;
                }
            }
            if required_letters.iter().any(|m| m & name_letters == 0) {
                continue;
            }
        }

        let Some(a) = solve::solve(name, &terms, required_mask, bonus_total, cfg) else {
            continue;
        };

        let mut score = score_assignment(lex, name, id, &a, required_mask, bonus_total, &terms);
        score.relation = match (&centroid, lex.concept(id)) {
            (Some(c), Some(v)) => semantics::Semantics::similarity(c, v),
            // no vector for the steering words or for this entry: score it
            // neutral rather than penalising a word merely for being unusual
            _ => 0.5,
        };
        score.niceness = lex.pleasantness(id);
        let expansion = a
            .slots
            .iter()
            .map(|s| match *s {
                Slot::Block { term, group, word, letter } => ExpandedSlot::Block {
                    word: blocks[term].groups[group].words[word].clone(),
                    term,
                    group,
                    letter,
                },
                Slot::SelfRef => ExpandedSlot::SelfRef { word: name.to_string() },
                Slot::Gap => ExpandedSlot::Gap,
            })
            .collect();

        out.push(Suggestion {
            name: name.to_string(),
            lemma: lex.lemma(id).to_string(),
            display: lex.display(id).to_string(),
            gloss: lex.gloss(id).to_string(),
            lang: lex.lang(id),
            expansion,
            gaps: a.gaps,
            score,
            total: score.total(&cfg.weights),
        });
    }

    out.sort_by(|a, b| b.total.total_cmp(&a.total));
    // two entries can fold to the same acronym -- Greek homographs like
    // agos "leader" and agos "awe", or a Latin form matching an English word.
    // The name is the product, so keep only the best-scoring reading of each.
    //
    // dedup_by would not do: it drops only *consecutive* duplicates, and equal
    // names need not sort adjacently once their scores differ.
    let mut seen = std::collections::HashSet::new();
    let mut per_lemma: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    out.retain(|s| {
        if !seen.insert(s.name.clone()) {
            return false;
        }
        match cfg.max_per_lemma {
            Some(limit) => {
                let n = per_lemma.entry(s.lemma.clone()).or_default();
                *n += 1;
                *n <= limit
            }
            None => true,
        }
    });
    out.truncate(cfg.top_k);
    Ok(out)
}

/// Score a solved assignment. Every component is exact.
fn score_assignment(
    lex: &Lexicon,
    name: &str,
    id: WordId,
    a: &solve::Assignment,
    required_mask: u32,
    bonus_total: usize,
    terms: &[TermScores],
) -> Score {
    let bytes = name.as_bytes();

    let mut usage = 0.0;
    for (i, s) in a.slots.iter().enumerate() {
        if let Slot::Block { term, group, .. } = *s
            && let Some(li) = lexicon::letter_index(bytes[i])
        {
            usage += terms[term].groups[group].score[li];
        }
    }

    Score {
        name: score::name_component(lex.name_quality(id), name.len()),
        coverage: solve::coverage(a.used, required_mask, bonus_total),
        usage: usage / a.slots.len() as f32,
        // filled in by the caller, which owns the query's centroid
        relation: 0.5,
        niceness: 0.5,
    }
}
