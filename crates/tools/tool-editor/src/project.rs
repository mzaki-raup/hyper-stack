//! What kind of project a folder holds, and how to build and run it.
//!
//! The editor already knew how to run a single file; a project needs more:
//! Maven and Gradle have a wrapper that should win over anything on PATH,
//! Spring Boot runs through its own goal rather than `exec:java`, and a web
//! application is worth opening in the browser once it says which port it
//! took. Owner's requirement, 2026-10-17.

use std::path::Path;

use hs_core::settings::Editor as EditorSettings;

/// The build tool a project uses. One per project root: the first match in
/// this order wins, which is also the order of "most specific first".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Cargo,
    Maven,
    Gradle,
    Node,
    Go,
    Dotnet,
    Python,
    Make,
    CMake,
}

impl Kind {
    /// The name shown in the toolbar and the tooltip.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Cargo => "Cargo",
            Kind::Maven => "Maven",
            Kind::Gradle => "Gradle",
            Kind::Node => "npm",
            Kind::Go => "Go",
            Kind::Dotnet => ".NET",
            Kind::Python => "Python",
            Kind::Make => "Make",
            Kind::CMake => "CMake",
        }
    }
}

/// A detected project: its build tool, whether it is a Spring Boot
/// application, and whether running it serves something a browser should
/// open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub kind: Kind,
    pub spring_boot: bool,
    /// Running this serves a web application, so the Browser is opened on
    /// the address it prints.
    pub web: bool,
    /// The wrapper script to use instead of the tool on PATH, if the
    /// project ships one (`mvnw`, `gradlew`).
    pub wrapper: Option<String>,
}

/// Read a file if it is there, lower-cased for matching.
fn read(root: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(root.join(name))
        .ok()
        .map(|s| s.to_ascii_lowercase())
}

fn has(root: &Path, name: &str) -> bool {
    root.join(name).exists()
}

/// Which project a folder holds, or `None` for a folder that is just files.
pub fn detect(root: &Path) -> Option<Project> {
    let wrapper = |unix: &str, win: &str| -> Option<String> {
        let name = if cfg!(windows) { win } else { unix };
        has(root, name).then(|| {
            if cfg!(windows) {
                name.to_string()
            } else {
                format!("./{name}")
            }
        })
    };
    if has(root, "pom.xml") {
        let pom = read(root, "pom.xml").unwrap_or_default();
        let spring = pom.contains("spring-boot");
        return Some(Project {
            kind: Kind::Maven,
            spring_boot: spring,
            // A Spring Boot application serves HTTP unless it is explicitly
            // a plain one; the web starter says so outright.
            web: spring && !pom.contains("spring-boot-starter-batch"),
            wrapper: wrapper("mvnw", "mvnw.cmd"),
        });
    }
    if has(root, "build.gradle") || has(root, "build.gradle.kts") || has(root, "settings.gradle") {
        let build = read(root, "build.gradle")
            .or_else(|| read(root, "build.gradle.kts"))
            .unwrap_or_default();
        let spring = build.contains("org.springframework.boot");
        return Some(Project {
            kind: Kind::Gradle,
            spring_boot: spring,
            web: spring,
            wrapper: wrapper("gradlew", "gradlew.bat"),
        });
    }
    if has(root, "Cargo.toml") {
        return Some(Project {
            kind: Kind::Cargo,
            spring_boot: false,
            web: false,
            wrapper: None,
        });
    }
    if has(root, "package.json") {
        let pkg = read(root, "package.json").unwrap_or_default();
        // A front-end dev server, or a framework that serves one.
        let web = [
            "vite",
            "next",
            "react-scripts",
            "@angular",
            "nuxt",
            "svelte",
        ]
        .iter()
        .any(|n| pkg.contains(n))
            || pkg.contains("\"dev\"");
        return Some(Project {
            kind: Kind::Node,
            spring_boot: false,
            web,
            wrapper: None,
        });
    }
    if has(root, "go.mod") {
        return Some(Project {
            kind: Kind::Go,
            spring_boot: false,
            web: false,
            wrapper: None,
        });
    }
    if std::fs::read_dir(root).ok().is_some_and(|rd| {
        rd.flatten()
            .any(|e| e.path().extension().is_some_and(|x| x == "csproj"))
    }) {
        return Some(Project {
            kind: Kind::Dotnet,
            spring_boot: false,
            web: false,
            wrapper: None,
        });
    }
    if has(root, "pyproject.toml") || has(root, "requirements.txt") {
        let deps = read(root, "pyproject.toml")
            .or_else(|| read(root, "requirements.txt"))
            .unwrap_or_default();
        let web = ["flask", "django", "fastapi", "uvicorn"]
            .iter()
            .any(|n| deps.contains(n));
        return Some(Project {
            kind: Kind::Python,
            spring_boot: false,
            web,
            wrapper: None,
        });
    }
    if has(root, "CMakeLists.txt") {
        return Some(Project {
            kind: Kind::CMake,
            spring_boot: false,
            web: false,
            wrapper: None,
        });
    }
    if has(root, "Makefile") || has(root, "makefile") {
        return Some(Project {
            kind: Kind::Make,
            spring_boot: false,
            web: false,
            wrapper: None,
        });
    }
    None
}

