use crate::models::{ContextMode, ProjectFileNode};
use crate::rag::skeleton::extract_skeleton;
use ignore::WalkBuilder;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

const MAX_FILE_SIZE_BYTES: u64 = 512 * 1024; // 512 KB лимит на один исходник
const MAX_SCAN_FILES: usize = 20_000; // защита от гигантских монореп

const IGNORED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".svelte-kit",
    ".next",
    ".nuxt",
    ".turbo",
    ".cache",
    ".parcel-cache",
    ".gradle",
    ".idea",
    ".vscode",
    "__pycache__",
    ".venv",
    "venv",
    "env",
    ".mypy_cache",
    ".pytest_cache",
    "coverage",
    "storybook-static",
    "vendor",
    "Pods",
    "__MACOSX",
];

const IGNORED_FILES: &[&str] = &[
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "Cargo.lock",
    "bun.lockb",
    "composer.lock",
    "Gemfile.lock",
    "poetry.lock",
    "Pipfile.lock",
    ".DS_Store",
    "Thumbs.db",
];

const BINARY_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "gif", "ico", "bmp", "svg", "mp3", "wav", "ogg", "mp4", "avi",
    "mov", "mkv", "webm", "zip", "tar", "gz", "7z", "rar", "exe", "dll", "so", "dylib", "bin",
    "pdf", "woff", "woff2", "ttf", "otf", "eot", "db", "sqlite", "class", "o", "a", "lib", "pdb",
    "wasm", "pyc", "pyo",
];

pub fn estimate_tokens_from_text(text: &str) -> usize {
    // Практичная эвристика для кода + кириллицы/латиницы (~3 символа на токен)
    let chars = text.chars().count();
    chars.div_ceil(3)
}

pub fn detect_language(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();
    if name == "dockerfile" {
        return "dockerfile".to_string();
    }
    if name == "makefile" {
        return "makefile".to_string();
    }
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "rs" => "rust",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "jsx",
        "svelte" => "svelte",
        "vue" => "vue",
        "py" => "python",
        "go" => "go",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" => "cpp",
        "cs" => "csharp",
        "rb" => "ruby",
        "php" => "php",
        "swift" => "swift",
        "dart" => "dart",
        "json" => "json",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "xml" => "xml",
        "css" | "scss" | "sass" | "less" => "css",
        "html" | "htm" => "html",
        "md" | "mdx" => "markdown",
        "sql" => "sql",
        "sh" | "bash" | "zsh" | "bat" | "ps1" => "shell",
        other if !other.is_empty() => other,
        _ => "text",
    }
    .to_string()
}

/// Секреты, которые нельзя отправлять во внешние LLM (`.hidden(false)` тянет dotfiles вроде .env)
fn is_secret_file(name: &str) -> bool {
    let n = name.to_lowercase();
    if n == ".env.example" || n == ".env.sample" || n == ".env.template" || n == ".env.dist" {
        return false;
    }
    n == ".env"
        || n.starts_with(".env.")
        || n == "id_rsa"
        || n == "id_ed25519"
        || n == "id_ecdsa"
        || n == "credentials.json"
        || n == "secrets.json"
        || n == ".npmrc"
        || n == ".netrc"
        || n == ".pgpass"
        || n == "wallet.dat"
        || [
            ".pem",
            ".key",
            ".p12",
            ".pfx",
            ".keystore",
            ".jks",
            ".ovpn",
            ".kdbx",
            ".p8",
        ]
        .iter()
        .any(|ext| n.ends_with(ext))
}

/// Сгенерированный мусор: минифицированные бандлы и source maps раздувают контекст.
fn is_generated_file(name: &str) -> bool {
    let n = name.to_lowercase();
    n.ends_with(".min.js")
        || n.ends_with(".min.css")
        || n.ends_with(".map")
        || n.ends_with(".bundle.js")
        || n.ends_with(".chunk.js")
}

fn is_binary_or_ignored(path: &Path) -> bool {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if IGNORED_FILES.contains(&name) || is_secret_file(name) || is_generated_file(name) {
            return true;
        }
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    BINARY_EXTENSIONS.contains(&ext.as_str())
}

