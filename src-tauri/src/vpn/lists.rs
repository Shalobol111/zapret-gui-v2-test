//! Списки доменов для маршрутизации: готовые наборы (itdoginfo/allow-domains + встроенные),
//! свои списки «всегда через VPN» / «всегда напрямую» и домены из списков zapret.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;

const ITDOG: &str = "https://raw.githubusercontent.com/itdoginfo/allow-domains/main/";
const MAX_AGE: Duration = Duration::from_secs(24 * 3600);

pub struct Preset {
    pub id: &'static str,
    pub title: &'static str,
    pub hint: &'static str,
    sources: &'static [&'static str],
    builtin: &'static [&'static str],
}

/// Наборы «через VPN» (режим «Списки → VPN»).
pub const PRESETS: [Preset; 5] = [
    Preset {
        id: "blocked",
        title: "Заблокировано в России",
        hint: "Около 1200 сайтов, недоступных из России, — список сообщества itdoginfo",
        sources: &["Russia/inside-raw.lst"],
        builtin: &[],
    },
    Preset {
        id: "ai",
        title: "ИИ-сервисы",
        hint: "ChatGPT, Claude, Gemini, Perplexity, Grok и другие",
        sources: &["Services/google_ai.lst"],
        builtin: &[
            "openai.com",
            "chatgpt.com",
            "oaistatic.com",
            "oaiusercontent.com",
            "sora.com",
            "anthropic.com",
            "claude.ai",
            "claude.com",
            "perplexity.ai",
            "grok.com",
            "x.ai",
            "mistral.ai",
            "poe.com",
            "character.ai",
            "midjourney.com",
            "gemini.google.com",
            "aistudio.google.com",
            "notebooklm.google",
        ],
    },
    Preset {
        id: "meta",
        title: "Instagram, Facebook, WhatsApp",
        hint: "Сервисы Meta",
        sources: &["Services/meta.lst"],
        builtin: &[],
    },
    Preset { id: "twitter", title: "X (Twitter)", hint: "", sources: &["Services/twitter.lst"], builtin: &[] },
    Preset { id: "tiktok", title: "TikTok", hint: "", sources: &["Services/tiktok.lst"], builtin: &[] },
];

/// Режим «Всё через VPN, кроме РФ»: что идёт напрямую.
pub const RU_DIRECT: Preset = Preset {
    id: "ru",
    title: "Российские сайты",
    hint: "Домены .ru, .рф, .su и российские сервисы, которые не пускают из-за границы",
    sources: &["Russia/outside-raw.lst"],
    builtin: &[
        "ru",
        "su",
        "xn--p1ai",
        "xn--p1acf",
        "xn--d1acj3b",
        "xn--80adxhks",
        "xn--80asehdb",
        "xn--80aswg",
        "xn--c1avg",
        "vk.com",
        "vk.me",
        "vkuser.net",
        "userapi.com",
        "vk-cdn.net",
        "yandex.com",
        "yandex.net",
        "yastatic.net",
        "yandex.cloud",
        "mycdn.me",
        "sberbank.com",
        "gosuslugi.ru",
    ],
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetInfo {
    pub id: &'static str,
    pub title: &'static str,
    pub hint: &'static str,
    /// Сколько доменов в наборе (с учётом скачанного списка).
    pub count: usize,
}

pub fn dir() -> PathBuf {
    super::dir().join("lists")
}

/// Домены из текста списка: по одному в строке, `#` — комментарий; zapret-овский `^` убирается.
pub fn parse(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim().trim_start_matches('^').to_lowercase())
        .filter(|l| !l.is_empty() && !l.contains(char::is_whitespace) && !l.contains('/'))
        .collect()
}

fn cache_file(source: &str) -> PathBuf {
    dir().join(source.replace('/', "_"))
}

fn fresh(path: &Path) -> bool {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .is_ok_and(|t| SystemTime::now().duration_since(t).unwrap_or_default() < MAX_AGE)
}

