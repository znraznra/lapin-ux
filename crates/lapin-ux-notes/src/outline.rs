// SPDX-License-Identifier: GPL-3.0-only
//! Function list: finds definitions (functions, classes, headings...) line by
//! line with small per-language patterns, plus the word list that
//! auto-completion draws from.

use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq)]
pub struct Symbol {
    pub line: usize,
    pub kind: &'static str,
    pub name: String,
    pub depth: usize,
}

fn ident(s: &str) -> String {
    s.chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$').collect()
}

fn after<'a>(s: &'a str, kw: &str) -> Option<&'a str> {
    let r = s.strip_prefix(kw)?;
    let c = r.chars().next()?;
    if c.is_whitespace() { Some(r.trim_start()) } else { None }
}

fn strip_mods<'a>(mut s: &'a str, mods: &[&str]) -> &'a str {
    loop {
        let mut hit = false;
        for m in mods {
            if let Some(r) = after(s, m) {
                s = r;
                hit = true;
            }
        }
        if !hit {
            return s;
        }
    }
}

fn depth_of(line: &str) -> usize {
    let mut n = 0;
    for c in line.chars() {
        match c {
            ' ' => n += 1,
            '\t' => n += 4,
            _ => break,
        }
    }
    n / 4
}

/// Symbols of `text` for the language `lang` (a syntect name or "Plain text").
pub fn symbols(text: &str, lang: &str) -> Vec<Symbol> {
    let mut out = Vec::new();
    let l = lang.to_lowercase();
    for (i, raw) in text.lines().enumerate() {
        if raw.len() > 400 {
            continue;
        }
        let t = raw.trim_start();
        let depth = depth_of(raw);
        let mut push = |kind: &'static str, name: String| {
            if !name.is_empty() {
                out.push(Symbol { line: i, kind, name, depth });
            }
        };
        if l == "rust" {
            let s = strip_mods(t, &["pub", "pub(crate)", "pub(super)", "async", "const", "unsafe", "extern"]);
            for (kw, kind) in [("fn", "function"), ("struct", "struct"), ("enum", "enum"), ("trait", "trait"), ("mod", "module"), ("impl", "impl"), ("type", "type")] {
                if let Some(r) = after(s, kw) {
                    let r = if kw == "impl" { r.trim_start_matches('<').trim_start_matches(|c: char| c != '>' && r.starts_with('<')).trim_start_matches('>').trim_start() } else { r };
                    let n = if kw == "impl" { r.split('{').next().unwrap_or("").trim().to_string() } else { ident(r) };
                    push(kind, n);
                    break;
                }
            }
        } else if l == "python" {
            if let Some(r) = after(strip_mods(t, &["async"]), "def") {
                push("function", ident(r));
            } else if let Some(r) = after(t, "class") {
                push("class", ident(r));
            }
        } else if l.contains("javascript") || l.contains("typescript") || l == "js" {
            let s = strip_mods(t, &["export", "default", "async"]);
            if let Some(r) = after(s, "function") {
                push("function", ident(r.trim_start_matches('*').trim_start()));
            } else if let Some(r) = after(s, "class") {
                push("class", ident(r));
            } else if let Some(r) = after(s, "interface") {
                push("interface", ident(r));
            } else if let Some(r) = after(s, "const").or_else(|| after(s, "let")) {
                let n = ident(r);
                let rest = r[n.len()..].trim_start();
                if !n.is_empty() && (rest.contains("=>") || rest.starts_with("= function") || rest.starts_with("= async")) {
                    push("function", n);
                }
            }
        } else if l == "go" {
            if let Some(r) = after(t, "func") {
                let r = if r.starts_with('(') { r.split_once(')').map(|x| x.1.trim_start()).unwrap_or("") } else { r };
                push("function", ident(r));
            } else if let Some(r) = after(t, "type") {
                push("type", ident(r));
            }
        } else if l == "c" || l == "c++" || l == "objective-c" || l == "java" || l == "c#" || l == "php" || l == "d" {
            if raw.starts_with(|c: char| c.is_whitespace()) && depth > 1 && l != "java" && l != "c#" && l != "php" {
                // inside a function body in C-like files: skip
            } else if let Some(r) = after(strip_mods(t, &["public", "private", "protected", "static", "final", "abstract", "sealed", "partial"]), "class") {
                push("class", ident(r));
            } else if let Some(r) = after(t, "struct") {
                if t.contains('{') || !t.contains(';') {
                    push("struct", ident(r));
                }
            } else if let Some(r) = after(t, "function") {
                push("function", ident(r));
            } else if let Some(p) = t.find('(') {
                let head = &t[..p];
                let first = head.split_whitespace().next().unwrap_or("");
                let name = head.split(|c: char| c.is_whitespace() || c == '*' || c == '&').filter(|x| !x.is_empty()).last().unwrap_or("");
                let name = name.rsplit("::").next().unwrap_or(name);
                let kw = ["if", "for", "while", "switch", "return", "else", "do", "catch", "sizeof", "new", "throw", "using", "case"];
                let called = t.ends_with(';') || t.ends_with(',') || t.ends_with(")");
                if head.split_whitespace().count() >= 2 && !kw.contains(&first) && !kw.contains(&name) && !called && !head.contains('=') && !head.starts_with('#') && !head.starts_with("//") {
                    push("function", ident(name));
                }
            }
        } else if l.contains("shell") || l.contains("bash") {
            if let Some(r) = after(t, "function") {
                push("function", ident(r));
            } else if let Some(p) = t.find("()") {
                let n = t[..p].trim();
                if !n.is_empty() && !n.contains(' ') {
                    push("function", n.to_string());
                }
            }
        } else if l == "ruby" {
            if let Some(r) = after(t, "def") {
                push("function", ident(r.trim_start_matches("self.")));
            } else if let Some(r) = after(t, "class") {
                push("class", ident(r));
            } else if let Some(r) = after(t, "module") {
                push("module", ident(r));
            }
        } else if l == "lua" {
            let s = strip_mods(t, &["local"]);
            if let Some(r) = after(s, "function") {
                push("function", r.split('(').next().unwrap_or("").trim().to_string());
            }
        } else if l.contains("markdown") {
            if t.starts_with('#') && !raw.starts_with(' ') {
                let n = t.chars().take_while(|c| *c == '#').count();
                if n <= 6 && t[n..].starts_with(' ') {
                    out.push(Symbol { line: i, kind: "heading", name: t[n..].trim().to_string(), depth: n - 1 });
                }
            }
        } else if l == "html" || l == "xml" {
            if let Some(p) = t.find("id=\"") {
                let n: String = t[p + 4..].chars().take_while(|c| *c != '"').collect();
                push("id", n);
            }
        } else if l == "css" || l == "scss" || l == "less" {
            if t.ends_with('{') && !t.starts_with('}') && depth == 0 {
                push("rule", t.trim_end_matches('{').trim().to_string());
            }
        } else if l == "makefile" {
            if depth == 0 && !raw.starts_with('\t') {
                if let Some((n, _)) = t.split_once(':') {
                    if !n.is_empty() && !n.contains(['=', ' ', '$', '#']) && !t.contains(":=") {
                        push("target", n.to_string());
                    }
                }
            }
        } else if l == "toml" || l == "ini" {
            if t.starts_with('[') && t.ends_with(']') {
                push("section", t.trim_matches(|c| c == '[' || c == ']').to_string());
            }
        } else if l == "yaml" {
            if depth == 0 && !raw.starts_with(['#', '-', ' ']) {
                if let Some((n, _)) = t.split_once(':') {
                    push("key", n.to_string());
                }
            }
        } else if l == "json" {
            if depth == 1 {
                if let Some(r) = t.strip_prefix('"') {
                    push("key", r.split('"').next().unwrap_or("").to_string());
                }
            }
        } else if l.contains("sql") {
            let u = t.to_uppercase();
            if u.starts_with("CREATE ") {
                let w: Vec<&str> = t.split_whitespace().collect();
                if let Some(k) = w.iter().position(|x| ["TABLE", "VIEW", "FUNCTION", "PROCEDURE", "INDEX", "TRIGGER"].contains(&x.to_uppercase().as_str())) {
                    let n = w.get(k + 1..).unwrap_or(&[]).iter().find(|x| !["IF", "NOT", "EXISTS"].contains(&x.to_uppercase().as_str())).copied().unwrap_or("");
                    push("create", ident(n.trim_matches('"')));
                }
            }
        }
    }
    out
}

