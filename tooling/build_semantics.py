"""Build the vendored vector table and sentiment lexicon.

The table is GloVe 6B 50d, trimmed and quantized to i8 -- 2.2 MB for 47k words,
small enough to commit and `include_bytes!`. Two things decide what to keep:

  * every word the engine must be able to score -- the English name list and
    every content token appearing in a classical gloss. Without these an entry
    has no concept vector and falls back to a neutral 0.5.
  * the most frequent 40k GloVe words, so that an arbitrary keyword the user
    types resolves to something.

Run this after build_classical.py: the gloss vocabulary does not exist until the
classical corpora do.
"""

import os
import re
import sys

import numpy as np

DIM = 50
# GloVe is frequency-ordered, so a prefix is the most common words
KEEP_TOP = 40000
# words too common to carry meaning when averaging a gloss; mirrors STOP in
# src/semantics.rs
STOP = set(
    "the a an of to in for and or is was be by on at as with from that this it its one".split()
)


def main(work, data):
    need = {l.split()[0] for l in open(f"{data}/words.txt")}

    gloss_tokens = set()
    for name in ("latin.tsv", "greek.tsv"):
        for line in open(f"{data}/{name}", encoding="utf8"):
            parts = line.rstrip("\n").split("\t")
            if len(parts) < 4:
                continue
            for t in re.findall(r"[a-z]+", parts[3].lower()):
                if len(t) >= 3 and t not in STOP:
                    gloss_tokens.add(t)
    need |= gloss_tokens
    print(f"  english names: {sum(1 for _ in open(f'{data}/words.txt'))}")
    print(f"  distinct gloss tokens: {len(gloss_tokens)}")

    vocab, vecs = [], []
    for i, line in enumerate(open(f"{work}/glove.6B.50d.txt", encoding="utf8")):
        parts = line.rstrip().split(" ")
        w = parts[0]
        if not re.fullmatch(r"[a-z]+", w):
            continue
        if i >= KEEP_TOP and w not in need:
            continue
        v = np.asarray(parts[1:], dtype=np.float32)
        n = np.linalg.norm(v)
        if n < 1e-6:
            continue
        vocab.append(w)
        # stored as unit vectors, so cosine is a dot product at run time
        vecs.append(v / n)

    quantized = np.clip(np.rint(np.stack(vecs) * 127.0), -127, 127).astype(np.int8)
    open(f"{data}/vocab.txt", "w").write("\n".join(vocab) + "\n")
    open(f"{data}/vocab_vec.i8", "wb").write(quantized.tobytes())

    have = set(vocab)
    covered = sum(1 for w in need if w in have)
    size = os.path.getsize(f"{data}/vocab_vec.i8") / 1048576
    print(f"  vocab.txt/vocab_vec.i8: {len(vocab)} words x {DIM} dims -> {size:.1f} MB")
    print(f"  coverage of words needing a vector: {covered}/{len(need)} "
          f"({100 * covered / len(need):.1f}%)")

    # VADER: hand-scored valence, roughly -4..=4. More reliable per word than the
    # good-bad axis in src/semantics.rs, but it only covers 7k of the vocabulary,
    # which is why both exist.
    kept = 0
    with open(f"{data}/sentiment.tsv", "w") as out:
        for line in open(f"{work}/vader_lexicon.txt"):
            parts = line.split("\t")
            if len(parts) >= 2 and re.fullmatch(r"[a-z]+", parts[0]):
                out.write(f"{parts[0]}\t{parts[1]}\n")
                kept += 1
    print(f"  sentiment.tsv: {kept} entries")


if __name__ == "__main__":
    main(*(sys.argv[1:3] or ["work", "../data"]))
