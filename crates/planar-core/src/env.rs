use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::error::{PlanarError, Result};
use crate::model::AppSettings;

/// Root directories scanned read-only for agent session trajectories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessRoots {
    /// Claude Code project transcripts root (`~/.claude/projects`).
    pub claude_projects: PathBuf,
    /// Codex active session rollouts root (`~/.codex/sessions`).
    pub codex_sessions: PathBuf,
    /// Codex archived session rollouts root (`~/.codex/archived_sessions`).
    pub codex_archived: PathBuf,
    /// Antigravity conversation brain root (`~/.gemini/antigravity/brain`).
    pub antigravity_brain: PathBuf,
    /// `DeepSeek` Harness (`dsh`) sessions root (`~/.dsh/sessions` or `$DSH_HOME/sessions`).
    pub dsh_sessions: PathBuf,
}

impl HarnessRoots {
    /// Resolves harness roots from environment overrides (`PLANAR_CLAUDE_DIR`,
    /// `PLANAR_CODEX_DIR`, `PLANAR_ANTIGRAVITY_DIR`, `PLANAR_DSH_DIR`, `DSH_HOME`)
    /// or the user's home directory.
    pub fn from_env_or_home() -> Result<Self> {
        let home = home_dir()?;

        let claude_projects =
            env_path("PLANAR_CLAUDE_DIR").unwrap_or_else(|| home.join(".claude").join("projects"));

        let codex_base = env_path("PLANAR_CODEX_DIR").unwrap_or_else(|| home.join(".codex"));
        let codex_sessions = codex_base.join("sessions");
        let codex_archived = codex_base.join("archived_sessions");

        let antigravity_brain = env_path("PLANAR_ANTIGRAVITY_DIR")
            .unwrap_or_else(|| home.join(".gemini").join("antigravity").join("brain"));

        let dsh_sessions = env_path("PLANAR_DSH_DIR").unwrap_or_else(|| {
            env_path("DSH_HOME").map_or_else(
                || home.join(".dsh").join("sessions"),
                |base| base.join("sessions"),
            )
        });

        Ok(Self {
            claude_projects,
            codex_sessions,
            codex_archived,
            antigravity_brain,
            dsh_sessions,
        })
    }

    /// Applies user-configured directory overrides from `settings.harness_paths`.
    #[must_use]
    pub fn effective_for(&self, settings: &AppSettings) -> Self {
        let mut out = self.clone();

        if let Some(p) = expand_override(&settings.harness_paths.claude) {
            out.claude_projects = p;
        }
        if let Some(p) = expand_override(&settings.harness_paths.antigravity) {
            out.antigravity_brain = p;
        }
        if let Some(p) = expand_override(&settings.harness_paths.dsh) {
            out.dsh_sessions = p;
        }
        if let Some(p) = expand_override(&settings.harness_paths.codex) {
            let nested = p.join("sessions");
            if nested.is_dir() {
                out.codex_sessions = nested;
                out.codex_archived = p.join("archived_sessions");
            } else {
                out.codex_sessions = p;
            }
        }

        out
    }

    /// Returns the primary display log root for canonical `harness` ID.
    pub fn primary_root_for(&self, harness: &str) -> &Path {
        match harness {
            "antigravity" => &self.antigravity_brain,
            "claude" => &self.claude_projects,
            "dsh" => &self.dsh_sessions,
            _ => &self.codex_sessions,
        }
    }
}

/// Maps canonical harness ID to its CLI executable binary name.
pub fn cli_binary_name_for_harness(harness: &str) -> Option<&'static str> {
    match harness {
        "antigravity" => Some("agy"),
        "claude" => Some("claude"),
        "codex" => Some("codex"),
        "dsh" => Some("dsh"),
        _ => None,
    }
}

fn shell_single_quote(raw: &str) -> String {
    format!("'{}'", raw.replace('\'', "'\\''"))
}

/// Builds a shell-safe command to resume a session in its source harness CLI,
/// prepending `cd '<cwd>' && ` when `cwd` exists on disk as a directory.
#[must_use]
pub fn build_resume_command(harness: &str, session_id: &str, cwd: Option<&str>) -> Option<String> {
    let raw_id = session_id
        .split_once(':')
        .map_or(session_id, |(_, rest)| rest);
    let quoted_id = shell_single_quote(raw_id);
    let base_cmd = match harness {
        "codex" => format!("codex resume {quoted_id}"),
        "claude" => format!("claude --resume {quoted_id}"),
        "dsh" => format!("dsh --resume {quoted_id}"),
        "antigravity" => format!("agy --conversation {quoted_id}"),
        _ => return None,
    };
    if let Some(dir) = cwd
        && Path::new(dir).is_dir()
    {
        return Some(format!("cd {} && {base_cmd}", shell_single_quote(dir)));
    }
    Some(base_cmd)
}

