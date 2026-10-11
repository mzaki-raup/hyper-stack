//! Environments and `{{variable}}` substitution (docs/05 §2.3–2.4).
//!
//! Scope chain, innermost first: caller-supplied scopes (request/collection) →
//! environment → parent environment(s) → workspace globals → built-ins.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::storage;

pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Environment {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub colour: Option<String>,
    pub protected: bool,
    pub parent: Option<String>,
    pub variables: HashMap<String, String>,
    /// Per tool kind, the default connection id(s).
    pub defaults: HashMap<String, toml::Value>,
}

impl Default for Environment {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            id: crate::new_id(),
            name: String::new(),
            colour: None,
            protected: false,
            parent: None,
            variables: HashMap::new(),
            defaults: HashMap::new(),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Environments {
    dir: PathBuf,
    envs: Vec<Environment>,
    pub globals: HashMap<String, String>,
    pub active: Option<String>,
}

impl Environments {
    pub fn load(dir: &Path) -> Self {
        let mut envs = Vec::new();
        if let Ok(files) = std::fs::read_dir(dir) {
            for f in files.flatten() {
                let p = f.path();
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if !name.ends_with(".toml") || name.ends_with(".local.toml") {
                    continue;
                }
                match storage::read_toml_versioned::<Environment>(&p, SCHEMA) {
                    Ok(mut e) => {
                        // `<name>.local.toml` overrides variables for this machine only.
                        let local = dir.join(name.replace(".toml", ".local.toml"));
                        if local.exists()
                            && let Ok(over) = storage::read_toml::<toml::Table>(&local)
                            && let Some(vars) = over.get("variables").and_then(|v| v.as_table())
                        {
                            for (k, v) in vars {
                                if let Some(s) = v.as_str() {
                                    e.variables.insert(k.clone(), s.to_string());
                                }
                            }
                        }
                        envs.push(e);
                    }
                    Err(e) => tracing::warn!("skipping environment {}: {e}", p.display()),
                }
            }
        }
        envs.sort_by_key(|a| a.name.to_lowercase());
        Self {
            dir: dir.to_path_buf(),
            envs,
            globals: HashMap::new(),
            active: None,
        }
    }

    pub fn all(&self) -> &[Environment] {
        &self.envs
    }
    pub fn by_id(&self, id: &str) -> Option<&Environment> {
        self.envs.iter().find(|e| e.id == id)
    }
    pub fn by_name(&self, name: &str) -> Option<&Environment> {
        self.envs.iter().find(|e| e.name == name)
    }
    pub fn active_env(&self) -> Option<&Environment> {
        self.active.as_deref().and_then(|id| self.by_id(id))
    }

    pub fn upsert(&mut self, mut e: Environment) -> storage::Result<()> {
        e.schema = SCHEMA;
        if let Some(old) = self.envs.iter().find(|x| x.id == e.id) {
            let old_file = self.dir.join(format!("{}.toml", storage::slug(&old.name)));
            if old.name != e.name && old_file.exists() {
                let _ = std::fs::remove_file(old_file);
            }
        }
        storage::write_toml(
            &self.dir.join(format!("{}.toml", storage::slug(&e.name))),
            &e,
        )?;
        self.envs.retain(|x| x.id != e.id);
        self.envs.push(e);
        self.envs.sort_by_key(|a| a.name.to_lowercase());
        Ok(())
    }

    /// Set a variable on an environment (the active one when `env_id` is
    /// `None`) and persist it. Errors when there is no such environment.
    pub fn set_var(&mut self, env_id: Option<&str>, key: &str, value: &str) -> storage::Result<()> {
        let id = env_id
            .map(str::to_string)
            .or_else(|| self.active.clone())
            .ok_or_else(|| storage::StorageError::Message("no active environment".into()))?;
        let mut env = self
            .by_id(&id)
            .cloned()
            .ok_or_else(|| storage::StorageError::Message("environment not found".into()))?;
        env.variables.insert(key.to_string(), value.to_string());
        self.upsert(env)
    }

