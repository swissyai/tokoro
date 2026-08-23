use crate::platform;
use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

pub(crate) use crate::platform::expand_home;

// ───────────────────────────── Config ─────────────────────────────

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
#[serde(default)]
pub(crate) struct Config {
    pub(crate) theme: ThemeConfig,
    pub(crate) visualization: VisualizationConfig,
    pub(crate) telemetry: TelemetryConfig,
    pub(crate) layout: LayoutConfig,
    pub(crate) benchmark: BenchmarkConfig,
    pub(crate) observability: ObservabilityConfig,
    pub(crate) connections: ConnectionsConfig,
    pub(crate) bloat: BloatConfig,
    pub(crate) server: ServerConfig,
    pub(crate) intro: IntroConfig,
    pub(crate) onboarding: OnboardingConfig,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
#[serde(default)]
pub(crate) struct ThemeConfig {
    pub(crate) name: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(default)]
pub(crate) struct VisualizationConfig {
    pub(crate) profile: String,
    pub(crate) custom_file: String,
}

impl Default for VisualizationConfig {
    fn default() -> Self {
        Self {
            profile: "tokoro".into(),
            custom_file: String::new(),
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
#[serde(default)]
pub(crate) struct OnboardingConfig {
    pub(crate) completed: bool,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(default)]
pub(crate) struct IntroConfig {
    pub(crate) enabled: bool,
    pub(crate) style: String,
    pub(crate) duration_ms: u64,
    pub(crate) sound: String,
    pub(crate) motion: String,
    pub(crate) slogan: String,
    pub(crate) frames_path: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(default)]
pub(crate) struct TelemetryConfig {
    pub(crate) idle_ms: u64,
    pub(crate) active_ms: u64,
    pub(crate) log_path: String,
    pub(crate) ports: Vec<u16>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(default)]
pub(crate) struct LayoutConfig {
    pub(crate) panels: Vec<String>,
    pub(crate) density: String,
    pub(crate) default_view: String,
    pub(crate) hidden_panels: Vec<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(default)]
pub(crate) struct BenchmarkConfig {
    pub(crate) prompt_tokens: u32,
    pub(crate) gen_tokens: u32,
    pub(crate) runs: u32,
    pub(crate) sweep: Vec<u32>,
    pub(crate) concurrency: Vec<u32>,
    pub(crate) budgets: Vec<WorkloadBudget>,
}

impl BenchmarkConfig {
    pub(crate) fn concurrency_levels(&self) -> Vec<u32> {
        let mut levels = self
            .concurrency
            .iter()
            .copied()
            .filter(|level| (1..=8).contains(level))
            .collect::<Vec<_>>();
        levels.sort_unstable();
        levels.dedup();
        if levels.is_empty() {
            vec![1, 2, 4, 8]
        } else {
            levels
        }
    }

    pub(crate) fn budget_for(&self, workload: &str) -> Option<&WorkloadBudget> {
        self.budgets
            .iter()
            .find(|budget| budget.workload.eq_ignore_ascii_case(workload))
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
#[serde(default)]
pub(crate) struct WorkloadBudget {
    pub(crate) workload: String,
    pub(crate) max_ttft_p95_ms: Option<f64>,
    pub(crate) max_tpot_p95_ms: Option<f64>,
    pub(crate) max_end_to_end_p95_ms: Option<f64>,
    pub(crate) min_decode_tokens_per_second: Option<f64>,
    pub(crate) min_system_tokens_per_second: Option<f64>,
    pub(crate) max_server_rss_gib: Option<f64>,
    pub(crate) max_swap_mib: Option<f64>,
    pub(crate) max_waiting_requests: Option<u64>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(default)]
pub(crate) struct ObservabilityConfig {
    pub(crate) focus: String,
    pub(crate) history_samples: usize,
    pub(crate) request_retention: usize,
}

impl ObservabilityConfig {
    pub(crate) fn focus(&self) -> &str {
        match self.focus.as_str() {
            "latency" | "throughput" | "memory" | "speculation" => &self.focus,
            _ => "balanced",
        }
    }

    pub(crate) fn history_samples(&self) -> usize {
        self.history_samples.clamp(24, 240)
    }

    pub(crate) fn request_retention(&self) -> usize {
        self.request_retention.clamp(8, 128)
    }

    pub(crate) fn cycle_focus(&mut self) {
        self.focus = match self.focus() {
            "balanced" => "latency",
            "latency" => "throughput",
            "throughput" => "memory",
            "memory" => "speculation",
            _ => "balanced",
        }
        .into();
    }

    pub(crate) fn cycle_history_samples(&mut self) {
        self.history_samples = match self.history_samples() {
            24..=40 => 80,
            41..=80 => 160,
            _ => 40,
        };
    }

    pub(crate) fn cycle_request_retention(&mut self) {
        self.request_retention = match self.request_retention() {
            8..=16 => 32,
            17..=32 => 64,
            33..=64 => 128,
            _ => 16,
        };
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(default)]
pub(crate) struct ConnectionsConfig {
    pub(crate) favorites: Vec<String>,
}

impl Default for ConnectionsConfig {
    fn default() -> Self {
        Self {
            favorites: ["pi", "OpenCode", "Codex CLI", "Claude Code"]
                .iter()
                .map(|name| name.to_string())
                .collect(),
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(default)]
pub(crate) struct BloatConfig {
    pub(crate) scan_project: bool,
    pub(crate) project_dir: String,
}

impl Default for BloatConfig {
    fn default() -> Self {
        Self {
            scan_project: true,
            project_dir: ".".into(),
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(default)]
pub(crate) struct ServerConfig {
    pub(crate) command: String,
    pub(crate) args: String,
    pub(crate) port: u16,
    pub(crate) models_dir: String,
}

impl Default for IntroConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            style: "cursor-threshold".into(),
            duration_ms: 1480,
            sound: "off".into(),
            motion: "full".into(),
            slogan: String::new(),
            frames_path: String::new(),
        }
    }
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            idle_ms: 2000,
            active_ms: 250,
            log_path: platform::state_home()
                .join("tokoro")
                .join("server.log")
                .to_string_lossy()
                .into_owned(),
            ports: vec![8080, 11434, 1234, 8000],
        }
    }
}
impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            panels: [
                "memory",
                "performance",
                "stages",
                "history",
                "interference",
                "streams",
                "bloat",
                "sources",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            density: "standard".into(),
            default_view: "home".into(),
            hidden_panels: Vec::new(),
        }
    }
}
impl LayoutConfig {
    pub(crate) fn panel_visible(&self, name: &str) -> bool {
        !self.hidden_panels.iter().any(|hidden| hidden == name)
    }

    pub(crate) fn toggle_panel(&mut self, name: &str) {
        if self.panel_visible(name) {
            self.hidden_panels.push(name.to_string());
        } else {
            self.hidden_panels.retain(|hidden| hidden != name);
        }
    }
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            prompt_tokens: 512,
            gen_tokens: 128,
            runs: 5,
            sweep: vec![512, 2048, 8192],
            concurrency: vec![1, 2, 4, 8],
            budgets: Vec::new(),
        }
    }
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            focus: "balanced".into(),
            history_samples: 80,
            request_retention: 32,
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        let (command, args) = platform::default_managed_server();
        Self {
            command,
            args,
            port: 8080,
            models_dir: platform::default_models_dir()
                .to_string_lossy()
                .into_owned(),
        }
    }
}

fn config_path() -> PathBuf {
    platform::config_home().join("tokoro").join("config.toml")
}

pub(crate) fn load_config() -> Config {
    fs::read_to_string(config_path())
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

pub(crate) fn save_config(cfg: &Config) -> Result<(), String> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let text = toml::to_string_pretty(cfg).map_err(|error| error.to_string())?;
    fs::write(path, text).map_err(|error| error.to_string())
}

// ───────────────────────────── Theme ─────────────────────────────

#[derive(Clone)]
pub(crate) struct Theme {
    pub(crate) fg: Color,
    pub(crate) dim: Color,
    pub(crate) ok: Color,
    pub(crate) warn: Color,
    pub(crate) err: Color,
    pub(crate) accent: Color,
    pub(crate) kv: Color,
    pub(crate) weights: Color,
    pub(crate) bg: Option<Color>,
}

fn configured_ghostty_theme() -> Option<String> {
    let path = platform::config_home().join("ghostty").join("config");
    let content = fs::read_to_string(path).ok()?;
    content.lines().find_map(|line| {
        let line = line.split('#').next()?.trim();
        let (key, value) = line.split_once('=')?;
        if key.trim() == "theme" {
            Some(value.trim().trim_matches('"').to_string())
        } else {
            None
        }
    })
}

pub(crate) fn theme_choices() -> Vec<String> {
    let mut choices = vec![
        "auto".into(),
        "classic".into(),
        "tokoro".into(),
        "operator".into(),
        "mono".into(),
        "terminal".into(),
    ];
    for dir in platform::ghostty_theme_dirs() {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                if entry.path().is_file() {
                    if let Some(name) = entry.file_name().to_str() {
                        choices.push(name.to_string());
                    }
                }
            }
        }
    }
    choices.sort();
    choices.dedup();
    choices
}

