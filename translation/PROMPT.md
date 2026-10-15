# Translating LovePlus+ scripts into English

You translate the game's script messages into natural English, one script file at a time. The game is a dating
sim: the player is a high-school boy; the three heroines are Takane Manaka (高嶺愛花, a classmate, earnest and
gentle), Kobayakawa Rinko (小早川凛子, a first-year, blunt and prickly at first, softens later) and Anegasaki Nene
(姉ヶ崎寧々, a third-year who works at a family restaurant, warm, teasing big-sister manner).

## Input

`work/translation/messages/NNNN.json`: `{"file": N, "messages": [...]}`, the script's messages in the order they
appear. Each message:

- `pc`, `sha`: its key; copy both unchanged into your output.
- `speaker`: the name tag (`{v0}` = the player himself; `　` = narration / no name).
- `text`: the Japanese, with the game's own line breaks (`\n`) and variables `{vN}`.
- `sub` (rare): a small label.
- `entry`: which part of the script it is in (messages of one entry form one scene or one variant of a line).

Read the whole file before translating: tone and who is speaking to whom come from the scene.

## Output

Write `translation/en/scripts/NNNN.json`:

```json
{"messages": [{"pc": 258, "sha": "ae3563f2fe75", "en": "Wait... did you turn it off without saving?"}]}
```

One row per input message, same order. Then run
`python3 tools/lp_translate.py check work/translation/messages translation/en/scripts/NNNN.json` and fix every
problem it reports.

## Rules

1. **Natural, spoken English.** Translate meaning and tone, not word order. Keep each heroine's voice consistent
   with the scene. Inner thoughts of the player are in parentheses in Japanese (（…）); keep them in parentheses.
2. **Names as the text renders them.** Romanize what the Japanese writes, in Hepburn, given name or family name as
   written: 高嶺 = Takane, 愛花 = Manaka, 小早川 = Kobayakawa, 凛子 / リンコ / りんこ = Rinko, 姉ヶ崎 = Anegasaki,
   寧々 / ネネ = Nene. Keep honorifics as written: 寧々さん = Nene-san, ネネさん = Nene-san, 先輩 = senpai,
   ～くん = -kun, ～ちゃん = -chan.
3. **Choice menus**: a message with `"choices": N` is a menu the player picks from, one option per line of the
   Japanese. Write exactly N options separated by `\n`, in the same order, each short enough for the menu row
   (about 25 characters). Options are what the player says or does; keep them short and parallel.
4. **No line breaks** (except between choice options). Write each message as one line; the patch wraps it to the text box.
   A message may be up to
   about five lines of 30 characters on screen, but shorter is better; never over 300 bytes.
5. **Variables.** `{vN}` are filled in at run time. Keep the ones you need, in any order; you may drop one (its slot
   becomes empty) but never add one the original does not have.
   - `{v0}` the player's name, `{v28}` his family name, `{v27}` his given name.
   - `{v1}` the player's first-person pronoun (俺 / 僕). English needs I / me / my depending on the sentence: drop
     `{v1}` and write the right pronoun yourself.
   - `{v3}`, `{v7}`, `{v34}`: what the player calls the heroine (e.g. 高嶺, ネネさん). `{v5}` likewise.
   - `{v26}`, `{v35}`: what the heroine calls the player.
   - `{v2}`, `{v6}`: a heroine's first-person pronoun (drop and write "I" / "me" as needed).
   - `{v8}` `{v9}` `{v11}` `{v20}` `{v21}`: parts of a date or time (month, day, time of day, place); `{v22}`
     `{v23}` times; `{v29}`–`{v32}`, `{v33}`: game-filled words. Keep them, in a sensible English order.
6. **Special characters.** The game's font draws α as `!?`, β as `!!`, γ … δ as quotation marks. Write `!?`, `!!`
   and `"…"` in English. ▼ is drawn as a **heart**: keep every ▼ in the English where it fits (usually the end
   of the sentence, in place of the full stop: `Okay▼`, `See you tomorrow▼`). ♪ (a music note) likewise: keep it. Write 「」 as `"`, …… as `...`, ～ / ー stretching as nothing or a natural spelling.
7. **Spoken into the microphone.** When a heroine asks the player to say a phrase out loud (the game listens for
   Japanese), keep that phrase in romaji, e.g. 「ネネごめんなさい」 = "Nene gomen nasai".
8. **Only ASCII** in `en`, except ▼ and ♪ (rule 6) (no Japanese, no curly quotes, no em dashes: use `--` or `...`).
9. **Skip wordless messages**: a message with no words (only ……, ！？, ・, spacing, line breaks, e.g. the
   vertical `・ ・ ・` time skip) keeps the original as it is; leave it out of your output.
10. Write the table with a script that copies `pc` and `sha` from the input, and check every row's English belongs
   to its own message (a row keyed to the wrong message is the worst mistake: the check cannot see it).

## Glossary (keep these the same everywhere)

十羽野 / とわの = Towano (the town; 十羽野高校 = Towano High), デキシーズ = Dixie's (Nene's family restaurant),
カレルビー = Carlbee (potato-chip brand), 俺 vs 僕 choices = brash vs mild wording (the English must differ).
Rinko often calls herself リンコ in the third person: keep it as "Rinko" ("Rinko doesn't like that"), it is her voice.
Manaka likewise sometimes says 愛花 of herself: keep "Manaka".
