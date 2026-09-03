//! A search request, and how a pattern's blocks may fill acronym letters.

use crate::lexicon::letter_index;
use crate::pattern::{Group, Pattern, Term};

/// Words that steer what a name should *mean*, as opposed to what it must
/// spell out.
///
/// The pattern says which letters the keywords account for; steering says
/// which region of meaning to prefer among the names that qualify. They are
/// usually the same words, which is why `inherit` defaults to on.
#[derive(Clone, Debug)]
pub struct Steering {
    pub words: Vec<String>,
    /// Also steer by every word in the pattern.
    pub inherit: bool,
}

impl Default for Steering {
    fn default() -> Steering {
        Steering { words: Vec::new(), inherit: true }
    }
}

impl Steering {
    /// Read a line of steering words. Everything but letters is a separator.
    pub fn parse(text: &str, inherit: bool) -> Steering {
        Steering {
            words: text
                .split(|c: char| !c.is_ascii_alphabetic())
                .filter(|w| !w.is_empty())
                .map(str::to_ascii_lowercase)
                .collect(),
            inherit,
        }
    }
}

/// A search request: the pattern to build a name around, and what it should
/// mean.
#[derive(Clone, Debug)]
pub struct Query {
    pub pattern: Pattern,
    pub steer: Steering,
}

impl Query {
    /// The words the `relation` component is scored against.
    pub fn steering_words(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self.steer.words.iter().map(String::as_str).collect();
        if self.steer.inherit {
            out.extend(self.pattern.words());
        }
        out
    }
}

/// The solver's state is a bitmask over the pattern's blocks, so the count has
/// to stay small enough that `1 << terms.len()` is cheap to sweep per
/// candidate. Alternatives within a block are free — they merge into one
/// table, below.
pub const MAX_TERMS: usize = 6;

/// Score for a word supplying its own first letter.
const INITIAL: f32 = 1.0;
/// Score for a word supplying its letter at index 1.
//
// hand-chosen so that any initial-letter use outranks every interior use:
// INTERIOR_BASE < INITIAL, and the decay keeps later letters worse than
// earlier ones without ever reaching zero
const INTERIOR_BASE: f32 = 0.5;
const INTERIOR_DECAY: f32 = 0.04;
const INTERIOR_FLOOR: f32 = 0.2;

/// How well one group covers a given slot letter, precomputed per letter.
//
// -1.0 marks a letter no word of the group can supply. Three arrays of 26 per
// group stay in cache across the whole lexicon scan, which matters because
// this is read in the DP's inner loop.
//
// **note: an alternation costs the solver nothing. Only one word of a group
// may ever be used and the group fills exactly one letter, so merging the
// alternatives best-per-letter leaves a group indistinguishable from a single
// keyword -- which word won is recovered at backtrack time from `word`.
pub(crate) struct GroupScores {
    pub(crate) score: [f32; 26],
    /// Index of the letter within the winning word, for reporting the use.
    pub(crate) at: [u8; 26],
    /// Which of the group's words supplies the letter.
    pub(crate) word: [u8; 26],
}

impl GroupScores {
    pub(crate) fn for_group(group: &Group) -> GroupScores {
        let mut s = GroupScores { score: [-1.0; 26], at: [0; 26], word: [0; 26] };

        for (w, text) in group.words.iter().enumerate() {
            for (i, b) in text.bytes().enumerate() {
                let Some(li) = letter_index(b) else { continue };
                if i > 0 && !group.interior {
                    break;
                }
                let gain = if i == 0 {
                    INITIAL
                } else {
                    (INTERIOR_BASE - INTERIOR_DECAY * (i - 1) as f32).max(INTERIOR_FLOOR)
                };
                // a letter can occur twice in one word, and in several words of
                // one group; keep the single best way of supplying it
                if gain > s.score[li] {
                    s.score[li] = gain;
                    s.at[li] = i as u8;
                    s.word[li] = w as u8;
                }
            }
        }
        s
    }

    /// The set of letters this group can supply, as a 26-bit mask.
    pub(crate) fn letter_mask(&self) -> u32 {
        let mut mask = 0;
        for (i, s) in self.score.iter().enumerate() {
            if *s >= 0.0 {
                mask |= 1 << i;
            }
        }
        mask
    }
}

/// One block, ready for the solver: a run of groups filling consecutive
/// letters, taken whole or not at all.
pub(crate) struct TermScores {
    pub(crate) groups: Vec<GroupScores>,
    pub(crate) optional: bool,
}

impl TermScores {
    pub(crate) fn for_term(term: &Term) -> TermScores {
        TermScores {
            groups: term.groups.iter().map(GroupScores::for_group).collect(),
            optional: term.optional,
        }
    }

    /// How many letters of the name this block occupies.
    pub(crate) fn width(&self) -> usize {
        self.groups.len()
    }
}
