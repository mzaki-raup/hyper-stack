//! The credential every newer activity's connection carries
//! (`.claude/rules/security.md`, "Credentials for the newer activities";
//! ADR-028): one of `password`, `token` or `api-key`, the secret a
//! `secret:<ulid>` in the vault.
//!
//! The fields live flat in the activity's own table of the connection
//! profile (`auth`, `user`, `secret`, `header`), beside its other fields,
//! so a profile file reads as one block. A tool reads them with
//! [`Credential::from_table`], has the form's save move the typed secret
//! into the vault through [`Credential::secret_fields`], and when it
//! connects, resolves them into an [`Auth`] and applies that to each
//! request in the scheme its service expects ([`TokenAs`]).

use secrecy::ExposeSecret;

use crate::secrets::{SecretRef, SecretStore};

/// What a credential can be. `none` is the fourth answer: a server on a
/// developer's machine often has no login.
pub const AUTH_KINDS: &[&str] = &["none", "password", "token", "api-key"];

/// The stored form, as the connection form edits it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Credential {
    /// none | password | token | api-key
    pub auth: String,
    /// The login (password, and the user a Jenkins token belongs to), or
    /// the key's id where a service pairs one with its key.
    pub user: String,
    /// `secret:<id>`, `{{var}}`, or typed text until the form saves it.
    pub secret: String,
    /// The header (or query parameter) an API key travels in.
    pub header: String,
}

/// Why a credential could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    VaultLocked,
    /// `{{names}}` the environment does not define.
    MissingVariables(Vec<String>),
    Secret(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::VaultLocked => f.write_str("the vault is locked"),
            AuthError::MissingVariables(v) => write!(f, "undefined variables: {}", v.join(", ")),
            AuthError::Secret(e) => f.write_str(e),
        }
    }
}

/// A credential, resolved: what goes on the wire.
#[derive(Clone, PartialEq, Eq)]
pub enum Auth {
    None,
    Password {
        user: String,
        password: String,
    },
    /// `user` is empty unless the service ties a token to a login
    /// (Jenkins).
    Token {
        user: String,
        token: String,
    },
    ApiKey {
        header: String,
        key: String,
    },
}

/// Never prints a secret, so a config holding one can be logged.
impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Auth::None => f.write_str("None"),
            Auth::Password { user, .. } => write!(f, "Password({user})"),
            Auth::Token { user, .. } if user.is_empty() => f.write_str("Token"),
            Auth::Token { user, .. } => write!(f, "Token({user})"),
            Auth::ApiKey { header, .. } => write!(f, "ApiKey({header})"),
        }
    }
}

/// How a service wants a token sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenAs {
    /// `Authorization: Bearer <token>` - most services.
    Bearer,
    /// `Authorization: token <token>` - Gitea and Forgejo.
    TokenPrefix,
    /// HTTP basic with the credential's user and the token as the
    /// password - Jenkins.
    Basic,
}

impl Credential {
    /// Read the credential's keys from a kind's table; a missing or
    /// unknown `auth` reads as `none`.
    pub fn from_table(t: &toml::Table) -> Self {
        let s = |k: &str| t.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let auth = s("auth");
        Self {
            auth: if AUTH_KINDS.contains(&auth.as_str()) {
                auth
            } else {
                "none".into()
            },
            user: s("user"),
            secret: s("secret"),
            header: s("header"),
        }
    }

    /// Write the credential's keys into a kind's table.
    pub fn write_into(&self, t: &mut toml::Table) {
        for (k, v) in [
            ("auth", self.auth.as_str()),
            ("user", self.user.as_str()),
            ("secret", self.secret.as_str()),
            ("header", self.header.as_str()),
        ] {
            t.insert(k.into(), toml::Value::String(v.to_string()));
        }
    }

    /// Narrow to the kinds a service accepts: one it cannot use reads as
    /// `none` rather than sending something meaningless.
    pub fn allowed(mut self, kinds: &[&str]) -> Self {
        if !kinds.contains(&self.auth.as_str()) {
            self.auth = "none".into();
        }
        self
    }

