//! A tiny SSH + SFTP server for tests and the `devsshd` example (feature
//! `testserver`): password auth (`dev` / `dev`), an echoing fake shell that
//! understands `cd` and announces its directory through the window title,
//! and an in-memory SFTP file system. Deterministic host key, loopback only.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use russh::server::{Auth, Msg, Session};
use russh::{Channel, ChannelId};
use russh_sftp::protocol::{
    Attrs, Data, File, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode, Version,
};
use tokio::sync::Mutex;

pub const USER: &str = "dev";
pub const PASSWORD: &str = "dev";
pub const HOME: &str = "/home/dev";

#[derive(Clone)]
enum Node {
    Dir,
    File(Vec<u8>),
}

#[derive(Default)]
pub struct Fs {
    nodes: HashMap<String, Node>,
}

impl Fs {
    fn seeded() -> Self {
        let mut fs = Fs::default();
        for d in [
            "/",
            "/home",
            HOME,
            "/home/dev/projects",
            "/home/dev/projects/demo",
            "/home/dev/logs",
            "/etc",
        ] {
            fs.nodes.insert(d.to_string(), Node::Dir);
        }
        fs.nodes.insert(
            "/home/dev/projects/demo/readme.txt".into(),
            Node::File(b"hello from the test server\n".to_vec()),
        );
        fs.nodes.insert(
            "/home/dev/logs/app.log".into(),
            Node::File(b"line1\nline2\n".to_vec()),
        );
        fs.nodes
            .insert("/etc/hostname".into(), Node::File(b"testbox\n".to_vec()));
        fs
    }

    fn normalize(path: &str) -> String {
        let path = if path.is_empty() || path == "." || path == "~" {
            HOME.to_string()
        } else if let Some(rest) = path.strip_prefix("~/") {
            format!("{HOME}/{rest}")
        } else if !path.starts_with('/') {
            format!("{HOME}/{path}")
        } else {
            path.to_string()
        };
        let mut parts: Vec<&str> = Vec::new();
        for p in path.split('/') {
            match p {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                other => parts.push(other),
            }
        }
        if parts.is_empty() {
            "/".into()
        } else {
            format!("/{}", parts.join("/"))
        }
    }

    fn attrs(&self, path: &str) -> Option<FileAttributes> {
        let node = self.nodes.get(path)?;
        let mut a = FileAttributes::default();
        match node {
            Node::Dir => {
                a.permissions = Some(0o040_755);
                a.size = Some(4096);
            }
            Node::File(bytes) => {
                a.permissions = Some(0o100_644);
                a.size = Some(bytes.len() as u64);
            }
        }
        a.mtime = Some(1_760_000_000);
        a.uid = Some(1000);
        a.gid = Some(1000);
        Some(a)
    }

    fn children(&self, dir: &str) -> Vec<(String, FileAttributes)> {
        let prefix = if dir == "/" {
            "/".to_string()
        } else {
            format!("{dir}/")
        };
        let mut out: Vec<(String, FileAttributes)> = self
            .nodes
            .keys()
            .filter(|p| {
                p.starts_with(&prefix) && p.len() > prefix.len() && !p[prefix.len()..].contains('/')
            })
            .map(|p| (p[prefix.len()..].to_string(), self.attrs(p).unwrap()))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }
}

type SharedFs = Arc<Mutex<Fs>>;

/// The listening server; keep it alive for as long as clients should connect.
pub struct TestServer {
    pub port: u16,
    task: tokio::task::JoinHandle<()>,
}

impl TestServer {
    /// Bind 127.0.0.1 on an ephemeral port (or `port` when given) and serve.
    pub async fn start(port: Option<u16>) -> Self {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port.unwrap_or(0)))
            .await
            .expect("bind loopback");
        let port = listener.local_addr().unwrap().port();
        let config = Arc::new(russh::server::Config {
            inactivity_timeout: Some(std::time::Duration::from_secs(3600)),
            auth_rejection_time: std::time::Duration::from_secs(3),
            auth_rejection_time_initial: Some(std::time::Duration::ZERO),
            keys: vec![host_key()],
            ..Default::default()
        });
        let fs: SharedFs = Arc::new(Mutex::new(Fs::seeded()));
        let mut server = ServerFactory { fs };
        let task = tokio::spawn(async move {
            use russh::server::Server as _;
            let running = server.run_on_socket(config, &listener);
            let _ = running.await;
        });
        Self { port, task }
    }

    pub fn stop(self) {
        self.task.abort();
    }
}

