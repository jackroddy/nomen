"""Add the recognizability column to the classical corpora, and admit the proper
nouns worth having.

Two things happen here, both needing the raw dump again, and both needing
data/vocab.txt -- which is why this runs after build_semantics.py rather than
inside build_classical.py.

1. The fifth column: whether Wiktionary records an English descendant of the
   lemma. There is no frequency corpus for Latin or Greek, so the band-pass that
   ranks English cannot be applied; this is the strongest of the three signals
   that stand in for it. It separates cleanly -- aquila, ferrum, opus, nexus,
   forma and pons all have one, while sapo ("an ancient hair product") and bacar
   ("a kind of wine glass") do not.

2. The proper nouns. Dropping them wholesale cost the evocative half of the
   classical vocabulary: juno, apollo, orion, minerva, ceres, flora and titan are
   exactly the names people reach for. Re-admitting them wholesale is no good
   either -- Latin alone carries 11,253, mostly obscure places. They come back
   only if English borrowed the form outright or the lemma has an English
   descendant, which keeps the names above and drops abdera, spqr, boeotia and
   liguria.
"""

import json
import re
import sys
import unicodedata

LIG = {"æ": "ae", "Æ": "ae", "œ": "oe", "Œ": "oe", "ø": "o", "ß": "ss"}
GREEK = {
    "α": "a", "β": "b", "γ": "g", "δ": "d", "ε": "e", "ζ": "z", "η": "e", "θ": "th",
    "ι": "i", "κ": "k", "λ": "l", "μ": "m", "ν": "n", "ξ": "x", "ο": "o", "π": "p",
    "ρ": "r", "σ": "s", "ς": "s", "τ": "t", "υ": "u", "φ": "ph", "χ": "kh", "ψ": "ps",
    "ω": "o", "ϝ": "w",
}

META = re.compile(r"Hesychius|definition as|meaning of this term|uncertain meaning", re.I)
GRAMMAR = re.compile(
    r"^(inflection|plural|genitive|dative|accusative|ablative|vocative|nominative|"
    r"singular|first|second|third|present|perfect|future|imperfect|past|masculine|"
    r"feminine|neuter|comparative|superlative|alternative|obsolete|archaic|"
    r"misspelling|abbreviation|initialism|romanization|synonym)\b",
    re.I,
)
HEAD = re.compile(r"^[^.;:]*")
# a proper noun reached this way inherits its target's gloss rather than being
# dropped for having a grammatical one
POINTER = re.compile(r"^alternative (?:spelling|form|letter-case form) of\s+(\S+)", re.I)


def fold(s):
    s = "".join(LIG.get(c, c) for c in s)
    s = unicodedata.normalize("NFD", s)
    return "".join(c for c in s if not unicodedata.combining(c)).lower()


def romanize(s):
    return "".join(GREEK.get(c, c) for c in fold(s))


def has_english(descendants):
    """Whether any descendant, at any depth, is English."""
    for d in descendants or ():
        if d.get("lang_code") == "en":
            return True
        if has_english(d.get("descendants")):
            return True
    return False


def annotate(raw, tsv, lang, vocab):
    rows = [l.rstrip("\n").split("\t") for l in open(tsv, encoding="utf8") if l.strip()]
    rows = [r[:4] for r in rows if len(r) >= 4]

    # every entry with an English descendant, for the fifth column
    descended = set()
    # proper nouns only, for the gate below
    name_gloss, name_pointer, name_descended = {}, {}, {}

    for line in open(raw, encoding="utf8", errors="replace"):
        try:
            d = json.loads(line)
        except Exception:
            continue
        w = d.get("word", "")
        english = has_english(d.get("descendants"))
        if english:
            descended.add(w)
        if d.get("pos") != "name":
            continue
        if english:
            name_descended[fold(w)] = True
        for s in d.get("senses") or ():
            fo = s.get("form_of") or s.get("alt_of")
            if fo and isinstance(fo[0], dict) and fo[0].get("word"):
                name_pointer.setdefault(w, fo[0]["word"])
                continue
            for g in s.get("glosses") or ():
                if META.search(g):
                    continue
                m = POINTER.match(g)
                if m:
                    name_pointer.setdefault(w, m.group(1).rstrip(":").strip())
                    continue
                if GRAMMAR.match(g):
                    continue
                name_gloss.setdefault(w, g)
                break

    rows = [r + ["1" if r[2] in descended else "0"] for r in rows]
    flagged = sum(1 for r in rows if r[4] == "1")
    common = len(rows)

    # macrons differ between an entry's headword and how other entries cite it
    # (Iuno holds the gloss, Juno points at "Iūnō"), so resolve on folded keys
    by_fold = {}
    for w, g in name_gloss.items():
        by_fold.setdefault(fold(w), g)

    have = {(r[0], r[2]) for r in rows}
    added = 0
    for w in sorted(set(name_gloss) | set(name_pointer)):
        target = w if w in name_gloss else name_pointer.get(w, "")
        gloss = name_gloss.get(target) or by_fold.get(fold(target))
        if not gloss:
            continue
        form = romanize(w) if lang == "grc" else fold(w)
        if not re.fullmatch(r"[a-z]{3,8}", form):
            continue
        descendant = name_descended.get(fold(target)) or name_descended.get(fold(w))
        # borrowed outright, or the lemma left an English descendant
        if not (form in vocab or descendant):
            continue
        key = (form, target)
        if key in have:
            continue
        have.add(key)
        head = HEAD.match(gloss).group(0).strip().rstrip(",")[:60]
        if not head:
            continue
        rows.append([form, w, target, head, "1" if descendant else "0"])
        added += 1

    with open(tsv, "w", encoding="utf8") as f:
        f.write("\n".join("\t".join(r) for r in rows) + "\n")
    pct = 100 * flagged / common if common else 0
    print(
        f"  {tsv}: {common} common nouns, {flagged} ({pct:.0f}%) with an English "
        f"descendant; +{added} proper nouns -> {len(rows)} rows"
    )


def main(work, data):
    vocab = {l.strip() for l in open(f"{data}/vocab.txt")}
    annotate(f"{work}/latin_raw.jsonl", f"{data}/latin.tsv", "la", vocab)
    annotate(f"{work}/greek_raw.jsonl", f"{data}/greek.tsv", "grc", vocab)


if __name__ == "__main__":
    main(*(sys.argv[1:3] or ["work", "../data"]))
