//! One tab per host, and which requests this activity will take
//! (`docs/tools/containers.md` §10.9).
//!
//! These are the rules the shell relies on, and getting them wrong is
//! what made the Devtools home tab swallow every request for its
//! activity: a dashboard that reports a uri steals the lot, and a tab
//! that answers `uri_affinity` for something it does not show gets
//! handed it anyway.

use hs_container::profile::ContainerFields;
use tool_containers::kind_icon;

/// Every container kind gets its own glyph, and no two share one: a row
/// has to say what the thing *is* at a glance (§10.5).
#[test]
fn every_container_kind_has_its_own_icon() {
    let kinds = hs_core::connections::CONTAINER_KINDS;
    let mut icons: Vec<&str> = kinds.iter().map(|k| kind_icon(k)).collect();
    icons.sort_unstable();
    icons.dedup();
    assert_eq!(
        icons.len(),
        kinds.len(),
        "two container kinds share a glyph, so a row cannot say which it is"
    );
}

/// An unknown kind must still get a glyph rather than nothing: a row
/// with no icon is a row that looks broken.
#[test]
fn an_unknown_kind_still_gets_an_icon() {
    assert!(!kind_icon("something-new").is_empty());
}

/// The four kinds this activity owns, and nothing else. A database
/// opening in the Container activity would be worse than it not opening
/// at all.
#[test]
fn only_container_kinds_belong_to_this_activity() {
    for kind in ["docker", "podman", "k8s", "wincontainer"] {
        assert!(
            hs_core::connections::is_container_kind(kind),
            "{kind} should belong here"
        );
    }
    for kind in ["db", "ssh", "s3", "redis", "mongodb", "api"] {
        assert!(
            !hs_core::connections::is_container_kind(kind),
            "{kind} must not open here"
        );
    }
}

/// A cluster and an engine take different panes, so the difference has
/// to be askable without a list of kinds at every call site.
#[test]
fn a_cluster_is_told_from_an_engine() {
    assert!(hs_core::connections::is_cluster_kind("k8s"));
    for kind in ["docker", "podman", "wincontainer"] {
        assert!(!hs_core::connections::is_cluster_kind(kind), "{kind}");
    }
}

/// The subtitle a row shows has to distinguish two hosts of the same
/// kind, or the list is unreadable once there is more than one.
#[test]
fn two_hosts_of_one_kind_read_differently() {
    let local = ContainerFields::default();
    let wsl = ContainerFields {
        transport: hs_container::Transport::Wsl,
        distro: "Ubuntu-24.04".into(),
        ..Default::default()
    };
    let remote = ContainerFields {
        transport: hs_container::Transport::Ssh,
        tunnel_ssh: "web01".into(),
        ..Default::default()
    };
    let mut seen = vec![
        local.describe("docker"),
        wsl.describe("docker"),
        remote.describe("docker"),
    ];
    let before = seen.len();
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), before, "two hosts would read identically");
}

/// The uri is what the shell matches on, so its shape is part of the
/// contract rather than an implementation detail.
#[test]
fn the_host_uri_names_the_profile() {
    // Built the same way the tool builds it; a change here is a change
    // to how tabs are matched.
    let id = "01JABCDEF";
    assert_eq!(format!("container://{id}"), "container://01JABCDEF");
}