    pub fn remove(&mut self, id: &str) -> storage::Result<()> {
        if let Some(e) = self.envs.iter().find(|x| x.id == id) {
            let f = self.dir.join(format!("{}.toml", storage::slug(&e.name)));
            if f.exists() {
                std::fs::remove_file(&f).map_err(|err| storage::StorageError::Io {
                    path: f.display().to_string(),
                    source: err,
                })?;
            }
        }
        self.envs.retain(|x| x.id != id);
        if self.active.as_deref() == Some(id) {
            self.active = None;
        }
        Ok(())
    }

    /// Look up a variable through the scope chain for `env_id` (or the active env).
    /// Every variable (key, value) visible for an environment, in resolution
    /// order: the environment itself, then its parent chain, then globals.
    /// Nearer scopes win, so a key is listed once with its effective value.
    /// Used to populate `{{ }}` autocomplete.
    pub fn available_vars(&self, env_id: Option<&str>) -> Vec<(String, String)> {
        let mut seen = std::collections::HashSet::new();
        let mut out: Vec<(String, String)> = Vec::new();
        let mut push = |k: &String, v: &String, seen: &mut std::collections::HashSet<String>| {
            if seen.insert(k.clone()) {
                out.push((k.clone(), v.clone()));
            }
        };
        let mut current = env_id.map(str::to_string).or_else(|| self.active.clone());
        let mut hops = 0;
        while let Some(id) = current {
            let Some(env) = self.by_id(&id) else { break };
            let mut keys: Vec<&String> = env.variables.keys().collect();
            keys.sort();
            for k in keys {
                push(k, &env.variables[k], &mut seen);
            }
            current = env.parent.clone();
            hops += 1;
            if hops > 16 {
                break;
            }
        }
        let mut gkeys: Vec<&String> = self.globals.keys().collect();
        gkeys.sort();
        for k in gkeys {
            push(k, &self.globals[k], &mut seen);
        }
        out
    }

    pub fn lookup(
        &self,
        name: &str,
        env_id: Option<&str>,
        extra: &[&HashMap<String, String>],
    ) -> Option<String> {
        for scope in extra {
            if let Some(v) = scope.get(name) {
                return Some(v.clone());
            }
        }
        let mut current = env_id.map(str::to_string).or_else(|| self.active.clone());
        let mut hops = 0;
        while let Some(id) = current {
            let Some(env) = self.by_id(&id) else { break };
            if let Some(v) = env.variables.get(name) {
                return Some(v.clone());
            }
            current = env.parent.clone();
            hops += 1;
            if hops > 16 {
                break; // parent cycle
            }
        }
        if let Some(v) = self.globals.get(name) {
            return Some(v.clone());
        }
        builtin(name)
    }

    /// Substitute every `{{name}}` in `input`. Unresolved names are returned
    /// in the error so the UI can highlight them.
    pub fn substitute(
        &self,
        input: &str,
        env_id: Option<&str>,
        extra: &[&HashMap<String, String>],
    ) -> Result<String, Vec<String>> {
        let mut out = String::with_capacity(input.len());
        let mut missing = Vec::new();
        let mut rest = input;
        while let Some(start) = rest.find("{{") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            match after.find("}}") {
                Some(end) => {
                    let name = after[..end].trim();
                    match self.lookup(name, env_id, extra) {
                        Some(v) => out.push_str(&v),
                        None => {
                            missing.push(name.to_string());
                            out.push_str(&rest[start..start + 2 + end + 2]);
                        }
                    }
                    rest = &after[end + 2..];
                }
                None => {
                    out.push_str(&rest[start..]);
                    rest = "";
                }
            }
        }
        out.push_str(rest);
        if missing.is_empty() {
            Ok(out)
        } else {
            Err(missing)
        }
    }
}

/// `{{$uuid}}`, `{{$ulid}}`, `{{$timestamp}}`, `{{$isoTimestamp}}`,
/// `{{$randomInt(min,max)}}`, `{{$env:NAME}}`.
fn builtin(name: &str) -> Option<String> {
    let n = name.strip_prefix('$')?;
    if n == "uuid" {
        return Some(uuid_v4());
    }
    if n == "ulid" {
        return Some(crate::new_id());
    }
    if n == "timestamp" {
        return Some(chrono::Utc::now().timestamp().to_string());
    }
    if n == "isoTimestamp" {
        return Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
    }
    if let Some(args) = n
        .strip_prefix("randomInt(")
        .and_then(|s| s.strip_suffix(')'))
    {
        let mut it = args.split(',').map(|s| s.trim().parse::<i64>());
        if let (Some(Ok(lo)), Some(Ok(hi))) = (it.next(), it.next())
            && hi >= lo
        {
            use rand::Rng;
            return Some(rand::rng().random_range(lo..=hi).to_string());
        }
        return None;
    }
    if let Some(var) = n.strip_prefix("env:") {
        return std::env::var(var).ok();
    }
    None
}

