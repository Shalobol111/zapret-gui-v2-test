//! Проверка HTTPS через отдельный профиль. Рабочее подключение и настройки Windows не меняются.
use super::links::Server;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    net::{TcpListener, TcpStream},
    os::windows::io::AsRawHandle,
    path::{Path, PathBuf},
    process::{Child, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE},
    System::JobObjects::*,
};

static ACTIVE: AtomicBool = AtomicBool::new(false);
struct Active;
impl Drop for Active {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Release);
    }
}

struct Folder(PathBuf);
impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct ManagedChild {
    child: Child,
    job: HANDLE,
}
impl ManagedChild {
    fn spawn(exe: &Path, config: &Path, xray: bool, dir: &Path) -> Result<Self, String> {
        let job = unsafe { CreateJobObjectW(None, windows::core::PCWSTR::null()) }.map_err(|e| e.to_string())?;
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if let Err(e) = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&info) as u32,
            )
        } {
            unsafe {
                let _ = CloseHandle(job);
            }
            return Err(e.to_string());
        }
        let mut command = crate::sys::cmd::hidden(&exe.to_string_lossy());
        if xray {
            command.args(["run", "-c"]).arg(config);
        } else {
            command.args(["run", "-c"]).arg(config).arg("-D").arg(dir);
        }
        let mut child = match command.current_dir(dir).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
            Ok(child) => child,
            Err(e) => {
                unsafe {
                    let _ = CloseHandle(job);
                }
                return Err(e.to_string());
            }
        };
        if let Err(e) = unsafe { AssignProcessToJobObject(job, HANDLE(child.as_raw_handle())) } {
            let _ = child.kill();
            let _ = child.wait();
            unsafe {
                let _ = CloseHandle(job);
            }
            return Err(e.to_string());
        }
        Ok(Self { child, job })
    }
}
impl Drop for ManagedChild {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.job);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn workspace() -> Result<Folder, String> {
    let mut random = [0u8; 12];
    getrandom::fill(&mut random).map_err(|e| e.to_string())?;
    let parent = super::dir().join("probes");
    fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
    let path = parent.join(crate::net::hex(&random));
    fs::create_dir(&path).map_err(|e| e.to_string())?;
    Ok(Folder(path))
}

fn config(server: &Server, port: u16, log: &Path) -> Result<Value, String> {
    if server.via_xray() {
        return super::xray::config(&[server], port, &log.to_string_lossy());
    }
    let mut outbound = server.outbound.clone().ok_or("Профиль не поддерживается")?;
    outbound["tag"] = json!("proxy");
    Ok(json!({
        "log": {"level":"warn","output":log.to_string_lossy()},
        "inbounds":[{"type":"mixed","tag":"in","listen":"127.0.0.1","listen_port":port}],
        "outbounds":[outbound],
        "dns":{"servers":[{"type":"local","tag":"local"}]},
        "route":{"final":"proxy","auto_detect_interface":true,"default_domain_resolver":"local"}
    }))
}

