use crate::{
    state::AppState,
    zapret::{
        diagnostics::{Item, Level},
        site::{Action, Report},
    },
};
use std::path::Path;

fn item(id: &'static str, title: &'static str, level: Level, value: &str, detail: String) -> Item {
    Item { id, title, level, value: value.into(), detail: Some(detail), fix: None, fix_label: None, fix_safe: false }
}

pub fn diagnostics(st: &AppState) -> Vec<Item> {
    let store = super::load();
    let s = super::status(&mut st.vpn.lock().unwrap(), &store);
    let settings = st.settings.lock().unwrap().get().clone();
    let mut rows = vec![item("vpn-core", "Ядра VPN", if s.installed && s.xray_installed { Level::Ok } else { Level::Info },
        if s.installed && s.xray_installed { "установлены" } else { "нужна настройка" },
        "Раздел VPN → скачать ядра, добавить подписку или ключ. Доступность сервера проверяется отдельной HTTPS-проверкой.".into())];
    rows.push(item(
        "vpn-state",
        "Подключение VPN",
        if s.error.is_some() || (s.running && !s.ready) {
            Level::Err
        } else if s.running {
            Level::Ok
        } else {
            Level::Info
        },
        if s.running && s.ready {
            "готово"
        } else if s.running {
            "не готово"
        } else {
            "выключено"
        },
        s.error.unwrap_or_else(|| {
            if s.running {
                "Процесс и локальный порт/адаптер готовы. Это не подтверждает доступность всех сайтов.".into()
            } else {
                "Включите VPN в его разделе или на Главной.".into()
            }
        }),
    ));
    if s.running {
        let cfg = std::fs::read_to_string(super::config_file())
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
        let dns_ok = cfg.as_ref().is_some_and(|c| {
            c["dns"]["servers"]
                .as_array()
                .is_some_and(|servers| servers.iter().any(|d| d["type"] == "https" && d["detour"] == "proxy"))
        });
        rows.push(item(
            "vpn-dns",
            "DNS VPN",
            if dns_ok { Level::Ok } else { Level::Warn },
            if dns_ok { "настроен" } else { "проверьте" },
            "Для VPN-доменов используется DNS через VPN. После изменения списков перезапустите VPN.".into(),
        ));
    }
    let proxy = super::sysproxy::current();
    if !s.running && super::sysproxy_is_ours(&proxy, store.port) {
        rows.push(item(
            "vpn-proxy",
            "Прокси Windows",
            Level::Err,
            "остался включён",
            "Локальный VPN не работает. Выключите VPN через приложение, чтобы восстановить прежний прокси.".into(),
        ));
    }
    let service = crate::zapret::service::status(st.root().as_deref());
    let bypass_auto = settings.start_bypass_on_launch || service.state != crate::zapret::service::State::NotInstalled;
    let joint = settings.autostart && store.autostart && bypass_auto;
    rows.push(item("vpn-autostart", "Совместный автозапуск", if joint { Level::Ok } else { Level::Info }, if joint { "настроен" } else { "не настроен" },
        "Для запуска вместе с Windows включите автозапуск приложения в Настройках, запуск обхода при старте (либо службу zapret), а в VPN — «Включать вместе с ZapretGUI».".into()));
    rows
}

pub fn explain_owned_proxy(st: &AppState, rows: &mut [Item]) {
    let store = super::load();
    if st.vpn.lock().unwrap().child.is_some() && super::sysproxy::is_ours(store.port) {
        if let Some(row) = rows.iter_mut().find(|r| r.id == "proxy") {
            row.level = Level::Ok;
            row.value = "VPN ZapretGUI".into();
            row.detail = Some("Системный прокси включён самим ZapretGUI. Домены из VPN-списков направляются через VPN; остальные — напрямую, где их может обработать zapret.".into());
        }
    }
}

pub fn enrich_site(st: &AppState, root: &Path, report: &mut Report) {
    let store = super::load();
    let _ = super::lists::ensure_general();
    let domain = &report.domain;
    let direct = super::lists::matches(&super::lists::custom(super::lists::DIRECT_FILE), domain);
    let own = super::lists::matches(&super::lists::custom(super::lists::VPN_FILE), domain);
    let zapret = store.zapret_direct && super::lists::matches(&super::lists::zapret_domains(Some(root)), domain);
    let general = super::lists::matches(&super::lists::custom(super::lists::GENERAL_FILE), domain);
    let ru = super::lists::matches(&super::lists::domains(&super::lists::RU_DIRECT), domain);
    let via_vpn = !direct
        && (own
            || (!zapret
                && match store.route_mode {
                    super::RouteMode::Lists => general,
                    super::RouteMode::AllExceptRu => !ru,
                }));
    let running = st.vpn.lock().unwrap().child.is_some();
    report.items.push(crate::zapret::site::Item {
        id: "vpn", title: "Правило VPN", level: Level::Info,
        value: if via_vpn { if running { "через VPN" } else { "через VPN · выключен" } } else { "напрямую" }.into(),
        detail: Some(format!("По текущим настройкам: {}. Проверка сайта не использует системный прокси; при TUN маршрут определяется его правилами.",
            if direct { "«Всегда напрямую»" } else if own { "личный VPN-список" } else if zapret { "сайт передан zapret" } else if via_vpn && general { "list-vpn-general" } else { "режим маршрутизации" })),
    });
    if via_vpn {
        for action in &mut report.actions {
            if matches!(action, Action::AddToVpn) {
                *action = Action::OpenVpn;
            }
        }
    }
}
