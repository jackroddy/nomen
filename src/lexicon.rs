//! The candidate name list.

use crate::semantics::{DIM, Semantics};

/// An index into the [`Lexicon`]'s word table.
pub type WordId = u32;

/// A list of words stored as one blob with an offset table.
//
// a String per word would be ~15k separate allocations; this is one
// allocation and every accessor borrows out of it
struct WordTable {
    blob: String,
    /// Byte offsets into `blob`, length `count + 1`. Word `i` spans
    /// `offsets[i]..offsets[i + 1]`.
    offsets: Vec<u32>,
}

impl WordTable {
    fn build(words: impl Iterator<Item = &'static str>) -> WordTable {
        let mut blob = String::new();
        let mut offsets = vec![0];
        for w in words {
            blob.push_str(w);
            offsets.push(blob.len() as u32);
        }
        WordTable { blob, offsets }
    }

    fn get(&self, id: WordId) -> &str {
        let i = id as usize;
        &self.blob[self.offsets[i] as usize..self.offsets[i + 1] as usize]
    }

    fn push(&mut self, word: &str) {
        self.blob.push_str(word);
        self.offsets.push(self.blob.len() as u32);
    }

    fn len(&self) -> usize {
        self.offsets.len() - 1
    }
}

/// Which language an entry comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    English,
    Latin,
    Greek,
}

impl Lang {
    pub fn tag(self) -> &'static str {
        match self {
            Lang::English => "en",
            Lang::Latin => "la",
            Lang::Greek => "grc",
        }
    }
}

/// The names a search draws on.
///
/// An entry's *name* is always folded ASCII — that is the acronym, and what
/// crates.io is asked about. Non-English entries additionally carry their
/// original spelling and an English gloss.
pub struct Lexicon {
    /// Folded ASCII, used for every letter comparison.
    names: WordTable,
    /// Original orthography: `aquila`, `νίκη`. Equal to the name for English.
    display: WordTable,
    /// English gloss. Empty for English entries, which are their own gloss.
    gloss: WordTable,
    /// Dictionary form this entry inflects. Its own name, for English.
    lemma: WordTable,
    lang: Vec<Lang>,
    /// Recognizability of each name, 0..=1.
    name_quality: Vec<f32>,

    /// Quantized concept vector per entry, `DIM` values each.
    //
    // an English entry is its own concept; a classical one is represented by
    // its gloss, which is what lets a single English vector table score every
    // language. Entries with no usable vector have a zero row and are flagged
    // in `has_concept`.
    concept: Vec<i8>,
    has_concept: Vec<bool>,
    semantics: Option<Semantics>,
    /// How pleasant each entry sounds, 0..=1, 0.5 being neutral.
    pleasantness: Vec<f32>,
}

/// Latin and Ancient Greek, harvested from Wiktionary. Tab-separated:
/// folded ASCII, original spelling, lemma, English gloss, and `1` if the lemma
/// has an English descendant.
//
// inflected forms are kept deliberately -- `aquila`, `aquilae`, `aquilam` are
// three different acronyms supporting different keywords -- and each inherits
// its lemma's gloss, because its own ("accusative singular of aquila") names
// no concept to score against.
const LATIN: &str = include_str!("../data/latin.tsv");
const GREEK: &str = include_str!("../data/greek.tsv");

/// The frequency-ranked name list, embedded at compile time.
//
// format: "word count", one per line, most frequent first. built by
// intersecting a subtitle frequency corpus with the lowercase entries of
// /usr/share/dict/words -- the corpus supplies ranking, the dictionary drops
// proper nouns (which web2 lists capitalized) and misspellings.
const WORDS: &str = include_str!("../data/words.txt");

impl Lexicon {
    /// Load every word list that ships with the crate.
    pub fn embedded() -> Lexicon {
        let mut lex = Lexicon::parse(WORDS);
        lex.add_glossed(LATIN, Lang::Latin);
        lex.add_glossed(GREEK, Lang::Greek);
        lex.attach_semantics(Semantics::embedded());
        lex
    }

    /// Compute each entry's concept vector and pleasantness, and refine the
    /// recognizability of classical entries now that the English vocabulary
    /// is available.
    ///
    /// Without this a lexicon still works; the learned scores stay neutral.
    pub fn attach_semantics(&mut self, sem: Semantics) {
        let n = self.names.len();
        self.concept = vec![0i8; n * DIM];
        self.has_concept = vec![false; n];
        self.pleasantness = vec![0.5; n];

        for id in 0..n {
            let idx = id as WordId;
            // English words stand for themselves; everything else is its gloss
            let text = if self.gloss.get(idx).is_empty() {
                self.names.get(idx)
            } else {
                self.gloss.get(idx)
            };
            let vec = sem.gloss_centroid(text);
            if let Some(v) = &vec {
                self.concept[id * DIM..(id + 1) * DIM].copy_from_slice(&Semantics::quantize(v));
                self.has_concept[id] = true;
            }
            self.pleasantness[id] = sem.pleasantness(text, vec.as_ref());

            // a classical form English has borrowed outright is one a reader
            // recognizes, whatever its etymology tree says
            if self.lang[id] != Lang::English && sem.knows(self.names.get(idx)) {
                self.name_quality[id] = (self.name_quality[id] + 0.15).min(1.0);
            }
        }
        self.semantics = Some(sem);
    }

