//! Score components and how they combine.

use crate::config::Weights;

/// The reasons a suggestion ranked where it did, kept as separate components so
/// a frontend can show the breakdown.
#[derive(Clone, Copy, Debug, Default)]
pub struct Score {
    /// How recognizable and well-shaped the name word is on its own.
    pub name: f32,
    /// Share of the non-required keywords the expansion managed to include.
    pub coverage: f32,
    /// Keyword density: what share of the name's letters a keyword accounts
    /// for, weighted by whether each supplied its own first letter. Every gap
    /// contributes zero, so this is what penalizes an unaccounted-for letter.
    pub usage: f32,
    /// How close the name's meaning is to the keywords, 0..=1 with 0.5 for
    /// unrelated. The reason `cargo` beats `crumpet` for a package manager.
    pub relation: f32,
    /// How pleasant the name's meaning is, 0..=1 with 0.5 for neutral.
    pub niceness: f32,
}

impl Score {
    /// The weighted sum used for ranking.
    pub fn total(&self, w: &Weights) -> f32 {
        self.name * w.name
            + self.coverage * w.coverage
            + self.usage * w.usage
            + self.relation * w.relation
            + self.niceness * w.niceness
    }
}

/// Desirability of a name by letter count, indexed by length.
//
// four and five letters is the sweet spot for a tool name; three is often too
// generic to be free, and past six the acronym stops being said out loud.
// hand-set, not fitted.
const LENGTH_CURVE: [f32; 9] = [0.0, 0.0, 0.0, 0.82, 1.0, 1.0, 0.92, 0.78, 0.62];

/// Compute the name component: how good the word is as a name, independent of
/// what it expands to.
pub(crate) fn name_component(recognizability: f32, len: usize) -> f32 {
    let curve = LENGTH_CURVE.get(len).copied().unwrap_or(0.0);
    // sqrt compresses the long tail: rank 8000 of 15000 is a perfectly good
    // name word, and a linear rank score would rate it half as good as `the`
    recognizability.sqrt() * curve
}
