//! The AI Assistant's prompt buttons, and the user's changes to them
//! (docs/04-ui-design.md, owner's request 2026-10-23).
//!
//! Every activity's assistant offers a row of one-click prompts. They used
//! to be written into each tool, so the only way to change one was to edit
//! the source. The defaults still live here in code - that is what makes
//! them shared and translatable - but Settings → AI Assistant → Prompt
//! Buttons can rename them, rewrite what they ask, hide them and put them
//! in another order, and a tool reads the result rather than its own list.
//!
//! The settings are **global, not per workspace** (owner's rule): a prompt
//! is how the user likes to ask a question, which does not change because
//! they opened a different project.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A prompt button as it ships: an activity, a stable id, and the Fluent
/// keys for what it says and what it asks.
pub struct Default_ {
    pub activity: &'static str,
    /// Stable across renames and translations: the label's Fluent key.
    pub id: &'static str,
    pub label_key: &'static str,
    pub prompt_key: &'static str,
}

/// The activities that have an assistant, in the order Settings lists
/// them. The second element is the Fluent key for the activity's name.
/// The editor and the Scratchpad are deliberately absent: their
/// assistants have no prompt buttons to configure, and listing an
/// activity whose edits would do nothing is worse than not listing it.
pub const ACTIVITIES: &[(&str, &str)] = &[
    ("terminal", "tool-terminal"),
    ("ssh", "tool-ssh"),
    ("db", "tool-db"),
    ("redis", "tool-redis"),
    ("nosql", "tool-nosql"),
    ("s3", "tool-s3"),
    ("api", "tool-api"),
    ("logs", "tool-logs"),
    ("graphics", "tool-graphics"),
    ("container", "tool-containers"),
    ("diagram", "gfx-fn-diagram"),
    ("generator", "gfx-fn-generator"),
];

macro_rules! qa {
    ($activity:literal, $id:literal) => {
        Default_ {
            activity: $activity,
            id: $id,
            label_key: $id,
            prompt_key: concat!($id, "-prompt"),
        }
    };
}

/// Every prompt button HyperStack ships, by activity. Adding one is a row
/// here plus its two Fluent messages; nothing else changes.
pub const DEFAULTS: &[Default_] = &[
    // The shell tools share one set: a command ran, and the question is
    // about what it printed.
    qa!("terminal", "ai-qa-explain-output"),
    qa!("terminal", "ai-qa-explain-error"),
    qa!("terminal", "ai-qa-next-command"),
    qa!("terminal", "ai-qa-summarise"),
    qa!("ssh", "ai-qa-explain-output"),
    qa!("ssh", "ai-qa-explain-error"),
    qa!("ssh", "ai-qa-next-command"),
    qa!("ssh", "ai-qa-summarise"),
    qa!("db", "ai-qa-select-all"),
    qa!("db", "ai-qa-count"),
    qa!("db", "ai-qa-select-ordered"),
    qa!("db", "ai-qa-explain-query"),
    qa!("db", "ai-qa-improve-query"),
    qa!("redis", "ai-qa-redis-explain-key"),
    qa!("redis", "ai-qa-redis-command"),
    qa!("redis", "ai-qa-redis-memory"),
    qa!("nosql", "ai-qa-nosql-filter"),
    qa!("nosql", "ai-qa-nosql-explain"),
    qa!("nosql", "ai-qa-nosql-index"),
    qa!("s3", "ai-qa-explain-selection"),
    qa!("s3", "ai-qa-aws-cli"),
    qa!("s3", "ai-qa-bucket-policy"),
    qa!("api", "ai-qa-explain-response"),
    qa!("api", "ai-qa-pre-script"),
    qa!("api", "ai-qa-post-script"),
    qa!("api", "ai-qa-save-vars"),
    qa!("api", "ai-qa-tests"),
    qa!("api", "ai-qa-docs"),
    // The Container activity asks about two different things with one
    // set of buttons: an object that is on screen (explain it, say why
    // it is unhealthy) and one the user wants written (a manifest). The
    // writing ones answer with a YAML block, which the pane can put
    // straight into its editor.
    qa!("container", "ai-qa-container-explain"),
    qa!("container", "ai-qa-container-unhealthy"),
    qa!("container", "ai-qa-container-event"),
    qa!("container", "ai-qa-container-review-yaml"),
    qa!("container", "ai-qa-container-fix-yaml"),
    qa!("container", "ai-qa-container-right-size"),
    qa!("container", "ai-qa-container-deployment"),
    qa!("container", "ai-qa-container-service"),
    qa!("container", "ai-qa-container-ingress"),
    qa!("container", "ai-qa-container-configmap"),
    qa!("container", "ai-qa-container-pvc"),
    qa!("container", "ai-qa-container-compose"),
    qa!("logs", "ai-qa-logs-explain"),
    qa!("logs", "ai-qa-logs-trace"),
    qa!("logs", "ai-qa-logs-why"),
    qa!("logs", "ai-qa-logs-filter"),
    qa!("logs", "ai-qa-logs-suggest"),
    qa!("logs", "ai-qa-logs-summarise"),
    qa!("graphics", "ai-qa-image-explain"),
    qa!("graphics", "ai-qa-image-edits"),
    qa!("graphics", "ai-qa-image-export"),
    qa!("graphics", "ai-qa-image-a11y"),
    qa!("diagram", "ai-qa-diagram-layout"),
    qa!("diagram", "ai-qa-diagram-missing"),
    qa!("diagram", "ai-qa-diagram-name"),
    qa!("diagram", "ai-qa-diagram-a11y"),
    qa!("generator", "ai-qa-gen-refine"),
    qa!("generator", "ai-qa-gen-critique"),
    qa!("generator", "ai-qa-gen-layout"),
    qa!("generator", "ai-qa-gen-explain"),
];

