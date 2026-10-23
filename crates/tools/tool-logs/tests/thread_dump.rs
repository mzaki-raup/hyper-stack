//! Reading a JVM thread dump (`jstack`, `kill -3`) as a log: one record
//! per thread, its stack folded in, and the fields a dump is actually
//! searched by - state, the monitor it holds, the monitor it wants -
//! pulled out of the lines *below* each thread header.
//!
//! Owner's request, 2026-10-24.

use tool_logs::parse::{
    Format, Level, Record, absorb_continuation, detect, is_continuation, parse,
};

/// A real HotSpot dump, trimmed: the timestamp and VM line, a daemon
/// thread parked on a queue, a thread holding a lock, a thread blocked on
/// that same lock, and the deadlock section.
const DUMP: &str = r#"2026-10-24 09:15:02
Full thread dump OpenJDK 64-Bit Server VM (17.0.9+9 mixed mode, sharing):

"http-nio-8080-exec-1" #42 daemon prio=5 os_prio=0 cpu=12.34ms elapsed=100.00s tid=0x00007f8a1c0b2000 nid=0x1a2b waiting on condition  [0x00007f89f4bfe000]
   java.lang.Thread.State: WAITING (parking)
	at jdk.internal.misc.Unsafe.park(java.base@17.0.9/Native Method)
	- parking to wait for  <0x000000070f1f1f10> (a java.util.concurrent.locks.AbstractQueuedSynchronizer$ConditionObject)
	at java.util.concurrent.LinkedBlockingQueue.take(java.base@17.0.9/LinkedBlockingQueue.java:433)

"worker-7" #51 prio=5 os_prio=0 tid=0x00007f8a1c0c4000 nid=0x1a3c runnable  [0x00007f89f4afd000]
   java.lang.Thread.State: RUNNABLE
	at com.example.Ledger.post(Ledger.java:88)
	- locked <0x000000070aaa0000> (a com.example.Ledger)

"worker-8" #52 prio=5 os_prio=0 tid=0x00007f8a1c0c5000 nid=0x1a3d waiting for monitor entry  [0x00007f89f49fc000]
   java.lang.Thread.State: BLOCKED (on object monitor)
	at com.example.Ledger.post(Ledger.java:88)
	- waiting to lock <0x000000070aaa0000> (a com.example.Ledger)

Found one Java-level deadlock:
=============================
"worker-8":
  waiting to lock monitor 0x00007f8a1c0d0000,
  which is held by "worker-7""#;

/// Reproduce what `engine::Shared::push_line` does: a record per header
/// line, with continuation lines folded in and their fields absorbed.
fn records(text: &str) -> Vec<Record> {
    let format = detect(&text.lines().collect::<Vec<_>>());
    assert_eq!(format, Format::ThreadDump, "a dump must detect as one");
    let mut out: Vec<Record> = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if let Some(last) = out.last_mut()
            && is_continuation(format, line)
        {
            last.raw.push('\n');
            last.raw.push_str(line);
            absorb_continuation(format, last, line);
            continue;
        }
        out.push(parse(format, i as u64 + 1, line));
    }
    out
}

fn thread<'a>(recs: &'a [Record], name: &str) -> &'a Record {
    recs.iter()
        .find(|r| r.field("thread") == Some(name))
        .unwrap_or_else(|| panic!("no thread {name} among {} records", recs.len()))
}

#[test]
fn a_thread_becomes_one_record_with_its_stack_folded_in() {
    let recs = records(DUMP);
    let t = thread(&recs, "http-nio-8080-exec-1");
    assert_eq!(t.field("id"), Some("42"));
    assert_eq!(t.field("daemon"), Some("true"));
    assert_eq!(t.field("prio"), Some("5"));
    assert_eq!(t.field("tid"), Some("0x00007f8a1c0b2000"));
    assert_eq!(t.field("nid"), Some("0x1a2b"));
    assert_eq!(t.field("cpu"), Some("12.34ms"));
    assert_eq!(t.field("elapsed"), Some("100.00s"));
    assert_eq!(t.field("activity"), Some("waiting on condition"));
    // Read from the lines below the header.
    assert_eq!(t.field("state"), Some("WAITING"));
    assert_eq!(t.field("state_detail"), Some("parking"));
    assert_eq!(t.field("frames"), Some("2"));
    assert_eq!(
        t.field("top"),
        Some("jdk.internal.misc.Unsafe.park(java.base@17.0.9/Native Method)")
    );
    // The whole stack is still there to read.
    assert!(t.raw.contains("LinkedBlockingQueue.take"));
    assert!(t.raw.lines().count() > 3);
}

#[test]
fn a_non_daemon_thread_is_not_marked_daemon() {
    let recs = records(DUMP);
    assert_eq!(thread(&recs, "worker-7").field("daemon"), None);
    assert_eq!(thread(&recs, "worker-7").field("state"), Some("RUNNABLE"));
}

/// What a dump is opened to find: who holds the monitor, who wants it,
/// and the one state worth colouring.
#[test]
fn the_lock_a_thread_holds_and_the_one_it_waits_for_are_fields() {
    let recs = records(DUMP);
    let holder = thread(&recs, "worker-7");
    assert_eq!(
        holder.field("locked"),
        Some("0x000000070aaa0000 (com.example.Ledger)")
    );
    assert_eq!(holder.field("waiting_for"), None);
    assert_eq!(holder.level, None, "a running thread is not a warning");

    let blocked = thread(&recs, "worker-8");
    assert_eq!(blocked.field("state"), Some("BLOCKED"));
    assert_eq!(blocked.field("state_detail"), Some("on object monitor"));
    assert_eq!(
        blocked.field("waiting_for"),
        Some("0x000000070aaa0000 (com.example.Ledger)")
    );
    assert_eq!(
        blocked.level,
        Some(Level::Error),
        "BLOCKED is why someone took the dump, so it gets a colour"
    );
    // Both threads name the same monitor, which is what makes the
    // contention findable by filtering on one address.
    assert_eq!(blocked.field("waiting_for"), holder.field("locked"));
}

