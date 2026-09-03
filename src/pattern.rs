//! The keyword pattern: what the acronym must spell out, and how loosely.
//!
//! A pattern is a sequence of **blocks**, and the sequence is the order the
//! blocks appear in the name. Every block is a hard requirement unless it is
//! marked `?`.
//!
//! ```text
//! pattern := term*
//! term    := mods? unit ('-' unit)*
//! unit    := '~'? group
//! group   := word | '(' word ('|' word)* ')'
//! word    := [A-Za-z]+
//! mods    := ('?' | '~')+          -- '?' scopes the term, '~' the first unit
//! ```
//!
//! | written | means |
//! |---|---|
//! | `source` | one required block |
//! | `a\|b` | either word, never both — one block, one letter |
//! | `?block` | the name may skip this block |
//! | `~block` | this block may supply any of its letters, not only its first |
//! | `a-b` | a and b land on consecutive letters, all or nothing |
//!
//! Whitespace between blocks is optional wherever a bracket already separates
//! them, so `(a|b)(c|d)` is two blocks.

use std::fmt;

/// One set of alternatives filling a single letter of the name.
///
/// A group with several words consumes exactly one letter: the alternatives
/// are choices, not a sequence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub words: Vec<String>,
    /// `~`: this group may supply any of its letters, not only its first.
    pub interior: bool,
}

/// One block of the pattern.
///
/// `groups` holds the `-` chain — several groups landing on consecutive
/// letters. A block is taken whole or not at all, which is what lets the
/// solver place it as a single move.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Term {
    pub groups: Vec<Group>,
    /// `?`: the name may skip this block. Unmarked blocks are required.
    pub optional: bool,
}

impl Term {
    /// How many letters of the name this block occupies.
    pub fn width(&self) -> usize {
        self.groups.len()
    }
}

/// A parsed keyword pattern.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pattern {
    pub terms: Vec<Term>,
}

impl Pattern {
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// Every word in the pattern, alternatives included.
    //
    // used to steer `relation` when the steering bar inherits: all of the
    // alternatives are on topic, whichever one a given name happens to take
    pub fn words(&self) -> impl Iterator<Item = &str> {
        self.terms
            .iter()
            .flat_map(|t| t.groups.iter())
            .flat_map(|g| g.words.iter())
            .map(String::as_str)
    }
}

/// Where a pattern stopped making sense, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    /// Byte offset into the pattern. Always a character boundary: the scanner
    /// only ever steps over bytes it has already recognised as ASCII.
    pub at: usize,
}