/// Deterministic ed25519 host key (test only: the seed is public).
pub fn host_key() -> russh::keys::PrivateKey {
    use russh::keys::ssh_key::private::{Ed25519Keypair, KeypairData};
    let seed: [u8; 32] = *b"hyperstack-test-host-key-seed-01";
    let kp = Ed25519Keypair::from_seed(&seed);
    russh::keys::PrivateKey::new(KeypairData::Ed25519(kp), "hyperstack-testserver")
        .expect("ed25519 host key")
}

#[derive(Clone)]
struct ServerFactory {
    fs: SharedFs,
}

impl russh::server::Server for ServerFactory {
    type Handler = ClientSession;
    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Self::Handler {
        ClientSession {
            fs: self.fs.clone(),
            channels: HashMap::new(),
            sftp_channels: HashSet::new(),
            cwd: HOME.into(),
            line: String::new(),
        }
    }
}

struct ClientSession {
    fs: SharedFs,
    channels: HashMap<ChannelId, Channel<Msg>>,
    /// Channels handed to the SFTP subsystem or a `direct-tcpip` stream:
    /// russh still calls `data` for them, so the shell handler must ignore
    /// their bytes.
    sftp_channels: HashSet<ChannelId>,
    cwd: String,
    line: String,
}

impl ClientSession {
    fn prompt(&self) -> Vec<u8> {
        format!(
            "\x1b]0;{USER}@testbox: {}\x07{USER}@testbox:{}$ ",
            self.cwd, self.cwd
        )
        .into_bytes()
    }
}

