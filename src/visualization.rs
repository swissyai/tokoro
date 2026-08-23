use crate::{platform, settings::VisualizationConfig};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) const PROFILE_SCHEMA: &str = "tokoro.visualization.v1";
const MAX_PROFILE_BYTES: u64 = 64 * 1024;
const PANEL_IDS: [&str; 12] = [
    "model",
    "capacity",
    "inventory",
    "next",
    "performance",
    "streams",
    "stages",
    "history",
    "memory",
    "interference",
    "bloat",
    "sources",
];

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Profile {
    pub(crate) schema: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) density: String,
    pub(crate) layout: String,
    pub(crate) graph_renderer: String,
    pub(crate) history_window: usize,
    pub(crate) panel_order: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct CustomEntry {
    pub(crate) file: String,
    pub(crate) profile: Option<Profile>,
    pub(crate) error: Option<String>,
}

impl Profile {
    fn builtin(
        name: &str,
        description: &str,
        density: &str,
        layout: &str,
        graph_renderer: &str,
        history_window: usize,
        panel_order: &[&str],
    ) -> Self {
        Self {
            schema: PROFILE_SCHEMA.into(),
            name: name.into(),
            description: description.into(),
            density: density.into(),
            layout: layout.into(),
            graph_renderer: graph_renderer.into(),
            history_window,
            panel_order: panel_order.iter().map(|value| value.to_string()).collect(),
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.schema != PROFILE_SCHEMA {
            return Err(format!(
                "schema must be {PROFILE_SCHEMA}; found '{}'",
                self.schema
            ));
        }
        if self.name.is_empty()
            || self.name.len() > 32
            || !self
                .name
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_'))
        {
            return Err(
                "name must use 1-32 lowercase letters, digits, hyphens, or underscores".into(),
            );
        }
        if self.description.trim().is_empty() || self.description.chars().count() > 160 {
            return Err("description must contain 1-160 characters".into());
        }
        if !matches!(self.density.as_str(), "compact" | "standard" | "expanded") {
            return Err("density must be compact, standard, or expanded".into());
        }
        if !matches!(
            self.layout.as_str(),
            "balanced" | "dense" | "focused" | "stacked"
        ) {
            return Err("layout must be balanced, dense, focused, or stacked".into());
        }
        if !matches!(self.graph_renderer.as_str(), "unicode" | "blocks" | "ascii") {
            return Err("graph_renderer must be unicode, blocks, or ascii".into());
        }
        if !(24..=240).contains(&self.history_window) {
            return Err("history_window must be from 24 to 240".into());
        }
        if self.panel_order.len() != PANEL_IDS.len() {
            return Err(format!(
                "panel_order must contain each of the {} panel ids exactly once",
                PANEL_IDS.len()
            ));
        }
        let mut seen = HashSet::new();
        for panel in &self.panel_order {
            if !PANEL_IDS.contains(&panel.as_str()) {
                return Err(format!("unknown panel id '{panel}'"));
            }
            if !seen.insert(panel.as_str()) {
                return Err(format!("duplicate panel id '{panel}'"));
            }
        }
        if let Some(missing) = PANEL_IDS.iter().find(|panel| !seen.contains(**panel)) {
            return Err(format!("panel_order is missing '{missing}'"));
        }
        Ok(())
    }

    pub(crate) fn is_focused(&self) -> bool {
        self.layout == "focused"
    }

    pub(crate) fn is_stacked(&self) -> bool {
        self.layout == "stacked"
    }
}

pub(crate) fn builtins() -> Vec<Profile> {
    vec![
        Profile::builtin(
            "tokoro",
            "Calm default with primary readings and supporting evidence.",
            "standard",
            "balanced",
            "unicode",
            80,
            &[
                "model",
                "capacity",
                "inventory",
                "next",
                "performance",
                "streams",
                "stages",
                "history",
                "memory",
                "interference",
                "bloat",
                "sources",
            ],
        ),
        Profile::builtin(
            "operator",
            "Dense instrumentation for experienced local inference operators.",
            "compact",
            "dense",
            "blocks",
            160,
            &[
                "model",
                "next",
                "capacity",
                "inventory",
                "streams",
                "performance",
                "stages",
                "history",
                "memory",
                "interference",
                "sources",
                "bloat",
            ],
        ),
        Profile::builtin(
            "focus",
            "One evidence panel at a time with full-width navigation.",
            "expanded",
            "focused",
            "unicode",
            80,
            &[
                "next",
                "model",
                "capacity",
                "inventory",
                "performance",
                "stages",
                "streams",
                "history",
                "memory",
                "interference",
                "bloat",
                "sources",
            ],
        ),
        Profile::builtin(
            "mono",
            "Portable ordering and ASCII graphs without Unicode graph dependence.",
            "compact",
            "balanced",
            "ascii",
            40,
            &[
                "model",
                "next",
                "capacity",
                "inventory",
                "performance",
                "stages",
                "streams",
                "history",
                "memory",
                "sources",
                "interference",
                "bloat",
            ],
        ),
    ]
}

pub(crate) fn builtin(name: &str) -> Option<Profile> {
    builtins().into_iter().find(|profile| profile.name == name)
}

fn profiles_dir() -> PathBuf {
    platform::config_home()
        .join("tokoro")
        .join("visualizations")
}

fn safe_stored_file(file: &str) -> Result<&str, String> {
    let path = Path::new(file);
    if file.is_empty()
        || path.is_absolute()
        || path.components().count() != 1
        || path.extension().and_then(|value| value.to_str()) != Some("toml")
    {
        return Err("custom profile reference must be one local .toml filename".into());
    }
    Ok(file)
}

pub(crate) fn load_path(path: &Path) -> Result<Profile, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| "profile file could not be opened".to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("profile input must be a regular file, not a symlink".into());
    }
    if metadata.len() > MAX_PROFILE_BYTES {
        return Err("profile exceeds the 64 KiB size limit".into());
    }
    let text =
        fs::read_to_string(path).map_err(|_| "profile must be readable UTF-8".to_string())?;
    let profile = toml::from_str::<Profile>(&text)
        .map_err(|error| format!("profile TOML is invalid: {error}"))?;
    profile.validate()?;
    if builtin(&profile.name).is_some() {
        return Err(format!(
            "custom profile name '{}' is reserved by an immutable built-in",
            profile.name
        ));
    }
    Ok(profile)
}

