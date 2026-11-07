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
    /// The tier set by hand; `None` means matched from the name
    /// ([`matched_tier`], FEAT-0106).
    #[serde(default)]
    pub tier: Option<Tier>,
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
            tier: None,
        }
    }
}

/// How far an environment is from production (FEAT-0106): it chooses the
/// environment's icon and colour, and Production and Production Support
/// default to protected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    Local,
    Development,
    Integration,
    Testing,
    Training,
    Acceptance,
    Staging,
    ProductionSupport,
    Production,
}

impl Tier {
    pub const ALL: [Tier; 9] = [
        Tier::Local,
        Tier::Development,
        Tier::Integration,
        Tier::Testing,
        Tier::Training,
        Tier::Acceptance,
        Tier::Staging,
        Tier::ProductionSupport,
        Tier::Production,
    ];

    /// The tier's English name, for logs and tests; the UI shows
    /// [`Tier::i18n_key`] through Fluent.
    pub fn label(self) -> &'static str {
        match self {
            Tier::Local => "Local",
            Tier::Development => "Development",
            Tier::Integration => "Integration",
            Tier::Testing => "Testing",
            Tier::Training => "Training",
            Tier::Acceptance => "Acceptance",
            Tier::Staging => "Staging",
            Tier::ProductionSupport => "Production Support",
            Tier::Production => "Production",
        }
    }

    /// The Fluent message id of the tier's name.
    pub fn i18n_key(self) -> &'static str {
        match self {
            Tier::Local => "env-tier-local",
            Tier::Development => "env-tier-development",
            Tier::Integration => "env-tier-integration",
            Tier::Testing => "env-tier-testing",
            Tier::Training => "env-tier-training",
            Tier::Acceptance => "env-tier-acceptance",
            Tier::Staging => "env-tier-staging",
            Tier::ProductionSupport => "env-tier-production-support",
            Tier::Production => "env-tier-production",
        }
    }

    /// Production and Production Support are read-only by default.
    pub fn is_production(self) -> bool {
        matches!(self, Tier::Production | Tier::ProductionSupport)
    }
}

/// The tier an environment's name implies, by keyword (FEAT-0106). Case
/// is ignored and the name is split on anything not a letter or digit,
/// with trailing digits dropped from each word (`uat2` reads as `uat`).
/// Production Support is checked before Production, and "pre prod"
/// before both, so `PROD-SUPPORT` and `PREPROD` are not Production.
pub fn matched_tier(name: &str) -> Option<Tier> {
    let lower = name.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .map(|w| w.trim_end_matches(|c: char| c.is_ascii_digit()))
        .filter(|w| !w.is_empty())
        .collect();
    let has = |set: &[&str]| words.iter().any(|w| set.iter().any(|k| k == w));
    if has(&["prodsupport", "prodsupp", "psupport", "hotfix"])
        || (has(&["prod", "production"]) && has(&["support", "supp"]))
    {
        return Some(Tier::ProductionSupport);
    }
    if has(&["preprod", "uat", "acceptance"])
        || words.windows(2).any(|p| p[0] == "pre" && p[1] == "prod")
    {
        return Some(Tier::Acceptance);
    }
    if has(&["prod", "production", "live", "prd"]) {
        return Some(Tier::Production);
    }
    let rest: [(Tier, &[&str]); 6] = [
        (Tier::Staging, &["staging", "stage", "stg"]),
        (Tier::Training, &["training", "trn", "demo"]),
        (Tier::Testing, &["test", "testing", "qa"]),
        (Tier::Integration, &["sit", "int", "integration"]),
        (
            Tier::Development,
            &["dev", "development", "lower", "sandbox"],
        ),
        (Tier::Local, &["local", "localhost"]),
    ];
    rest.into_iter().find(|&(_, set)| has(set)).map(|(t, _)| t)
}

impl Environment {
    /// The tier set by hand, else the one matched from the name.
    pub fn effective_tier(&self) -> Option<Tier> {
        self.tier.or_else(|| matched_tier(&self.name))
    }

    /// Production and Production Support default to protected: called
    /// when an environment is created and when its tier is set or
    /// changed, never on load, so the user's later choice stands.
    pub fn apply_protected_default(&mut self) {
        if self.effective_tier().is_some_and(Tier::is_production) {
            self.protected = true;
        }
    }

