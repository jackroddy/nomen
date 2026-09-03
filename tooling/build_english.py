"""Build data/words.txt: the English name candidates, frequency-ordered.

A subtitle frequency corpus supplies the ranking; web2 supplies validity. Neither
works alone — the corpus is full of misspellings and proper nouns, and web2 is
unranked Webster's 2nd, full of archaisms like `besoot` and `slepez`.

Do not use /usr/share/dict/words directly as a name source.
"""

import re
import sys

# web2 lists proper nouns capitalised, so taking only the lowercase headwords
# drops them for free -- which is also why English proper nouns are absent from
# the corpus entirely. See CLAUDE.md, "Deferred".
DICT = "/usr/share/dict/words"


def stems(w):
    """The word, plus every form it could be a regular inflection of.

    web2 is a lemma dictionary: it lists `run` but not `runs`, `pie` but not
    `pies`. Checking the stems as well is what keeps inflections, which are
    distinct acronyms supporting different keywords.
    """
    yield w
    if w.endswith("s"):
        yield w[:-1]
    if w.endswith("es"):
        yield w[:-2]
        yield w[:-2] + "e"
    if w.endswith("ed"):
        yield w[:-2]
        yield w[:-2] + "e"
    if w.endswith("ing"):
        yield w[:-3]
        yield w[:-3] + "e"
    if w.endswith("ies"):
        yield w[:-3] + "y"
    # dropped -> drop, running -> run
    if len(w) > 4 and w[-1] == w[-2]:
        for suffix in ("ed", "ing"):
            if w.endswith(suffix):
                yield w[: -len(suffix) - 1]


def main(work, data):
    valid = {w for w in (l.strip() for l in open(DICT)) if w and re.fullmatch(r"[a-z]+", w)}

    out = []
    for line in open(f"{work}/en_50k.txt"):
        parts = line.split()
        if len(parts) != 2:
            continue
        word, count = parts
        # 3 letters is the shortest name with room for keywords; 8 is where a
        # name stops reading as a word people would adopt
        if not (3 <= len(word) <= 8) or not re.fullmatch(r"[a-z]+", word):
            continue
        if any(s in valid for s in stems(word)):
            out.append((word, count))

    with open(f"{data}/words.txt", "w") as f:
        for word, count in out:
            f.write(f"{word} {count}\n")
    print(f"  words.txt: {len(out)} English names, frequency-ordered")


if __name__ == "__main__":
    main(*(sys.argv[1:3] or ["work", "../data"]))