fn measure(server: &Server, exe: &Path, parent: &Path, n: usize) -> Option<u32> {
    if !server.usable() {
        return None;
    }
    let dir = parent.join(format!("profile-{n}"));
    fs::create_dir(&dir).ok()?;
    let config_file = dir.join("config.json");
    let listener = TcpListener::bind("127.0.0.1:0").ok()?;
    let port = listener.local_addr().ok()?.port();
    let cfg = config(server, port, &dir.join("core.log")).ok()?;
    fs::write(&config_file, serde_json::to_vec(&cfg).ok()?).ok()?;
    drop(listener);
    let mut child = ManagedChild::spawn(exe, &config_file, server.via_xray(), &dir).ok()?;
    let started = Instant::now();
    loop {
        if !matches!(child.child.try_wait(), Ok(None)) {
            return None;
        }
        if TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(100)).is_ok() {
            break;
        }
        if started.elapsed() > Duration::from_secs(4) {
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let curl = crate::zapret::tests::curl::curl_path()?;
    let proxy = format!("{}://127.0.0.1:{port}", if server.via_xray() { "socks5h" } else { "http" });
    for url in ["https://www.gstatic.com/generate_204", "https://cp.cloudflare.com/generate_204"] {
        let output = crate::sys::cmd::hidden(&curl.to_string_lossy())
            .args([
                "--proxy",
                &proxy,
                "--noproxy",
                "",
                "--connect-timeout",
                "5",
                "--max-time",
                "8",
                "--silent",
                "--output",
                "NUL",
                "--write-out",
                "%{http_code}|%{time_total}",
                url,
            ])
            .output()
            .ok()?;
        if output.status.success() {
            if let Some(ms) = parse_timing(&String::from_utf8_lossy(&output.stdout)) {
                return matches!(child.child.try_wait(), Ok(None)).then_some(ms);
            }
        }
    }
    None
}

fn parse_timing(text: &str) -> Option<u32> {
    let (code, time) = text.trim().split_once('|')?;
    let seconds: f64 = time.parse().ok()?;
    (code == "204" && seconds.is_finite() && (0.0..=20.0).contains(&seconds))
        .then(|| (seconds * 1000.0).ceil().max(1.0) as u32)
}

pub fn run(store: &super::Store) -> Result<HashMap<String, Option<u32>>, String> {
    ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "Проверка серверов уже идёт")?;
    let _active = Active;
    let folder = workspace()?;
    let servers: Vec<Server> = super::servers(store).into_iter().cloned().collect();
    // Копии exe не совпадают с путями kill_orphans; остановка рабочего VPN не прерывает проверку.
    let sb = folder.0.join("sing-box.exe");
    let xr = folder.0.join("xray.exe");
    if servers.iter().any(|s| s.usable() && !s.via_xray()) {
        fs::copy(super::singbox::exe(), &sb).map_err(|e| e.to_string())?;
    }
    if servers.iter().any(Server::via_xray) {
        fs::copy(super::xray::exe(), &xr).map_err(|e| e.to_string())?;
    }
    let mut results = HashMap::new();
    for (batch_index, batch) in servers.chunks(3).enumerate() {
        std::thread::scope(|scope| {
            let handles: Vec<_> = batch
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let exe = if s.via_xray() { &xr } else { &sb };
                    let dir = &folder.0;
                    scope.spawn(move || (s.id.clone(), measure(s, exe, dir, batch_index * 3 + i)))
                })
                .collect();
            for handle in handles {
                if let Ok((id, delay)) = handle.join() {
                    results.insert(id, delay);
                }
            }
        });
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore]
    fn isolated_https_and_rejected_profile() {
        use super::*;
        assert!(std::env::var_os("ZAPRETGUI_DATA_DIR").is_some());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let accepted = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = accepted.clone();
        let endpoint = std::thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                if let Ok((connection, _)) = listener.accept() {
                    count.fetch_add(1, Ordering::Relaxed);
                    drop(connection);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        let direct = Server {
            id: "https-control".into(),
            name: "HTTPS control".into(),
            protocol: "Test".into(),
            host: "localhost".into(),
            port: 0,
            outbound: Some(json!({"type":"direct"})),
            unsupported: None,
            link: None,
        };
        let link = format!("vless://00000000-0000-0000-0000-000000000001@127.0.0.1:{port}?security=none#Rejected");
        let mut rejected = super::super::links::parse(&link).unwrap();
        rejected.id = "rejected".into();
        let store = super::super::Store { manual: vec![direct, rejected], ..Default::default() };
        let result = run(&store);
        stop.store(true, Ordering::Release);
        endpoint.join().unwrap();
        let result = result.unwrap();
        assert!(accepted.load(Ordering::Relaxed) > 0, "Xray must contact the test endpoint");
        assert_eq!(result["rejected"], None, "An open TCP port is not an HTTPS success");
        assert!(result["https-control"].is_some(), "Control HTTPS request must succeed");
        assert_eq!(
            fs::read_dir(super::super::dir().join("probes")).unwrap().count(),
            0,
            "Temporary profiles must be removed"
        );
    }

    #[test]
    fn requires_https_success_and_valid_timing() {
        assert_eq!(super::parse_timing("204|0.125"), Some(125));
        for text in ["200|0.1", "403|0.1", "204|NaN", "204|-1", "204|99"] {
            assert_eq!(super::parse_timing(text), None);
        }
    }
    #[test]
    #[ignore]
    fn live_profiles() {
        assert!(std::env::var_os("ZAPRETGUI_DATA_DIR").is_some());
        let source = std::env::var("ZAPRETGUI_PROBE_PROFILE_FILE").expect("Profile file path required");
        let store: crate::vpn::Store = serde_json::from_str(&std::fs::read_to_string(source).unwrap()).unwrap();
        let result = super::run(&store).unwrap();
        println!("Проверено: {}, HTTPS успешно: {}", result.len(), result.values().filter(|v| v.is_some()).count());
        assert!(result.values().any(Option::is_some), "Ни один профиль не прошёл HTTPS-проверку");
    }
}
