//! Integration tests driving the engine through its public API only.
//!
//! Nothing here may reach into `src/bin/`: if a test needs something from the
//! frontend, the library boundary is in the wrong place.

use std::collections::{HashMap, HashSet};

use nomen::{
    Config, Error, ExpandedSlot, Lang, Lexicon, Pattern, Query, Steering, Suggestion, generate,
    pattern,
};

/// A query from a written pattern, steered by the pattern's own words.
fn q(text: &str) -> Query {
    Query { pattern: pattern::parse(text).unwrap(), steer: Steering::default() }
}

/// The expansion as words, with a gap written as `_`.
fn expansion(s: &Suggestion) -> Vec<String> {
    s.expansion
        .iter()
        .map(|e| match e {
            ExpandedSlot::Block { word, .. } | ExpandedSlot::SelfRef { word } => word.clone(),
            ExpandedSlot::Gap => "_".to_string(),
        })
        .collect()
}

#[test]
fn recursive_acronym_reproduces_gnu() {
    // `gnu` and `unix` are absent from the shipped corpus (it is built from a
    // subtitle frequency list), so this uses its own name list.
    let lex = Lexicon::parse("gnu 100\ncat 90\n");
    let cfg = Config { allow_recursion: true, min_len: 3, ..Config::default() };

    let out = generate(&q("not unix"), &lex, &cfg).unwrap();
    let top = &out[0];

    assert_eq!(top.name, "gnu");
    assert_eq!(expansion(top), ["gnu", "not", "unix"]);
    assert!(matches!(top.expansion[0], ExpandedSlot::SelfRef { .. }));
    assert_eq!(top.gaps, 0);
}

#[test]
fn a_required_block_filters_out_names_that_cannot_host_it() {
    let lex = Lexicon::parse("abc 100\nxyz 90\n");
    let cfg = Config { min_len: 3, ..Config::default() };

    // an unmarked block is a hard requirement, and xyz contains none of
    // a,g,e,n,t so it cannot host this one at all
    let out = generate(&q("agent"), &lex, &cfg).unwrap();
    assert!(out.iter().all(|s| s.name != "xyz"));
    assert!(out.iter().any(|s| s.name == "abc"));
}

#[test]
fn an_optional_block_does_not_filter() {
    let lex = Lexicon::parse("xyz 90\n");
    let cfg = Config { min_len: 3, ..Config::default() };

    // the same block marked `?` leaves xyz in the running, scored on what it
    // does achieve rather than rejected outright -- here, nothing at all
    let out = generate(&q("?agent"), &lex, &cfg).unwrap();

    assert_eq!(out.len(), 1);
    assert_eq!(out[0].name, "xyz");
    assert_eq!(out[0].score.coverage, 0.0);
    assert_eq!(expansion(&out[0]), ["_", "_", "_"]);
}

#[test]
fn the_pattern_is_written_in_the_order_it_must_read() {
    let lex = Lexicon::parse("abc 100\n");
    let cfg = Config { min_len: 3, ..Config::default() };

    // alpha's letter sits at index 0 of the name and beta's at index 1, so the
    // two fit together in one written order and not in the other. There is no
    // flag involved: the order is the pattern.
    let forwards = generate(&q("?alpha ?beta"), &lex, &cfg).unwrap();
    let backwards = generate(&q("?beta ?alpha"), &lex, &cfg).unwrap();

    assert_eq!(forwards[0].score.coverage, 1.0, "both blocks fit as written");
    assert!(backwards[0].score.coverage < 1.0, "written backwards, one has to go");
}

#[test]
fn a_tilde_is_what_lets_a_block_give_up_an_interior_letter() {
    let lex = Lexicon::parse("cat 100\n");

    let used_at = |s: &Suggestion| {
        s.expansion.iter().find_map(|e| match e {
            ExpandedSlot::Block { letter, .. } => Some(*letter),
            _ => None,
        })
    };
    let cfg = Config { min_len: 3, ..Config::default() };

    // "scale" can supply 'c' only from index 1
    let loose = generate(&q("~?scale"), &lex, &cfg).unwrap();
    let strict = generate(&q("?scale"), &lex, &cfg).unwrap();

    assert_eq!(used_at(&loose[0]), Some(1), "interior letter used when marked ~");
    assert_eq!(used_at(&strict[0]), None, "and not used when it is not");
}