/// Whether a file of this extension is the project's own code, so that
/// Run means "run the project" rather than "run this script". A Python
/// helper inside a Maven project still runs as a script.
pub fn owns_file(kind: Kind, ext: &str) -> bool {
    let exts: &[&str] = match kind {
        Kind::Cargo => &["rs"],
        Kind::Maven | Kind::Gradle => &["java", "kt", "kts", "groovy", "scala"],
        Kind::Node => &["js", "ts", "jsx", "tsx", "mjs", "cjs", "vue", "svelte"],
        Kind::Go => &["go"],
        Kind::Dotnet => &["cs", "fs", "vb"],
        Kind::Python => &["py"],
        Kind::Make | Kind::CMake => &["c", "cc", "cpp", "cxx", "h", "hpp"],
    };
    exts.contains(&ext)
}

/// Whether `file` is this project's own build file, so that Run and
/// Build mean the project while it is the open file - looking at pom.xml
/// and being offered nothing to run reads as the editor not knowing what
/// the project is. Owner's report, 2026-10-18.
pub fn is_manifest(kind: Kind, file: &str) -> bool {
    let lower = file.to_ascii_lowercase();
    match kind {
        Kind::Cargo => lower == "cargo.toml",
        Kind::Maven => lower == "pom.xml",
        Kind::Gradle => lower.starts_with("build.gradle") || lower.starts_with("settings.gradle"),
        Kind::Node => lower == "package.json",
        Kind::Go => lower == "go.mod",
        Kind::Dotnet => lower.ends_with(".csproj") || lower.ends_with(".sln"),
        Kind::Python => lower == "pyproject.toml" || lower == "requirements.txt",
        Kind::Make => lower == "makefile",
        Kind::CMake => lower == "cmakelists.txt",
    }
}

/// The command that starts Maven or Gradle: the project's wrapper when it
/// has one and the setting allows it, then whatever Settings → Editor says,
/// then the bare name from PATH.
fn tool(p: &Project, settings: &EditorSettings) -> String {
    let configured = match p.kind {
        Kind::Maven => settings.maven_command.trim(),
        Kind::Gradle => settings.gradle_command.trim(),
        _ => "",
    };
    if settings.prefer_wrapper
        && let Some(w) = &p.wrapper
    {
        return quote(w);
    }
    if !configured.is_empty() {
        return quote(configured);
    }
    match p.kind {
        Kind::Maven => "mvn".into(),
        Kind::Gradle => "gradle".into(),
        _ => String::new(),
    }
}

/// Wrap a path in quotes when it has a space, so a command line survives
/// `C:\Program Files\...`.
fn quote(s: &str) -> String {
    if s.contains(' ') && !s.starts_with('"') {
        format!("\"{s}\"")
    } else {
        s.to_string()
    }
}

/// What **Build** runs for this project.
pub fn build_command(p: &Project, settings: &EditorSettings) -> String {
    match p.kind {
        Kind::Cargo => "cargo build".into(),
        Kind::Maven => format!("{} package -DskipTests", tool(p, settings)),
        Kind::Gradle => format!("{} build -x test", tool(p, settings)),
        Kind::Node => "npm run build".into(),
        Kind::Go => "go build ./...".into(),
        Kind::Dotnet => "dotnet build".into(),
        Kind::Python => "python -m compileall .".into(),
        Kind::Make => "make".into(),
        Kind::CMake => "cmake --build build".into(),
    }
}

