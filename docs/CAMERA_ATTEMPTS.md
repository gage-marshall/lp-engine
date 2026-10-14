# Camera and motion attempts

This is a history of **attempted engine changes**, not a specification of the
native renderer.  A component can agree with a RAM capture and still be wrong
when composed with the rest of the model, animation, and draw pipeline.  In
particular, none of the attempts below has reproduced the supplied target
frame, so they must not be reintroduced merely because an isolated test
passes.

The engine source is restored to commit `2605f45` after this document was
written.  That commit is the baseline; this document intentionally remains as
an audit record.

## Visual checkpoint index

These images are the visual record for the specific scene under discussion.
They are useful for rejecting a proposed transform, but not by themselves for
inferring a replacement transform.

| State | Frame | Interpretation |
| --- | --- | --- |
| Native emulator reference | `export/scene/user_target.png` | Clean target frame: Nene is in the waitress uniform and cap, facing mostly toward the viewer with closed eyes. |
| Baseline `2605f45` | `export/scene/col2_024.50.png` | Baseline engine render; it has a recognizably front-facing character but does not reproduce the target framing, pose, costume, or expression. |
| Attempts through `0bd72da` | `export/scene/col6_057.50.png` | Earlier committed camera/body-yaw/head-turn experiment; visually different from both the baseline and the native target. |
| Latest uncommitted attempt | `export/scene/audit_final_009.00.png` | Reconstructed-transform render: character is rear-facing, which falsifies the proposed composed basis. |
| Latest uncommitted attempted correction | `export/scene/audit_basis_fixed_009.00.png` | Added half-turn correction: character is front-facing but clipped/displaced and still wrong; this correction was backed out before the full rollback. |

The comparison is intentionally chronological: no image in this table is
evidence that a nearby transform sign, angle, or offset is correct.  The
target reference is the native output; the remaining four images show that the
three engine states have all failed in different ways.

## Terminology and evidence boundary

For preset 3014, the captures establish several isolated facts:

- native camera eye/target and rolled look-at coefficients can be reproduced
  from the saved camera structure;
- camera `+0xb4` produces a Y-rotation matrix that appears in the native draw
  state; and
- active parts have a nonzero native base translation in some captures.

They do **not** establish the port's complete coordinate conversion, the
ordering of native model/joint/part callbacks, mesh/SBC assembly, or the final
native vertex position for the target frame.  The current target also has
unresolved costume/accessory, face, eye, timing, and backdrop differences.

## Committed appearance-driven attempts (already reverted)

### `32b7781` — decoded projection and rolled camera

Changed the scene player to use a decoded projection and a rolled camera,
plus portrait aspect handling.  The projection parameters were evidence-led,
but this did not by itself validate the model-to-camera basis.

### `939cc97` — camera/model half-turn convention

Changed the relation of preset eye/target and the model half-turn, including
the camera X-sign interpretation.  This was based on attempting to reconcile
the rendered portrait with recorded imagery.  The sign/order was not proven by
a final-vertex native comparison.

### `dcaf436` — heroine placement at preset target X

Placed the heroine at the preset target X as a camera-relative stage rule.
That is a scene-appearance hypothesis, not a traced actor-placement rule.

### `0bd72da` — body yaw and head turn from the preset angle

Applied the preset angle as heroine body yaw and added a camera-directed,
clamped head turn.  A RAM capture did confirm a global Y-rotation associated
with this angle, but it did not prove that this rotation should be added to the
port's existing pose and camera transforms in that way.  The head turn was a
visual compensation.

### `2605f45` — rollback baseline

Reverted the preceding camera X-sign, stage-X, body-yaw, head-turn, and
per-scene appearance defaults.  It retained notes distinguishing decoded facts
from appearance-derived assumptions.  This is the source state restored below.

## Uncommitted camera/motion implementation attempt (removed)

The following was an uncommitted patch on top of `2605f45`.  It modified ten
tracked engine files (`+415/-113`) and added two audit documents plus an oracle
test.  The patch was rendered against the target and did not match: first it
put the character rear-facing; an attempted Y-half-turn correction put a
front-facing partial character offscreen.  That correction was itself backed
out before this final rollback.

### Camera and renderer changes

- Added a `portrait_transform` that combined a reconstructed rolled look-at,
  a portrait basis conversion, the captured angle-word Y rotation, and a base
  translation.
- Replaced the existing renderer's model half-turn / reflected-camera-Z view
  construction with that combined helper.
- Forced parsed target Z to zero based on a reading of camera transition code.
- Added quarter-sine camera interpolation, a `0x5000` angle-wrap rule, and
  revised near/far values.
- Rotated light vectors and changed normal handling in the camera shader path.

The helper agreed with selected raw matrices after applying its own assumed
portrait conversion.  This is circular as an end-to-end oracle: it validates
the helper's chosen composition, not native vertices.  The bad render rejects
it as a complete port transform.

### Motion, animation, and placement changes

- Added a script-motion-key resolver using the extracted model-class table.
  This uncovered a genuine distinction between script keys and animation
  indices (for example, observed Nene keys 582/587 map to animation 172 for
  the normal class).  Its incorporation into the scene player was nevertheless
  part of the unvalidated combined patch.
- Added three-layer blend plumbing, weighted-row joint blending, motion modes,
  queue timeout handling, and delayed mood handling.
- Added actor/base-translation table handling and diagnostic `--base-y` and
  `--bg-scroll` options.
- Added a packed-tile backdrop viewport with explicit scroll support.

Some of these trace code paths or file formats, but none was verified against
the exact target-frame actor state.  They are removed with the combined patch
rather than retained as presumed-correct behavior.

### Why it failed as a method

The patch combined multiple unknowns—coordinate basis, global rotation order,
animation choice and phase, blend behavior, actor placement, wardrobe
assembly, facial state, and background scroll—then checked intermediate
matrices.  A successful matrix check cannot identify which of those unknowns
caused a final-frame mismatch.  The target's missing cap/accessory and
different face/hair also mean a visual result cannot isolate camera behavior.

## Current state after rollback

- Tracked engine files match `2605f45`.
- The uncommitted implementation files and its two generated audit documents
  are removed.
- This file remains as the explicit record of what was tried and why it is not
  a valid fix.

## Required oracle before another transform change

Capture the exact target scene/time in the native runtime and trace at least
one named mesh vertex through: model-space coordinates, sampled joint result,
part callback/base transform, global angle rotation, view/projection, and
portrait output coordinates.  Compare every stage numerically with the Rust
pipeline.  Only then change a transform, one evidenced stage at a time.
