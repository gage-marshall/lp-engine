# Translating LovePlus+ text resources (menus, items, mail, help) into English

Read `translation/PROMPT.md` first: the characters, the tone, the glossary and rules 1, 2, 6 and 8 apply here too.
This file covers the game's other text: `text/gd_textres*_jpn.txrc`, exported per file.

## Input

`work/translation/text/<file>.json`: `{"file": ..., "groups": [...]}`. A group is one list of related lines (one
menu, one item list, one heroine's mail). Each group has:

- `id` ("g.s"), `box`: where the game draws it. `width` in pixels, `lines` the most lines the box has (0 = the
  message window, which pages). The patch wraps your English to the width at spaces, so **write no line breaks**
  except where the Japanese line is a list or the line break means something (rare).
- `keep`: the group stays Japanese (input data, not shown as text). Write nothing for it.
- `partial`: credits. Translate the role titles (`[シナリオ]` = `[Scenario]`) and headings; leave out lines that are
  only a person's name (they stay as written).
- `rows`: `{"id": "g.s.l", "sha": ..., "text": ...}`, the Japanese, `\n` = the game's own line break.

## Output

`translation/en/text/<file>.json`: `{"rows": [{"id": "0.1.3", "sha": "...", "en": "..."}]}`, one row per input row
(except kept groups and names in credits). Write it with a script that copies `id` and `sha` from the input and
takes the English from a dict keyed by id. Then run

    python3 tools/lp_translate.py check-text work/translation/text translation/en/text/<file>.json

and fix every problem. "does not fit its box" means: shorter English. Width is the hard limit: a one-line box of
96 px holds about 16 lower-case letters; 156 px about 26.

## Rules

1. **Short and natural.** These are labels, buttons, item names, short mails. Prefer the shortest natural wording.
   A label stays a label ("就寝中" = "Asleep"), a button a verb ("もどる" = "Back").
2. **Variables.** The letter is the person: p = the player, t / k / a = Takane / Kobayakawa / Anegasaki.
   `[p-name2]`: what the heroine calls the player: keep it where a name fits. `[t-name2]`, `[k-name2]`,
   `[a-name2]`: what the player calls that heroine: keep it, or write "you" when he is talking to her.
   `[p-name1]` (his word for himself, 俺 / 僕) and `[k-name1]` (Rinko's word for herself, わたし / リンコ):
   never keep them, write I / me / my. `%d`, `%s`: numbers / words the game fills in: keep every one, in the same
   order as the Japanese, and build the sentence around that order: "%d月、第%d日曜日より開催。" (month, then
   which Sunday) = "Starts month %d, Sunday no. %d." (not "the %dth Sunday of month %d", which swaps them).
3. **Mail** (gd_textres04, groups 0.0 / 0.2 / 0.4 = Manaka / Rinko / Nene; 0.1 / 0.3 / 0.5 = the player's
   reply options and quiz answers for that heroine, one line each): text messages from the heroine to the player. Casual, warm, in her voice
   (Manaka earnest and gentle, Rinko blunt, Nene teasing big sister). Reply options are short. Keep emoticons
   such as (^_^) as written; write Japanese-only symbols as ASCII.
4. **Names, places, items.** Use the glossary in PROMPT.md. Shops, venues and events with katakana English names
   get the English (パルファムLIVE = Parfum LIVE). Food and Japanese things keep a recognisable name
   (たこ焼き = takoyaki).
5. **Heroine names lists** (gd_textres07 group 0.2 / 0.3 / 0.4): what the player may call each heroine:
   romanise as written, keep honorifics (高嶺さん = Takane-san, マナカ = Manaka, まなっち = Manacchi).
   Group 0.0 (俺 / 僕) is the player's pronoun: "I" for both. Group 0.1 is how a heroine refers to herself:
   わたし / 私 = "I", 愛花 = "Manaka", リンコ / 凛子 = "Rinko", 寧々 = "Nene".
6. **Same line, same English**: lines repeated in the file get the same translation.
7. **Only ASCII** in `en` (plus ▼ and ♪ where the Japanese has them).