    /// The vector table, when one has been attached.
    pub fn semantics(&self) -> Option<&Semantics> {
        self.semantics.as_ref()
    }

    /// Build a lexicon from word-list text, for tests and custom vocabularies.
    ///
    /// Expects `"word count"` per line, most frequent first. Lines starting
    /// with `#` are ignored.
    pub fn parse(words: &'static str) -> Lexicon {
        let name_words: Vec<&str> = words
            .lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .filter_map(|l| l.split_whitespace().next())
            .collect();

        let n = name_words.len().max(1) as f32;
        let name_quality = (0..name_words.len()).map(|i| band_pass(i as f32 / n)).collect();

        let n_entries = name_words.len();
        Lexicon {
            concept: Vec::new(),
            has_concept: vec![false; n_entries],
            semantics: None,
            pleasantness: vec![0.5; n_entries],
            names: WordTable::build(name_words.iter().copied()),
            display: WordTable::build(name_words.iter().copied()),
            gloss: WordTable::build(std::iter::repeat_n("", n_entries)),
            lemma: WordTable::build(name_words.into_iter()),
            lang: vec![Lang::English; n_entries],
            name_quality,
        }
    }

    /// Append a tab-separated glossed word list for one language.
    pub fn add_glossed(&mut self, text: &'static str, lang: Lang) {
        for line in text.lines() {
            let mut f = line.split('\t');
            let (Some(name), Some(display), Some(lemma), Some(gloss)) =
                (f.next(), f.next(), f.next(), f.next())
            else {
                continue;
            };
            let has_descendant = f.next() == Some("1");
            self.names.push(name);
            self.display.push(display);
            self.gloss.push(gloss);
            self.lemma.push(lemma);
            self.lang.push(lang);
            self.name_quality.push(classical_quality(display == lemma, has_descendant, false));
            self.has_concept.push(false);
            self.pleasantness.push(0.5);
        }
    }

    pub fn name_count(&self) -> usize {
        self.names.len()
    }

    pub fn name(&self, id: WordId) -> &str {
        self.names.get(id)
    }

    pub fn name_quality(&self, id: WordId) -> f32 {
        self.name_quality[id as usize]
    }

    /// The entry's original spelling: `aquila`, `νίκη`.
    pub fn display(&self, id: WordId) -> &str {
        self.display.get(id)
    }

    /// The entry's English gloss, empty for English entries.
    pub fn gloss(&self, id: WordId) -> &str {
        self.gloss.get(id)
    }

    /// The dictionary form this entry inflects.
    pub fn lemma(&self, id: WordId) -> &str {
        self.lemma.get(id)
    }

    pub fn lang(&self, id: WordId) -> Lang {
        self.lang[id as usize]
    }

    /// The entry's concept vector, if one could be built.
    pub fn concept(&self, id: WordId) -> Option<&[i8]> {
        let i = id as usize;
        self.has_concept[i].then(|| &self.concept[i * DIM..(i + 1) * DIM])
    }

    pub fn pleasantness(&self, id: WordId) -> f32 {
        self.pleasantness[id as usize]
    }
}

/// Recognizability as a function of position in the frequency list, `0..=1`.
//
// what: a band-pass over frequency rank, not "more frequent is better".
// why: both tails are bad names. The most frequent words are too generic to
//      own -- at rank <400 the 4+ letter entries are `able`, `reason`,
//      `trouble`, `city` -- while past the 13k mark nobody recognizes the
//      word. Words that actually get used as software names sit in the middle:
//      cargo 3771, forge 6248, muse 8525, atlas 9954, prism 13499 of 21558,
//      i.e. p in 0.18..0.63. PEAK and WIDTH are set to cover that band, and
//      still do: those five score 0.52..1.00 here while `able` (rank 412)
//      scores 0.16.
fn band_pass(p: f32) -> f32 {
    const PEAK: f32 = 0.40;
    const WIDTH: f32 = 0.28;
    let z = (p - PEAK) / WIDTH;
    (-z * z).exp()
}

/// Recognizability of a Latin or Greek entry.
//
// there is no frequency corpus for these languages, so the band-pass that
// ranks English cannot be applied. Three signals stand in for it, each worth
// what it says about whether a reader has plausibly met the word:
//
//   descendant  Wiktionary records an English word derived from this lemma.
//               The strongest of the three, and it separates cleanly:
//               aquila/ferrum/opus/nexus/forma/pons all have one, while
//               sapo ("an ancient hair product"), bacar ("a kind of wine
//               glass") and istac ("this same") do not. 35% of Latin, 13%
//               of Greek.
//   borrowed    the folded form is itself a token English uses -- lux, opus,
//               nexus. Overlaps with `descendant` but catches what the
//               etymology trees miss.
//   lemma       a dictionary form rather than an inflection: aquila, not
//               aquilis.
fn classical_quality(is_lemma: bool, has_descendant: bool, borrowed: bool) -> f32 {
    let mut q: f32 = 0.35;
    if has_descendant {
        q += 0.25;
    }
    if borrowed {
        q += 0.15;
    }
    if is_lemma {
        q += 0.15;
    }
    q
}

/// Map an ASCII letter to `0..26`, or `None` if it is not `a`-`z`.
pub(crate) fn letter_index(byte: u8) -> Option<usize> {
    byte.is_ascii_lowercase().then(|| (byte - b'a') as usize)
}
