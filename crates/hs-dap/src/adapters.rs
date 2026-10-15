//! Which adapter debugs what, how it is started, and where launch
//! configurations come from: `.hyperstack/debug.json` (Zed's shape:
//! `label`, `adapter`, `request`, `program`, `build`, …),
//! `.vscode/launch.json` (`name`, `type`, …) and ones generated for Cargo,
//! Python and Go projects. Nothing is bundled: [`on_path`] says whether an
//! adapter is installed and the spec says how to get it.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

/// How the client reaches the adapter process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// Messages on the process's stdin/stdout.
    Stdio,
    /// The adapter listens on a TCP port given as `{port}` in its arguments.
    Tcp,
}

/// A debug adapter the editor knows how to start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdapterSpec {
    /// Name used in `debug.json` and the UI.
    pub id: &'static str,
    /// `launch.json` `type` values that mean this adapter.
    pub vscode_types: &'static [&'static str],
    /// Editor language names it serves (`hs_editor::languages::Language::name`).
    pub languages: &'static [&'static str],
    pub command: &'static str,
    pub args: &'static [&'static str],
    pub transport: Transport,
    pub install: &'static str,
    /// A command that installs it on every platform HyperStack runs on,
    /// empty when installation is platform-specific.
    pub install_command: &'static str,
    /// The download or documentation page.
    pub install_url: &'static str,
}

pub const ADAPTERS: &[AdapterSpec] = &[
    AdapterSpec {
        id: "codelldb",
        vscode_types: &["lldb", "codelldb"],
        languages: &["Rust", "C", "C++", "Swift"],
        command: "codelldb",
        args: &["--port", "{port}"],
        transport: Transport::Tcp,
        install: "github.com/vadimcn/codelldb/releases (unpack the vsix; adapter/codelldb on PATH)",
        install_command: "",
        install_url: "https://github.com/vadimcn/codelldb/releases",
    },
    AdapterSpec {
        id: "lldb-dap",
        vscode_types: &["lldb-dap"],
        languages: &["Rust", "C", "C++", "Swift"],
        command: "lldb-dap",
        args: &[],
        transport: Transport::Stdio,
        install: "LLVM (winget install LLVM.LLVM / apt install lldb)",
        install_command: "",
        install_url: "https://lldb.llvm.org/resources/lldbdap.html",
    },
    AdapterSpec {
        id: "gdb",
        vscode_types: &["gdb"],
        languages: &["Rust", "C", "C++", "Go"],
        command: "gdb",
        args: &["-i", "dap"],
        transport: Transport::Stdio,
        install: "GDB 14 or newer",
        install_command: "",
        install_url: "https://sourceware.org/gdb/",
    },
    AdapterSpec {
        id: "debugpy",
        vscode_types: &["python", "debugpy"],
        languages: &["Python"],
        command: "python",
        args: &["-m", "debugpy.adapter"],
        transport: Transport::Stdio,
        install: "pip install debugpy",
        install_command: "pip install debugpy",
        install_url: "https://github.com/microsoft/debugpy",
    },
    AdapterSpec {
        id: "delve",
        vscode_types: &["go", "delve"],
        languages: &["Go"],
        command: "dlv",
        args: &["dap", "--listen", "127.0.0.1:{port}"],
        transport: Transport::Tcp,
        install: "go install github.com/go-delve/delve/cmd/dlv@latest",
        install_command: "go install github.com/go-delve/delve/cmd/dlv@latest",
        install_url: "https://github.com/go-delve/delve",
    },
    AdapterSpec {
        id: "netcoredbg",
        vscode_types: &["coreclr", "netcoredbg"],
        languages: &["C#"],
        command: "netcoredbg",
        args: &["--interpreter=vscode"],
        transport: Transport::Stdio,
        install: "github.com/Samsung/netcoredbg/releases",
        install_command: "",
        install_url: "https://github.com/Samsung/netcoredbg/releases",
    },
];

pub fn spec(id: &str) -> Option<&'static AdapterSpec> {
    ADAPTERS.iter().find(|a| {
        a.id.eq_ignore_ascii_case(id) || a.vscode_types.iter().any(|t| t.eq_ignore_ascii_case(id))
    })
}

/// Adapters that serve a language, in preference order.
pub fn for_language(language: &str) -> Vec<&'static AdapterSpec> {
    ADAPTERS
        .iter()
        .filter(|a| a.languages.contains(&language))
        .collect()
}

