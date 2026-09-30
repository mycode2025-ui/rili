//! 云同步协议边界。
//!
//! 本模块只负责本地同步配置与协议状态，不假定某一家云厂商的接口，也不保存普通备份里的密码/token。
//! 服务端适配器可以实现：上传 `LocalBackup` 快照、返回带 `format_version` 的快照，并在服务端做版本冲突处理。

use crate::db;
use anyhow::{Context, Result};
use serde::Serialize;
use std::time::Duration;
use url::Url;

const ENDPOINT_KEY: &str = "sync_endpoint";
const ACCOUNT_KEY: &str = "sync_account";

#[derive(Debug, Clone, Serialize)]
pub struct SyncStatus {
    pub configured: bool,
    pub endpoint: String,
    pub account: String,
    pub protocol: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncTransfer {
    pub endpoint: String,
    pub status: String,
    pub bytes: usize,
}

pub fn status(conn: &rusqlite::Connection) -> SyncStatus {
    let endpoint = db::get_setting(conn, ENDPOINT_KEY, "").unwrap_or_default();
    let account = db::get_setting(conn, ACCOUNT_KEY, "").unwrap_or_default();
    let configured = !endpoint.trim().is_empty();
    SyncStatus {
        configured,
        endpoint,
        account,
        protocol: "rili-sync/v1（LocalBackup JSON 快照）".to_string(),
        message: if configured {
            "已配置地址；当前尚未执行云端上传/拉取，token 通过外部凭据注入".to_string()
        } else {
            "未配置云端地址；数据仍只保存在本机".to_string()
        },
    }
}

pub fn configure(conn: &rusqlite::Connection, endpoint: &str, account: &str) -> Result<SyncStatus> {
    let endpoint = endpoint.trim();
    validate_endpoint(endpoint)?;
    db::set_setting(conn, ENDPOINT_KEY, endpoint).context("保存同步地址失败")?;
    db::set_setting(conn, ACCOUNT_KEY, account.trim()).context("保存同步账号失败")?;
    Ok(status(conn))
}

fn validate_endpoint(endpoint: &str) -> Result<()> {
    let parsed = Url::parse(endpoint).context("同步地址不是有效 URL")?;
    let local_http = parsed.scheme() == "http"
        && matches!(
            parsed.host_str(),
            Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
        );
    anyhow::ensure!(
        parsed.scheme() == "https" || local_http,
        "同步地址必须使用 HTTPS；仅本机回环地址允许 HTTP"
    );
    anyhow::ensure!(
        parsed.username().is_empty() && parsed.password().is_none(),
        "同步地址不能包含账号密码"
    );
    anyhow::ensure!(
        parsed.query().is_none() && parsed.fragment().is_none(),
        "同步地址不能包含查询参数或片段"
    );
    Ok(())
}

pub fn clear(conn: &rusqlite::Connection) -> Result<SyncStatus> {
    db::set_setting(conn, ENDPOINT_KEY, "")?;
    db::set_setting(conn, ACCOUNT_KEY, "")?;
    Ok(status(conn))
}

pub fn protocol_description() -> &'static str {
    "rili-sync/v1：仅 HTTPS（本机回环地址可用 HTTP）；GET/PUT 服务端快照；PUT 响应须含 X-Rili-Protocol: rili-sync/v1 和 JSON {format_version:1,accepted:true}；冲突由服务端处理；token 不进入普通备份。"
}

fn endpoint(conn: &rusqlite::Connection) -> Result<String> {
    let endpoint = db::get_setting(conn, ENDPOINT_KEY, "")?;
    anyhow::ensure!(
        !endpoint.trim().is_empty(),
        "未配置同步地址，请先执行 sync configure"
    );
    validate_endpoint(&endpoint)?;
    Ok(format!("{}/snapshot", endpoint.trim_end_matches('/')))
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .redirects(0)
        .timeout_connect(Duration::from_secs(12))
        .timeout_read(Duration::from_secs(12))
        .timeout_write(Duration::from_secs(12))
        .build()
}

