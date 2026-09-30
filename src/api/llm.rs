//! `om.llm(prompt, {model=, system=})`: ask the configured model.

use std::time::Duration;

use mlua::{Error, Lua, Result, Table};

use super::Context;
use crate::backend::ChatRequest;

pub fn install(lua: &Lua, om: &Table, cx: &Context) -> Result<()> {
    let llm = cx.backends.llm.clone();
    let config = cx.config.clone();
    om.set(
        "llm",
        lua.create_async_function(move |_, (prompt, options): (String, Option<Table>)| {
            let llm = llm.clone();
            let config = config.clone();
            async move {
                let (model, system) = match &options {
                    Some(options) => (
                        options.get::<Option<String>>("model")?,
                        options.get::<Option<String>>("system")?,
                    ),
                    None => (None, None),
                };
                let api_key = match &config.model.api_key_env {
                    Some(name) => Some(std::env::var(name).map_err(|_| {
                        Error::runtime(format!(
                            "omaestro.toml says the model key is in ${name}, but that variable is not set"
                        ))
                    })?),
                    None => None,
                };
                let request = ChatRequest {
                    endpoint: config.model.endpoint.clone(),
                    model: model.unwrap_or_else(|| config.model.name.clone()),
                    system,
                    prompt,
                    api_key,
                    timeout: Duration::from_secs(config.model.timeout_secs),
                };
                let started = std::time::Instant::now();
                let answer = llm.chat(&request).await.map_err(Error::external)?;
                tracing::debug!(
                    "llm: {} answered {} bytes to {} bytes in {:.1}s",
                    request.model,
                    answer.len(),
                    request.prompt.len(),
                    started.elapsed().as_secs_f64()
                );
                Ok(answer)
            }
        })?,
    )
}