/// A user's change to one adapter's command (Settings → Editor → Debug
/// Adapters): an empty command keeps the default.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Override {
    pub id: String,
    pub command: String,
    pub args: Vec<String>,
    pub enabled: bool,
}

/// An adapter as it would be started: the command resolved on `PATH`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub spec: &'static AdapterSpec,
    pub command: String,
    pub args: Vec<String>,
    pub path: Option<PathBuf>,
}

pub fn resolve(id: &str, overrides: &[Override]) -> Option<Resolved> {
    let spec = spec(id)?;
    let ov = overrides.iter().find(|o| o.id == spec.id);
    if ov.is_some_and(|o| !o.enabled) {
        return None;
    }
    let (command, args) = match ov {
        Some(o) if !o.command.trim().is_empty() => (o.command.trim().to_string(), o.args.clone()),
        _ => (
            spec.command.to_string(),
            spec.args.iter().map(|a| a.to_string()).collect(),
        ),
    };
    let path = on_path(&command);
    Some(Resolved {
        spec,
        command,
        args,
        path,
    })
}

/// Where `command` is on `PATH` (with `PATHEXT` on Windows); a path with
/// directories is checked as it is.
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
    let mut out = vec![p.to_path_buf()];
    if cfg!(windows) && p.extension().is_none() {
        let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into());
        for ext in exts.split(';').filter(|e| !e.is_empty()) {
            let mut s = p.as_os_str().to_os_string();
            s.push(ext.to_ascii_lowercase());
            out.push(PathBuf::from(s));
        }
    }
    out
}

// ---- launch configurations ---------------------------------------------------

/// A command run before the adapter launches (`cargo build`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildStep {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

/// One entry of the debug picker.
#[derive(Debug, Clone, PartialEq)]
pub struct LaunchConfig {
    pub label: String,
    pub adapter: String,
    /// `launch` or `attach`.
    pub request: String,
    /// Everything the adapter receives as the request's arguments.
    pub arguments: Map<String, Value>,
    pub build: Option<BuildStep>,
    pub cwd: PathBuf,
    /// Where it came from, for the picker.
    pub origin: Origin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// `.hyperstack/debug.json`.
    Project,
    /// `.vscode/launch.json`.
    VsCode,
    /// Made up from the project's build files.
    Generated,
}

/// Every configuration for a project root and the active file.
pub fn configs(root: &Path, file: Option<&Path>) -> Vec<LaunchConfig> {
    let mut out = Vec::new();
    out.extend(read_project_file(root, file));
    out.extend(read_vscode_file(root, file));
    out.extend(generated(root, file));
    out
}

/// Zed-shaped `.hyperstack/debug.json`: an array of objects with `label`,
/// `adapter`, `request` (default `launch`), optional `build` (`{command,
/// args}`) and `cwd`; everything else goes to the adapter as is.
pub fn read_project_file(root: &Path, file: Option<&Path>) -> Vec<LaunchConfig> {
    let path = root.join(".hyperstack").join("debug.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<Value>(&strip_comments(&text)) else {
        tracing::warn!("{} is not valid JSON", path.display());
        return Vec::new();
    };
    let Some(items) = v.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let obj = item.as_object()?;
            let label = obj.get("label")?.as_str()?.to_string();
            let adapter = obj.get("adapter")?.as_str()?.to_string();
            let mut arguments = Map::new();
            let mut build = None;
            let mut cwd = root.to_path_buf();
            for (k, val) in obj {
                let val = substitute(val, root, file);
                match k.as_str() {
                    "label" | "adapter" | "tcp_connection" => {}
                    "build" => build = build_step(&val, root),
                    "cwd" => {
                        if let Some(c) = val.as_str() {
                            cwd = PathBuf::from(c);
                            arguments.insert(k.clone(), val.clone());
                        }
                    }
                    _ => {
                        arguments.insert(k.clone(), val);
                    }
                }
            }
            let request = arguments
                .get("request")
                .and_then(Value::as_str)
                .unwrap_or("launch")
                .to_string();
            arguments.insert("request".into(), Value::String(request.clone()));
            Some(LaunchConfig {
                label,
                adapter,
                request,
                arguments,
                build,
                cwd,
                origin: Origin::Project,
            })
        })
        .collect()
}

