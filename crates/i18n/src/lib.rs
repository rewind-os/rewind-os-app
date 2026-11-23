//! Translation lookup. Every user-facing string is written in English and that English text is
//! the key. Catalogs live in `locales/<lang>.tsv` as `English<TAB>Translation` lines. A missing
//! translation shows the English text.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Languages with a bundled catalog. English needs none.
const CATALOGS: &[(&str, &str)] = &[("es", include_str!("../locales/es.tsv"))];

struct Catalog {
    lang: String,
    map: HashMap<&'static str, &'static str>,
}

static ACTIVE: OnceLock<Catalog> = OnceLock::new();

fn parse(source: &'static str) -> HashMap<&'static str, &'static str> {
    source
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once('\t'))
        .collect()
}

/// Picks the language from `REWINDOWS_LANG`, then the system locale. Only the first call has
/// an effect; later calls keep the language already chosen.
pub fn init(explicit: Option<&str>) {
    ACTIVE.get_or_init(|| {
        let requested = explicit
            .map(str::to_owned)
            .or_else(|| std::env::var("REWINDOWS_LANG").ok())
            .or_else(sys_locale::get_locale)
            .unwrap_or_else(|| "en".to_owned());
        let lang: String = requested
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect::<String>()
            .to_ascii_lowercase();
        let map = CATALOGS
            .iter()
            .find(|(code, _)| *code == lang)
            .map(|(_, source)| parse(source))
            .unwrap_or_default();
        Catalog { lang, map }
    });
}

fn catalog() -> &'static Catalog {
    init(None);
    // `init` always fills the cell, so this lookup cannot miss; fall back to an empty catalog.
    ACTIVE.get_or_init(|| Catalog {
        lang: "en".to_owned(),
        map: HashMap::new(),
    })
}

/// The active language code, for example `es` or `en`.
pub fn language() -> &'static str {
    &catalog().lang
}

/// Translates `key`, the English text. Returns `key` when there is no translation.
pub fn tr(key: &'static str) -> &'static str {
    catalog().map.get(key).copied().unwrap_or(key)
}

/// Translates `key` and fills `{name}` placeholders from `args`.
pub fn trf(key: &'static str, args: &[(&str, &str)]) -> String {
    let mut text = tr(key).to_owned();
    for (name, value) in args {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_catalog_line_has_a_translation_and_keeps_placeholders() {
        for (code, source) in CATALOGS {
            for line in source
                .lines()
                .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            {
                let (key, value) = line
                    .split_once('\t')
                    .unwrap_or_else(|| panic!("{code}: line without a tab: {line:?}"));
                assert!(
                    !value.trim().is_empty(),
                    "{code}: empty translation for {key:?}"
                );
                for part in key.split('{').skip(1) {
                    if let Some(name) = part.split('}').next() {
                        let placeholder = format!("{{{name}}}");
                        assert!(
                            value.contains(&placeholder),
                            "{code}: {key:?} loses {placeholder}"
                        );
                    }
                }
            }
        }
    }
}
