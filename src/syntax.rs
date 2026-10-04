//! Syntax highlighting: extension-based language detection and a per-line
//! tokenizer that classifies every character into a [`Class`].
//!
//! Design:
//! - No regex, no dependencies — plain scanners per token kind.
//! - Block comments carry state across lines (`State::BlockComment`); the
//!   renderer threads it through visible lines.
//! - Strings always terminate at end of line (an unbalanced quote can't
//!   color the rest of the file).
//! - `Class` maps to a foreground color in [`crate::ui`]; `None` = default.

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    C,
    Cpp,
    JavaScript,
    TypeScript,
    Python,
    Go,
    Java,
    Shell,
    Json,
    Toml,
}

impl Lang {
    /// Extension → language. Callers lowercase the extension.
    pub fn from_extension(ext: &str) -> Option<Lang> {
        match ext {
            "rs" => Some(Lang::Rust),
            "c" | "h" => Some(Lang::C),
            "cc" | "cpp" | "cxx" | "hpp" | "hh" => Some(Lang::Cpp),
            "js" | "jsx" | "mjs" | "cjs" => Some(Lang::JavaScript),
            "ts" | "tsx" => Some(Lang::TypeScript),
            "py" | "pyi" => Some(Lang::Python),
            "go" => Some(Lang::Go),
            "java" => Some(Lang::Java),
            "sh" | "bash" | "zsh" => Some(Lang::Shell),
            "json" => Some(Lang::Json),
            "toml" => Some(Lang::Toml),
            _ => None,
        }
    }

    pub fn from_path(path: &Path) -> Option<Lang> {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .and_then(|e| Lang::from_extension(&e))
    }

