//! The server monitor strip (docs/tools/ssh-sftp.md §9): one read-only
//! script over the exec channel every few seconds, parsed here into the
//! figures the strip shows. Linux `/proc` only; a host that answers in
//! another shape shows "no figures" rather than wrong ones.

/// What the strip asks the host, with `---` between the parts so a part
/// that is missing (no `ps`, no `df`) does not shift the others.
pub const SCRIPT: &str = "cat /proc/loadavg 2>/dev/null; echo ---; head -1 /proc/stat 2>/dev/null; echo ---; \
grep -E '^(MemTotal|MemAvailable)' /proc/meminfo 2>/dev/null; echo ---; cat /proc/uptime 2>/dev/null; echo ---; \
df -Pk / 2>/dev/null | tail -1; echo ---; ps -eo pcpu,pmem,comm --sort=-pcpu 2>/dev/null | head -6";

/// How often the strip asks.
pub const EVERY: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sample {
    pub load: [f32; 3],
    /// Jiffies from `/proc/stat`'s `cpu` line: everything, and idle.
    pub cpu_total: u64,
    pub cpu_idle: u64,
    pub mem_total_kb: u64,
    pub mem_avail_kb: u64,
    pub uptime_secs: u64,
    pub disk_used_kb: u64,
    pub disk_total_kb: u64,
    /// (%cpu, %mem, command), hottest first.
    pub procs: Vec<(f32, f32, String)>,
}

/// Parse the script's output. `None` when not even the load average was
/// there, which is what a non-Linux host answers.
pub fn parse(text: &str) -> Option<Sample> {
    let parts: Vec<&str> = text.split("---").collect();
    let part = |i: usize| parts.get(i).map(|p| p.trim()).unwrap_or("");
    let mut s = Sample::default();
    let mut load = part(0).split_whitespace();
    s.load = [
        load.next()?.parse().ok()?,
        load.next().and_then(|x| x.parse().ok()).unwrap_or(0.0),
        load.next().and_then(|x| x.parse().ok()).unwrap_or(0.0),
    ];
    let stat: Vec<u64> = part(1)
        .split_whitespace()
        .skip(1)
        .filter_map(|x| x.parse().ok())
        .collect();
    if stat.len() >= 4 {
        s.cpu_total = stat.iter().sum();
        s.cpu_idle = stat[3] + stat.get(4).copied().unwrap_or(0);
    }
    for line in part(2).lines() {
        let kb = line
            .split_whitespace()
            .nth(1)
            .and_then(|x| x.parse().ok())
            .unwrap_or(0);
        if line.starts_with("MemTotal") {
            s.mem_total_kb = kb;
        } else if line.starts_with("MemAvailable") {
            s.mem_avail_kb = kb;
        }
    }
    s.uptime_secs = part(3)
        .split_whitespace()
        .next()
        .and_then(|x| x.parse::<f64>().ok())
        .unwrap_or(0.0) as u64;
    let df: Vec<&str> = part(4).split_whitespace().collect();
    if df.len() >= 4 {
        s.disk_total_kb = df[1].parse().unwrap_or(0);
        s.disk_used_kb = df[2].parse().unwrap_or(0);
    }
    for line in part(5).lines().skip(1) {
        let mut cols = line.split_whitespace();
        let (Some(c), Some(m)) = (cols.next(), cols.next()) else {
            continue;
        };
        let name = cols.collect::<Vec<_>>().join(" ");
        if let (Ok(c), Ok(m)) = (c.parse::<f32>(), m.parse::<f32>()) {
            s.procs.push((c, m, name));
        }
    }
    Some(s)
}

/// CPU use between two readings, in percent.
pub fn cpu_percent(prev: &Sample, cur: &Sample) -> Option<f32> {
    let total = cur.cpu_total.checked_sub(prev.cpu_total)?;
    let idle = cur.cpu_idle.checked_sub(prev.cpu_idle)?;
    (total > 0).then(|| 100.0 * (total - idle) as f32 / total as f32)
}

pub fn human_kb(kb: u64) -> String {
    let b = kb as f64 * 1024.0;
    const K: f64 = 1024.0;
    if b >= K * K * K {
        format!("{:.1} GB", b / (K * K * K))
    } else if b >= K * K {
        format!("{:.0} MB", b / (K * K))
    } else {
        format!("{kb} KB")
    }
}

pub fn human_uptime(s: u64) -> String {
    let (d, h, m) = (s / 86_400, (s % 86_400) / 3600, (s % 3600) / 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUT: &str = "0.52 0.31 0.20 2/1038 4321\n---\ncpu  1000 10 500 8000 100 0 20 0 0 0\n---\nMemTotal:       8000000 kB\nMemAvailable:   5000000 kB\n---\n123456.78 400000.00\n---\n/dev/sda1 100000000 45000000 50000000 48% /\n---\n%CPU %MEM COMMAND\n12.5  3.1 postgres: writer\n 2.0  0.5 sshd\n";

    #[test]
    fn a_linux_answer_parses() {
        let s = parse(OUT).expect("parsed");
        assert_eq!(s.load, [0.52, 0.31, 0.20]);
        assert_eq!(s.cpu_total, 9630);
        assert_eq!(s.cpu_idle, 8100);
        assert_eq!(s.mem_total_kb, 8_000_000);
        assert_eq!(s.mem_avail_kb, 5_000_000);
        assert_eq!(s.uptime_secs, 123_456);
        assert_eq!(s.disk_total_kb, 100_000_000);
        assert_eq!(s.disk_used_kb, 45_000_000);
        assert_eq!(s.procs.len(), 2);
        assert_eq!(s.procs[0].2, "postgres: writer");
        assert!(parse("bash: cat: not found\n---\n").is_none());
    }

    #[test]
    fn cpu_use_comes_from_two_readings() {
        let a = parse(OUT).unwrap();
        let mut b = a.clone();
        b.cpu_total += 1000;
        b.cpu_idle += 750;
        assert_eq!(cpu_percent(&a, &b), Some(25.0));
        assert_eq!(cpu_percent(&b, &a), None);
        assert_eq!(human_uptime(123_456), "1d 10h");
        assert_eq!(human_kb(5_000_000), "4.8 GB");
    }
}
