//! The model: one HTTP request to a chat endpoint. Ollama's `/api/chat` by
//! default; an OpenAI-compatible `/v1/chat/completions` works the same way.
//! Nothing is sent anywhere except the endpoint the user configured.

use serde_json::{Value, json};

use super::{BackendError, BoxFuture, ChatRequest, Llm, Result};

pub struct HttpLlm {
    client: reqwest::Client,
}

impl HttpLlm {
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder().build().map_err(|err| {
            BackendError::Other(format!("could not set up the HTTP client: {err}"))
        })?;
        Ok(Self { client })
    }
}

impl Llm for HttpLlm {
    fn chat<'a>(&'a self, request: &'a ChatRequest) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let mut call = self
                .client
                .post(&request.endpoint)
                .timeout(request.timeout)
                .json(&request_body(request));
            if let Some(key) = &request.api_key {
                call = call.bearer_auth(key);
            }
            let failed = |err: reqwest::Error| {
                BackendError::Other(if err.is_timeout() {
                    format!(
                        "model endpoint {} did not answer within {}s",
                        host(&request.endpoint),
                        request.timeout.as_secs()
                    )
                } else if err.is_connect() {
                    down(&request.endpoint)
                } else {
                    format!("model endpoint {}: {err}", host(&request.endpoint))
                })
            };
            let response = call.send().await.map_err(failed)?;
            let status = response.status().as_u16();
            let body = response.text().await.map_err(failed)?;
            parse_response(request, status, &body).map_err(BackendError::Other)
        })
    }
}

/// The same body suits Ollama and OpenAI-compatible servers.
fn request_body(request: &ChatRequest) -> Value {
    let mut messages = Vec::new();
    if let Some(system) = &request.system {
        messages.push(json!({"role": "system", "content": system}));
    }
    messages.push(json!({"role": "user", "content": request.prompt}));
    json!({"model": request.model, "messages": messages, "stream": false})
}

/// `127.0.0.1:11434` out of `http://127.0.0.1:11434/api/chat`.
fn host(endpoint: &str) -> &str {
    let rest = endpoint
        .split_once("://")
        .map_or(endpoint, |(_, rest)| rest);
    rest.split('/').next().unwrap_or(rest)
}

fn down(endpoint: &str) -> String {
    let host = host(endpoint);
    if host.ends_with(":11434") {
        format!("model endpoint {host} is down, start ollama")
    } else {
        format!("model endpoint {host} is down")
    }
}

fn parse_response(
    request: &ChatRequest,
    status: u16,
    body: &str,
) -> std::result::Result<String, String> {
    let host = host(&request.endpoint);
    let json: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    if !(200..300).contains(&status) {
        // Ollama: {"error": "..."}; OpenAI: {"error": {"message": "..."}}.
        let reason = json["error"]
            .as_str()
            .or_else(|| json["error"]["message"].as_str())
            .unwrap_or_else(|| body.trim());
        let hint = if status == 404 && reason.contains("not found") {
            format!(
                "; pull it with `ollama pull {}` or set the model name in omaestro.toml",
                request.model
            )
        } else {
            String::new()
        };
        return Err(format!(
            "model endpoint {host} answered {status}: {reason}{hint}"
        ));
    }
    // Ollama: message.content; OpenAI: choices[0].message.content.
    json["message"]["content"]
        .as_str()
        .or_else(|| json["choices"][0]["message"]["content"].as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("model endpoint {host} sent a response with no text in it"))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn request(system: Option<&str>) -> ChatRequest {
        ChatRequest {
            endpoint: "http://127.0.0.1:11434/api/chat".into(),
            model: "llama3.2".into(),
            system: system.map(str::to_string),
            prompt: "rewrite this".into(),
            api_key: None,
            timeout: Duration::from_secs(60),
        }
    }

    #[test]
    fn body_is_a_non_streaming_chat() {
        assert_eq!(
            request_body(&request(None)),
            json!({"model": "llama3.2", "stream": false,
                   "messages": [{"role": "user", "content": "rewrite this"}]})
        );
        assert_eq!(
            request_body(&request(Some("be brief")))["messages"],
            json!([{"role": "system", "content": "be brief"}, {"role": "user", "content": "rewrite this"}])
        );
    }

    #[test]
    fn text_from_ollama_and_openai_shapes() {
        let req = request(None);
        let ollama = r#"{"model": "llama3.2", "message": {"role": "assistant", "content": "done"}, "done": true}"#;
        assert_eq!(parse_response(&req, 200, ollama).unwrap(), "done");
        let openai = r#"{"choices": [{"index": 0, "message": {"role": "assistant", "content": "done too"}}]}"#;
        assert_eq!(parse_response(&req, 200, openai).unwrap(), "done too");
        assert_eq!(
            parse_response(&req, 200, "{}").unwrap_err(),
            "model endpoint 127.0.0.1:11434 sent a response with no text in it"
        );
    }

    #[test]
    fn errors_say_what_to_do() {
        let req = request(None);
        assert_eq!(
            parse_response(&req, 404, r#"{"error": "model 'llama3.2' not found"}"#).unwrap_err(),
            "model endpoint 127.0.0.1:11434 answered 404: model 'llama3.2' not found; pull it with \
             `ollama pull llama3.2` or set the model name in omaestro.toml"
        );
        assert_eq!(
            parse_response(&req, 401, r#"{"error": {"message": "bad key"}}"#).unwrap_err(),
            "model endpoint 127.0.0.1:11434 answered 401: bad key"
        );
        assert_eq!(
            parse_response(&req, 502, "Bad Gateway\n").unwrap_err(),
            "model endpoint 127.0.0.1:11434 answered 502: Bad Gateway"
        );
    }

    #[test]
    fn down_endpoint_message() {
        assert_eq!(
            down("http://127.0.0.1:11434/api/chat"),
            "model endpoint 127.0.0.1:11434 is down, start ollama"
        );
        assert_eq!(
            down("https://api.example.com/v1/chat/completions"),
            "model endpoint api.example.com is down"
        );
        assert_eq!(host("localhost:8080"), "localhost:8080");
    }
}
