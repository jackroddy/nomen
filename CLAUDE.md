# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`nomen` — *Nomen Offers Maybe-Excellent Names* — proposes software names that work as acronyms
for a set of keywords, ranked by a heuristic score. The name is its own output: `nomen` is Latin
for "name", it is in the corpus, and the tool generates that expansion itself with `recursive` on.

Letters no keyword accounts for are **gaps**, rendered as a dim letter (or `_` without color).
There are no filler words: an expansion is keywords and gaps, nothing else.

Names come from English, Latin, and Ancient Greek — one results tab each. A classical name's
acronym is its **folded ASCII** form (`νίκη` → `nike`); the original spelling and English gloss
ride along as an annotation.

## The keyword pattern

Keywords are written as a small pattern (`src/pattern.rs`). A pattern is a sequence of **blocks**,
and the sequence *is* the order they must appear in the name.

```text
pattern := term*
term    := mods? unit ('-' unit)*
unit    := '~'? group
group   := word | '(' word ('|' word)* ')'
word    := [A-Za-z]+
mods    := ('?' | '~')+          -- '?' scopes the term, '~' the first unit
```

| written | means |
|---|---|
| `source` | one block, a **hard requirement** |
| `a\|b`, `(a\|b\|c)` | either word, never both — one block, one letter |
| `?block` | the name may skip this block |
| `~block` | this block may supply any of its letters, not only its first |
| `a-b` | a and b land on consecutive letters, all or nothing |

`~?(source|code) (graph|map)-~(builder|viewer)` — an optional first block taking either word at
any letter, then a required chain of two.

Whitespace is optional wherever a bracket already separates blocks, so `(a|b)(c|d)` is two.
A parse error carries a byte offset, which the TUI turns into a column.

**Three global options died into this syntax**: keyword order (the pattern is the order), the
required/bonus distinction (`?`), and initials-only (`~`). Whether a keyword may give up an
interior letter is a property of *that keyword* — `~rust` is worth it, `~api` is not — so it was
never honestly a search-wide switch.

## Commands

```sh
cargo run          # the TUI, which is the only frontend
cargo test
cargo test <name>
cargo clippy --all-targets
```

## Architecture

Library crate (the engine) plus one thin frontend, the TUI in `src/bin/nomen/`. There was a CLI;
it was deleted once the pattern language moved the interesting part of a query into a text field
the TUI already owned, leaving the CLI a second parser and a second renderer for no gain.

**The library returns data, never formatted strings.** `generate()` hands back `Vec<Suggestion>`
carrying the name, expansion slots with provenance, and the full `Score` breakdown as numbers.
All formatting lives in `src/bin/nomen/theme.rs` and `ui.rs`. If the engine ever needs to know
how output is displayed, the split has leaked — and with only one frontend left, `tests/api.rs`
driving the public API alone is what actually holds the line.

Pipeline:

```
pattern::parse (frontend) → generate(): scan lexicon → prefilter → solve::solve (DP)
                                        → score → dedup → top-K → Suggestion
```

`generate()` does no I/O. crates.io availability is looked up separately by the caller
(`src/registry.rs`), so ranking stays deterministic and network-independent.

There are deliberately **no traits**. Each of the user-facing rules takes effect at a different
stage — candidate filtering, solver constraint, or scoring — so they cannot share an honest
interface. `Config` holds what applies to a whole search; the pattern holds what varies from
keyword to keyword. Variation points are enums.

### The solver (`src/solve.rs`)

`best[i][state]`: best score filling slots `0..i` having consumed the blocks in `state`. The
state is a bitmask over the pattern's blocks plus one high bit recording whether the path took
the self-reference — that bit is needed because two paths reaching the same block mask can
differ in whether they took it.

Two things make the pattern language nearly free here:

- **An alternation is one merged `GroupScores` table.** Only one word of a group may be used and
  the group fills exactly one letter, so taking the best score per letter across the
  alternatives leaves a group indistinguishable from a single keyword. Which word won is
  recovered at backtrack time from the `word[26]` array.
- **A `-` chain is one multi-slot move.** A block of *k* groups is placed over *k* consecutive
  letters in a single transition and costs one bit. Adjacency is structural rather than a
  constraint to check, so the state needs no memory of what filled the previous slot — and that
  is exactly why a chain is all-or-nothing rather than "adjacent if both are used".

Because a block can be wider than one letter, the gap count is **not** the popcount:
`gaps = len - Σ width(block) over the mask - self_ref_used`, summed at the terminal.

**Every component is exact**, so one pass finds the true optimum per name: `usage` is additive
per slot, `coverage` and the gap count are read off the state, and `name` does not depend on the
assignment. (Before gaps replaced filler words there was a second `polish` DP and a
rank-then-refine pipeline, because the filler-quality term was a mean over a slot count the path
had not yet fixed. Deleting fillers deleted all of that.)

### Semantic scoring (`src/semantics.rs`)

Two learned components, both backed by one vendored English vector table (GloVe 6B 50d, trimmed
to 47k words and quantized to `i8`, 2.2MB):

- **`relation`** — cosine between the query's steering centroid and the entry's concept vector.
  Query-dependent, recomputed per search. Steering is its own input (`Steering`): the pattern
  says what the name must *spell*, steering says what it should *mean*, and `inherit` folds the
  pattern's own words in — which is the default, and is exactly the old behaviour. An
  alternation contributes **all** of its words, not the one a given name took: making the
  centroid per-assignment would make `relation` depend on the solved assignment and break the
  single-pass DP.
- **`niceness`** — a blend of the VADER lexicon with a projection onto a good–bad axis built
  from seed words. The lexicon is more reliable per word but covers only 7k of the 47k
  vocabulary; the axis scores everything we have a vector for. Query-independent: baked into
  each entry at load, so it is a property of the word, not of the search.

Recursion is **not** a weight. It is a structural choice the caller already made with
`allow_recursion`, so there is nothing to trade off; `solve::SELF_REF_BONUS` is a fixed constant
sized to beat a gap without ever displacing a keyword that fits.

**A classical entry is represented by its English gloss**, which is why one English vector table
serves all three languages and no cross-lingual alignment is needed. That matters: aligned
vectors are not published for Latin or Ancient Greek at all.

### Things that will bite

- **Only the head word of a gloss is embedded (`MAX_GLOSS_TOKENS = 1`).** Averaging more words
  drags a vector toward generic English, which is close to *any* query — so longer glosses score
  higher against everything, systematically favouring glossed entries over plain English words.
  Measured: +0.037 mean bias at four tokens, +0.031 at two, +0.001 at one.
- **`relation` bounds are calibrated, not arbitrary.** Mapping raw cosine `-1..=1` onto `0..=1`
  squeezes every candidate into `0.58..0.78` and the component stops discriminating; a ceiling
  at the p90 mark makes the whole top of the ranking clip to 1.00.
- **Coverage must be added at `solve`'s terminal comparison.** It is not part of the per-slot
  gain, so leaving it out lets the search silently drop bonus keywords.
- **The pattern is strict by default: every block required, initials only, in order.** The three
  relaxations are `?`, `~` and writing the blocks in a different order. Taken names are still
  hidden by default (`^a`).
- **`~` is load-bearing, far more than order.** Measured on `source code graph`: all three blocks
  required and initials-only gives 43 names; adding `~` to each gives 14,146. Without it a
  keyword can only ever fill a slot matching its own first letter, so three keywords cover three
  letters and every longer name is mostly gaps.
- **Required-by-default is a real cliff, and the empty screen has to teach.** `App::empty_hint()`
  reads the pattern and names the relaxation that would help, rather than shrugging. This is
  the same finding as the `{rust, package, manager}` measurement below: strictness collapses the
  pool fast, and the fix is always one character.
