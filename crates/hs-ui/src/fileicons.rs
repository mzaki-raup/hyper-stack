//! Material-style file and folder icons for trees (the VS Code "Material
//! Icon Theme" SVGs, MIT, see `assets/icons/material/README.md`). Icons are
//! embedded and rendered by egui's SVG loader (`egui_extras` `svg` feature,
//! installed once with [`crate::icons::install_image_loaders`]).

use egui::ImageSource;

/// Every bundled icon by its Material name. Generated from the folder listing
/// by `scratchpad/gen_fileicons.py`; regenerate after adding SVGs.
pub static ICONS: &[(&str, ImageSource<'static>)] = &[
    (
        "assembly",
        egui::include_image!("../../../assets/icons/material/assembly.svg"),
    ),
    (
        "audio",
        egui::include_image!("../../../assets/icons/material/audio.svg"),
    ),
    (
        "babel",
        egui::include_image!("../../../assets/icons/material/babel.svg"),
    ),
    (
        "c",
        egui::include_image!("../../../assets/icons/material/c.svg"),
    ),
    (
        "certificate",
        egui::include_image!("../../../assets/icons/material/certificate.svg"),
    ),
    (
        "changelog",
        egui::include_image!("../../../assets/icons/material/changelog.svg"),
    ),
    (
        "clojure",
        egui::include_image!("../../../assets/icons/material/clojure.svg"),
    ),
    (
        "console",
        egui::include_image!("../../../assets/icons/material/console.svg"),
    ),
    (
        "cpp",
        egui::include_image!("../../../assets/icons/material/cpp.svg"),
    ),
    (
        "csharp",
        egui::include_image!("../../../assets/icons/material/csharp.svg"),
    ),
    (
        "css",
        egui::include_image!("../../../assets/icons/material/css.svg"),
    ),
    (
        "cypress",
        egui::include_image!("../../../assets/icons/material/cypress.svg"),
    ),
    (
        "dart",
        egui::include_image!("../../../assets/icons/material/dart.svg"),
    ),
    (
        "database",
        egui::include_image!("../../../assets/icons/material/database.svg"),
    ),
    (
        "diff",
        egui::include_image!("../../../assets/icons/material/diff.svg"),
    ),
    (
        "docker",
        egui::include_image!("../../../assets/icons/material/docker.svg"),
    ),
    (
        "document",
        egui::include_image!("../../../assets/icons/material/document.svg"),
    ),
    (
        "editorconfig",
        egui::include_image!("../../../assets/icons/material/editorconfig.svg"),
    ),
    (
        "elixir",
        egui::include_image!("../../../assets/icons/material/elixir.svg"),
    ),
    (
        "erlang",
        egui::include_image!("../../../assets/icons/material/erlang.svg"),
    ),
    (
        "eslint",
        egui::include_image!("../../../assets/icons/material/eslint.svg"),
    ),
    (
        "exe",
        egui::include_image!("../../../assets/icons/material/exe.svg"),
    ),
    (
        "folder-api",
        egui::include_image!("../../../assets/icons/material/folder-api.svg"),
    ),
    (
        "folder-app",
        egui::include_image!("../../../assets/icons/material/folder-app.svg"),
    ),
    (
        "folder-base",
        egui::include_image!("../../../assets/icons/material/folder-base.svg"),
    ),
    (
        "folder-ci",
        egui::include_image!("../../../assets/icons/material/folder-ci.svg"),
    ),
    (
        "folder-config",
        egui::include_image!("../../../assets/icons/material/folder-config.svg"),
    ),
    (
        "folder-core",
        egui::include_image!("../../../assets/icons/material/folder-core.svg"),
    ),
    (
        "folder-css",
        egui::include_image!("../../../assets/icons/material/folder-css.svg"),
    ),
    (
        "folder-database",
        egui::include_image!("../../../assets/icons/material/folder-database.svg"),
    ),
    (
        "folder-dist",
        egui::include_image!("../../../assets/icons/material/folder-dist.svg"),
    ),
    (
        "folder-docker",
        egui::include_image!("../../../assets/icons/material/folder-docker.svg"),
    ),
    (
        "folder-docs",
        egui::include_image!("../../../assets/icons/material/folder-docs.svg"),
    ),
    (
        "folder-examples",
        egui::include_image!("../../../assets/icons/material/folder-examples.svg"),
    ),
    (
        "folder-font",
        egui::include_image!("../../../assets/icons/material/folder-font.svg"),
    ),
    (
        "folder-git",
        egui::include_image!("../../../assets/icons/material/folder-git.svg"),
    ),
    (
        "folder-github",
        egui::include_image!("../../../assets/icons/material/folder-github.svg"),
    ),
    (
        "folder-i18n",
        egui::include_image!("../../../assets/icons/material/folder-i18n.svg"),
    ),
    (
        "folder-images",
        egui::include_image!("../../../assets/icons/material/folder-images.svg"),
    ),
    (
        "folder-javascript",
        egui::include_image!("../../../assets/icons/material/folder-javascript.svg"),
    ),
    (
        "folder-json",
        egui::include_image!("../../../assets/icons/material/folder-json.svg"),
    ),
    (
        "folder-lib",
        egui::include_image!("../../../assets/icons/material/folder-lib.svg"),
    ),
    (
        "folder-log",
        egui::include_image!("../../../assets/icons/material/folder-log.svg"),
    ),
    (
        "folder-markdown",
        egui::include_image!("../../../assets/icons/material/folder-markdown.svg"),
    ),
    (
        "folder-node",
        egui::include_image!("../../../assets/icons/material/folder-node.svg"),
    ),
    (
        "folder-other",
        egui::include_image!("../../../assets/icons/material/folder-other.svg"),
    ),
    (
        "folder-packages",
        egui::include_image!("../../../assets/icons/material/folder-packages.svg"),
    ),
    (
        "folder-powershell",
        egui::include_image!("../../../assets/icons/material/folder-powershell.svg"),
    ),
    (
        "folder-public",
        egui::include_image!("../../../assets/icons/material/folder-public.svg"),
    ),
    (
        "folder-python",
        egui::include_image!("../../../assets/icons/material/folder-python.svg"),
    ),
    (
        "folder-resource",
        egui::include_image!("../../../assets/icons/material/folder-resource.svg"),
    ),
    (
        "folder-rust",
        egui::include_image!("../../../assets/icons/material/folder-rust.svg"),
    ),
    (
        "folder-scripts",
        egui::include_image!("../../../assets/icons/material/folder-scripts.svg"),
    ),
    (
        "folder-secure",
        egui::include_image!("../../../assets/icons/material/folder-secure.svg"),
    ),
    (
        "folder-src",
        egui::include_image!("../../../assets/icons/material/folder-src.svg"),
    ),
    (
        "folder-target",
        egui::include_image!("../../../assets/icons/material/folder-target.svg"),
    ),
    (
        "folder-temp",
        egui::include_image!("../../../assets/icons/material/folder-temp.svg"),
    ),
    (
        "folder-terraform",
        egui::include_image!("../../../assets/icons/material/folder-terraform.svg"),
    ),
    (
        "folder-test",
        egui::include_image!("../../../assets/icons/material/folder-test.svg"),
    ),
    (
        "folder-tools",
        egui::include_image!("../../../assets/icons/material/folder-tools.svg"),
    ),
    (
        "folder-typescript",
        egui::include_image!("../../../assets/icons/material/folder-typescript.svg"),
    ),
    (
        "font",
        egui::include_image!("../../../assets/icons/material/font.svg"),
    ),
    (
        "fsharp",
        egui::include_image!("../../../assets/icons/material/fsharp.svg"),
    ),
    (
        "git",
        egui::include_image!("../../../assets/icons/material/git.svg"),
    ),
    (
        "github-actions-workflow",
        egui::include_image!("../../../assets/icons/material/github-actions-workflow.svg"),
    ),
    (
        "gitlab",
        egui::include_image!("../../../assets/icons/material/gitlab.svg"),
    ),
    (
        "go",
        egui::include_image!("../../../assets/icons/material/go.svg"),
    ),
    (
        "go-mod",
        egui::include_image!("../../../assets/icons/material/go-mod.svg"),
    ),
    (
        "gradle",
        egui::include_image!("../../../assets/icons/material/gradle.svg"),
    ),
    (
        "graphql",
        egui::include_image!("../../../assets/icons/material/graphql.svg"),
    ),
    (
        "groovy",
        egui::include_image!("../../../assets/icons/material/groovy.svg"),
    ),
    (
        "h",
        egui::include_image!("../../../assets/icons/material/h.svg"),
    ),
    (
        "handlebars",
        egui::include_image!("../../../assets/icons/material/handlebars.svg"),
    ),
    (
        "haskell",
        egui::include_image!("../../../assets/icons/material/haskell.svg"),
    ),
    (
        "hcl",
        egui::include_image!("../../../assets/icons/material/hcl.svg"),
    ),
    (
        "hpp",
        egui::include_image!("../../../assets/icons/material/hpp.svg"),
    ),
    (
        "html",
        egui::include_image!("../../../assets/icons/material/html.svg"),
    ),
    (
        "http",
        egui::include_image!("../../../assets/icons/material/http.svg"),
    ),
    (
        "image",
        egui::include_image!("../../../assets/icons/material/image.svg"),
    ),
    (
        "java",
        egui::include_image!("../../../assets/icons/material/java.svg"),
    ),
    (
        "javascript",
        egui::include_image!("../../../assets/icons/material/javascript.svg"),
    ),
    (
        "jest",
        egui::include_image!("../../../assets/icons/material/jest.svg"),
    ),
    (
        "jinja",
        egui::include_image!("../../../assets/icons/material/jinja.svg"),
    ),
    (
        "json",
        egui::include_image!("../../../assets/icons/material/json.svg"),
    ),
    (
        "julia",
        egui::include_image!("../../../assets/icons/material/julia.svg"),
    ),
    (
        "key",
        egui::include_image!("../../../assets/icons/material/key.svg"),
    ),
    (
        "kotlin",
        egui::include_image!("../../../assets/icons/material/kotlin.svg"),
    ),
    (
        "license",
        egui::include_image!("../../../assets/icons/material/license.svg"),
    ),
    (
        "lock",
        egui::include_image!("../../../assets/icons/material/lock.svg"),
    ),
    (
        "log",
        egui::include_image!("../../../assets/icons/material/log.svg"),
    ),
    (
        "lua",
        egui::include_image!("../../../assets/icons/material/lua.svg"),
    ),
    (
        "makefile",
        egui::include_image!("../../../assets/icons/material/makefile.svg"),
    ),
    (
        "markdown",
        egui::include_image!("../../../assets/icons/material/markdown.svg"),
    ),
    (
        "matlab",
        egui::include_image!("../../../assets/icons/material/matlab.svg"),
    ),
    (
        "maven",
        egui::include_image!("../../../assets/icons/material/maven.svg"),
    ),
    (
        "nginx",
        egui::include_image!("../../../assets/icons/material/nginx.svg"),
    ),
    (
        "nim",
        egui::include_image!("../../../assets/icons/material/nim.svg"),
    ),
    (
        "nodejs",
        egui::include_image!("../../../assets/icons/material/nodejs.svg"),
    ),
    (
        "npm",
        egui::include_image!("../../../assets/icons/material/npm.svg"),
    ),
    (
        "ocaml",
        egui::include_image!("../../../assets/icons/material/ocaml.svg"),
    ),
    (
        "pdf",
        egui::include_image!("../../../assets/icons/material/pdf.svg"),
    ),
    (
        "perl",
        egui::include_image!("../../../assets/icons/material/perl.svg"),
    ),
    (
        "php",
        egui::include_image!("../../../assets/icons/material/php.svg"),
    ),
    (
        "playwright",
        egui::include_image!("../../../assets/icons/material/playwright.svg"),
    ),
    (
        "powershell",
        egui::include_image!("../../../assets/icons/material/powershell.svg"),
    ),
    (
        "prettier",
        egui::include_image!("../../../assets/icons/material/prettier.svg"),
    ),
    (
        "prisma",
        egui::include_image!("../../../assets/icons/material/prisma.svg"),
    ),
    (
        "proto",
        egui::include_image!("../../../assets/icons/material/proto.svg"),
    ),
    (
        "pug",
        egui::include_image!("../../../assets/icons/material/pug.svg"),
    ),
    (
        "python",
        egui::include_image!("../../../assets/icons/material/python.svg"),
    ),
    (
        "r",
        egui::include_image!("../../../assets/icons/material/r.svg"),
    ),
    (
        "react",
        egui::include_image!("../../../assets/icons/material/react.svg"),
    ),
    (
        "readme",
        egui::include_image!("../../../assets/icons/material/readme.svg"),
    ),
    (
        "ruby",
        egui::include_image!("../../../assets/icons/material/ruby.svg"),
    ),
    (
        "rust",
        egui::include_image!("../../../assets/icons/material/rust.svg"),
    ),
    (
        "sass",
        egui::include_image!("../../../assets/icons/material/sass.svg"),
    ),
    (
        "scala",
        egui::include_image!("../../../assets/icons/material/scala.svg"),
    ),
    (
        "settings",
        egui::include_image!("../../../assets/icons/material/settings.svg"),
    ),
    (
        "storybook",
        egui::include_image!("../../../assets/icons/material/storybook.svg"),
    ),
    (
        "svelte",
        egui::include_image!("../../../assets/icons/material/svelte.svg"),
    ),
    (
        "svg",
        egui::include_image!("../../../assets/icons/material/svg.svg"),
    ),
    (
        "swift",
        egui::include_image!("../../../assets/icons/material/swift.svg"),
    ),
    (
        "table",
        egui::include_image!("../../../assets/icons/material/table.svg"),
    ),
    (
        "tailwindcss",
        egui::include_image!("../../../assets/icons/material/tailwindcss.svg"),
    ),
    (
        "terraform",
        egui::include_image!("../../../assets/icons/material/terraform.svg"),
    ),
    (
        "test-ts",
        egui::include_image!("../../../assets/icons/material/test-ts.svg"),
    ),
    (
        "tex",
        egui::include_image!("../../../assets/icons/material/tex.svg"),
    ),
    (
        "toml",
        egui::include_image!("../../../assets/icons/material/toml.svg"),
    ),
    (
        "tune",
        egui::include_image!("../../../assets/icons/material/tune.svg"),
    ),
    (
        "twig",
        egui::include_image!("../../../assets/icons/material/twig.svg"),
    ),
    (
        "typescript",
        egui::include_image!("../../../assets/icons/material/typescript.svg"),
    ),
    (
        "typescript-def",
        egui::include_image!("../../../assets/icons/material/typescript-def.svg"),
    ),
    (
        "video",
        egui::include_image!("../../../assets/icons/material/video.svg"),
    ),
    (
        "vite",
        egui::include_image!("../../../assets/icons/material/vite.svg"),
    ),
    (
        "vitest",
        egui::include_image!("../../../assets/icons/material/vitest.svg"),
    ),
    (
        "vue",
        egui::include_image!("../../../assets/icons/material/vue.svg"),
    ),
    (
        "webpack",
        egui::include_image!("../../../assets/icons/material/webpack.svg"),
    ),
    (
        "xml",
        egui::include_image!("../../../assets/icons/material/xml.svg"),
    ),
    (
        "yaml",
        egui::include_image!("../../../assets/icons/material/yaml.svg"),
    ),
    (
        "zig",
        egui::include_image!("../../../assets/icons/material/zig.svg"),
    ),
    (
        "zip",
        egui::include_image!("../../../assets/icons/material/zip.svg"),
    ),
];

pub fn by_name(name: &str) -> Option<ImageSource<'static>> {
    ICONS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, s)| s.clone())
}