    /// The secret, with the label the vault files it under, for the
    /// form's save to move into the vault (`store_typed_secrets`).
    pub fn secret_fields(&mut self) -> [(&'static str, &mut String); 1] {
        let label = match self.auth.as_str() {
            "token" => "token",
            "api-key" => "API key",
            _ => "password",
        };
        [(label, &mut self.secret)]
    }

    /// Resolve variables and the secret. `lookup` substitutes `{{names}}`
    /// from the profile's environment and returns the names it could not
    /// find.
    pub fn resolve(
        &self,
        secrets: &dyn SecretStore,
        lookup: &dyn Fn(&str) -> Result<String, Vec<String>>,
    ) -> Result<Auth, AuthError> {
        if self.auth == "none" || self.auth.is_empty() {
            return Ok(Auth::None);
        }
        let mut missing = Vec::new();
        let mut sub = |v: &str| match lookup(v) {
            Ok(s) => s,
            Err(m) => {
                missing.extend(m);
                v.to_string()
            }
        };
        let user = sub(&self.user);
        let header = sub(&self.header);
        let raw = sub(&self.secret);
        if !missing.is_empty() {
            missing.sort();
            missing.dedup();
            return Err(AuthError::MissingVariables(missing));
        }
        let secret = match SecretRef::parse(&raw) {
            Some(r) => {
                if secrets.is_locked() {
                    return Err(AuthError::VaultLocked);
                }
                secrets
                    .get(&r)
                    .map_err(|e| AuthError::Secret(e.to_string()))?
                    .expose_secret()
                    .to_string()
            }
            None => raw,
        };
        Ok(match self.auth.as_str() {
            "token" => Auth::Token {
                user,
                token: secret,
            },
            "api-key" => Auth::ApiKey {
                header,
                key: secret,
            },
            _ => Auth::Password {
                user,
                password: secret,
            },
        })
    }
}

impl Auth {
    /// Put the credential on a request.
    pub fn apply(&self, rb: reqwest::RequestBuilder, token_as: TokenAs) -> reqwest::RequestBuilder {
        match self {
            Auth::None => rb,
            Auth::Password { user, password } => rb.basic_auth(user, Some(password)),
            Auth::Token { user, token } => match token_as {
                TokenAs::Bearer => rb.bearer_auth(token),
                TokenAs::TokenPrefix => rb.header("Authorization", format!("token {token}")),
                TokenAs::Basic => rb.basic_auth(user, Some(token)),
            },
            Auth::ApiKey { header, key } => rb.header(header.as_str(), key.as_str()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Locked;
    impl SecretStore for Locked {
        fn is_locked(&self) -> bool {
            true
        }
        fn exists(&self) -> bool {
            true
        }
        fn unlock(&self, _: &secrecy::SecretString) -> crate::secrets::Result<()> {
            Ok(())
        }
        fn lock(&self) {}
        fn get(&self, _: &SecretRef) -> crate::secrets::Result<secrecy::SecretString> {
            Err(crate::secrets::SecretError::Locked)
        }
        fn set(
            &self,
            _: &SecretRef,
            _: &str,
            _: &secrecy::SecretString,
        ) -> crate::secrets::Result<()> {
            Err(crate::secrets::SecretError::Locked)
        }
        fn delete(&self, _: &SecretRef) -> crate::secrets::Result<()> {
            Ok(())
        }
        fn list(&self) -> crate::secrets::Result<Vec<(SecretRef, String)>> {
            Ok(Vec::new())
        }
    }

    fn same(v: &str) -> Result<String, Vec<String>> {
        Ok(v.to_string())
    }

    #[test]
    fn a_credential_round_trips_through_its_table() {
        let c = Credential {
            auth: "api-key".into(),
            user: String::new(),
            secret: "secret:01J0000000000000000000000".into(),
            header: "PRIVATE-TOKEN".into(),
        };
        let mut t = toml::Table::new();
        c.write_into(&mut t);
        assert_eq!(Credential::from_table(&t), c);
        assert_eq!(Credential::from_table(&toml::Table::new()).auth, "none");
    }

    #[test]
    fn a_kind_a_service_cannot_use_reads_as_none() {
        let c = Credential {
            auth: "api-key".into(),
            ..Default::default()
        };
        assert_eq!(c.allowed(&["none", "token"]).auth, "none");
    }

    #[test]
    fn a_vault_secret_needs_the_vault_and_none_needs_nothing() {
        let c = Credential {
            auth: "token".into(),
            secret: "secret:01J0000000000000000000000".into(),
            ..Default::default()
        };
        assert_eq!(c.resolve(&Locked, &same), Err(AuthError::VaultLocked));
        assert_eq!(
            Credential::default().resolve(&Locked, &same),
            Ok(Auth::None)
        );
    }

    #[test]
    fn variables_are_substituted_and_missing_ones_named() {
        let c = Credential {
            auth: "password".into(),
            user: "{{USER}}".into(),
            secret: "{{PASS}}".into(),
            ..Default::default()
        };
        let env = |v: &str| match v {
            "{{USER}}" => Ok("ada".to_string()),
            "{{PASS}}" => Err(vec!["PASS".to_string()]),
            other => Ok(other.to_string()),
        };
        assert_eq!(
            c.resolve(&Locked, &env),
            Err(AuthError::MissingVariables(vec!["PASS".into()]))
        );
    }

    #[test]
    fn each_scheme_puts_the_secret_where_its_service_reads_it() {
        let client = reqwest::Client::new();
        let header = |auth: &Auth, how: TokenAs| {
            let req = auth
                .apply(client.get("http://h/"), how)
                .build()
                .expect("request");
            req.headers()
                .iter()
                .map(|(k, v)| format!("{k}: {}", v.to_str().unwrap_or("")))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let token = Auth::Token {
            user: "ada".into(),
            token: "t0k".into(),
        };
        assert_eq!(header(&token, TokenAs::Bearer), "authorization: Bearer t0k");
        assert_eq!(
            header(&token, TokenAs::TokenPrefix),
            "authorization: token t0k"
        );
        // ada:t0k
        assert_eq!(
            header(&token, TokenAs::Basic),
            "authorization: Basic YWRhOnQwaw=="
        );
        let key = Auth::ApiKey {
            header: "PRIVATE-TOKEN".into(),
            key: "k".into(),
        };
        assert_eq!(header(&key, TokenAs::Bearer), "private-token: k");
        // The debug form never carries a secret.
        assert!(!format!("{token:?}{key:?}").contains("t0k"));
    }
}
