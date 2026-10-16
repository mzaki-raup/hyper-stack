//! Driver tests against real servers, skipped unless the environment names
//! them: `HS_MONGO=127.0.0.1:27017` and
//! `HS_COUCH=http://admin:password@127.0.0.1:5984`. Both write only inside a
//! database called `hstest` and clean up after themselves.
//!
//! `docker run -d -p 27017:27017 mongo:7` and
//! `docker run -d -p 5984:5984 -e COUCHDB_USER=admin -e COUCHDB_PASSWORD=… couchdb:3`
//! are enough.

use std::time::Duration;

use tool_nosql::couch::{self, CouchStore};
use tool_nosql::mongo::{self, MongoStore};
use tool_nosql::store::{ContainerRef, DocStore, Document, PutMode, Query, Scope};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn mongo_config() -> Option<mongo::ConnectConfig> {
    let spec = std::env::var("HS_MONGO").ok()?;
    let (host, port) = spec.split_once(':')?;
    Some(mongo::ConnectConfig {
        host: host.to_string(),
        port: port.parse().ok()?,
        auth_source: "admin".into(),
        connect_timeout: Duration::from_secs(5),
        ..Default::default()
    })
}

fn couch_config() -> Option<couch::ConnectConfig> {
    let url = std::env::var("HS_COUCH").ok()?;
    // http://user:password@host:port
    let (scheme, rest) = url.split_once("://")?;
    let (auth, hostport) = match rest.rsplit_once('@') {
        Some((a, h)) => (Some(a), h),
        None => (None, rest),
    };
    let (user, password) = match auth {
        Some(a) => match a.split_once(':') {
            Some((u, p)) => (u.to_string(), Some(p.to_string())),
            None => (a.to_string(), None),
        },
        None => (String::new(), None),
    };
    Some(couch::ConnectConfig {
        url: format!("{scheme}://{hostport}"),
        user,
        password,
        connect_timeout: Duration::from_secs(5),
        ..Default::default()
    })
}

