//! The Docker pane, Portainer-shaped (`docs/tools/containers.md`
//! §10.6): a resource rail, a filterable table, and a detail block with
//! the verbs under it.
//!
//! Logs and shells are **handed to the tools that already do them**
//! (§10.1): View Logs opens a Logs tab with the follow command
//! pre-filled, Exec opens a terminal running the exec line. Neither is
//! rebuilt here, so the Logs tool's filters, levels, time strip and
//! assistant all come free.

use hs_container::cli::Verb;
use hs_container::compose::ComposeVerb;
use hs_container::{Container, Image, Network, Project, State, Volume};
use hs_core::Services;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use serde::{Deserialize, Serialize};

/// Which list the pane is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resource {
    #[default]
    Containers,
    /// Compose projects, grouped from the containers' own labels.
    Projects,
    Images,
    Volumes,
    Networks,
    /// Swarm, listed only where the engine is a manager.
    Stacks,
    Services,
    Nodes,
}

impl Resource {
    /// Everything a Docker host can show. The Swarm three are kept in
    /// the list and *disabled* off a swarm rather than hidden: a rail
    /// that changes length as hosts are switched is harder to use, and
    /// "not a swarm" is information.
    pub const ALL: [Resource; 8] = [
        Resource::Containers,
        Resource::Projects,
        Resource::Images,
        Resource::Volumes,
        Resource::Networks,
        Resource::Stacks,
        Resource::Services,
        Resource::Nodes,
    ];

    /// Whether this is a Swarm surface, which only a manager answers.
    pub fn is_swarm(self) -> bool {
        matches!(
            self,
            Resource::Stacks | Resource::Services | Resource::Nodes
        )
    }

    pub fn key(self) -> &'static str {
        match self {
            Resource::Containers => "container-res-containers",
            Resource::Projects => "container-res-projects",
            Resource::Images => "container-res-images",
            Resource::Volumes => "container-res-volumes",
            Resource::Networks => "container-res-networks",
            Resource::Stacks => "container-res-stacks",
            Resource::Services => "container-res-services",
            Resource::Nodes => "container-res-nodes",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Resource::Containers => ph::SHIPPING_CONTAINER,
            Resource::Projects => ph::SQUARES_FOUR,
            Resource::Images => ph::STACK,
            Resource::Volumes => ph::HARD_DRIVES,
            Resource::Networks => ph::TREE_STRUCTURE,
            Resource::Stacks => ph::CUBE,
            Resource::Services => ph::GEAR_SIX,
            Resource::Nodes => ph::DESKTOP,
        }
    }
}

/// Why a verb is not available. Saying which is the point: "you cannot"
/// and "you cannot *here*" send someone to different places.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The profile is marked read-only.
    ReadOnly,
    /// The environment is protected, and Settings says those are
    /// read-only (the default).
    Protected(String),
}

impl Refusal {
    pub fn key(&self) -> &'static str {
        match self {
            Refusal::ReadOnly => "container-refused-read-only",
            Refusal::Protected(_) => "container-refused-protected",
        }
    }
}

/// The sentence a refusal is explained with, wherever it is shown.
///
/// One place rather than the same `match` at every verb row: the
/// protected form takes an argument and the read-only form does not, so
/// a copy of this is a copy of that difference too.
pub fn refusal_text(s: &Services, r: &Refusal) -> String {
    match r {
        Refusal::ReadOnly => s.t(r.key()),
        Refusal::Protected(env) => s.i18n.t_args(r.key(), &[("env", env)]),
    }
}