/// Folder icon from the folder name (Material theme conventions), with
/// `folder-base` as the fallback.
pub fn for_folder(name: &str) -> ImageSource<'static> {
    let n = name.to_ascii_lowercase();
    let key = match n.as_str() {
        "src" | "source" | "sources" => "folder-src",
        "docs" | "doc" | "documentation" => "folder-docs",
        "scripts" | "script" | "bin" => "folder-scripts",
        ".git" => "folder-git",
        ".github" | ".forgejo" | ".gitea" | ".gitlab" => "folder-github",
        "config" | ".config" | "configs" | "settings" | ".cargo" | ".vscode" | ".idea" => {
            "folder-config"
        }
        "dist" | "build" | "out" | "release" => "folder-dist",
        "target" => "folder-target",
        "test" | "tests" | "spec" | "specs" | "__tests__" | "e2e" => "folder-test",
        "images" | "image" | "img" | "icons" | "icon" | "assets" | "media" => "folder-images",
        "node_modules" => "folder-node",
        "resources" | "resource" | "res" | "static" => "folder-resource",
        "locales" | "locale" | "i18n" | "lang" | "translations" => "folder-i18n",
        "tools" | "tool" | "utils" | "util" => "folder-tools",
        "keys" | "secrets" | "secret" | "certs" | ".ssh" | "security" => "folder-secure",
        "packages" | "package" | "crates" | "crate" | "modules" | "vendor" => "folder-packages",
        "lib" | "libs" | "library" => "folder-lib",
        "tmp" | "temp" | ".tmp" | "cache" | ".cache" => "folder-temp",
        "logs" | "log" => "folder-log",
        "db" | "database" | "data" | "migrations" => "folder-database",
        "public" | "www" | "wwwroot" => "folder-public",
        "app" | "apps" => "folder-app",
        "core" => "folder-core",
        "api" | "apis" => "folder-api",
        "ci" | ".ci" | "pipelines" => "folder-ci",
        "examples" | "example" | "samples" | "demo" => "folder-examples",
        "fonts" | "font" => "folder-font",
        "python" | "py" => "folder-python",
        "rust" => "folder-rust",
        "typescript" | "ts" => "folder-typescript",
        "javascript" | "js" => "folder-javascript",
        "docker" | ".docker" => "folder-docker",
        "terraform" | ".terraform" => "folder-terraform",
        "powershell" => "folder-powershell",
        "markdown" => "folder-markdown",
        "json" => "folder-json",
        "css" | "styles" | "style" => "folder-css",
        _ => "folder-other",
    };
    by_name(key)
        .or_else(|| by_name("folder-base"))
        .expect("folder-base is bundled")
}