/// VS Code's `.vscode/launch.json`: `configurations` with `name`, `type`
/// (mapped to an adapter), `request` and adapter fields.
pub fn read_vscode_file(root: &Path, file: Option<&Path>) -> Vec<LaunchConfig> {
    let path = root.join(".vscode").join("launch.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<Value>(&strip_comments(&text)) else {
        tracing::warn!("{} is not valid JSON", path.display());
        return Vec::new();
    };
    let Some(items) = v.get("configurations").and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let obj = item.as_object()?;
            let label = obj.get("name")?.as_str()?.to_string();
            let kind = obj.get("type")?.as_str()?;
            let adapter = spec(kind)?.id.to_string();
            let mut arguments = Map::new();
            let mut cwd = root.to_path_buf();
            for (k, val) in obj {
                let val = substitute(val, root, file);
                match k.as_str() {
                    "name"
                    | "type"
                    | "preLaunchTask"
                    | "postDebugTask"
                    | "presentation"
                    | "internalConsoleOptions" => {}
                    "cwd" => {
                        if let Some(c) = val.as_str() {
                            cwd = PathBuf::from(c);
                        }
                        arguments.insert(k.clone(), val);
                    }
                    _ => {
                        arguments.insert(k.clone(), val);
                    }
                }
            }
            let request = arguments
                .get("request")
                .and_then(Value::as_str)
                .unwrap_or("launch")
                .to_string();
            Some(LaunchConfig {
                label,
                adapter,
                request,
                arguments,
                build: None,
                cwd,
                origin: Origin::VsCode,
            })
        })
        .collect()
}

/// Configurations made from what the project is: a Cargo package (build,
/// then debug its binary), the open Python file, a Go module.
pub fn generated(root: &Path, file: Option<&Path>) -> Vec<LaunchConfig> {
    let mut out = Vec::new();
    let cargo = root.join("Cargo.toml");
    if cargo.is_file()
        && let Some(name) = cargo_package_name(&cargo)
    {
        let exe = if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.clone()
        };
        let program = root.join("target").join("debug").join(exe);
        let adapter = ["codelldb", "lldb-dap", "gdb"]
            .into_iter()
            .find(|a| spec(a).is_some_and(|s| on_path(s.command).is_some()))
            .unwrap_or("codelldb");
        let mut arguments = Map::new();
        arguments.insert("request".into(), json!("launch"));
        arguments.insert("program".into(), json!(program.to_string_lossy()));
        arguments.insert("cwd".into(), json!(root.to_string_lossy()));
        arguments.insert("sourceLanguages".into(), json!(["rust"]));
        out.push(LaunchConfig {
            label: format!("Debug {name} (cargo build)"),
            adapter: adapter.into(),
            request: "launch".into(),
            arguments,
            build: Some(BuildStep {
                command: "cargo".into(),
                args: vec!["build".into()],
                cwd: root.to_path_buf(),
            }),
            cwd: root.to_path_buf(),
            origin: Origin::Generated,
        });
    }
    if let Some(f) = file
        && f.extension().is_some_and(|e| e == "py")
    {
        let mut arguments = Map::new();
        arguments.insert("request".into(), json!("launch"));
        arguments.insert("program".into(), json!(f.to_string_lossy()));
        arguments.insert("cwd".into(), json!(root.to_string_lossy()));
        arguments.insert("console".into(), json!("internalConsole"));
        arguments.insert("justMyCode".into(), json!(true));
        out.push(LaunchConfig {
            label: format!(
                "Debug {} (debugpy)",
                f.file_name()
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default()
            ),
            adapter: "debugpy".into(),
            request: "launch".into(),
            arguments,
            build: None,
            cwd: root.to_path_buf(),
            origin: Origin::Generated,
        });
    }
    if root.join("go.mod").is_file() {
        let mut arguments = Map::new();
        arguments.insert("request".into(), json!("launch"));
        arguments.insert("mode".into(), json!("debug"));
        arguments.insert("program".into(), json!(root.to_string_lossy()));
        arguments.insert("cwd".into(), json!(root.to_string_lossy()));
        out.push(LaunchConfig {
            label: "Debug Go module (delve)".into(),
            adapter: "delve".into(),
            request: "launch".into(),
            arguments,
            build: None,
            cwd: root.to_path_buf(),
            origin: Origin::Generated,
        });
    }
    out
}

