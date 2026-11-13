//! One masking function for every piece of server JSON the activity shows
//! or hands on - a realm export, an admin event's representation, More
//! Settings (docs/tools/idm.md §6.5). The assistant only ever sees the
//! masked form.

use serde_json::Value;

/// What a masked value reads as.
pub const MASK: &str = "**********";

/// Keys whose value is a secret wherever they appear.
const SECRET_KEYS: &[&str] = &[
    "secret",
    "clientsecret",
    "password",
    "bindcredential",
    "privatekey",
    "private_key",
    "secretdata",
    "credentialdata",
    "client_secret",
    "access_token",
    "refresh_token",
    "id_token",
];

fn is_secret_key(k: &str) -> bool {
    let k = k.to_ascii_lowercase();
    SECRET_KEYS.contains(&k.as_str())
}

/// Mask every secret in `v`, in place: the keys above anywhere, `value`
/// inside a credential (an object with a `type` beside it, as a user's
/// `credentials` carry), and the private half of a key provider's
/// `config` (`privateKey`, `secret`). A key whose value is already the
/// server's own mask is left as it is.
pub fn mask(v: &mut Value) {
    match v {
        Value::Object(map) => {
            let credential = map.contains_key("type")
                && (map.contains_key("value") || map.contains_key("secretData"));
            for (k, child) in map.iter_mut() {
                let secret = is_secret_key(k) || (credential && k == "value");
                if secret && !child.is_null() {
                    match child {
                        // Keycloak's config maps hold lists of strings.
                        Value::Array(items) => {
                            for item in items.iter_mut() {
                                if !item.is_null() {
                                    *item = Value::String(MASK.into());
                                }
                            }
                        }
                        Value::String(s) if s.is_empty() => {}
                        _ => *child = Value::String(MASK.into()),
                    }
                } else {
                    mask(child);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(mask),
        _ => {}
    }
}

/// `text` masked when it is JSON; otherwise `None`, for the caller to
/// withhold rather than show something it could not check.
pub fn mask_text(text: &str) -> Option<String> {
    let mut v: Value = serde_json::from_str(text).ok()?;
    mask(&mut v);
    serde_json::to_string_pretty(&v).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_secret_key_is_masked_in_nested_json() {
        let mut v = json!({
            "realm": "hs-demo",
            "clients": [
                {"clientId": "orders-api", "secret": "plain-secret-1", "attributes": {"x": "y"}},
                {"clientId": "web-app", "secret": "**********"}
            ],
            "identityProviders": [{"alias": "github", "config": {"clientSecret": "gh-secret-2"}}],
            "components": {"org.keycloak.keys.KeyProvider": [
                {"name": "rsa", "config": {"privateKey": ["-----BEGIN PRIVATE KEY-----abc"], "certificate": ["MIIC"]}}
            ]},
            "userFederation": [{"config": {"bindCredential": ["ldap-pw-3"]}}],
            "users": [{"username": "a", "credentials": [{"type": "password", "value": "pw-4"}]}],
            "smtpServer": {"password": "smtp-pw-5", "user": "mailer"}
        });
        mask(&mut v);
        let text = v.to_string();
        for leaked in [
            "plain-secret-1",
            "gh-secret-2",
            "BEGIN PRIVATE KEY",
            "ldap-pw-3",
            "pw-4",
            "smtp-pw-5",
        ] {
            assert!(!text.contains(leaked), "{leaked} survived: {text}");
        }
        // What is not a secret stays readable.
        assert!(text.contains("orders-api"));
        assert!(
            text.contains("MIIC"),
            "a public certificate is not a secret"
        );
        assert!(text.contains("mailer"));
        assert_eq!(v["clients"][1]["secret"], MASK);
    }

    #[test]
    fn a_value_outside_a_credential_is_not_masked() {
        let mut v = json!({"attributes": {"value": "shown"}, "type": "x"});
        mask(&mut v);
        assert_eq!(v["attributes"]["value"], "shown");
    }

    #[test]
    fn text_that_is_not_json_is_withheld() {
        assert_eq!(mask_text("not json"), None);
        let out = mask_text(r#"{"password":"p"}"#).expect("json");
        assert!(!out.contains("\"p\""));
    }
}
