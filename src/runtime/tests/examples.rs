//! Every recipe in `examples/` loads, registers what it says, and no two
//! claim the same chord.

use super::*;

#[tokio::test]
async fn all_examples_load_together() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "lua") {
            let name = format!("rules.d/{}", path.file_name().unwrap().to_string_lossy());
            files.push((name, std::fs::read_to_string(&path).unwrap()));
        }
    }
    assert!(
        files.len() >= 10,
        "ten recipes were promised, found {}",
        files.len()
    );
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(n, c)| (n.as_str(), c.as_str()))
        .collect();

    let h = Harness::start(&files).await;
    assert_eq!(h.errors(), Vec::<String>::new());
    let status = h.status().await;
    assert_eq!(status.files.len(), files.len());
    assert_eq!(status.load_error, None);

    let rows: Vec<TriggerRow> =
        serde_json::from_value(h.ask(Request::List).await.data.unwrap()).unwrap();
    let mut kinds: Vec<&str> = rows.iter().map(|r| r.kind.as_str()).collect();
    kinds.sort();
    kinds.dedup();
    assert_eq!(kinds, ["every", "hotkey", "on_focus", "trigger"]);
    // Every hotkey made it into (fake) Hyprland: none was refused as a duplicate.
    let hotkeys = rows.iter().filter(|r| r.kind == "hotkey").count();
    assert_eq!(h.fakes.hypr.chords().len(), hotkeys);
}
