pub mod scanner;
pub mod skeleton;

use crate::models::{BuiltContextResult, ChatMessage, ContextMode, ProjectFileNode};
use crate::rag::scanner::estimate_tokens_from_text;
use crate::rag::skeleton::extract_skeleton;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

const MAX_TREE_LINES: usize = 250;
const MAX_HISTORY_MESSAGES: usize = 8;

fn mode_label(mode: ContextMode) -> &'static str {
    match mode {
        ContextMode::Full => "full",
        ContextMode::Skeleton => "skeleton",
        ContextMode::None => "off",
    }
}

/// Собирает системный промпт из выбранных файлов.
///
/// `token_budget` — мягкий лимит: сначала идут Full-файлы, и если файл не влезает
/// целиком, он автоматически понижается до скелета (как и было задумано в README).
/// Файлы, которые не влезли даже скелетом, пропускаются, а `truncated = true`.
pub fn build_smart_context(
    project_root: Option<&str>,
    nodes: &[ProjectFileNode],
    token_budget: usize,
) -> BuiltContextResult {
    build_smart_context_with_query(project_root, nodes, token_budget, "")
}

/// Query-aware version: the most relevant full files are packed first, so a
/// 32k budget is spent on the files this task actually mentions instead of
/// arbitrary scanner order. Skeleton files follow the same ranking.
pub fn build_smart_context_with_query(
    project_root: Option<&str>,
    nodes: &[ProjectFileNode],
    token_budget: usize,
    query: &str,
) -> BuiltContextResult {
    let mut parts: Vec<String> = Vec::new();
    let mut used_tokens: usize = 0;
    let mut full_count: usize = 0;
    let mut skeleton_count: usize = 0;
    let mut truncated = false;

    parts.push(
        "You are an expert senior software engineer working inside LimitKill.\n\
         Give concise, accurate, production-ready code. Respect existing project conventions and types.\n\
         When you change code, show complete files or precise diffs, not vague descriptions."
            .to_string(),
    );
    used_tokens += estimate_tokens_from_text(parts.last().unwrap());

    if !nodes.is_empty() {
        let tree_lines: Vec<String> = nodes
            .iter()
            .take(MAX_TREE_LINES)
            .map(|n| format!("- {} ({} tokens)", n.path, n.estimated_tokens))
            .collect();
        let mut tree_block = format!(
            "\n### Project File Map ({} files total in scanner)\n{}",
            nodes.len(),
            tree_lines.join("\n")
        );
        if nodes.len() > MAX_TREE_LINES {
            tree_block.push_str(&format!(
                "\n- … and {} more files",
                nodes.len() - MAX_TREE_LINES
            ));
        }
        used_tokens += estimate_tokens_from_text(&tree_block);
        parts.push(tree_block);
    }

    let Some(root_str) = project_root else {
        return BuiltContextResult {
            system_prompt: parts.join("\n\n"),
            total_tokens: used_tokens,
            full_files_count: 0,
            skeleton_files_count: 0,
            truncated: false,
        };
    };

    let root = Path::new(root_str);
    let terms: Vec<String> = query
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .filter(|t| t.len() >= 3)
        .take(24)
        .map(str::to_string)
        .collect();
    let score = |n: &ProjectFileNode| -> usize {
        if terms.is_empty() {
            return 0;
        }
        let path = n.path.to_lowercase();
        terms
            .iter()
            .map(|t| if path.contains(t) { 10 } else { 0 })
            .sum()
    };
    let bm25 = bm25_scores(root, nodes, &terms);
    let mut ranked: Vec<&ProjectFileNode> = nodes.iter().collect();
    ranked.sort_by_key(|n| {
        let content_score = bm25.get(&n.path).copied().unwrap_or(0.0);
        std::cmp::Reverse(score(n) * 1000 + (content_score * 10.0) as usize)
    });

    // 1. Full-файлы. Если файл не влезает в бюджет — пробуем хотя бы его скелет.
    for node in ranked
        .iter()
        .copied()
        .filter(|n| n.context_mode == ContextMode::Full)
    {
        let abs_path = root.join(&node.path);
        let Ok(content) = fs::read_to_string(&abs_path) else {
            continue;
        };
        let block = format!(
            "### File (FULL): `{}` ({})\n```{}\n{}\n```",
            node.path, node.language, node.language, content
        );
        let cost = estimate_tokens_from_text(&block);
        if used_tokens + cost <= token_budget {
            used_tokens += cost;
            full_count += 1;
            parts.push(block);
            continue;
        }

        truncated = true;
        let skel = extract_skeleton(&content, &node.language);
        let skel_block = format!(
            "### File (SKELETON, downgraded from full by context budget): `{}`\n```text\n{}\n```",
            node.path, skel
        );
        let skel_cost = estimate_tokens_from_text(&skel_block);
        if used_tokens + skel_cost <= token_budget {
            used_tokens += skel_cost;
            skeleton_count += 1;
            parts.push(skel_block);
        }
    }

    // 2. Skeleton-файлы.
    for node in ranked
        .iter()
        .copied()
        .filter(|n| n.context_mode == ContextMode::Skeleton)
    {
        let abs_path = root.join(&node.path);
        let Ok(content) = fs::read_to_string(&abs_path) else {
            continue;
        };
        let skel = extract_skeleton(&content, &node.language);
        let block = format!(
            "### File (SKELETON): `{}`\n```text\n{}\n```",
            node.path, skel
        );
        let cost = estimate_tokens_from_text(&block);
        if used_tokens + cost <= token_budget {
            used_tokens += cost;
            skeleton_count += 1;
            parts.push(block);
        } else {
            truncated = true;
        }
    }

    BuiltContextResult {
        system_prompt: parts.join("\n\n"),
        total_tokens: used_tokens,
        full_files_count: full_count,
        skeleton_files_count: skeleton_count,
        truncated: truncated || used_tokens > token_budget,
    }
}

