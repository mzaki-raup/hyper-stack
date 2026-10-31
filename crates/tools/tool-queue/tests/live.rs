//! Driver tests against real brokers, each skipped unless its variable
//! names one:
//!
//! - `HS_RABBITMQ=127.0.0.1:15672:user:password` (the management API)
//! - `HS_KAFKA=127.0.0.1:9092`
//! - `HS_REDIS_STREAMS=127.0.0.1:6379`
//! - `HS_NATS=127.0.0.1:4222` (JetStream on: `nats-server -js`)
//! - `HS_MQTT=127.0.0.1:1883`
//!
//! They write only to destinations named `hstest…`, made per run, and
//! remove them afterwards where the broker allows.

use std::time::Duration;

use tool_queue::driver::{self, Destination, Driver, PeekRequest, Progress, PublishRequest};
use tool_queue::profile::{Auth, ConnectConfig};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn config(kind: &str, address: String, auth: Auth) -> ConnectConfig {
    ConnectConfig {
        kind: kind.into(),
        address,
        tls: false,
        auth,
        vhost: String::new(),
        sasl: "plain".into(),
        db: 0,
        client_id: String::new(),
        protocol: "3.1.1".into(),
        read_only: false,
        timeout: Duration::from_secs(10),
        mqtt_window: Duration::from_secs(2),
    }
}

/// A name no earlier run used.
fn unique(prefix: &str) -> String {
    format!("{prefix}{}", chrono::Utc::now().timestamp_millis())
}

