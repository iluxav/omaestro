//! `om.shell`, `om.prompt`, `om.clipboard`, `om.set_clipboard`, `om.every`.

use super::*;
use crate::backend::ClipContent;

#[tokio::test]
async fn shell_returns_stdout_without_the_trailing_newline() {
    let h = Harness::start(&[]).await;
    h.fakes.shell.answer("hello\n");
    assert_eq!(
        h.eval("return om.shell('echo hello')").await.unwrap(),
        ["hello"]
    );
    assert_eq!(h.fakes.journal.entries(), ["sh echo hello"]);
    // Nothing queued: success, empty output.
    assert_eq!(h.eval("return om.shell('true')").await.unwrap(), [""]);
}

#[tokio::test]
async fn shell_failures_carry_the_exit_code_and_stderr() {
    let h = Harness::start(&[]).await;
    h.fakes.shell.fail(
        Some(2),
        "ls: cannot access 'nope': No such file or directory\n",
    );
    assert_eq!(
        h.eval("local out = om.shell('ls nope') return out")
            .await
            .unwrap_err(),
        "eval:1: `ls nope` exited with 2: ls: cannot access 'nope': No such file or directory"
    );
    h.fakes.shell.fail(None, "");
    assert_eq!(
        h.eval("local out = om.shell('sleep 9') return out")
            .await
            .unwrap_err(),
        "eval:1: `sleep 9` was killed"
    );
}

#[tokio::test]
async fn prompt_runs_the_configured_command_with_the_label_quoted() {
    let config = "prompt_command = \"mymenu --ask {label}\"\n";
    let h = Harness::start(&[("omaestro.toml", config)]).await;
    h.fakes.shell.answer("what I typed\n");
    assert_eq!(
        h.eval("return om.prompt(\"What's up?\")").await.unwrap(),
        ["what I typed"]
    );
    assert_eq!(
        h.fakes.journal.entries(),
        [r"sh mymenu --ask 'What'\''s up?'"]
    );

    // Cancelled (non-zero exit) or nothing typed: nil, not an error.
    h.fakes.shell.fail(Some(1), "");
    assert_eq!(h.eval("return om.prompt('x')").await.unwrap(), ["nil"]);
    h.fakes.shell.answer("\n");
    assert_eq!(h.eval("return om.prompt('x')").await.unwrap(), ["nil"]);
    h.fakes.shell.answer("no label\n");
    assert_eq!(h.eval("return om.prompt()").await.unwrap(), ["no label"]);
}

#[tokio::test]
async fn clipboard_reads_text_and_writes_text() {
    let h = Harness::start(&[]).await;
    assert_eq!(h.eval("return om.clipboard()").await.unwrap(), [""]);
    h.eval("om.set_clipboard('copied by a rule')")
        .await
        .unwrap();
    assert_eq!(
        h.eval("return om.clipboard()").await.unwrap(),
        ["copied by a rule"]
    );
    assert_eq!(
        h.fakes.clipboard.content(),
        Some(ClipContent::text("copied by a rule"))
    );

    // An image reads as empty text.
    h.fakes.clipboard.copy(ClipContent {
        mime: "image/png".into(),
        data: vec![1, 2, 3],
    });
    assert_eq!(h.eval("return om.clipboard()").await.unwrap(), [""]);
}

#[tokio::test(start_paused = true)]
async fn every_fires_on_its_interval_and_stops_when_removed() {
    let h = Harness::start(&[(
        "init.lua",
        "ticks = 0\nhandle = om.every('2s', function() ticks = ticks + 1 om.notify('tick') end)",
    )])
    .await;
    let rows: Vec<TriggerRow> =
        serde_json::from_value(h.ask(Request::List).await.data.unwrap()).unwrap();
    assert_eq!(rows[0].kind, "every");
    assert_eq!(rows[0].origin, "init.lua:2");

    tokio::time::sleep(Duration::from_millis(1900)).await;
    h.settle().await;
    assert_eq!(h.titles().len(), 0);
    tokio::time::sleep(Duration::from_millis(200)).await;
    h.settle().await;
    assert_eq!(h.titles(), ["tick"]);
    tokio::time::sleep(Duration::from_secs(4)).await;
    h.settle().await;
    assert_eq!(h.titles().len(), 3);

    assert_eq!(h.eval("return handle:remove()").await.unwrap(), ["true"]);
    tokio::time::sleep(Duration::from_secs(10)).await;
    h.settle().await;
    assert_eq!(h.titles().len(), 3);
}

#[tokio::test(start_paused = true)]
async fn a_slow_timer_handler_skips_ticks_instead_of_piling_up() {
    let h = Harness::start(&[(
        "init.lua",
        "om.every('1s', function() om.notify('tick') end)",
    )])
    .await;
    // The first tick's handler parks inside notify for a while.
    h.fakes.notifier.hold_next(1);
    tokio::time::sleep(Duration::from_millis(3500)).await;
    assert_eq!(h.titles(), ["tick"]);
    h.fakes.notifier.release(1);
    tokio::time::sleep(Duration::from_millis(1000)).await;
    h.settle().await;
    // Ticks 2 and 3 were skipped; tick 4 ran.
    assert_eq!(h.titles().len(), 2);
}

#[tokio::test(start_paused = true)]
async fn reload_restarts_timers_from_the_new_rules() {
    let h = Harness::start(&[(
        "init.lua",
        "om.every('1s', function() om.notify('old') end)",
    )])
    .await;
    tokio::time::sleep(Duration::from_millis(1100)).await;
    h.settle().await;
    assert_eq!(h.titles(), ["old"]);

    assert!(
        h.save(&[(
            "init.lua",
            "om.every('1s', function() om.notify('new') end)"
        )])
        .await
        .ok
    );
    tokio::time::sleep(Duration::from_millis(2200)).await;
    h.settle().await;
    let titles = h.titles();
    assert!(titles[1..].iter().all(|t| t == "new"), "{titles:?}");
    assert_eq!(titles.len(), 3);
}

#[tokio::test]
async fn bad_intervals_fail_at_the_rule_line() {
    let h = Harness::start(&[("init.lua", "\nom.every('soon', function() end)")]).await;
    assert_eq!(
        h.errors(),
        ["init.lua:2: om.every: 'soon': a number must come before 's' (no rules loaded)"]
    );
}
