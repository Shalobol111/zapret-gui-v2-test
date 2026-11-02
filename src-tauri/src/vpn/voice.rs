//! Временные маршруты голосовых IP Discord мимо TUN, без перезапуска VPN.
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};
use windows::Win32::{
    NetworkManagement::{IpHelper::*, Ndis::IfOperStatusUp},
    Networking::WinSock::*,
};

const LEASE: u32 = 30;
static OWNED: Mutex<Option<HashMap<IpAddr, MIB_IPFORWARD_ROW2>>> = Mutex::new(None);

fn addr(ip: IpAddr) -> SOCKADDR_INET {
    match ip {
        IpAddr::V4(ip) => SOCKADDR_INET {
            Ipv4: SOCKADDR_IN {
                sin_family: AF_INET,
                sin_addr: IN_ADDR { S_un: IN_ADDR_0 { S_addr: u32::from_ne_bytes(ip.octets()) } },
                ..Default::default()
            },
        },
        IpAddr::V6(ip) => SOCKADDR_INET {
            Ipv6: SOCKADDR_IN6 {
                sin6_family: AF_INET6,
                sin6_addr: IN6_ADDR { u: IN6_ADDR_0 { Byte: ip.octets() } },
                ..Default::default()
            },
        },
    }
}

// Используем активный физический default route с учётом метрики интерфейса.
// Не угадываем gateway и не берём маршруты самого TUN.
fn gateway(ip: IpAddr) -> Result<MIB_IPFORWARD_ROW2, String> {
    unsafe {
        let family = if ip.is_ipv4() { AF_INET } else { AF_INET6 };
        let mut table = std::ptr::null_mut();
        GetIpForwardTable2(family, &mut table).ok().map_err(|e| e.to_string())?;
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
        let mut best = None;
        let mut score = u64::MAX;
        for row in rows.iter().filter(|r| r.DestinationPrefix.PrefixLength == 0) {
            let mut iface = MIB_IF_ROW2 { InterfaceIndex: row.InterfaceIndex, ..Default::default() };
            if GetIfEntry2(&mut iface).is_err() || iface.OperStatus != IfOperStatusUp || !matches!(iface.Type, 6 | 71) {
                continue;
            }
            let mut metric =
                MIB_IPINTERFACE_ROW { Family: family, InterfaceIndex: row.InterfaceIndex, ..Default::default() };
            if GetIpInterfaceEntry(&mut metric).is_err() {
                continue;
            }
            let cost = u64::from(row.Metric) + u64::from(metric.Metric);
            if cost < score {
                best = Some(*row);
                score = cost;
            }
        }
        FreeMibTable(table.cast());
        best.ok_or_else(|| "Нет физического маршрута для голоса Discord".into())
    }
}

fn make_row(ip: IpAddr) -> Result<MIB_IPFORWARD_ROW2, String> {
    let gw = gateway(ip)?;
    let mut row = MIB_IPFORWARD_ROW2::default();
    unsafe {
        InitializeIpForwardEntry(&mut row);
    }
    row.InterfaceIndex = gw.InterfaceIndex;
    row.InterfaceLuid = gw.InterfaceLuid;
    row.NextHop = gw.NextHop;
    row.DestinationPrefix.Prefix = addr(ip);
    row.DestinationPrefix.PrefixLength = if ip.is_ipv4() { 32 } else { 128 };
    row.SitePrefixLength = 0;
    row.Metric = 0;
    row.Protocol = MIB_IPPROTO_NETMGMT;
    row.Immortal = false;
    row.ValidLifetime = LEASE;
    row.PreferredLifetime = LEASE;
    Ok(row)
}

pub fn clear() {
    if let Some(rows) = OWNED.lock().unwrap().take() {
        for row in rows.values() {
            unsafe {
                let _ = DeleteIpForwardEntry2(row);
            }
        }
    }
}