/// What **Run** runs for this project, with the project's own run
/// settings applied: a goal override, build profiles, skip-tests and
/// extra arguments. The environment (Spring profiles, JVM flags, the
/// user's variables) is set on the process rather than typed, so it
/// reaches the application whatever the shell is.
pub fn run_command_with(
    p: &Project,
    settings: &EditorSettings,
    run: &hs_core::projects::RunConfig,
) -> String {
    let base = match p.kind {
        Kind::Maven | Kind::Gradle if !run.goal.trim().is_empty() => {
            format!("{} {}", tool(p, settings), run.goal.trim())
        }
        _ => run_command(p, settings),
    };
    let mut parts = vec![base];
    let profiles = run.build_profiles.trim();
    match p.kind {
        Kind::Maven => {
            if !profiles.is_empty() {
                parts.push(format!("-P{}", profiles.replace(' ', "")));
            }
            if run.skip_tests {
                parts.push("-DskipTests".into());
            }
        }
        Kind::Gradle => {
            for prof in profiles.split(',').map(str::trim).filter(|x| !x.is_empty()) {
                parts.push(format!("-P{prof}"));
            }
            if run.skip_tests {
                parts.push("-x test".into());
            }
        }
        _ => {}
    }
    if !run.args.trim().is_empty() {
        parts.push(run.args.trim().to_string());
    }
    parts.join(" ")
}

/// What **Run** runs for this project.
pub fn run_command(p: &Project, settings: &EditorSettings) -> String {
    match p.kind {
        Kind::Cargo => "cargo run".into(),
        Kind::Maven if p.spring_boot => format!("{} spring-boot:run", tool(p, settings)),
        Kind::Maven => format!("{} -q compile exec:java", tool(p, settings)),
        Kind::Gradle if p.spring_boot => format!("{} bootRun", tool(p, settings)),
        Kind::Gradle => format!("{} run", tool(p, settings)),
        Kind::Node if p.web => "npm run dev".into(),
        Kind::Node => "npm start".into(),
        Kind::Go => "go run .".into(),
        Kind::Dotnet => "dotnet run".into(),
        Kind::Python => "python -m main".into(),
        Kind::Make => "make run".into(),
        Kind::CMake => "cmake --build build --target run".into(),
    }
}

/// The address a running web application printed, if this output names one.
///
/// Dev servers print a URL (`Local: http://localhost:5173/`); Spring Boot
/// prints a port instead ("Tomcat started on port(s): 8080 (http)"), so
/// both shapes are read. Guessing a port before the program says it would
/// open the browser on nothing.
pub fn served_url(output: &str) -> Option<String> {
    if let Some(url) = find_http(output) {
        return Some(url);
    }
    let lower = output.to_ascii_lowercase();
    let at = lower.find("tomcat started on port")?;
    let rest = &lower[at..];
    let digits: String = rest
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    (!digits.is_empty()).then(|| format!("http://localhost:{digits}"))
}

