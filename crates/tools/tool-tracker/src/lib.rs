//! HyperStack Tracker tool (docs/tools/tracker.md): issue trackers and
//! wikis - Jira, Redmine, GitHub, GitLab and Forgejo issues, Confluence
//! pages - behind one [`driver::Driver`] trait, one tab per tracker.

pub mod confluence;
pub mod driver;
pub mod forgejo;
pub mod github;
pub mod gitlab;
pub mod jira;
pub mod markup;
pub mod profile;
pub mod redmine;
pub mod runner;
pub mod ui;

pub use ui::{DESCRIPTOR, TrackerFactory};
