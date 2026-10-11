//! The tunnel manager end to end: a profile and a vault secret, the
//! in-process SSH server, a TCP echo service behind it, and bytes through
//! the forward.

use std::sync::Arc;
use std::time::Duration;

use hs_core::connections::{ConnectionProfile, Connections};
use hs_core::runtime::{AppRuntime, NoopWaker};
use hs_core::secrets::{KdfParams, SecretStore, VaultStore};
use hs_core::tunnels::{ForwardSpec, TunnelError, TunnelState, Tunnels};
use hs_ssh::testserver::{self, TestServer};
use hs_ssh::tunnels::TunnelManager;
use hs_ssh::{KnownHosts, SshFields};
use parking_lot::RwLock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn ssh_profile(port: u16, password_ref: &str) -> ConnectionProfile {
    let mut p = ConnectionProfile {
        name: "test box".into(),
        kind: "ssh".into(),
        ..Default::default()
    };
    SshFields {
        host: "127.0.0.1".into(),
        port,
        user: testserver::USER.into(),
        password: password_ref.into(),
        connect_timeout_secs: 5,
        ..Default::default()
    }
    .write_into(&mut p.fields);
    p
}

async fn echo_server() -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let (mut r, mut w) = s.split();
                let _ = tokio::io::copy(&mut r, &mut w).await;
            });
        }
    });
    port
}

async fn round_trip(port: u16, msg: &[u8]) -> Vec<u8> {
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    s.write_all(msg).await.unwrap();
    let mut got = vec![0u8; msg.len()];
    tokio::time::timeout(Duration::from_secs(10), s.read_exact(&mut got))
        .await
        .expect("echo in time")
        .unwrap();
    got
}

#[test]
fn local_forward_carries_bytes_and_is_shared() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = AppRuntime::new(Arc::new(NoopWaker)).unwrap();
    let rt = runtime.handle();
    rt.block_on(async {
        let server = TestServer::start(None).await;
        let echo = echo_server().await;

        // Vault with the password; the profile only holds the reference.
        let vault = Arc::new(VaultStore::new(
            &dir.path().join("vault.hsv"),
            KdfParams::FAST,
        ));
        vault
            .initialize(&secrecy::SecretString::from("master".to_string()))
            .unwrap();
        let r = vault
            .create(
                "ssh",
                &secrecy::SecretString::from(testserver::PASSWORD.to_string()),
            )
            .unwrap();
        let profile = ssh_profile(server.port, &r.to_string_ref());
        let ssh_id = profile.id.clone();
        let mut conns = Connections::load(&dir.path().join("connections"));
        conns.upsert(profile).unwrap();
        let conns = Arc::new(RwLock::new(conns));

        // An untrusted host key fails a background tunnel…
        let known_hosts = dir.path().join("known_hosts");
        let tunnels =
            TunnelManager::new(runtime.clone(), conns, vault.clone(), known_hosts.clone());
        let err = tunnels
            .ensure_local_forward(&ssh_id, "127.0.0.1", echo)
            .await
            .unwrap();
        assert_eq!(err, Err(TunnelError::UnknownHostKey));

        // …and works once the key is trusted (as the SSH tool's dialog does).
        let key = testserver::host_key();
        let b64 = key.public_key().to_openssh().unwrap();
        let mut parts = b64.split_whitespace();
        let (alg, data) = (parts.next().unwrap(), parts.next().unwrap());
        KnownHosts::load(&known_hosts).trust("127.0.0.1", server.port, alg, data);

        let port = tunnels
            .ensure_local_forward(&ssh_id, "127.0.0.1", echo)
            .await
            .unwrap()
            .expect("forward starts");
        assert_ne!(port, 0);
        assert_eq!(
            round_trip(port, b"hello through ssh").await,
            b"hello through ssh"
        );

        // Asking again reuses the same forward.
        let again = tunnels
            .ensure_local_forward(&ssh_id, "127.0.0.1", echo)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(again, port);
        let list = tunnels.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].via, "test box");
        assert_eq!(list[0].state, TunnelState::Listening { local_port: port });
        assert!(list[0].total >= 1);
        // Counters settle once the echo connection is closed.
        tokio::time::sleep(Duration::from_millis(200)).await;
        let info = &tunnels.list()[0];
        assert!(info.bytes_up >= 17 && info.bytes_down >= 17, "{info:?}");

        // An explicit second forward, then stop: the port closes.
        let id = tunnels.open(ForwardSpec {
            ssh_id: ssh_id.clone(),
            remote_host: "127.0.0.1".into(),
            remote_port: echo,
            local_port: 0,
        });
        let mut second = None;
        for _ in 0..100 {
            if let Some(TunnelState::Listening { local_port }) = tunnels
                .list()
                .iter()
                .find(|t| t.id == id)
                .map(|t| t.state.clone())
            {
                second = Some(local_port);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let second = second.expect("second forward listening");
        assert_eq!(round_trip(second, b"x").await, b"x");
        tunnels.stop(id);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            tokio::net::TcpStream::connect(("127.0.0.1", second))
                .await
                .is_err()
        );
        assert_eq!(tunnels.list().len(), 1);

        // A target the server refuses: the forward listens, the connection drops.
        server.stop();
    });
}