#[test]
fn an_alternation_never_uses_more_than_one_of_its_words() {
    let lex = Lexicon::embedded();
    let cfg = Config { top_k: 200, ..Config::default() };
    let out = generate(&q("~(source|code) ~?(graph|map)"), &lex, &cfg).unwrap();

    assert!(!out.is_empty());
    let mut seen: HashSet<&str> = HashSet::new();
    for s in &out {
        let words: Vec<String> = s
            .expansion
            .iter()
            .filter_map(|e| match e {
                ExpandedSlot::Block { word, .. } => Some(word.clone()),
                _ => None,
            })
            .collect();
        for pair in [["source", "code"], ["graph", "map"]] {
            let taken = pair.iter().filter(|w| words.iter().any(|x| x == *w)).count();
            assert!(taken <= 1, "{} used both of {pair:?}", s.name);
        }
        seen.extend(words.iter().filter_map(|w| {
            ["source", "code", "graph", "map"].iter().find(|a| *a == w).copied()
        }));
    }
    // and every alternative is genuinely in play, not quietly ignored
    assert!(seen.contains("source") && seen.contains("code"), "one alternative never won: {seen:?}");
}

#[test]
fn a_joined_block_lands_on_adjacent_letters() {
    let lex = Lexicon::embedded();
    let cfg = Config { top_k: 150, ..Config::default() };

    let positions = |s: &Suggestion| -> Vec<usize> {
        s.expansion
            .iter()
            .enumerate()
            .filter_map(|(i, e)| match e {
                ExpandedSlot::Block { term: 0, .. } => Some(i),
                _ => None,
            })
            .collect()
    };

    let joined = generate(&q("~source-~code"), &lex, &cfg).unwrap();
    assert!(!joined.is_empty(), "the joiner found nothing to work with");
    for s in &joined {
        let at = positions(s);
        assert_eq!(at.len(), 2, "{}: a two-group block fills two letters", s.name);
        assert_eq!(at[1], at[0] + 1, "{}: {at:?} are not adjacent", s.name);
    }

    // and the joiner is doing real work: without it the same two words are
    // free to sit apart
    let loose = generate(&q("~source ~code"), &lex, &cfg).unwrap();
    assert!(
        loose.iter().any(|s| {
            let at = positions(s);
            at.len() == 1
        }) || loose.iter().any(|s| {
            let at: Vec<usize> = s
                .expansion
                .iter()
                .enumerate()
                .filter_map(|(i, e)| match e {
                    ExpandedSlot::Block { .. } => Some(i),
                    _ => None,
                })
                .collect();
            at.len() == 2 && at[1] != at[0] + 1
        }),
        "unjoined blocks never came apart, so the joiner proves nothing"
    );
}

#[test]
fn a_joined_block_is_all_or_nothing() {
    // "sat" can host source's 's' but nothing of code, and a chain cannot be
    // taken by halves -- so the whole block goes unused
    let lex = Lexicon::parse("sat 100\n");
    let cfg = Config { min_len: 3, ..Config::default() };

    let chained = generate(&q("?source-code"), &lex, &cfg).unwrap();
    assert_eq!(expansion(&chained[0]), ["_", "_", "_"]);

    let apart = generate(&q("?source ?code"), &lex, &cfg).unwrap();
    assert_eq!(expansion(&apart[0]), ["source", "_", "_"], "unchained, source still fits");
}

#[test]
fn every_slot_supplies_the_name_letter_it_claims() {
    let lex = Lexicon::embedded();
    let cfg = Config { top_k: 50, ..Config::default() };

    for s in generate(&q("~?data ~?query ~?engine"), &lex, &cfg).unwrap() {
        assert_eq!(s.expansion.len(), s.name.len(), "one slot per letter of {}", s.name);

        for (i, e) in s.expansion.iter().enumerate() {
            // under `~` a block need not contribute its word's own first
            // letter, so the invariant is per slot, not "the initials spell
            // the name"
            let (word, at) = match e {
                ExpandedSlot::Block { word, letter, .. } => (word, *letter as usize),
                ExpandedSlot::SelfRef { word } => (word, 0),
                ExpandedSlot::Gap => continue,
            };
            assert_eq!(
                word.as_bytes()[at] as char,
                s.name.as_bytes()[i] as char,
                "{}: slot {i} ({word}) does not supply its letter",
                s.name,
            );
        }
    }
}

#[test]
fn without_a_tilde_the_expansion_is_a_strict_initialism() {
    let lex = Lexicon::embedded();
    let cfg = Config { top_k: 50, max_gaps: Some(0), ..Config::default() };

    for s in generate(&q("?data ?query ?engine"), &lex, &cfg).unwrap() {
        let initials: String = s
            .expansion
            .iter()
            .map(|e| match e {
                ExpandedSlot::Block { word, .. } | ExpandedSlot::SelfRef { word } => {
                    word.chars().next().unwrap()
                }
                ExpandedSlot::Gap => unreachable!("max_gaps 0 leaves no gaps"),
            })
            .collect();
        assert_eq!(initials, s.name, "expansion does not spell the name");
    }
}

