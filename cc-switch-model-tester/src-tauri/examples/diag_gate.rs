//! 临时诊断：对比工具仿真请求与 Node 成功请求的差异（不入仓库）。
//! 用法：cargo run --example diag_gate [-- proxy]
//! 无参数 = 直连；带 proxy = 走 socks5://127.0.0.1:1080

use cc_switch_model_tester_lib::domain::TestMode;
use cc_switch_model_tester_lib::{emulation, protocol};

use rusqlite::Connection;
use serde_json::Value;

fn redact(s: &str) -> String {
    // 脱敏长 token（保留前 8 位）
    let redact_tok = |m: &str| -> String {
        if m.len() > 12 {
            format!("{}***", &m[..8])
        } else {
            "***".into()
        }
    };
    let mut out = s.to_string();
    for pat in ["sk-ant-", "sk-", "Bearer "] {
        while let Some(i) = out.find(pat) {
            let start = i + pat.len();
            let end = out[start..]
                .find(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_'))
                .map(|j| start + j)
                .unwrap_or(out.len());
            if end > start {
                let tok = out[start..end].to_string();
                out = out.replace(&tok, &redact_tok(&tok));
            } else {
                break;
            }
        }
    }
    out
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Runtime::new()?;
    // 参数：供应商过滤关键字（默认 zzzcoding）、画像名（默认 claude-code）、proxy
    let args: Vec<String> = std::env::args().collect();
    let filter = args.get(1).cloned().unwrap_or_else(|| "zzzcoding".into());
    let profile_name = args.get(2).cloned().unwrap_or_else(|| "claude-code".into());
    let use_proxy = args.iter().any(|a| a == "proxy");
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    let conn = Connection::open_with_flags(
        format!("{home}\\.cc-switch\\cc-switch.db"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let mut stmt = conn.prepare("SELECT id, name, app_type, settings_config, meta FROM providers")?;
    let rows: Vec<(String, String, String, String, String)> = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get::<_, Option<String>>(4)?.unwrap_or_default(),
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();
    let (id, name, app_raw, sc_raw, meta_raw) = rows
        .into_iter()
        .find(|(i, n, _, sc, _)| n.contains(&filter) || sc.contains(&filter) || i.contains(&filter))
        .expect("未找到供应商");
    let app = cc_switch_model_tester_lib::domain::AppType::from_db_str(&app_raw)
        .ok_or("未知 app_type")?;
    let _sc: Value = serde_json::from_str(&sc_raw)?;
    let meta: Value = serde_json::from_str(&meta_raw)?;
    println!("供应商 {name} ({app_raw}) | 画像 {profile_name} | proxy={use_proxy}");

    // 完整工具链路：parse_provider → to_test_target → 每模型仿真选择
    let snap = cc_switch_model_tester_lib::parser::parse_provider(
        app,
        &id,
        &name,
        &sc_raw,
        &meta_raw,
        vec![],
    );
    println!("快照状态: {:?} | 协议 {:?}", snap.status, snap.protocol);
    let model = snap
        .models
        .first()
        .expect("无模型")
        .clone();
    let mut target = snap.to_test_target(&model);
    target.emulation = profile_name != "off";
    target.emulation_profile = if profile_name == "off" { None } else { Some(profile_name.clone()) };
    println!("凭据: {:?} | 端点: {} | 模型: {}", target.credential.as_ref().map(|c| format!("{:?}({}字符)", c.kind, c.secret.len())).unwrap_or_default(), target.endpoint_url, target.model_id);
    println!("customUserAgent: {:?}", meta["customUserAgent"].as_str().unwrap_or("(无)"));
    println!(
        "localProxyRequestOverrides: {}",
        redact(&meta["localProxyRequestOverrides"].to_string())
    );

    let target = target;
    let _ = &meta_raw;

    let mut prepared = protocol::build_request(&target, "hi", TestMode::NonStreaming, 64)?;
    let profile = emulation::resolve_profile(&target).unwrap();
    emulation::apply_client_emulation(&mut prepared, &target, profile, "diag-run");
    if let Some(patch) = &target.local_proxy_body_patch {
        emulation::apply_local_proxy_body_patch(&mut prepared.body, patch);
    }

    println!("\n=== 实际发送 headers ===");
    for (k, v) in &prepared.headers {
        println!("  {k}: {}", redact(v));
    }
    println!("=== body（脱敏）===");
    println!("  {}", redact(&prepared.body.to_string()));

    // 发送
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .connect_timeout(std::time::Duration::from_secs(15))
        // 与 http.rs 一致：默认 UA（理论上会被显式 header 覆盖）
        .user_agent(cc_switch_model_tester_lib::APP_USER_AGENT);
    if use_proxy {
        builder = builder.proxy(reqwest::Proxy::all("socks5://127.0.0.1:1080")?);
    }
    let client = builder.build()?;
    let mut req = client.post(&prepared.url);
    for (k, v) in &prepared.headers {
        req = req.header(k, v);
    }
    let t0 = std::time::Instant::now();
    let (status, text) = rt.block_on(async {
        let resp = req.json(&prepared.body).send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        Ok::<_, reqwest::Error>((status, text))
    })?;
    println!("\n=== 响应 ===");
    println!("HTTP {status} ({}ms)", t0.elapsed().as_millis());
    println!("{}", redact(&text.chars().take(300).collect::<String>()));
    Ok(())
}