const KEYWORDS: &[(&str, &[&str])] = &[
    ("rust", &["fn", "let", "mut", "pub", "struct", "enum", "impl", "trait", "match", "loop", "while", "for", "return", "use", "mod", "crate", "self", "Self", "const", "static", "async", "await", "unsafe", "where", "type", "Option", "Result", "Some", "None", "Ok", "Err", "String", "Vec"]),
    ("python", &["def", "class", "return", "import", "from", "as", "if", "elif", "else", "for", "while", "try", "except", "finally", "with", "lambda", "yield", "pass", "break", "continue", "None", "True", "False", "self", "print", "range", "len"]),
    ("javascript", &["function", "const", "let", "var", "return", "if", "else", "for", "while", "class", "extends", "new", "this", "import", "export", "default", "async", "await", "try", "catch", "throw", "null", "undefined", "true", "false", "console", "typeof"]),
    ("c", &["int", "char", "void", "struct", "typedef", "return", "if", "else", "for", "while", "switch", "case", "break", "continue", "static", "const", "unsigned", "sizeof", "include", "define", "NULL"]),
    ("shell", &["if", "then", "else", "fi", "for", "do", "done", "while", "case", "esac", "function", "echo", "export", "local", "return", "exit"]),
    ("html", &["div", "span", "class", "href", "script", "style", "section", "header", "footer", "button", "input", "form", "table"]),
];