- **`coverage` goes inert when no block is marked `?`.** It scores the share of *optional* blocks
  used, so with everything required it is a constant 1.0 and its weight only adds an offset. The
  weights panel says so.
- **`usage` is the only term that penalizes gaps.** A gap contributes exactly zero, so if you
  rebalance weights, lowering `usage` makes gappy acronyms win.
- **Coverage belongs in the score, not the filter.** Requiring every keyword collapses the
  candidate pool — `{rust, package, manager}` all required leaves 385 of 76k dictionary words,
  all of them like `scrump` and `rampire`. Only unmarked blocks constrain; `?` blocks are scored.
- **`~` breaks the initialism invariant.** When `rust` supplies its `u`, the expansion's initials
  no longer spell the name. The invariant is per slot: the word's letter at
  `ExpandedSlot::Block.letter` equals the name's letter at that position, and `tests/api.rs`
  checks it on every result.
- **Non-English entries have no frequency signal.** `band_pass` cannot rank them, so
  `classical_quality` stands in with three signals: whether Wiktionary records an English
  descendant of the lemma (35% of Latin, 13% of Greek), whether English borrowed the form
  outright (`lux`, `opus`, `nexus`), and whether it is a lemma rather than an inflection.
- **Boosting a lemma boosts all its inflections at once**, which fills a shortlist with
  `logos`/`logo`/`loge`. `max_per_lemma` caps that at display time, but it defaults to `None`:
  the forms are distinct acronyms with distinct availability, so the default shows all of them
  and the cap is one arrow key away in the options box.
- **Names are deduped by acronym, and `dedup_by` is not enough** — it drops only *consecutive*
  duplicates, and equal names need not sort adjacently once scores differ.
- **Name recognizability is a band-pass over frequency, not a low-pass.** The most frequent words
  are too generic to own (`able`, `reason`, `city`); words that get used as real names sit
  mid-corpus (`cargo` 3771, `forge` 6248, `muse` 8525 of 21558).

## The TUI (`src/bin/nomen/`)

`app.rs` holds every piece of state and every transition, and knows nothing about terminals —
that separation is what makes it testable. `ui.rs` only renders. Three worker threads
(`std::sync::mpsc`, no async runtime): search, snapshot, and single-name re-check.

- **Three boxes, and the arrows walk all of them.** `Focus` is one vertical chain — pattern line,
  steer line, options box, results list — and ↑↓ walk it end to end, crossing box borders like
  any other row. Running off the top of the results lands on the options box rather than
  stopping dead. ←→ move *within* a box (along the options, or through the language tabs); tab
  jumps whole boxes. There is no separate "which pane has the keys" concept any more.
- **The lit border is the only thing saying where the cursor landed**, which is why every box has
  one. `border_style()` reads `Focus::box_()`; an overlay dims all three, because the arrows are
  elsewhere.
- **Two input lines, one border.** The pattern line and the steering line. The border carries
  nothing but a parse error, and only while there is one — so a decorated border always means
  something is wrong, and the eye learns to ignore it otherwise.
- **Every option is on screen at once**, as a grid that reflows with the terminal width. This was
  a panel behind `^o`, which meant the state of the search was only visible while you were
  changing it. `[x]`/`[ ]` carry a switch's state without leaning on colour.
- **The grid's cell width is measured against the widest *possible* value, not the current one**
  (`VALUE_WIDTH`), so stepping max-gaps from `none` to `8` cannot reflow the grid under the
  cursor. `ui.rs` measures the terminal and tells `app.rs` the column count each frame, the same
  way `set_viewport` does — navigation has to agree with what was drawn and cannot measure a
  terminal itself.
- **↑↓ walk the grid's rows before leaving the box**, and from a short bottom row ↓ lands on the
  last cell rather than skipping the box: seven options over four columns leave a gap.
