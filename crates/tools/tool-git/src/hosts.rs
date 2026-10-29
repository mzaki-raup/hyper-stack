//! The hosts' REST APIs, as far as the Git activity needs them
//! (docs/tools/git-activity.md §3): **Fetch Project** lists the
//! repositories a credential can see, and **Test** asks who the token
//! belongs to. One request shape per host kind; the answers are reduced
//! to [`Project`] so the form never sees a host's own JSON.
//!
//! Every call is `async` on the tokio runtime and returns through a slot
//! the caller polls; nothing here blocks the UI thread (ADR-013).

use hs_core::git_hosts::{GitCredential, HostKind};
use serde_json::Value;

/// One repository a host lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// `owner/name`.
    pub full_name: String,
    pub description: String,
    pub clone_url: String,
    pub default_branch: String,
    pub private: bool,
}

/// What to send: the first page's URL, how to go on, the headers.
struct Shape {
    page_url: fn(&str, u32) -> String,
}

fn shape(kind: HostKind) -> Option<Shape> {
    Some(match kind {
        HostKind::GitHub => Shape {
            page_url: |base, page| {
                format!(
                    "{base}/user/repos?per_page=100&page={page}&sort=updated&affiliation=owner,collaborator,organization_member"
                )
            },
        },
        HostKind::GitLab => Shape {
            page_url: |base, page| {
                format!(
                    "{base}/api/v4/projects?membership=true&simple=true&per_page=100&page={page}&order_by=last_activity_at"
                )
            },
        },
        HostKind::Forgejo | HostKind::Gitea => Shape {
            page_url: |base, page| format!("{base}/api/v1/user/repos?limit=50&page={page}"),
        },
        HostKind::Bitbucket => Shape {
            page_url: |base, page| {
                format!("{base}/2.0/repositories?role=member&pagelen=100&page={page}")
            },
        },
        HostKind::Azure | HostKind::Generic => return None,
    })
}

/// GitHub's API lives on a different host than its pages.
fn api_base(cred: &GitCredential) -> String {
    let host = cred.host();
    match cred.kind {
        HostKind::GitHub if host == "https://github.com" => "https://api.github.com".into(),
        HostKind::GitHub => format!("{host}/api/v3"),
        HostKind::Bitbucket if host == "https://bitbucket.org" => {
            "https://api.bitbucket.org".into()
        }
        _ => host,
    }
}

fn whoami_url(cred: &GitCredential) -> Option<String> {
    let base = api_base(cred);
    Some(match cred.kind {
        HostKind::GitHub => format!("{base}/user"),
        HostKind::GitLab => format!("{base}/api/v4/user"),
        HostKind::Forgejo | HostKind::Gitea => format!("{base}/api/v1/user"),
        HostKind::Bitbucket => format!("{base}/2.0/user"),
        HostKind::Azure | HostKind::Generic => return None,
    })
}

fn request(
    http: &reqwest::Client,
    url: &str,
    cred: &GitCredential,
    token: &str,
) -> reqwest::RequestBuilder {
    let mut r = http.get(url).header("Accept", "application/json");
    if !token.is_empty() {
        r = match cred.kind {
            // Bitbucket app passwords are basic auth; everything else
            // takes a bearer token (GitLab accepts it for PATs too).
            HostKind::Bitbucket => r.basic_auth(&cred.username, Some(token)),
            _ => r.bearer_auth(token),
        };
    }
    if cred.kind == HostKind::GitHub {
        r = r.header("X-GitHub-Api-Version", "2022-11-28");
    }
    r
}

/// The login the token belongs to.
pub async fn whoami(
    http: &reqwest::Client,
    cred: &GitCredential,
    token: &str,
) -> Result<String, String> {
    let url = whoami_url(cred).ok_or_else(|| "this host kind has no API".to_string())?;
    let resp = request(http, &url, cred, token)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("{status}: {}", error_text(&body)));
    }
    parse_whoami(cred.kind, &body).ok_or_else(|| "no login in the answer".into())
}

/// Every repository the credential can see, page after page.
pub async fn list_projects(
    http: &reqwest::Client,
    cred: &GitCredential,
    token: &str,
) -> Result<Vec<Project>, String> {
    let shape = shape(cred.kind).ok_or_else(|| "this host kind lists no projects".to_string())?;
    let base = api_base(cred);
    let mut out = Vec::new();
    for page in 1..=20u32 {
        let url = (shape.page_url)(&base, page);
        let resp = request(http, &url, cred, token)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = resp.status();
        let body: Value = resp.json().await.map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format!("{status}: {}", error_text(&body)));
        }
        let (items, more) = parse_projects(cred.kind, &body);
        let n = items.len();
        out.extend(items);
        if n == 0 || !more {
            break;
        }
    }
    out.sort_by_key(|p| p.full_name.to_lowercase());
    out.dedup_by(|a, b| a.full_name == b.full_name);
    Ok(out)
}

fn error_text(body: &Value) -> String {
    body.get("message")
        .or_else(|| body.get("error"))
        .or_else(|| body.get("error_description"))
        .and_then(Value::as_str)
        .unwrap_or("request refused")
        .to_string()
}

/// The login field of each host's "current user" answer.
pub fn parse_whoami(kind: HostKind, body: &Value) -> Option<String> {
    let key = match kind {
        HostKind::GitHub | HostKind::Forgejo | HostKind::Gitea => "login",
        HostKind::GitLab | HostKind::Bitbucket => "username",
        HostKind::Azure | HostKind::Generic => return None,
    };
    body.get(key).and_then(Value::as_str).map(str::to_string)
}