    fn rules(self) -> Rules {
        match self {
            Lang::Rust => Rules {
                keywords: &[
                    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else",
                    "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop",
                    "match", "mod", "move", "mut", "pub", "ref", "return", "self", "static",
                    "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while",
                ],
                types: &[
                    "bool", "char", "f32", "f64", "i128", "i16", "i32", "i64", "i8", "isize",
                    "str", "u128", "u16", "u32", "u64", "u8", "usize", "String", "Vec", "Option",
                    "Result", "Box", "Some", "None", "Ok", "Err",
                ],
                line_comment: Some("//"),
                block_comment: Some(("/*", "*/")),
                strings: &['"', '\''],
                dollar_vars: false,
                directive: false,
            },
            Lang::C => Rules {
                keywords: &[
                    "auto", "break", "case", "const", "continue", "default", "do", "else", "enum",
                    "extern", "for", "goto", "if", "inline", "register", "restrict", "return",
                    "sizeof", "static", "struct", "switch", "typedef", "union", "volatile",
                    "while",
                ],
                types: &[
                    "bool", "char", "double", "float", "int", "long", "short", "signed",
                    "unsigned", "void", "size_t", "ssize_t", "int8_t", "int16_t", "int32_t",
                    "int64_t", "uint8_t", "uint16_t", "uint32_t", "uint64_t", "FILE", "NULL",
                ],
                line_comment: Some("//"),
                block_comment: Some(("/*", "*/")),
                strings: &['"', '\''],
                dollar_vars: false,
                directive: true,
            },
            Lang::Cpp => {
                let mut r = Lang::C.rules();
                r.keywords = &[
                    "auto",
                    "break",
                    "case",
                    "catch",
                    "class",
                    "const",
                    "consteval",
                    "constexpr",
                    "continue",
                    "decltype",
                    "default",
                    "delete",
                    "do",
                    "else",
                    "enum",
                    "explicit",
                    "export",
                    "extern",
                    "false",
                    "for",
                    "friend",
                    "goto",
                    "if",
                    "inline",
                    "namespace",
                    "new",
                    "noexcept",
                    "nullptr",
                    "operator",
                    "private",
                    "protected",
                    "public",
                    "return",
                    "sizeof",
                    "static",
                    "struct",
                    "switch",
                    "template",
                    "this",
                    "throw",
                    "true",
                    "try",
                    "typedef",
                    "typename",
                    "union",
                    "using",
                    "virtual",
                    "volatile",
                    "while",
                ];
                r
            }
            Lang::JavaScript => Rules {
                keywords: &[
                    "async",
                    "await",
                    "break",
                    "case",
                    "catch",
                    "class",
                    "const",
                    "continue",
                    "debugger",
                    "default",
                    "delete",
                    "do",
                    "else",
                    "export",
                    "extends",
                    "finally",
                    "for",
                    "function",
                    "if",
                    "import",
                    "in",
                    "instanceof",
                    "let",
                    "new",
                    "return",
                    "static",
                    "super",
                    "switch",
                    "this",
                    "throw",
                    "try",
                    "typeof",
                    "var",
                    "void",
                    "while",
                    "with",
                    "yield",
                    "true",
                    "false",
                    "null",
                    "undefined",
                ],
                types: &[
                    "Array",
                    "Boolean",
                    "Console",
                    "Date",
                    "JSON",
                    "Map",
                    "Math",
                    "Number",
                    "Object",
                    "Promise",
                    "RegExp",
                    "Set",
                    "String",
                    "Symbol",
                    "console",
                    "document",
                    "globalThis",
                    "window",
                ],
                line_comment: Some("//"),
                block_comment: Some(("/*", "*/")),
                strings: &['"', '\'', '`'],
                dollar_vars: false,
                directive: false,
            },
            Lang::TypeScript => {
                let mut r = Lang::JavaScript.rules();
                r.keywords = &[
                    "abstract",
                    "any",
                    "as",
                    "asserts",
                    "async",
                    "await",
                    "break",
                    "case",
                    "catch",
                    "class",
                    "const",
                    "continue",
                    "debugger",
                    "declare",
                    "default",
                    "delete",
                    "do",
                    "else",
                    "enum",
                    "export",
                    "extends",
                    "finally",
                    "for",
                    "from",
                    "function",
                    "if",
                    "implements",
                    "import",
                    "in",
                    "infer",
                    "instanceof",
                    "interface",
                    "is",
                    "keyof",
                    "let",
                    "namespace",
                    "never",
                    "new",
                    "readonly",
                    "return",
                    "satisfies",
                    "static",
                    "super",
                    "switch",
                    "this",
                    "throw",
                    "try",
                    "type",
                    "typeof",
                    "unknown",
                    "var",
                    "void",
                    "while",
                    "with",
                    "yield",
                    "true",
                    "false",
                    "null",
                    "undefined",
                ];
                r
            }
            Lang::Python => Rules {
                keywords: &[
                    "False", "None", "True", "and", "as", "assert", "async", "await", "break",
                    "class", "continue", "def", "del", "elif", "else", "except", "finally", "for",
                    "from", "global", "if", "import", "in", "is", "lambda", "nonlocal", "not",
                    "or", "pass", "raise", "return", "while", "with", "yield",
                ],
                types: &[
                    "bool",
                    "bytes",
                    "dict",
                    "float",
                    "frozenset",
                    "int",
                    "list",
                    "set",
                    "str",
                    "tuple",
                    "self",
                    "cls",
                    "print",
                    "len",
                    "range",
                    "enumerate",
                    "zip",
                    "map",
                    "open",
                    "type",
                    "isinstance",
                    "Exception",
                    "ValueError",
                    "TypeError",
                ],
                line_comment: Some("#"),
                block_comment: None,
                strings: &['"', '\''],
                dollar_vars: false,
                directive: false,
            },
            Lang::Go => Rules {
                keywords: &[
                    "break",
                    "case",
                    "chan",
                    "const",
                    "continue",
                    "default",
                    "defer",
                    "else",
                    "fallthrough",
                    "for",
                    "func",
                    "go",
                    "goto",
                    "if",
                    "import",
                    "interface",
                    "iota",
                    "map",
                    "package",
                    "range",
                    "return",
                    "select",
                    "struct",
                    "switch",
                    "type",
                    "var",
                    "nil",
                    "true",
                    "false",
                ],
                types: &[
                    "any",
                    "bool",
                    "byte",
                    "complex128",
                    "complex64",
                    "error",
                    "float32",
                    "float64",
                    "int",
                    "int16",
                    "int32",
                    "int64",
                    "int8",
                    "rune",
                    "string",
                    "uint",
                    "uint16",
                    "uint32",
                    "uint64",
                    "uint8",
                    "uintptr",
                ],
                line_comment: Some("//"),
                block_comment: Some(("/*", "*/")),
                strings: &['"', '\'', '`'],
                dollar_vars: false,
                directive: false,
            },
            Lang::Java => Rules {
                keywords: &[
                    "abstract",
                    "assert",
                    "break",
                    "case",
                    "catch",
                    "class",
                    "const",
                    "continue",
                    "default",
                    "do",
                    "else",
                    "enum",
                    "extends",
                    "final",
                    "finally",
                    "for",
                    "goto",
                    "if",
                    "implements",
                    "import",
                    "instanceof",
                    "interface",
                    "native",
                    "new",
                    "package",
                    "private",
                    "protected",
                    "public",
                    "return",
                    "static",
                    "strictfp",
                    "super",
                    "switch",
                    "synchronized",
                    "this",
                    "throw",
                    "throws",
                    "transient",
                    "try",
                    "var",
                    "volatile",
                    "while",
                    "true",
                    "false",
                    "null",
                ],
                types: &[
                    "boolean",
                    "byte",
                    "char",
                    "double",
                    "float",
                    "int",
                    "long",
                    "short",
                    "void",
                    "String",
                    "Integer",
                    "Boolean",
                    "Object",
                    "List",
                    "Map",
                    "Set",
                    "ArrayList",
                    "HashMap",
                    "System",
                    "Exception",
                    "Record",
                ],
                line_comment: Some("//"),
                block_comment: Some(("/*", "*/")),
                strings: &['"', '\''],
                dollar_vars: false,
                directive: false,
            },
            Lang::Shell => Rules {
                keywords: &[
                    "break", "case", "continue", "coproc", "do", "done", "elif", "else", "esac",
                    "fi", "for", "function", "if", "in", "return", "select", "then", "time",
                    "until", "while",
                ],
                types: &[
                    "cd", "echo", "eval", "exec", "exit", "export", "local", "printf", "pwd",
                    "read", "readonly", "set", "shift", "source", "test", "trap", "unset", "wait",
                ],
                line_comment: Some("#"),
                block_comment: None,
                strings: &['"', '\''],
                dollar_vars: true,
                directive: false,
            },
            Lang::Json => Rules {
                keywords: &["true", "false", "null"],
                types: &[],
                line_comment: None,
                block_comment: None,
                strings: &['"'],
                dollar_vars: false,
                directive: false,
            },
            Lang::Toml => Rules {
                keywords: &["true", "false"],
                types: &[],
                line_comment: Some("#"),
                block_comment: None,
                strings: &['"', '\''],
                dollar_vars: false,
                directive: false,
            },
        }
    }
}