- **The ctrl shortcuts are no longer drawn beside the labels.** A `^n ` in front of every third
  cell broke the columns, for a hint the help overlay already carries. They still fire, and
  `Opt::key`/`Opt::from_key` keep the key and the option it flips declared together.
- **Enter flips a switch, or *picks up* a number.** A picked-up number takes ↑↓ instead of
  letting them leave the box (`App::adjusting`), and moving the cursor puts it down — a
  half-adjusted number that kept stealing ↑↓ would be a trap. The box has no room to explain
  itself, so the status line shows the hovered option's description; `Opt::example` went with the
  panel it was drawn in.
- **The language tabs are the results box's title.** They select which slice of the ranking it
  shows, so they belong to it — and that buys back the row they used to occupy.
- **The gloss column is measured, not fixed.** `measure_expansions` takes the width that covers
  95% of the glossed rows (`FIT`) and the renderer starts the gloss just past it; rows wider than
  that push their own gloss right rather than being cut short — the expansion is the answer, the
  gloss only annotates it. Measured **once per search**, not per frame: a column that moved as
  the list scrolled would be worse than one that never lined up. Only non-English rows are
  counted, since they are the only ones with anything to line up.
  Measured across four real queries the spread is tight — p50 to max is 2–7 columns, because the
  expansion is bounded by an 8-letter name — so the ragged tail is rare and the win is that the
  column tracks the query: 37 for `~data ~query`, 44 for a four-block pattern, against the 46 it
  was nailed to.
- **The headword is italic, and the language tag appears only on the `all` tab**: `[λόγος “that
  which is said”]` on a language tab, `[grc λόγος “…”]` on `all`. Anywhere else the tab has
  already said which corpus it came from.
- **Esc opens a menu — help, weights, exit — and everything modal hangs off it.** All four are
  floating boxes over the layout, so leaving one never rearranges what is underneath. `Overlay`
  is the whole modal state; `from_menu` records whether the thing on screen was reached from the
  menu or from a shortcut, so esc falls back to the right place rather than to a menu the user
  never opened.
- **Quitting takes three deliberate keys**: esc, enter on `exit`, enter on `[yes]`. `^q` opens
  the same dialog directly, so the short way still passes through the confirmation. The dialog
  arms `[yes]`, because reaching it took either a menu choice or a modifier — it is a last check,
  not the thing standing between a stray keypress and the exit.
- **`from_menu` is cleared when the last overlay closes.** It describes the chain that is open,
  and a stale flag would send esc back to a menu that is no longer there — which is exactly what
  `^q` after visiting and leaving the menu would have hit.
- **The pattern is parsed on the search debounce, not per keystroke.** A half-typed
  `(source|code` is `Typed::Invalid`: the last good results stay up and nothing flashes an error
  under the cursor while the closing bracket is still being typed.
- **`^n` toggles inherit, not `^i`.** In a terminal `^i` is indistinguishable from Tab, and Tab
  jumps boxes. Recursive got `^e` for the same reason: the mnemonic letters of "recursive" were
  either taken or already terminal control codes. `^w` reaches the weights without the menu;
  `^r` and `^f` act on data rather than opening anything, so they never belonged in the menu.
  `^q` is safe to bind because raw mode clears `IXON`, so the terminal's flow control never sees
  it.
- **Tabs are filtered views over one result vector.** Ranking is by absolute score, so filtering
  by language cannot reorder anything. Four `Vec<u32>` index lists instead of four result sets.
- **Only visible rows are built each frame.** A loose pattern runs to ~40k entries; materialising
  all of them per frame would be hopeless.
- **Replies carry the generation they were asked for.** Without it, a slow search lands on top
  of a newer one when you type quickly.
- **Esc opens the menu; from an overlay it backs out.** A stray keypress should never end a
  session mid-tune, which is why the exit sits two more keys past it.