fn s(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

/// One page of a host's listing as projects, and whether a page may
/// follow (a full page, or Bitbucket's `next`).
pub fn parse_projects(kind: HostKind, body: &Value) -> (Vec<Project>, bool) {
    match kind {
        HostKind::GitHub | HostKind::Forgejo | HostKind::Gitea => {
            let items: Vec<Project> = body
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|r| Project {
                            full_name: s(r, "full_name"),
                            description: s(r, "description"),
                            clone_url: s(r, "clone_url"),
                            default_branch: s(r, "default_branch"),
                            private: r.get("private").and_then(Value::as_bool).unwrap_or(false),
                        })
                        .filter(|p| !p.full_name.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            let full = if kind == HostKind::GitHub { 100 } else { 50 };
            let more = items.len() >= full;
            (items, more)
        }
        HostKind::GitLab => {
            let items: Vec<Project> = body
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|r| Project {
                            full_name: s(r, "path_with_namespace"),
                            description: s(r, "description"),
                            clone_url: s(r, "http_url_to_repo"),
                            default_branch: s(r, "default_branch"),
                            private: r.get("visibility").and_then(Value::as_str) != Some("public"),
                        })
                        .filter(|p| !p.full_name.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            let more = items.len() >= 100;
            (items, more)
        }
        HostKind::Bitbucket => {
            let items: Vec<Project> = body
                .get("values")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .map(|r| {
                            let https = r
                                .pointer("/links/clone")
                                .and_then(Value::as_array)
                                .and_then(|links| {
                                    links.iter().find(|l| {
                                        l.get("name").and_then(Value::as_str) == Some("https")
                                    })
                                })
                                .map(|l| s(l, "href"))
                                .unwrap_or_default();
                            Project {
                                full_name: s(r, "full_name"),
                                description: s(r, "description"),
                                clone_url: https,
                                default_branch: r
                                    .pointer("/mainbranch/name")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string(),
                                private: r
                                    .get("is_private")
                                    .and_then(Value::as_bool)
                                    .unwrap_or(false),
                            }
                        })
                        .filter(|p| !p.full_name.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            let more = body.get("next").and_then(Value::as_str).is_some();
            (items, more)
        }
        HostKind::Azure | HostKind::Generic => (Vec::new(), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn github_and_gitea_list_full_name_and_clone_url() {
        let body = json!([
            {"full_name": "owner/hyper-stack", "description": "d", "clone_url": "https://github.com/owner/hyper-stack.git", "default_branch": "main", "private": true},
            {"full_name": "", "clone_url": "x"}
        ]);
        let (items, more) = parse_projects(HostKind::GitHub, &body);
        assert_eq!(items.len(), 1, "a row without a name is dropped");
        assert_eq!(items[0].full_name, "owner/hyper-stack");
        assert!(items[0].private);
        assert!(!more, "a short page is the last one");
        let (items, _) = parse_projects(HostKind::Forgejo, &body);
        assert_eq!(
            items[0].clone_url,
            "https://github.com/owner/hyper-stack.git"
        );
    }

    #[test]
    fn gitlab_uses_its_own_names() {
        let body = json!([
            {"path_with_namespace": "group/app", "http_url_to_repo": "https://gitlab.com/group/app.git", "default_branch": "main", "visibility": "private", "description": null}
        ]);
        let (items, _) = parse_projects(HostKind::GitLab, &body);
        assert_eq!(items[0].full_name, "group/app");
        assert_eq!(items[0].clone_url, "https://gitlab.com/group/app.git");
        assert!(items[0].private);
        assert_eq!(items[0].description, "");
    }

    #[test]
    fn bitbucket_pages_through_next_and_picks_the_https_clone_link() {
        let body = json!({
            "values": [{
                "full_name": "team/repo",
                "is_private": false,
                "mainbranch": {"name": "master"},
                "links": {"clone": [
                    {"name": "ssh", "href": "git@bitbucket.org:team/repo.git"},
                    {"name": "https", "href": "https://bitbucket.org/team/repo.git"}
                ]}
            }],
            "next": "https://api.bitbucket.org/2.0/repositories?page=2"
        });
        let (items, more) = parse_projects(HostKind::Bitbucket, &body);
        assert_eq!(items[0].clone_url, "https://bitbucket.org/team/repo.git");
        assert_eq!(items[0].default_branch, "master");
        assert!(more);
    }

    #[test]
    fn whoami_reads_login_or_username() {
        assert_eq!(
            parse_whoami(HostKind::GitHub, &json!({"login": "me"})).as_deref(),
            Some("me")
        );
        assert_eq!(
            parse_whoami(HostKind::GitLab, &json!({"username": "me"})).as_deref(),
            Some("me")
        );
        assert_eq!(
            parse_whoami(HostKind::Generic, &json!({"login": "me"})),
            None
        );
    }

    #[test]
    fn github_api_lives_on_its_own_host() {
        let c = GitCredential {
            kind: HostKind::GitHub,
            base_url: "https://github.com".into(),
            ..Default::default()
        };
        assert_eq!(api_base(&c), "https://api.github.com");
        let e = GitCredential {
            kind: HostKind::GitHub,
            base_url: "https://github.example.com/".into(),
            ..Default::default()
        };
        assert_eq!(api_base(&e), "https://github.example.com/api/v3");
        let f = GitCredential {
            kind: HostKind::Forgejo,
            base_url: "https://git.zacx.net".into(),
            ..Default::default()
        };
        assert_eq!(
            whoami_url(&f).as_deref(),
            Some("https://git.zacx.net/api/v1/user")
        );
    }
}
