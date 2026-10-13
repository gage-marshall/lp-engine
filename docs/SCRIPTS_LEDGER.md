# Scripts understanding: closure ledger

Each item that was once listed as open, with the evidence that closes it (all re-runnable).

| Earlier open item | Closed by | Evidence |
|---|---|---|
| ~20 of 107 condition ops not checked line by line | `tools/cond_check.py` (native ARM in Unicorn vs `cond_eval.py`) | 107/107 ops clean, 20 seeds; `cond_check_files.py`: 4,959 whole-record evaluations, 0 mismatches |
| 31 of 36 stages without id tables | `tools/stage_tables.py`, `stage_eval.py`, `stage_check.py` | 50 tables dumped (`content/engine/stage_tables.json`, local); all 36 stages 0 mismatches over 38,304 states each |
| 8 stage handlers not functions in Ghidra | `fid/scripts/DecompileAt.java` | created in memory and decompiled; stage 28 size corrected to 304 bytes; ported and verified like the rest |
| session layers | `tools/pipeline_check.py 100` | UpdateTimed 100 cases, Clear 500 cases (all 36 stages reached), 0 mismatches |
| RNG policy | `docs/EVENT_STAGES.md` | `Rand_Next` 64-bit LCG seeded once at boot from the RTC; op 148 uses the 32-bit LCG; call order matters, so every condition is evaluated (no short circuit) |
| flag ids / calendar | `CONDITIONS.md`, `CALENDAR.md` | scope bits, 175 codes x 28 years probed |

## Honest residue (not closed)
* 69 commands (L/E tier, 4.2% of call sites) have a specified native effect chain but no behavioural verification; verifying them needs the Rust engine (or a trace diff) to exist.
* Medium-tier argument units (fade/blend frames, etc.) are inferred; only `wait` was measured.
* Leaf values under the verified layers are documented, not re-derived.
* A recorded run replays exactly only with its boot RTC value.