- **A bare character key is always text**, so every command needs a modifier (`^a`, `^r`, `^f`)
  or a key of its own. `q` types the letter q. `App::type_char` returns whether anything was
  typed, which is how `main.rs` knows not to restart the search debounce when the cursor is not
  on an input line.
- **Every option carries a description and every weight a worked ASCII example**, shown while it
  is selected. A test asserts none is missing, that no example is too wide for the weights
  panel, and that no help row is too wide for the overlay's fixed width. The examples do the real work — a rule takes a moment to parse, a worked line takes
  none. The options panel is down to six: `keep order` and `initials only` are written into the
  pattern now.
- `top_k` is `usize::MAX` here — the TUI scrolls, so truncating would be pointless.
- **A weight change re-sorts; a structural option re-searches.** Weights only feed
  `Score::total`, so nudging one reuses the results in hand and feels immediate. Note the
  caveat: the set being re-sorted was already deduped by name and capped per lemma under the
  old weights, so which inflected form represents a lemma can lag until the next real search.
  Tuned weights persist to `~/.config/nomen/weights` and are saved on leaving the panel.

## Data

Roughly 81k entries: 21.5k English, 39.8k Latin, 19.8k Greek.

Three files, all `include_str!`d: `data/words.txt` (English, `word count`, frequency-ordered),
`data/latin.tsv` and `data/greek.tsv`
(`folded<TAB>display<TAB>lemma<TAB>gloss<TAB>has-english-descendant`).

**Inflections are kept on purpose.** `pie`/`pies` and `aquila`/`aquilae`/`aquilam` are different
acronyms supporting different keywords with different crates.io availability — throwing them
away throws away candidates. Each inflected form inherits its *lemma's* gloss, because its own
("accusative singular of aquila") names no concept to score against.

Latin and Greek come from Wiktionary via kaikki.org, harvested by a two-pass stream: collect
lemma glosses and `form_of` pointers, then join. Three filters that matter, each fixing an
observed defect: never let a name-only sense become a lemma's gloss (`aquila` was resolving to
"a male given name" instead of "eagle"); dedup on `(folded, lemma)`, since τέχνη and τέχνῃ both
fold to `tekhne`; and drop `pos == "name"` in the first pass — **but see below, because that one
is only half the story**. Greek uses the dump's romanization field with a transliteration
fallback.

**Proper nouns are dropped and then partly put back.** Dropping them wholesale removed 19% of
Latin, which is right for `Gottinga` and `Sicambri` and wrong for `juno`, `apollo`, `minerva`,
`ceres`, `flora` and `titan` — exactly the names people reach for. A second pass re-admits one
on either signal: English borrowed the form outright, or the lemma has an English descendant.
That is +2,126 Latin and +1,018 Greek out of ~11,000, and it drops `spqr`, `boeotia` and
`liguria`. The same two signals feed `classical_quality` as a *ranking* term, so admission and
ranking agree by construction. Two extraction fixes were needed to get there: a proper noun
reached by an "alternative spelling of X" pointer inherits X's gloss rather than being dropped
for having a grammatical one, and pointers resolve on the **folded** form — the Juno entry cites
`Iūnō` while the entry holding the gloss is spelled `Iuno`.

The English word list was
built by intersecting a subtitle frequency corpus with the lowercase entries of
`/usr/share/dict/words` — the corpus supplies ranking, the dictionary drops proper nouns and
misspellings. Do not use `/usr/share/dict/words` directly as a name source: it is unranked
Webster's 2nd, full of archaisms like `besoot` and `slepez`.

## Rebuilding the data (`tooling/`)

Everything in `data/` is generated, and `tooling/` is what generates it — the frequency
intersection, the kaikki two-pass extractor, the proper-noun gate, the GloVe trim. A clone
builds without any of it; you need it only to rebuild a corpus.

