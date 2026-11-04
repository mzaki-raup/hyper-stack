//! A fal.ai key goes into the vault and the provider keeps only the
//! `secret:<ulid>` reference: after the save, no file under the data
//! folder holds the key in clear (the rule in `.claude/rules/security.md`).

use std::path::Path;

use hs_core::ai::Provider;
use hs_core::secrets::{KdfParams, SecretStore, VaultStore};
use hs_core::settings::{AiProvider, Settings};
use secrecy::SecretString;

const KEY: &str = "fal-key-7f3a9c1e-ULTRA-SECRET-value-0042";

fn files_under(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let p = entry.unwrap().path();
        if p.is_dir() {
            files_under(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[test]
fn the_key_lives_in_the_vault_and_nowhere_else_in_the_data_folder() {
    let dir = tempfile::tempdir().unwrap();
    let vault = VaultStore::new(&dir.path().join("vault.bin"), KdfParams::FAST);
    vault
        .initialize(&SecretString::from("master".to_string()))
        .unwrap();
    let r = vault
        .create("AI fal.ai: api key", &SecretString::from(KEY.to_string()))
        .unwrap();

    let mut settings = Settings::default();
    settings.ai.providers.push(AiProvider {
        id: "p1".into(),
        name: "fal.ai".into(),
        kind: Provider::Fal,
        api_key: r.to_string_ref(),
        fal: hs_core::ai_fal::FalModels {
            generate: "fal-ai/flux/dev".into(),
            describe: "fal-ai/florence-2-large/caption".into(),
            ..Default::default()
        },
        ..Default::default()
    });
    let settings_file = dir.path().join("settings.toml");
    settings.save(&settings_file).unwrap();

    let text = std::fs::read_to_string(&settings_file).unwrap();
    assert!(text.contains("kind = \"fal\""), "{text}");
    assert!(text.contains("api_key = \"secret:"), "{text}");
    assert!(text.contains("generate = \"fal-ai/flux/dev\""), "{text}");

    let mut files = Vec::new();
    files_under(dir.path(), &mut files);
    assert!(files.len() >= 2, "{files:?}");
    for f in &files {
        let bytes = std::fs::read(f).unwrap();
        assert!(
            !bytes.windows(KEY.len()).any(|w| w == KEY.as_bytes()),
            "{} holds the key in clear",
            f.display()
        );
    }

    // The reference reads back through the unlocked vault, and a locked
    // vault refuses rather than answering empty.
    let back = vault.get(&r).unwrap();
    assert_eq!(secrecy::ExposeSecret::expose_secret(&back), KEY);
    vault.lock();
    assert!(vault.get(&r).is_err());

    // The provider reads back with its per-purpose choices intact.
    let loaded = Settings::load(&settings_file).unwrap();
    let p = &loaded.ai.providers[0];
    assert_eq!(p.kind, Provider::Fal);
    assert_eq!(p.fal.generate, "fal-ai/flux/dev");
    assert_eq!(p.fal.describe, "fal-ai/florence-2-large/caption");
    assert!(p.fal.edit.is_empty());
}
