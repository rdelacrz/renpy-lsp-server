//! Loads and deserializes embedded JSON asset files into [`GameData`].
//!
//! The assets are included at compile time via `include_str!` so no runtime filesystem
//! access is required.

use std::collections::HashMap;

/// Entry for a Ren'Py function/config/internal from renpy.json.
/// The JSON stores arrays: [storage, kind, args, parentClass, accessKind, docstring]
#[derive(Debug, Clone, Default)]
pub struct RenpyEntry {
    pub storage: Option<String>,
    pub kind: Option<String>,
    pub args: Option<String>,
    pub parent_class: Option<String>,
    pub access_kind: Option<String>,
    pub docstring: Option<String>,
}

/// Loaded data from JSON asset files.
#[derive(Debug, Clone, Default)]
pub struct GameData {
    /// Entries under the `renpy.*` namespace (functions, classes).
    pub renpy: HashMap<String, RenpyEntry>,
    /// Entries under the `config.*` namespace (configuration variables).
    pub config: HashMap<String, RenpyEntry>,
    /// Internal/undocumented API entries.
    pub internal: HashMap<String, RenpyEntry>,
    /// Keyword → pipe-delimited completion suffixes (from renpyauto.json).
    pub auto_complete: HashMap<String, String>,
}

/// Embedded JSON containing the full Ren'Py API catalog (renpy, config, internal namespaces).
const RENPY_JSON: &str = include_str!("../../assets/renpy.json");
/// Embedded JSON containing keyword auto-completion mappings.
const RENPYAUTO_JSON: &str = include_str!("../../assets/renpyauto.json");

/// Parse a JSON array entry into a structured [`RenpyEntry`].
fn parse_entry(arr: &[serde_json::Value]) -> RenpyEntry {
    let get = |i: usize| -> Option<String> {
        arr.get(i)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    };
    RenpyEntry {
        storage: get(0),
        kind: get(1),
        args: get(2),
        parent_class: get(3),
        access_kind: get(4),
        docstring: get(5),
    }
}

/// Load game data from embedded asset files (compile-time inclusion).
pub fn load_game_data() -> crate::error::Result<GameData> {
    let raw: HashMap<String, HashMap<String, Vec<serde_json::Value>>> =
        serde_json::from_str(RENPY_JSON)?;

    let mut data = GameData::default();

    if let Some(map) = raw.get("config") {
        for (k, v) in map {
            data.config.insert(k.clone(), parse_entry(v));
        }
    }
    if let Some(map) = raw.get("renpy") {
        for (k, v) in map {
            data.renpy.insert(k.clone(), parse_entry(v));
        }
    }
    if let Some(map) = raw.get("internal") {
        for (k, v) in map {
            data.internal.insert(k.clone(), parse_entry(v));
        }
    }

    let auto: HashMap<String, String> = serde_json::from_str(RENPYAUTO_JSON)?;
    data.auto_complete = auto;

    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_game_data_succeeds() {
        let data = load_game_data().unwrap();
        assert!(!data.config.is_empty());
        assert!(!data.renpy.is_empty());
        assert!(!data.auto_complete.is_empty());
    }

    #[test]
    fn test_config_entry_has_docstring() {
        let data = load_game_data().unwrap();
        let entry = data.config.get("config.afm_bonus").unwrap();
        assert!(entry.docstring.is_some());
    }

    #[test]
    fn test_auto_complete_has_entries() {
        let data = load_game_data().unwrap();
        assert!(data.auto_complete.contains_key("window"));
        assert_eq!(data.auto_complete.get("window").unwrap(), "auto|hide|show");
    }
}
