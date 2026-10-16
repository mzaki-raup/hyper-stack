//! SSH for HyperStack (docs/tools/ssh-sftp.md): one `russh` client per
//! connection, shell channels exposed as `hs_term` transports, an SFTP
//! session for the file pane, and host-key memory in the data folder.
//!
//! Everything here is async on the app's tokio runtime; the UI talks to it
//! through channels and shared state, never by blocking.

pub mod config;
pub mod known_hosts;
pub mod profile;
pub mod shell;
#[cfg(feature = "testserver")]
pub mod testserver;
pub mod tunnels;

use std::path::PathBuf;
use std::sync::Arc;

use russh::client::{self, Handle};
use russh::keys::HashAlg;
use secrecy::{ExposeSecret, SecretString};
use tokio::sync::oneshot;

pub use known_hosts::{KnownHosts, Verdict};
pub use profile::SshFields;
pub use russh_sftp::client::SftpSession;
/// The attribute block an SFTP `setstat` carries: what the permissions
/// dialog writes back.
pub use russh_sftp::protocol::FileAttributes;
pub use shell::{ShellReader, ShellTransport};

#[derive(Debug, thiserror::Error)]
pub enum SshError {
    #[error("{0}")]
    Ssh(#[from] russh::Error),
    #[error("authentication failed for {user}@{host} ({method})")]
    Auth {
        user: String,
        host: String,
        method: &'static str,
    },
    #[error("host key for {host} rejected")]
    HostKeyRejected { host: String },
    #[error("host key for {host} has CHANGED (stored {stored}); refusing to connect")]
    HostKeyChanged { host: String, stored: String },
    #[error("key file {0}: {1}")]
    Key(String, String),
    #[error("sftp: {0}")]
    Sftp(#[from] russh_sftp::client::error::Error),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, SshError>;

#[derive(Clone)]
pub enum Auth {
    Password(SecretString),
    KeyFile {
        path: PathBuf,
        passphrase: Option<SecretString>,
    },
    Agent,
}

#[derive(Clone)]
pub struct SshConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub auth: Auth,
    pub keepalive_secs: u64,
    pub connect_timeout_secs: u64,
}

/// A first-connect / changed-key question for the user. Answer with
/// [`HostKeyPrompt::answer`]; dropping it means "no".
pub struct HostKeyPrompt {
    pub host: String,
    pub port: u16,
    pub algorithm: String,
    pub fingerprint: String,
    pub changed: Option<String>,
    key_b64: String,
    reply: Option<oneshot::Sender<bool>>,
}

impl HostKeyPrompt {
    pub fn answer(mut self, trust: bool, known_hosts: &KnownHosts) {
        if trust {
            known_hosts.trust(&self.host, self.port, &self.algorithm, &self.key_b64);
        }
        if let Some(tx) = self.reply.take() {
            let _ = tx.send(trust);
        }
    }
}

/// Where the client sends host-key questions. The UI shows a dialog and
/// answers; `None` makes unknown keys fail.
pub type PromptSink = Arc<dyn Fn(HostKeyPrompt) + Send + Sync>;

struct ClientHandler {
    host: String,
    port: u16,
    known_hosts: Arc<KnownHosts>,
    prompt: Option<PromptSink>,
}

impl client::Handler for ClientHandler {
    type Error = SshError;

    async fn check_server_key(
        &mut self,
        server_key: &russh::keys::PublicKeyOrCertificate,
    ) -> std::result::Result<bool, Self::Error> {
        let server_public_key = server_key.public_key();
        let algorithm = server_public_key.algorithm().to_string();
        let key_b64 = server_public_key
            .to_openssh()
            .map_err(|e| SshError::Other(e.to_string()))?;
        // Keep only the base64 part ("algorithm base64 [comment]").
        let key_b64 = key_b64
            .split_whitespace()
            .nth(1)
            .unwrap_or_default()
            .to_string();
        let fingerprint = server_public_key.fingerprint(HashAlg::Sha256).to_string();
        let verdict = self
            .known_hosts
            .check(&self.host, self.port, &algorithm, &key_b64);
        match verdict {
            Verdict::Known => Ok(true),
            Verdict::Unknown | Verdict::Changed { .. } => {
                let changed = match &verdict {
                    Verdict::Changed { stored } => Some(stored.clone()),
                    _ => None,
                };
                let Some(prompt) = &self.prompt else {
                    return Err(match changed {
                        Some(stored) => SshError::HostKeyChanged {
                            host: self.host.clone(),
                            stored,
                        },
                        None => SshError::HostKeyRejected {
                            host: self.host.clone(),
                        },
                    });
                };
                let (tx, rx) = oneshot::channel();
                prompt(HostKeyPrompt {
                    host: self.host.clone(),
                    port: self.port,
                    algorithm,
                    fingerprint,
                    changed,
                    key_b64,
                    reply: Some(tx),
                });
                match rx.await {
                    Ok(true) => Ok(true),
                    _ => Err(SshError::HostKeyRejected {
                        host: self.host.clone(),
                    }),
                }
            }
        }
    }
}

/// A connected, authenticated SSH session.
pub struct SshClient {
    handle: Handle<ClientHandler>,
    pub config: SshConfig,
}

impl SshClient {
    pub async fn connect(
        config: SshConfig,
        known_hosts: Arc<KnownHosts>,
        prompt: Option<PromptSink>,
    ) -> Result<Self> {
        let russh_config = Arc::new(client::Config {
            keepalive_interval: (config.keepalive_secs > 0)
                .then(|| std::time::Duration::from_secs(config.keepalive_secs)),
            inactivity_timeout: None,
            ..Default::default()
        });
        let handler = ClientHandler {
            host: config.host.clone(),
            port: config.port,
            known_hosts,
            prompt,
        };
        // Only the TCP connect is bounded by the timeout: the handshake may
        // wait for the user's answer to a host-key question.
        let timeout = std::time::Duration::from_secs(config.connect_timeout_secs.max(1));
        let addr = (config.host.as_str(), config.port);
        let stream = match tokio::time::timeout(timeout, tokio::net::TcpStream::connect(addr)).await
        {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                return Err(SshError::Other(format!(
                    "cannot reach {}:{}: {e}",
                    config.host, config.port
                )));
            }
            Err(_) => {
                return Err(SshError::Other(format!(
                    "connection to {}:{} timed out after {}s",
                    config.host, config.port, config.connect_timeout_secs
                )));
            }
        };
        let _ = stream.set_nodelay(true);
        let mut handle = client::connect_stream(russh_config, stream, handler).await?;
        let user = config.user.clone();
        let ok = match &config.auth {
            Auth::Password(p) => handle
                .authenticate_password(&user, p.expose_secret())
                .await?
                .success(),
            Auth::KeyFile { path, passphrase } => {
                let key = russh::keys::load_secret_key(
                    path,
                    passphrase.as_ref().map(|p| p.expose_secret()),
                )
                .map_err(|e| SshError::Key(path.display().to_string(), e.to_string()))?;
                let hash = handle.best_supported_rsa_hash().await?.flatten();
                handle
                    .authenticate_publickey(
                        &user,
                        russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), hash),
                    )
                    .await?
                    .success()
            }
            Auth::Agent => {
                let mut agent = connect_agent().await?;
                let identities = agent
                    .request_identities()
                    .await
                    .map_err(|e| SshError::Other(format!("ssh-agent: {e}")))?;
                let hash = handle.best_supported_rsa_hash().await?.flatten();
                let mut ok = false;
                for identity in identities {
                    let key = identity.public_key().into_owned();
                    let r = handle
                        .authenticate_publickey_with(&user, key, hash, &mut agent)
                        .await
                        .map_err(|e| SshError::Other(format!("ssh-agent: {e}")))?;
                    if r.success() {
                        ok = true;
                        break;
                    }
                }
                ok
            }
        };
        if !ok {
            return Err(SshError::Auth {
                user,
                host: config.host.clone(),
                method: match config.auth {
                    Auth::Password(_) => "password",
                    Auth::KeyFile { .. } => "key file",
                    Auth::Agent => "agent",
                },
            });
        }
        Ok(Self { handle, config })
    }

    /// Open an interactive shell with a PTY and hand it over as a terminal
    /// transport plus its output reader.
    pub async fn open_shell(&self, cols: u16, rows: u16) -> Result<(ShellTransport, ShellReader)> {
        let channel = self.handle.channel_open_session().await?;
        channel
            .request_pty(false, "xterm-256color", cols as u32, rows as u32, 0, 0, &[])
            .await?;
        channel.request_shell(false).await?;
        Ok(shell::spawn(channel))
    }

    /// Open the SFTP subsystem.
    pub async fn open_sftp(&self) -> Result<SftpSession> {
        let channel = self.handle.channel_open_session().await?;
        channel.request_subsystem(true, "sftp").await?;
        let sftp = SftpSession::new(channel.into_stream()).await?;
        Ok(sftp)
    }

    /// A `direct-tcpip` channel to `host:port` as seen from the server (one
    /// connection of an `ssh -L` forward).
    pub async fn open_direct_tcpip(
        &self,
        host: &str,
        port: u16,
        originator: &str,
        originator_port: u16,
    ) -> Result<russh::ChannelStream<client::Msg>> {
        let channel = self
            .handle
            .channel_open_direct_tcpip(host, port as u32, originator, originator_port as u32)
            .await?;
        Ok(channel.into_stream())
    }

    pub fn is_closed(&self) -> bool {
        self.handle.is_closed()
    }

    pub async fn disconnect(&self) {
        let _ = self
            .handle
            .disconnect(russh::Disconnect::ByApplication, "", "en")
            .await;
    }
}