/// Keywords for a language name (syntect names are matched loosely).
pub fn keywords(lang: &str) -> &'static [&'static str] {
    let l = lang.to_lowercase();
    for (k, w) in KEYWORDS {
        let hit = l.contains(k) || (*k == "c" && (l == "c++" || l == "c" || l == "java")) || (*k == "javascript" && l.contains("typescript"));
        if hit && (*k != "c" || l == "c" || l == "c++" || l == "java") {
            return w;
        }
    }
    &[]
}

/// Words starting with `prefix` (case-insensitive) from the document plus the
/// language's keywords, most useful first, never `prefix` itself.
pub fn completions(text: &str, prefix: &str, lang: &str, limit: usize) -> Vec<String> {
    if prefix.chars().count() < 1 {
        return Vec::new();
    }
    let p = prefix.to_lowercase();
    let mut set = BTreeSet::new();
    let mut cur = String::new();
    let mut flush = |cur: &mut String, set: &mut BTreeSet<String>| {
        if cur.chars().count() >= 3 && cur.to_lowercase().starts_with(&p) && cur != prefix {
            set.insert(cur.clone());
        }
        cur.clear();
    };
    for c in text.chars() {
        if c.is_alphanumeric() || c == '_' {
            cur.push(c);
        } else {
            flush(&mut cur, &mut set);
        }
    }
    flush(&mut cur, &mut set);
    for k in keywords(lang) {
        if k.to_lowercase().starts_with(&p) && *k != prefix {
            set.insert((*k).to_string());
        }
    }
    let mut v: Vec<String> = set.into_iter().collect();
    v.sort_by_key(|w| (!w.starts_with(prefix), w.len(), w.to_lowercase()));
    v.truncate(limit);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_symbols() {
        let t = "pub struct A;\nimpl A {\n    pub async fn go(&self) {}\n}\nfn main() {}\n";
        let s = symbols(t, "Rust");
        let n: Vec<_> = s.iter().map(|x| (x.line, x.name.as_str())).collect();
        assert_eq!(n, vec![(0, "A"), (1, "A"), (2, "go"), (4, "main")]);
        assert_eq!(s[2].depth, 1);
    }

    #[test]
    fn python_and_markdown() {
        let s = symbols("class A:\n    def f(self):\n        pass\n", "Python");
        assert_eq!(s.len(), 2);
        assert_eq!(s[1].name, "f");
        let m = symbols("# One\ntext\n## Two\n", "Markdown");
        assert_eq!(m.len(), 2);
        assert_eq!(m[1].depth, 1);
    }

    #[test]
    fn c_functions_not_calls() {
        let s = symbols("int main(int a) {\n  foo(a);\n  return bar(1);\n}\n", "C");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "main");
    }

    #[test]
    fn js_arrow_and_shell() {
        let s = symbols("export const f = (a) => a;\nfunction g() {}\n", "JavaScript");
        assert_eq!(s.len(), 2);
        let b = symbols("build() {\n}\nfunction deploy {\n}\n", "Bourne Again Shell (bash)");
        assert_eq!(b.len(), 2);
    }

    #[test]
    fn completes_from_text_and_keywords() {
        let c = completions("let counter = 1; let count_all = 2; cou", "cou", "Rust", 10);
        assert_eq!(c, vec!["counter", "count_all"]);
        let k = completions("", "ma", "Rust", 10);
        assert!(k.contains(&"match".to_string()));
        assert!(completions("abc", "", "Rust", 10).is_empty());
    }
}