impl russh::server::Handler for ClientSession {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        Ok(if user == USER && password == PASSWORD {
            Auth::Accept
        } else {
            Auth::reject()
        })
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.channels.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        _col_width: u32,
        _row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        Ok(())
    }

    async fn window_change_request(
        &mut self,
        channel: ChannelId,
        _col_width: u32,
        _row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        Ok(())
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        let mut banner = b"Welcome to the HyperStack test server.\r\n".to_vec();
        banner.extend(self.prompt());
        session.data(channel, banner)?;
        Ok(())
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if self.sftp_channels.contains(&channel) {
            return Ok(()); // SFTP data is handled by the subsystem stream.
        }
        let mut out = Vec::new();
        for &b in data {
            match b {
                b'\r' | b'\n' => {
                    out.extend_from_slice(b"\r\n");
                    let line = std::mem::take(&mut self.line);
                    let line = line.trim();
                    // A real shell swallows a comment line; this toy one has
                    // to as well, or the SFTP pane's echoed commands (which
                    // are comments on purpose) come back as errors.
                    if line.starts_with('#') {
                        out.extend(self.prompt());
                        continue;
                    }
                    if let Some(target) = line.strip_prefix("cd ") {
                        let target = target.trim().trim_matches('\'');
                        let p = Fs::normalize(if target.is_empty() { "~" } else { target });
                        let exists = matches!(self.fs.lock().await.nodes.get(&p), Some(Node::Dir));
                        if exists {
                            self.cwd = p;
                        } else {
                            out.extend_from_slice(
                                format!("bash: cd: {target}: No such file or directory\r\n")
                                    .as_bytes(),
                            );
                        }
                    } else if line == "pwd" {
                        out.extend_from_slice(format!("{}\r\n", self.cwd).as_bytes());
                    } else if line == "ls" {
                        let names: Vec<String> = self
                            .fs
                            .lock()
                            .await
                            .children(&self.cwd)
                            .into_iter()
                            .map(|(n, _)| n)
                            .collect();
                        out.extend_from_slice(format!("{}\r\n", names.join("  ")).as_bytes());
                    } else if let Some(rest) = line.strip_prefix("echo ") {
                        out.extend_from_slice(format!("{rest}\r\n").as_bytes());
                    } else if line == "exit" {
                        session.data(channel, out)?;
                        session.exit_status_request(channel, 0)?;
                        session.eof(channel)?;
                        session.close(channel)?;
                        return Ok(());
                    } else if !line.is_empty() {
                        out.extend_from_slice(
                            format!("bash: {line}: command not found\r\n").as_bytes(),
                        );
                    }
                    out.extend(self.prompt());
                }
                0x7f | 0x08 => {
                    if self.line.pop().is_some() {
                        out.extend_from_slice(b"\x08 \x08");
                    }
                }
                3 => {
                    self.line.clear();
                    out.extend_from_slice(b"^C\r\n");
                    out.extend(self.prompt());
                }
                _ => {
                    self.line.push(b as char);
                    out.push(b);
                }
            }
        }
        session.data(channel, out)?;
        Ok(())
    }

    async fn subsystem_request(
        &mut self,
        channel_id: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if name == "sftp"
            && let Some(channel) = self.channels.remove(&channel_id)
        {
            self.sftp_channels.insert(channel_id);
            session.channel_success(channel_id)?;
            let fs = self.fs.clone();
            tokio::spawn(async move {
                russh_sftp::server::run(
                    channel.into_stream(),
                    SftpHandler {
                        fs,
                        done_dirs: HashSet::new(),
                    },
                )
                .await;
            });
        } else {
            session.channel_failure(channel_id)?;
        }
        Ok(())
    }

    /// `ssh -L` support for the tunnel tests: loopback targets only, so the
    /// test server is never an open proxy.
    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host_to_connect: &str,
        port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if host_to_connect != "127.0.0.1" && host_to_connect != "localhost" {
            reply
                .reject(russh::ChannelOpenFailure::AdministrativelyProhibited)
                .await;
            return Ok(());
        }
        // Streamed like SFTP: `data` must ignore it.
        self.sftp_channels.insert(channel.id());
        reply.accept().await;
        let target = format!("127.0.0.1:{port_to_connect}");
        tokio::spawn(async move {
            if let Ok(mut tcp) = tokio::net::TcpStream::connect(target).await {
                let mut stream = channel.into_stream();
                let _ = tokio::io::copy_bidirectional(&mut tcp, &mut stream).await;
            }
        });
        Ok(())
    }

    async fn channel_eof(
        &mut self,
        _channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

struct SftpHandler {
    fs: SharedFs,
    done_dirs: HashSet<String>,
}

fn ok(id: u32) -> Status {
    Status {
        id,
        status_code: StatusCode::Ok,
        error_message: "Ok".into(),
        language_tag: "en-US".into(),
    }
}

impl russh_sftp::server::Handler for SftpHandler {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn init(
        &mut self,
        _version: u32,
        _ext: HashMap<String, String>,
    ) -> Result<Version, Self::Error> {
        Ok(Version::new())
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
        let p = Fs::normalize(&path);
        let attrs = self
            .fs
            .lock()
            .await
            .attrs(&p)
            .ok_or(StatusCode::NoSuchFile)?;
        Ok(Name {
            id,
            files: vec![File::new(p, attrs)],
        })
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        let p = Fs::normalize(&path);
        let attrs = self
            .fs
            .lock()
            .await
            .attrs(&p)
            .ok_or(StatusCode::NoSuchFile)?;
        Ok(Attrs { id, attrs })
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        self.stat(id, path).await
    }

    async fn fstat(&mut self, id: u32, handle: String) -> Result<Attrs, Self::Error> {
        let p = handle
            .trim_start_matches("f:")
            .trim_start_matches("d:")
            .to_string();
        let attrs = self
            .fs
            .lock()
            .await
            .attrs(&p)
            .ok_or(StatusCode::NoSuchFile)?;
        Ok(Attrs { id, attrs })
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, Self::Error> {
        let p = Fs::normalize(&path);
        match self.fs.lock().await.nodes.get(&p) {
            Some(Node::Dir) => {}
            _ => return Err(StatusCode::NoSuchFile),
        }
        let handle = format!("d:{p}");
        self.done_dirs.remove(&handle);
        Ok(Handle { id, handle })
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, Self::Error> {
        if self.done_dirs.contains(&handle) {
            return Err(StatusCode::Eof);
        }
        self.done_dirs.insert(handle.clone());
        let dir = handle.trim_start_matches("d:");
        let files: Vec<File> = self
            .fs
            .lock()
            .await
            .children(dir)
            .into_iter()
            .map(|(name, attrs)| File::new(name, attrs))
            .collect();
        Ok(Name { id, files })
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<Status, Self::Error> {
        self.done_dirs.remove(&handle);
        Ok(ok(id))
    }

    async fn open(
        &mut self,
        id: u32,
        filename: String,
        pflags: OpenFlags,
        _attrs: FileAttributes,
    ) -> Result<Handle, Self::Error> {
        let p = Fs::normalize(&filename);
        let mut fs = self.fs.lock().await;
        let exists = matches!(fs.nodes.get(&p), Some(Node::File(_)));
        if pflags.contains(OpenFlags::CREATE) || pflags.contains(OpenFlags::WRITE) {
            if !exists || pflags.contains(OpenFlags::TRUNCATE) {
                fs.nodes.insert(p.clone(), Node::File(Vec::new()));
            }
        } else if !exists {
            return Err(StatusCode::NoSuchFile);
        }
        Ok(Handle {
            id,
            handle: format!("f:{p}"),
        })
    }

    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, Self::Error> {
        let p = handle.trim_start_matches("f:");
        let fs = self.fs.lock().await;
        let Some(Node::File(bytes)) = fs.nodes.get(p) else {
            return Err(StatusCode::NoSuchFile);
        };
        let start = offset as usize;
        if start >= bytes.len() {
            return Err(StatusCode::Eof);
        }
        let end = (start + len as usize).min(bytes.len());
        Ok(Data {
            id,
            data: bytes[start..end].to_vec(),
        })
    }

    async fn write(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<Status, Self::Error> {
        let p = handle.trim_start_matches("f:");
        let mut fs = self.fs.lock().await;
        let Some(Node::File(bytes)) = fs.nodes.get_mut(p) else {
            return Err(StatusCode::NoSuchFile);
        };
        let start = offset as usize;
        if bytes.len() < start + data.len() {
            bytes.resize(start + data.len(), 0);
        }
        bytes[start..start + data.len()].copy_from_slice(&data);
        Ok(ok(id))
    }

    async fn mkdir(
        &mut self,
        id: u32,
        path: String,
        _attrs: FileAttributes,
    ) -> Result<Status, Self::Error> {
        let p = Fs::normalize(&path);
        let mut fs = self.fs.lock().await;
        if fs.nodes.contains_key(&p) {
            return Err(StatusCode::Failure);
        }
        fs.nodes.insert(p, Node::Dir);
        Ok(ok(id))
    }

    async fn remove(&mut self, id: u32, filename: String) -> Result<Status, Self::Error> {
        let p = Fs::normalize(&filename);
        let mut fs = self.fs.lock().await;
        match fs.nodes.get(&p) {
            Some(Node::File(_)) => {
                fs.nodes.remove(&p);
                Ok(ok(id))
            }
            _ => Err(StatusCode::NoSuchFile),
        }
    }

    async fn rmdir(&mut self, id: u32, path: String) -> Result<Status, Self::Error> {
        let p = Fs::normalize(&path);
        let mut fs = self.fs.lock().await;
        if !matches!(fs.nodes.get(&p), Some(Node::Dir)) {
            return Err(StatusCode::NoSuchFile);
        }
        if !fs.children(&p).is_empty() {
            return Err(StatusCode::Failure);
        }
        fs.nodes.remove(&p);
        Ok(ok(id))
    }

    async fn rename(
        &mut self,
        id: u32,
        oldpath: String,
        newpath: String,
    ) -> Result<Status, Self::Error> {
        let (o, n) = (Fs::normalize(&oldpath), Fs::normalize(&newpath));
        let mut fs = self.fs.lock().await;
        let keys: Vec<String> = fs
            .nodes
            .keys()
            .filter(|k| **k == o || k.starts_with(&format!("{o}/")))
            .cloned()
            .collect();
        if keys.is_empty() {
            return Err(StatusCode::NoSuchFile);
        }
        for k in keys {
            let node = fs.nodes.remove(&k).unwrap();
            let nk = format!("{n}{}", &k[o.len()..]);
            fs.nodes.insert(nk, node);
        }
        Ok(ok(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fs_normalizes_and_lists() {
        assert_eq!(Fs::normalize("."), HOME);
        assert_eq!(Fs::normalize("~/projects"), "/home/dev/projects");
        assert_eq!(Fs::normalize("/a/b/../c"), "/a/c");
        assert_eq!(Fs::normalize("projects/demo/.."), "/home/dev/projects");
        let fs = Fs::seeded();
        let names: Vec<String> = fs.children(HOME).into_iter().map(|(n, _)| n).collect();
        assert!(names.contains(&"projects".to_string()));
        assert!(names.contains(&"logs".to_string()));
        assert!(
            fs.attrs("/home/dev/projects/demo/readme.txt")
                .unwrap()
                .size
                .unwrap()
                > 0
        );
        assert!(fs.attrs("/nope").is_none());
    }

    #[test]
    fn host_key_is_deterministic() {
        let a = host_key().public_key().to_openssh().unwrap();
        let b = host_key().public_key().to_openssh().unwrap();
        assert_eq!(a, b);
    }
}