#[test]
fn the_snapshot_header_and_the_deadlock_section_are_their_own_records() {
    let recs = records(DUMP);
    let snaps: Vec<&Record> = recs
        .iter()
        .filter(|r| r.field("kind") == Some("snapshot"))
        .collect();
    assert_eq!(snaps.len(), 2, "the timestamp line and the VM line");
    assert!(snaps[0].ts.is_some(), "the dump's own time is read");
    assert!(snaps[1].message.starts_with("Full thread dump"));

    let dead: Vec<&Record> = recs
        .iter()
        .filter(|r| r.field("kind") == Some("deadlock"))
        .collect();
    assert_eq!(dead.len(), 1);
    assert_eq!(dead[0].level, Some(Level::Error));
    assert!(dead[0].message.contains("deadlock"));
}

#[test]
fn every_thread_is_found_and_no_line_is_lost() {
    let recs = records(DUMP);
    let threads: Vec<&str> = recs
        .iter()
        .filter(|r| r.field("kind") == Some("thread"))
        .map(|r| r.field("thread").unwrap())
        .collect();
    assert_eq!(
        threads,
        ["http-nio-8080-exec-1", "worker-7", "worker-8"],
        "one record per thread, in dump order"
    );
    // The strongest form of "nothing is lost": the records put back
    // together are the file again, blank lines and tabs included.
    let rebuilt = recs
        .iter()
        .map(|r| r.raw.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(rebuilt, DUMP, "the records must reassemble into the dump");
}

/// A dump taken repeatedly into one file is several snapshots; each
/// `Full thread dump` must start a new one rather than fold into the
/// thread above it.
#[test]
fn repeated_dumps_in_one_file_stay_separate_snapshots() {
    let twice = format!("{DUMP}\n{DUMP}");
    let recs = records(&twice);
    let snaps = recs
        .iter()
        .filter(|r| r.message.starts_with("Full thread dump"))
        .count();
    assert_eq!(snaps, 2, "each jstack run is its own snapshot header");
    let threads = recs
        .iter()
        .filter(|r| r.field("kind") == Some("thread"))
        .count();
    assert_eq!(threads, 6);
}

/// A plain log with a stack trace in it must not be read as a dump, and
/// a single thread header is enough to recognise one.
#[test]
fn detection_does_not_confuse_a_dump_with_an_ordinary_log() {
    let plain = [
        "2026-10-24 09:00:00 INFO  starting up",
        "2026-10-24 09:00:01 ERROR failed to connect",
        "\tat com.example.Boot.main(Boot.java:12)",
    ];
    assert_eq!(detect(&plain), Format::Plain);
    let dump = ["\"main\" #1 prio=5 tid=0x1 nid=0x2 runnable"];
    assert_eq!(detect(&dump), Format::ThreadDump);
}

/// The deadlock section names the threads in the cycle; following it is
/// the next question after "there is a deadlock" (owner's request,
/// 2026-10-24).
#[test]
fn the_deadlock_cycle_names_the_threads_it_involves() {
    let recs = records(DUMP);
    let section = recs
        .iter()
        .find(|r| r.field("kind") == Some("deadlock"))
        .expect("the deadlock section is a record");
    let cycle = tool_logs::parse::deadlock_cycle(&section.raw);
    assert_eq!(cycle.len(), 1, "one link in this dump");
    assert_eq!(cycle[0].waiter, "worker-8");
    assert_eq!(cycle[0].holder, "worker-7");
    assert_eq!(cycle[0].monitor, "0x00007f8a1c0d0000");
    // Both ends must be threads the dump actually carries, or the names
    // could not be used to open them.
    for name in [&cycle[0].waiter, &cycle[0].holder] {
        assert!(
            recs.iter()
                .any(|r| r.field("thread") == Some(name.as_str())),
            "{name} is not a thread in the dump"
        );
    }
}

/// A full cycle: two threads each waiting on the other's monitor, which
/// is what a real `jstack` prints.
#[test]
fn both_halves_of_a_two_thread_cycle_are_read() {
    let section = "Found one Java-level deadlock:\n\
         =============================\n\
         \"worker-8\":\n\
         \x20 waiting to lock monitor 0x00007f8a1c0d0000,\n\
         \x20 which is held by \"worker-7\"\n\
         \"worker-7\":\n\
         \x20 waiting to lock monitor 0x00007f8a1c0d0010,\n\
         \x20 which is held by \"worker-8\"";
    let cycle = tool_logs::parse::deadlock_cycle(section);
    assert_eq!(cycle.len(), 2);
    assert_eq!(cycle[1].waiter, "worker-7");
    assert_eq!(cycle[1].holder, "worker-8");
    assert_eq!(cycle[1].monitor, "0x00007f8a1c0d0010");
}

/// A section with no deadlock in it, and a half-written entry, must
/// yield nothing rather than a half-read edge.
#[test]
fn an_unrecognised_deadlock_section_yields_nothing() {
    assert!(tool_logs::parse::deadlock_cycle("JNI global refs: 42").is_empty());
    assert!(
        tool_logs::parse::deadlock_cycle("\"worker-8\":\n  waiting to lock monitor 0x1,")
            .is_empty(),
        "an entry with no holder is not an edge"
    );
}