/// Per-language token rules.
struct Rules {
    keywords: &'static [&'static str],
    types: &'static [&'static str],
    line_comment: Option<&'static str>,
    block_comment: Option<(&'static str, &'static str)>,
    strings: &'static [char],
    /// `$var` in shell gets the Type color.
    dollar_vars: bool,
    /// C-style `#include` directives get the Keyword color.
    directive: bool,
}

/// Color classification of a single character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Normal,
    Keyword,
    Type,
    Str,
    Comment,
    Number,
}

/// Multi-line tokenizer state carried between lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum State {
    #[default]
    Normal,
    /// Inside a `/* ... */` block comment.
    BlockComment,
}

fn is_word_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn starts_with(line: &[char], i: usize, marker: &str) -> bool {
    marker
        .chars()
        .enumerate()
        .all(|(k, mc)| line.get(i + k) == Some(&mc))
}

/// Classify every char of `line`, given the state at the end of the previous
/// line. Returns the classes plus the state at end of this line.
pub fn highlight_line(lang: Lang, line: &[char], mut state: State) -> (Vec<Class>, State) {
    let rules = lang.rules();
    let len = line.len();
    let mut classes = vec![Class::Normal; len];
    let mut i = 0;

    // C-style directive: `#` as the first non-space char colors `#word`.
    if rules.directive && state == State::Normal {
        let first_nonspace = line.iter().position(|c| !c.is_whitespace());
        if first_nonspace == Some(0) && line[0] == '#' {
            let mut j = 1;
            while j < len && is_word_char(line[j]) {
                j += 1;
            }
            for c in &mut classes[..j] {
                *c = Class::Keyword;
            }
            i = j;
        }
    }

    while i < len {
        match state {
            State::BlockComment => {
                let Some((_, end)) = rules.block_comment else {
                    // Language lost its block-comment rules; bail out.
                    for c in &mut classes[i..] {
                        *c = Class::Comment;
                    }
                    break;
                };
                match find_marker(line, i, end) {
                    Some(j) => {
                        for c in &mut classes[i..j + end.len()] {
                            *c = Class::Comment;
                        }
                        i = j + end.len();
                        state = State::Normal;
                    }
                    None => {
                        for c in &mut classes[i..] {
                            *c = Class::Comment;
                        }
                        break;
                    }
                }
            }
            State::Normal => {
                let c = line[i];

                // Line comment: rest of the line.
                if let Some(marker) = rules.line_comment {
                    if starts_with(line, i, marker) {
                        for cc in &mut classes[i..] {
                            *cc = Class::Comment;
                        }
                        i = len;
                        continue;
                    }
                }

                // Block comment start.
                if let Some((start, _)) = rules.block_comment {
                    if starts_with(line, i, start) {
                        state = State::BlockComment;
                        continue; // re-scan as BlockComment (handles `/**/`)
                    }
                }

                // String literal; closes at the quote or end of line.
                if rules.strings.contains(&c) {
                    classes[i] = Class::Str;
                    let mut j = i + 1;
                    while j < len {
                        if line[j] == '\\' {
                            if j + 1 < len {
                                classes[j] = Class::Str;
                                classes[j + 1] = Class::Str;
                                j += 2;
                                continue;
                            }
                            break;
                        }
                        classes[j] = Class::Str;
                        if line[j] == c {
                            j += 1;
                            break;
                        }
                        j += 1;
                    }
                    i = j;
                    continue;
                }

                // Shell `$var`.
                if rules.dollar_vars && c == '$' && i + 1 < len && is_word_start(line[i + 1]) {
                    let mut j = i + 1;
                    while j < len && is_word_char(line[j]) {
                        j += 1;
                    }
                    for cc in &mut classes[i..j] {
                        *cc = Class::Type;
                    }
                    i = j;
                    continue;
                }

                // Word → keyword / type lookup.
                if is_word_start(c) {
                    let mut j = i + 1;
                    while j < len && is_word_char(line[j]) {
                        j += 1;
                    }
                    let word: String = line[i..j].iter().collect();
                    let class = if rules.keywords.contains(&word.as_str()) {
                        Class::Keyword
                    } else if rules.types.contains(&word.as_str()) {
                        Class::Type
                    } else {
                        Class::Normal
                    };
                    for cc in &mut classes[i..j] {
                        *cc = class;
                    }
                    i = j;
                    continue;
                }

                // Number: digit-led run (covers hex/binary via the alnum tail).
                if c.is_ascii_digit() {
                    let mut j = i + 1;
                    while j < len && (is_word_char(line[j]) || line[j] == '.') {
                        j += 1;
                    }
                    for cc in &mut classes[i..j] {
                        *cc = Class::Number;
                    }
                    i = j;
                    continue;
                }

                i += 1;
            }
        }
    }

    // Strings never carry across lines; only block comments do.
    (classes, state)
}

