"""Build data/latin.tsv and data/greek.tsv from the Wiktionary (kaikki) dumps.

Two passes over one stream: collect lemma glosses and form->lemma pointers, then
join. Inflected forms are kept -- each is a distinct acronym with its own
crates.io availability -- but they inherit their lemma's gloss, since their own
("accusative singular of aquila") names no concept to score against.

This pass emits four columns and common nouns only. annotate_classical.py adds
the fifth and admits the proper nouns worth having; it runs later because its
gate needs the vector vocabulary, which is not built until the glosses exist.
"""

import json
import re
import sys
import unicodedata

LIG = {"æ": "ae", "Æ": "ae", "œ": "oe", "Œ": "oe", "ø": "o", "ß": "ss", "đ": "d", "ħ": "h"}
GREEK = {
    "α": "a", "β": "b", "γ": "g", "δ": "d", "ε": "e", "ζ": "z", "η": "e", "θ": "th",
    "ι": "i", "κ": "k", "λ": "l", "μ": "m", "ν": "n", "ξ": "x", "ο": "o", "π": "p",
    "ρ": "r", "σ": "s", "ς": "s", "τ": "t", "υ": "u", "φ": "ph", "χ": "kh", "ψ": "ps",
    "ω": "o", "ϝ": "w",
}

# a gloss that is purely grammatical describes the inflection, not the concept
GRAMMAR = re.compile(
    r"^(inflection|plural|genitive|dative|accusative|ablative|vocative|nominative|"
    r"singular|first|second|third|present|perfect|future|imperfect|past|"
    r"masculine|feminine|neuter|comparative|superlative|alternative|obsolete|"
    r"archaic|misspelling|abbreviation|initialism|romanization|synonym)\b",
    re.I,
)

# a gloss that only names a person or place describes no concept to score
NAMEY = re.compile(
    r"^(a |an |the )?((male|female|unisex) )?(given name|surname|"
    r"family name|patronymic|nickname)|^A (city|town|village|river|"
    r"province|region|municipality|commune|placename)",
    re.I,
)


def fold(s):
    """Strip diacritics and expand ligatures to bare ASCII."""
    s = "".join(LIG.get(c, c) for c in s)
    s = unicodedata.normalize("NFD", s)
    return "".join(c for c in s if not unicodedata.combining(c)).lower()


def romanize(s):
    return "".join(GREEK.get(c, c) for c in fold(s))


def extract(path, lang, out):
    lemma_gloss = {}  # headword -> English gloss
    form_of = {}      # surface form -> the lemma it inflects
    roman = {}        # surface form -> romanization, when the dump supplies one

    for line in open(path, encoding="utf8", errors="replace"):
        try:
            d = json.loads(line)
        except Exception:
            continue
        w = d.get("word")
        if not w:
            continue
        # kaikki tags proper nouns as pos "name"; they were 19% of Latin and are
        # mostly obscure places (Gottinga, Sicambri). The common-noun homograph
        # is a separate entry, so dropping these keeps `victory` and loses only
        # the goddess -- annotate_classical.py brings back the ones worth having.
        if d.get("pos") == "name" or w[:1].isupper():
            continue

        for f in d.get("forms") or ():
            if "romanization" in (f.get("tags") or ()) and f.get("form"):
                roman.setdefault(w, f["form"])

        for s in d.get("senses") or ():
            fo = s.get("form_of") or s.get("alt_of")
            if fo:
                target = fo[0].get("word") if isinstance(fo[0], dict) else None
                if target:
                    form_of.setdefault(w, target)
                continue
            gl = s.get("glosses") or ()
            # first usable sense wins, but a name-only sense never counts --
            # otherwise `aquila` takes "a male given name" over "eagle"
            if gl and not GRAMMAR.match(gl[0]) and not NAMEY.match(gl[0]):
                lemma_gloss.setdefault(w, gl[0])

    seen = {}
    rows = []
    for w in sorted(set(lemma_gloss) | set(form_of)):
        lemma = w if w in lemma_gloss else form_of.get(w, "")
        gloss = lemma_gloss.get(lemma)
        if not gloss:
            continue

        ascii_form = romanize(w) if lang == "grc" else fold(w)
        if not re.fullmatch(r"[a-z]{3,8}", ascii_form):
            # fall back to the dump's own romanization when transliteration fails
            r = roman.get(w)
            ascii_form = fold(r) if r else ""
            if not re.fullmatch(r"[a-z]{3,8}", ascii_form or ""):
                continue

        gloss = gloss.split(";")[0].strip()[:60]
        # several spellings fold to one ASCII string -- τέχνη and τέχνῃ both give
        # `tekhne` -- so keep one row per concept per acronym, preferring the
        # lemma's own spelling
        key = (ascii_form, lemma)
        if key in seen:
            if w == lemma:
                rows[seen[key]] = (ascii_form, w, lemma, gloss)
            continue
        seen[key] = len(rows)
        rows.append((ascii_form, w, lemma, gloss))

    with open(out, "w", encoding="utf8") as f:
        for r in rows:
            f.write("\t".join(r) + "\n")
    print(f"  {out}: {len(rows)} rows  (lemmas={len(lemma_gloss)} forms={len(form_of)})")


def main(work, data):
    extract(f"{work}/latin_raw.jsonl", "la", f"{data}/latin.tsv")
    extract(f"{work}/greek_raw.jsonl", "grc", f"{data}/greek.tsv")


if __name__ == "__main__":
    main(*(sys.argv[1:3] or ["work", "../data"]))
