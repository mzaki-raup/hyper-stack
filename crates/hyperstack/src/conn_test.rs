//! "Test Connection" on the Settings → Connections form.
//!
//! The point of the button is to answer "will this work?" before the profile
//! is saved, so the test opens the connection for real with the same driver
//! the tool would use, reports what the server said, and drops it again.
//! Nothing is written: the vault is only read, and a failed test leaves the
//! form exactly as it was.
//!
//! Every kind with a driver is covered (SSH, S3, SQL, Redis, MongoDB,
//! CouchDB); the rest say so rather than pretending to pass.

use hs_core::Services;
use hs_core::connections::ConnectionProfile;
use parking_lot::Mutex;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// What the form shows under the buttons. Each variant carries data, not a
/// sentence — the wording is Fluent's job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Running,
    /// Connected. The detail is what the server said about itself (a
    /// version, a bucket count); it can be empty.
    Ok(String),
    Failed(String),
    /// SSH only: the server answered but its host key has never been
    /// accepted, so there is nothing to authenticate against yet.
    UnknownHostKey(String),
    /// The kind has no driver to test (an API collection, say).
    NotTestable,
    /// Nothing was tried: the profile could not be resolved into a config.
    /// These are our own conditions, not the driver's, so they are named
    /// rather than stringified and the page can word them.
    VaultLocked,
    /// `{{variables}}` the environment does not define, comma-separated.
    MissingVariables(String),
    /// Password authentication with no password stored (SSH).
    NoPassword,
    /// No access key to sign with (S3).
    NoKeys,
    UnknownEngine(String),
}

fn db_error(e: tool_db::profile::ProfileError) -> Outcome {
    use tool_db::profile::ProfileError as E;
    match e {
        E::VaultLocked => Outcome::VaultLocked,
        E::MissingVariables(v) => Outcome::MissingVariables(v.join(", ")),
        E::UnknownEngine(x) => Outcome::UnknownEngine(x),
        E::Secret(s) => Outcome::Failed(s),
    }
}

fn redis_error(e: tool_redis::profile::ProfileError) -> Outcome {
    use tool_redis::profile::ProfileError as E;
    match e {
        E::VaultLocked => Outcome::VaultLocked,
        E::MissingVariables(v) => Outcome::MissingVariables(v.join(", ")),
        other => Outcome::Failed(other.to_string()),
    }
}

fn nosql_error(e: tool_nosql::profile::ProfileError) -> Outcome {
    use tool_nosql::profile::ProfileError as E;
    match e {
        E::VaultLocked => Outcome::VaultLocked,
        E::MissingVariables(v) => Outcome::MissingVariables(v.join(", ")),
        other => Outcome::Failed(other.to_string()),
    }
}

fn s3_error(e: hs_core::objstore::profile::ProfileError) -> Outcome {
    use hs_core::objstore::profile::ProfileError as E;
    match e {
        E::VaultLocked => Outcome::VaultLocked,
        E::NoKeys => Outcome::NoKeys,
        E::Secret(s) => Outcome::Failed(s),
    }
}

fn ssh_error(e: hs_ssh::config::ConfigError) -> Outcome {
    use hs_ssh::config::ConfigError as E;
    match e {
        E::VaultLocked => Outcome::VaultLocked,
        E::NoPassword => Outcome::NoPassword,
        E::Secret(s) => Outcome::Failed(s),
    }
}

/// One test, owned by the settings page so a slow answer lands on the
/// connection it was started for and not on whichever is selected by then.
pub struct Test {
    pub profile_id: String,
    slot: Arc<Mutex<Outcome>>,
    /// Aborts the spawned connect (the runtime's task handle, kept behind a
    /// closure so this crate need not name the runtime's types).
    abort: Option<Box<dyn FnOnce() + Send>>,
}

impl Drop for Test {
    /// Closing the form must not leave a connect running (a slow host with
    /// no connect timeout would hold a session until it answered).
    fn drop(&mut self) {
        if let Some(abort) = self.abort.take() {
            abort();
        }
    }
}

