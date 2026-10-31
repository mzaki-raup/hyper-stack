//! Connection profiles shared by every tool (docs/05 §2.2). One TOML file per
//! profile under `<workspace>/connections/<kind>/<slug>.toml`; kind-specific
//! fields stay a free-form table the owning tool interprets.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::storage;

pub const SCHEMA: u32 = 1;

/// The connection kinds the app knows, in the order they appear in pickers.
/// The id (lowercase) is what is stored; [`kind_label`] gives the display text.
pub const KINDS: &[&str] = &[
    "db",
    "ssh",
    "s3",
    "api",
    "docker",
    "podman",
    "k8s",
    "wincontainer",
    "redis",
    "valkey",
    "mongodb",
    "couchdb",
    "qdrant",
    "elasticsearch",
    "dynamodb",
    "cassandra",
    "webdav",
    "nexus",
    "rabbitmq",
    "kafka",
    "redis-streams",
    "nats",
    "mqtt",
    "jenkins",
    "teamcity",
    "github-actions",
    "gitlab-ci",
    "forgejo-actions",
    "awx",
];

/// The kinds the Container activity owns (ADR-026,
/// `docs/tools/containers.md` §10.1). A container host is an ordinary
/// connection profile, so the vault, per-environment hosts, read-only
/// handling, folders, tags, pinning and ordering are inherited rather
/// than rewritten - and Settings → Containers is a filtered view of this
/// one store, which is why it can never disagree with Settings →
/// Connections about the same host.
pub const CONTAINER_KINDS: &[&str] = &["docker", "podman", "k8s", "wincontainer"];

/// The kinds the **S3** tool opens.
///
/// All three are file-shaped stores the one browser can show, behind
/// `hs_core::objstore::ObjectStore`: S3 buckets, a WebDAV share and a
/// Nexus Repository's assets. What each can actually do differs, and
/// the store's `caps()` is what the pane asks before it offers a
/// control (`docs/tools/s3.md` §11).
pub const OBJECT_KINDS: &[&str] = &["s3", "webdav", "nexus"];

/// Whether a kind is opened by the S3 (object browser) tool.
pub fn is_object_kind(kind: &str) -> bool {
    OBJECT_KINDS.contains(&kind)
}

/// The kinds the **Redis** tool opens.
///
/// Valkey is a fork of Redis that speaks the same protocol, so the one
/// driver serves both and the client already tells them apart from
/// `INFO` (it reports `valkey_version` instead). The kind exists so
/// that someone running Valkey finds it by name when adding a
/// connection, rather than having to know it is "a Redis".
pub const REDIS_KINDS: &[&str] = &["redis", "valkey"];

/// Whether a kind is opened by the Redis tool.
pub fn is_redis_kind(kind: &str) -> bool {
    REDIS_KINDS.contains(&kind)
}

/// The kinds the **Queue** activity opens (`docs/tools/queue.md` §5).
///
/// `redis-streams` is a kind of its own rather than a reuse of `redis`:
/// the Redis activity opens a `redis` profile, and one profile cannot
/// belong to two activities without the sidebar of each listing the
/// other's servers.
pub const QUEUE_KINDS: &[&str] = &["rabbitmq", "kafka", "redis-streams", "nats", "mqtt"];

/// The kinds the **Automation** activity opens (`docs/tools/automation.md`
/// §5).
pub const AUTOMATION_KINDS: &[&str] = &[
    "jenkins",
    "teamcity",
    "github-actions",
    "gitlab-ci",
    "forgejo-actions",
    "awx",
];

/// Whether a kind is opened by the Automation activity.
pub fn is_automation_kind(kind: &str) -> bool {
    AUTOMATION_KINDS.contains(&kind)
}

/// Whether a kind is opened by the Queue activity.
pub fn is_queue_kind(kind: &str) -> bool {
    QUEUE_KINDS.contains(&kind)
}

/// Whether a kind belongs to the Container activity.
pub fn is_container_kind(kind: &str) -> bool {
    CONTAINER_KINDS.contains(&kind)
}

/// Whether a kind is a Kubernetes cluster rather than a container
/// engine. The two take different forms and different panes, and the
/// difference is asked for often enough to belong here.
pub fn is_cluster_kind(kind: &str) -> bool {
    kind == "k8s"
}