    /// Set the manual tier (`None` = automatic) and, when it changed,
    /// apply the protected default.
    pub fn set_tier(&mut self, tier: Option<Tier>) {
        if self.tier != tier {
            self.tier = tier;
            self.apply_protected_default();
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
        if self.by_id(&e.id).is_none() {
            // A new environment: Production and Production Support start
            // protected (FEAT-0106).
            e.apply_protected_default();
        }
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

impl Environments {
    /// Substitute what can be substituted and leave the rest as written.
    ///
    /// A note or a page is not a request: an unknown `{{name}}` in prose
    /// is not an error to refuse the whole text over, it is simply text
    /// the reader wrote (owner's requirement for the Scratchpad,
    /// 2026-10-29). [`substitute`] stays strict for the tools that must
    /// not send half a URL.
    pub fn substitute_lenient(&self, input: &str, env_id: Option<&str>) -> String {
        let mut out = String::with_capacity(input.len());
        let mut rest = input;
        while let Some(start) = rest.find("{{") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            match after.find("}}") {
                Some(end) => {
                    let name = after[..end].trim();
                    match self.lookup(name, env_id, &[]) {
                        Some(v) => out.push_str(&v),
                        None => out.push_str(&rest[start..start + 2 + end + 2]),
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
        out
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
        // Lenient: the known name resolves, the unknown stays as written.
        assert_eq!(
            e.substitute_lenient("{{api_base}} and {{nope}}", Some(&stg)),
            format!(
                "{} and {{{{nope}}}}",
                e.substitute("{{api_base}}", Some(&stg), &[]).unwrap()
            )
        );
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

    #[test]
    fn every_listed_name_maps_to_its_tier() {
        let cases = [
            ("LOCAL", Tier::Local),
            ("LOCALHOST", Tier::Local),
            ("LOWER", Tier::Development),
            ("DEV", Tier::Development),
            ("sandbox", Tier::Development),
            ("SIT", Tier::Integration),
            ("integration", Tier::Integration),
            ("TESTING", Tier::Testing),
            ("QA", Tier::Testing),
            ("TRAINING", Tier::Training),
            ("demo", Tier::Training),
            ("UAT", Tier::Acceptance),
            ("PREPROD", Tier::Acceptance),
            ("pre prod", Tier::Acceptance),
            ("uat-eu", Tier::Acceptance),
            ("UAT_2", Tier::Acceptance),
            ("STAGING", Tier::Staging),
            ("stg", Tier::Staging),
            ("PROD-SUPPORT", Tier::ProductionSupport),
            ("PRODSUPPORT", Tier::ProductionSupport),
            ("PROD_SUPP", Tier::ProductionSupport),
            ("HOTFIX", Tier::ProductionSupport),
            ("PROD", Tier::Production),
            ("PRODUCTION", Tier::Production),
            ("live", Tier::Production),
            ("prd", Tier::Production),
        ];
        for (name, tier) in cases {
            assert_eq!(matched_tier(name), Some(tier), "{name}");
        }
        assert_ne!(matched_tier("PROD-SUPPORT"), Some(Tier::Production));
        assert_eq!(matched_tier("customer-a"), None);
        assert_eq!(matched_tier(""), None);
    }

    #[test]
    fn a_manual_tier_wins_over_the_name() {
        let mut e = Environment {
            name: "uat".into(),
            ..Default::default()
        };
        assert_eq!(e.effective_tier(), Some(Tier::Acceptance));
        e.tier = Some(Tier::Training);
        assert_eq!(e.effective_tier(), Some(Tier::Training));
        e.tier = None;
        e.name = "customer-a".into();
        assert_eq!(e.effective_tier(), None);
    }

    #[test]
    fn production_tiers_default_to_protected_on_create_and_tier_change() {
        let tmp = tempfile::tempdir().unwrap();
        let mut envs = Environments::load(tmp.path());
        for name in ["PROD", "prod-support", "dev"] {
            envs.upsert(Environment {
                name: name.into(),
                ..Default::default()
            })
            .unwrap();
        }
        assert!(envs.by_name("PROD").unwrap().protected);
        assert!(envs.by_name("prod-support").unwrap().protected);
        assert!(!envs.by_name("dev").unwrap().protected);

        // The user may untick it; saving again keeps their choice.
        let mut prod = envs.by_name("PROD").unwrap().clone();
        prod.protected = false;
        envs.upsert(prod).unwrap();
        assert!(!envs.by_name("PROD").unwrap().protected);
        // Nor does loading change it.
        let reloaded = Environments::load(tmp.path());
        assert!(!reloaded.by_name("PROD").unwrap().protected);

        // A tier set to Production Support protects; an unchanged tier
        // does not re-apply it; a non-production tier leaves it alone.
        let mut dev = envs.by_name("dev").unwrap().clone();
        dev.set_tier(Some(Tier::Staging));
        assert!(!dev.protected);
        dev.set_tier(Some(Tier::ProductionSupport));
        assert!(dev.protected);
        dev.protected = false;
        dev.set_tier(Some(Tier::ProductionSupport));
        assert!(!dev.protected);
    }

    #[test]
    fn the_tier_round_trips_with_and_without_a_value() {
        let tmp = tempfile::tempdir().unwrap();
        let mut envs = Environments::load(tmp.path());
        envs.upsert(Environment {
            name: "alpha".into(),
            tier: Some(Tier::ProductionSupport),
            ..Default::default()
        })
        .unwrap();
        envs.upsert(Environment {
            name: "beta".into(),
            ..Default::default()
        })
        .unwrap();
        let text = std::fs::read_to_string(tmp.path().join("alpha.toml")).unwrap();
        assert!(text.contains("tier = \"production-support\""), "{text}");
        let reloaded = Environments::load(tmp.path());
        assert_eq!(
            reloaded.by_name("alpha").unwrap().tier,
            Some(Tier::ProductionSupport)
        );
        assert_eq!(reloaded.by_name("beta").unwrap().tier, None);
        // A file written before tiers existed still loads.
        std::fs::write(
            tmp.path().join("old.toml"),
            "schema = 1\nid = \"x\"\nname = \"old\"\nprotected = false\n",
        )
        .unwrap();
        let old = Environments::load(tmp.path());
        assert_eq!(old.by_name("old").unwrap().tier, None);
    }
}