impl Test {
    pub fn outcome(&self) -> Outcome {
        self.slot.lock().clone()
    }

    pub fn running(&self) -> bool {
        self.outcome() == Outcome::Running
    }
}

/// Whether the button should be offered at all.
pub fn testable(kind: &str) -> bool {
    matches!(
        kind,
        "ssh"
            | "s3"
            | "webdav"
            | "nexus"
            | "db"
            | "redis"
            | "valkey"
            | "mongodb"
            | "couchdb"
            | "qdrant"
            | "docker"
            | "podman"
            | "wincontainer"
            | "k8s"
    )
}

type Probe = Pin<Box<dyn Future<Output = Outcome> + Send>>;

/// Start a test. Everything that needs `&Services` — the environment
/// substitution and the vault lookup — happens here on the UI thread, which
/// is cheap; only the connect goes to the runtime.
pub fn start(s: &Services, profile: &ConnectionProfile) -> Test {
    let slot = Arc::new(Mutex::new(Outcome::Running));
    let mut test = Test {
        profile_id: profile.id.clone(),
        slot: slot.clone(),
        abort: None,
    };
    match probe(s, profile) {
        Ok(fut) => {
            let waker = s.runtime.waker();
            let handle = s.runtime.spawn(async move {
                let outcome = fut.await;
                *slot.lock() = outcome;
                waker.request_repaint();
            });
            let abort = handle.abort_handle();
            test.abort = Some(Box::new(move || abort.abort()));
        }
        // Nothing to await: a resolution failure is already the answer.
        Err(outcome) => *slot.lock() = outcome,
    }
    test
}