pub(crate) fn resolve(config: &VisualizationConfig) -> Result<Profile, String> {
    let selected = if config.profile.is_empty() {
        "tokoro"
    } else {
        &config.profile
    };
    if let Some(profile) = builtin(selected) {
        return Ok(profile);
    }
    let Some(custom_name) = selected.strip_prefix("custom:") else {
        return Err(format!("unknown visualization profile '{selected}'"));
    };
    let file = safe_stored_file(&config.custom_file)?;
    let profile = load_path(&profiles_dir().join(file))?;
    if profile.name != custom_name {
        return Err(format!(
            "custom profile name '{}' does not match active name '{custom_name}'",
            profile.name
        ));
    }
    Ok(profile)
}

pub(crate) fn custom_entries() -> Vec<CustomEntry> {
    let Ok(entries) = fs::read_dir(profiles_dir()) else {
        return Vec::new();
    };
    let mut paths = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("toml"))
        .collect::<Vec<_>>();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let file = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("invalid.toml")
                .to_string();
            match load_path(&path) {
                Ok(profile) => CustomEntry {
                    file,
                    profile: Some(profile),
                    error: None,
                },
                Err(error) => CustomEntry {
                    file,
                    profile: None,
                    error: Some(error),
                },
            }
        })
        .collect()
}