/// The arguments the adapter gets, with the console kept inside the
/// editor (no `runInTerminal`) unless the configuration says otherwise.
pub fn launch_arguments(config: &LaunchConfig) -> Value {
    let mut args = config.arguments.clone();
    match config.adapter.as_str() {
        "debugpy" => {
            args.entry("console")
                .or_insert_with(|| json!("internalConsole"));
        }
        "codelldb" => {
            args.entry("terminal").or_insert_with(|| json!("console"));
        }
        "delve" => {
            args.entry("console")
                .or_insert_with(|| json!("internalConsole"));
        }
        _ => {}
    }
    Value::Object(args)
}

/// The text written to a new `.hyperstack/debug.json`.
pub fn template(root: &Path, file: Option<&Path>) -> String {
    let mut items: Vec<Value> = generated(root, file)
        .into_iter()
        .map(|c| {
            let mut o = Map::new();
            o.insert("label".into(), json!(c.label));
            o.insert("adapter".into(), json!(c.adapter));
            if let Some(b) = &c.build {
                o.insert(
                    "build".into(),
                    json!({"command": b.command, "args": b.args}),
                );
            }
            for (k, v) in c.arguments {
                o.insert(k, v);
            }
            Value::Object(o)
        })
        .collect();
    if items.is_empty() {
        items.push(json!({
            "label": "Debug my program",
            "adapter": "codelldb",
            "request": "launch",
            "program": "${workspaceFolder}/target/debug/my-program",
            "cwd": "${workspaceFolder}",
            "args": []
        }));
    }
    serde_json::to_string_pretty(&Value::Array(items)).unwrap_or_default()
}

fn build_step(v: &Value, root: &Path) -> Option<BuildStep> {
    let obj = v.as_object()?;
    let command = obj.get("command")?.as_str()?.to_string();
    let args = obj
        .get("args")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let cwd = obj
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| root.to_path_buf());
    Some(BuildStep { command, args, cwd })
}

fn cargo_package_name(cargo_toml: &Path) -> Option<String> {
    let text = std::fs::read_to_string(cargo_toml).ok()?;
    let mut in_package = false;
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_package = l == "[package]";
            continue;
        }
        if in_package
            && let Some(rest) = l.strip_prefix("name")
            && let Some(v) = rest.trim_start().strip_prefix('=')
        {
            return Some(v.trim().trim_matches('"').to_string());
        }
    }
    None
}

