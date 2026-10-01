//! HTTP requests for `om.http`, on the same reqwest client type the model
//! backend uses.

use std::time::Duration;

use super::{BackendError, BoxFuture, Http, HttpRequest, HttpResponse, Result};

pub struct ReqwestHttp {
    client: reqwest::Client,
}

impl ReqwestHttp {
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder().build().map_err(|err| {
            BackendError::Other(format!("could not set up the HTTP client: {err}"))
        })?;
        Ok(Self { client })
    }
}

impl Http for ReqwestHttp {
    fn request<'a>(&'a self, request: &'a HttpRequest) -> BoxFuture<'a, Result<HttpResponse>> {
        Box::pin(async move {
            let method = reqwest::Method::from_bytes(request.method.as_bytes()).map_err(|_| {
                BackendError::Other(format!("http: unknown method '{}'", request.method))
            })?;
            let mut call = self
                .client
                .request(method, &request.url)
                .timeout(request.timeout.unwrap_or(Duration::from_secs(30)));
            for (name, value) in &request.headers {
                call = call.header(name, value);
            }
            if let Some(body) = &request.body {
                call = call.body(body.clone());
            }
            let response = call.send().await.map_err(|err| {
                BackendError::Other(if err.is_timeout() {
                    format!("http: {} did not answer in time", request.url)
                } else if err.is_connect() {
                    format!("http: could not connect to {}", request.url)
                } else {
                    format!("http: {err}")
                })
            })?;
            let status = response.status().as_u16();
            let headers = response
                .headers()
                .iter()
                .map(|(name, value)| {
                    (
                        name.as_str().to_string(),
                        String::from_utf8_lossy(value.as_bytes()).into_owned(),
                    )
                })
                .collect();
            let body = response
                .text()
                .await
                .map_err(|err| BackendError::Other(format!("http: reading the response: {err}")))?;
            Ok(HttpResponse {
                status,
                headers,
                body,
            })
        })
    }
}
