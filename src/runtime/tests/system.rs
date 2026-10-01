//! `om.on_sleep`, `om.on_wake`, `om.on_usb`, `om.on_battery`,
//! `om.on_network`, and `om.mouse`.

use super::*;
use crate::backend::SystemEvent;

#[tokio::test]
async fn system_events_reach_their_handlers_and_sources_follow_the_rules() {
    let h = Harness::start(&[(
        "init.lua",
        "om.on_sleep(function() om.notify('power', 'sleep') end)\n\
         om.on_wake(function() om.notify('power', 'wake') end)\n\
         om.on_usb(function(d) om.notify('usb', d.action .. ' ' .. d.device) end)\n\
         om.on_battery(function(b) om.notify('battery', b.percent .. ' ' .. b.status) end)\n\
         om.on_network(function(n) om.notify('net', n.line) end)",
    )])
    .await;
    let mut watched: Vec<String> = h.fakes.journal.entries();
    watched.sort();
    assert_eq!(
        watched,
        [
            "watch battery",
            "watch login1",
            "watch network",
            "watch usb"
        ]
    );

    for event in [
        SystemEvent::Sleep,
        SystemEvent::Wake,
        SystemEvent::Usb {
            action: "add".into(),
            device: "/devices/usb1/1-3".into(),
        },
        SystemEvent::Battery {
            percent: 42,
            status: "Charging".into(),
        },
        SystemEvent::Network {
            line: "wlan0: connected".into(),
        },
    ] {
        assert!(h.events.send(Event::System(event)).await.is_ok());
    }
    h.settle().await;
    let seen: Vec<String> = h
        .fakes
        .notifier
        .sent()
        .into_iter()
        .map(|(t, b)| format!("{t}:{b}"))
        .collect();
    assert_eq!(
        seen,
        [
            "power:sleep",
            "power:wake",
            "usb:add /devices/usb1/1-3",
            "battery:42 Charging",
            "net:wlan0: connected"
        ]
    );

    // Only sleep left: one source stays, the rest are released.
    assert!(
        h.save(&[("init.lua", "om.on_sleep(function() end)")])
            .await
            .ok
    );
    let rows: Vec<TriggerRow> =
        serde_json::from_value(h.ask(Request::List).await.data.unwrap()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, "on_sleep");
    let mut entries = h.fakes.journal.entries();
    entries.sort();
    assert_eq!(
        entries.iter().filter(|e| *e == "watch login1").count(),
        1,
        "{entries:?}"
    );
}

#[tokio::test]
async fn mouse_position_and_movement() {
    let h = Harness::start(&[]).await;
    assert_eq!(
        h.eval("local m = om.mouse() return m.x, m.y")
            .await
            .unwrap(),
        ["640", "360"]
    );
    h.eval("om.mouse_to(10, 20)").await.unwrap();
    assert_eq!(
        h.fakes.journal.entries(),
        ["dispatch hl.dsp.cursor.move({ x = 10, y = 20 })"]
    );
}