/// `${workspaceFolder}`, `${workspaceFolderBasename}`, `${file}`,
/// `${fileBasename}`, `${fileBasenameNoExtension}`, `${fileDirname}`,
/// `$ZED_WORKTREE_ROOT`, `$ZED_FILE`, and `~` at the start of a path.
fn substitute(v: &Value, root: &Path, file: Option<&Path>) -> Value {
    match v {
        Value::String(s) => Value::String(substitute_str(s, root, file)),
        Value::Array(a) => Value::Array(a.iter().map(|x| substitute(x, root, file)).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, x)| (k.clone(), substitute(x, root, file)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn substitute_str(s: &str, root: &Path, file: Option<&Path>) -> String {
    let root_s = root.to_string_lossy().to_string();
    let base = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let file_s = file
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();
    let file_base = file
        .and_then(|f| f.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let file_stem = file
        .and_then(|f| f.file_stem())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let file_dir = file
        .and_then(|f| f.parent())
        .map(|d| d.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut out = s
        .replace("${workspaceFolderBasename}", &base)
        .replace("${workspaceFolder}", &root_s)
        .replace("${workspaceRoot}", &root_s)
        .replace("${fileBasenameNoExtension}", &file_stem)
        .replace("${fileBasename}", &file_base)
        .replace("${fileDirname}", &file_dir)
        .replace("${file}", &file_s)
        .replace("$ZED_WORKTREE_ROOT", &root_s)
        .replace("$ZED_FILE", &file_s);
    if (out.starts_with("~/") || out.starts_with("~\\"))
        && let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))
    {
        out = format!("{}{}", home.to_string_lossy(), &out[1..]);
    }
    out
}

/// JSON with `//` and `/* */` comments and trailing commas (what VS Code
/// writes) reduced to plain JSON.
pub fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut in_str = false;
    while i < chars.len() {
        let c = chars[i];
        if in_str {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                out.push(c);
                i += 1;
            }
            '/' if i + 1 < chars.len() && chars[i + 1] == '/' => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '/' if i + 1 < chars.len() && chars[i + 1] == '*' => {
                i += 2;
                while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                    i += 1;
                }
                i += 2;
            }
            ',' => {
                // Drop a comma that only whitespace separates from `]` or `}`.
                let mut j = i + 1;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                if j < chars.len() && (chars[j] == ']' || chars[j] == '}') {
                    i += 1;
                } else {
                    out.push(c);
                    i += 1;
                }
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hs-dap-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn project_file_and_vscode_file_become_configs() {
        let root = temp_root("cfg");
        std::fs::create_dir_all(root.join(".hyperstack")).unwrap();
        std::fs::write(
            root.join(".hyperstack/debug.json"),
            r#"[{"label": "App", "adapter": "codelldb", "program": "${workspaceFolder}/target/debug/app",
                 "build": {"command": "cargo", "args": ["build"]}, "args": ["--fast"]}]"#,
        )
        .unwrap();
        std::fs::create_dir_all(root.join(".vscode")).unwrap();
        std::fs::write(
            root.join(".vscode/launch.json"),
            r#"{
              // VS Code style
              "version": "0.2.0",
              "configurations": [
                {"name": "Python: file", "type": "python", "request": "launch", "program": "${file}", "console": "integratedTerminal",},
                {"name": "Unknown", "type": "cppvsdbg", "request": "launch"}
              ]
            }"#,
        )
        .unwrap();
        let file = root.join("main.py");
        let cfgs = configs(&root, Some(&file));
        // Project file, VS Code file, and the generated one for main.py.
        assert_eq!(cfgs.len(), 3, "{cfgs:?}");
        assert_eq!(cfgs[2].origin, Origin::Generated);
        let app = &cfgs[0];
        assert_eq!(
            (app.label.as_str(), app.adapter.as_str()),
            ("App", "codelldb")
        );
        assert_eq!(app.build.as_ref().unwrap().args, vec!["build"]);
        assert_eq!(
            app.arguments["program"],
            json!(format!("{}/target/debug/app", root.to_string_lossy()))
        );
        assert_eq!(app.arguments["request"], json!("launch"));
        let py = &cfgs[1];
        assert_eq!(py.adapter, "debugpy");
        assert_eq!(py.origin, Origin::VsCode);
        assert_eq!(py.arguments["program"], json!(file.to_string_lossy()));
        // The console the file asked for is kept; a missing one is internal.
        assert_eq!(launch_arguments(py)["console"], json!("integratedTerminal"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cargo_and_python_configs_are_generated() {
        let root = temp_root("gen");
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"demo-app\"\nversion = \"0.1.0\"\n\n[dependencies]\n",
        )
        .unwrap();
        let file = root.join("tool.py");
        let cfgs = generated(&root, Some(&file));
        assert_eq!(cfgs.len(), 2);
        assert!(cfgs[0].label.contains("demo-app"));
        assert!(cfgs[0].build.is_some());
        let program = cfgs[0].arguments["program"].as_str().unwrap();
        assert!(program.ends_with(if cfg!(windows) {
            "demo-app.exe"
        } else {
            "demo-app"
        }));
        assert_eq!(cfgs[1].adapter, "debugpy");
        assert_eq!(
            launch_arguments(&cfgs[1])["console"],
            json!("internalConsole")
        );
        let text = template(&root, Some(&file));
        assert!(text.contains("\"adapter\": \"debugpy\""));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn comments_and_trailing_commas_are_stripped() {
        let text = "{\n // c\n \"a\": [1, 2,], /* x */ \"b\": \"//not\",\n}";
        let v: Value = serde_json::from_str(&strip_comments(text)).unwrap();
        assert_eq!(v["a"], json!([1, 2]));
        assert_eq!(v["b"], json!("//not"));
    }

    #[test]
    fn every_adapter_says_how_to_install_itself() {
        for spec in ADAPTERS {
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
    }

    #[test]
    fn adapters_resolve_and_map_types() {
        assert_eq!(spec("lldb").unwrap().id, "codelldb");
        assert_eq!(spec("PYTHON").unwrap().id, "debugpy");
        assert!(spec("node").is_none());
        let off = [Override {
            id: "debugpy".into(),
            enabled: false,
            ..Default::default()
        }];
        assert!(resolve("debugpy", &off).is_none());
        let custom = [Override {
            id: "debugpy".into(),
            command: "C:/venv/Scripts/python.exe".into(),
            args: vec!["-m".into(), "debugpy.adapter".into()],
            enabled: true,
        }];
        let r = resolve("debugpy", &custom).unwrap();
        assert_eq!(r.command, "C:/venv/Scripts/python.exe");
        assert_eq!(for_language("Rust").len(), 3);
    }
}
