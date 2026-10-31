//! HyperStack Automation tool (docs/tools/automation.md): build servers and
//! automation controllers - Jenkins, TeamCity, GitHub Actions, GitLab CI,
//! Forgejo Actions and AWX - behind one [`driver::Driver`] trait, one tab
//! per server.

pub mod awx;
pub mod driver;
pub mod forgejo;
pub mod github;
pub mod gitlab;
pub mod http;
pub mod jenkins;
pub mod profile;
pub mod runner;
pub mod teamcity;
pub mod ui;

pub use ui::{AutomationFactory, DESCRIPTOR};
