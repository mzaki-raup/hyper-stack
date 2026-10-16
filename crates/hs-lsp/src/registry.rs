//! Which server serves which language. Nothing is bundled or downloaded:
//! the table knows the usual command for each language and how it is
//! installed, [`on_path`] says whether it is there, and the editor's
//! settings can replace the command for any language (that is how a
//! language "extension" is added: any server speaking LSP over stdio).

use std::path::{Path, PathBuf};

/// A language server the editor knows how to start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerSpec {
    /// `hs_editor::languages::Language::name`.
    pub language: &'static str,
    /// The `languageId` sent in `textDocument/didOpen`.
    pub language_id: &'static str,
    /// Short name for status lines and settings (`rust-analyzer`).
    pub id: &'static str,
    pub command: &'static str,
    pub args: &'static [&'static str],
    /// How to install it, shown in Settings when it is missing.
    pub install: &'static str,
    /// A command that installs it on every platform HyperStack runs on,
    /// empty when installation is platform-specific (then `install_url`
    /// is the only route offered).
    pub install_command: &'static str,
    /// The download or documentation page.
    pub install_url: &'static str,
}

pub const DEFAULTS: &[ServerSpec] = &[
    ServerSpec {
        language: "Rust",
        language_id: "rust",
        id: "rust-analyzer",
        command: "rust-analyzer",
        args: &[],
        install: "rustup component add rust-analyzer",
        install_command: "rustup component add rust-analyzer",
        install_url: "https://rust-analyzer.github.io",
    },
    ServerSpec {
        language: "TypeScript",
        language_id: "typescript",
        id: "typescript-language-server",
        command: "typescript-language-server",
        args: &["--stdio"],
        install: "npm install -g typescript-language-server typescript",
        install_command: "npm install -g typescript-language-server typescript",
        install_url: "https://github.com/typescript-language-server/typescript-language-server",
    },
    ServerSpec {
        language: "JavaScript",
        language_id: "javascript",
        id: "typescript-language-server",
        command: "typescript-language-server",
        args: &["--stdio"],
        install: "npm install -g typescript-language-server typescript",
        install_command: "npm install -g typescript-language-server typescript",
        install_url: "https://github.com/typescript-language-server/typescript-language-server",
    },
    ServerSpec {
        language: "Python",
        language_id: "python",
        id: "pyright",
        command: "pyright-langserver",
        args: &["--stdio"],
        install: "pip install pyright   (or: npm install -g pyright)",
        install_command: "pip install pyright",
        install_url: "https://github.com/microsoft/pyright",
    },
    ServerSpec {
        language: "Go",
        language_id: "go",
        id: "gopls",
        command: "gopls",
        args: &[],
        install: "go install golang.org/x/tools/gopls@latest",
        install_command: "go install golang.org/x/tools/gopls@latest",
        install_url: "https://pkg.go.dev/golang.org/x/tools/gopls",
    },
    ServerSpec {
        language: "C",
        language_id: "c",
        id: "clangd",
        command: "clangd",
        args: &[],
        install: "LLVM (winget install LLVM.LLVM / apt install clangd)",
        install_command: "",
        install_url: "https://clangd.llvm.org/installation",
    },
    ServerSpec {
        language: "C++",
        language_id: "cpp",
        id: "clangd",
        command: "clangd",
        args: &[],
        install: "LLVM (winget install LLVM.LLVM / apt install clangd)",
        install_command: "",
        install_url: "https://clangd.llvm.org/installation",
    },
    ServerSpec {
        language: "Java",
        language_id: "java",
        id: "jdtls",
        command: "jdtls",
        args: &[],
        // A VS Code Java extension is not a language server HyperStack can
        // talk to: the download below is the server itself, a tarball to
        // unpack, with bin/ added to PATH. On Windows the thing that starts
        // is bin/jdtls.bat, which runs the bin/jdtls Python script, so
        // Python has to be on PATH too; Java 21 or newer is the server's
        // own requirement.
        install: "Unpack the Eclipse JDT LS tarball and put its bin folder on PATH; its launcher needs Python, and the server needs Java 21+",
        install_command: "",
        install_url: "https://download.eclipse.org/jdtls/milestones/",
    },
    ServerSpec {
        language: "Kotlin",
        language_id: "kotlin",
        id: "kotlin-language-server",
        command: "kotlin-language-server",
        args: &[],
        install: "github.com/fwcd/kotlin-language-server releases",
        install_command: "",
        install_url: "https://github.com/fwcd/kotlin-language-server/releases",
    },
    ServerSpec {
        language: "C#",
        language_id: "csharp",
        id: "csharp-ls",
        command: "csharp-ls",
        args: &[],
        install: "dotnet tool install -g csharp-ls",
        install_command: "dotnet tool install -g csharp-ls",
        install_url: "https://github.com/razzmatazz/csharp-language-server",
    },
    ServerSpec {
        language: "JSON",
        language_id: "json",
        id: "vscode-json-language-server",
        command: "vscode-json-language-server",
        args: &["--stdio"],
        install: "npm install -g vscode-langservers-extracted",
        install_command: "npm install -g vscode-langservers-extracted",
        install_url: "https://github.com/hrsh7th/vscode-langservers-extracted",
    },
    ServerSpec {
        language: "YAML",
        language_id: "yaml",
        id: "yaml-language-server",
        command: "yaml-language-server",
        args: &["--stdio"],
        install: "npm install -g yaml-language-server",
        install_command: "npm install -g yaml-language-server",
        install_url: "https://github.com/redhat-developer/yaml-language-server",
    },
    ServerSpec {
        language: "TOML",
        language_id: "toml",
        id: "taplo",
        command: "taplo",
        args: &["lsp", "stdio"],
        install: "cargo install taplo-cli",
        install_command: "cargo install taplo-cli --features lsp",
        install_url: "https://taplo.tamasfe.dev",
    },
    ServerSpec {
        language: "HTML",
        language_id: "html",
        id: "vscode-html-language-server",
        command: "vscode-html-language-server",
        args: &["--stdio"],
        install: "npm install -g vscode-langservers-extracted",
        install_command: "npm install -g vscode-langservers-extracted",
        install_url: "https://github.com/hrsh7th/vscode-langservers-extracted",
    },
    ServerSpec {
        language: "CSS",
        language_id: "css",
        id: "vscode-css-language-server",
        command: "vscode-css-language-server",
        args: &["--stdio"],
        install: "npm install -g vscode-langservers-extracted",
        install_command: "npm install -g vscode-langservers-extracted",
        install_url: "https://github.com/hrsh7th/vscode-langservers-extracted",
    },
    ServerSpec {
        language: "SCSS",
        language_id: "scss",
        id: "vscode-css-language-server",
        command: "vscode-css-language-server",
        args: &["--stdio"],
        install: "npm install -g vscode-langservers-extracted",
        install_command: "npm install -g vscode-langservers-extracted",
        install_url: "https://github.com/hrsh7th/vscode-langservers-extracted",
    },
    ServerSpec {
        language: "PHP",
        language_id: "php",
        id: "intelephense",
        command: "intelephense",
        args: &["--stdio"],
        install: "npm install -g intelephense",
        install_command: "npm install -g intelephense",
        install_url: "https://intelephense.com",
    },
    ServerSpec {
        language: "Ruby",
        language_id: "ruby",
        id: "solargraph",
        command: "solargraph",
        args: &["stdio"],
        install: "gem install solargraph",
        install_command: "gem install solargraph",
        install_url: "https://solargraph.org",
    },
    ServerSpec {
        language: "Lua",
        language_id: "lua",
        id: "lua-language-server",
        command: "lua-language-server",
        args: &[],
        install: "github.com/LuaLS/lua-language-server releases",
        install_command: "",
        install_url: "https://github.com/LuaLS/lua-language-server/releases",
    },
    ServerSpec {
        language: "Shell",
        language_id: "shellscript",
        id: "bash-language-server",
        command: "bash-language-server",
        args: &["start"],
        install: "npm install -g bash-language-server",
        install_command: "npm install -g bash-language-server",
        install_url: "https://github.com/bash-lsp/bash-language-server",
    },
    ServerSpec {
        language: "PowerShell",
        language_id: "powershell",
        id: "powershell-editor-services",
        command: "",
        args: &[],
        install: "PowerShell Editor Services: set the command in Settings → Editor → Language Servers",
        install_command: "",
        install_url: "https://github.com/PowerShell/PowerShellEditorServices",
    },
    ServerSpec {
        language: "Dockerfile",
        language_id: "dockerfile",
        id: "docker-langserver",
        command: "docker-langserver",
        args: &["--stdio"],
        install: "npm install -g dockerfile-language-server-nodejs",
        install_command: "npm install -g dockerfile-language-server-nodejs",
        install_url: "https://github.com/rcjsuen/dockerfile-language-server-nodejs",
    },
    ServerSpec {
        language: "HCL",
        language_id: "terraform",
        id: "terraform-ls",
        command: "terraform-ls",
        args: &["serve"],
        install: "github.com/hashicorp/terraform-ls releases",
        install_command: "",
        install_url: "https://github.com/hashicorp/terraform-ls/releases",
    },
    ServerSpec {
        language: "Markdown",
        language_id: "markdown",
        id: "marksman",
        command: "marksman",
        args: &["server"],
        install: "github.com/artempyanykh/marksman releases",
        install_command: "",
        install_url: "https://github.com/artempyanykh/marksman/releases",
    },
    ServerSpec {
        language: "Swift",
        language_id: "swift",
        id: "sourcekit-lsp",
        command: "sourcekit-lsp",
        args: &[],
        install: "ships with the Swift toolchain",
        install_command: "",
        install_url: "https://github.com/swiftlang/sourcekit-lsp",
    },
    ServerSpec {
        language: "Scala",
        language_id: "scala",
        id: "metals",
        command: "metals",
        args: &[],
        install: "coursier install metals",
        install_command: "coursier install metals",
        install_url: "https://scalameta.org/metals",
    },
    ServerSpec {
        language: "GraphQL",
        language_id: "graphql",
        id: "graphql-lsp",
        command: "graphql-lsp",
        args: &["server", "-m", "stream"],
        install: "npm install -g graphql-language-service-cli",
        install_command: "npm install -g graphql-language-service-cli",
        install_url: "https://github.com/graphql/graphiql/tree/main/packages/graphql-language-service-cli",
    },
    ServerSpec {
        language: "SQL",
        language_id: "sql",
        id: "sqls",
        command: "sqls",
        args: &[],
        install: "go install github.com/sqls-server/sqls@latest",
        install_command: "go install github.com/sqls-server/sqls@latest",
        install_url: "https://github.com/sqls-server/sqls",
    },
];

