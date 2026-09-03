//! Word vectors, and the two things we ask of them: does this name relation to
//! the keywords, and does it sound pleasant.
//!
//! Everything here works in English. A Latin or Greek entry is represented by
//! its gloss, so one English vector table serves every language and no
//! cross-lingual alignment is needed — which is just as well, since aligned
//! vectors are not published for Latin or Ancient Greek at all.

use std::collections::HashMap;

/// Width of the vendored vectors (GloVe 6B, 50 dimensions).
pub const DIM: usize = 50;

const VOCAB: &str = include_str!("../data/vocab.txt");
/// Unit vectors quantized to `i8`, row-major, `DIM` per word.
const VOCAB_VEC: &[u8] = include_bytes!("../data/vocab_vec.i8");
/// VADER valence scores, `word<TAB>score`, roughly -4..=4.
const SENTIMENT: &str = include_str!("../data/sentiment.tsv");

/// Words too common to carry meaning when averaging a gloss.
const STOP: [&str; 22] = [
    "the", "a", "an", "of", "to", "in", "for", "and", "or", "is", "was", "be", "by", "on", "at",
    "as", "with", "from", "that", "this", "it", "its",
];

/// Direction of "pleasant" in the embedding space, as a unit vector.
//
// what: mean(positive seed words) - mean(negative seed words), normalized.
// why: a curated sentiment lexicon is more reliable per word but covers only
//      7k of the 47k vocabulary. Projecting onto this axis scores every word
//      we have a vector for.
// seeds: good/excellent/beautiful/elegant/wonderful/superb/delightful/
//        pleasant/lovely/graceful against bad/awful/ugly/disgusting/
//        horrible/terrible/nasty/filthy/vile/repulsive
const PLEASANT_AXIS: [f32; DIM] = [
    0.031646, 0.368460, -0.107592, 0.007754, 0.138708, -0.060001, -0.292415, -0.055073, 0.072187,
    0.082339, 0.034419, -0.058875, 0.206761, 0.029722, -0.251951, -0.007073, 0.086464, -0.003454,
    -0.016212, -0.164687, 0.060706, 0.092694, -0.245041, -0.045303, 0.082862, 0.118199, 0.088233,
    -0.017708, -0.272297, -0.042650, 0.230133, -0.015366, 0.004977, 0.145354, 0.262850, -0.078559,
    -0.032680, 0.353743, -0.089292, -0.095282, 0.192796, 0.043986, -0.057299, -0.127304, -0.131431,
    -0.017742, 0.092727, -0.098219, -0.121809, -0.011663,
];

/// How much a VADER entry outweighs the axis projection when both exist.
//
// the lexicon is hand-scored and more trustworthy per word; the axis is the
// only signal for the 40k words VADER has never heard of
const VADER_SHARE: f32 = 0.6;
/// VADER scores run about -4..=4; divide to land in -1..=1 like the axis.
const VADER_SCALE: f32 = 4.0;

/// Content words to take from one gloss: just the head word.
//
// **note: averaging drags a vector toward generic English, and generic English
// is close to *any* query -- so a longer gloss scores higher against everything.
// That is a systematic advantage for glossed entries over plain English words,
// and it is measurable: mean cosine against a fixed query ran +0.037 higher for
// Latin than for English at four tokens, +0.031 at two, and +0.001 at one.
// Taking only the head word removes the bias outright rather than correcting
// for it afterwards. Cutting the gloss to its head sense at build time is what
// makes one token enough.
const MAX_GLOSS_TOKENS: usize = 1;

/// Cosine below which a pairing is treated as unrelated.
//
// measured against a real query with MAX_GLOSS_TOKENS applied: p50 +0.26,
// p90 +0.50, p99 +0.66, max +0.83. Mapping the raw -1..=1 range onto 0..=1
// squeezed everything into 0.58..0.78 and the component stopped
// discriminating; a ceiling at the p90 mark instead made the whole top of the
// ranking clip to 1.00. These bounds put p50 near 0.27 and p99 near 0.93.
const RELATION_FLOOR: f32 = 0.10;
const RELATION_CEIL: f32 = 0.70;

/// The vendored vector table and sentiment lexicon.
pub struct Semantics {
    index: HashMap<&'static str, u32>,
    valence: HashMap<&'static str, f32>,
}

impl Semantics {
    pub fn embedded() -> Semantics {
        let index = VOCAB
            .lines()
            .enumerate()
            .map(|(i, w)| (w, i as u32))
            .collect::<HashMap<_, _>>();

        let valence = SENTIMENT
            .lines()
            .filter_map(|l| {
                let (w, s) = l.split_once('\t')?;
                Some((w, s.trim().parse::<f32>().ok()? / VADER_SCALE))
            })
            .collect();

        Semantics { index, valence }
    }

