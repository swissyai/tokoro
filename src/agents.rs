use crate::platform;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub struct Detected {
    pub snippet_name: &'static str,
    pub display_name: &'static str,
    pub evidence: &'static str,
    pub direct: bool,
}

pub struct Inventory {
    detected: Vec<Detected>,
}

impl Inventory {
    pub fn detect() -> Self {
        let mut detected = Vec::new();
        let mut add_command = |command, snippet_name, display_name, direct| {
            if command_exists(command) {
                detected.push(Detected {
                    snippet_name,
                    display_name,
                    evidence: "command found in PATH",
                    direct,
                });
            }
        };
        add_command("pi", "pi", "Pi", true);
        add_command("opencode", "OpenCode", "OpenCode", true);
        add_command("codex", "Codex CLI", "Codex CLI", true);
        add_command("claude", "Claude Code", "Claude Code", false);
        add_command("aider", "Aider", "Aider", true);
        add_command("nvim", "Neovim", "Neovim", true);

        if !detected.iter().any(|agent| agent.snippet_name == "Cursor") && cursor_app_exists() {
            detected.push(Detected {
                snippet_name: "Cursor",
                display_name: "Cursor",
                evidence: "desktop app found",
                direct: true,
            });
        }
        for (needle, snippet_name, display_name) in [
            ("continue", "Continue", "Continue"),
            ("saoudrizwan.claude-dev", "Cline", "Cline"),
            ("rooveterinaryinc.roo-cline", "Roo Code", "Roo Code"),
        ] {
            if extension_exists(needle)
                && !detected
                    .iter()
                    .any(|agent| agent.snippet_name == snippet_name)
            {
                detected.push(Detected {
                    snippet_name,
                    display_name,
                    evidence: "editor extension found",
                    direct: true,
                });
            }
        }
        Self { detected }
    }

    pub fn detected(&self) -> &[Detected] {
        &self.detected
    }

    pub fn has(&self, snippet_name: &str) -> bool {
        self.detected
            .iter()
            .any(|agent| agent.snippet_name == snippet_name)
    }

    pub fn direct_count(&self) -> usize {
        self.detected.iter().filter(|agent| agent.direct).count()
    }
}

fn command_exists(command: &str) -> bool {
    platform::command_exists(command)
}

fn cursor_app_exists() -> bool {
    command_exists("cursor")
        || [
            PathBuf::from("/Applications/Cursor.app"),
            home_dir().join("Applications").join("Cursor.app"),
            PathBuf::from("/opt/Cursor"),
            std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join("Programs")
                .join("Cursor")
                .join("Cursor.exe"),
        ]
        .iter()
        .any(|path| path.exists())
}

fn extension_exists(needle: &str) -> bool {
    [
        home_dir().join(".vscode/extensions"),
        home_dir().join(".cursor/extensions"),
        home_dir().join(".vscode-insiders/extensions"),
    ]
    .iter()
    .any(|directory| directory_contains(directory, needle))
}

fn directory_contains(directory: &Path, needle: &str) -> bool {
    fs::read_dir(directory).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .to_lowercase()
                .contains(needle)
        })
    })
}

fn home_dir() -> PathBuf {
    platform::home_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detected_agents_have_unique_connection_names() {
        let inventory = Inventory::detect();
        let mut names = inventory
            .detected()
            .iter()
            .map(|agent| agent.snippet_name)
            .collect::<Vec<_>>();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count);
    }
}
