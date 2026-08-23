use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

pub(crate) fn os_version() -> String {
    sysinfo::System::long_os_version().unwrap_or_else(|| os_name().into())
}

pub(crate) fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("USERPROFILE")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .or_else(|| {
            let drive = env::var_os("HOMEDRIVE")?;
            let path = env::var_os("HOMEPATH")?;
            Some(PathBuf::from(drive).join(path))
        })
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

pub(crate) fn config_home() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            cfg!(windows)
                .then(|| env::var_os("APPDATA"))
                .flatten()
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| home_dir().join(".config"))
}

pub(crate) fn cache_home() -> PathBuf {
    env::var_os("XDG_CACHE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            cfg!(windows)
                .then(|| env::var_os("LOCALAPPDATA"))
                .flatten()
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| home_dir().join(".cache"))
}

pub(crate) fn state_home() -> PathBuf {
    env::var_os("XDG_STATE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            cfg!(windows)
                .then(|| env::var_os("LOCALAPPDATA"))
                .flatten()
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| home_dir().join(".local").join("state"))
}

pub(crate) fn expand_home(value: &str) -> PathBuf {
    if value == "~" {
        return home_dir();
    }
    if let Some(relative) = value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
    {
        return home_dir().join(relative);
    }
    PathBuf::from(value)
}

pub(crate) fn command_exists(command: &str) -> bool {
    let path = Path::new(command);
    if path.components().count() > 1 {
        return path.is_file();
    }
    let windows = cfg!(windows);
    let path_ext = env::var_os("PATHEXT").unwrap_or_else(|| OsString::from(".COM;.EXE;.BAT;.CMD"));
    let names = executable_names(command, windows, &path_ext.to_string_lossy());
    env::var_os("PATH").is_some_and(|paths| {
        env::split_paths(&paths)
            .any(|directory| names.iter().any(|name| directory.join(name).is_file()))
    })
}

fn executable_names(command: &str, windows: bool, path_ext: &str) -> Vec<OsString> {
    let mut names = vec![OsString::from(command)];
    if !windows || Path::new(command).extension().is_some() {
        return names;
    }
    for extension in path_ext.split(';').filter(|value| !value.is_empty()) {
        let extension = if extension.starts_with('.') {
            extension.to_string()
        } else {
            format!(".{extension}")
        };
        names.push(OsString::from(format!(
            "{}{}",
            command,
            extension.to_ascii_lowercase()
        )));
        names.push(OsString::from(format!(
            "{}{}",
            command,
            extension.to_ascii_uppercase()
        )));
    }
    names.sort();
    names.dedup();
    names
}

pub(crate) const fn os_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "linux") {
        "Linux"
    } else {
        "other"
    }
}

pub(crate) const fn memory_kind() -> &'static str {
    if cfg!(target_os = "macos") {
        "unified"
    } else {
        "system"
    }
}

pub(crate) const fn has_unified_memory() -> bool {
    cfg!(target_os = "macos")
}

pub(crate) fn default_models_dir() -> PathBuf {
    env::var_os("TOKORO_MODELS_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join("models"))
}

pub(crate) fn default_managed_server() -> (String, String) {
    #[cfg(target_os = "macos")]
    {
        let dspark = home_dir()
            .join("models")
            .join(".dspark-venv")
            .join("bin")
            .join("mlx-dspark");
        if dspark.is_file() {
            return (
                dspark.to_string_lossy().into_owned(),
                "serve --mode auto --no-thinking --max-batch 1 --model {model} --port {port}"
                    .into(),
            );
        }
    }
    if command_exists("llama-server") {
        return (
            "llama-server".into(),
            "--model {model} --port {port}".into(),
        );
    }
    (String::new(), String::new())
}

pub(crate) fn ghostty_theme_dirs() -> Vec<PathBuf> {
    let mut directories = vec![
        config_home().join("ghostty").join("themes"),
        PathBuf::from("/usr/share/ghostty/themes"),
        PathBuf::from("/usr/local/share/ghostty/themes"),
        PathBuf::from("/opt/homebrew/share/ghostty/themes"),
        PathBuf::from("/Applications/Ghostty.app/Contents/Resources/ghostty/themes"),
    ];
    directories.retain(|path| path.is_dir());
    directories.sort();
    directories.dedup();
    directories
}

pub(crate) fn cpu_name() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("sysctl")
            .args(["-n", "machdep.cpu.brand_string"])
            .output()
        {
            let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !value.is_empty() {
                return value;
            }
        }
    }
    let mut system = sysinfo::System::new();
    system.refresh_cpu_all();
    system
        .cpus()
        .first()
        .map(|cpu| cpu.brand().trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("{} {}", os_name(), std::env::consts::ARCH))
}

pub(crate) fn is_omarchy() -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    fs::read_to_string("/etc/os-release").is_ok_and(|content| {
        content
            .lines()
            .any(|line| line.to_ascii_lowercase().contains("omarchy"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_command_candidates_honor_pathext() {
        let names = executable_names("ollama", true, ".EXE;.CMD")
            .into_iter()
            .map(|value| value.to_string_lossy().to_ascii_lowercase())
            .collect::<Vec<_>>();
        assert!(names.contains(&"ollama.exe".to_string()));
        assert!(names.contains(&"ollama.cmd".to_string()));
    }

    #[test]
    fn unix_command_candidates_do_not_invent_extensions() {
        assert_eq!(
            executable_names("ollama", false, ".EXE"),
            vec![OsString::from("ollama")]
        );
    }

    #[test]
    fn explicit_paths_are_not_treated_as_path_searches() {
        assert!(!command_exists("./definitely-not-a-tokoro-command"));
    }
}
