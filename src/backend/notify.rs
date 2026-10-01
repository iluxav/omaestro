//! Desktop notifications through `notify-send`.

use std::time::Duration;

use super::{BoxFuture, Notifier, Result, run};

pub struct NotifySend;

impl Notifier for NotifySend {
    fn notify<'a>(&'a self, title: &'a str, body: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            run::run(
                "notify-send",
                &["--app-name", "omaestro", "--", title, body],
            )
            .await?;
            Ok(())
        })
    }

    fn ask<'a>(
        &'a self,
        title: &'a str,
        body: &'a str,
        actions: &'a [(String, String)],
        timeout: Option<Duration>,
    ) -> BoxFuture<'a, Result<Option<String>>> {
        Box::pin(async move {
            // `--action` makes notify-send wait and print the chosen key.
            let mut args = vec!["--app-name".to_string(), "omaestro".to_string()];
            for (key, label) in actions {
                args.push("--action".to_string());
                args.push(format!("{key}={label}"));
            }
            if let Some(timeout) = timeout {
                args.push("--expire-time".to_string());
                args.push(timeout.as_millis().to_string());
            }
            args.push("--".to_string());
            args.push(title.to_string());
            args.push(body.to_string());
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            let output = run::run("notify-send", &args).await?;
            let chosen = output.stdout_text().trim().to_string();
            Ok((!chosen.is_empty()).then_some(chosen))
        })
    }
}