#[test]
fn mongo_documents_round_trip_with_their_types() {
    let Some(cfg) = mongo_config() else {
        eprintln!("HS_MONGO not set; skipping");
        return;
    };
    runtime().block_on(async {
        let mut s = MongoStore::connect(&cfg).await.expect("connect");
        assert!(!s.version().is_empty(), "buildInfo should answer");
        let c = ContainerRef::new("hstest", "orders");
        // A clean slate, however the last run ended.
        let _ = s.delete_many(&c, r#"{"test": true}"#, false).await;

        let doc = Document::new(
            serde_json::json!({"_id": "order-1", "test": true, "total": 129.9, "status": "failed"}),
            "order-1",
            None,
        );
        let saved = s.put(&c, &doc, PutMode::Insert).await.expect("insert");
        assert_eq!(saved.id, "order-1");

        // The database and the collection now exist.
        let dbs = s.databases().await.unwrap();
        assert!(dbs.iter().any(|d| d == "hstest"), "{dbs:?}");
        let containers = s
            .containers(&Scope {
                database: "hstest".into(),
            })
            .await
            .unwrap();
        assert!(containers.iter().any(|x| x.name == "orders"));

        // Find with a filter, a projection and a sort.
        let q = Query {
            filter: r#"{"status": "failed"}"#.into(),
            projection: r#"{"total": 1}"#.into(),
            sort: r#"{"_id": 1}"#.into(),
            limit: 10,
            skip: 0,
        };
        let (docs, summary) = s.find(&c, &q).await.expect("find");
        assert_eq!(summary.returned, 1);
        assert!(!summary.more);
        assert_eq!(
            docs[0].json.pointer("/total").and_then(|v| v.as_f64()),
            Some(129.9)
        );
        // The projection left the other fields out.
        assert!(docs[0].json.get("status").is_none());

        assert_eq!(s.count(&c, r#"{"status": "failed"}"#).await.unwrap(), 1);
        assert_eq!(s.count(&c, "").await.unwrap(), 1);

        // Edit and save it back.
        let mut edited = docs[0].clone();
        edited.json = serde_json::json!({"_id": "order-1", "test": true, "status": "paid"});
        s.put(&c, &edited, PutMode::Replace { rev: None })
            .await
            .expect("replace");
        let (docs, _) = s
            .find(
                &c,
                &Query {
                    filter: r#"{"_id": "order-1"}"#.into(),
                    limit: 1,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            docs[0].json.pointer("/status").and_then(|v| v.as_str()),
            Some("paid")
        );

        // An aggregation and the indexes both answer.
        let out = s
            .aggregate(&c, r#"[{"$match": {"test": true}}]"#, 10)
            .await
            .expect("aggregate");
        assert_eq!(out.len(), 1);
        let indexes = s.indexes(&c).await.expect("indexes");
        assert!(indexes.iter().any(|i| i.name == "_id_"), "{indexes:?}");
        assert!(s.explain(&c, &q).await.is_ok());

        // A dry run counts without deleting; the real one deletes.
        let dry = s.delete_many(&c, r#"{"test": true}"#, true).await.unwrap();
        assert_eq!((dry.matched, dry.changed, dry.dry_run), (1, 0, true));
        let real = s.delete_many(&c, r#"{"test": true}"#, false).await.unwrap();
        assert_eq!(real.changed, 1);
        assert_eq!(s.count(&c, "").await.unwrap(), 0);
    });
}

#[test]
fn couch_documents_round_trip_with_their_revisions() {
    let Some(cfg) = couch_config() else {
        eprintln!("HS_COUCH not set; skipping");
        return;
    };
    runtime().block_on(async {
        let mut s = CouchStore::connect(&cfg).await.expect("connect");
        assert!(
            !s.version().is_empty(),
            "the welcome document names a version"
        );
        let c = ContainerRef::new("hstest", "hstest");
        // CouchDB needs the database created; PUT is idempotent enough here.
        let http = reqwest::Client::new();
        let _ = http
            .put(format!("{}/hstest", cfg.url))
            .basic_auth(&cfg.user, cfg.password.clone())
            .send()
            .await;

        let doc = Document::new(
            serde_json::json!({"_id": "order-1", "test": true, "status": "failed"}),
            "order-1",
            None,
        );
        let saved = s.put(&c, &doc, PutMode::Insert).await.expect("insert");
        assert_eq!(saved.id, "order-1");
        let rev = saved.rev.clone().expect("a saved document has a rev");

        let dbs = s.databases().await.unwrap();
        assert!(dbs.iter().any(|d| d == "hstest"), "{dbs:?}");
        let containers = s
            .containers(&Scope {
                database: "hstest".into(),
            })
            .await
            .unwrap();
        assert_eq!(containers[0].count, Some(1));

        // `_all_docs` with no selector, then a Mango selector.
        let (docs, summary) = s.find(&c, &Query::with_limit(10)).await.expect("all docs");
        assert_eq!(summary.returned, 1);
        assert_eq!(docs[0].id, "order-1");
        let (docs, _) = s
            .find(
                &c,
                &Query {
                    filter: r#"{"status": "failed"}"#.into(),
                    limit: 10,
                    ..Default::default()
                },
            )
            .await
            .expect("mango");
        assert_eq!(docs.len(), 1);

        // Saving without the current rev is a conflict, which is the point.
        let stale = Document::new(
            serde_json::json!({"_id": "order-1", "status": "paid"}),
            "order-1",
            Some("1-wrong".into()),
        );
        assert!(
            s.put(&c, &stale, PutMode::Replace { rev: None })
                .await
                .is_err()
        );

        let mut edited = saved.clone();
        edited.json = serde_json::json!({"_id": "order-1", "test": true, "status": "paid"});
        let saved = s
            .put(&c, &edited, PutMode::Replace { rev: Some(rev) })
            .await
            .expect("replace");
        assert_ne!(saved.rev, edited.rev, "a save moves the revision on");

        assert_eq!(s.count(&c, "").await.unwrap(), 1);
        s.delete(&c, &saved).await.expect("delete");
        assert_eq!(s.count(&c, "").await.unwrap(), 0);
        let _ = http
            .delete(format!("{}/hstest", cfg.url))
            .basic_auth(&cfg.user, cfg.password.clone())
            .send()
            .await;
    });
}