/// Скачать списки наборов, если их нет или им больше суток. Ошибки не критичны — остаётся кэш.
pub async fn refresh(presets: &[&Preset], force: bool) -> Vec<String> {
    let _ = fs::create_dir_all(dir());
    let mut errors = Vec::new();
    for src in presets.iter().flat_map(|p| p.sources.iter()) {
        let file = cache_file(src);
        if !force && fresh(&file) {
            continue;
        }
        match crate::net::get_text(&format!("{ITDOG}{src}"), Duration::from_secs(20)).await {
            Ok(text) if !parse(&text).is_empty() => {
                let _ = fs::write(&file, text);
            }
            Ok(_) => errors.push(format!("{src}: пустой список")),
            Err(e) => errors.push(format!("{src}: {e}")),
        }
    }
    errors
}

/// Домены набора: встроенные + скачанные (из кэша).
pub fn domains(p: &Preset) -> Vec<String> {
    let mut out: Vec<String> = p.builtin.iter().map(|s| s.to_string()).collect();
    for src in p.sources {
        if let Ok(text) = fs::read_to_string(cache_file(src)) {
            out.extend(parse(&text));
        }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
pub fn preset(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}

pub fn infos() -> Vec<PresetInfo> {
    PRESETS.iter().map(|p| PresetInfo { id: p.id, title: p.title, hint: p.hint, count: domains(p).len() }).collect()
}

// ── Свои списки ────────────────────────────────────────────────

pub const VPN_FILE: &str = "vpn-domains.txt";
pub const DIRECT_FILE: &str = "direct-domains.txt";
pub const GENERAL_FILE: &str = "list-vpn-general.txt";

fn initial_general(extra: impl IntoIterator<Item = String>) -> String {
    let mut domains: Vec<String> = parse(include_str!("../../data/list-vpn-general.txt"))
        .into_iter()
        .chain(extra)
        .map(|s| s.trim_start_matches('.').to_lowercase())
        .filter(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'_')))
        .collect();
    domains.sort();
    domains.dedup();
    format!("# Основной список VPN: заблокированные сайты, ИИ, Meta, X, TikTok.\n# Источник начального списка: https://github.com/itdoginfo/allow-domains\n# По одному домену на строку. Поддомены включаются автоматически.\n# Ваши изменения не перезаписываются при запуске приложения.\n\n{}\n", domains.join("\n"))
}