/// One button as the user has it: what it says, what it asks, and whether
/// it is shown. A blank `label` or `prompt` means "whatever it ships as",
/// so a translation still applies to a button the user only reordered or
/// hid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Button {
    pub id: String,
    pub label: String,
    pub prompt: String,
    pub hidden: bool,
}

/// The user's changes, by activity. An activity absent here uses the
/// defaults as they ship.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Prompts {
    /// Activity id to its buttons, in the order the assistant shows them.
    /// An id that is not in [`DEFAULTS`] is the user's own button.
    pub by_activity: BTreeMap<String, Vec<Button>>,
}

/// A button resolved for an activity: what to draw and what to send.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub id: String,
    pub label: String,
    pub prompt: String,
    pub hidden: bool,
    /// False for a button the user added, which can be deleted.
    pub shipped: bool,
}

impl Prompts {
    /// Every button for an activity, in order, shipped and user-added,
    /// hidden ones included. The caller filters; Settings wants them all.
    ///
    /// `text` resolves a Fluent key, so this stays free of the i18n type.
    pub fn resolve(&self, activity: &str, text: &dyn Fn(&str) -> String) -> Vec<Resolved> {
        let shipped: Vec<&Default_> = DEFAULTS.iter().filter(|d| d.activity == activity).collect();
        let saved = self.by_activity.get(activity);
        let mut out: Vec<Resolved> = Vec::new();

        // The saved order wins, and carries the user's edits.
        if let Some(saved) = saved {
            for b in saved {
                let ship = shipped.iter().find(|d| d.id == b.id);
                out.push(Resolved {
                    id: b.id.clone(),
                    label: if b.label.is_empty() {
                        ship.map(|d| text(d.label_key))
                            .unwrap_or_else(|| b.id.clone())
                    } else {
                        b.label.clone()
                    },
                    prompt: if b.prompt.is_empty() {
                        ship.map(|d| text(d.prompt_key)).unwrap_or_default()
                    } else {
                        b.prompt.clone()
                    },
                    hidden: b.hidden,
                    shipped: ship.is_some(),
                });
            }
        }
        // A button added by a later version is appended rather than lost,
        // so an upgrade brings its new prompts with it.
        for d in shipped {
            if out.iter().any(|r| r.id == d.id) {
                continue;
            }
            out.push(Resolved {
                id: d.id.to_string(),
                label: text(d.label_key),
                prompt: text(d.prompt_key),
                hidden: false,
                shipped: true,
            });
        }
        out
    }

    /// What the assistant draws: in order, with the hidden ones gone.
    pub fn visible(&self, activity: &str, text: &dyn Fn(&str) -> String) -> Vec<Resolved> {
        self.resolve(activity, text)
            .into_iter()
            .filter(|r| !r.hidden)
            .collect()
    }

    /// Replace an activity's list, keeping only what differs from the
    /// defaults so the file stays small and a translation keeps applying.
    pub fn set(&mut self, activity: &str, buttons: Vec<Resolved>, text: &dyn Fn(&str) -> String) {
        let shipped: Vec<&Default_> = DEFAULTS.iter().filter(|d| d.activity == activity).collect();
        let kept: Vec<Button> = buttons
            .into_iter()
            .map(|r| {
                let ship = shipped.iter().find(|d| d.id == r.id);
                Button {
                    label: match ship {
                        Some(d) if text(d.label_key) == r.label => String::new(),
                        _ => r.label,
                    },
                    prompt: match ship {
                        Some(d) if text(d.prompt_key) == r.prompt => String::new(),
                        _ => r.prompt,
                    },
                    hidden: r.hidden,
                    id: r.id,
                }
            })
            .collect();
        self.by_activity.insert(activity.to_string(), kept);
    }