fn with_token(request: ureq::Request) -> ureq::Request {
    match std::env::var("RILI_SYNC_TOKEN") {
        Ok(token) if !token.trim().is_empty() => {
            request.set("Authorization", &format!("Bearer {token}"))
        }
        _ => request,
    }
}

/// 把当前本地快照显式上传到配置的同步服务。
pub fn push(conn: &rusqlite::Connection) -> Result<SyncTransfer> {
    let url = endpoint(conn)?;
    let backup = db::export_backup(conn)?;
    let payload = serde_json::to_string(&backup).context("序列化同步快照失败")?;
    let response = with_token(agent().put(&url))
        .set("Content-Type", "application/json")
        .set("X-Rili-Protocol", "rili-sync/v1")
        .send_string(&payload)
        .with_context(|| format!("上传同步快照失败：{url}"))?;
    verify_push_response(response)?;
    Ok(SyncTransfer {
        endpoint: url,
        status: "已确认接收".to_string(),
        bytes: payload.len(),
    })
}

fn verify_push_response(response: ureq::Response) -> Result<()> {
    anyhow::ensure!(
        (200..300).contains(&response.status()),
        "同步服务返回 HTTP {}",
        response.status()
    );
    anyhow::ensure!(
        response.header("X-Rili-Protocol") == Some("rili-sync/v1"),
        "同步服务未确认 rili-sync/v1 协议，不能判定上传成功"
    );
    let acknowledgement: serde_json::Value =
        response.into_json().context("同步服务未返回 JSON 确认")?;
    anyhow::ensure!(
        acknowledgement
            .get("format_version")
            .and_then(|value| value.as_u64())
            == Some(1)
            && acknowledgement
                .get("accepted")
                .and_then(|value| value.as_bool())
                == Some(true),
        "同步服务未确认接收快照，不能判定上传成功"
    );
    Ok(())
}

/// 拉取远端快照。为防止误操作，调用方必须显式确认 `append=true`，导入采用本地数据库的追加策略。
pub fn pull(conn: &mut rusqlite::Connection, append: bool) -> Result<db::ImportStats> {
    anyhow::ensure!(append, "pull 会把远端快照追加到本地，确认后请加 --append");
    let url = endpoint(conn)?;
    let response = with_token(agent().get(&url))
        .set("Accept", "application/json")
        .set("X-Rili-Protocol", "rili-sync/v1")
        .call()
        .with_context(|| format!("下载同步快照失败：{url}"))?;
    anyhow::ensure!(
        (200..300).contains(&response.status()),
        "同步服务返回 HTTP {}",
        response.status()
    );
    let backup: db::LocalBackup = response.into_json().context("解析远端同步快照失败")?;
    db::import_backup(conn, &backup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn sync_endpoint_requires_tls_except_loopback() {
        assert!(validate_endpoint("https://sync.example.test/api").is_ok());
        assert!(validate_endpoint("http://127.0.0.1:8080/api").is_ok());
        assert!(validate_endpoint("http://localhost:8080/api").is_ok());
        assert!(validate_endpoint("http://sync.example.test/api").is_err());
        assert!(validate_endpoint("https://name:password@sync.example.test/api").is_err());
        assert!(validate_endpoint("https://sync.example.test/api?token=secret").is_err());
    }

    #[test]
    fn html_login_page_is_not_a_successful_upload_acknowledgement() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            for (header, body) in [
                ("Content-Type: text/html\r\n", "<html>Sign in</html>"),
                (
                    "Content-Type: application/json\r\nX-Rili-Protocol: rili-sync/v1\r\n",
                    "{\"format_version\":1,\"accepted\":true}",
                ),
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0u8; 1024];
                let _ = stream.read(&mut request).unwrap();
                write!(stream, "HTTP/1.1 200 OK\r\n{header}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        assert!(verify_push_response(agent().get(&url).call().unwrap()).is_err());
        assert!(verify_push_response(agent().get(&url).call().unwrap()).is_ok());
        server.join().unwrap();
    }
}