/// Display label for a connection kind id ("db" → "DB", "mongodb" → "MongoDB").
/// Unknown kinds are returned unchanged so custom ids still show.
pub fn kind_label(kind: &str) -> &str {
    match kind {
        "db" => "DB",
        "ssh" => "SSH",
        "s3" => "S3",
        "api" => "API",
        "docker" => "Docker",
        "podman" => "Podman",
        "k8s" => "Kubernetes",
        "wincontainer" => "Windows Container",
        "redis" => "Redis",
        "valkey" => "Valkey",
        "mongodb" => "MongoDB",
        "couchdb" => "CouchDB",
        "qdrant" => "Qdrant",
        "elasticsearch" => "Elasticsearch",
        "dynamodb" => "DynamoDB",
        "cassandra" => "Cassandra",
        "webdav" => "WebDAV",
        "nexus" => "Nexus",
        "rabbitmq" => "RabbitMQ",
        "kafka" => "Kafka",
        "redis-streams" => "Redis Streams",
        "nats" => "NATS",
        "mqtt" => "MQTT",
        "jenkins" => "Jenkins",
        "teamcity" => "TeamCity",
        "github-actions" => "GitHub Actions",
        "gitlab-ci" => "GitLab CI",
        "forgejo-actions" => "Forgejo Actions",
        "awx" => "AWX",
        other => other,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ConnectionProfile {
    pub schema: u32,
    pub id: String,
    pub name: String,
    /// db | ssh | s3 | api | docker | k8s | redis | mongodb | couchdb | elasticsearch | dynamodb | cassandra
    pub kind: String,
    pub folder: String,
    pub tags: Vec<String>,
    pub colour: Option<String>,
    pub environment: Option<String>,
    pub read_only: bool,
    /// Pinned to Home, so the connections someone actually uses are one
    /// click away. Home used to list every profile, which is not a pin.
    #[serde(default)]
    pub pinned: bool,
    /// Where this profile sits in the lists. Written by dragging a row;
    /// profiles that have never been dragged share 0 and fall back to
    /// sorting by name, which is what every list did before.
    #[serde(default)]
    pub order: i32,
    /// Kind-specific fields (`[db]`, `[ssh]`, …) plus optional `[tunnel]`.
    #[serde(flatten)]
    pub fields: toml::Table,
}

impl Default for ConnectionProfile {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            id: crate::new_id(),
            name: String::new(),
            kind: String::new(),
            folder: String::new(),
            tags: Vec::new(),
            colour: None,
            environment: None,
            read_only: false,
            order: 0,
            pinned: false,
            fields: toml::Table::new(),
        }
    }
}

/// Whether a profile belongs to the view being reordered. No kinds means
/// the whole list, as Settings → Connections shows it.
fn in_view(kinds: &[&str], kind: &str) -> bool {
    kinds.is_empty() || kinds.contains(&kind)
}

#[derive(Debug, Default, Clone)]
pub struct Connections {
    dir: PathBuf,
    profiles: Vec<ConnectionProfile>,
}

impl Connections {
    pub fn load(dir: &Path) -> Self {
        let mut profiles = Vec::new();
        if let Ok(kinds) = std::fs::read_dir(dir) {
            for kind in kinds.flatten().filter(|e| e.path().is_dir()) {
                if let Ok(files) = std::fs::read_dir(kind.path()) {
                    for f in files.flatten() {
                        let p = f.path();
                        if p.extension().is_some_and(|e| e == "toml") {
                            match storage::read_toml_versioned::<ConnectionProfile>(&p, SCHEMA) {
                                Ok(c) => profiles.push(c),
                                Err(e) => {
                                    tracing::warn!("skipping connection {}: {e}", p.display())
                                }
                            }
                        }
                    }
                }
            }
        }
        profiles.sort_by_key(|a| (a.order, a.name.to_lowercase()));
        Self {
            dir: dir.to_path_buf(),
            profiles,
        }
    }

    pub fn all(&self) -> &[ConnectionProfile] {
        &self.profiles
    }

    pub fn by_id(&self, id: &str) -> Option<&ConnectionProfile> {
        self.profiles.iter().find(|c| c.id == id)
    }

