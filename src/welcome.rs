//! The daemon's first start on a machine (no `~/.config/omaestro` yet): an
//! `init.lua` that says where things go, and one notification offering the
//! starter plugins. Nothing is installed unless its button is pressed, and
//! the offer is not repeated: the config directory exists from then on.

use std::future::Future;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::backend::Notifier;
use crate::plugins::STARTERS;

/// How long the offer waits for an answer before it lets go.
const OFFER_TIMEOUT: Duration = Duration::from_secs(600);

pub const INIT_LUA: &str = r#"-- omaestro: your rules. This file loads first, then rules.d/*.lua in name
-- order. Saving any of them reloads the rules; a mistake shows up as a
-- notification with the file and line, and the previous rules keep running.
--
-- Plugins to start with:
--   om plugin available                              what there is
--   om plugin add panel window-halves text-tools      the starter set
--
-- A rule of your own:
--
--   om.hotkey("SUPER + ALT + H", function()
--     om.notify("omaestro", "hello from a rule")
--   end)
--
-- om --help, and https://github.com/iluxav/omaestro for the rest.
"#;

/// Writes the commented `init.lua` into a new config directory.
pub fn write_init(config_dir: &Path) -> Result<()> {
    let path = config_dir.join("init.lua");
    if path.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(config_dir.join("rules.d"))
        .with_context(|| format!("creating {}", config_dir.display()))?;
    std::fs::write(&path, INIT_LUA).with_context(|| format!("writing {}", path.display()))
}

/// Asks once whether to install the starter plugins, and does it on yes.
/// `install` gets the names and says what went wrong, if anything.
pub async fn offer<F, Fut>(notifier: &dyn Notifier, install: F)
where
    F: FnOnce(Vec<String>) -> Fut,
    Fut: Future<Output = Result<(), String>>,
{
    let actions = [
        ("starter".to_string(), "Install starter plugins".to_string()),
        ("later".to_string(), "Not now".to_string()),
    ];
    let body = format!(
        "Install the starter plugins ({})? They add SUPER+ALT+O for the rules panel, \
         CTRL+ALT+arrows for window halves and SUPER+ALT+D for today's date. \
         Later: om plugin available",
        STARTERS.join(", ")
    );
    let answer = match notifier
        .ask("omaestro is running", &body, &actions, Some(OFFER_TIMEOUT))
        .await
    {
        Ok(answer) => answer,
        Err(err) => {
            tracing::warn!("could not offer the starter plugins: {err}");
            return;
        }
    };
    if answer.as_deref() != Some("starter") {
        return;
    }
    let names: Vec<String> = STARTERS.iter().map(|name| name.to_string()).collect();
    let (title, body) = match install(names).await {
        Ok(()) => (
            "omaestro",
            "Starter plugins installed: SUPER+ALT+O opens the rules panel".to_string(),
        ),
        Err(err) => (
            "omaestro",
            format!(
                "Could not install the starter plugins: {err}. Try: om plugin add {}",
                STARTERS.join(" ")
            ),
        ),
    };
    if let Err(err) = notifier.notify(title, &body).await {
        tracing::warn!("could not notify: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::fake::FakeNotifier;
    use std::sync::{Arc, Mutex};

    async fn run(answer: &str, result: Result<(), String>) -> (FakeNotifier, Option<Vec<String>>) {
        let notifier = FakeNotifier::default();
        notifier.choose(answer);
        let got = Arc::new(Mutex::new(None));
        let seen = got.clone();
        offer(&notifier, move |names| async move {
            *seen.lock().unwrap() = Some(names);
            result
        })
        .await;
        let names = got.lock().unwrap().clone();
        (notifier, names)
    }

    #[tokio::test]
    async fn yes_installs_the_starters_and_says_so() {
        let (notifier, names) = run("starter", Ok(())).await;
        assert_eq!(names.unwrap(), ["panel", "window-halves", "text-tools"]);
        let asked = notifier.asked();
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].0, "omaestro is running");
        assert!(asked[0].1.contains("om plugin available"));
        assert_eq!(
            notifier.sent(),
            [(
                "omaestro".to_string(),
                "Starter plugins installed: SUPER+ALT+O opens the rules panel".to_string()
            )]
        );
    }

    #[tokio::test]
    async fn no_or_dismissed_installs_nothing() {
        for answer in ["later", ""] {
            let (notifier, names) = run(answer, Ok(())).await;
            assert_eq!(names, None, "{answer:?}");
            assert!(notifier.sent().is_empty());
        }
    }

    #[tokio::test]
    async fn a_failed_install_says_what_to_run() {
        let (notifier, _) = run("starter", Err("offline".to_string())).await;
        let sent = notifier.sent();
        assert!(
            sent[0]
                .1
                .starts_with("Could not install the starter plugins: offline."),
            "{sent:?}"
        );
        assert!(
            sent[0]
                .1
                .ends_with("om plugin add panel window-halves text-tools")
        );
    }

    #[test]
    fn init_lua_is_written_once() {
        let dir = crate::testutil::TempDir::new("welcome");
        let config = dir.path().join("omaestro");
        write_init(&config).unwrap();
        assert_eq!(
            std::fs::read_to_string(config.join("init.lua")).unwrap(),
            INIT_LUA
        );
        assert!(config.join("rules.d").is_dir());
        std::fs::write(config.join("init.lua"), "-- mine").unwrap();
        write_init(&config).unwrap();
        assert_eq!(
            std::fs::read_to_string(config.join("init.lua")).unwrap(),
            "-- mine"
        );
    }
}
