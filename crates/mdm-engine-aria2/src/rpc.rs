//! JSON-RPC client for a local aria2c daemon.

use std::time::Duration;

use serde_json::{json, Value};
use tracing::warn;

#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    #[error("http: {0}")]
    Http(String),
    #[error("rpc: {0}")]
    Rpc(String),
}

pub struct Aria2Rpc {
    url: String,
    secret: String,
    http: reqwest::Client,
}

impl Aria2Rpc {
    pub fn new(rpc_addr: String, secret: String) -> Aria2Rpc {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();
        Aria2Rpc {
            url: format!("http://{rpc_addr}/jsonrpc"),
            secret,
            http,
        }
    }

    async fn call(&self, method: &str, mut params: Vec<Value>) -> Result<Value, RpcError> {
        params.insert(0, json!(format!("token:{}", self.secret)));
        let body = json!({
            "jsonrpc": "2.0",
            "id": format!("mdm-{method}"),
            "method": method,
            "params": params,
        });
        let resp = self
            .http
            .post(&self.url)
            .json(&body)
            .send()
            .await
            .map_err(|e| RpcError::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(RpcError::Http(format!("status {}", resp.status())));
        }
        let v: Value = resp.json().await.map_err(|e| RpcError::Http(e.to_string()))?;
        if let Some(err) = v.get("error") {
            return Err(RpcError::Rpc(err.to_string()));
        }
        Ok(v.get("result").cloned().unwrap_or(Value::Null))
    }

    /// Batched `[active, waiting, stopped]` snapshot for the poll loop.
    #[allow(dead_code)]
    pub async fn tell_all(
        &self,
    ) -> Result<(Vec<Value>, Vec<Value>, Vec<Value>), RpcError> {
        let body = json!([
            { "jsonrpc": "2.0", "id": "b-active", "method": "aria2.tellActive", "params": [json!(format!("token:{}", self.secret)), keys()] },
            { "jsonrpc": "2.0", "id": "b-waiting", "method": "aria2.tellWaiting", "params": [json!(format!("token:{}", self.secret)), 0, 100, keys()] },
            { "jsonrpc": "2.0", "id": "b-stopped", "method": "aria2.tellStopped", "params": [json!(format!("token:{}", self.secret)), 0, 100, keys()] },
        ]);
        let resp = self
            .http
            .post(&self.url)
            .json(&body)
            .send()
            .await
            .map_err(|e| RpcError::Http(e.to_string()))?;
        let v: Vec<Value> = resp.json().await.map_err(|e| RpcError::Http(e.to_string()))?;
        let pull = |id: &str| -> Vec<Value> {
            v.iter()
                .find(|item| item.get("id").and_then(|i| i.as_str()) == Some(id))
                .and_then(|item| item.get("result").cloned())
                .map(|r| r.as_array().cloned().unwrap_or_default())
                .unwrap_or_default()
        };
        Ok((pull("b-active"), pull("b-waiting"), pull("b-stopped")))
    }

    pub async fn add_uri(
        &self,
        uris: Vec<String>,
        dir: &str,
        extra_opts: Value,
    ) -> Result<String, RpcError> {
        let mut opts = extra_opts;
        opts["dir"] = json!(dir);
        let gid = self
            .call("aria2.addUri", vec![json!(uris), opts])
            .await?
            .as_str()
            .map(String::from)
            .ok_or_else(|| RpcError::Rpc("no gid returned".into()))?;
        Ok(gid)
    }

    pub async fn add_torrent(
        &self,
        torrent_b64: &str,
        dir: &str,
        extra_opts: Value,
    ) -> Result<String, RpcError> {
        let mut opts = extra_opts;
        opts["dir"] = json!(dir);
        let gid = self
            .call(
                "aria2.addTorrent",
                vec![json!(torrent_b64), json!(Vec::<String>::new()), opts],
            )
            .await?
            .as_str()
            .map(String::from)
            .ok_or_else(|| RpcError::Rpc("no gid returned".into()))?;
        Ok(gid)
    }

    pub async fn tell_status(&self, gid: &str) -> Result<Value, RpcError> {
        self.call("aria2.tellStatus", vec![json!(gid), keys()]).await
    }

    pub async fn force_pause(&self, gid: &str) -> Result<(), RpcError> {
        let r = self.call("aria2.forcePause", vec![json!(gid)]).await;
        match r {
            Ok(_) => Ok(()),
            Err(RpcError::Rpc(e)) => {
                // Unknown gid: treat as gone.
                if e.contains("GID.is.not.recognized") || e.contains("cannot be paused") {
                    Ok(())
                } else {
                    Err(RpcError::Rpc(e))
                }
            }
            Err(e) => Err(e),
        }
    }

    pub async fn unpause(&self, gid: &str) -> Result<(), RpcError> {
        let r = self.call("aria2.unpause", vec![json!(gid)]).await;
        match r {
            Ok(_) => Ok(()),
            Err(RpcError::Rpc(e)) => {
                if e.contains("GID.is.not.recognized") {
                    warn!("unpause: unknown gid {gid}");
                    Ok(())
                } else {
                    Err(RpcError::Rpc(e))
                }
            }
            Err(e) => Err(e),
        }
    }

    pub async fn force_remove(&self, gid: &str) -> Result<(), RpcError> {
        let _ = self.call("aria2.forceRemove", vec![json!(gid)]).await;
        Ok(())
    }

    pub async fn remove_download_result(&self, gid: &str) -> Result<(), RpcError> {
        let _ = self.call("aria2.removeDownloadResult", vec![json!(gid)]).await;
        Ok(())
    }

    pub async fn version(&self) -> Result<String, RpcError> {
        let v = self.call("aria2.getVersion", vec![]).await?;
        Ok(v.get("version").and_then(|x| x.as_str()).unwrap_or("?").to_string())
    }

    pub async fn shutdown(self) -> Result<(), RpcError> {
        let _ = self.call("aria2.shutdown", vec![]).await;
        Ok(())
    }
}

fn keys() -> Value {
    json!([
        "gid",
        "status",
        "totalLength",
        "completedLength",
        "downloadSpeed",
        "uploadSpeed",
        "connections",
        "numSeeders",
        "seeder",
        "followedBy",
        "files",
        "errorMessage",
        "errorCode",
        "seedingTime",
    ])
}
