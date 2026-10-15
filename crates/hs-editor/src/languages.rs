//! Language table: which syntect syntax to use, how to comment, how to indent.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Language {
    pub name: &'static str,
    /// Token passed to `SyntaxSet::find_syntax_by_token` (extension or name).
    pub syntax_token: &'static str,
    pub line_comment: Option<&'static str>,
    pub block_comment: Option<(&'static str, &'static str)>,
    pub indent: usize,
    pub use_tabs: bool,
    pub is_markdown: bool,
}

const PLAIN: Language = Language {
    name: "Plain Text",
    syntax_token: "txt",
    line_comment: None,
    block_comment: None,
    indent: 4,
    use_tabs: false,
    is_markdown: false,
};

macro_rules! lang {
    ($name:expr, $tok:expr, $lc:expr, $bc:expr, $indent:expr, $tabs:expr) => {
        Language {
            name: $name,
            syntax_token: $tok,
            line_comment: $lc,
            block_comment: $bc,
            indent: $indent,
            use_tabs: $tabs,
            is_markdown: false,
        }
    };
}

/// Detect from a file name or path.
pub fn detect(name: &str) -> Language {
    let lower = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase();
    match lower.as_str() {
        "dockerfile" | "containerfile" => {
            return lang!("Dockerfile", "Dockerfile", Some("#"), None, 4, false);
        }
        "makefile" | "gnumakefile" => {
            return lang!("Makefile", "Makefile", Some("#"), None, 4, true);
        }
        "cargo.lock" => return lang!("TOML", "toml", Some("#"), None, 2, false),
        ".env" | ".gitignore" | ".gitattributes" | ".editorconfig" => {
            return lang!("INI", "ini", Some("#"), None, 2, false);
        }
        _ => {}
    }
    if lower.ends_with(".env") || lower.starts_with(".env.") {
        return lang!("INI", "ini", Some("#"), None, 2, false);
    }
    let ext = lower.rsplit('.').next().unwrap_or("");
    by_extension(ext)
}

pub fn by_extension(ext: &str) -> Language {
    let c = Some(("/*", "*/"));
    match ext {
        "rs" => lang!("Rust", "rs", Some("//"), c, 4, false),
        "ts" | "tsx" | "mts" | "cts" => lang!("TypeScript", "ts", Some("//"), c, 2, false),
        "js" | "jsx" | "mjs" | "cjs" => lang!("JavaScript", "js", Some("//"), c, 2, false),
        // `.map` is a source map: JSON on one line.
        "json" | "jsonc" | "json5" | "har" | "map" => {
            lang!("JSON", "json", Some("//"), c, 2, false)
        }
        "yaml" | "yml" => lang!("YAML", "yaml", Some("#"), None, 2, false),
        "toml" => lang!("TOML", "toml", Some("#"), None, 2, false),
        "ini" | "cfg" | "conf" | "properties" => lang!("INI", "ini", Some("#"), None, 2, false),
        "md" | "markdown" | "mdx" => Language {
            name: "Markdown",
            syntax_token: "md",
            line_comment: None,
            block_comment: Some(("<!--", "-->")),
            indent: 2,
            use_tabs: false,
            is_markdown: true,
        },
        "html" | "htm" | "xhtml" | "vue" | "svelte" => {
            lang!("HTML", "html", None, Some(("<!--", "-->")), 2, false)
        }
        "xml" | "xsd" | "xsl" | "xslt" | "svg" | "csproj" | "plist" | "pom" => {
            lang!("XML", "xml", None, Some(("<!--", "-->")), 2, false)
        }
        "css" => lang!("CSS", "css", None, c, 2, false),
        "scss" | "sass" | "less" => lang!("SCSS", "scss", Some("//"), c, 2, false),
        "sql" | "pgsql" | "mysql" | "psql" => lang!("SQL", "sql", Some("--"), c, 2, false),
        "sh" | "bash" | "zsh" | "fish" | "bashrc" | "zshrc" => {
            lang!("Shell", "sh", Some("#"), None, 2, false)
        }
        "ps1" | "psm1" | "psd1" => {
            lang!("PowerShell", "ps1", Some("#"), Some(("<#", "#>")), 4, false)
        }
        "bat" | "cmd" => lang!("Batch", "bat", Some("REM "), None, 4, false),
        "py" | "pyi" | "pyw" => lang!("Python", "py", Some("#"), None, 4, false),
        "go" => lang!("Go", "go", Some("//"), c, 4, true),
        "java" => lang!("Java", "java", Some("//"), c, 4, false),
        "kt" | "kts" => lang!("Kotlin", "kt", Some("//"), c, 4, false),
        "cs" => lang!("C#", "cs", Some("//"), c, 4, false),
        "c" | "h" => lang!("C", "c", Some("//"), c, 4, false),
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => lang!("C++", "cpp", Some("//"), c, 4, false),
        "php" => lang!("PHP", "php", Some("//"), c, 4, false),
        "rb" | "rake" | "gemspec" => lang!("Ruby", "rb", Some("#"), None, 2, false),
        "lua" => lang!("Lua", "lua", Some("--"), Some(("--[[", "]]")), 2, false),
        "pl" | "pm" => lang!("Perl", "pl", Some("#"), None, 4, false),
        "r" => lang!("R", "r", Some("#"), None, 2, false),
        "swift" => lang!("Swift", "swift", Some("//"), c, 4, false),
        "scala" => lang!("Scala", "scala", Some("//"), c, 2, false),
        "hcl" | "tf" | "tfvars" | "nomad" => lang!("HCL", "hcl", Some("#"), c, 2, false),
        "graphql" | "gql" => lang!("GraphQL", "graphql", Some("#"), None, 2, false),
        "proto" => lang!("Protobuf", "proto", Some("//"), c, 2, false),
        "diff" | "patch" => lang!("Diff", "diff", None, None, 2, false),
        "log" => lang!("Log", "log", None, None, 2, false),
        "csv" | "tsv" => lang!("CSV", "csv", None, None, 2, false),
        "txt" | "text" | "" => PLAIN,
        other => Language {
            name: "Plain Text",
            syntax_token: Box::leak(other.to_string().into_boxed_str()),
            ..PLAIN
        },
    }
}

pub fn plain() -> Language {
    PLAIN
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_by_name_and_extension() {
        assert_eq!(detect("src/main.rs").name, "Rust");
        assert_eq!(detect("C:\\x\\Dockerfile").name, "Dockerfile");
        assert_eq!(detect("docker-compose.yml").name, "YAML");
        assert_eq!(detect(".env.staging").name, "INI");
        assert!(detect("README.md").is_markdown);
        assert_eq!(detect("notes").name, "Plain Text");
        assert!(detect("a.go").use_tabs);
        assert_eq!(detect("a.sql").line_comment, Some("--"));
    }
}
