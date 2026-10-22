//! Fluent localisation. Locales are embedded from `locales/*.ftl`; `en` is the
//! fallback for every missing message.

use std::collections::HashMap;

use fluent_bundle::FluentArgs;
use fluent_bundle::FluentResource;
use fluent_bundle::concurrent::FluentBundle;
use unic_langid::LanguageIdentifier;

#[derive(rust_embed::Embed)]
#[folder = "../../locales/"]
struct Locales;

pub struct I18n {
    bundle: FluentBundle<FluentResource>,
    fallback: FluentBundle<FluentResource>,
    pub locale: String,
}

impl I18n {
    pub fn available() -> Vec<String> {
        let mut v: Vec<String> = Locales::iter()
            .filter_map(|f| f.strip_suffix(".ftl").map(str::to_string))
            .collect();
        v.sort();
        v
    }

    /// Load `requested` (empty = OS locale), falling back to `en`.
    pub fn load(requested: &str) -> Self {
        let wanted = if requested.is_empty() {
            sys_locale::get_locale().unwrap_or_else(|| "en".into())
        } else {
            requested.to_string()
        };
        let available = Self::available();
        let pick = available
            .iter()
            .find(|l| l.as_str() == wanted)
            .or_else(|| available.iter().find(|l| wanted.starts_with(l.as_str())))
            .or_else(|| {
                wanted
                    .split(['-', '_'])
                    .next()
                    .and_then(|p| available.iter().find(|l| l.as_str() == p))
            })
            .cloned()
            .unwrap_or_else(|| "en".to_string());
        Self {
            bundle: Self::bundle_for(&pick),
            fallback: Self::bundle_for("en"),
            locale: pick,
        }
    }

    fn bundle_for(locale: &str) -> FluentBundle<FluentResource> {
        let langid: LanguageIdentifier = locale.parse().unwrap_or_else(|_| "en".parse().unwrap());
        let mut bundle = FluentBundle::new_concurrent(vec![langid]);
        bundle.set_use_isolating(false);
        if let Some(file) = Locales::get(&format!("{locale}.ftl")) {
            let text = String::from_utf8_lossy(&file.data).to_string();
            match FluentResource::try_new(text) {
                Ok(res) => {
                    let _ = bundle.add_resource(res);
                }
                Err((res, errors)) => {
                    tracing::warn!("locale {locale} has {} syntax errors", errors.len());
                    let _ = bundle.add_resource(res);
                }
            }
        }
        bundle
    }

    fn format(
        bundle: &FluentBundle<FluentResource>,
        key: &str,
        args: Option<&FluentArgs>,
    ) -> Option<String> {
        let msg = bundle.get_message(key)?;
        let pattern = msg.value()?;
        let mut errors = Vec::new();
        let s = bundle.format_pattern(pattern, args, &mut errors);
        Some(s.into_owned())
    }

    pub fn t(&self, key: &str) -> String {
        Self::format(&self.bundle, key, None)
            .or_else(|| Self::format(&self.fallback, key, None))
            .unwrap_or_else(|| key.to_string())
    }

    pub fn t_args(&self, key: &str, args: &[(&str, &str)]) -> String {
        let mut fa = FluentArgs::new();
        for (k, v) in args {
            // Integers go in as numbers so plural selectors (`[one]`) match;
            // only canonical ones, so "007" or "0.12.1" stay text.
            match v.parse::<i64>() {
                Ok(n) if n.to_string() == *v => fa.set(*k, n),
                _ => fa.set(*k, *v),
            }
        }
        Self::format(&self.bundle, key, Some(&fa))
            .or_else(|| Self::format(&self.fallback, key, Some(&fa)))
            .unwrap_or_else(|| key.to_string())
    }

    /// Keys present in `en` but missing from `locale` (for the sync check).
    pub fn missing_keys(locale: &str) -> Vec<String> {
        let en = Self::bundle_for("en");
        let other = Self::bundle_for(locale);
        let mut missing = Vec::new();
        if let Some(file) = Locales::get("en.ftl") {
            let text = String::from_utf8_lossy(&file.data);
            for line in text.lines() {
                if let Some((k, _)) = line.split_once('=') {
                    let k = k.trim();
                    if !k.is_empty()
                        && !k.starts_with('#')
                        && !k.starts_with('-')
                        && !line.starts_with(' ')
                        && en.has_message(k)
                        && !other.has_message(k)
                    {
                        missing.push(k.to_string());
                    }
                }
            }
        }
        missing
    }

    pub fn keys() -> HashMap<String, usize> {
        let mut m = HashMap::new();
        for locale in Self::available() {
            if let Some(file) = Locales::get(&format!("{locale}.ftl")) {
                let n = String::from_utf8_lossy(&file.data)
                    .lines()
                    .filter(|l| !l.starts_with(' ') && !l.starts_with('#') && l.contains('='))
                    .count();
                m.insert(locale, n);
            }
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A message with a syntax error is dropped by the parser and its
    /// key then shows raw in the UI; `locale en has 1 syntax errors` in
    /// the log was the only sign (a `{ $name }` eaten by a shell heredoc).
    #[test]
    fn the_bundled_locale_parses_without_junk() {
        let file = Locales::get("en.ftl").expect("en.ftl is embedded");
        let text = String::from_utf8_lossy(&file.data).to_string();
        if let Err((_, errors)) = FluentResource::try_new(text.clone()) {
            let described: Vec<String> = errors
                .iter()
                .map(|e| {
                    let line = text[..e.pos.start.min(text.len())].matches('\n').count() + 1;
                    format!("line {line}: {:?}", e.kind)
                })
                .collect();
            panic!("en.ftl has syntax errors: {described:?}");
        }
    }

    #[test]
    fn english_loads_and_unknown_keys_echo() {
        let i = I18n::load("en");
        assert_eq!(i.locale, "en");
        assert_eq!(i.t("app-name"), "HyperStack");
        assert_eq!(i.t("no-such-key"), "no-such-key");
        assert!(i.t_args("tab-count", &[("n", "3")]).contains('3'));
    }

    #[test]
    fn integer_arguments_select_plural_forms() {
        let i = I18n::load("en");
        let strip = |s: String| s.replace(['\u{2068}', '\u{2069}'], "");
        assert_eq!(strip(i.t_args("tab-count", &[("n", "1")])), "1 tab");
        assert_eq!(strip(i.t_args("tab-count", &[("n", "2")])), "2 tabs");
        assert_eq!(
            strip(i.t_args("db-affected", &[("rows", "1"), ("ms", "4")])),
            "1 row affected in 4 ms."
        );
        // Not canonical integers: passed through as text.
        assert_eq!(
            strip(i.t_args("db-exported", &[("path", "007")])),
            "Exported to 007"
        );
    }

    #[test]
    fn unknown_locale_falls_back_to_english() {
        let i = I18n::load("xx-YY");
        assert_eq!(i.locale, "en");
    }
}