    /// Forget an activity's changes, so it ships as it shipped.
    pub fn reset(&mut self, activity: &str) {
        self.by_activity.remove(activity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(key: &str) -> String {
        // A stand-in for Fluent: the key with a marker, so a label that
        // came from the defaults is told apart from one the user typed.
        format!("<{key}>")
    }

    #[test]
    fn an_activity_with_no_changes_ships_as_it_ships() {
        let p = Prompts::default();
        let got = p.resolve("logs", &text);
        assert_eq!(got.len(), 6, "every Logs button");
        assert_eq!(got[0].id, "ai-qa-logs-explain");
        assert_eq!(got[0].label, "<ai-qa-logs-explain>");
        assert_eq!(got[0].prompt, "<ai-qa-logs-explain-prompt>");
        assert!(got.iter().all(|r| r.shipped && !r.hidden));
    }

    #[test]
    fn hiding_reordering_and_renaming_all_survive_a_round_trip() {
        let mut p = Prompts::default();
        let mut list = p.resolve("logs", &text);
        list.reverse();
        list[0].hidden = true;
        list[1].label = "Why?".into();
        list[1].prompt = "Tell me why this failed.".into();
        p.set("logs", list, &text);

        let back = p.resolve("logs", &text);
        assert_eq!(back[0].id, "ai-qa-logs-summarise");
        assert!(back[0].hidden, "the order and the hiding both kept");
        assert_eq!(back[1].label, "Why?");
        assert_eq!(back[1].prompt, "Tell me why this failed.");
        // Only the two changed buttons carry anything; the rest defer to
        // the defaults, so a translation still reaches them.
        let saved = &p.by_activity["logs"];
        assert_eq!(saved.iter().filter(|b| !b.label.is_empty()).count(), 1);
        assert_eq!(saved.iter().filter(|b| b.hidden).count(), 1);

        let shown = p.visible("logs", &text);
        assert_eq!(shown.len(), 5, "the hidden one is not drawn");
    }

    #[test]
    fn a_button_added_by_a_later_version_appears_without_losing_the_order() {
        let mut p = Prompts::default();
        // A saved list from before `ai-qa-logs-suggest` existed.
        p.by_activity.insert(
            "logs".into(),
            vec![
                Button {
                    id: "ai-qa-logs-why".into(),
                    ..Default::default()
                },
                Button {
                    id: "ai-qa-logs-explain".into(),
                    ..Default::default()
                },
            ],
        );
        let got = p.resolve("logs", &text);
        assert_eq!(got[0].id, "ai-qa-logs-why", "the saved order leads");
        assert_eq!(got[1].id, "ai-qa-logs-explain");
        assert!(
            got.iter().any(|r| r.id == "ai-qa-logs-suggest"),
            "a newly shipped button is appended, not dropped"
        );
        assert_eq!(got.len(), 6);
    }

    #[test]
    fn a_button_the_user_added_is_kept_and_marked_as_theirs() {
        let mut p = Prompts::default();
        let mut list = p.resolve("db", &text);
        list.push(Resolved {
            id: "mine-1".into(),
            label: "Find the slow bit".into(),
            prompt: "Which part of this query is slow?".into(),
            hidden: false,
            shipped: false,
        });
        p.set("db", list, &text);
        let back = p.resolve("db", &text);
        let mine = back.iter().find(|r| r.id == "mine-1").expect("kept");
        assert_eq!(mine.label, "Find the slow bit");
        assert!(!mine.shipped, "it is the user's, so it can be deleted");
    }

    #[test]
    fn resetting_an_activity_forgets_every_change() {
        let mut p = Prompts::default();
        let mut list = p.resolve("logs", &text);
        list[0].hidden = true;
        p.set("logs", list, &text);
        assert!(p.by_activity.contains_key("logs"));
        p.reset("logs");
        assert_eq!(
            p.resolve("logs", &text),
            Prompts::default().resolve("logs", &text)
        );
    }

    #[test]
    fn every_default_belongs_to_an_activity_settings_lists() {
        for d in DEFAULTS {
            assert!(
                ACTIVITIES.iter().any(|(id, _)| *id == d.activity),
                "{} is not an activity Settings shows",
                d.activity
            );
        }
        // And no id is used twice within one activity.
        for (activity, _) in ACTIVITIES {
            let ids: Vec<&str> = DEFAULTS
                .iter()
                .filter(|d| d.activity == *activity)
                .map(|d| d.id)
                .collect();
            let mut sorted = ids.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(ids.len(), sorted.len(), "{activity} has a duplicate id");
        }
    }
}