impl Theme {
    fn terminal_default() -> Self {
        Self {
            fg: Color::Reset,
            dim: Color::DarkGray,
            ok: Color::Green,
            warn: Color::Yellow,
            err: Color::Red,
            accent: Color::Cyan,
            kv: Color::Magenta,
            weights: Color::Blue,
            bg: None,
        }
    }

    fn tokoro() -> Self {
        Self {
            fg: Color::Rgb(244, 247, 247),
            dim: Color::Rgb(102, 114, 118),
            ok: Color::Rgb(145, 199, 160),
            warn: Color::Rgb(214, 196, 119),
            err: Color::Rgb(219, 107, 107),
            accent: Color::Rgb(89, 217, 232),
            kv: Color::Rgb(142, 134, 201),
            weights: Color::Rgb(113, 148, 196),
            bg: Some(Color::Rgb(3, 5, 6)),
        }
    }

    fn operator() -> Self {
        Self {
            fg: Color::Rgb(217, 222, 212),
            dim: Color::Rgb(104, 112, 102),
            ok: Color::Rgb(169, 199, 157),
            warn: Color::Rgb(214, 196, 119),
            err: Color::Rgb(219, 107, 107),
            accent: Color::Rgb(169, 199, 157),
            kv: Color::Rgb(142, 134, 201),
            weights: Color::Rgb(113, 148, 196),
            bg: Some(Color::Rgb(2, 4, 3)),
        }
    }

