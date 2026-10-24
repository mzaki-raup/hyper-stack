//! The driver against a real engine. Skipped unless it is told where one
//! is, exactly as the SQL, Redis and NoSQL driver tests are.
//!
//! ```text
//! HS_DOCKER=local                 cargo test -p hs-container --test live
//! HS_DOCKER=wsl:Ubuntu-24.04      cargo test -p hs-container --test live
//! HS_DOCKER=tcp:10.0.0.5:2375     cargo test -p hs-container --test live
//! HS_PODMAN=local                 cargo test -p hs-container --test live
//! ```
//!
//! On Windows the engine is very often inside WSL rather than on the
//! host, which is why `wsl:<distro>` is a first-class form here and not
//! an afterthought (owner's report, 2026-10-23).
//!
//! **Every test here only reads.** Nothing is started, stopped or
//! removed: a developer's own containers are not a fixture, and a test
//! suite that could stop them would be a test suite nobody runs.

use hs_container::cli::Engine;
use hs_container::profile::{ContainerFields, Transport};
use hs_container::run;

/// `(kind, engine)` for each engine the environment points at.
fn engines() -> Vec<(&'static str, Engine)> {
    let mut out = Vec::new();
    for (var, kind) in [("HS_DOCKER", "docker"), ("HS_PODMAN", "podman")] {
        let Ok(spec) = std::env::var(var) else {
            continue;
        };
        let Some(fields) = parse_spec(&spec) else {
            panic!("{var}={spec} is not a form this test understands");
        };
        out.push((kind, Engine::new(kind, fields)));
    }
    out
}

fn parse_spec(spec: &str) -> Option<ContainerFields> {
    let spec = spec.trim();
    if spec == "local" {
        return Some(ContainerFields::default());
    }
    if let Some(distro) = spec.strip_prefix("wsl:") {
        return Some(ContainerFields {
            transport: Transport::Wsl,
            distro: distro.to_string(),
            ..Default::default()
        });
    }
    if let Some(rest) = spec.strip_prefix("tcp:") {
        let (host, port) = rest.rsplit_once(':')?;
        return Some(ContainerFields {
            transport: Transport::Tcp,
            host: host.to_string(),
            port: port.parse().ok()?,
            ..Default::default()
        });
    }
    None
}

/// Say why a run did nothing, rather than passing silently: a live test
/// that quietly skips is a live test that is never noticed to be
/// skipping.
fn skip_note() {
    eprintln!(
        "no engine configured: set HS_DOCKER or HS_PODMAN \
         (local | wsl:<distro> | tcp:<host>:<port>)"
    );
}

#[test]
fn the_engine_answers_and_names_its_version() {
    let engines = engines();
    if engines.is_empty() {
        skip_note();
        return;
    }
    for (kind, e) in &engines {
        let version = run::probe(e).unwrap_or_else(|err| panic!("{kind}: {err}"));
        assert!(!version.is_empty(), "{kind} answered with no version");
        eprintln!("{kind} {version}");
    }
}

#[test]
fn containers_come_back_parsed() {
    let engines = engines();
    if engines.is_empty() {
        skip_note();
        return;
    }
    for (kind, e) in &engines {
        let got = run::containers(e).unwrap_or_else(|err| panic!("{kind}: {err}"));
        eprintln!("{kind}: {} containers", got.len());
        for c in &got {
            // Whatever is running, these are the invariants the panes
            // depend on. An empty id would make every verb unaddressable.
            assert!(!c.id.is_empty(), "a container with no id");
            assert!(
                c.id.len() >= 12,
                "--no-trunc should give the full id, got {:?}",
                c.id
            );
            assert!(!c.name.is_empty(), "{} has no name", c.id);
            assert!(!c.image.is_empty(), "{} has no image", c.name);
            // The state must be recognised; `Unknown` means the engine
            // used a word this driver has not been taught.
            assert_ne!(
                c.state,
                hs_container::State::Unknown,
                "{} reported an unrecognised state: {:?}",
                c.name,
                c.status
            );
            // Every port must have parsed into something addressable.
            for p in &c.ports {
                assert!(p.container > 0, "{} has a zero container port", c.name);
                assert!(!p.protocol.is_empty());
            }
        }
    }
}

#[test]
fn images_volumes_and_networks_come_back_parsed() {
    let engines = engines();
    if engines.is_empty() {
        skip_note();
        return;
    }
    for (kind, e) in &engines {
        let images = run::images(e).unwrap_or_else(|err| panic!("{kind} images: {err}"));
        for i in &images {
            assert!(!i.id.is_empty());
            assert!(!i.reference().is_empty());
        }
        let volumes = run::volumes(e).unwrap_or_else(|err| panic!("{kind} volumes: {err}"));
        for v in &volumes {
            assert!(!v.name.is_empty());
        }
        let networks = run::networks(e).unwrap_or_else(|err| panic!("{kind} networks: {err}"));
        // Every engine has a default network, so an empty list here
        // means the parser missed them rather than that there are none.
        assert!(
            !networks.is_empty(),
            "{kind} reported no networks at all, which no engine does"
        );
        for n in &networks {
            assert!(!n.name.is_empty());
        }
        eprintln!(
            "{kind}: {} images, {} volumes, {} networks",
            images.len(),
            volumes.len(),
            networks.len()
        );
    }
}

/// Asking about something that does not exist must be a refusal, not an
/// "unreachable": the pane says different things for the two, and
/// getting it backwards would send someone to check their socket when
/// they mistyped a name.
#[test]
fn a_missing_container_is_a_refusal_rather_than_an_unreachable_engine() {
    let engines = engines();
    if engines.is_empty() {
        skip_note();
        return;
    }
    for (kind, e) in &engines {
        let inv = e.invoke(["inspect", "hs-container-definitely-not-here"]);
        match run::output(&inv, &e.env()) {
            Err(run::Error::Refused(message)) => {
                eprintln!("{kind} refused as expected: {message}");
            }
            Err(other) => panic!("{kind}: expected a refusal, got {other}"),
            Ok(text) => panic!("{kind}: expected a refusal, got output {text:?}"),
        }
    }
}