**The order is load-bearing and circular-looking.** English → classical (common nouns) →
vectors → classical annotation. The vector table is trimmed to cover the gloss vocabulary, so
it cannot be built before the glosses; the proper-noun gate asks whether English borrowed a
form, and that question is answered by `data/vocab.txt`. That is why the classical corpus is
built in two stages rather than one, and why `annotate_classical.py` exists separately.

`tooling/README.md` says which scripts have been re-run and diffed against the committed
output (`build_english.py` and the sentiment half of `build_semantics.py` reproduce theirs byte
for byte) and which have not, because they need 2 GB of dumps.

## crates.io availability (`src/snapshot.rs`)

Availability comes from a **bulk snapshot**, not per-name lookups. crates.io publishes a full
database dump; its 10th and 14th tar members — `data/crates.csv` and
`data/reserved_crate_names.csv`, paths *inside that archive*, not files in this repo — both sit
ahead of the multi-GB version tables, so streaming and stopping there costs ~356 MB of 1815 MB
and yields all ~327k names. Stored normalized at `~/.cache/nomen/taken.txt` (4.3 MB)
with a fetch timestamp; refreshed at 7 days, or on demand with `^f`.

This makes availability an O(1) local lookup, so it covers the entire ranking rather than the
rows on screen, and the `only available` filter is free instead of a deep search.

- **Store the full name list, not its intersection with our corpus.** The intersection is 60 KB
  but goes silently wrong the moment the corpus is rebuilt.
- **Reserved names matter.** 98 names (`std`, `core`, …) are unavailable while unpublished. A
  per-name 404 check reports them as free; the snapshot gets them right.
- **`crates.csv` needs a real CSV parser.** Descriptions carry embedded commas, quotes and
  newlines — the file is 26M lines for 327k crates, so splitting on `\n` or `,` is wrong.
- Names are folded before comparison: crates.io is case-insensitive and treats `-` and `_` as
  the same character, so `Foo-Bar` blocks `foo_bar`.

`src/registry.rs` keeps the per-name sparse-index lookup for one job only: an authoritative
re-check of a single name on demand. Its `~/.cache/nomen/crates.tsv` is append-only and is never
compacted, which is fine now that it grows by one line per `^r` rather than by hundreds per
search: entries older than the 7-day TTL are dropped when the file is read, and later lines
supersede earlier ones.

Two numbers worth holding onto, measured over five loose queries (136k candidates) against the
current snapshot: only **10.6%** of candidates are taken, but **41% of any top 20** is — the
ranking concentrates on exactly the names people already wanted. The gap is the whole reason the
availability column earns its place.

## Deferred

English proper nouns are still excluded: they fail the web2 lowercase-headword check, and the
personal names the frequency corpus carries (`oprah`, `nigel`, `cratchit`) are noise. The
classical route already supplies the evocative ones, so this is low value until it is not.

## Status

Everything described above is built and tested: 90 tests, `cargo clippy --all-targets` clean.

- Structural scoring, multilingual corpora, semantic scoring (`relation` + `niceness`).
- The keyword pattern language, and steering as a separate input.
- crates.io availability from a bulk snapshot. It deliberately does not affect ranking.
- One frontend, the TUI. The CLI was deleted once the pattern language moved the interesting
  part of a query into a text field the TUI already owned.
- Classical proper nouns, gated on recognizability. English proper nouns are not (see Deferred).
- `tooling/` rebuilds every generated file in `data/`.

Reference numbers, current as of the last measurement:

| | |
|---|---|
| corpus | 81,091 entries — 21,558 English, 39,777 Latin, 19,756 Greek |
| results, loose pattern | ~40k; a fully-required one can be 43 |
| search, debug build | ~12 ms strict, ~350 ms for three `~` blocks |
| search, release build | ~1 ms to ~26 ms |
| snapshot | 327k crate names, 4.3 MB cached |

There is no README and no LICENSE. `nomen` is unclaimed on crates.io.
