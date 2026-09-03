#!/bin/sh
# Download every source data/ is built from into work/.
#
# The kaikki dumps are cached rather than streamed: build_classical.py and
# annotate_classical.py each read them, and re-downloading 1.4 GB to add one
# column is not a trade worth making.
set -e
cd "$(dirname "$0")"
mkdir -p work
cd work

fetch() {
    [ -s "$2" ] && { echo "have $2"; return; }
    echo "fetching $2..."
    curl -sSL --max-time 3600 -o "$2" "$1"
}

fetch "https://raw.githubusercontent.com/hermitdave/FrequencyWords/master/content/2018/en/en_50k.txt" en_50k.txt
fetch "https://raw.githubusercontent.com/cjhutto/vaderSentiment/master/vaderSentiment/vader_lexicon.txt" vader_lexicon.txt
fetch "https://kaikki.org/dictionary/Latin/kaikki.org-dictionary-Latin.jsonl" latin_raw.jsonl
fetch "https://kaikki.org/dictionary/Ancient%20Greek/kaikki.org-dictionary-AncientGreek.jsonl" greek_raw.jsonl

if [ ! -s glove.6B.50d.txt ]; then
    fetch "https://huggingface.co/stanfordnlp/glove/resolve/main/glove.6B.zip" glove.6B.zip
    unzip -o glove.6B.zip glove.6B.50d.txt
fi

echo "sources in $(pwd):"
ls -la
