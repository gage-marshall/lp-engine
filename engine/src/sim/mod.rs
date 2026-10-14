//! The scene simulation: the game's per-update logic for a heroine scene, deterministic and free of IO, wall-clock time
//! and rendering. One `SceneSim::step_frame` = one display frame of the DS (59.8261 Hz); logic updates run inside it on the
//! Capture3D cycle (`clock`). Output: a `SceneFrame` snapshot per update and the script/audio events, which presentation
//! (heroine, render, backdrop, audio) turns into pixels and sound. Reference: `sandbox/gx` (docs/RENDER_PRIMITIVES.md).
pub mod clock;
pub mod eyes;
pub mod fade;
pub mod lipsync;
pub mod motion;
pub mod scene;
pub mod sched;