pub fn scan_project(
    root_str: &str,
    saved_modes: &HashMap<String, ContextMode>,
) -> Result<Vec<ProjectFileNode>, String> {
    let root = Path::new(root_str);
    if !root.exists() || !root.is_dir() {
        return Err(format!(
            "Папка проекта не существует или недоступна: {root_str}"
        ));
    }

    let mut walker = WalkBuilder::new(root);
    walker
        .hidden(false)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true)
        .require_git(false)
        .filter_entry(|entry| {
            if entry.depth() == 0 {
                return true;
            }
            if let Some(name) = entry.file_name().to_str() {
                if IGNORED_DIRS.contains(&name) {
                    return false;
                }
            }
            true
        });

    let mut nodes: Vec<ProjectFileNode> = Vec::new();

    for result in walker.build() {
        if nodes.len() >= MAX_SCAN_FILES {
            eprintln!("[scanner] reached MAX_SCAN_FILES={MAX_SCAN_FILES}, stopping early");
            break;
        }
        let entry = match result {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        if path == root {
            continue;
        }

        let rel_path = match path.strip_prefix(root) {
            Ok(p) => p.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };

        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
        if is_dir {
            continue; // путь уже содержит папки, плоский список удобнее для сайдбара
        }

        if is_binary_or_ignored(path) {
            continue;
        }

        let meta = match fs::metadata(path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.len() > MAX_FILE_SIZE_BYTES || meta.len() == 0 {
            continue;
        }

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue, // битая кодировка / бинарник
        };
        if content.as_bytes().contains(&0) {
            continue; // NUL-байты = бинарный файл с текстовым расширением
        }

        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let language = detect_language(path);
        let estimated_tokens = estimate_tokens_from_text(&content);
        let skeleton = extract_skeleton(&content, &language);
        let skeleton_tokens = estimate_tokens_from_text(&skeleton);
        let context_mode = saved_modes.get(&rel_path).copied().unwrap_or_default();

        nodes.push(ProjectFileNode {
            path: rel_path,
            name,
            is_dir: false,
            size_bytes: meta.len(),
            estimated_tokens,
            skeleton_tokens,
            language,
            context_mode,
        });
    }

    nodes.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(nodes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn scanner_skips_secrets_binaries_and_ignored_dirs() {
        let dir = std::env::temp_dir().join(format!("deepbridge-scan-{}", Uuid::new_v4()));
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::create_dir_all(dir.join("node_modules/pkg")).unwrap();
        fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(dir.join(".env"), "SECRET=1\n").unwrap();
        fs::write(dir.join("key.pem"), "-----BEGIN KEY-----").unwrap();
        fs::write(dir.join("app.min.js"), "var a=1;").unwrap();
        fs::write(dir.join("node_modules/pkg/index.js"), "module.exports={}").unwrap();
        fs::write(dir.join("photo.png"), [0u8, 1, 2, 3]).unwrap();

        let nodes = scan_project(&dir.to_string_lossy(), &HashMap::new()).unwrap();
        let paths: Vec<&str> = nodes.iter().map(|n| n.path.as_str()).collect();
        assert_eq!(paths, vec!["src/main.rs"]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn scanner_restores_saved_modes() {
        let dir = std::env::temp_dir().join(format!("deepbridge-scan-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("lib.rs"), "pub fn a() {}\n").unwrap();
        let mut modes = HashMap::new();
        modes.insert("lib.rs".to_string(), ContextMode::Full);
        let nodes = scan_project(&dir.to_string_lossy(), &modes).unwrap();
        assert_eq!(nodes[0].context_mode, ContextMode::Full);
        assert_eq!(nodes[0].language, "rust");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn scanner_rejects_missing_root() {
        assert!(scan_project("Z:\\definitely-missing-deepbridge", &HashMap::new()).is_err());
    }

    #[test]
    fn token_estimate_is_stable() {
        assert_eq!(estimate_tokens_from_text(""), 0);
        assert_eq!(estimate_tokens_from_text("abc"), 1);
        assert_eq!(estimate_tokens_from_text("abcdef"), 2);
    }
}
