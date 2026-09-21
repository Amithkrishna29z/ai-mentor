//! Fetching a learner's project repository for AI verification.

use anyhow::{anyhow, bail, Result};
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

/// Directories and files that never help a review and blow the size budget.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "build",
    "dist",
    "out",
    "vendor",
    ".venv",
    "venv",
    "__pycache__",
    ".idea",
    ".gradle",
    ".next",
];

const SKIP_FILES: &[&str] = &[
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "Cargo.lock",
    "poetry.lock",
    "composer.lock",
    "go.sum",
];

/// Extensions worth sending to a reviewer.
const SOURCE_EXTS: &[&str] = &[
    "java", "kt", "xml", "gradle", "properties", "yml", "yaml", "sql", "rs", "go", "py", "js",
    "jsx", "ts", "tsx", "css", "scss", "html", "json", "toml", "sh", "ps1", "md", "dockerfile",
    "tf", "cs", "rb", "php",
];

/// Shallow-clone a repository into a temp dir and resolve its HEAD sha.
/// The `TempDir` must stay alive for as long as the clone is needed.
pub fn clone_repo(git_path: &str, url: &str) -> Result<(TempDir, String)> {
    if url.trim().is_empty() {
        bail!("no GitHub URL supplied");
    }
    let tmp = TempDir::new()?;
    let dest = tmp.path().join("repo");

    let output = Command::new(git_path)
        .arg("clone")
        .arg("--depth")
        .arg("1")
        .arg(url)
        .arg(&dest)
        .output()
        .map_err(|e| {
            anyhow!("could not run '{git_path}': {e}. Install git or set its path in Settings.")
        })?;

    if !output.status.success() {
        bail!(
            "git clone failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let sha = Command::new(git_path)
        .arg("rev-parse")
        .arg("HEAD")
        .current_dir(&dest)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    Ok((tmp, sha))
}

/// Path of the cloned working tree inside the temp dir.
pub fn repo_dir(tmp: &TempDir) -> std::path::PathBuf {
    tmp.path().join("repo")
}

fn is_source(path: &Path) -> bool {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if SKIP_FILES.contains(&name) {
            return false;
        }
        if name.eq_ignore_ascii_case("dockerfile") || name.eq_ignore_ascii_case("makefile") {
            return true;
        }
    }
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| SOURCE_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Concatenate filtered source files into one prompt-sized blob. Used as the
/// fallback when the CLI cannot read the working directory itself.
pub fn gather_sources(root: &Path, cap_bytes: usize) -> Result<String> {
    let mut out = String::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if !SKIP_DIRS.contains(&name.as_str()) {
                    stack.push(path);
                }
                continue;
            }
            if !is_source(&path) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue; // binary or unreadable
            };
            let rel = path.strip_prefix(root).unwrap_or(&path).display().to_string();
            if out.len() + text.len() > cap_bytes {
                out.push_str(&format!(
                    "\n\n--- {rel} (omitted: size cap reached) ---\n"
                ));
                return Ok(out);
            }
            out.push_str(&format!("\n\n--- {rel} ---\n{text}"));
        }
    }
    Ok(out)
}

/// Rough file/line census shown next to a verification run.
pub fn count_sources(root: &Path) -> usize {
    let mut count = 0;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if !SKIP_DIRS.contains(&name.as_str()) {
                    stack.push(path);
                }
            } else if is_source(&path) {
                count += 1;
            }
        }
    }
    count
}
