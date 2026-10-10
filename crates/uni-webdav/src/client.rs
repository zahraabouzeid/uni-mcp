use async_trait::async_trait;
use reqwest::Method;
use uni_core::document::{self, View};
use uni_core::model::{Document, FileEntry};
use uni_core::{Error, Files, Result};
use url::Url;

use crate::multistatus;

const PROPFIND_BODY: &str = r#"<?xml version="1.0"?>
<d:propfind xmlns:d="DAV:"><d:prop>
<d:resourcetype/><d:getcontentlength/><d:getlastmodified/>
</d:prop></d:propfind>"#;

pub struct WebdavClient {
    http: reqwest::Client,
    service: String,
    root: Url,
    user: String,
    password: String,
}

impl WebdavClient {
    /// `service` names the store in error messages; `{user}` in `files_url`
    /// is replaced by the username.
    pub fn new(
        http: reqwest::Client,
        service: &str,
        files_url: &str,
        user: String,
        password: String,
    ) -> Result<Self> {
        let invalid =
            |why: String| Error::Config(format!("invalid WebDAV URL `{files_url}`: {why}"));
        let mut root =
            Url::parse(&files_url.replace("{user}", "")).map_err(|e| invalid(e.to_string()))?;
        if files_url.contains("{user}") {
            root.path_segments_mut()
                .map_err(|()| invalid("cannot be a base".into()))?
                .pop_if_empty()
                .pop_if_empty()
                .push(&user)
                .push("");
        }
        Ok(Self {
            http,
            service: service.to_owned(),
            root,
            user,
            password,
        })
    }

    /// The URL of a path inside the file root, and that path normalised.
    fn url_for(&self, path: &str) -> Result<(Url, String)> {
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if segments.iter().any(|s| matches!(*s, "." | "..")) {
            return Err(Error::InvalidInput(
                "path must not contain `.` or `..`".into(),
            ));
        }
        let mut url = self.root.clone();
        url.path_segments_mut()
            .map_err(|()| Error::Config("WebDAV URL cannot be a base".into()))?
            .pop_if_empty()
            .extend(&segments);
        Ok((url, segments.join("/")))
    }

    async fn send(
        &self,
        method: Method,
        url: Url,
        body: Option<&'static str>,
    ) -> Result<reqwest::Response> {
        let mut request = self
            .http
            .request(method, url)
            .basic_auth(&self.user, Some(&self.password));
        if let Some(body) = body {
            request = request
                .header("Depth", "1")
                .header("Content-Type", "application/xml")
                .body(body);
        }
        let response = request.send().await.map_err(|e| self.network(e))?;
        match response.status().as_u16() {
            401 | 403 => Err(Error::Auth {
                service: self.service.clone(),
            }),
            404 => Err(Error::InvalidInput("no such file or folder".into())),
            _ => response.error_for_status().map_err(|e| self.network(e)),
        }
    }

    fn network(&self, e: reqwest::Error) -> Error {
        Error::Network {
            service: self.service.clone(),
            message: e.without_url().to_string(),
        }
    }
}

#[async_trait]
impl Files for WebdavClient {
    async fn list(&self, path: &str) -> Result<Vec<FileEntry>> {
        let (url, listed) = self.url_for(path)?;
        let propfind = Method::from_bytes(b"PROPFIND").expect("PROPFIND is a valid method");
        let xml = self
            .send(propfind, url, Some(PROPFIND_BODY))
            .await?
            .text()
            .await
            .map_err(|e| self.network(e))?;
        multistatus::parse(&xml, self.root.path(), &listed).map_err(|message| Error::Protocol {
            service: self.service.clone(),
            message,
        })
    }

    async fn read(&self, path: &str, pages: Option<&str>, view: View) -> Result<Document> {
        let (url, _) = self.url_for(path)?;
        let response = self.send(Method::GET, url, None).await?;
        document::check_size(response.content_length().unwrap_or(0))?;
        let bytes = response.bytes().await.map_err(|e| self.network(e))?;
        document::read(&bytes, pages, view)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> WebdavClient {
        WebdavClient::new(
            reqwest::Client::new(),
            "cloud",
            "https://cloud.example/remote.php/dav/files/{user}/",
            "ab123456@uni.example".into(),
            "app-password".into(),
        )
        .expect("client")
    }

    #[test]
    fn username_becomes_one_path_segment() {
        assert_eq!(
            client().root.path(),
            "/remote.php/dav/files/ab123456@uni.example/"
        );
    }

    #[test]
    fn paths_are_encoded_and_stay_inside_the_root() {
        let c = client();
        let (url, listed) = c.url_for("/Uni/Analysis I/").expect("url");
        assert_eq!(
            url.path(),
            "/remote.php/dav/files/ab123456@uni.example/Uni/Analysis%20I"
        );
        assert_eq!(listed, "Uni/Analysis I");
        assert!(c.url_for("Uni/../../other-user").is_err());
    }
}