fn find_marker(line: &[char], from: usize, marker: &str) -> Option<usize> {
    (from..line.len()).find(|&i| starts_with(line, i, marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classes_of(lang: Lang, line: &str) -> Vec<Class> {
        highlight_line(lang, &line.chars().collect::<Vec<_>>(), State::Normal).0
    }

    fn classes_with_state(lang: Lang, line: &str, state: State) -> (Vec<Class>, State) {
        highlight_line(lang, &line.chars().collect::<Vec<_>>(), state)
    }

    fn spans(classes: &[Class], class: Class) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut start = None;
        for (i, c) in classes.iter().enumerate() {
            if *c == class {
                if start.is_none() {
                    start = Some(i);
                }
            } else if let Some(s) = start.take() {
                out.push((s, i));
            }
        }
        if let Some(s) = start {
            out.push((s, classes.len()));
        }
        out
    }

    #[test]
    fn extension_detection() {
        assert_eq!(Lang::from_path(Path::new("main.rs")), Some(Lang::Rust));
        assert_eq!(Lang::from_path(Path::new("a/b/x.PY")), Some(Lang::Python));
        assert_eq!(Lang::from_path(Path::new("x.TOML")), Some(Lang::Toml));
        assert_eq!(Lang::from_path(Path::new("notes.txt")), None);
        assert_eq!(Lang::from_path(Path::new("no-ext")), None);
    }

    #[test]
    fn rust_keywords_and_types() {
        let cs = classes_of(Lang::Rust, "let x: Vec<String> = ok();");
        // "let" keyword, "Vec"/"String" types
        assert!(spans(&cs, Class::Keyword).contains(&(0, 3)));
        assert!(spans(&cs, Class::Type).contains(&(7, 10)));
        assert!(spans(&cs, Class::Type).contains(&(11, 17)));
    }

    #[test]
    fn line_comment_to_eol() {
        let cs = classes_of(Lang::Rust, "let x = 1; // trailing");
        assert!(spans(&cs, Class::Comment).contains(&(11, 22)));
        // keyword before the comment still classified
        assert!(spans(&cs, Class::Keyword).contains(&(0, 3)));
    }

    #[test]
    fn strings_with_escapes() {
        let cs = classes_of(Lang::Rust, r#"print("a\"b");"#);
        let s = spans(&cs, Class::Str);
        // string spans from opening quote (6) through closing quote (11)
        assert!(s.contains(&(6, 12)));
    }

    #[test]
    fn string_unterminated_terminates_at_eol() {
        let (cs, state) = classes_with_state(Lang::Rust, "let s = \"open", State::Normal);
        assert_eq!(state, State::Normal); // does not leak to next line
        assert!(spans(&cs, Class::Str).contains(&(8, 13)));
    }

    #[test]
    fn numbers_including_hex() {
        let cs = classes_of(Lang::C, "int x = 0xFF + 42 + 3.14;");
        let n = spans(&cs, Class::Number);
        assert!(n.contains(&(8, 12))); // 0xFF
        assert!(n.contains(&(15, 17))); // 42
        assert!(n.contains(&(20, 24))); // 3.14
    }

    #[test]
    fn block_comment_carries_across_lines() {
        let (cs1, st) = classes_with_state(Lang::Rust, "/* start", State::Normal);
        assert_eq!(st, State::BlockComment);
        assert!(spans(&cs1, Class::Comment).contains(&(0, 8)));

        let (cs2, st2) = classes_with_state(Lang::Rust, "end */ let", st);
        assert_eq!(st2, State::Normal);
        assert!(spans(&cs2, Class::Comment).contains(&(0, 6)));
        assert!(spans(&cs2, Class::Keyword).contains(&(7, 10)));
    }

    #[test]
    fn block_comment_opens_and_closes_on_one_line() {
        let (cs, st) = classes_with_state(Lang::Rust, "a /* mid */ b", State::Normal);
        assert_eq!(st, State::Normal);
        assert!(spans(&cs, Class::Comment).contains(&(2, 11)));
    }

    #[test]
    fn c_directive_colored() {
        let cs = classes_of(Lang::C, "#include <stdio.h>");
        assert!(spans(&cs, Class::Keyword).contains(&(0, 8)));
    }

    #[test]
    fn python_hash_comments() {
        let cs = classes_of(Lang::Python, "def f(): # note");
        assert!(spans(&cs, Class::Keyword).contains(&(0, 3)));
        assert!(spans(&cs, Class::Comment).contains(&(9, 15)));
    }

    #[test]
    fn shell_dollar_vars_and_comments() {
        let cs = classes_of(Lang::Shell, "echo $HOME # done");
        assert!(spans(&cs, Class::Type).contains(&(5, 10))); // $HOME
        assert!(spans(&cs, Class::Comment).contains(&(11, 17)));
    }

    #[test]
    fn json_has_strings_numbers_no_comments() {
        let cs = classes_of(Lang::Json, r#"{"k": 1} // not a comment"#);
        assert!(spans(&cs, Class::Str).contains(&(1, 4)));
        assert!(spans(&cs, Class::Number).contains(&(6, 7)));
        assert!(spans(&cs, Class::Comment).is_empty());
    }

    #[test]
    fn toml_table_headers() {
        // `[table]` section header: '[' and ']' stay Normal, the name is a
        // word — plain lookup means it stays Normal unless keyword/type.
        let cs = classes_of(Lang::Toml, "[owner] # comment");
        assert!(spans(&cs, Class::Comment).contains(&(8, 17)));
    }

    #[test]
    fn empty_line_passthrough() {
        let (cs, st) = classes_with_state(Lang::Rust, "", State::BlockComment);
        assert!(cs.is_empty());
        assert_eq!(st, State::BlockComment);
    }

    #[test]
    fn word_boundaries_respected() {
        // "letra" must not match "let"
        let cs = classes_of(Lang::Rust, "letra let");
        let k = spans(&cs, Class::Keyword);
        assert_eq!(k, vec![(6, 9)]);
    }

    #[test]
    fn unicode_words_dont_panic() {
        let cs = classes_of(Lang::Rust, "日本語 let");
        assert!(spans(&cs, Class::Keyword).contains(&(4, 7)));
    }
}
