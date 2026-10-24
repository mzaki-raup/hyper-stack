//! The assistant's action blocks (`docs/tools/containers.md` §10.8).
//!
//! An answer may carry an **`hs-container`** code block describing
//! something to do rather than something to read: scale a workload,
//! roll it, set an image. The pane carries it out through exactly the
//! paths a person's clicks use, guards included - the block is a
//! request, never a command.
//!
//! Mirrors the API client's `hs-api` blocks: one fence language, one
//! JSON object, and a verb the host understands. Anything else is
//! refused in words rather than guessed at, because a misread action
//! against a cluster is not a typo the user can see.

use hs_core::Services;
use hs_core::connections::ConnectionProfile;
use serde::Deserialize;

/// The fence language an action block carries.
pub const LANG: &str = "hs-container";

/// What an action block asks for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    /// Set a workload's replica count.
    Scale {
        kind: String,
        #[serde(default)]
        namespace: String,
        name: String,
        replicas: i32,
    },
    /// Roll a workload's pods.
    Restart {
        kind: String,
        #[serde(default)]
        namespace: String,
        name: String,
    },
    /// Change the image of one container in a workload.
    SetImage {
        kind: String,
        #[serde(default)]
        namespace: String,
        name: String,
        container: String,
        image: String,
    },
}

impl Action {
    /// The object it is about, for the confirmation.
    pub fn what(&self) -> String {
        match self {
            Action::Scale { kind, name, .. }
            | Action::Restart { kind, name, .. }
            | Action::SetImage { kind, name, .. } => format!("{kind}/{name}"),
        }
    }

    /// The namespace it names, which may be empty (the tab's own).
    pub fn namespace(&self) -> &str {
        match self {
            Action::Scale { namespace, .. }
            | Action::Restart { namespace, .. }
            | Action::SetImage { namespace, .. } => namespace,
        }
    }

    /// The sentence the confirmation asks.
    pub fn question(&self, s: &Services) -> String {
        match self {
            Action::Scale { replicas, .. } => s.i18n.t_args(
                "cluster-confirm-scale",
                &[("what", &self.what()), ("replicas", &replicas.to_string())],
            ),
            Action::Restart { .. } => s.i18n.t_args(
                "cluster-confirm-verb",
                &[
                    ("verb", &s.t("cluster-verb-restart")),
                    ("what", &self.what()),
                ],
            ),
            Action::SetImage {
                container, image, ..
            } => s.i18n.t_args(
                "container-action-confirm-image",
                &[
                    ("what", &self.what()),
                    ("container", container),
                    ("image", image),
                ],
            ),
        }
    }
}

/// The manifest a Set Image action applies.
///
/// A **strategic merge patch expressed as a manifest**: apiVersion,
/// kind, name and the one container being changed. Server-side apply
/// merges containers by name, so this changes that container's image
/// and leaves everything else - the other containers, the replica
/// count, the labels - exactly as they are.
pub fn set_image_patch(
    kind: &str,
    namespace: &str,
    name: &str,
    container: &str,
    image: &str,
) -> String {
    let api_version = match kind {
        "Deployment" | "StatefulSet" | "DaemonSet" => "apps/v1",
        "Job" | "CronJob" => "batch/v1",
        _ => "apps/v1",
    };
    let ns = if namespace.trim().is_empty() {
        String::new()
    } else {
        format!("  namespace: {}\n", namespace.trim())
    };
    format!(
        "apiVersion: {api_version}\n\
         kind: {kind}\n\
         metadata:\n\
         {ns}  name: {name}\n\
         spec:\n\
         \x20 template:\n\
         \x20   spec:\n\
         \x20     containers:\n\
         \x20       - name: {container}\n\
         \x20         image: {image}\n"
    )
}

/// Read an action block.
///
/// The error is the reader's, not serde's: "that is not an action this
/// pane knows" is something a person can act on, where a type error
/// deep in a JSON path is not.
pub fn parse(body: &str, s: &Services) -> Result<Action, String> {
    serde_json::from_str::<Action>(body.trim()).map_err(|e| {
        s.i18n
            .t_args("container-action-bad", &[("why", &e.to_string())])
    })
}

