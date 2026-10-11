//! What a terminal session talks to: a local PTY today, an SSH channel in
//! `hs-ssh`. Output arrives through a separate blocking `Read`; this trait
//! covers the write side and lifecycle.

pub trait Transport: Send {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()>;
    fn resize(&mut self, cols: u16, rows: u16);
    /// Exit code once the other side has gone away.
    fn try_wait(&mut self) -> Option<u32>;
    /// Ask the other side to stop (SIGKILL / channel close).
    fn kill(&mut self);
    /// Release handles so a blocked reader unblocks.
    fn close(&mut self);
}

impl Transport for crate::pty::Pty {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        crate::pty::Pty::write(self, bytes)
    }
    fn resize(&mut self, cols: u16, rows: u16) {
        crate::pty::Pty::resize(self, cols, rows)
    }
    fn try_wait(&mut self) -> Option<u32> {
        crate::pty::Pty::try_wait(self)
    }
    fn kill(&mut self) {
        crate::pty::Pty::kill(self)
    }
    fn close(&mut self) {
        crate::pty::Pty::close(self)
    }
}

/// A `Read` fed by another thread or task through a channel; `read` blocks
/// until bytes arrive and reports EOF when every sender is gone.
pub struct ChannelReader {
    rx: std::sync::mpsc::Receiver<Vec<u8>>,
    pending: Vec<u8>,
    offset: usize,
}

impl ChannelReader {
    pub fn new(rx: std::sync::mpsc::Receiver<Vec<u8>>) -> Self {
        Self {
            rx,
            pending: Vec::new(),
            offset: 0,
        }
    }
}

impl std::io::Read for ChannelReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.offset >= self.pending.len() {
            match self.rx.recv() {
                Ok(chunk) => {
                    self.pending = chunk;
                    self.offset = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let n = (self.pending.len() - self.offset).min(buf.len());
        buf[..n].copy_from_slice(&self.pending[self.offset..self.offset + n]);
        self.offset += n;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn channel_reader_delivers_chunks_then_eof() {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(b"hello ".to_vec()).unwrap();
        tx.send(b"world".to_vec()).unwrap();
        drop(tx);
        let mut r = ChannelReader::new(rx);
        let mut out = String::new();
        r.read_to_string(&mut out).unwrap();
        assert_eq!(out, "hello world");
    }
}
