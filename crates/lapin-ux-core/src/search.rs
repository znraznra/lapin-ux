// SPDX-License-Identifier: GPL-3.0-only
//! Start menu search ranking.
//!
//! Typing in the Start menu should behave like the benchmark: the best match
//! is highlighted immediately, an app whose name *starts* with what you typed
//! beats one that merely contains it, and abbreviations such as "vsc" for
//! "Visual Studio Code" work.

/// The searchable text of one application.
#[derive(Debug, Clone, Default)]
pub struct Searchable<'a> {
    pub name: &'a str,
    pub generic_name: Option<&'a str>,
    pub keywords: &'a [String],
    pub exec: Option<&'a str>,
    pub id: &'a str,
}

/// Higher is better. `None` means "not a match".
pub fn score(query: &str, item: &Searchable<'_>) -> Option<u32> {
    let q = normalize(query);
    let q = q.trim();
    if q.is_empty() {
        return None;
    }
    let name = normalize(item.name);

    let mut best = score_text(q, &name, 1000);

    if let Some(generic) = item.generic_name {
        best = best.max(score_text(q, &normalize(generic), 600));
    }
    for kw in item.keywords {
        let kw = normalize(kw);
        if kw.starts_with(q) {
            best = best.max(Some(500 - len_penalty(&kw, q)));
        } else if q.len() >= 3 && kw.contains(q) {
            best = best.max(Some(350));
        }
    }
    // Executable / desktop id: lets "nautilus" or "org.gnome" find an app
    // whose visible name is something else.
    for raw in [item.exec.and_then(exec_program), Some(item.id)].into_iter().flatten() {
        let raw = normalize(raw);
        if raw.starts_with(q) || raw.split(['.', '-', '_']).any(|part| part.starts_with(q)) {
            best = best.max(Some(300));
        }
    }
    best
}

/// Sort `items` best-first for `query`, dropping non-matches. Ties keep
/// alphabetical order by name so results are stable while typing.
pub fn rank<'a, T>(query: &str, items: &'a [T], view: impl Fn(&'a T) -> Searchable<'a>) -> Vec<&'a T> {
    let mut scored: Vec<(u32, String, &T)> = items
        .iter()
        .filter_map(|it| {
            let s = view(it);
            score(query, &s).map(|sc| (sc, normalize(s.name), it))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, it)| it).collect()
}

fn score_text(q: &str, text: &str, base: u32) -> Option<u32> {
    if text == q {
        return Some(base + 200);
    }
    if text.starts_with(q) {
        return Some(base + 100 - len_penalty(text, q));
    }
    // Start of a later word: "code" in "visual studio code".
    if words(text).skip(1).any(|w| w.starts_with(q)) {
        return Some(base + 50 - len_penalty(text, q));
    }
    // Initials: "vsc" -> "Visual Studio Code".
    if q.chars().count() >= 2 {
        let initials: String = words(text).filter_map(|w| w.chars().next()).collect();
        if initials.starts_with(q) {
            return Some(base + 40);
        }
    }
    if q.chars().count() >= 2 && text.contains(q) {
        return Some(base / 2);
    }
    // Loose subsequence ("fiefx" -> "firefox") only for longer queries,
    // otherwise two letters match half the system.
    if q.chars().count() >= 3 && is_subsequence(q, text) {
        return Some(base / 4);
    }
    None
}

fn len_penalty(text: &str, q: &str) -> u32 {
    (text.chars().count().saturating_sub(q.chars().count())).min(40) as u32
}

fn words(s: &str) -> impl Iterator<Item = &str> {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty())
}

fn is_subsequence(needle: &str, hay: &str) -> bool {
    let mut it = hay.chars();
    needle.chars().filter(|c| !c.is_whitespace()).all(|c| it.any(|h| h == c))
}

/// First program in an Exec= line, without its path.
fn exec_program(exec: &str) -> Option<&str> {
    let first = exec.split_whitespace().find(|t| !t.contains('='))?;
    let first = first.trim_matches('"');
    Some(first.rsplit('/').next().unwrap_or(first))
}

/// Lowercase and strip common Latin diacritics so "café" matches "cafe".
pub fn normalize(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'ç' => 'c',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ñ' => 'n',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'ý' | 'ÿ' => 'y',
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct App {
        name: &'static str,
        generic: Option<&'static str>,
        keywords: Vec<String>,
        exec: &'static str,
        id: &'static str,
    }

    fn app(name: &'static str, generic: Option<&'static str>, kw: &[&str], exec: &'static str, id: &'static str) -> App {
        App { name, generic, keywords: kw.iter().map(|s| s.to_string()).collect(), exec, id }
    }

    fn view(a: &App) -> Searchable<'_> {
        Searchable { name: a.name, generic_name: a.generic, keywords: &a.keywords, exec: Some(a.exec), id: a.id }
    }

    fn catalog() -> Vec<App> {
        vec![
            app("Firefox", Some("Web Browser"), &["internet", "web"], "/usr/lib/firefox/firefox %u", "firefox"),
            app("Files", Some("File Manager"), &["folder", "explorer"], "nautilus --new-window", "org.gnome.Nautilus"),
            app("Visual Studio Code", Some("Text Editor"), &["vscode"], "code %F", "code"),
            app("Calculator", None, &["math"], "gnome-calculator", "org.gnome.Calculator"),
            app("Calendar", None, &[], "gnome-calendar", "org.gnome.Calendar"),
            app("Café Notes", None, &[], "cafe-notes", "cafe-notes"),
            app("Terminal", Some("Terminal Emulator"), &["shell", "prompt", "command line"], "kgx", "org.gnome.Console"),
        ]
    }

    fn names(q: &str) -> Vec<&'static str> {
        let c = catalog();
        rank(q, &c, view).into_iter().map(|a| a.name).collect()
    }

    #[test]
    fn prefix_beats_contains() {
        let r = names("cal");
        assert_eq!(&r[..2], &["Calendar", "Calculator"]); // shorter name first on ties
    }

    #[test]
    fn initials_work() {
        assert_eq!(names("vsc")[0], "Visual Studio Code");
    }

    #[test]
    fn later_word_prefix() {
        assert_eq!(names("code")[0], "Visual Studio Code");
        assert_eq!(names("studio")[0], "Visual Studio Code");
    }

    #[test]
    fn generic_name_and_keywords() {
        assert_eq!(names("browser")[0], "Firefox");
        assert_eq!(names("explorer")[0], "Files");
        assert_eq!(names("command")[0], "Terminal");
    }

    #[test]
    fn exec_and_id() {
        assert_eq!(names("nautilus")[0], "Files");
        assert_eq!(names("console")[0], "Terminal");
    }

    #[test]
    fn diacritics_and_case() {
        assert_eq!(names("CAFE")[0], "Café Notes");
        assert_eq!(names("café")[0], "Café Notes");
    }

    #[test]
    fn exact_match_wins() {
        assert_eq!(names("files")[0], "Files");
    }

    #[test]
    fn empty_and_nonsense() {
        assert!(names("").is_empty());
        assert!(names("   ").is_empty());
        assert!(names("zzqx").is_empty());
    }

    #[test]
    fn fuzzy_subsequence_only_for_longer_queries() {
        assert!(names("fx").iter().all(|n| *n != "Firefox"));
        assert_eq!(names("ffox")[0], "Firefox");
    }
}