/// The `languageId` for an editor language name (`"rust"` for `"Rust"`;
/// the lower-cased name when the table has no entry).
pub fn language_id(language: &str) -> String {
    spec_for(language)
        .map(|s| s.language_id.to_string())
        .unwrap_or_else(|| language.to_ascii_lowercase().replace(' ', ""))
}

pub fn spec_for(language: &str) -> Option<&'static ServerSpec> {
    DEFAULTS.iter().find(|s| s.language == language)
}

/// A user's change to one language's server (Settings → Editor →
/// Language Servers): an empty command keeps the default command.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Override {
    pub language: String,
    pub command: String,
    pub args: Vec<String>,
    pub enabled: bool,
}

/// The server to start for a language after overrides, with its command
/// resolved on `PATH` (`None` when the language has no server, or the
/// user turned it off).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub language: String,
    pub language_id: String,
    pub id: String,
    pub command: String,
    pub args: Vec<String>,
    pub install: String,
    /// A command that installs the server, empty when there is none for
    /// every platform.
    pub install_command: String,
    /// The server's download or documentation page.
    pub install_url: String,
    /// Where the command was found; `None` when it is not installed.
    pub path: Option<PathBuf>,
}

pub fn resolve(language: &str, overrides: &[Override]) -> Option<Resolved> {
    let spec = spec_for(language);
    let ov = overrides.iter().find(|o| o.language == language);
    if ov.is_some_and(|o| !o.enabled) {
        return None;
    }
    let command = ov
        .map(|o| o.command.trim().to_string())
        .filter(|c| !c.is_empty())
        .or_else(|| spec.map(|s| s.command.to_string()))
        .filter(|c| !c.is_empty())?;
    let args = match ov {
        Some(o) if !o.command.trim().is_empty() => o.args.clone(),
        _ => spec
            .map(|s| s.args.iter().map(|a| a.to_string()).collect())
            .unwrap_or_default(),
    };
    let id = spec
        .map(|s| s.id.to_string())
        .unwrap_or_else(|| command.clone());
    let path = on_path(&command);
    Some(Resolved {
        language: language.to_string(),
        language_id: language_id(language),
        id,
        command,
        args,
        install: spec.map(|s| s.install.to_string()).unwrap_or_default(),
        install_command: spec
            .map(|s| s.install_command.to_string())
            .unwrap_or_default(),
        install_url: spec.map(|s| s.install_url.to_string()).unwrap_or_default(),
        path,
    })
}