/// The first `http://localhost:<port>` (or 127.0.0.1) in the text.
fn find_http(output: &str) -> Option<String> {
    for host in ["http://localhost:", "http://127.0.0.1:"] {
        if let Some(at) = output.find(host) {
            let rest = &output[at + host.len()..];
            let port: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if !port.is_empty() {
                let tail: String = rest[port.len()..]
                    .chars()
                    .take_while(|c| !c.is_whitespace() && *c != '"' && *c != ',')
                    .collect();
                // Keep a path if the server printed one, drop a trailing
                // slash so the tab's address is the plain origin.
                let tail = tail.trim_end_matches('/');
                return Some(format!("{host}{port}{tail}"));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, body: &str) {
        std::fs::write(dir.join(name), body).unwrap();
    }

    fn settings() -> EditorSettings {
        EditorSettings::default()
    }

    #[test]
    fn a_spring_boot_maven_project_runs_its_own_goal_and_is_a_web_app() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "pom.xml",
            r#"<project><dependencies><dependency>
               <artifactId>spring-boot-starter-web</artifactId>
               </dependency></dependencies></project>"#,
        );
        let p = detect(tmp.path()).unwrap();
        assert_eq!(p.kind, Kind::Maven);
        assert!(p.spring_boot && p.web);
        assert_eq!(run_command(&p, &settings()), "mvn spring-boot:run");
        // The project's own run settings land on the command line; the
        // environment is set on the process, not typed.
        let run = hs_core::projects::RunConfig {
            build_profiles: "dev, fast".into(),
            skip_tests: true,
            args: "-Dserver.port=9090".into(),
            spring_profiles: "local".into(),
            ..Default::default()
        };
        assert_eq!(
            run_command_with(&p, &settings(), &run),
            "mvn spring-boot:run -Pdev,fast -DskipTests -Dserver.port=9090"
        );
        let goal = hs_core::projects::RunConfig {
            goal: "quarkus:dev".into(),
            ..Default::default()
        };
        assert_eq!(run_command_with(&p, &settings(), &goal), "mvn quarkus:dev");
        assert_eq!(build_command(&p, &settings()), "mvn package -DskipTests");
    }

    #[test]
    fn a_plain_maven_project_compiles_and_runs_its_main_class() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "pom.xml", "<project/>");
        let p = detect(tmp.path()).unwrap();
        assert!(!p.spring_boot && !p.web);
        assert_eq!(run_command(&p, &settings()), "mvn -q compile exec:java");
    }

    #[test]
    fn the_projects_wrapper_wins_over_the_configured_command() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "pom.xml", "<project/>");
        write(
            tmp.path(),
            if cfg!(windows) { "mvnw.cmd" } else { "mvnw" },
            "",
        );
        let p = detect(tmp.path()).unwrap();
        let mut st = settings();
        st.maven_command = r"C:\Tools\apache-maven\bin\mvn.cmd".into();
        let with_wrapper = run_command(&p, &st);
        assert!(
            with_wrapper.starts_with(if cfg!(windows) { "mvnw.cmd" } else { "./mvnw" }),
            "{with_wrapper}"
        );
        // Turning the preference off falls back to the configured path.
        st.prefer_wrapper = false;
        assert!(run_command(&p, &st).starts_with(r"C:\Tools\apache-maven\bin\mvn.cmd"));
    }

    #[test]
    fn a_configured_path_with_a_space_is_quoted() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "build.gradle", "plugins { id 'java' }");
        let p = detect(tmp.path()).unwrap();
        let mut st = settings();
        st.gradle_command = r"C:\Program Files\Gradle\bin\gradle.bat".into();
        assert_eq!(
            run_command(&p, &st),
            "\"C:\\Program Files\\Gradle\\bin\\gradle.bat\" run"
        );
    }

    #[test]
    fn a_spring_boot_gradle_project_uses_bootrun() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "build.gradle",
            "plugins { id 'org.springframework.boot' version '3.2.0' }",
        );
        let p = detect(tmp.path()).unwrap();
        assert!(p.spring_boot && p.web);
        assert_eq!(run_command(&p, &settings()), "gradle bootRun");
    }

    #[test]
    fn a_vite_project_is_a_web_app_and_runs_the_dev_server() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            tmp.path(),
            "package.json",
            r#"{"scripts":{"dev":"vite"},"devDependencies":{"vite":"^5"}}"#,
        );
        let p = detect(tmp.path()).unwrap();
        assert_eq!(p.kind, Kind::Node);
        assert!(p.web);
        assert_eq!(run_command(&p, &settings()), "npm run dev");
    }

    #[test]
    fn the_address_comes_from_the_programs_own_output() {
        assert_eq!(
            served_url("  ➜  Local:   http://localhost:5173/\n"),
            Some("http://localhost:5173".into())
        );
        assert_eq!(
            served_url("Tomcat started on port(s): 8080 (http) with context path ''"),
            Some("http://localhost:8080".into())
        );
        assert_eq!(
            served_url("Listening on http://127.0.0.1:3000/admin\n"),
            Some("http://127.0.0.1:3000/admin".into())
        );
        assert_eq!(served_url("compiling...\nBUILD SUCCESSFUL\n"), None);
    }

    #[test]
    fn a_folder_of_loose_files_is_not_a_project() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "notes.txt", "hello");
        assert!(detect(tmp.path()).is_none());
    }
}