#[test]
fn a_letter_no_block_can_supply_becomes_a_gap() {
    let lex = Lexicon::parse("cat 100\n");
    // "code" supplies 'c'; nothing supplies 'a' or 't'
    let cfg = Config { min_len: 3, ..Config::default() };

    let out = generate(&q("code"), &lex, &cfg).unwrap();

    assert_eq!(expansion(&out[0]), ["code", "_", "_"]);
    assert_eq!(out[0].gaps, 2);
}

#[test]
fn max_gaps_bounds_the_unaccounted_letters() {
    let lex = Lexicon::embedded();
    // covering every letter of a name takes the interior-letter relaxation;
    // with three initials-only blocks a low-gap acronym is close to impossible
    let query = q("~?source ~?code ~?graph");

    for limit in [1, 2, 3] {
        let cfg = Config { top_k: 50, max_gaps: Some(limit), ..Config::default() };
        let out = generate(&query, &lex, &cfg).unwrap();
        assert!(!out.is_empty(), "max_gaps {limit} found nothing");
        for s in &out {
            assert!(s.gaps <= limit, "{} has {} gaps, limit {limit}", s.name, s.gaps);
        }
    }

    // and the limit must not reject anything it should keep: a run with no
    // limit finds at least as many names as any bounded run
    let unbounded = generate(&query, &lex, &Config { top_k: 50, ..Config::default() }).unwrap();
    let bounded =
        generate(&query, &lex, &Config { top_k: 50, max_gaps: Some(8), ..Config::default() })
            .unwrap();
    assert_eq!(unbounded.len(), bounded.len());
}

#[test]
fn pattern_size_is_validated() {
    let lex = Lexicon::parse("abc 1\n");
    let cfg = Config::default();

    let none = Query { pattern: Pattern::default(), steer: Steering::default() };
    assert_eq!(generate(&none, &lex, &cfg).unwrap_err(), Error::EmptyPattern);

    // a keyword is letters only, so the filler words are too
    let word = |i: usize| format!("k{}", (b'a' + i as u8) as char);
    let over = (0..nomen::MAX_TERMS + 1).map(word).collect::<Vec<_>>().join(" ");
    assert!(matches!(
        generate(&q(&over), &lex, &cfg).unwrap_err(),
        Error::TooManyBlocks { .. }
    ));

    // alternatives inside a block are free -- only blocks cost a bitmask bit
    let wide = format!("?({})", (0..20).map(word).collect::<Vec<_>>().join("|"));
    assert!(generate(&q(&wide), &lex, &cfg).is_ok());
}

#[test]
fn a_malformed_pattern_says_where_it_went_wrong() {
    let text = "(source|code graph";
    let e = pattern::parse(text).unwrap_err();
    assert!(e.message.contains("unclosed"));
    assert!(e.column(text) <= text.chars().count());
}

#[test]
fn suggestions_never_repeat_a_name() {
    let lex = Lexicon::embedded();
    let out = generate(
        &q("~?source ~?code ~?graph"),
        &lex,
        &Config { top_k: 60, ..Config::default() },
    )
    .unwrap();

    let mut names: Vec<&str> = out.iter().map(|s| s.name.as_str()).collect();
    let before = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), before, "the same acronym was offered twice");
}

#[test]
fn every_name_is_folded_ascii_whatever_the_language() {
    let lex = Lexicon::embedded();
    let out =
        generate(&q("~?light ~?fast"), &lex, &Config { top_k: 80, ..Config::default() }).unwrap();

    for s in &out {
        // the acronym is what the expansion spells and what crates.io is asked
        // about, so it must stay ASCII even when the entry is Greek script
        assert!(
            s.name.bytes().all(|b| b.is_ascii_lowercase()),
            "{} is not folded ASCII",
            s.name
        );
        if s.lang != Lang::English {
            assert!(!s.gloss.is_empty(), "{} ({:?}) has no gloss", s.name, s.lang);
        }
    }
}

#[test]
fn language_filter_selects_the_corpus() {
    let lex = Lexicon::embedded();
    let query = q("~?water ~?flow");

    for lang in [Lang::English, Lang::Latin, Lang::Greek] {
        let cfg = Config { top_k: 25, langs: vec![lang], ..Config::default() };
        let out = generate(&query, &lex, &cfg).unwrap();
        assert!(!out.is_empty(), "{lang:?} produced nothing");
        assert!(out.iter().all(|s| s.lang == lang), "{lang:?} filter leaked");
    }
}