fn tokenize_for_bm25(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|t| t.len() >= 2)
        .take(200_000)
        .map(str::to_string)
        .collect()
}

/// BM25 over the *selected* text files. This stays entirely local and does not
/// require an embedding runtime; it catches function names/errors that path-only
/// ranking misses. The scan is intentionally bounded so a huge repository cannot
/// make a single chat request scan gigabytes.
fn bm25_scores(root: &Path, nodes: &[ProjectFileNode], terms: &[String]) -> HashMap<String, f32> {
    if terms.is_empty() {
        return HashMap::new();
    }
    let query: HashSet<&str> = terms.iter().map(String::as_str).collect();
    let mut docs: Vec<(String, HashMap<String, usize>, usize)> = Vec::new();
    let mut total_bytes = 0usize;
    for node in nodes
        .iter()
        .filter(|n| n.context_mode != ContextMode::None)
        .take(160)
    {
        if total_bytes >= 12 * 1024 * 1024 {
            break;
        }
        let ext = Path::new(&node.path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(
            ext.as_str(),
            "rs" | "ts"
                | "tsx"
                | "js"
                | "jsx"
                | "svelte"
                | "py"
                | "go"
                | "java"
                | "c"
                | "h"
                | "cpp"
                | "hpp"
                | "cs"
                | "json"
                | "toml"
                | "yaml"
                | "yml"
                | "md"
                | "txt"
                | "html"
                | "css"
                | "sql"
                | "sh"
                | "ps1"
                | "vue"
        ) {
            continue;
        }
        let Ok(bytes) = fs::read(root.join(&node.path)) else {
            continue;
        };
        if bytes.contains(&0) || bytes.len() > 220_000 {
            continue;
        }
        total_bytes += bytes.len();
        let text = String::from_utf8_lossy(&bytes);
        let tokens = tokenize_for_bm25(&text);
        let mut tf: HashMap<String, usize> = HashMap::new();
        for token in &tokens {
            if query.contains(token.as_str()) {
                *tf.entry(token.clone()).or_insert(0) += 1;
            }
        }
        if !tf.is_empty() {
            docs.push((node.path.clone(), tf, tokens.len().max(1)));
        }
    }
    if docs.is_empty() {
        return HashMap::new();
    }

    let n = docs.len() as f32;
    let mut df: HashMap<String, usize> = HashMap::new();
    for (_, tf, _) in &docs {
        for term in tf.keys() {
            *df.entry(term.clone()).or_insert(0) += 1;
        }
    }
    let avgdl = docs.iter().map(|(_, _, len)| *len as f32).sum::<f32>() / n;
    let k1 = 1.2f32;
    let b = 0.75f32;
    let mut scores = HashMap::new();
    for (path, tf, len) in docs {
        let mut score = 0.0f32;
        for (term, freq) in &tf {
            let df_t = *df.get(term).unwrap_or(&0) as f32;
            let idf = (1.0 + (n - df_t + 0.5) / (df_t + 0.5)).ln();
            let tf_f = *freq as f32;
            score += idf * (tf_f * (k1 + 1.0))
                / (tf_f + k1 * (1.0 - b + b * len as f32 / avgdl.max(1.0)));
        }
        scores.insert(path, score);
    }
    scores
}

pub fn generate_handoff_snapshot(
    project_root: Option<&str>,
    active_nodes: &[ProjectFileNode],
    messages: &[ChatMessage],
) -> String {
    let project_label = project_root
        .and_then(|p| Path::new(p).file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("current-project");

    let focused_files: Vec<String> = active_nodes
        .iter()
        .filter(|n| n.context_mode != ContextMode::None)
        .map(|n| format!("{} ({})", n.path, mode_label(n.context_mode)))
        .collect();

    let first_user = messages
        .iter()
        .find(|m| m.role == "user")
        .map(|m| m.content.chars().take(700).collect::<String>())
        .unwrap_or_else(|| "—".to_string());

    let last_assistant = messages
        .iter()
        .rev()
        .find(|m| m.role == "assistant")
        .map(|m| {
            let clean = crate::strip_notice(&m.content);
            let snippet: String = clean.chars().take(900).collect();
            if clean.chars().count() > 900 {
                format!("{snippet}…")
            } else {
                snippet
            }
        })
        .unwrap_or_else(|| "—".to_string());

    let recent_msgs: Vec<String> = messages
        .iter()
        .rev()
        .take(MAX_HISTORY_MESSAGES)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|m| {
            let role = m.role.to_uppercase();
            let clean = if m.role == "assistant" {
                crate::strip_notice(&m.content)
            } else {
                m.content.as_str()
            };
            let snippet: String = clean.chars().take(400).collect();
            let suffix = if clean.chars().count() > 400 {
                "..."
            } else {
                ""
            };
            format!("[{role}]: {snippet}{suffix}")
        })
        .collect();

    format!(
        "# LIMITKILL SESSION HANDOFF\n\
         Project: {project_label}\n\
         Active Context Files: {}\n\n\
         ## Original Goal\n{first_user}\n\n\
         ## Last Assistant State\n{last_assistant}\n\n\
         ## Recent Progress & State\n{}\n\n\
         ## Instruction\n\
         Continue from the exact state above. Do not repeat completed steps; focus on the immediate next code change. \
         If you need a file that is not in the context, ask for it instead of guessing.",
        if focused_files.is_empty() {
            "none selected".to_string()
        } else {
            focused_files.join(", ")
        },
        if recent_msgs.is_empty() {
            "No prior messages — this is a fresh start.".to_string()
        } else {
            recent_msgs.join("\n---\n")
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn temp_project() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("deepbridge-rag-{}", Uuid::new_v4()));
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(
            dir.join("src/main.rs"),
            "use std::io;\n\npub fn main() {\n    println!(\"hello\");\n}\n\nfn helper(x: i32) -> i32 { x + 1 }\n",
        )
        .unwrap();
        fs::write(dir.join("src/lib.rs"), "pub fn api() -> u8 { 1 }\n").unwrap();
        dir
    }

    fn node(path: &str, mode: ContextMode, tokens: usize) -> ProjectFileNode {
        ProjectFileNode {
            path: path.to_string(),
            name: path.rsplit('/').next().unwrap_or(path).to_string(),
            is_dir: false,
            size_bytes: 100,
            estimated_tokens: tokens,
            skeleton_tokens: tokens / 2,
            language: "rust".to_string(),
            context_mode: mode,
        }
    }

    #[test]
    fn budget_downgrades_full_to_skeleton_and_flags_truncation() {
        let dir = temp_project();
        let root = dir.to_string_lossy().to_string();
        let mut body = String::from("pub fn huge() -> i32 {\n    let mut total = 0;\n");
        for i in 0..120 {
            body.push_str(&format!("    total += {i};\n"));
        }
        body.push_str("    total\n}\n");
        fs::write(dir.join("src/big.rs"), body).unwrap();

        let nodes = vec![
            node("src/big.rs", ContextMode::Full, 300),
            node("src/lib.rs", ContextMode::Skeleton, 5),
        ];
        let result = build_smart_context(Some(&root), &nodes, 200);
        assert!(!result.system_prompt.is_empty());
        assert!(result.truncated);
        assert_eq!(result.full_files_count, 0);
        assert!(result.skeleton_files_count >= 1);
        assert!(result.system_prompt.contains("SKELETON"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn large_budget_keeps_full_file_content() {
        let dir = temp_project();
        let root = dir.to_string_lossy().to_string();
        let nodes = vec![node("src/lib.rs", ContextMode::Full, 5)];
        let result = build_smart_context(Some(&root), &nodes, 100_000);
        assert!(!result.truncated);
        assert_eq!(result.full_files_count, 1);
        assert!(result.system_prompt.contains("pub fn api() -> u8 { 1 }"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn bm25_prefers_file_with_query_term_in_content() {
        let dir = temp_project();
        let root = dir.to_string_lossy().to_string();
        fs::write(dir.join("src/plain.rs"), "pub fn unrelated() {}\n").unwrap();
        fs::write(
            dir.join("src/retrieval.rs"),
            "pub fn needle_search() { let needle = 1; }\n",
        )
        .unwrap();
        let nodes = vec![
            node("src/plain.rs", ContextMode::Full, 5),
            node("src/retrieval.rs", ContextMode::Full, 5),
        ];
        let scores = bm25_scores(Path::new(&root), &nodes, &["needle".to_string()]);
        assert!(
            scores.get("src/retrieval.rs").copied().unwrap_or_default()
                > scores.get("src/plain.rs").copied().unwrap_or_default()
        );
        let result = build_smart_context_with_query(Some(&root), &nodes, 100_000, "needle");
        assert_eq!(result.full_files_count, 2);
        assert!(result.system_prompt.contains("needle_search"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn handoff_contains_goal_and_files() {
        let msgs = vec![
            ChatMessage {
                id: "1".into(),
                role: "user".into(),
                content: "Add auth".into(),
                provider_name: None,
                model_name: None,
                timestamp: 1,
                tokens_approx: 2,
                attachments: Vec::new(),
            },
            ChatMessage {
                id: "2".into(),
                role: "assistant".into(),
                content: "> *Авто-переключение с **A** на **B***\n\nWorking on it".into(),
                provider_name: None,
                model_name: None,
                timestamp: 2,
                tokens_approx: 2,
                attachments: Vec::new(),
            },
        ];
        let nodes = vec![node("src/main.rs", ContextMode::Full, 10)];
        let snapshot = generate_handoff_snapshot(Some("D:/proj/demo"), &nodes, &msgs);
        assert!(snapshot.contains("demo"));
        assert!(snapshot.contains("Add auth"));
        assert!(snapshot.contains("Working on it"));
        assert!(!snapshot.contains("Авто-переключение"));
        assert!(snapshot.contains("src/main.rs (full)"));
    }
}