    /// Whether English uses this token at all.
    pub fn knows(&self, word: &str) -> bool {
        self.index.contains_key(word)
    }

    /// The quantized unit vector for one word, if we have one.
    //
    // the file stores signed bytes; callers widen each with `as i8` rather
    // than reinterpreting the slice, which would need unsafe for no gain
    pub fn vector(&self, word: &str) -> Option<&'static [u8]> {
        let i = *self.index.get(word)? as usize;
        Some(&VOCAB_VEC[i * DIM..(i + 1) * DIM])
    }


    /// Mean unit vector of up to `max_tokens` content words in `text`,
    /// normalized. `None` when no word in it is one we have a vector for.
    //
    // used for both a query's keywords and a classical entry's gloss, which is
    // what lets a Latin word be scored in English vector space
    pub fn centroid_capped(&self, text: &str, max_tokens: usize) -> Option<[f32; DIM]> {
        let mut sum = [0.0f32; DIM];
        let mut n = 0;
        for token in text.split(|c: char| !c.is_ascii_alphabetic()) {
            if n >= max_tokens {
                break;
            }
            let token = token.to_ascii_lowercase();
            if token.len() < 3 || STOP.contains(&token.as_str()) {
                continue;
            }
            let Some(v) = self.vector(&token) else { continue };
            for (s, x) in sum.iter_mut().zip(v) {
                *s += *x as i8 as f32;
            }
            n += 1;
        }
        if n == 0 {
            return None;
        }
        let norm = sum.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm < 1e-6 {
            return None;
        }
        for s in &mut sum {
            *s /= norm;
        }
        Some(sum)
    }

    /// Relatedness of a concept to a query, `0..=1`.
    ///
    /// A cosine of [`RELATION_CEIL`] or better scores 1.0; anything at or below
    /// [`RELATION_FLOOR`] scores 0.0.
    pub fn similarity(a: &[f32; DIM], b: &[i8]) -> f32 {
        // b was scaled by 127 when quantized
        let cos: f32 = a.iter().zip(b).map(|(x, y)| x * (*y as f32)).sum::<f32>() / 127.0;
        ((cos - RELATION_FLOOR) / (RELATION_CEIL - RELATION_FLOOR)).clamp(0.0, 1.0)
    }

    /// Quantize a unit vector for storage.
    pub fn quantize(v: &[f32; DIM]) -> [i8; DIM] {
        let mut out = [0i8; DIM];
        for (o, x) in out.iter_mut().zip(v) {
            *o = (x * 127.0).round().clamp(-127.0, 127.0) as i8;
        }
        out
    }

    /// How pleasant `text` sounds, `0..=1`, with 0.5 meaning neutral.
    ///
    /// Blends the hand-scored lexicon with a projection onto [`PLEASANT_AXIS`],
    /// falling back to the projection alone for words the lexicon lacks.
    /// Centroid of a query's keywords, which are few and all meaningful.
    pub fn centroid(&self, text: &str) -> Option<[f32; DIM]> {
        self.centroid_capped(text, usize::MAX)
    }

    /// Centroid of one entry's gloss, capped to limit generic-English drift.
    pub fn gloss_centroid(&self, text: &str) -> Option<[f32; DIM]> {
        self.centroid_capped(text, MAX_GLOSS_TOKENS)
    }

    pub fn pleasantness(&self, text: &str, concept: Option<&[f32; DIM]>) -> f32 {
        let mut lex = 0.0;
        let mut n = 0;
        for token in text.split(|c: char| !c.is_ascii_alphabetic()) {
            let token = token.to_ascii_lowercase();
            if token.len() < 3 || STOP.contains(&token.as_str()) {
                continue;
            }
            if let Some(v) = self.valence.get(token.as_str()) {
                lex += *v;
                n += 1;
            }
        }

        let axis = concept
            .map(|c| c.iter().zip(&PLEASANT_AXIS).map(|(x, y)| x * y).sum::<f32>())
            .unwrap_or(0.0);

        let raw = if n > 0 {
            VADER_SHARE * (lex / n as f32) + (1.0 - VADER_SHARE) * axis
        } else {
            axis
        };
        // the axis spans roughly -0.5..0.5 in practice, so widen before
        // clamping or almost everything lands in the middle of the range
        (raw * 2.0 + 1.0).clamp(0.0, 2.0) * 0.5
    }
}