pub(crate) fn install_custom(profile: &Profile, replace: bool) -> Result<String, String> {
    profile.validate()?;
    if builtin(&profile.name).is_some() {
        return Err(format!(
            "'{}' is an immutable built-in profile name",
            profile.name
        ));
    }
    let directory = profiles_dir();
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let filename = format!("{}.toml", profile.name);
    let destination = directory.join(&filename);
    let text = toml::to_string_pretty(profile).map_err(|error| error.to_string())?;
    if let Ok(metadata) = fs::symlink_metadata(&destination) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(format!(
                "stored profile '{}' is not a regular file",
                profile.name
            ));
        }
        if fs::read_to_string(&destination).ok().as_deref() == Some(text.as_str()) {
            return Ok(filename);
        }
        if !replace {
            return Err(format!(
                "custom profile '{}' already exists; inspect it and add --replace to overwrite",
                profile.name
            ));
        }
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let temporary = directory.join(format!(
        ".{}.{}.{}.tmp",
        profile.name,
        std::process::id(),
        nonce
    ));
    let mut temporary_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    if let Err(error) = temporary_file
        .write_all(text.as_bytes())
        .and_then(|_| temporary_file.sync_all())
    {
        let _ = fs::remove_file(&temporary);
        return Err(error.to_string());
    }
    drop(temporary_file);
    #[cfg(windows)]
    let backup = if replace && destination.exists() {
        let backup = directory.join(format!(".{}.{}.backup", profile.name, nonce));
        fs::rename(&destination, &backup).map_err(|error| error.to_string())?;
        Some(backup)
    } else {
        None
    };
    if let Err(error) = fs::rename(&temporary, &destination) {
        let _ = fs::remove_file(&temporary);
        #[cfg(windows)]
        if let Some(backup) = &backup {
            let _ = fs::rename(backup, &destination);
        }
        return Err(error.to_string());
    }
    #[cfg(windows)]
    if let Some(backup) = backup {
        let _ = fs::remove_file(backup);
    }
    Ok(filename)
}

pub(crate) fn schema_value() -> serde_json::Value {
    serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": PROFILE_SCHEMA,
        "title": "Tokoro visualization profile",
        "type": "object",
        "additionalProperties": false,
        "required": [
            "schema", "name", "description", "density", "layout",
            "graph_renderer", "history_window", "panel_order"
        ],
        "properties": {
            "schema": {"const": PROFILE_SCHEMA},
            "name": {"type": "string", "pattern": "^[a-z0-9_-]{1,32}$"},
            "description": {"type": "string", "minLength": 1, "maxLength": 160},
            "density": {"enum": ["compact", "standard", "expanded"]},
            "layout": {"enum": ["balanced", "dense", "focused", "stacked"]},
            "graph_renderer": {"enum": ["unicode", "blocks", "ascii"]},
            "history_window": {"type": "integer", "minimum": 24, "maximum": 240},
            "panel_order": {
                "type": "array",
                "minItems": PANEL_IDS.len(),
                "maxItems": PANEL_IDS.len(),
                "uniqueItems": true,
                "items": {"enum": PANEL_IDS},
            },
        },
        "boundaries": {
            "palette": "separate; profiles never contain colors",
            "code": "data-only TOML; executable plugins are not loaded",
            "builtins": "immutable",
        },
    })
}

pub(crate) fn preview(profile: &Profile) -> String {
    let graph = match profile.graph_renderer.as_str() {
        "ascii" => " .:-=+*#@",
        "blocks" => " ▏▎▍▌▋▊▉█",
        _ => " ▁▂▃▄▅▆▇█",
    };
    let panel_rows = profile
        .panel_order
        .chunks(4)
        .map(|panels| format!("  {}", panels.join(" -> ")))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "TOKORO VISUALIZATION PREVIEW\n\n{}\n{}\n\nlayout          {}\ndensity         {}\ngraph renderer  {}  {}\nhistory window  {} samples\n\npanel order\n{}\n\npalette         unchanged\ncustody         local versioned TOML\n",
        profile.name,
        profile.description,
        profile.layout,
        profile.density,
        profile.graph_renderer,
        graph,
        profile.history_window,
        panel_rows
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_complete_valid_and_color_free() {
        for profile in builtins() {
            profile.validate().expect("valid built-in");
            let text = toml::to_string(&profile).expect("serialize profile");
            assert!(!text.contains("color"));
            assert!(!text.contains("palette"));
        }
    }

    #[test]
    fn strict_parser_rejects_palette_and_unknown_fields() {
        let mut text = toml::to_string(&builtin("tokoro").expect("profile")).expect("serialize");
        text.push_str("palette = 'cyan'\n");
        let path = std::env::temp_dir().join(format!(
            "tokoro-invalid-profile-{}.toml",
            std::process::id()
        ));
        fs::write(&path, text).expect("write fixture");
        let error = load_path(&path).expect_err("unknown field rejected");
        let _ = fs::remove_file(path);
        assert!(error.contains("unknown field"), "{error}");
    }

    #[test]
    fn duplicate_or_missing_panels_fail_visibly() {
        let mut profile = builtin("operator").expect("profile");
        profile.panel_order[1] = profile.panel_order[0].clone();
        assert!(profile
            .validate()
            .expect_err("invalid order")
            .contains("duplicate"));
    }
}