/// Whether a destructive verb may run against this host.
///
/// A protected environment wins over the profile: a production cluster
/// is not a place to discover a mis-click, and the setting that governs
/// it is on by default (`Settings → Containers → Defaults and Safety`).
pub fn guard(s: &Services, profile: &hs_core::connections::ConnectionProfile) -> Option<Refusal> {
    if profile.read_only {
        return Some(Refusal::ReadOnly);
    }
    if !s.settings.read().containers.protected_read_only {
        return None;
    }
    let envs = s.environments.read();
    let env = profile
        .environment
        .as_deref()
        .and_then(|id| envs.all().iter().find(|e| e.id == id).cloned());
    match env {
        Some(e) if e.protected => Some(Refusal::Protected(e.name)),
        _ => None,
    }
}

/// What the detail block is showing.
#[derive(Debug, Clone, Default)]
pub struct Selection {
    pub container: Option<Container>,
}

/// Draw the resource rail. Returns the resource clicked, if any.
///
/// `swarm` says whether the engine is a swarm manager: the three Swarm
/// rows are **disabled and explained** off a swarm rather than hidden,
/// because a rail that changes length between hosts is harder to use
/// and "this engine is not in a swarm" is information someone wants.
pub fn resource_rail(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    current: Resource,
    counts: &[usize],
    swarm: hs_container::swarm::SwarmState,
) -> Option<Resource> {
    let mut picked = None;
    ui.vertical(|ui| {
        ui.set_width(150.0);
        for (i, r) in Resource::ALL.into_iter().enumerate() {
            if r == Resource::Stacks {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(s.t("container-res-swarm"))
                        .small()
                        .color(theme.text_muted),
                );
                ui.add_space(2.0);
            }
            // The count is the quieter half of the row: it changes on
            // every refresh, and the name is what is being chosen.
            let mut label = egui::text::LayoutJob::default();
            let font = egui::FontId::proportional(14.0);
            label.append(
                &format!("{}  {}  ", r.icon(), s.t(r.key())),
                0.0,
                egui::TextFormat::simple(font.clone(), theme.text),
            );
            if let Some(n) = counts.get(i) {
                label.append(
                    &n.to_string(),
                    0.0,
                    egui::TextFormat::simple(font, theme.text_muted),
                );
            }
            let usable = !r.is_swarm() || swarm.can_manage();
            let resp = ui.add_enabled(
                usable,
                egui::Button::selectable(current == r, label).min_size(egui::vec2(140.0, 0.0)),
            );
            let resp = if usable {
                resp
            } else {
                resp.on_disabled_hover_text(s.t(swarm.key()))
            };
            if resp.clicked() {
                picked = Some(r);
            }
            ui.add_space(2.0);
        }
    });
    picked
}