fn probe(s: &Services, profile: &ConnectionProfile) -> Result<Probe, Outcome> {
    let read_only = profile.read_only;
    let lookup = {
        let envs = s.environments.read().clone();
        let env = profile.environment.clone();
        move |v: &str| envs.substitute(v, env.as_deref(), &[])
    };
    let secrets = s.secrets.as_ref();
    let fields = &profile.fields;
    match profile.kind.as_str() {
        "db" => {
            let cfg = tool_db::profile::DbFields::from_table(fields)
                .resolve(read_only, secrets, &lookup)
                .map_err(db_error)?;
            Ok(Box::pin(async move {
                match tool_db::driver::connect(&cfg).await {
                    Ok(session) => Outcome::Ok(session.server_info().version.clone()),
                    Err(e) => Outcome::Failed(e.to_string()),
                }
            }))
        }
        "redis" | "valkey" => {
            let cfg = tool_redis::profile::RedisFields::from_table(fields)
                .resolve(read_only, secrets, &lookup)
                .map_err(redis_error)?;
            Ok(Box::pin(async move {
                match tool_redis::client::Session::connect(&cfg).await {
                    Ok(session) => {
                        let i = session.server_info();
                        Outcome::Ok(match (i.flavour.as_str(), i.version.as_str()) {
                            ("", "") => String::new(),
                            ("", v) => v.to_string(),
                            (f, "") => f.to_string(),
                            (f, v) => format!("{f} {v}"),
                        })
                    }
                    Err(e) => Outcome::Failed(e.to_string()),
                }
            }))
        }
        "mongodb" => {
            let cfg = tool_nosql::profile::NoSqlFields::from_table("mongodb", fields)
                .resolve_mongo(read_only, secrets, &lookup)
                .map_err(nosql_error)?;
            Ok(Box::pin(async move {
                match tool_nosql::mongo::MongoStore::connect(&cfg).await {
                    Ok(store) => {
                        use tool_nosql::store::DocStore;
                        Outcome::Ok(store.version().to_string())
                    }
                    Err(e) => Outcome::Failed(e.to_string()),
                }
            }))
        }
        "couchdb" => {
            let cfg = tool_nosql::profile::NoSqlFields::from_table("couchdb", fields)
                .resolve_couch(read_only, secrets, &lookup)
                .map_err(nosql_error)?;
            Ok(Box::pin(async move {
                match tool_nosql::couch::CouchStore::connect(&cfg).await {
                    Ok(store) => {
                        use tool_nosql::store::DocStore;
                        Outcome::Ok(store.version().to_string())
                    }
                    Err(e) => Outcome::Failed(e.to_string()),
                }
            }))
        }
        "s3" => {
            let f = hs_core::objstore::profile::S3Fields::from_table(fields);
            let config = f.config(secrets).map_err(s3_error)?;
            let client = hs_core::objstore::s3::S3Client::new(config)
                .map_err(|e| Outcome::Failed(e.to_string()))?;
            let bucket = f.default_bucket.clone();
            Ok(Box::pin(async move {
                // A profile scoped to one bucket usually cannot list them
                // all, so ask about the bucket it names instead.
                if bucket.is_empty() {
                    match client.list_buckets().await {
                        Ok(b) => Outcome::Ok(b.len().to_string()),
                        Err(e) => Outcome::Failed(e.to_string()),
                    }
                } else {
                    match client.list(&bucket, "", true, None, 1).await {
                        Ok(_) => Outcome::Ok(bucket),
                        Err(e) => Outcome::Failed(e.to_string()),
                    }
                }
            }))
        }
        // The two HTTP object stores: the cheapest thing either can be
        // asked is what containers it has.
        "webdav" => {
            let f = hs_core::objstore::http_profile::WebdavFields::from_table(fields);
            let config = f.config(secrets).map_err(s3_error)?;
            let client = hs_core::objstore::webdav::WebdavClient::new(config)
                .map_err(|e| Outcome::Failed(e.to_string()))?;
            Ok(Box::pin(async move {
                use hs_core::objstore::store::ObjectStore;
                match client.containers().await {
                    Ok(b) => Outcome::Ok(b.len().to_string()),
                    Err(e) => Outcome::Failed(e.to_string()),
                }
            }))
        }
        "nexus" => {
            let f = hs_core::objstore::http_profile::NexusFields::from_table(fields);
            let config = f.config(secrets).map_err(s3_error)?;
            let client = hs_core::objstore::nexus::NexusClient::new(config)
                .map_err(|e| Outcome::Failed(e.to_string()))?;
            Ok(Box::pin(async move {
                use hs_core::objstore::store::ObjectStore;
                match client.containers().await {
                    Ok(b) => Outcome::Ok(b.len().to_string()),
                    Err(e) => Outcome::Failed(e.to_string()),
                }
            }))
        }
        "qdrant" => {
            let cfg = tool_nosql::profile::NoSqlFields::from_table("qdrant", fields)
                .resolve_qdrant(read_only, secrets, &lookup)
                .map_err(nosql_error)?;
            Ok(Box::pin(async move {
                match tool_nosql::qdrant::QdrantStore::connect(&cfg).await {
                    Ok(store) => {
                        use tool_nosql::store::DocStore;
                        Outcome::Ok(store.version().to_string())
                    }
                    Err(e) => Outcome::Failed(e.to_string()),
                }
            }))
        }
        // An engine answers over its own CLI, which is a blocking process
        // call, so it runs on the blocking pool; the button was disabled
        // for every container kind until 2026-10-29 (owner's report).
        "docker" | "podman" | "wincontainer" => {
            let f = hs_container::profile::ContainerFields::from_table(fields);
            if f.transport == hs_container::profile::Transport::Ssh {
                // Run on the far side through the SSH tool, which this
                // page cannot do; opening the host does.
                return Err(Outcome::Failed(s.t("conn-test-over-ssh")));
            }
            let engine = hs_container::Engine::new(&profile.kind, f);
            let handle = s.runtime.handle();
            Ok(Box::pin(async move {
                match handle
                    .spawn_blocking(move || hs_container::run::probe(&engine))
                    .await
                {
                    Ok(Ok(v)) => Outcome::Ok(v),
                    Ok(Err(e)) => Outcome::Failed(e.to_string()),
                    Err(e) => Outcome::Failed(e.to_string()),
                }
            }))
        }
        "k8s" => {
            let f = hs_container::profile::ContainerFields::from_table(fields);
            Ok(Box::pin(async move {
                match hs_container::k8s::connect(&f).await {
                    Ok(client) => match hs_container::k8s::version(&client).await {
                        Ok(v) => Outcome::Ok(v),
                        Err(e) => Outcome::Failed(e.to_string()),
                    },
                    Err(e) => Outcome::Failed(e.to_string()),
                }
            }))
        }
        "ssh" => {
            let f = hs_ssh::SshFields::from_table(fields);
            let cfg = hs_ssh::config::config_from_fields(&f, secrets).map_err(ssh_error)?;
            let known_hosts = Arc::new(hs_ssh::KnownHosts::load(&s.paths.known_hosts_file()));
            // The settings page is no place to ask about a host key, so the
            // prompt is captured and declined: an unknown key is reported
            // with its fingerprint, and the user accepts it by opening the
            // connection once.
            let seen: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
            let sink_seen = seen.clone();
            let prompt: hs_ssh::PromptSink = Arc::new(move |p: hs_ssh::HostKeyPrompt| {
                *sink_seen.lock() = Some(p.fingerprint.clone());
            });
            Ok(Box::pin(async move {
                match hs_ssh::SshClient::connect(cfg, known_hosts, Some(prompt)).await {
                    Ok(client) => {
                        let host = client.config.host.clone();
                        Outcome::Ok(host)
                    }
                    Err(e) => match seen.lock().clone() {
                        Some(fingerprint) => Outcome::UnknownHostKey(fingerprint),
                        None => Outcome::Failed(e.to_string()),
                    },
                }
            }))
        }
        _ => Err(Outcome::NotTestable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_kinds_with_a_driver_are_offered_a_test() {
        for k in [
            "ssh",
            "s3",
            "webdav",
            "nexus",
            "db",
            "redis",
            "valkey",
            "mongodb",
            "couchdb",
            "qdrant",
            "docker",
            "podman",
            "wincontainer",
            "k8s",
        ] {
            assert!(testable(k), "{k}");
        }
        for k in ["api", "browser", "", "sftp"] {
            assert!(!testable(k), "{k}");
        }
    }

    #[test]
    fn our_own_conditions_are_named_not_stringified() {
        // The page can only word these if they arrive as variants; a
        // `format!("{e:?}")` would put "VaultLocked" on screen.
        assert_eq!(
            db_error(tool_db::profile::ProfileError::VaultLocked),
            Outcome::VaultLocked
        );
        assert_eq!(
            redis_error(tool_redis::profile::ProfileError::MissingVariables(vec![
                "db_host".into(),
                "db_port".into()
            ])),
            Outcome::MissingVariables("db_host, db_port".into())
        );
        assert_eq!(
            s3_error(hs_core::objstore::profile::ProfileError::NoKeys),
            Outcome::NoKeys
        );
        assert_eq!(
            ssh_error(hs_ssh::config::ConfigError::NoPassword),
            Outcome::NoPassword
        );
        assert_eq!(
            nosql_error(tool_nosql::profile::ProfileError::UnknownKind("x".into())),
            Outcome::Failed("x is not a document store".into())
        );
    }

    #[test]
    fn a_test_starts_out_running() {
        let t = Test {
            profile_id: "x".into(),
            slot: Arc::new(Mutex::new(Outcome::Running)),
            abort: None,
        };
        assert!(t.running());
        *t.slot.lock() = Outcome::Ok(String::new());
        assert!(!t.running());
        assert_eq!(t.outcome(), Outcome::Ok(String::new()));
    }
}
