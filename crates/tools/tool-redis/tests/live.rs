//! Driver tests against a real server, skipped unless `HS_REDIS` names one
//! (`HS_REDIS=127.0.0.1:6379`, optionally `host:port:password`). They write
//! only to keys under `hstest:` and delete them afterwards.
//!
//! `redis-server --port 6379 --save '' --appendonly no` is enough.

use std::time::Duration;

use tool_redis::client::{ConnectConfig, KeyType, Session, ValuePage};

fn config() -> Option<ConnectConfig> {
    let spec = std::env::var("HS_REDIS").ok()?;
    let mut parts = spec.split(':');
    let host = parts.next()?.to_string();
    let port = parts.next().and_then(|p| p.parse().ok()).unwrap_or(6379);
    let password = parts.next().map(str::to_string);
    Some(ConnectConfig {
        host,
        port,
        password,
        connect_timeout: Duration::from_secs(5),
        ..Default::default()
    })
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

#[test]
fn every_type_round_trips_through_the_session() {
    let Some(cfg) = config() else {
        eprintln!("HS_REDIS not set; skipping");
        return;
    };
    runtime().block_on(async {
        let mut s = Session::connect(&cfg).await.expect("connect");
        assert!(!s.server_info().version.is_empty(), "INFO should answer");
        let keys: Vec<String> = [
            "hstest:types:str",
            "hstest:types:hash",
            "hstest:types:list",
            "hstest:types:set",
            "hstest:types:zset",
        ]
        .iter()
        .map(|k| k.to_string())
        .collect();
        s.delete(&keys).await.expect("clean");

        s.set_string("hstest:types:str", b"{\"a\":1}", false)
            .await
            .unwrap();
        s.hset("hstest:types:hash", "field", b"value")
            .await
            .unwrap();
        s.rpush("hstest:types:list", b"one").await.unwrap();
        s.rpush("hstest:types:list", b"two").await.unwrap();
        s.sadd("hstest:types:set", b"member").await.unwrap();
        s.zadd("hstest:types:zset", b"ada", 10.0).await.unwrap();

        let info = s.key_info("hstest:types:str").await.unwrap();
        assert_eq!(info.kind, Some(KeyType::String));
        assert_eq!(info.ttl_ms, None, "a new key has no TTL");
        assert_eq!(info.len, Some(7));

        match s
            .read("hstest:types:str", &KeyType::String, 0, 0, false)
            .await
            .unwrap()
        {
            ValuePage::Text { bytes, total } => {
                assert_eq!(bytes, b"{\"a\":1}");
                assert_eq!(total, 7);
            }
            other => panic!("wrong page: {other:?}"),
        }
        match s
            .read("hstest:types:hash", &KeyType::Hash, 0, 0, false)
            .await
            .unwrap()
        {
            ValuePage::Pairs { items, .. } => {
                assert_eq!(items[0].0, "field");
                assert_eq!(items[0].1, b"value");
            }
            other => panic!("wrong page: {other:?}"),
        }
        match s
            .read("hstest:types:list", &KeyType::List, 0, 0, false)
            .await
            .unwrap()
        {
            ValuePage::Items {
                items,
                cursor,
                start,
            } => {
                assert_eq!(items.len(), 2);
                assert_eq!((cursor, start), (0, 0), "a short page ends the list");
            }
            other => panic!("wrong page: {other:?}"),
        }
        match s
            .read("hstest:types:zset", &KeyType::ZSet, 0, 0, false)
            .await
            .unwrap()
        {
            ValuePage::Scored { items, .. } => assert_eq!(items[0].1, 10.0),
            other => panic!("wrong page: {other:?}"),
        }

        // TTL, rename and delete.
        s.set_ttl("hstest:types:str", Some(60_000)).await.unwrap();
        let info = s.key_info("hstest:types:str").await.unwrap();
        assert!(info.ttl_ms.is_some_and(|t| t > 0 && t <= 60_000));
        s.set_ttl("hstest:types:str", None).await.unwrap();
        assert_eq!(s.key_info("hstest:types:str").await.unwrap().ttl_ms, None);

        let (found, capped) = s.count_prefix("hstest:types:*", 100).await.unwrap();
        assert_eq!(found.len(), 5, "the scan should find what was written");
        assert!(!capped);
        assert_eq!(s.delete(&keys).await.unwrap(), 5);
        assert!(!s.exists("hstest:types:str").await);
    });
}

#[test]
fn a_scan_walks_the_keyspace_in_pages() {
    let Some(cfg) = config() else {
        eprintln!("HS_REDIS not set; skipping");
        return;
    };
    runtime().block_on(async {
        let mut s = Session::connect(&cfg).await.expect("connect");
        let keys: Vec<String> = (0..50).map(|i| format!("hstest:scan:{i}")).collect();
        for k in &keys {
            s.set_string(k, b"x", false).await.unwrap();
        }
        let mut cursor = 0;
        let mut seen = 0;
        loop {
            let page = s.scan(cursor, "hstest:scan:*", 10).await.unwrap();
            seen += page.keys.len();
            cursor = page.cursor;
            if cursor == 0 {
                break;
            }
        }
        assert_eq!(seen, 50);
        // Types come back in the order asked, in one pipeline.
        let types = s.types(&keys[..3]).await.unwrap();
        assert_eq!(types, vec![KeyType::String; 3]);
        s.delete(&keys).await.unwrap();
    });
}

#[test]
fn the_depth_helpers_answer_against_a_server() {
    let Some(cfg) = config() else {
        eprintln!("HS_REDIS not set; skipping");
        return;
    };
    runtime().block_on(async {
        let mut s = Session::connect(&cfg).await.expect("connect");
        let keys: Vec<String> = ["hstest:depth:a", "hstest:depth:b", "hstest:depth:stream"]
            .iter()
            .map(|k| k.to_string())
            .collect();
        s.delete(&keys).await.expect("clean");
        s.delete(&["hstest:moved:a".to_string(), "hstest:moved:b".to_string()])
            .await
            .expect("clean");

        // Slowlog and the client list answer, and we are in the list.
        let _ = s.slowlog(10).await.expect("slowlog");
        let clients = s.client_list().await.expect("client list");
        assert!(!clients.is_empty(), "this session is a client");
        assert!(clients.iter().all(|c| !c.id.is_empty()));

        // Memory per key, in one pipeline, with a missing key as None.
        s.set_string("hstest:depth:a", b"hello", false)
            .await
            .unwrap();
        s.set_string("hstest:depth:b", &vec![b'x'; 4096], false)
            .await
            .unwrap();
        let sizes = s
            .memory_usages(&[
                "hstest:depth:a".to_string(),
                "hstest:depth:b".to_string(),
                "hstest:depth:missing".to_string(),
            ])
            .await
            .expect("memory usages");
        assert_eq!(sizes.len(), 3);
        assert!(sizes[1].unwrap_or(0) > sizes[0].unwrap_or(0), "{sizes:?}");
        assert_eq!(sizes[2], None);

        // Expire, persist, rename by prefix.
        let two = vec!["hstest:depth:a".to_string(), "hstest:depth:b".to_string()];
        assert_eq!(s.expire_many(&two, 600).await.unwrap(), 2);
        assert!(s.key_info("hstest:depth:a").await.unwrap().ttl_ms.is_some());
        assert_eq!(s.persist_many(&two).await.unwrap(), 2);
        assert!(s.key_info("hstest:depth:a").await.unwrap().ttl_ms.is_none());
        let pairs = vec![
            ("hstest:depth:a".to_string(), "hstest:moved:a".to_string()),
            ("hstest:depth:b".to_string(), "hstest:moved:b".to_string()),
        ];
        assert_eq!(s.rename_many(&pairs).await.unwrap(), 2);
        assert_eq!(
            s.key_info("hstest:moved:a").await.unwrap().kind,
            Some(KeyType::String)
        );

        // A stream with a group.
        for i in 0..3 {
            s.command(&[
                "XADD".into(),
                "hstest:depth:stream".into(),
                "*".into(),
                "n".into(),
                i.to_string(),
            ])
            .await
            .unwrap();
        }
        s.command(&[
            "XGROUP".into(),
            "CREATE".into(),
            "hstest:depth:stream".into(),
            "g1".into(),
            "0".into(),
        ])
        .await
        .unwrap();
        let view = s
            .stream_view("hstest:depth:stream", 2)
            .await
            .expect("xinfo");
        assert_eq!(view.length, 3);
        assert!(!view.last_id.is_empty());
        assert_eq!(view.groups.len(), 1);
        assert_eq!(view.groups[0].name, "g1");
        assert_eq!(view.entries.len(), 2, "only the newest two");
        assert_eq!(view.entries[1].1[0], ("n".to_string(), "2".to_string()));

        // Pub/Sub: a second connection hears what the first publishes.
        let mut ps = s.pubsub().await.expect("pubsub connection");
        ps.subscribe("hstest:depth:chan").await.unwrap();
        let n = s.publish("hstest:depth:chan", "ping").await.unwrap();
        assert_eq!(n, 1, "one subscriber");
        let mut stream = ps.into_on_message();
        let msg = tokio::time::timeout(Duration::from_secs(5), async {
            use futures_util::StreamExt;
            stream.next().await
        })
        .await
        .expect("a message within 5 s")
        .expect("stream open");
        assert_eq!(msg.get_channel_name(), "hstest:depth:chan");
        assert_eq!(msg.get_payload::<String>().unwrap(), "ping");

        s.delete(&[
            "hstest:moved:a".to_string(),
            "hstest:moved:b".to_string(),
            "hstest:depth:stream".to_string(),
        ])
        .await
        .unwrap();
    });
}