/// The columns and rows for one resource, as the shared table wants
/// them. Pure, so the shapes are testable without a window.
pub fn table_data(
    resource: Resource,
    s: &Services,
    inv: &crate::Inventory,
) -> (Vec<hs_ui::table::ColumnSpec>, Vec<Vec<String>>) {
    use hs_ui::table::ColumnSpec;
    let containers: &[Container] = &inv.containers;
    let images: &[Image] = &inv.image_list;
    let volumes: &[Volume] = &inv.volume_list;
    let networks: &[Network] = &inv.network_list;
    match resource {
        Resource::Containers => (
            vec![
                ColumnSpec::new(s.t("container-col-name"), 200.0),
                ColumnSpec::new(s.t("container-col-image"), 220.0),
                ColumnSpec::new(s.t("container-col-state"), 100.0),
                ColumnSpec::new(s.t("container-col-status"), 170.0),
                ColumnSpec::new(s.t("container-col-ports"), 220.0),
                ColumnSpec::new(s.t("container-col-stack"), 140.0),
            ],
            containers
                .iter()
                .map(|c| {
                    vec![
                        c.name.clone(),
                        c.image.clone(),
                        s.t(c.state.key()),
                        c.status.clone(),
                        c.ports
                            .iter()
                            .map(|p| p.describe())
                            .collect::<Vec<_>>()
                            .join(", "),
                        c.compose_project().unwrap_or("").to_string(),
                    ]
                })
                .collect(),
        ),
        Resource::Images => (
            vec![
                ColumnSpec::new(s.t("container-col-image"), 320.0),
                ColumnSpec::new(s.t("container-col-size"), 120.0),
                ColumnSpec::new(s.t("container-col-created"), 200.0),
                ColumnSpec::new(s.t("container-col-id"), 160.0),
            ],
            images
                .iter()
                .map(|i| {
                    vec![
                        i.reference(),
                        i.size.clone(),
                        i.created.clone(),
                        short(&i.id),
                    ]
                })
                .collect(),
        ),
        Resource::Volumes => (
            vec![
                ColumnSpec::new(s.t("container-col-name"), 280.0),
                ColumnSpec::new(s.t("container-col-driver"), 120.0),
                ColumnSpec::new(s.t("container-col-mountpoint"), 420.0),
            ],
            volumes
                .iter()
                .map(|v| vec![v.name.clone(), v.driver.clone(), v.mountpoint.clone()])
                .collect(),
        ),
        Resource::Networks => (
            vec![
                ColumnSpec::new(s.t("container-col-name"), 240.0),
                ColumnSpec::new(s.t("container-col-driver"), 120.0),
                ColumnSpec::new(s.t("container-col-scope"), 120.0),
                ColumnSpec::new(s.t("container-col-id"), 160.0),
            ],
            networks
                .iter()
                .map(|n| {
                    vec![
                        n.name.clone(),
                        n.driver.clone(),
                        n.scope.clone(),
                        short(&n.id),
                    ]
                })
                .collect(),
        ),
        Resource::Projects => (
            vec![
                ColumnSpec::new(s.t("container-col-name"), 220.0),
                ColumnSpec::new(s.t("container-col-services"), 110.0),
                ColumnSpec::new(s.t("container-col-state"), 120.0),
                ColumnSpec::new(s.t("container-col-files"), 420.0),
            ],
            inv.projects
                .iter()
                .map(|p| {
                    vec![
                        p.name.clone(),
                        p.services.len().to_string(),
                        // Running out of total, which is the whole
                        // story of a project that is half up.
                        format!("{}/{}", p.running(), p.services.len()),
                        p.config_files.join(", "),
                    ]
                })
                .collect(),
        ),
        Resource::Stacks => (
            vec![
                ColumnSpec::new(s.t("container-col-name"), 240.0),
                ColumnSpec::new(s.t("container-col-services"), 110.0),
                ColumnSpec::new(s.t("container-col-orchestrator"), 140.0),
            ],
            inv.stacks
                .iter()
                .map(|st| {
                    vec![
                        st.name.clone(),
                        st.services.to_string(),
                        st.orchestrator.clone(),
                    ]
                })
                .collect(),
        ),
        Resource::Services => (
            vec![
                ColumnSpec::new(s.t("container-col-name"), 240.0),
                ColumnSpec::new(s.t("container-col-mode"), 120.0),
                ColumnSpec::new(s.t("container-col-replicas"), 110.0),
                ColumnSpec::new(s.t("container-col-image"), 260.0),
                ColumnSpec::new(s.t("container-col-ports"), 200.0),
            ],
            inv.services
                .iter()
                .map(|sv| {
                    vec![
                        sv.name.clone(),
                        sv.mode.clone(),
                        sv.replicas(),
                        sv.image.clone(),
                        sv.ports.clone(),
                    ]
                })
                .collect(),
        ),
        Resource::Nodes => (
            vec![
                ColumnSpec::new(s.t("container-col-hostname"), 220.0),
                ColumnSpec::new(s.t("container-col-status"), 120.0),
                ColumnSpec::new(s.t("container-col-availability"), 140.0),
                ColumnSpec::new(s.t("container-col-manager"), 140.0),
                ColumnSpec::new(s.t("container-col-engine"), 120.0),
            ],
            inv.nodes
                .iter()
                .map(|n| {
                    vec![
                        n.hostname.clone(),
                        n.status.clone(),
                        n.availability.clone(),
                        n.manager_status.clone(),
                        n.engine_version.clone(),
                    ]
                })
                .collect(),
        ),
    }
}