#[cfg(windows)]
async fn connect_agent()
-> Result<russh::keys::agent::client::AgentClient<tokio::net::windows::named_pipe::NamedPipeClient>>
{
    russh::keys::agent::client::AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent")
        .await
        .map_err(|e| SshError::Other(format!("ssh-agent: {e}")))
}

#[cfg(not(windows))]
async fn connect_agent() -> Result<russh::keys::agent::client::AgentClient<tokio::net::UnixStream>>
{
    russh::keys::agent::client::AgentClient::connect_env()
        .await
        .map_err(|e| SshError::Other(format!("ssh-agent: {e}")))
}

/// Shell-quote a path for `cd`.
pub fn shell_quote(path: &str) -> String {
    format!("'{}'", path.replace('\'', "'\\''"))
}

/// The working directory a shell announces through its window title
/// (`user@host: /path`, `user@host:~/dir`, `/path`), if any.
pub fn cwd_from_title(title: &str) -> Option<String> {
    let t = title.trim();
    let candidate = match t.split_once(": ") {
        Some((_, rest)) => rest.trim(),
        None => match t.split_once(':') {
            Some((left, rest)) if left.contains('@') => rest.trim(),
            _ => t,
        },
    };
    let candidate = candidate.split(" - ").next().unwrap_or(candidate).trim();
    if candidate.starts_with('/') || candidate == "~" || candidate.starts_with("~/") {
        Some(candidate.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cwd_is_parsed_from_common_prompt_titles() {
        assert_eq!(
            cwd_from_title("ubuntu@web-01: /var/www").as_deref(),
            Some("/var/www")
        );
        assert_eq!(
            cwd_from_title("ubuntu@web-01:~/app").as_deref(),
            Some("~/app")
        );
        assert_eq!(cwd_from_title("ubuntu@web-01: ~").as_deref(), Some("~"));
        assert_eq!(cwd_from_title("/srv - vim").as_deref(), Some("/srv"));
        assert_eq!(cwd_from_title("vim main.rs"), None);
        assert_eq!(shell_quote("/it's here"), "'/it'\\''s here'");
    }
}
