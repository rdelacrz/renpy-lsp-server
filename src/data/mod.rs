//! Static Ren'Py API data and keyword lists.
//!
//! Loads pre-compiled JSON assets (`renpy.json`, `renpyauto.json`) that describe the Ren'Py
//! API surface (functions, config variables, internal symbols) for completion, hover, and
//! signature help. Also provides constant keyword/property lists for contextual completion.

pub mod keywords;
pub mod loader;
