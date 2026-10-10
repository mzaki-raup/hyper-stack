//! `tracing` setup: a daily rolling file in `logs/`, an in-memory ring buffer
//! for the Output panel, and redaction of every secret value currently known.

use std::collections::VecDeque;
use std::io::Write;
use std::sync::Arc;

use parking_lot::{Mutex, RwLock};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Values to replace with `***` in every log line.
#[derive(Clone, Default)]
pub struct Redactor {
    values: Arc<RwLock<Vec<String>>>,
}

impl Redactor {
    pub fn add(&self, value: &str) {
        if value.len() >= 4 {
            let mut v = self.values.write();
            if !v.iter().any(|x| x == value) {
                v.push(value.to_string());
            }
        }
    }
    pub fn clear(&self) {
        self.values.write().clear();
    }
    pub fn apply(&self, text: &str) -> String {
        crate::secrets::redact(text, &self.values.read())
    }
}

/// The last N log lines, for the Output dock.
#[derive(Clone, Default)]
pub struct LogBuffer {
    lines: Arc<Mutex<VecDeque<String>>>,
}

impl LogBuffer {
    pub fn snapshot(&self) -> Vec<String> {
        self.lines.lock().iter().cloned().collect()
    }
    pub fn clear(&self) {
        self.lines.lock().clear();
    }
}

struct TeeWriter {
    file: Option<tracing_appender::non_blocking::NonBlocking>,
    buffer: LogBuffer,
    redactor: Redactor,
}

impl Write for TeeWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let text = self.redactor.apply(&String::from_utf8_lossy(buf));
        if let Some(f) = &mut self.file {
            let _ = f.write_all(text.as_bytes());
        }
        let mut lines = self.buffer.lines.lock();
        for l in text.lines() {
            lines.push_back(l.to_string());
        }
        while lines.len() > 5000 {
            lines.pop_front();
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        if let Some(f) = &mut self.file {
            f.flush()?;
        }
        Ok(())
    }
}

#[derive(Clone)]
struct TeeMaker {
    file: Option<tracing_appender::non_blocking::NonBlocking>,
    buffer: LogBuffer,
    redactor: Redactor,
}

impl<'a> MakeWriter<'a> for TeeMaker {
    type Writer = TeeWriter;
    fn make_writer(&'a self) -> Self::Writer {
        TeeWriter {
            file: self.file.clone(),
            buffer: self.buffer.clone(),
            redactor: self.redactor.clone(),
        }
    }
}

pub struct Logging {
    pub buffer: LogBuffer,
    pub redactor: Redactor,
    _guard: Option<tracing_appender::non_blocking::WorkerGuard>,
}

/// Initialise once. `logs_dir = None` keeps logs in memory only (tests, self-test).
pub fn init(logs_dir: Option<&std::path::Path>, filter: &str) -> Logging {
    let buffer = LogBuffer::default();
    let redactor = Redactor::default();
    let (file, guard) = match logs_dir {
        Some(dir) => {
            let appender = tracing_appender::rolling::daily(dir, "hyperstack.log");
            let (nb, guard) = tracing_appender::non_blocking(appender);
            (Some(nb), Some(guard))
        }
        None => (None, None),
    };
    let maker = TeeMaker {
        file,
        buffer: buffer.clone(),
        redactor: redactor.clone(),
    };
    let env_filter = tracing_subscriber::EnvFilter::try_new(filter)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::registry()
        .with(env_filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_target(true)
                .with_writer(maker),
        )
        .try_init();
    Logging {
        buffer,
        redactor,
        _guard: guard,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redactor_masks_known_values() {
        let r = Redactor::default();
        r.add("hunter22");
        r.add("ab"); // too short, ignored
        assert_eq!(r.apply("password=hunter22 user=ab"), "password=*** user=ab");
    }
}
