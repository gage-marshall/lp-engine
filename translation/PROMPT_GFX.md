# Relabelling LovePlus+ UI graphics in English

Read `translation/PROMPT.md` (glossary, names) first. The game draws much of its UI as images: buttons, tabs, HUD
labels with Japanese drawn in. `tools/lp_gfx.py` redraws such an image in English: it finds the Japanese strokes,
fills them with the background (or, for a free-standing word on transparency, clears the cell) and draws your
English in the game's own font, rotated like the original. You decide which images have Japanese and what they
should say.

## Input

Inventory sheets (PNG) per archive: every cell of the archive, 2x, each labelled `ncgr/ncer/cell` (archive entry
numbers). Colours may be off (palettes are guessed); the text is readable.

## Output

`translation/en/gfx/<archive with / replaced by _>.json`, e.g. `translation/en/gfx/arc_date_ui.bin.json`:

```json
{"archive": "arc/date_ui.bin", "items": [
  {"ncgr": 34, "ncer": 35, "cell": 0, "en": "(Mon)", "rot": 90}
]}
```

One item per cell that shows Japanese text (the tables hold no Japanese: name the label in your report, not in
the table). Rules:

1. **Every copy.** Archives hold several banks with the same labels (one per screen); list each cell that has
   Japanese, in every bank, with the same English.
2. **Skip** cells without Japanese: icons, frames, digits, Latin text (UP, DOWN, GAME START), pictures.
3. **rot**: 90 when the Japanese runs top to bottom with each character turned (the sideways DS screen; most of the
   UI), 0 when it reads left to right, upright.
4. **Short.** The English must fit where the Japanese was; buttons hold about 7-12 letters across their length,
   small tags 3-5. Use the shortest natural label ("もどる" = "Back", "決定" = "OK", "予定入力" = "Plan Day",
   "自室" = "My Room", weekdays "(Mon)", seasons "Spring" / "Summer" / "Fall" / "Winter", 魅力 = "Charm",
   感性 = "Sense", 知識 = "Smarts", 運動 = "Sports"). Same Japanese, same English, in every archive.
5. A cell that holds a single kanji or kana that is clearly a piece of a longer word built from several cells
   (e.g. one character of a big title): list it with `"en": ""` and say so in your report; it is handled by hand.

## Check

    python3 tools/lp_gfx.py check content translation/en/gfx/<file>.json      # what does not fit / has no text
    python3 tools/lp_gfx.py preview content translation/en/gfx/<file>.json OUT.png   # before (top) / after

Fix what `check` reports (shorter English, or `"mode": "glyph"` / `"mode": "label"` when the tool picked the wrong
kind: label = text on a solid button, glyph = a word with an outline on transparency; for anything harder see below). Then make a preview and
look at it: each English label should be readable, in the right direction, where the Japanese was.

## Hard cells: look, then say exactly what to redraw

The automatic modes fail on gradient or two-tone buttons, text joined to a frame, several labels in one cell,
a word next to an icon. For those, look at the cell and give the tool the exact place:

    python3 tools/lp_gfx.py show content ARCHIVE NCGR NCER CELL OUT.png

draws the cell 4x with an 8 px grid (coordinates on the edges), each object (OAM) outlined in orange with its
number (`obj4`), and prints the palette indices it uses with their colours. Then pick, in this order:

1. **`"objs": [4, 5]`**: the text sits in objects of its own (very common: a label object laid over a button).
   Those objects are cleared and the English drawn in their area, with their own ink and outline colours.
   Nothing else changes. Use it whenever the Japanese is exactly some objects' content.
2. **`"mode": "label", "box": [x, y, w, h]`**: text on a solid ground inside a box (cell pixels); everything in
   the box that is not the box's commonest colour is replaced. Keep the box inside the button, off its border.
3. **`"mode": "fill", "box": ...`**: the box is repainted line by line from the colours just outside it (gradients),
   or with `"ground": N` (an index; 0 = transparent), then the English drawn in `"ink": N` (default: the
   original's strongest colour) with an optional `"outline": N`.
A box can be a list of boxes (`[[x, y, w, h], [x, y, w, h]]`) for an L-shaped text area. `"rot": 270` for text
read bottom to top. Several labels in one cell: one item per label, each with its own box or objects.

Always preview and look: the English must be where the Japanese was, the Japanese fully gone, the art intact.