/// File icon from the file name: well-known names first, then the extension.
pub fn for_file(name: &str) -> ImageSource<'static> {
    let lower = name.to_ascii_lowercase();
    let by_full = match lower.as_str() {
        "cargo.toml" | "cargo.lock" => Some("toml"),
        "readme" | "readme.md" | "readme.txt" => Some("readme"),
        "changelog" | "changelog.md" | "changes.md" | "history.md" => Some("changelog"),
        "license" | "license.md" | "license.txt" | "licence" | "licence.md" | "copying" => {
            Some("license")
        }
        ".editorconfig" => Some("editorconfig"),
        ".gitignore" | ".gitattributes" | ".gitmodules" | ".gitkeep" => Some("git"),
        "dockerfile"
        | "containerfile"
        | "docker-compose.yml"
        | "docker-compose.yaml"
        | "compose.yml"
        | "compose.yaml" => Some("docker"),
        "makefile" | "gnumakefile" | "justfile" => Some("makefile"),
        "package.json" | "package-lock.json" => Some("npm"),
        "deny.toml" | "rustfmt.toml" | "rust-toolchain" | "rust-toolchain.toml" => Some("rust"),
        "tsconfig.json" => Some("typescript-def"),
        "go.mod" | "go.sum" => Some("go-mod"),
        "nginx.conf" => Some("nginx"),
        "build.gradle" | "settings.gradle" | "build.gradle.kts" => Some("gradle"),
        "pom.xml" => Some("maven"),
        ".prettierrc" => Some("prettier"),
        ".eslintrc" | "eslint.config.js" => Some("eslint"),
        "vite.config.ts" | "vite.config.js" => Some("vite"),
        "tailwind.config.js" | "tailwind.config.ts" => Some("tailwindcss"),
        "webpack.config.js" => Some("webpack"),
        _ => None,
    };
    if let Some(k) = by_full {
        return by_name(k).unwrap_or_else(document);
    }
    if lower.starts_with(".env") {
        return by_name("tune").unwrap_or_else(document);
    }
    if lower.ends_with(".test.ts") || lower.ends_with(".spec.ts") {
        return by_name("test-ts").unwrap_or_else(document);
    }
    if lower.ends_with(".d.ts") {
        return by_name("typescript-def").unwrap_or_else(document);
    }
    let ext = lower.rsplit('.').next().unwrap_or("");
    let key = match ext {
        "rs" => "rust",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" | "jsx" => "react",
        "js" | "mjs" | "cjs" => "javascript",
        "json" | "jsonc" | "json5" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "md" | "markdown" | "mdx" => "markdown",
        "html" | "htm" | "xhtml" => "html",
        "css" => "css",
        "scss" | "sass" | "less" => "sass",
        "sql" | "pgsql" | "mysql" | "psql" | "db" | "sqlite" | "sqlite3" => "database",
        "sh" | "bash" | "zsh" | "fish" | "bat" | "cmd" => "console",
        "ps1" | "psm1" | "psd1" => "powershell",
        "py" | "pyi" | "pyw" => "python",
        "go" => "go",
        "java" | "jar" => "java",
        "kt" | "kts" => "kotlin",
        "cs" | "csproj" | "sln" => "csharp",
        "c" => "c",
        "h" => "h",
        "cpp" | "cc" | "cxx" => "cpp",
        "hpp" | "hh" | "hxx" => "hpp",
        "php" => "php",
        "rb" | "rake" | "gemspec" => "ruby",
        "lua" => "lua",
        "lock" => "lock",
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "ico" | "webp" | "tiff" | "avif" => "image",
        "svg" => "svg",
        "zip" | "gz" | "tgz" | "tar" | "7z" | "rar" | "xz" | "bz2" | "zst" => "zip",
        "pdf" => "pdf",
        "log" => "log",
        "xml" | "xsd" | "xsl" | "xslt" | "plist" => "xml",
        "ini" | "cfg" | "conf" | "properties" | "config" => "settings",
        "tf" | "tfvars" => "terraform",
        "hcl" | "nomad" => "hcl",
        "graphql" | "gql" => "graphql",
        "proto" => "proto",
        "diff" | "patch" => "diff",
        "ttf" | "otf" | "woff" | "woff2" | "eot" => "font",
        "pem" | "key" | "pub" | "asc" | "gpg" => "key",
        "crt" | "cer" | "p12" | "pfx" => "certificate",
        "vue" => "vue",
        "svelte" => "svelte",
        "swift" => "swift",
        "scala" => "scala",
        "pl" | "pm" => "perl",
        "r" | "rmd" => "r",
        "gradle" => "gradle",
        "exe" | "dll" | "msi" | "so" | "dylib" | "bin" => "exe",
        "http" | "rest" => "http",
        "csv" | "tsv" | "xls" | "xlsx" | "ods" => "table",
        "wasm" | "s" | "asm" => "assembly",
        "mp3" | "wav" | "flac" | "ogg" | "m4a" => "audio",
        "mp4" | "mkv" | "mov" | "avi" | "webm" => "video",
        "hs" => "haskell",
        "dart" => "dart",
        "ex" | "exs" => "elixir",
        "erl" | "hrl" => "erlang",
        "zig" => "zig",
        "nim" => "nim",
        "ml" | "mli" => "ocaml",
        "fs" | "fsx" => "fsharp",
        "clj" | "cljs" | "edn" => "clojure",
        "groovy" => "groovy",
        "jl" => "julia",
        "m" => "matlab",
        "tex" | "bib" => "tex",
        "pug" => "pug",
        "hbs" | "handlebars" => "handlebars",
        "twig" => "twig",
        "j2" | "jinja" | "jinja2" => "jinja",
        "prisma" => "prisma",
        _ => "document",
    };
    by_name(key).unwrap_or_else(document)
}

fn document() -> ImageSource<'static> {
    by_name("document").expect("document is bundled")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mapped_name_is_bundled() {
        for n in [
            "folder-base",
            "document",
            "rust",
            "toml",
            "readme",
            "git",
            "tune",
            "table",
            "console",
            "folder-src",
            "folder-secure",
            "typescript-def",
            "test-ts",
        ] {
            assert!(by_name(n).is_some(), "{n} is bundled");
        }
        assert!(ICONS.len() > 100);
    }
}