/// Resolves an executable binary path using `custom_override` first, then `hydrated_path`.
pub fn resolve_cli_binary(
    bin_name: &str,
    custom_override: &str,
    hydrated_path: &OsStr,
) -> Option<PathBuf> {
    let trimmed = custom_override.trim();
    if !trimmed.is_empty() {
        let candidate = expand_tilde_path(trimmed);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
        if candidate.is_absolute() {
            return None;
        }
    }

    for dir in std::env::split_paths(hydrated_path) {
        let candidate = dir.join(bin_name);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn expand_override(raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(expand_tilde_path(trimmed))
    }
}

fn expand_tilde_path(raw: &str) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    if raw == "~"
        && let Some(home) = dirs::home_dir()
    {
        return home;
    }
    PathBuf::from(raw)
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Resolves the SQLite database path from `PLANAR_DB_PATH` or `~/.planar/planar.db`.
pub fn default_db_path() -> Result<PathBuf> {
    if let Some(path) = env_path("PLANAR_DB_PATH") {
        return Ok(path);
    }
    let home = home_dir()?;
    Ok(home.join(".planar").join("planar.db"))
}

/// Hydrates `$PATH` using the macOS login shell (`/bin/zsh -ilc "printenv PATH"`)
/// and ensures standard user/Homebrew bin directories are included.
///
/// Always appends fallback tool directories (`/opt/homebrew/bin`, `~/.local/bin`,
/// `~/.cargo/bin`) so background CLI tools resolve reliably from Finder/Dock/Menu Bar.
pub fn hydrate_macos_path() -> OsString {
    let mut seen = HashSet::new();
    let mut entries: Vec<PathBuf> = Vec::new();

    if cfg!(target_os = "macos")
        && let Some(login_path) = read_zsh_login_path(Duration::from_millis(1500))
    {
        push_split_paths(&login_path, &mut seen, &mut entries);
    }

    if let Some(current) = std::env::var_os("PATH") {
        for p in std::env::split_paths(&current) {
            push_unique_path(p, &mut seen, &mut entries);
        }
    }

    for fallback in fallback_bin_dirs() {
        push_unique_path(fallback, &mut seen, &mut entries);
    }

    std::env::join_paths(entries).unwrap_or_else(|_| {
        std::env::var_os("PATH").unwrap_or_else(|| OsString::from("/usr/bin:/bin"))
    })
}

fn home_dir() -> Result<PathBuf> {
    dirs::home_dir()
        .ok_or_else(|| PlanarError::NotFound("could not determine user home directory".into()))
}

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn fallback_bin_dirs() -> Vec<PathBuf> {
    let mut dirs_out = Vec::new();
    if cfg!(target_os = "macos") {
        dirs_out.push(PathBuf::from("/opt/homebrew/bin"));
        dirs_out.push(PathBuf::from("/usr/local/bin"));
    }
    if let Some(home) = dirs::home_dir() {
        dirs_out.push(home.join(".local").join("bin"));
        dirs_out.push(home.join(".cargo").join("bin"));
    }
    dirs_out
}

fn push_split_paths(raw: &str, seen: &mut HashSet<PathBuf>, out: &mut Vec<PathBuf>) {
    for p in std::env::split_paths(raw) {
        push_unique_path(p, seen, out);
    }
}

fn push_unique_path(p: PathBuf, seen: &mut HashSet<PathBuf>, out: &mut Vec<PathBuf>) {
    if !p.as_os_str().is_empty() && seen.insert(p.clone()) {
        out.push(p);
    }
}

fn read_zsh_login_path(timeout: Duration) -> Option<String> {
    let zsh = Path::new("/bin/zsh");
    if !zsh.exists() {
        return None;
    }

    let mut child = Command::new(zsh)
        .args(["-ilc", "printenv PATH"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let output = child.wait_with_output().ok()?;
                let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
                return (!text.is_empty()).then_some(text);
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn build_resume_command_handles_all_harnesses_and_shell_quotes_cwd() {
        let dir = TempDir::new().unwrap();
        let weird_dir = dir.path().join("team's repo");
        std::fs::create_dir_all(&weird_dir).unwrap();
        let weird_str = weird_dir.to_string_lossy();

        assert_eq!(
            build_resume_command("codex", "codex:abc-1", Some(&weird_str)).as_deref(),
            Some(
                format!(
                    "cd '{}' && codex resume 'abc-1'",
                    weird_str.replace('\'', "'\\''")
                )
                .as_str()
            )
        );
        assert_eq!(
            build_resume_command("claude", "claude:s-2", None).as_deref(),
            Some("claude --resume 's-2'")
        );
        assert_eq!(
            build_resume_command("dsh", "dsh:s-3", Some("/nonexistent/path/xyz")).as_deref(),
            Some("dsh --resume 's-3'")
        );
        assert_eq!(
            build_resume_command("antigravity", "antigravity:uuid-4", None).as_deref(),
            Some("agy --conversation 'uuid-4'")
        );
        assert_eq!(build_resume_command("unknown", "unknown:x", None), None);
    }
}
