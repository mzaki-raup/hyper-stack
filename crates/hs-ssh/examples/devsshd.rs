//! Run the in-process test SSH/SFTP server on 127.0.0.1:2222 for manual
//! testing of the SSH tool: `cargo run -p hs-ssh --features testserver
//! --example devsshd`. User `dev`, password `dev`, files under /home/dev.

#[tokio::main]
async fn main() {
    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(2222);
    let server = hs_ssh::testserver::TestServer::start(Some(port)).await;
    println!(
        "devsshd listening on 127.0.0.1:{} (user {} / password {}); kill the process to stop",
        server.port,
        hs_ssh::testserver::USER,
        hs_ssh::testserver::PASSWORD
    );
    // Runs until the process is killed.
    std::future::pending::<()>().await;
    server.stop();
}