impl ParseError {
    /// Which column to point a caret at, counting characters rather than bytes.
    pub fn column(&self, text: &str) -> usize {
        text[..self.at.min(text.len())].chars().count()
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ParseError {}

/// Read a pattern. An empty string is an empty pattern, not an error — it is
/// what the input bar holds before anything is typed.
pub fn parse(text: &str) -> Result<Pattern, ParseError> {
    let mut p = Scanner { s: text.as_bytes(), i: 0 };
    let mut terms = Vec::new();
    p.space();
    while !p.done() {
        if p.peek() == Some(b')') {
            return p.fail("unmatched ')'");
        }
        terms.push(p.term()?);
        p.space();
    }
    Ok(Pattern { terms })
}

struct Scanner<'a> {
    s: &'a [u8],
    i: usize,
}

impl Scanner<'_> {
    fn done(&self) -> bool {
        self.i >= self.s.len()
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn eat(&mut self, b: u8) -> bool {
        if self.peek() == Some(b) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn space(&mut self) {
        while matches!(self.peek(), Some(b) if b.is_ascii_whitespace()) {
            self.i += 1;
        }
    }

    fn fail<T>(&self, message: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError { message: message.into(), at: self.i })
    }

    fn term(&mut self) -> Result<Term, ParseError> {
        let (optional, interior) = self.mods(true)?;
        let mut groups = vec![self.group(interior, "")?];
        while self.eat(b'-') {
            let (_, interior) = self.mods(false)?;
            groups.push(self.group(interior, "-")?);
        }
        Ok(Term { groups, optional })
    }

    /// Read the `?` and `~` markers in front of a group.
    //
    // `~` belongs to the group it precedes, but `?` marks a whole block, so it
    // is only legal on the block's first group -- `a-?b` would be asking for
    // half a chain, and the chain is indivisible by design
    fn mods(&mut self, first: bool) -> Result<(bool, bool), ParseError> {
        let (mut optional, mut interior) = (false, false);
        loop {
            match self.peek() {
                Some(b'~') => interior = true,
                Some(b'?') if first => optional = true,
                Some(b'?') => {
                    return self
                        .fail("'?' marks a whole block; write it before the block's first word");
                }
                _ => return Ok((optional, interior)),
            }
            self.i += 1;
        }
    }

    /// `after` names the separator just consumed, so an alternation or a chain
    /// left dangling says so rather than reporting a missing word.
    fn group(&mut self, interior: bool, after: &str) -> Result<Group, ParseError> {
        let bracketed = self.eat(b'(');
        let mut words = vec![self.word(after)?];
        while self.eat(b'|') {
            words.push(self.word("|")?);
        }
        if bracketed && !self.eat(b')') {
            return self.fail("unclosed '('");
        }
        Ok(Group { words, interior })
    }

    fn word(&mut self, after: &str) -> Result<String, ParseError> {
        let start = self.i;
        while matches!(self.peek(), Some(b) if b.is_ascii_alphabetic()) {
            self.i += 1;
        }
        if self.i > start {
            // the corpus is folded lowercase ASCII, so the pattern is too
            return Ok(self.s[start..self.i].iter().map(|b| b.to_ascii_lowercase() as char).collect());
        }
        if !after.is_empty() {
            return self.fail(format!("nothing on this side of '{after}'"));
        }
        match self.peek() {
            Some(b')') => self.fail("empty '()'"),
            Some(b'|') => self.fail("nothing on this side of '|'"),
            Some(b'(') => self.fail("'(' where a word was expected"),
            Some(b) if b.is_ascii_whitespace() => self.fail("expected a word"),
            Some(b) if b.is_ascii() => self.fail(format!("unexpected '{}'", b as char)),
            Some(_) => self.fail("only the letters a-z can appear in a keyword"),
            None => self.fail("expected a word"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(p: &Pattern) -> Vec<Vec<Vec<&str>>> {
        p.terms
            .iter()
            .map(|t| t.groups.iter().map(|g| g.words.iter().map(String::as_str).collect()).collect())
            .collect()
    }

    #[test]
    fn bare_words_are_one_required_block_each() {
        let p = parse("source code").unwrap();
        assert_eq!(words(&p), vec![vec![vec!["source"]], vec![vec!["code"]]]);
        assert!(p.terms.iter().all(|t| !t.optional), "unmarked blocks are required");
        assert!(p.terms.iter().all(|t| t.width() == 1));
    }

    #[test]
    fn a_pipe_makes_one_block_of_alternatives() {
        // one block, one letter -- either word, never both
        let p = parse("code|source").unwrap();
        assert_eq!(words(&p), vec![vec![vec!["code", "source"]]]);
        assert_eq!(p.terms[0].width(), 1);
    }

    #[test]
    fn brackets_need_no_space_between_them() {
        let p = parse("(source|code)(map|graph)").unwrap();
        assert_eq!(
            words(&p),
            vec![vec![vec!["source", "code"]], vec![vec!["map", "graph"]]]
        );
    }

    #[test]
    fn a_hyphen_chains_groups_into_one_wide_block() {
        let p = parse("source-code").unwrap();
        assert_eq!(p.terms.len(), 1, "a chain is a single block");
        assert_eq!(p.terms[0].width(), 2);
    }

    #[test]
    fn modifiers_may_be_written_in_either_order() {
        for text in ["~?(source|code)", "?~(source|code)"] {
            let p = parse(text).unwrap();
            assert!(p.terms[0].optional, "{text}");
            assert!(p.terms[0].groups[0].interior, "{text}");
        }
    }

    #[test]
    fn a_tilde_marks_only_the_group_it_precedes() {
        let p = parse("(graph|map)-~(builder|viewer)").unwrap();
        assert!(!p.terms[0].groups[0].interior);
        assert!(p.terms[0].groups[1].interior);
    }

    #[test]
    fn the_worked_example_parses() {
        let p = parse("~?(source|code) (graph|map)-~(builder|viewer)").unwrap();
        assert_eq!(p.terms.len(), 2);
        assert!(p.terms[0].optional && p.terms[0].groups[0].interior);
        assert!(!p.terms[1].optional && p.terms[1].width() == 2);
        assert_eq!(
            p.words().collect::<Vec<_>>(),
            ["source", "code", "graph", "map", "builder", "viewer"]
        );
    }

    #[test]
    fn an_empty_pattern_is_not_an_error() {
        assert!(parse("").unwrap().is_empty());
        assert!(parse("   ").unwrap().is_empty());
    }

    #[test]
    fn each_way_of_writing_nonsense_says_what_is_wrong() {
        let cases = [
            ("(source|code", "unclosed"),
            ("()", "empty"),
            ("source|", "'|'"),
            ("|source", "'|'"),
            ("source-", "'-'"),
            ("a-?b", "whole block"),
            ("~", "expected a word"),
            ("source!", "unexpected '!'"),
        ];
        for (text, want) in cases {
            let e = parse(text).unwrap_err();
            assert!(
                e.message.contains(want),
                "{text:?} reported {:?}, which does not mention {want:?}",
                e.message
            );
            assert!(e.at <= text.len());
        }
    }

    #[test]
    fn an_error_points_at_the_offending_character() {
        let text = "(source|code (map|graph)";
        let e = parse(text).unwrap_err();
        assert_eq!(e.column(text), 12, "the caret lands where the ')' should have been");
    }

    #[test]
    fn keywords_are_folded_to_lowercase() {
        let p = parse("Source-Code").unwrap();
        assert_eq!(words(&p), vec![vec![vec!["source"], vec!["code"]]]);
    }
}
