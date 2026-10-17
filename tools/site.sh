#!/bin/sh
# The guide and function map (GitHub Pages), built from site/ and the symbol map.
#
#   tools/site.sh build      symbols/ from the local labels (tools/symbols_export.py), then _site/ (git-ignored)
#   tools/site.sh serve      build, then serve _site/ on http://localhost:8000
#   tools/site.sh publish    build, then commit _site/ to the gh-pages branch and push it (Pages serves that branch)
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT="$ROOT/_site"
build() {
    python3 "$ROOT/tools/symbols_export.py" "$ROOT/content" "$ROOT/symbols"
    rm -rf "$OUT"
    mkdir -p "$OUT/data" "$OUT/img"
    cp "$ROOT"/site/*.html "$ROOT"/site/*.css "$ROOT"/site/*.js "$OUT/"
    cp "$ROOT/symbols/loveplus_plus_boxj.json" "$OUT/data/symbols.json"
    cp "$ROOT/docs/screenshots/date-dialogue.webp" "$ROOT/tools/package/icon/icon_256.png" "$OUT/img/"
    touch "$OUT/.nojekyll"
    echo "built $OUT"
}
case "${1:-build}" in
    build) build ;;
    serve) build; cd "$OUT" && python3 -m http.server 8000 ;;
    publish)
        build
        WT="$ROOT/engine/target/gh-pages"
        rm -rf "$WT"; git -C "$ROOT" worktree prune
        if git -C "$ROOT" ls-remote --exit-code --heads origin gh-pages >/dev/null 2>&1; then
            git -C "$ROOT" fetch -q origin gh-pages
            git -C "$ROOT" worktree add -q -B gh-pages "$WT" origin/gh-pages
        else
            git -C "$ROOT" worktree add -q --orphan -b gh-pages "$WT"
        fi
        find "$WT" -mindepth 1 -maxdepth 1 ! -name .git -exec rm -rf {} +
        cp -R "$OUT"/. "$WT"/
        git -C "$WT" add -A
        git -C "$WT" commit -q -m "Site from $(git -C "$ROOT" rev-parse --short HEAD)" || echo "no changes"
        git -C "$WT" push -q origin gh-pages
        git -C "$ROOT" worktree remove --force "$WT"
        echo "pushed gh-pages"
        ;;
    *) echo "usage: tools/site.sh build|serve|publish" >&2; exit 1 ;;
esac
