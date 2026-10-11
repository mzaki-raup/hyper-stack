//! End-to-end against the in-process test server: host-key trust on first
//! connect, password auth, a shell channel driven through the terminal
//! transport, and SFTP listing/read/write/rename/delete.

use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use hs_ssh::testserver::{self, TestServer};
use hs_ssh::{Auth, KnownHosts, SshClient, SshConfig};
use hs_term::transport::Transport;

fn config(port: u16) -> SshConfig {
    SshConfig {
        host: "127.0.0.1".into(),
        port,
        user: testserver::USER.into(),
        auth: Auth::Password(secrecy::SecretString::from(
            testserver::PASSWORD.to_string(),
        )),
        keepalive_secs: 0,
        connect_timeout_secs: 5,
    }
}

/// Read from the blocking shell reader until `needle` shows up (or time runs out).
async fn read_until(
    mut reader: Box<dyn Read + Send>,
    needle: &'static str,
) -> (Box<dyn Read + Send>, String) {
    tokio::task::spawn_blocking(move || {
        let mut out = String::new();
        let start = Instant::now();
        let mut buf = [0u8; 4096];
        while !out.contains(needle) && start.elapsed() < Duration::from_secs(10) {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => out.push_str(&String::from_utf8_lossy(&buf[..n])),
                Err(_) => break,
            }
        }
        (reader, out)
    })
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shell_and_sftp_round_trip_with_host_key_trust() {
    let server = TestServer::start(None).await;
    let dir = tempfile::tempdir().unwrap();
    let known = Arc::new(KnownHosts::load(&dir.path().join("known_hosts")));
    let prompts = Arc::new(AtomicUsize::new(0));
    let p2 = prompts.clone();
    let k2 = known.clone();
    let sink: hs_ssh::PromptSink = Arc::new(move |prompt: hs_ssh::HostKeyPrompt| {
        p2.fetch_add(1, Ordering::SeqCst);
        assert!(prompt.changed.is_none());
        assert!(prompt.fingerprint.starts_with("SHA256:"));
        prompt.answer(true, &k2);
    });

    // First connect asks about the host key; the second one does not.
    let client = SshClient::connect(config(server.port), known.clone(), Some(sink.clone()))
        .await
        .expect("connect");
    assert_eq!(prompts.load(Ordering::SeqCst), 1);
    let again = SshClient::connect(config(server.port), known.clone(), Some(sink.clone()))
        .await
        .expect("second connect");
    assert_eq!(
        prompts.load(Ordering::SeqCst),
        1,
        "known key needs no prompt"
    );
    again.disconnect().await;

    // Wrong password is an auth error, not a hang.
    let mut bad = config(server.port);
    bad.auth = Auth::Password(secrecy::SecretString::from("nope".to_string()));
    let err = SshClient::connect(bad, known.clone(), Some(sink.clone()))
        .await
        .err()
        .expect("bad password fails");
    assert!(matches!(err, hs_ssh::SshError::Auth { .. }), "{err}");

    // Shell through the terminal transport.
    let (mut transport, reader) = client.open_shell(80, 24).await.expect("shell");
    let (reader, banner) = read_until(reader, "$ ").await;
    assert!(banner.contains("Welcome"), "{banner}");
    transport.write(b"cd projects/demo\r").unwrap();
    let (reader, out) = read_until(reader, "projects/demo$ ").await;
    assert!(
        out.contains("\x1b]0;dev@testbox: /home/dev/projects/demo\x07"),
        "title announces cwd: {out:?}"
    );
    assert_eq!(
        hs_ssh::cwd_from_title("dev@testbox: /home/dev/projects/demo").as_deref(),
        Some("/home/dev/projects/demo")
    );
    transport.write(b"echo ping\r").unwrap();
    let (_reader, out) = read_until(reader, "ping\r\n").await;
    assert!(out.contains("ping"));
    transport.resize(100, 40);

    // SFTP.
    let sftp = client.open_sftp().await.expect("sftp");
    let home = sftp.canonicalize(".").await.expect("canonicalize");
    assert_eq!(home, testserver::HOME);
    let names: Vec<String> = sftp
        .read_dir(testserver::HOME)
        .await
        .unwrap()
        .map(|e| e.file_name())
        .collect();
    assert!(
        names.contains(&"projects".to_string()) && names.contains(&"logs".to_string()),
        "{names:?}"
    );
    let text = sftp
        .read("/home/dev/projects/demo/readme.txt")
        .await
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&text),
        "hello from the test server
"
    );
    sftp.write("/home/dev/new.txt", b"written by the test")
        .await
        .unwrap();
    assert_eq!(
        sftp.read("/home/dev/new.txt").await.unwrap(),
        b"written by the test"
    );
    sftp.create_dir("/home/dev/made").await.unwrap();
    sftp.rename("/home/dev/new.txt", "/home/dev/made/moved.txt")
        .await
        .unwrap();
    assert!(sftp.read("/home/dev/made/moved.txt").await.is_ok());
    sftp.remove_file("/home/dev/made/moved.txt").await.unwrap();
    sftp.remove_dir("/home/dev/made").await.unwrap();
    assert!(sftp.read_dir("/home/dev/made").await.is_err());

    transport.close();
    client.disconnect().await;
    server.stop();
}