/// Wait for a destination to show `depth`, reading the listing again:
/// some brokers (RabbitMQ's statistics) report a moment late.
async fn wait_for_depth(d: &mut dyn Driver, name: &str, depth: u64) -> Destination {
    let mut last = None;
    for _ in 0..30 {
        let l = d.destinations().await.expect("list");
        if let Some(found) = l.destinations.into_iter().find(|x| x.name == name) {
            if found.depth == Some(depth) {
                return found;
            }
            last = Some(found);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    panic!("{name} never showed depth {depth}; last seen {last:?}");
}

fn publish(target: &str, key: &str, body: &str) -> PublishRequest {
    PublishRequest {
        target: target.into(),
        key: key.into(),
        headers: vec![("hs-test".into(), "yes".into())],
        payload: body.as_bytes().to_vec(),
        ..Default::default()
    }
}

fn peek(count: usize) -> PeekRequest {
    PeekRequest {
        count,
        ..Default::default()
    }
}

/// The checks every kind shares: publish three, see three, peek three
/// without taking them, purge to none. Returns the destination.
async fn round_trip(d: &mut dyn Driver, name: &str, target: &str, key: &str) -> Destination {
    for i in 0..3 {
        d.publish(&publish(target, key, &format!("{{\"n\":{i}}}")))
            .await
            .expect("publish");
    }
    let dest = wait_for_depth(d, name, 3).await;
    let got = d.peek(&dest, &peek(10)).await.expect("peek");
    assert_eq!(got.len(), 3, "peek should see all three");
    assert!(
        got.iter().any(|m| m.payload == b"{\"n\":0}"),
        "the first message should be among them: {got:?}"
    );
    // Peek took nothing.
    wait_for_depth(d, name, 3).await;
    let purged = d.purge(&dest).await.expect("purge");
    assert!(!purged.is_empty());
    wait_for_depth(d, name, 0).await
}

#[test]
fn rabbitmq_lists_peeks_publishes_and_purges_a_queue() {
    let Ok(spec) = std::env::var("HS_RABBITMQ") else {
        eprintln!("HS_RABBITMQ not set; skipping");
        return;
    };
    let parts: Vec<&str> = spec.splitn(4, ':').collect();
    let [host, port, user, password] = parts[..] else {
        panic!("HS_RABBITMQ is host:port:user:password");
    };
    let base = format!("http://{host}:{port}");
    let auth = Auth::Password {
        user: user.into(),
        password: password.into(),
    };
    runtime().block_on(async {
        let queue = unique("hstest.q");
        // Make the queue through the same API, as a broker admin would.
        // Durable: RabbitMQ 4 refuses new transient queues.
        reqwest::Client::new()
            .put(format!("{base}/api/queues/%2F/{queue}"))
            .basic_auth(user, Some(password))
            .json(&serde_json::json!({"durable": true}))
            .send()
            .await
            .expect("create queue")
            .error_for_status()
            .expect("create queue");
        let mut d = driver::connect(&config("rabbitmq", base.clone(), auth.clone()))
            .await
            .expect("connect");
        assert_eq!(d.info().product, "RabbitMQ");
        // The default exchange, routed by the queue's name.
        let dest = round_trip(d.as_mut(), &queue, "", &queue).await;
        let bindings = d.bindings(&dest).await.expect("bindings");
        assert!(bindings.iter().any(|b| b.routing_key == queue));
        d.delete(&dest).await.expect("delete");
        let l = d.destinations().await.expect("list");
        assert!(!l.destinations.iter().any(|x| x.name == queue));

        // Wrong credentials are named as such.
        let bad = Auth::Password {
            user: user.into(),
            password: "wrong".into(),
        };
        let e = driver::connect(&config("rabbitmq", base, bad)).await.err();
        assert!(e.expect("refused").contains("401"));
    });
}

#[test]
fn kafka_lists_peeks_publishes_and_purges_a_topic() {
    let Ok(addr) = std::env::var("HS_KAFKA") else {
        eprintln!("HS_KAFKA not set; skipping");
        return;
    };
    runtime().block_on(async {
        let topic = unique("hstest-t");
        let admin = rskafka::client::ClientBuilder::new(vec![addr.clone()])
            .build()
            .await
            .expect("admin");
        admin
            .controller_client()
            .expect("controller")
            .create_topic(&topic, 2, 1, 5_000)
            .await
            .expect("create topic");
        let mut d = driver::connect(&config("kafka", addr.clone(), Auth::None))
            .await
            .expect("connect");
        // Keyed records land by key; all three on one partition.
        let dest = round_trip(d.as_mut(), &topic, &topic, "same-key").await;
        assert_eq!(dest.figures[0], "2", "two partitions");
        let groups = d
            .consumers(&dest, &Progress::default())
            .await
            .expect("groups");
        assert!(groups.is_empty(), "nobody consumes a new topic");
        // A partition that does not exist is refused, not guessed.
        let mut bad = publish(&topic, "", "x");
        bad.partition = Some(9);
        assert!(d.publish(&bad).await.is_err());
        admin
            .controller_client()
            .expect("controller")
            .delete_topic(&topic, 5_000)
            .await
            .expect("delete topic");
    });
}

#[test]
fn redis_streams_lists_peeks_publishes_and_purges_a_stream() {
    let Ok(addr) = std::env::var("HS_REDIS_STREAMS") else {
        eprintln!("HS_REDIS_STREAMS not set; skipping");
        return;
    };
    runtime().block_on(async {
        let key = unique("hstest:s:");
        let mut d = driver::connect(&config("redis-streams", addr, Auth::None))
            .await
            .expect("connect");
        let dest = round_trip(d.as_mut(), &key, &key, "").await;
        let groups = d
            .consumers(&dest, &Progress::default())
            .await
            .expect("groups");
        assert!(groups.is_empty());
        d.delete(&dest).await.expect("delete");
    });
}

#[test]
fn nats_lists_peeks_publishes_and_purges_a_stream() {
    let Ok(addr) = std::env::var("HS_NATS") else {
        eprintln!("HS_NATS not set; skipping");
        return;
    };
    runtime().block_on(async {
        let stream = unique("HSTEST");
        let subject = format!("{}.created", stream.to_lowercase());
        let client = async_nats::connect(addr.as_str()).await.expect("nats");
        let js = async_nats::jetstream::new(client);
        js.create_stream(async_nats::jetstream::stream::Config {
            name: stream.clone(),
            subjects: vec![subject.clone()],
            ..Default::default()
        })
        .await
        .expect("create stream");
        let mut d = driver::connect(&config("nats", addr, Auth::None))
            .await
            .expect("connect");
        assert!(!d.info().version.is_empty());
        round_trip(d.as_mut(), &stream, &subject, "").await;
        // A subject no stream captures is still published, as core NATS.
        let said = d
            .publish(&publish("hstest.nowhere", "", "x"))
            .await
            .expect("core publish");
        assert!(said.contains("no stream"));
        js.delete_stream(&stream).await.expect("delete stream");
    });
}

#[test]
fn mqtt_sees_retained_topics_and_clears_them() {
    let Ok(addr) = std::env::var("HS_MQTT") else {
        eprintln!("HS_MQTT not set; skipping");
        return;
    };
    runtime().block_on(async {
        let topic = unique("hstest/m/");
        for protocol in ["3.1.1", "5"] {
            let mut cfg = config("mqtt", addr.clone(), Auth::None);
            cfg.protocol = protocol.into();
            let mut d = driver::connect(&cfg).await.expect("connect");
            let mut retained = publish(&topic, "", "{\"on\":true}");
            retained.retain = true;
            retained.qos = 1;
            d.publish(&retained).await.expect("publish");
            let l = d.destinations().await.expect("look");
            let dest = l
                .destinations
                .iter()
                .find(|x| x.name == topic)
                .unwrap_or_else(|| panic!("{protocol}: the retained topic should be seen"))
                .clone();
            assert_eq!(dest.figures[1], "yes", "retained");
            let got = d.peek(&dest, &peek(5)).await.expect("peek");
            assert_eq!(got[0].payload, b"{\"on\":true}");
            d.purge(&dest).await.expect("clear");
            let l = d.destinations().await.expect("look again");
            assert!(
                !l.destinations.iter().any(|x| x.name == topic),
                "{protocol}: a cleared topic keeps nothing to see"
            );
        }
    });
}

#[test]
fn a_read_only_session_refuses_writes_on_a_real_broker() {
    let Ok(addr) = std::env::var("HS_REDIS_STREAMS") else {
        eprintln!("HS_REDIS_STREAMS not set; skipping");
        return;
    };
    runtime().block_on(async {
        let key = unique("hstest:ro:");
        let mut cfg = config("redis-streams", addr, Auth::None);
        cfg.read_only = true;
        let mut d = driver::connect(&cfg).await.expect("connect");
        let e = d.publish(&publish(&key, "", "x")).await.unwrap_err();
        assert!(e.contains("read-only"), "{e}");
        let l = d.destinations().await.expect("list");
        assert!(!l.destinations.iter().any(|x| x.name == key));
    });
}