    fn mono() -> Self {
        Self {
            fg: Color::Reset,
            dim: Color::DarkGray,
            ok: Color::Reset,
            warn: Color::Reset,
            err: Color::Reset,
            accent: Color::Reset,
            kv: Color::Reset,
            weights: Color::Reset,
            bg: None,
        }
    }

    fn ghostty(name: &str) -> Option<Self> {
        let content = platform::ghostty_theme_dirs()
            .into_iter()
            .map(|directory| directory.join(name))
            .find_map(|path| fs::read_to_string(path).ok())?;
        Self::parse_ghostty(&content)
    }

    fn parse_ghostty(content: &str) -> Option<Self> {
        let mut pal: [Option<Color>; 16] = [None; 16];
        let mut bg = None;
        let mut fg = None;
        let hex = |s: &str| -> Option<Color> {
            let s = s.trim().trim_start_matches('#');
            if s.len() != 6 {
                return None;
            }
            Some(Color::Rgb(
                u8::from_str_radix(&s[0..2], 16).ok()?,
                u8::from_str_radix(&s[2..4], 16).ok()?,
                u8::from_str_radix(&s[4..6], 16).ok()?,
            ))
        };
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') || !line.contains('=') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let (k, v) = (k.trim(), v.trim());
            if k == "palette" {
                if let Some((n, c)) = v.split_once('=') {
                    if let (Ok(i), Some(c)) = (n.trim().parse::<usize>(), hex(c)) {
                        if i < 16 {
                            pal[i] = Some(c);
                        }
                    }
                }
            } else if k == "background" {
                bg = hex(v);
            } else if k == "foreground" {
                fg = hex(v);
            }
        }
        Some(Self {
            fg: fg.unwrap_or(Color::Reset),
            dim: pal[8].unwrap_or(Color::DarkGray),
            ok: pal[2].unwrap_or(Color::Green),
            warn: pal[3].unwrap_or(Color::Yellow),
            err: pal[1].unwrap_or(Color::Red),
            accent: pal[6].unwrap_or(Color::Cyan),
            kv: pal[5].unwrap_or(Color::Magenta),
            weights: pal[4].unwrap_or(Color::Blue),
            bg,
        })
    }

    pub(crate) fn load(cfg: &ThemeConfig) -> Self {
        let name = if cfg.name.is_empty() || cfg.name == "auto" {
            configured_ghostty_theme()
        } else {
            Some(cfg.name.clone())
        };

        match name.as_deref() {
            None | Some("classic" | "terminal") => Self::terminal_default(),
            Some("tokoro") => Self::tokoro(),
            Some("operator") => Self::operator(),
            Some("mono") => Self::mono(),
            Some(name) => Self::ghostty(name).unwrap_or_else(Self::terminal_default),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_party_themes_are_available_without_terminal_theme_files() {
        let choices = theme_choices();
        for name in ["classic", "tokoro", "operator", "mono"] {
            assert!(choices.iter().any(|choice| choice == name), "{name}");
        }

        let operator = Theme::load(&ThemeConfig {
            name: "operator".into(),
        });
        assert_eq!(operator.accent, Color::Rgb(169, 199, 157));
        assert_eq!(operator.bg, Some(Color::Rgb(2, 4, 3)));

        let mono = Theme::load(&ThemeConfig {
            name: "mono".into(),
        });
        assert_eq!(mono.accent, Color::Reset);
        assert_eq!(mono.bg, None);
    }
}
