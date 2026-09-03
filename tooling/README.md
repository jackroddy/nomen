# tooling

Everything in `data/` is generated. These scripts are what generated it.

The data files are committed, so a clone builds and runs without any of this. You need
these only to rebuild a corpus — a newer Wiktionary dump, a different frequency list, a
changed filter.

## Provenance

These scripts were **reconstructed from the session transcripts that originally built
`data/`**, after the fact: the originals were written in a scratch directory and never
committed. The logic, regexes and thresholds are the ones that ran.

What has actually been re-run and diffed against the committed output:

| script | verified |
|---|---|
| `build_english.py` | **yes** — reproduces `data/words.txt` byte for byte (21,558 lines) |
| `build_semantics.py` | **the lexicon half** — reproduces `data/sentiment.tsv` byte for byte (7,217 lines). The vector half needs the 822 MB GloVe archive and has not been run. |
| `build_classical.py` | no — needs the 1.4 GB Latin and 450 MB Greek dumps |
| `annotate_classical.py` | no — same |

Treat the unverified two as faithful but untested. If you run them, the counts they print
are the check: they should land near the numbers below.

## Order

The steps are not independent, and the order is not arbitrary:

```
1. build_english.py       →  data/words.txt
2. build_classical.py     →  data/latin.tsv, data/greek.tsv   (common nouns, 4 columns)
3. build_semantics.py     →  data/vocab.txt, data/vocab_vec.i8, data/sentiment.tsv
4. annotate_classical.py  →  adds the 5th column, then admits proper nouns
```

**3 must come after 2**, because the vector table is trimmed to cover the gloss vocabulary,
which does not exist until the classical corpora do. **4 must come after 3**, because the
proper-noun gate asks whether English borrowed a form outright, and that question is
answered by `data/vocab.txt`. That cycle is why the classical corpus is built in two stages
rather than one.

## Sources

| what | where | size |
|---|---|---|
| English frequency | `hermitdave/FrequencyWords` `content/2018/en/en_50k.txt` | 1 MB |
| English headwords | `/usr/share/dict/words` (web2, on the machine) | 2 MB |
| Latin | `kaikki.org/dictionary/Latin/kaikki.org-dictionary-Latin.jsonl` | ~1.4 GB |
| Ancient Greek | `kaikki.org/dictionary/Ancient Greek/…-AncientGreek.jsonl` | ~450 MB |
| Vectors | `stanfordnlp/glove` `glove.6B.zip`, the 50d file | 822 MB |
| Sentiment | `cjhutto/vaderSentiment` `vader_lexicon.txt` | 400 KB |

`./fetch.sh` puts all of them in `tooling/work/`, which is gitignored. The kaikki dumps are
cached there rather than streamed, because steps 2 and 4 each read them.

## Running it

```sh
cd tooling
./fetch.sh                                    # ~2.5 GB, slow
python3 build_english.py    work ../data
python3 build_classical.py  work ../data
python3 build_semantics.py  work ../data
python3 annotate_classical.py work ../data
cd .. && cargo test
```

Each script prints the counts it produced. They should land near: 21,558 English; 39,777
Latin; 19,756 Greek; 47k vectors; 7,217 sentiment entries. Exact numbers move with the
upstream dumps.
