//! Client side of the CLI: talks to a (local or remote) daemon over the HTTP control API.
//! `nockd deploy/ps/logs/restart/stop` are all thin wrappers over these calls.

use anyhow::{bail, Context, Result};

use crate::api::{AppStatus, DeployRequest, DeployResponse};

pub struct Client {
    base: String,
    http: reqwest::Client,
}

impl Client {
    pub fn new(host: &str, port: u16) -> Self {
        Client {
            base: format!("http://{host}:{port}"),
            http: reqwest::Client::new(),
        }
    }

    async fn check(resp: reqwest::Response) -> Result<reqwest::Response> {
        if resp.status().is_success() {
            Ok(resp)
        } else {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("daemon returned {status}: {body}");
        }
    }

    pub async fn deploy(&self, req: &DeployRequest) -> Result<DeployResponse> {
        let resp = self
            .http
            .post(format!("{}/api/apps", self.base))
            .json(req)
            .send()
            .await
            .context("connecting to daemon (is `nockd serve` running?)")?;
        Ok(Self::check(resp).await?.json().await?)
    }

    pub async fn list(&self) -> Result<Vec<AppStatus>> {
        let resp = self
            .http
            .get(format!("{}/api/apps", self.base))
            .send()
            .await
            .context("connecting to daemon (is `nockd serve` running?)")?;
        Ok(Self::check(resp).await?.json().await?)
    }

    pub async fn logs(&self, name: &str, lines: usize) -> Result<String> {
        let resp = self
            .http
            .get(format!("{}/api/apps/{name}/logs?lines={lines}", self.base))
            .send()
            .await
            .context("connecting to daemon")?;
        Ok(Self::check(resp).await?.text().await?)
    }

    pub async fn action(&self, name: &str, action: &str) -> Result<()> {
        let resp = self
            .http
            .post(format!("{}/api/apps/{name}/{action}", self.base))
            .send()
            .await
            .context("connecting to daemon")?;
        Self::check(resp).await?;
        Ok(())
    }

    pub async fn endpoints(&self) -> Result<serde_json::Value> {
        let resp = self
            .http
            .get(format!("{}/api/v1/endpoints", self.base))
            .send()
            .await
            .context("connecting to daemon")?;
        Ok(Self::check(resp).await?.json().await?)
    }

    pub async fn add_endpoint(&self, name: &str, url: &str, kind: &str) -> Result<()> {
        let resp = self
            .http
            .post(format!("{}/api/v1/endpoints", self.base))
            .json(&serde_json::json!({ "name": name, "url": url, "kind": kind }))
            .send()
            .await
            .context("connecting to daemon")?;
        Self::check(resp).await?;
        Ok(())
    }

    pub async fn remove_endpoint(&self, name: &str) -> Result<()> {
        let resp = self
            .http
            .delete(format!("{}/api/v1/endpoints/{name}", self.base))
            .send()
            .await
            .context("connecting to daemon")?;
        Self::check(resp).await?;
        Ok(())
    }
}
