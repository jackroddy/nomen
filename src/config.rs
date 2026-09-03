//! Search rules and scoring weights.

use crate::lexicon::Lang;

/// The rules governing which names are legal and how many come back.
///
/// These are the rules that apply to a whole search. Anything that varies from
/// keyword to keyword — whether a block is required, whether it may give up an
/// interior letter — lives in the [pattern](crate::pattern) instead, because
/// that is where it belongs: `~rust` is worth relaxing, `~api` is not.
///
/// The rules deliberately do not share an interface: each one takes effect at a
/// different stage of the pipeline (candidate filtering, solver constraint, or
/// scoring), so they are plain fields rather than a uniform abstraction.
#[derive(Clone, Debug)]
pub struct Config {
    /// Shortest acceptable name, in letters.
    pub min_len: usize,
    /// Longest acceptable name, in letters.
    pub max_len: usize,
    /// Allow the name to expand to a phrase containing itself, as in
    /// `GNU: Gnu is Not Unix`.
    pub allow_recursion: bool,
    /// Reject names with more than this many letters no keyword accounts for.
    /// `None` places no limit.
    pub max_gaps: Option<usize>,
    /// Which languages to draw names from.
    pub langs: Vec<Lang>,
    /// At most this many forms of any one dictionary word may be returned.
    /// `None`, the default, places no limit.
    //
    // inflections are kept in the corpus on purpose -- logos, logo and loge
    // are three different acronyms -- so the default shows all of them. Capping
    // is there for when one Greek noun's declension crowds out everything else,
    // and it is one arrow key away in the options box.
    pub max_per_lemma: Option<usize>,
    /// How many suggestions to return.
    pub top_k: usize,
    pub weights: Weights,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            // below 3 letters there is no room for keywords; above 8 the name
            // stops reading as a word people would adopt
            min_len: 3,
            max_len: 8,
            allow_recursion: false,
            max_gaps: None,
            langs: vec![Lang::English, Lang::Latin, Lang::Greek],
            max_per_lemma: None,
            top_k: 20,
            weights: Weights::default(),
        }
    }
}

/// Relative importance of each [`Score`](crate::Score) component.
#[derive(Clone, Debug)]
pub struct Weights {
    pub name: f32,
    pub coverage: f32,
    pub usage: f32,
    pub relation: f32,
    pub niceness: f32,
}

impl Default for Weights {
    fn default() -> Weights {
        // calibrated so each component's *influence* matches its intended
        // importance, which a weight alone does not express: influence is
        // weight x spread, and the components have very different spreads.
        //
        // measured over the top 400 of five real queries (sd of each component,
        // and its resulting share of total influence):
        //
        //     component   sd      share
        //     name        0.098   22%   what makes a word usable as a name
        //     coverage    0.038    7%   near-constant up here; see below
        //     usage       0.097   25%   the only term that penalizes gaps
        //     relation      0.088   34%   the point of the whole exercise
        //     niceness    0.120   11%   widest spread, so a small weight goes far
        //
        // **note: coverage looks under-weighted and is not. Almost every
        // candidate this far up the ranking already covers every keyword, so
        // the term is nearly constant here and a larger weight would only add
        // an offset. It earns its keep further down, where coverage varies.
        Weights {
            name: 1.4,
            coverage: 1.2,
            usage: 1.6,
            relation: 2.4,
            niceness: 0.55,
        }
    }
}