    pub fn of_kind<'a>(
        &'a self,
        kind: &'a str,
    ) -> impl Iterator<Item = &'a ConnectionProfile> + 'a {
        self.profiles.iter().filter(move |c| c.kind == kind)
    }

    /// Every profile the object browser opens, in order.
    pub fn of_object_kinds(&self) -> impl Iterator<Item = &ConnectionProfile> + '_ {
        self.profiles.iter().filter(|c| is_object_kind(&c.kind))
    }

    /// Every profile the Redis tool opens, in order. A helper rather
    /// than two `of_kind` calls at each site, so adding a third
    /// Redis-compatible server later is one list and not a hunt.
    pub fn of_redis_kinds(&self) -> impl Iterator<Item = &ConnectionProfile> + '_ {
        self.profiles.iter().filter(|c| is_redis_kind(&c.kind))
    }

    /// Every server the Automation activity opens, in order.
    pub fn of_automation_kinds(&self) -> impl Iterator<Item = &ConnectionProfile> + '_ {
        self.profiles.iter().filter(|c| is_automation_kind(&c.kind))
    }

    /// Every broker the Queue activity opens, in order.
    pub fn of_queue_kinds(&self) -> impl Iterator<Item = &ConnectionProfile> + '_ {
        self.profiles.iter().filter(|c| is_queue_kind(&c.kind))
    }

    fn file_for(&self, c: &ConnectionProfile) -> PathBuf {
        self.dir
            .join(&c.kind)
            .join(format!("{}.toml", storage::slug(&c.name)))
    }

    /// Insert or replace by id and write the file. A renamed profile's old file
    /// is removed.
    pub fn upsert(&mut self, mut c: ConnectionProfile) -> storage::Result<()> {
        c.schema = SCHEMA;
        if let Some(old) = self.profiles.iter().find(|p| p.id == c.id) {
            let old_file = self.file_for(old);
            if old_file != self.file_for(&c) && old_file.exists() {
                let _ = std::fs::remove_file(old_file);
            }
        }
        storage::write_toml(&self.file_for(&c), &c)?;
        self.profiles.retain(|p| p.id != c.id);
        self.profiles.push(c);
        self.profiles
            .sort_by_key(|a| (a.order, a.name.to_lowercase()));
        Ok(())
    }

    /// Move a profile within the list a tool shows — the kinds it displays,
    /// or every kind when `kinds` is empty — from row `from` to boundary
    /// `to`, and write the files whose order changed.
    ///
    /// Only the profiles in that view are renumbered, over the positions
    /// they already occupy, so reordering the SSH hosts never disturbs
    /// where the Redis connections sit. `kinds` must be in the same order
    /// the list was built in, which is store order.
    pub fn reorder(&mut self, kinds: &[&str], from: usize, to: usize) -> storage::Result<()> {
        let mut ids: Vec<String> = self
            .profiles
            .iter()
            .filter(|c| in_view(kinds, &c.kind))
            .map(|c| c.id.clone())
            .collect();
        if from >= ids.len() || to > ids.len() || to == from || to == from + 1 {
            return Ok(());
        }
        let id = ids.remove(from);
        let at = if to > from { to - 1 } else { to };
        ids.insert(at, id);
        // The order values this view already uses, handed out in the new
        // sequence. Ties on 0 (nothing dragged yet) become 0, 1, 2, …
        let mut slots: Vec<i32> = self
            .profiles
            .iter()
            .filter(|c| in_view(kinds, &c.kind))
            .map(|c| c.order)
            .collect();
        if slots.windows(2).any(|w| w[0] == w[1]) {
            slots = (0..slots.len() as i32).collect();
        }
        let mut changed: Vec<ConnectionProfile> = Vec::new();
        for (slot, id) in slots.into_iter().zip(ids) {
            if let Some(c) = self.profiles.iter_mut().find(|c| c.id == id)
                && c.order != slot
            {
                c.order = slot;
                changed.push(c.clone());
            }
        }
        for c in &changed {
            storage::write_toml(&self.file_for(c), c)?;
        }
        self.profiles
            .sort_by_key(|a| (a.order, a.name.to_lowercase()));
        Ok(())
    }

    /// Pin or unpin a profile and write it back.
    pub fn set_pinned(&mut self, id: &str, pinned: bool) -> storage::Result<()> {
        let Some(c) = self.profiles.iter_mut().find(|p| p.id == id) else {
            return Ok(());
        };
        if c.pinned == pinned {
            return Ok(());
        }
        c.pinned = pinned;
        let c = c.clone();
        storage::write_toml(&self.file_for(&c), &c)
    }

    /// The pinned profiles, in list order.
    pub fn pinned(&self) -> impl Iterator<Item = &ConnectionProfile> {
        self.profiles.iter().filter(|c| c.pinned)
    }

    pub fn remove(&mut self, id: &str) -> storage::Result<()> {
        if let Some(c) = self.profiles.iter().find(|p| p.id == id) {
            let f = self.file_for(c);
            if f.exists() {
                std::fs::remove_file(&f).map_err(|e| storage::StorageError::Io {
                    path: f.display().to_string(),
                    source: e,
                })?;
            }
        }
        self.profiles.retain(|p| p.id != id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every kind a tool claims has to be offered when adding a
    /// connection, or the tool opens something nobody can create.
    #[test]
    fn every_claimed_kind_is_one_a_user_can_add() {
        for k in REDIS_KINDS
            .iter()
            .chain(CONTAINER_KINDS)
            .chain(QUEUE_KINDS)
            .chain(AUTOMATION_KINDS)
        {
            assert!(KINDS.contains(k), "{k} is claimed but not offered");
            assert_ne!(kind_label(k), *k, "{k} has no label of its own");
        }
        assert!(
            is_redis_kind("valkey"),
            "Valkey is opened by the Redis tool"
        );
        assert!(is_redis_kind("redis"));
        assert!(!is_redis_kind("mongodb"));
        // A Redis server used for streams is the Queue activity's, and
        // a plain Redis profile stays the Redis activity's.
        assert!(is_queue_kind("redis-streams"));
        assert!(!is_queue_kind("redis"));
        assert!(!is_redis_kind("redis-streams"));
    }

    #[test]
    fn profiles_persist_one_file_per_kind_and_survive_rename() {
        let tmp = tempfile::tempdir().unwrap();
        let mut conns = Connections::load(tmp.path());
        let mut fields = toml::Table::new();
        let mut db = toml::Table::new();
        db.insert("engine".into(), toml::Value::String("postgres".into()));
        db.insert("host".into(), toml::Value::String("{{db_host}}".into()));
        fields.insert("db".into(), toml::Value::Table(db));
        let c = ConnectionProfile {
            name: "orders — staging".into(),
            kind: "db".into(),
            fields,
            ..Default::default()
        };
        let id = c.id.clone();
        conns.upsert(c).unwrap();
        assert!(tmp.path().join("db").join("orders-staging.toml").exists());

        let mut c2 = conns.by_id(&id).unwrap().clone();
        c2.name = "orders — prod".into();
        conns.upsert(c2).unwrap();
        assert!(!tmp.path().join("db").join("orders-staging.toml").exists());
        assert!(tmp.path().join("db").join("orders-prod.toml").exists());

        let reloaded = Connections::load(tmp.path());
        let back = reloaded.by_id(&id).unwrap();
        assert_eq!(back.fields["db"]["engine"].as_str(), Some("postgres"));
        assert_eq!(reloaded.of_kind("db").count(), 1);
    }

    fn named(name: &str, kind: &str) -> ConnectionProfile {
        ConnectionProfile {
            name: name.into(),
            kind: kind.into(),
            ..Default::default()
        }
    }

    fn names(c: &Connections, kinds: &[&str]) -> Vec<String> {
        c.all()
            .iter()
            .filter(|p| in_view(kinds, &p.kind))
            .map(|p| p.name.clone())
            .collect()
    }

    #[test]
    fn reordering_one_kind_leaves_the_others_where_they_were() {
        let tmp = tempfile::tempdir().unwrap();
        let mut c = Connections::load(tmp.path());
        for (n, k) in [
            ("alpha", "ssh"),
            ("bravo", "ssh"),
            ("charlie", "ssh"),
            ("delta", "redis"),
            ("echo", "redis"),
        ] {
            c.upsert(named(n, k)).unwrap();
        }
        let redis_before = names(&c, &["redis"]);
        // charlie to the top of the SSH list.
        c.reorder(&["ssh"], 2, 0).unwrap();
        assert_eq!(names(&c, &["ssh"]), ["charlie", "alpha", "bravo"]);
        assert_eq!(names(&c, &["redis"]), redis_before, "redis was disturbed");

        // The order survives a reload, which is the point of writing it.
        let again = Connections::load(tmp.path());
        assert_eq!(names(&again, &["ssh"]), ["charlie", "alpha", "bravo"]);
    }

    #[test]
    fn a_view_of_several_kinds_reorders_across_them() {
        let tmp = tempfile::tempdir().unwrap();
        let mut c = Connections::load(tmp.path());
        for (n, k) in [("m1", "mongodb"), ("c1", "couchdb"), ("m2", "mongodb")] {
            c.upsert(named(n, k)).unwrap();
        }
        let kinds = ["mongodb", "couchdb"];
        assert_eq!(names(&c, &kinds), ["c1", "m1", "m2"]);
        c.reorder(&kinds, 2, 0).unwrap();
        assert_eq!(names(&c, &kinds), ["m2", "c1", "m1"]);
    }

    #[test]
    fn an_untouched_store_still_sorts_by_name() {
        let tmp = tempfile::tempdir().unwrap();
        let mut c = Connections::load(tmp.path());
        for n in ["zulu", "alpha", "mike"] {
            c.upsert(named(n, "ssh")).unwrap();
        }
        assert_eq!(names(&c, &["ssh"]), ["alpha", "mike", "zulu"]);
    }
}
