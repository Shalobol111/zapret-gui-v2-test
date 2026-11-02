//! Снимок реальных соединений sing-box по локальному API с одноразовым секретом.
use serde_json::{json, Value};
use std::time::Duration;

pub fn configure(cfg: &mut Value) -> Result<(), String> {
    let socket = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let addr = socket.local_addr().map_err(|e| e.to_string())?;
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).map_err(|e| e.to_string())?;
    cfg["experimental"]["clash_api"] = json!({
        "external_controller": addr.to_string(), "secret": crate::net::hex(&secret),
        "access_control_allow_origin": ["http://127.0.0.1"],
        "access_control_allow_private_network": false
    });
    Ok(())
}

pub async fn snapshot() -> Result<String, String> {
    Ok(format_snapshot(&connections().await?))
}

pub async fn voice_ips() -> Result<Vec<String>, String> {
    let data = connections().await?;
    extract_voice_ips(&data)
}

fn extract_voice_ips(data: &Value) -> Result<Vec<String>, String> {
    let mut ips = Vec::new();
    if let Some(rows) = data["connections"].as_array() {
        for row in rows {
            let m = &row["metadata"];
            let name =
                m["processPath"].as_str().unwrap_or("").rsplit(['\\', '/']).next().unwrap_or("").to_ascii_lowercase();
            if m["network"] == "udp"
                && matches!(
                    name.as_str(),
                    "discord.exe" | "discordptb.exe" | "discordcanary.exe" | "discorddevelopment.exe"
                )
            {
                if let Some(ip) = m["destinationIP"].as_str().and_then(|s| s.parse::<std::net::IpAddr>().ok()) {
                    if !ip.is_loopback() && !ip.is_unspecified() && !ip.is_multicast() {
                        ips.push(format!("{ip}/{}", if ip.is_ipv4() { 32 } else { 128 }));
                    }
                }
            }
        }
    }
    ips.sort();
    ips.dedup();
    if ips.is_empty() {
        return Err("Сначала попробуйте подключиться к голосовому каналу Discord, затем нажмите снова".into());
    }
    Ok(ips)
}

async fn connections() -> Result<Value, String> {
    let cfg: Value = serde_json::from_str(&std::fs::read_to_string(super::config_file()).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let api = &cfg["experimental"]["clash_api"];
    let addr: std::net::SocketAddr = api["external_controller"]
        .as_str()
        .ok_or("Перезапустите VPN из новой сборки")?
        .parse()
        .map_err(|_| "Неверный адрес диагностики")?;
    if addr.ip() != std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST) {
        return Err("Диагностика доступна только локально".into());
    }
    let client =
        reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(3)).build().map_err(|e| e.to_string())?;
    let mut response = client
        .get(format!("http://{addr}/connections"))
        .bearer_auth(api["secret"].as_str().ok_or("Нет ключа диагностики")?)
        .send()
        .await
        .map_err(|_| "Не удалось получить маршруты — включите VPN")?
        .error_for_status()
        .map_err(|_| "Ядро отказало в диагностике")?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
            return Err("Слишком много соединений".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let data: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    Ok(data)
}

fn format_snapshot(data: &Value) -> String {
    let Some(rows) = data["connections"].as_array() else {
        return "Нет данных о соединениях".into();
    };
    let mut lines = Vec::new();
    for row in rows.iter().take(300) {
        let m = &row["metadata"];
        let host = m["host"].as_str().filter(|s| !s.is_empty()).or_else(|| m["destinationIP"].as_str()).unwrap_or("?");
        let chains: Vec<&str> =
            row["chains"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        let route = if chains.contains(&"proxy") {
            "VPN"
        } else if chains.contains(&"direct") {
            "Напрямую"
        } else {
            "Не определён"
        };
        let port = m["destinationPort"].as_str().map(str::to_owned).unwrap_or_else(|| m["destinationPort"].to_string());
        let process = m["processPath"]
            .as_str()
            .filter(|p| !p.is_empty())
            .and_then(|p| p.rsplit(['\\', '/']).next())
            .unwrap_or("процесс неизвестен");
        let ip = m["destinationIP"].as_str().unwrap_or("?");
        lines.push(format!(
            "{} | {}:{} | IP {} | {} | {} | ↑{} ↓{} байт",
            route,
            host,
            port,
            ip,
            m["network"].as_str().unwrap_or("?"),
            process,
            row["upload"].as_u64().unwrap_or(0),
            row["download"].as_u64().unwrap_or(0)
        ));
    }
    if lines.is_empty() {
        "Активных соединений нет. Откройте видео или подключитесь к голосу и обновите снимок.".into()
    } else {
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn bypass_only_exact_udp_destinations_owned_by_discord() {
        let data = serde_json::json!({"connections":[
            {"metadata":{"processPath":"C:\\Discord\\Discord.exe","network":"udp","destinationIP":"104.29.142.116"}},
            {"metadata":{"processPath":"C:\\Discord\\Discord.exe","network":"udp","destinationIP":"104.29.142.116"}},
            {"metadata":{"processPath":"C:\\Discord\\Discord.exe","network":"tcp","destinationIP":"1.2.3.4"}},
            {"metadata":{"processPath":"C:\\Windows\\svchost.exe","network":"udp","destinationIP":"20.201.213.2"}},
            {"metadata":{"processPath":"Discord.exe","network":"udp","destinationIP":"127.0.0.1"}}
        ]});
        assert_eq!(super::extract_voice_ips(&data).unwrap(), vec!["104.29.142.116/32"]);
        assert!(super::extract_voice_ips(&serde_json::json!({"connections":[]})).is_err());
    }

    #[test]
    fn routes_come_from_actual_chain_not_domain_guess() {
        let data = serde_json::json!({"connections":[
            {"metadata":{"host":"youtube.com","network":"tcp"},"chains":["proxy"]},
            {"metadata":{"host":"chatgpt.com","network":"udp"},"chains":["direct"]}
        ]});
        let text = super::format_snapshot(&data);
        assert!(text.contains("VPN | youtube.com"));
        assert!(text.contains("Напрямую | chatgpt.com"));
    }
}