/// Where `command` is on `PATH` (with `PATHEXT` on Windows, so an npm
/// `.cmd` shim counts). An absolute or relative path is checked as is.
pub fn on_path(command: &str) -> Option<PathBuf> {
    let command = command.trim();
    if command.is_empty() {
        return None;
    }
    let direct = Path::new(command);
    if direct.components().count() > 1 || direct.is_absolute() {
        return candidates(direct).into_iter().find(|p| p.is_file());
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        for c in candidates(&dir.join(command)) {
            if c.is_file() {
                return Some(c);
            }
        }
    }
    None
}

fn candidates(p: &Path) -> Vec<PathBuf> {
    // Windows resolves an extensionless command through PATHEXT, and a file
    // with no extension is not executable there: jdtls ships bin/jdtls (a
    // Python script) beside bin/jdtls.bat, and picking the former gets
    // "%1 is not a valid Win32 application" from CreateProcess. So the
    // shims come first, and the bare name is not a candidate at all.
    if cfg!(windows) && p.extension().is_none() {
        let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into());
        return exts
            .split(';')
            .filter(|e| !e.is_empty())
            .map(|ext| {
                let mut s = p.as_os_str().to_os_string();
                s.push(ext.to_ascii_lowercase());
                PathBuf::from(s)
            })
            .collect();
    }
    vec![p.to_path_buf()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_overrides_resolve() {
        let r = resolve("Rust", &[]).unwrap();
        assert_eq!(
            (r.id.as_str(), r.command.as_str()),
            ("rust-analyzer", "rust-analyzer")
        );
        assert_eq!(r.language_id, "rust");
        let off = [Override {
            language: "Rust".into(),
            enabled: false,
            ..Default::default()
        }];
        assert!(resolve("Rust", &off).is_none());
        let custom = [Override {
            language: "Python".into(),
            command: "pylsp".into(),
            args: vec![],
            enabled: true,
        }];
        let p = resolve("Python", &custom).unwrap();
        assert_eq!(p.command, "pylsp");
        assert!(p.args.is_empty());
        assert!(resolve("Plain Text", &[]).is_none());
        assert!(resolve("PowerShell", &[]).is_none(), "no default command");
        let ps = [Override {
            language: "PowerShell".into(),
            command: "pwsh".into(),
            args: vec!["-File".into(), "Start-EditorServices.ps1".into()],
            enabled: true,
        }];
        assert_eq!(resolve("PowerShell", &ps).unwrap().args.len(), 2);
        assert_eq!(language_id("Plain Text"), "plaintext");
    }

    #[test]
    fn every_server_says_how_to_install_itself() {
        for spec in DEFAULTS {
            assert!(!spec.install.is_empty(), "{}: no install text", spec.id);
            assert!(
                !spec.install_command.is_empty() || !spec.install_url.is_empty(),
                "{}: neither a command nor a page to install from",
                spec.id
            );
            assert!(
                spec.install_url.is_empty() || spec.install_url.starts_with("https://"),
                "{}: install page is not https",
                spec.id
            );
        }
        let r = resolve("Rust", &[]).unwrap();
        assert_eq!(r.install_command, "rustup component add rust-analyzer");
        assert!(r.install_url.starts_with("https://"));
    }

    #[test]
    fn path_lookup_finds_a_known_program() {
        let name = if cfg!(windows) { "cmd" } else { "sh" };
        assert!(on_path(name).is_some());
        assert!(on_path("definitely-not-a-program-xyz").is_none());
        assert!(on_path("").is_none());
    }

    /// jdtls unpacks as bin/jdtls (a Python script) next to bin/jdtls.bat.
    /// On Windows only the shim can be started, so only the shim may be
    /// found; elsewhere the extensionless file is the program.
    #[test]
    fn a_shim_beats_an_extensionless_script_on_windows() {
        let dir = std::env::temp_dir().join("hs-lsp-candidates-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bare = dir.join("jdtls");
        std::fs::write(
            &bare,
            "#!/usr/bin/env python
",
        )
        .unwrap();
        let found = |p: &Path| {
            candidates(p)
                .into_iter()
                .find(|c| c.is_file())
                .map(|c| c.file_name().unwrap().to_string_lossy().to_string())
        };
        if cfg!(windows) {
            assert_eq!(found(&bare), None, "a Python script cannot be started here");
            std::fs::write(
                dir.join("jdtls.bat"),
                "@echo off
",
            )
            .unwrap();
            assert_eq!(found(&bare).as_deref(), Some("jdtls.bat"));
        } else {
            assert_eq!(found(&bare).as_deref(), Some("jdtls"));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