fn update(ips: &[String]) -> Result<usize, String> {
    let mut guard = OWNED.lock().unwrap();
    let rows = guard.get_or_insert_with(HashMap::new);
    let mut added = 0;
    // Продление раз в несколько секунд; новый default route учитывается без рестарта.
    let known: Vec<IpAddr> = rows.keys().copied().collect();
    for ip in known {
        let new = make_row(ip)?;
        let old = rows[&ip];
        let same_gateway = unsafe {
            old.InterfaceIndex == new.InterfaceIndex
                && if ip.is_ipv4() {
                    old.NextHop.Ipv4.sin_addr.S_un.S_addr == new.NextHop.Ipv4.sin_addr.S_un.S_addr
                } else {
                    old.NextHop.Ipv6.sin6_addr.u.Byte == new.NextHop.Ipv6.sin6_addr.u.Byte
                }
        };
        if same_gateway && unsafe { SetIpForwardEntry2(&new) }.is_ok() {
            continue;
        }
        unsafe {
            let _ = DeleteIpForwardEntry2(&old);
        }
        rows.remove(&ip);
        if unsafe { CreateIpForwardEntry2(&new) }.is_ok() {
            rows.insert(ip, new);
        }
    }
    for ip in ips.iter().filter_map(|s| s.split('/').next()?.parse::<IpAddr>().ok()) {
        if rows.len() >= 128 || rows.contains_key(&ip) {
            continue;
        }
        let row = make_row(ip)?;
        let result = unsafe { CreateIpForwardEntry2(&row) };
        if result.is_ok() {
            rows.insert(ip, row);
            added += 1;
        }
        // Чужой существующий маршрут не изменяем и не берём на очистку.
        else if result.0 != 5010 {
            return Err(format!("Маршрут Discord: {}", result.0));
        }
    }
    Ok(added)
}

pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let mut session = None;
        let mut tick = Instant::now() - Duration::from_secs(10);
        let mut warned = false;
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let st = app.state::<crate::state::AppState>();
            let active = {
                let rt = st.vpn.lock().unwrap();
                rt.voice_auto.then(|| rt.child.as_ref().map(|c| (c.id(), rt.started_at))).flatten()
            };
            if active != session {
                clear();
                session = active;
                warned = false;
            }
            let Some(id) = active else {
                continue;
            };
            // API только при активном TUN; один ограниченный снимок раз в 2 с.
            let discord = crate::sys::process::find_cached(&[
                "Discord.exe",
                "DiscordPTB.exe",
                "DiscordCanary.exe",
                "DiscordDevelopment.exe",
            ]);
            if discord.is_empty() {
                clear();
                continue;
            }
            let ips = tauri::async_runtime::block_on(super::routes::voice_ips()).unwrap_or_default();
            if ips.is_empty() && tick.elapsed() < Duration::from_secs(8) {
                continue;
            }
            let rt = st.vpn.lock().unwrap();
            if !rt.voice_auto || rt.child.as_ref().map(|c| (c.id(), rt.started_at)) != Some(id) {
                continue;
            }
            match update(&ips) {
                Ok(n) if n > 0 => crate::journal::info(format!("Discord: голосовые адреса ({n}) направлены мимо TUN")),
                Ok(_) => {}
                Err(e) if !warned => {
                    crate::journal::warn(e);
                    warned = true;
                }
                Err(_) => {}
            }
            tick = Instant::now();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore]
    fn live_route_lease_and_cleanup() {
        assert!(std::env::var_os("ZAPRETGUI_DATA_DIR").is_some());
        let ip: IpAddr = "192.0.2.254".parse().unwrap();
        let row = make_row(ip).unwrap();
        unsafe {
            CreateIpForwardEntry2(&row).ok().expect("Test route must not already exist");
        }
        struct Cleanup(MIB_IPFORWARD_ROW2);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                unsafe {
                    let _ = DeleteIpForwardEntry2(&self.0);
                }
            }
        }
        let _cleanup = Cleanup(row);
        let mut query = row;
        unsafe {
            GetIpForwardEntry2(&mut query).ok().unwrap();
        }
        assert!(query.ValidLifetime <= LEASE);
        let mut best = MIB_IPFORWARD_ROW2::default();
        let mut source = SOCKADDR_INET::default();
        unsafe {
            GetBestRoute2(None, 0, None, &addr(ip), 0, &mut best, &mut source).ok().unwrap();
        }
        assert_eq!(best.DestinationPrefix.PrefixLength, 32);
        assert_eq!(best.InterfaceIndex, row.InterfaceIndex);
        assert_eq!(update(&[format!("{ip}/32")]).unwrap(), 0, "Existing route is not ours");
        clear();
        unsafe {
            GetIpForwardEntry2(&mut query).ok().expect("Cleanup must leave an existing foreign route");
        }
        unsafe {
            SetIpForwardEntry2(&row).ok().unwrap();
        }
        std::thread::sleep(Duration::from_secs(u64::from(LEASE) + 3));
        assert!(unsafe { GetIpForwardEntry2(&mut query) }.is_err(), "Route must expire after process loss");
    }
}
