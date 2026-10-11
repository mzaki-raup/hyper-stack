//! The `Tunnels` service (docs/03-architecture.md §5, docs/tools/ssh-sftp.md
//! §8): SSH local port forwards that any tool can ask for. The interface
//! lives here so tools depend only on `hs-core`; the russh implementation is
//! `hs_ssh::tunnels::TunnelManager`, installed by the binary at start-up.

use tokio::sync::oneshot;

/// What a forward connects: `127.0.0.1:local_port` → (via the SSH host)
/// `remote_host:remote_port`. `local_port == 0` picks a free port.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ForwardSpec {
    /// Id of a `kind = "ssh"` connection profile.
    pub ssh_id: String,
    pub remote_host: String,
    pub remote_port: u16,
    pub local_port: u16,
}

/// Why a forward could not start. The UI translates the named cases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TunnelError {
    /// No SSH connection profile with that id.
    NoProfile,
    /// The vault is locked, so the password or passphrase is unavailable.
    VaultLocked,
    /// Password authentication without a stored password.
    NoPassword,
    /// The host key is not trusted yet; a background tunnel cannot ask, so
    /// the user connects once in the SSH tool to trust it.
    UnknownHostKey,
    /// Anything else (network, authentication, bind), as reported.
    Other(String),
}

impl std::fmt::Display for TunnelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TunnelError::NoProfile => f.write_str("no such SSH connection"),
            TunnelError::VaultLocked => f.write_str("the vault is locked"),
            TunnelError::NoPassword => f.write_str("no password is stored for this host"),
            TunnelError::UnknownHostKey => f.write_str("the host key is not trusted yet"),
            TunnelError::Other(e) => f.write_str(e),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TunnelState {
    Starting,
    Listening { local_port: u16 },
    Failed(TunnelError),
}

/// One forward as the UI lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelInfo {
    pub id: u64,
    pub spec: ForwardSpec,
    /// The SSH profile's display name.
    pub via: String,
    pub state: TunnelState,
    /// Connections open now, and accepted in total.
    pub open: u32,
    pub total: u64,
    /// Bytes local → remote and remote → local.
    pub bytes_up: u64,
    pub bytes_down: u64,
}

pub type ForwardResult = Result<u16, TunnelError>;

pub trait Tunnels: Send + Sync {
    /// The local port of a forward to `remote_host:remote_port` through
    /// `ssh_id`, starting one unless an equivalent forward is already
    /// listening (or starting, in which case the answer waits for it).
    fn ensure_local_forward(
        &self,
        ssh_id: &str,
        remote_host: &str,
        remote_port: u16,
    ) -> oneshot::Receiver<ForwardResult>;

    /// Start a forward exactly as specified (the SSH tool's "New Tunnel").
    /// Progress shows in [`Tunnels::list`].
    fn open(&self, spec: ForwardSpec) -> u64;

    /// Close a forward and its connections.
    fn stop(&self, id: u64);

    fn list(&self) -> Vec<TunnelInfo>;
}

/// Used where no SSH implementation is installed (tests, `--self-test`).
pub struct NoTunnels;

impl Tunnels for NoTunnels {
    fn ensure_local_forward(&self, _: &str, _: &str, _: u16) -> oneshot::Receiver<ForwardResult> {
        let (tx, rx) = oneshot::channel();
        let _ = tx.send(Err(TunnelError::Other(
            "SSH tunnels are not available".into(),
        )));
        rx
    }
    fn open(&self, _: ForwardSpec) -> u64 {
        0
    }
    fn stop(&self, _: u64) {}
    fn list(&self) -> Vec<TunnelInfo> {
        Vec::new()
    }
}
