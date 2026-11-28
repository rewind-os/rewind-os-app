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

/// Picks the language from `REWIND_OS_LANG`, then the system locale. Only the first call has
/// an effect; later calls keep the language already chosen.
pub fn init(explicit: Option<&str>) {
    ACTIVE.get_or_init(|| {
        let requested = explicit
            .map(str::to_owned)
            .or_else(|| std::env::var("REWIND_OS_LANG").ok())
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

/// Translates `key`, the English text. Returns `key` when there is no translation, in the words
/// of this system (see [`in_system_words`]).
pub fn tr(key: &'static str) -> &'static str {
    if let Some(text) = catalog().map.get(key) {
        return text;
    }
    if cfg!(windows) || !key.to_ascii_lowercase().contains("recycle bin") {
        return key;
    }
    // English keys name Windows' recycle bin; macOS and Linux call it the Trash. Each changed
    // text is made once and kept for the life of the program (a few dozen at most).
    static CHANGED: OnceLock<parking_lot::Mutex<HashMap<&'static str, &'static str>>> =
        OnceLock::new();
    let mut changed = CHANGED.get_or_init(Default::default).lock();
    changed
        .entry(key)
        .or_insert_with(|| Box::leak(in_system_words(key, false).into_boxed_str()))
}

/// English text in the words of the system it runs on: on Windows as written, elsewhere the
/// recycle bin is the Trash.
pub fn in_system_words(text: &str, windows: bool) -> String {
    if windows {
        return text.to_owned();
    }
    text.replace("the recycle bin", "the Trash")
        .replace("to recycle bin", "to Trash")
        .replace("Recycle bin", "Trash")
        .replace("recycle bin", "Trash")
}

/// Translates text that is only known at run time (a reason an engine error carries). Returns
/// the text unchanged when there is no translation.
pub fn tr_text(text: &str) -> String {
    catalog()
        .map
        .get(text)
        .map_or_else(|| in_system_words(text, cfg!(windows)), |t| (*t).to_owned())
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

#[cfg(test)]
mod system_words {
    use super::in_system_words;

    #[test]
    fn the_recycle_bin_is_the_trash_outside_windows() {
        let text = "Move to recycle bin: 3 items will be moved to the recycle bin.";
        assert_eq!(in_system_words(text, true), text);
        assert_eq!(
            in_system_words(text, false),
            "Move to Trash: 3 items will be moved to the Trash."
        );
        assert_eq!(in_system_words("Recycle bin", false), "Trash");
    }
}
