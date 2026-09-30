//! Desktop notifications through `notify-send`.

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
}
