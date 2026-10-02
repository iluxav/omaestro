//! `om.busy`: a notification that lives as long as the handler run.

use super::*;

#[tokio::test]
async fn busy_is_replaced_and_closed_when_the_handler_ends() {
    let h = Harness::start(&[(
        "init.lua",
        "om.trigger('work', function()\n\
           om.busy('Rewriting…')\n\
           om.busy('Rewriting…', 'almost there')\n\
         end)",
    )])
    .await;
    assert!(h.trigger("work").await.ok);
    h.settle().await;
    assert_eq!(
        h.fakes.notifier.busy(),
        [
            (1, "Rewriting…".to_string(), String::new(), None),
            (
                1,
                "Rewriting…".to_string(),
                "almost there".to_string(),
                Some(1)
            ),
        ]
    );
    assert_eq!(h.fakes.notifier.closed(), [1]);
}

#[tokio::test]
async fn busy_is_closed_before_the_error_shows() {
    let h = Harness::start(&[(
        "rules.d/foo.lua",
        "om.trigger('work', function()\n  om.busy('Working')\n  error('model down')\nend)",
    )])
    .await;
    assert!(h.trigger("work").await.ok);
    h.settle().await;
    assert_eq!(h.fakes.notifier.closed(), [1]);
    assert_eq!(h.errors(), ["rules.d/foo.lua:3: model down"]);
}

#[tokio::test]
async fn busy_nil_closes_early_and_each_run_gets_its_own() {
    let h = Harness::start(&[(
        "init.lua",
        "om.trigger('work', function()\n\
           om.busy('Working')\n\
           om.busy()\n\
           om.notify('done')\n\
         end)",
    )])
    .await;
    assert!(h.trigger("work").await.ok);
    h.settle().await;
    assert!(h.trigger("work").await.ok);
    h.settle().await;
    let ids: Vec<u32> = h.fakes.notifier.busy().iter().map(|b| b.0).collect();
    assert_eq!(ids, [1, 2]);
    // Closed once each, by om.busy(); the end of the run finds nothing left.
    assert_eq!(h.fakes.notifier.closed(), [1, 2]);
    assert_eq!(h.fakes.notifier.sent().len(), 2);
}

#[tokio::test]
async fn busy_outside_a_handler_just_shows() {
    let h = Harness::start(&[]).await;
    h.eval("om.busy('from eval')").await.unwrap();
    h.eval("om.busy()").await.unwrap();
    assert_eq!(h.fakes.notifier.busy().len(), 1);
    assert!(h.fakes.notifier.closed().is_empty());
}
