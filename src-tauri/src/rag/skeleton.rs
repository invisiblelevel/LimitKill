use regex::Regex;
use std::sync::OnceLock;

struct SkeletonPatterns {
    rust_sig: Regex,
    ts_js_sig: Regex,
    ts_js_field: Regex,
    py_sig: Regex,
    py_decorator: Regex,
    import_line: Regex,
}

fn patterns() -> &'static SkeletonPatterns {
    static PATTERNS: OnceLock<SkeletonPatterns> = OnceLock::new();
    PATTERNS.get_or_init(|| SkeletonPatterns {
        rust_sig: Regex::new(
            r"^\s*(pub(\([^)]+\))?\s+)?(default\s+)?(async\s+)?(unsafe\s+)?(fn|struct|enum|trait|type|mod|const|static|impl|macro_rules!)\b",
        )
        .unwrap(),
        ts_js_sig: Regex::new(
            r"^\s*(export\s+)?(default\s+)?(declare\s+)?(async\s+)?(function|class|interface|type|enum|namespace)\s+[A-Za-z_$][A-Za-z0-9_$]*|^\s*(export\s+)?(const|let|var)\s+[A-Za-z_$][A-Za-z0-9_$]*\s*[:=]|^\s*(public|private|protected|static|async|get|set)\s+[A-Za-z_$][A-Za-z0-9_$]*\s*\(",
        )
        .unwrap(),
        ts_js_field: Regex::new(
            r"^\s*(export\s+)?(const|let|var)\s+[A-Za-z_$][A-Za-z0-9_$]*\s*(:|=)",
        )
        .unwrap(),
        py_sig: Regex::new(
            r"^\s*(async\s+)?(def|class)\s+[A-Za-z_][A-Za-z0-9_]*\s*(\([^)]*\))?(\s*->\s*[^:]+)?\s*:",
        )
        .unwrap(),
        py_decorator: Regex::new(r"^\s*@[A-Za-z_][A-Za-z0-9_.]*").unwrap(),
        import_line: Regex::new(
            r"^\s*(use\s+|import\s+|from\s+\S+\s+import\s+|export\s+\*\s+from\s+|export\s+\{[^}]*\}\s+from\s+|require\s*\()",
        )
        .unwrap(),
    })
}

fn clean_signature(line: &str) -> String {
    let trimmed = line.trim_end();
    match trimmed.find('{') {
        Some(pos) => format!("{}{}", trimmed[..pos].trim_end(), " { ... }"),
        None => trimmed.trim().to_string(),
    }
}

/// Грубый, но быстрый парсер сигнатур: импорты + объявления функций/типов.
/// Для Svelte дополнительно вытаскиваем runes-объявления.
pub fn extract_skeleton(content: &str, language: &str) -> String {
    let p = patterns();
    let mut lines_out: Vec<String> = Vec::new();
    let mut imports_added = 0usize;

    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Собираем ключевые импорты (не больше 25 строк, чтобы не засорять)
        if imports_added < 25 && p.import_line.is_match(line) {
            lines_out.push(format!("L{}: {}", idx + 1, trimmed));
            imports_added += 1;
            continue;
        }

        let matched = match language {
            "rust" => p.rust_sig.is_match(line),
            "typescript" | "tsx" | "javascript" | "jsx" | "vue" => {
                p.ts_js_sig.is_match(line) || p.ts_js_field.is_match(line)
            }
            "svelte" => {
                p.ts_js_sig.is_match(line)
                    || p.ts_js_field.is_match(line)
                    || trimmed.starts_with("$state")
                    || trimmed.starts_with("$derived")
                    || trimmed.starts_with("$effect")
                    || trimmed.starts_with("$props")
                    || trimmed.starts_with("export let ")
            }
            "python" => p.py_sig.is_match(line) || p.py_decorator.is_match(line),
            "go" => {
                trimmed.starts_with("func ")
                    || trimmed.starts_with("type ")
                    || trimmed.starts_with("package ")
            }
            "java" | "kotlin" | "csharp" | "c" | "cpp" | "swift" | "dart" | "php" | "ruby" => {
                (trimmed.contains('(') && trimmed.ends_with('{'))
                    || trimmed.starts_with("class ")
                    || trimmed.starts_with("interface ")
                    || trimmed.starts_with("struct ")
                    || trimmed.starts_with("enum ")
                    || trimmed.starts_with("def ")
                    || trimmed.starts_with("func ")
            }
            _ => {
                trimmed.starts_with("# ")
                    || trimmed.starts_with("## ")
                    || trimmed.starts_with("pub fn ")
                    || trimmed.starts_with("fn ")
                    || trimmed.starts_with("export ")
            }
        };

        if matched {
            lines_out.push(format!("L{}: {}", idx + 1, clean_signature(line)));
        }
    }

    if lines_out.is_empty() {
        // Фоллбэк: первые 12 непустых строк файла
        return content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .take(12)
            .map(|l| l.trim().to_string())
            .collect::<Vec<_>>()
            .join("\n");
    }

    lines_out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_skeleton_keeps_imports_and_signatures() {
        let src = "use std::io;\n\npub async fn run(x: u8) -> u8 {\n    let y = x + 1;\n    y\n}\n\nstruct Thing { a: u8 }\n";
        let skel = extract_skeleton(src, "rust");
        assert!(skel.contains("use std::io;"));
        assert!(skel.contains("pub async fn run"));
        assert!(skel.contains("struct Thing"));
        assert!(!skel.contains("let y = x + 1"));
    }

    #[test]
    fn ts_skeleton_handles_exports_and_fields() {
        let src = "export const API_URL = 'x';\nexport interface User { id: string }\nfunction helper(a: number) {\n  return a;\n}\nconst internal = 1;\n";
        let skel = extract_skeleton(src, "typescript");
        assert!(skel.contains("export const API_URL"));
        assert!(skel.contains("export interface User"));
        assert!(skel.contains("function helper"));
        assert!(skel.contains("const internal"));
    }

    #[test]
    fn svelte_skeleton_keeps_runes() {
        let src = "<script>\n  let count = $state(0);\n  const doubled = $derived(count * 2);\n  $effect(() => console.log(count));\n</script>\n";
        let skel = extract_skeleton(src, "svelte");
        assert!(skel.contains("$state") || skel.contains("let count"));
        assert!(skel.contains("$derived"));
        assert!(skel.contains("$effect"));
    }

    #[test]
    fn empty_or_weird_file_falls_back_to_head() {
        let skel = extract_skeleton("plain text\nsecond line\n", "text");
        assert!(skel.contains("plain text"));
    }
}
