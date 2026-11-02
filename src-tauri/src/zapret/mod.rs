pub mod batparse;
pub mod diagnostics;
pub mod discord;
pub mod extras;
pub mod filters;
pub mod hosts;
pub mod layout;
pub mod runner;
pub mod service;
pub mod site;
pub mod strategy;
pub mod tests;
pub mod updater;
pub mod userlists;

use std::path::Path;

use batparse::Vars;
use strategy::Strategy;

/// Фильтр отозванной проверочной сборки. Оставлен только для распознавания старых процессов.
pub const TUN_FILTER: &str = "--wf-raw-part=(not ip or (ip.SrcAddr != 172.19.0.1 and ip.DstAddr != 172.19.0.1)) and (not ipv6 or (ipv6.SrcAddr != fdfe:dcba:9876::1 and ipv6.DstAddr != fdfe:dcba:9876::1))";

/// Переменные для подстановки в стратегию с учётом текущего Game Filter.
pub fn vars(root: &Path) -> Vars {
    let (game_tcp, game_udp) = filters::game(root).vars();
    Vars { root: root.to_path_buf(), game_tcp, game_udp }
}

/// Стратегия по имени и её аргументы winws.
pub fn strategy_args(root: &Path, name: &str) -> Result<(Strategy, Vec<String>), String> {
    let s = strategy::find(root, name).ok_or_else(|| format!("стратегия «{name}» не найдена"))?;
    let args = batparse::parse_file(&s.file, &vars(root)).map_err(|e| format!("«{name}»: {e}"))?;
    Ok((s, args))
}

/// Имя стратегии, чьи аргументы совпадают с данными (для winws, запущенного не нами).
pub fn match_strategy(root: &Path, args: &[String]) -> Option<String> {
    let args: Vec<String> = args.iter().filter(|a| a.as_str() != TUN_FILTER).cloned().collect();
    let v = vars(root);
    strategy::list(root)
        .into_iter()
        .find(|s| batparse::parse_file(&s.file, &v).map(|a| a == args).unwrap_or(false))
        .map(|s| s.name)
}

#[cfg(test)]
mod compat_tests {
    #[test]
    fn revoked_filter_is_not_generated_but_old_process_is_recognized() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/flowseal-1.10.3");
        for strategy in super::strategy::list(&root) {
            let (_, mut args) = super::strategy_args(&root, &strategy.name).unwrap();
            assert!(!args.iter().any(|a| a == super::TUN_FILTER));
            assert_eq!(super::match_strategy(&root, &args), Some(strategy.name.clone()));
            args.insert(0, super::TUN_FILTER.into());
            assert_eq!(super::match_strategy(&root, &args), Some(strategy.name));
        }
    }
    use std::path::Path;
}
