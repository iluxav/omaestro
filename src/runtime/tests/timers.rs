//! `om.after` and `om.at`.

use super::*;

#[tokio::test(start_paused = true)]
async fn after_fires_once_and_forgets_itself() {
    let h = Harness::start(&[(
        "init.lua",
        "om.after('2s', function() om.notify('later') end)\nhandle = om.after('5s', function() om.notify('never') end)",
    )])
    .await;
    assert_eq!(h.status().await.triggers, 2);
    assert_eq!(h.eval("return handle:cancel()").await.unwrap(), ["true"]);
    assert_eq!(h.status().await.triggers, 1);

    tokio::time::sleep(Duration::from_millis(2100)).await;
    h.settle().await;
    assert_eq!(h.titles(), ["later"]);
    assert_eq!(h.status().await.triggers, 0, "a fired one-shot is gone");

    tokio::time::sleep(Duration::from_secs(10)).await;
    h.settle().await;
    assert_eq!(h.titles(), ["later"]);
}

#[tokio::test(start_paused = true)]
async fn after_registered_from_a_handler_runs() {
    let h = Harness::start(&[(
        "init.lua",
        "om.trigger('go', function() om.after('1s', function() om.notify('chained') end) end)",
    )])
    .await;
    assert!(h.trigger("go").await.ok);
    h.settle().await;
    tokio::time::sleep(Duration::from_millis(1100)).await;
    h.settle().await;
    assert_eq!(h.titles(), ["chained"]);
}

#[tokio::test(start_paused = true)]
async fn at_fires_at_the_clock_time_and_arms_again() {
    // Two minutes from now, on the wall clock the rule sees.
    let h = Harness::start(&[(
        "init.lua",
        "local t = os.date('*t', os.time() + 120)\n\
         om.at(string.format('%02d:%02d', t.hour, t.min), function() om.notify('alarm') end)",
    )])
    .await;
    let rows: Vec<TriggerRow> =
        serde_json::from_value(h.ask(Request::List).await.data.unwrap()).unwrap();
    assert_eq!(rows[0].kind, "at");

    // The target is the start of that minute: between 61 and 120 s away.
    tokio::time::sleep(Duration::from_secs(59)).await;
    h.settle().await;
    assert_eq!(h.titles().len(), 0);
    tokio::time::sleep(Duration::from_secs(62)).await;
    h.settle().await;
    assert_eq!(h.titles(), ["alarm"]);
    // Still registered, armed for the next day (the wall clock has barely
    // moved under the paused runtime, so "next" is again about two minutes).
    assert_eq!(h.status().await.triggers, 1);
    tokio::time::sleep(Duration::from_secs(125)).await;
    h.settle().await;
    assert!(h.titles().len() >= 2, "{:?}", h.titles());
}

#[tokio::test]
async fn bad_times_fail_at_the_rule_line() {
    let h = Harness::start(&[("init.lua", "\nom.at('25:00', function() end)")]).await;
    assert_eq!(
        h.errors(),
        ["init.lua:2: om.at: '25:00': hours go to 23 and minutes to 59 (no rules loaded)"]
    );
    let h = Harness::start(&[("init.lua", "om.after('x', function() end)")]).await;
    assert_eq!(
        h.errors(),
        ["init.lua:1: om.after: 'x': unknown unit 'x', use s, m or h (no rules loaded)"]
    );
}