fn uuid_v4() -> String {
    use rand::RngCore;
    let mut b = [0u8; 16];
    rand::rng().fill_bytes(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
        b[5],
        b[6],
        b[7],
        b[8],
        b[9],
        b[10],
        b[11],
        b[12],
        b[13],
        b[14],
        b[15]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envs() -> Environments {
        let tmp = tempfile::tempdir().unwrap();
        let mut e = Environments::load(tmp.path());
        let base = Environment {
            name: "base".into(),
            variables: HashMap::from([
                ("api_base".into(), "https://base".into()),
                ("shared".into(), "b".into()),
            ]),
            ..Default::default()
        };
        let base_id = base.id.clone();
        let staging = Environment {
            name: "staging".into(),
            parent: Some(base_id),
            variables: HashMap::from([
                ("api_base".into(), "https://staging".into()),
                ("db_host".into(), "stg-db".into()),
            ]),
            ..Default::default()
        };
        e.upsert(base).unwrap();
        e.upsert(staging).unwrap();
        e.globals.insert("org".into(), "acme".into());
        std::mem::forget(tmp);
        e
    }

    #[test]
    fn scope_chain_resolves_innermost_first() {
        let e = envs();
        let stg = e.by_name("staging").unwrap().id.clone();
        let local = HashMap::from([("api_base".to_string(), "http://local".to_string())]);
        assert_eq!(
            e.substitute("{{api_base}}/x", Some(&stg), &[&local])
                .unwrap(),
            "http://local/x"
        );
        assert_eq!(
            e.substitute("{{api_base}}", Some(&stg), &[]).unwrap(),
            "https://staging"
        );
        assert_eq!(
            e.substitute("{{shared}}-{{org}}", Some(&stg), &[]).unwrap(),
            "b-acme"
        );
    }

    #[test]
    fn missing_variables_are_reported_and_left_in_place() {
        let e = envs();
        let stg = e.by_name("staging").unwrap().id.clone();
        let err = e
            .substitute("{{db_host}}:{{db_port}}", Some(&stg), &[])
            .unwrap_err();
        assert_eq!(err, vec!["db_port".to_string()]);
        assert_eq!(e.substitute("plain text", None, &[]).unwrap(), "plain text");
        assert_eq!(e.substitute("{{unclosed", None, &[]).unwrap(), "{{unclosed");
    }

    #[test]
    fn builtins_work() {
        let e = envs();
        let u = e.substitute("{{$uuid}}", None, &[]).unwrap();
        assert_eq!(u.len(), 36);
        assert_eq!(&u[14..15], "4");
        let n: i64 = e
            .substitute("{{$randomInt(5, 5)}}", None, &[])
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(n, 5);
        assert!(
            e.substitute("{{$timestamp}}", None, &[])
                .unwrap()
                .parse::<i64>()
                .is_ok()
        );
    }

    #[test]
    fn local_override_file_wins_and_is_not_written_back() {
        let tmp = tempfile::tempdir().unwrap();
        let mut e = Environments::load(tmp.path());
        e.upsert(Environment {
            name: "dev".into(),
            variables: HashMap::from([("db_user".into(), "app".into())]),
            ..Default::default()
        })
        .unwrap();
        std::fs::write(
            tmp.path().join("dev.local.toml"),
            "[variables]\ndb_user = \"me\"\n",
        )
        .unwrap();
        let e2 = Environments::load(tmp.path());
        assert_eq!(e2.by_name("dev").unwrap().variables["db_user"], "me");
        let on_disk: Environment = storage::read_toml(&tmp.path().join("dev.toml")).unwrap();
        assert_eq!(on_disk.variables["db_user"], "app");
    }
}