/// The Compose verbs offered for a project, in the order shown.
///
/// A project with no compose file can only be acted on through the
/// labels the engine already holds: `up` there would be run against
/// whatever is in the current directory, which is how a project gets
/// recreated as something else entirely.
pub fn compose_verbs_for(p: &Project) -> Vec<ComposeVerb> {
    let all = [
        ComposeVerb::Up,
        ComposeVerb::Restart,
        ComposeVerb::Stop,
        ComposeVerb::Start,
        ComposeVerb::Pull,
        ComposeVerb::Config,
        ComposeVerb::Down,
    ];
    all.into_iter()
        .filter(|v| p.has_files() || !v.needs_files())
        .collect()
}

/// An id as every container tool shows it, without panicking on a short
/// one or splitting a character in half.
fn short(id: &str) -> String {
    let body = id.strip_prefix("sha256:").unwrap_or(id);
    body.chars().take(12).collect()
}

/// The verbs offered for a container, in the order they are shown.
///
/// Start is offered only for something stopped and Stop only for
/// something running: a button that cannot do anything is worse than no
/// button, because it reads as the tool being broken.
pub fn verbs_for(state: State) -> Vec<Verb> {
    if state.is_running() {
        vec![Verb::Restart, Verb::Stop, Verb::Kill, Verb::Remove]
    } else {
        vec![Verb::Start, Verb::Remove]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_id_drops_the_digest_prefix_and_never_splits_a_character() {
        assert_eq!(short("sha256:abcdef0123456789"), "abcdef012345");
        assert_eq!(short("abc"), "abc");
        assert_eq!(short(""), "");
        // A multi-byte id must not panic or produce invalid text.
        assert_eq!(short("ünïcödé-in-an-id").chars().count(), 12);
    }

    /// A button that cannot do anything reads as the tool being broken,
    /// so the verbs follow the state.
    #[test]
    fn the_verbs_follow_the_state() {
        let running = verbs_for(State::Running);
        assert!(running.contains(&Verb::Stop));
        assert!(running.contains(&Verb::Restart));
        assert!(!running.contains(&Verb::Start), "it is already running");

        let stopped = verbs_for(State::Exited);
        assert!(stopped.contains(&Verb::Start));
        assert!(!stopped.contains(&Verb::Stop), "it is already stopped");
        assert!(!stopped.contains(&Verb::Kill), "there is nothing to kill");

        // Remove is offered either way, and is destructive either way.
        for v in [verbs_for(State::Running), verbs_for(State::Exited)] {
            assert!(v.contains(&Verb::Remove));
        }
    }

    /// Every verb this pane offers that changes the world must be
    /// marked destructive, or the guards never fire for it.
    #[test]
    fn every_offered_verb_that_changes_things_is_marked_destructive() {
        for state in [State::Running, State::Exited, State::Paused] {
            for v in verbs_for(state) {
                if v == Verb::Start {
                    assert!(!v.destructive(), "starting breaks nothing");
                } else {
                    assert!(v.destructive(), "{v:?} should be guarded");
                }
            }
        }
    }

    #[test]
    fn every_resource_has_its_own_icon_and_label() {
        let mut icons: Vec<&str> = Resource::ALL.iter().map(|r| r.icon()).collect();
        icons.sort_unstable();
        icons.dedup();
        assert_eq!(icons.len(), Resource::ALL.len());
        for r in Resource::ALL {
            assert!(r.key().starts_with("container-res-"));
        }
    }

    /// A refusal has to say *which* refusal: "you cannot" and "you
    /// cannot here" send someone to different places.
    #[test]
    fn the_two_refusals_are_told_apart() {
        assert_ne!(
            Refusal::ReadOnly.key(),
            Refusal::Protected("prod".into()).key()
        );
    }
}