/// Однократное создание из встроенного списка и кэша всех пяти наборов.
/// Даже намеренно пустой существующий файл не заполняем повторно.
pub fn ensure_general() -> Result<(), String> {
    let path = custom_path(GENERAL_FILE);
    if path.try_exists().map_err(|e| e.to_string())? {
        return Ok(());
    }
    fs::create_dir_all(super::dir()).map_err(|e| e.to_string())?;
    let text = initial_general(PRESETS.iter().flat_map(domains));
    match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => {
            if let Err(e) = file.write_all(text.as_bytes()) {
                drop(file);
                let _ = fs::remove_file(&path);
                return Err(format!("Не удалось создать {GENERAL_FILE}: {e}"));
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(format!("Не удалось создать {GENERAL_FILE}: {e}")),
    }
}

pub fn custom_path(name: &str) -> PathBuf {
    super::dir().join(name)
}

pub fn ensure_custom() {
    let _ = fs::create_dir_all(super::dir());
    for (name, head) in [
        (VPN_FILE, "# Всегда через VPN — по домену на строку, поддомены подхватываются\n"),
        (DIRECT_FILE, "# Всегда напрямую (мимо VPN) — по домену на строку\n"),
    ] {
        let p = custom_path(name);
        if !p.exists() {
            let _ = fs::write(p, head);
        }
    }
}

pub fn custom(name: &str) -> Vec<String> {
    fs::read_to_string(custom_path(name)).map(|t| parse(&t)).unwrap_or_default()
}

pub fn matches(domains: &[String], domain: &str) -> bool {
    domains.iter().any(|s| {
        let suffix = s.trim_start_matches('.');
        domain == suffix || domain.strip_suffix(suffix).is_some_and(|prefix| prefix.ends_with('.'))
    })
}

/// Явное добавление в личный список имеет приоритет над общим списком и списками zapret.
pub fn add_site(domain: &str) -> Result<bool, String> {
    if matches(&custom(DIRECT_FILE), domain) {
        return Err("Домен входит в «Всегда напрямую». Сначала измените этот список в разделе «Списки → VPN»".into());
    }
    let data = crate::zapret::userlists::read(&super::dir(), "vpn_domains")?;
    if matches(&parse(&data.text), domain) {
        return Ok(false);
    }
    let text = format!(
        "{}{}{}\n",
        data.text,
        if data.text.is_empty() || data.text.ends_with('\n') { "" } else { "\n" },
        domain
    );
    crate::zapret::userlists::save(&super::dir(), "vpn_domains", &text, &data.hash, false).map_err(|e| match e {
        crate::zapret::userlists::SaveError::Conflict => "VPN-список изменился — повторите добавление".into(),
        crate::zapret::userlists::SaveError::Other(e) => e,
    })?;
    Ok(true)
}

/// Домены, которые обходит zapret (его списки) — им место напрямую, через zapret.
pub fn zapret_domains(root: Option<&Path>) -> Vec<String> {
    let Some(root) = root else { return vec![] };
    let mut out = Vec::new();
    for f in ["list-general.txt", "list-general-user.txt", "list-google.txt"] {
        if let Ok(t) = fs::read_to_string(root.join("lists").join(f)) {
            out.extend(parse(&t).into_iter().filter(|d| d.contains('.')));
        }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_domains_at_label_boundaries() {
        let domains = vec!["example.com".into(), ".xn--p1ai".into()];
        for domain in ["example.com", "a.example.com", "a.b.example.com", "site.xn--p1ai"] {
            assert!(matches(&domains, domain));
        }
        for domain in ["badexample.com", "example.com.bad", "example.org"] {
            assert!(!matches(&domains, domain));
        }
    }

    #[test]
    #[ignore]
    fn isolated_add_site_preserves_direct_priority_and_deduplicates() {
        assert!(std::env::var_os("ZAPRETGUI_DATA_DIR").is_some());
        ensure_custom();
        fs::write(custom_path(DIRECT_FILE), "direct.example\n").unwrap();
        fs::write(custom_path(VPN_FILE), "# Keep this comment\nexisting.example\n").unwrap();
        assert!(add_site("a.direct.example").is_err());
        assert!(!add_site("a.existing.example").unwrap());
        assert!(add_site("new.example").unwrap());
        let saved = fs::read_to_string(custom_path(VPN_FILE)).unwrap();
        assert!(saved.contains("# Keep this comment"));
        assert!(saved.contains("existing.example"));
        assert!(saved.contains("new.example"));
        assert!(!add_site("new.example").unwrap());
    }

    #[test]
    fn general_seed_covers_every_category_and_deduplicates() {
        let text = initial_general(vec![".CHATGPT.COM".into(), "only-in-cache.example".into()]);
        let entries = parse(&text);
        assert!(entries.len() > 1100);
        for domain in [
            "4pda.to",
            "chatgpt.com",
            "claude.ai",
            "facebook.com",
            "instagram.com",
            "whatsapp.com",
            "twitter.com",
            "tiktok.com",
            "only-in-cache.example",
        ] {
            assert!(entries.iter().any(|d| d == domain), "Missing {domain}");
        }
        assert_eq!(entries.iter().filter(|d| d.as_str() == "chatgpt.com").count(), 1);
    }

    #[test]
    #[ignore]
    fn isolated_general_preserves_edits_and_empty_file() {
        assert!(std::env::var_os("ZAPRETGUI_DATA_DIR").is_some());
        ensure_general().unwrap();
        assert!(custom(GENERAL_FILE).len() > 1100);
        fs::write(custom_path(GENERAL_FILE), "my.example\n").unwrap();
        ensure_general().unwrap();
        assert_eq!(custom(GENERAL_FILE), vec!["my.example"]);
        fs::write(custom_path(GENERAL_FILE), "").unwrap();
        ensure_general().unwrap();
        assert!(fs::read(custom_path(GENERAL_FILE)).unwrap().is_empty());
    }

    #[test]
    fn parses_list_lines() {
        let d = parse("# заголовок\nYouTube.com\n\n^exact.com\n.ua\nsome thing\nhttp://x/y\nfoo.org # c\n");
        assert_eq!(d, vec!["youtube.com", "exact.com", ".ua", "foo.org"]);
    }

    #[test]
    fn builtin_ru_direct_has_cyrillic_tld() {
        assert!(RU_DIRECT.builtin.contains(&"xn--p1ai")); // .рф
        assert!(preset("ai").unwrap().builtin.contains(&"chatgpt.com"));
        assert!(preset("nope").is_none());
    }
}
