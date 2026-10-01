//! `om.http(url, opts)`: one HTTP request, the response as a table.

use std::time::Duration;

use mlua::{Error, Lua, Result, Table, Value};

use super::Context;
use crate::backend::HttpRequest;
use crate::luajson::{from_json, to_json};

pub fn install(lua: &Lua, om: &Table, cx: &Context) -> Result<()> {
    let http = cx.backends.http.clone();
    om.set(
        "http",
        lua.create_async_function(move |lua, (url, options): (String, Option<Table>)| {
            let http = http.clone();
            async move {
                let mut request = HttpRequest {
                    method: "GET".to_string(),
                    url,
                    headers: Vec::new(),
                    body: None,
                    timeout: None,
                };
                let mut want_json = false;
                if let Some(options) = options {
                    if let Some(method) = options.get::<Option<String>>("method")? {
                        request.method = method.to_uppercase();
                    }
                    if let Some(headers) = options.get::<Option<Table>>("headers")? {
                        for pair in headers.pairs::<String, String>() {
                            let (name, value) = pair?;
                            request.headers.push((name, value));
                        }
                    }
                    if let Some(timeout) = options.get::<Option<f64>>("timeout")? {
                        request.timeout = Some(Duration::from_secs_f64(timeout));
                    }
                    // `json = value` sends it as JSON and reads the answer as JSON.
                    let json: Value = options.get("json")?;
                    if json != Value::Nil {
                        want_json = true;
                        if json != Value::Boolean(true) {
                            request.body = Some(to_json(&json)?.to_string());
                            request
                                .headers
                                .push(("content-type".to_string(), "application/json".to_string()));
                            if request.method == "GET" {
                                request.method = "POST".to_string();
                            }
                        }
                    }
                    if let Some(body) = options.get::<Option<String>>("body")? {
                        request.body = Some(body);
                    }
                }
                let response = http.request(&request).await.map_err(Error::external)?;
                let table = lua.create_table()?;
                table.set("status", response.status)?;
                table.set("ok", (200..300).contains(&response.status))?;
                table.set("body", response.body.as_str())?;
                let headers = lua.create_table()?;
                let mut is_json = false;
                for (name, value) in &response.headers {
                    if name == "content-type" && value.contains("json") {
                        is_json = true;
                    }
                    headers.set(name.as_str(), value.as_str())?;
                }
                table.set("headers", headers)?;
                if want_json || is_json {
                    match serde_json::from_str::<serde_json::Value>(&response.body) {
                        Ok(json) => table.set("json", from_json(&lua, &json)?)?,
                        Err(err) if want_json => {
                            return Err(Error::runtime(format!(
                                "http: the answer is not JSON: {err}"
                            )));
                        }
                        Err(_) => {}
                    }
                }
                Ok(table)
            }
        })?,
    )
}