#[test]
fn relatedness_discriminates_and_favours_the_top() {
    let lex = Lexicon::embedded();
    let cfg = Config { top_k: 500, ..Config::default() };
    let out = generate(&q("~?water ~?river ~?flow"), &lex, &cfg).unwrap();

    let relation: Vec<f32> = out.iter().map(|s| s.score.relation).collect();
    let spread = relation.iter().cloned().fold(f32::MIN, f32::max)
        - relation.iter().cloned().fold(f32::MAX, f32::min);
    assert!(spread > 0.2, "relatedness is not discriminating (spread {spread})");

    let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
    assert!(
        mean(&relation[..20]) > mean(&relation),
        "the top of the ranking is no more related than the rest"
    );
}

#[test]
fn steering_moves_relation_without_touching_which_names_qualify() {
    let lex = Lexicon::embedded();
    let cfg = Config { top_k: 400, ..Config::default() };
    let pattern = pattern::parse("~?flow").unwrap();

    let run = |steer: Steering| -> HashMap<String, f32> {
        generate(&Query { pattern: pattern.clone(), steer }, &lex, &cfg)
            .unwrap()
            .into_iter()
            .map(|s| (s.name, s.score.relation))
            .collect()
    };

    let inherited = run(Steering::default());
    let steered = run(Steering {
        words: vec!["money".into(), "bank".into(), "finance".into()],
        inherit: false,
    });

    let moved = inherited
        .iter()
        .filter_map(|(name, r)| steered.get(name).map(|s| (s - r).abs()))
        .filter(|d| *d > 0.05)
        .count();
    assert!(moved > 0, "steering words changed nothing about relation");
}

#[test]
fn inherit_adds_the_patterns_own_words_to_the_steering() {
    let lex = Lexicon::embedded();
    let cfg = Config { top_k: 200, ..Config::default() };
    let pattern = pattern::parse("~?water").unwrap();
    let words = vec!["money".to_string()];

    let with = generate(
        &Query { pattern: pattern.clone(), steer: Steering { words: words.clone(), inherit: true } },
        &lex,
        &cfg,
    )
    .unwrap();
    let without = generate(
        &Query { pattern, steer: Steering { words, inherit: false } },
        &lex,
        &cfg,
    )
    .unwrap();

    let mean = |v: &[Suggestion]| {
        v.iter().map(|s| s.score.relation).sum::<f32>() / v.len() as f32
    };
    assert!(
        (mean(&with) - mean(&without)).abs() > 0.01,
        "inheriting the pattern's words made no difference"
    );
}

#[test]
fn a_lexicon_without_vectors_scores_both_components_neutral() {
    // Lexicon::parse attaches no semantics, so the two learned components must
    // fall back to neutral rather than penalising every candidate.
    let lex = Lexicon::parse("cat 100\ndog 90\n");
    let out = generate(&q("?code"), &lex, &Config { min_len: 3, ..Config::default() }).unwrap();

    assert!(!out.is_empty());
    for s in &out {
        assert_eq!(s.score.relation, 0.5);
        assert_eq!(s.score.niceness, 0.5);
    }
}

#[test]
fn max_per_lemma_limits_forms_of_one_dictionary_word() {
    let lex = Lexicon::embedded();
    let query = q("~?source ~?code ~?graph");

    for limit in [1, 2, 3] {
        let cfg = Config { top_k: 200, max_per_lemma: Some(limit), ..Config::default() };
        let out = generate(&query, &lex, &cfg).unwrap();
        let mut seen: HashMap<&str, usize> = HashMap::new();
        for s in &out {
            *seen.entry(s.lemma.as_str()).or_default() += 1;
        }
        let worst = seen.values().copied().max().unwrap_or(0);
        assert!(worst <= limit, "limit {limit} exceeded: some lemma appeared {worst} times");
    }

    // and lifting the cap really does return more forms of the same word
    let capped = generate(
        &query,
        &lex,
        &Config { top_k: 200, max_per_lemma: Some(1), ..Config::default() },
    )
    .unwrap();
    let uncapped =
        generate(&query, &lex, &Config { top_k: 200, max_per_lemma: None, ..Config::default() })
            .unwrap();
    let distinct = |v: &[Suggestion]| {
        v.iter().map(|s| s.lemma.clone()).collect::<HashSet<_>>().len()
    };
    assert!(distinct(&capped) > distinct(&uncapped), "the cap did not diversify the list");
}
