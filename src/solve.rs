//! The assignment search: which letters of a name a pattern's blocks supply,
//! and which are left unaccounted for.

use crate::config::Config;
use crate::lexicon::letter_index;
use crate::query::TermScores;

/// What accounts for one letter position of a name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Slot {
    /// Block `term` of the pattern, its `group`th letter, supplied by the
    /// group's word `word` at that word's letter `letter` (0 being the word's
    /// own first letter).
    Block { term: usize, group: usize, word: usize, letter: u8 },
    /// No block supplies this letter.
    Gap,
    /// The name itself, as in `GNU: Gnu Not Unix`.
    SelfRef,
}

/// A solved assignment of blocks to a name's letter positions.
pub(crate) struct Assignment {
    pub(crate) slots: Vec<Slot>,
    /// Bitmask of which blocks the assignment consumed.
    pub(crate) used: u32,
    pub(crate) gaps: usize,
}

/// What taking the self-reference is worth.
//
// not a tunable weight: recursion is a structural choice the caller already
// made with `allow_recursion`, so there is nothing to trade off. It only has
// to beat leaving the letter as a gap (worth zero) while staying below what a
// real block earns in the same slot -- roughly `usage / name length`, which
// is about 0.5 for a short name -- so a self-reference never displaces a
// block that fits.
const SELF_REF_BONUS: f32 = 0.25;

/// How a boundary was reached, for reconstructing the best path.
#[derive(Clone, Copy)]
enum Step {
    Unreached,
    Gap,
    Term(u8),
    SelfRef,
}

/// Assign the pattern's blocks to the letters of `name`, maximizing the score.
///
/// Returns `None` if a required block cannot be placed, or if every assignment
/// exceeds `config.max_gaps`.
//
// what: a DP over (letter boundary, set of blocks consumed).
//
//   best[i][state] = the best score achievable having filled slots 0..i
//                    using exactly the blocks in `state`
//
// `state` is a bitmask over the pattern's blocks plus one high bit recording
// whether the path used the self-reference. For blocks
// [rust, package, manager], state 0b0101 means rust and manager are placed and
// package is still available; 0b1001 means rust is placed and slot 0 is the
// name referring to itself.
//
// the self-reference needs its own bit because the gap count at the terminal
// is `len - letters filled by blocks - self_reference_used`, and two paths
// reaching the same block mask can differ in whether they took it.
//
// **note: a block is placed in one move, over as many consecutive letters as
// it is wide. That is what makes the `-` joiner free: adjacency is structural
// rather than a constraint to check, so the state needs no memory of what
// filled the previous slot. It is also why a chain is all-or-nothing.
//
// every score component is exact here: `usage` is additive per slot, `coverage`
// and the gap count are read off the state, and `name` does not depend on the
// assignment at all.
pub(crate) fn solve(
    name: &str,
    terms: &[TermScores],
    required_mask: u32,
    bonus_total: usize,
    cfg: &Config,
) -> Option<Assignment> {
    let bytes = name.as_bytes();
    let n = bytes.len();
    let m = terms.len();
    let self_bit = 1usize << m;
    let key_mask = self_bit - 1;
    let states = 1usize << (m + 1);
    let w = &cfg.weights;
    let inv_n = 1.0 / n as f32;

    let mut best = vec![f32::NEG_INFINITY; (n + 1) * states];
    let mut step = vec![Step::Unreached; (n + 1) * states];
    best[0] = 0.0;

    for i in 0..n {
        for state in 0..states {
            let here = best[i * states + state];
            if here == f32::NEG_INFINITY {
                continue;
            }

            let mut relax = |to: usize, next: usize, gain: f32, how: Step| {
                let at = to * states + next;
                if here + gain > best[at] {
                    best[at] = here + gain;
                    step[at] = how;
                }
            };

            // a gap contributes nothing; leaving a letter unaccounted for is
            // neither rewarded nor separately punished, it simply forgoes the
            // usage a block would have earned
            relax(i + 1, state, 0.0, Step::Gap);

            for (j, term) in terms.iter().enumerate() {
                if state & (1 << j) != 0 {
                    continue;
                }
                // the pattern is written in the order it must read, so a block
                // may only be placed if every block already used precedes it
                if (state & key_mask) >> j != 0 {
                    continue;
                }
                let k = term.width();
                if i + k > n {
                    continue;
                }
                let mut gain = 0.0;
                let mut fits = true;
                for (d, g) in term.groups.iter().enumerate() {
                    match letter_index(bytes[i + d]) {
                        Some(li) if g.score[li] >= 0.0 => gain += g.score[li],
                        _ => {
                            fits = false;
                            break;
                        }
                    }
                }
                if fits {
                    relax(i + k, state | (1 << j), w.usage * gain * inv_n, Step::Term(j as u8));
                }
            }

            // only slot 0 may self-refer: every real recursive acronym
            // (GNU, WINE, PHP) puts the self-reference first
            if cfg.allow_recursion && i == 0 && state & self_bit == 0 {
                relax(i + 1, state | self_bit, SELF_REF_BONUS, Step::SelfRef);
            }
        }
    }

    // how many letters a set of blocks accounts for: no longer the popcount,
    // since a `-` chain is one bit but several letters
    let filled = |mask: u32| -> usize {
        (0..m).filter(|j| mask & (1 << j) != 0).map(|j| terms[j].width()).sum()
    };

    let mut chosen = None;
    let mut best_total = f32::NEG_INFINITY;
    for state in 0..states {
        let used = (state & key_mask) as u32;
        if used & required_mask != required_mask {
            continue;
        }
        let score = best[n * states + state];
        if score == f32::NEG_INFINITY {
            continue;
        }

        let gaps = n - filled(used) - usize::from(state & self_bit != 0);
        if let Some(limit) = cfg.max_gaps
            && gaps > limit
        {
            continue;
        }

        // coverage is exact rather than approximated, because the set of
        // blocks used is part of the DP state; without it the search happily
        // drops an optional block whenever another slot outscores it
        let total = score + w.coverage * coverage(used, required_mask, bonus_total);

        if total > best_total {
            best_total = total;
            chosen = Some(state);
        }
    }

    let mut state = chosen?;
    let used = (state & key_mask) as u32;
    let gaps = n - filled(used) - usize::from(state & self_bit != 0);

    let mut slots = vec![Slot::Gap; n];
    let mut b = n;
    while b > 0 {
        match step[b * states + state] {
            Step::Unreached => return None,
            Step::Gap => {
                slots[b - 1] = Slot::Gap;
                b -= 1;
            }
            Step::SelfRef => {
                slots[b - 1] = Slot::SelfRef;
                state &= !self_bit;
                b -= 1;
            }
            Step::Term(j) => {
                let j = j as usize;
                let k = terms[j].width();
                for (d, g) in terms[j].groups.iter().enumerate() {
                    let pos = b - k + d;
                    let li = letter_index(bytes[pos]).expect("a placed block slot has a letter");
                    slots[pos] = Slot::Block {
                        term: j,
                        group: d,
                        word: g.word[li] as usize,
                        letter: g.at[li],
                    };
                }
                state &= !(1 << j);
                b -= k;
            }
        }
    }

    Some(Assignment { slots, used, gaps })
}

/// The share of the pattern's optional blocks an assignment took.
///
/// Required blocks are not counted: they are guaranteed present, so including
/// them would only add a constant.
pub(crate) fn coverage(used: u32, required_mask: u32, bonus_total: usize) -> f32 {
    if bonus_total == 0 {
        return 1.0;
    }
    (used & !required_mask).count_ones() as f32 / bonus_total as f32
}