/// What the assistant is told it may ask for.
///
/// Sent as part of the system prompt, and **only** when the host allows
/// writes: an assistant that is told it can scale a production cluster
/// will offer to, and a refusal afterwards is a worse answer than never
/// having been offered (ADR-019's rule for `can_run`).
pub fn system_prompt(s: &Services, profile: Option<&ConnectionProfile>) -> String {
    let Some(profile) = profile else {
        return String::new();
    };
    if !hs_core::connections::is_cluster_kind(&profile.kind) {
        return String::new();
    }
    if crate::docker::guard(s, profile).is_some() {
        // Read-only host or protected environment: say so, so the
        // answer suggests what to change rather than offering to do it.
        return s.t("container-action-system-read-only");
    }
    s.t("container-action-system")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_action_block_is_read_as_its_verb() {
        let scale: Action = serde_json::from_str(
            r#"{"action":"scale","kind":"Deployment","namespace":"shop","name":"web","replicas":3}"#,
        )
        .expect("scale");
        assert_eq!(
            scale,
            Action::Scale {
                kind: "Deployment".into(),
                namespace: "shop".into(),
                name: "web".into(),
                replicas: 3,
            }
        );
        assert_eq!(scale.what(), "Deployment/web");
        assert_eq!(scale.namespace(), "shop");

        // The namespace is optional: an answer about the open list need
        // not repeat what the tab already knows.
        let restart: Action =
            serde_json::from_str(r#"{"action":"restart","kind":"StatefulSet","name":"db"}"#)
                .expect("restart");
        assert_eq!(restart.namespace(), "");
        assert_eq!(restart.what(), "StatefulSet/db");

        let image: Action = serde_json::from_str(
            r#"{"action":"set_image","kind":"Deployment","name":"web","container":"nginx","image":"nginx:1.29"}"#,
        )
        .expect("set_image");
        assert!(matches!(image, Action::SetImage { .. }));
    }

    /// A Set Image patch must name the container and change nothing
    /// else: server-side apply merges containers by name, so a patch
    /// that listed the others would claim ownership of them too.
    #[test]
    fn the_set_image_patch_touches_one_container() {
        let yaml = set_image_patch("Deployment", "shop", "web", "nginx", "nginx:1.29");
        assert!(yaml.contains("apiVersion: apps/v1"), "{yaml}");
        assert!(yaml.contains("kind: Deployment"), "{yaml}");
        assert!(yaml.contains("namespace: shop"), "{yaml}");
        assert!(yaml.contains("name: web"), "{yaml}");
        assert!(yaml.contains("- name: nginx"), "{yaml}");
        assert!(yaml.contains("image: nginx:1.29"), "{yaml}");
        // Nothing about replicas, labels or anything else the manifest
        // being patched owns.
        assert!(!yaml.contains("replicas"), "{yaml}");
        assert!(!yaml.contains("selector"), "{yaml}");

        // A namespace the block left out is the tab's, so the manifest
        // must not invent one.
        let bare = set_image_patch("StatefulSet", "", "db", "postgres", "postgres:17");
        assert!(!bare.contains("namespace"), "{bare}");
        assert!(bare.contains("kind: StatefulSet"), "{bare}");
    }

    /// An answer that invents a verb must be refused, not half-read:
    /// the one thing worse than "I cannot do that" is doing something
    /// else.
    #[test]
    fn an_unknown_action_is_refused() {
        assert!(
            serde_json::from_str::<Action>(r#"{"action":"delete","kind":"Pod","name":"web"}"#)
                .is_err()
        );
        assert!(serde_json::from_str::<Action>(r#"{"kind":"Pod","name":"web"}"#).is_err());
        // A scale with no number is not a scale to zero.
        assert!(
            serde_json::from_str::<Action>(
                r#"{"action":"scale","kind":"Deployment","name":"web"}"#
            )
            .is_err()
        );
    }
}
