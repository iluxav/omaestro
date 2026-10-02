//! Selection, model, paste and window: what the fakes saw.

use super::*;
use crate::backend::ClipContent;

#[tokio::test]
async fn selection_is_the_primary_selection() {
    let h = Harness::start(&[]).await;
    assert_eq!(
        h.eval("return om.selection() == ''").await.unwrap(),
        ["true"]
    );
    h.fakes.clipboard.select("picked text");
    assert_eq!(
        h.eval("return om.selection()").await.unwrap(),
        ["picked text"]
    );
}

#[tokio::test]
async fn llm_uses_the_configured_model_and_returns_its_text() {
    let h = Harness::start(&[]).await;
    h.fakes.llm.answer("Better text.");
    assert_eq!(
        h.eval("return om.llm('fix this')").await.unwrap(),
        ["Better text."]
    );

    let requests = h.fakes.llm.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].endpoint, "http://127.0.0.1:11434/api/chat");
    assert_eq!(requests[0].model, "llama3.2");
    assert_eq!(requests[0].prompt, "fix this");
    assert_eq!(requests[0].system, None);
    assert_eq!(requests[0].api_key, None);
    assert_eq!(requests[0].timeout, Duration::from_secs(60));
}

#[tokio::test]
async fn llm_options_and_config_file_override_the_defaults() {
    let config = "[model]\nendpoint = \"http://box:8080/v1/chat/completions\"\nname = \"qwen3\"\n\
                  timeout_secs = 5\napi_key_env = \"PATH\"\n";
    let h = Harness::start(&[("omaestro.toml", config)]).await;
    h.eval("om.llm('a')").await.unwrap();
    h.eval("om.llm('b', {model = 'tiny', system = 'be brief'})")
        .await
        .unwrap();

    let requests = h.fakes.llm.requests();
    assert_eq!(requests[0].endpoint, "http://box:8080/v1/chat/completions");
    assert_eq!(requests[0].model, "qwen3");
    assert_eq!(requests[0].timeout, Duration::from_secs(5));
    // The key comes from the environment variable the file names.
    assert_eq!(requests[0].api_key, std::env::var("PATH").ok());
    assert_eq!(requests[1].model, "tiny");
    assert_eq!(requests[1].system.as_deref(), Some("be brief"));
}

#[tokio::test]
async fn llm_failures_read_like_instructions() {
    let config = "[model]\napi_key_env = \"OMAESTRO_TEST_NO_SUCH_VARIABLE\"\n";
    let h = Harness::start(&[]).await;
    h.fakes
        .llm
        .fail("model endpoint 127.0.0.1:11434 is down, start ollama");
    assert_eq!(
        h.eval("local text = om.llm('x') return text")
            .await
            .unwrap_err(),
        "eval:1: model endpoint 127.0.0.1:11434 is down, start ollama"
    );

    assert!(h.save(&[("omaestro.toml", config)]).await.ok);
    assert_eq!(
        h.eval("local text = om.llm('x') return text")
            .await
            .unwrap_err(),
        "eval:1: omaestro.toml says the model key is in $OMAESTRO_TEST_NO_SUCH_VARIABLE, \
         but that variable is not set"
    );
}

#[tokio::test]
async fn broken_config_file_fails_the_reload() {
    let h = Harness::start(&[("init.lua", "kept = true")]).await;
    let response = h
        .save(&[
            ("omaestro.toml", "[model]\nnmae = 1\n"),
            ("init.lua", "kept = false"),
        ])
        .await;
    let error = response.error.unwrap();
    assert!(error.starts_with("omaestro.toml: "), "{error}");
    assert!(
        error.ends_with("(reload failed, previous rules kept)"),
        "{error}"
    );
    assert_eq!(h.eval("return kept").await.unwrap(), ["true"]);
}

#[tokio::test(start_paused = true)]
async fn paste_goes_through_the_clipboard_and_gives_it_back() {
    let h = Harness::start(&[]).await;
    h.fakes
        .clipboard
        .copy(ClipContent::text("what the user had copied"));
    h.fakes.hypr.set_window("firefox", "Some page");

    h.eval("om.paste('new text')").await.unwrap();
    assert_eq!(
        h.fakes.journal.entries(),
        [
            r#"clipboard = text/plain "new text""#,
            "key CTRL+V",
            r#"clipboard = text/plain "what the user had copied""#,
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn paste_chord_depends_on_the_focused_app() {
    let config = "[paste]\nchord = \"shift+Insert\"\n[paste.apps]\nEmacs = \"ctrl+y\"\n";
    let h = Harness::start(&[("omaestro.toml", config)]).await;
    let keys = |h: &Harness| -> Vec<String> {
        let entries = h.fakes.journal.entries().into_iter();
        entries.filter(|e| e.starts_with("key ")).collect()
    };

    // Nothing focused: the default chord, here the configured one.
    h.eval("om.paste('a')").await.unwrap();
    h.fakes.hypr.set_window("Alacritty", "shell");
    h.eval("om.paste('b')").await.unwrap();
    h.fakes.hypr.set_window("emacs", "notes.org");
    h.eval("om.paste('c')").await.unwrap();
    assert_eq!(
        keys(&h),
        ["key SHIFT+Insert", "key CTRL+SHIFT+V", "key CTRL+Y"]
    );
}

#[tokio::test(start_paused = true)]
async fn paste_restores_an_empty_or_non_text_clipboard_too() {
    let h = Harness::start(&[]).await;
    h.eval("om.paste('x')").await.unwrap();
    assert_eq!(
        h.fakes.journal.entries().last().unwrap(),
        "clipboard cleared"
    );
    assert_eq!(h.fakes.clipboard.content(), None);

    let image = ClipContent {
        mime: "image/png".into(),
        data: vec![0x89, b'P', b'N', b'G'],
    };
    h.fakes.clipboard.copy(image.clone());
    h.eval("om.paste('y')").await.unwrap();
    assert_eq!(h.fakes.clipboard.content(), Some(image));
}

#[tokio::test]
async fn window_describes_the_focused_window() {
    let h = Harness::start(&[]).await;
    assert_eq!(h.eval("return om.window()").await.unwrap(), ["nil"]);
    h.fakes.hypr.set_window("firefox", "Some page");
    assert_eq!(
        h.eval("local w = om.window() return w.address, w.class, w.title, w.workspace, w.floating")
            .await
            .unwrap(),
        ["0x1", "firefox", "Some page", "1", "false"]
    );
}

#[tokio::test(start_paused = true)]
async fn the_ai_text_plugin_rewrites_the_selection_in_place() {
    let h = Harness::start(&[]).await;
    h.install_builtin("ai-text");
    assert!(
        h.save(&[("rules.d/ai.lua", "om.use('ai-text').setup({})")])
            .await
            .ok
    );
    h.fakes.hypr.set_window("firefox", "Compose");
    h.fakes.clipboard.select("me wants cofee now");
    h.fakes.llm.answer("  I would like a coffee now.\n");

    assert!(h.trigger("hotkey:SUPER+ALT+J").await.ok);
    h.settle().await;

    assert!(h.errors().is_empty(), "{:?}", h.errors());
    let requests = h.fakes.llm.requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].prompt.ends_with("me wants cofee now"),
        "{}",
        requests[0].prompt
    );
    let journal = h.fakes.journal.entries();
    assert!(
        journal.ends_with(&[
            r#"clipboard = text/plain "I would like a coffee now.""#.to_string(),
            "key CTRL+V".to_string(),
            "clipboard cleared".to_string(),
        ]),
        "{journal:?}"
    );

    // With nothing selected the model is not asked and nothing is pasted.
    h.fakes.clipboard.select("");
    assert!(h.trigger("hotkey:SUPER+ALT+J").await.ok);
    h.settle().await;
    assert_eq!(h.fakes.llm.requests().len(), 1);
    assert_eq!(h.titles(), ["omaestro"]);
}

#[tokio::test(start_paused = true)]
async fn the_ai_text_modes_menu_applies_the_picked_mode_and_remembers_it() {
    let config = "choose_command = \"pick {label} {options}\"\nprompt_command = \"ask {label}\"\n";
    let h = Harness::start(&[]).await;
    h.install_builtin("ai-text");
    let rules = "om.use('ai-text').setup({ modes = {\n\
                   { 'Fix only', 'Fix the spelling only.' },\n\
                   { 'More polite', 'Make it polite.' },\n\
                   { 'Summarize', 'Summarize it.', show = true },\n\
                 } })";
    assert!(
        h.save(&[("omaestro.toml", config), ("rules.d/ai.lua", rules)])
            .await
            .ok
    );
    h.fakes.hypr.set_window("firefox", "Compose");
    h.fakes.clipboard.select("send it now");
    h.fakes.shell.answer("More polite\n");
    h.fakes.llm.answer("Could you send it now, please?");

    assert!(h.trigger("hotkey:SUPER+ALT+J").await.ok);
    h.settle().await;

    assert!(h.errors().is_empty(), "{:?}", h.errors());
    // The model call showed a busy notification, gone when the paste was done.
    assert_eq!(h.fakes.notifier.busy()[0].1, "More polite…");
    assert_eq!(h.fakes.notifier.closed(), [1]);
    let requests = h.fakes.llm.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].prompt, "send it now");
    let system = requests[0].system.clone().unwrap_or_default();
    assert!(system.contains("Make it polite."), "{system}");
    assert!(
        system.starts_with("The user's message is a piece of text"),
        "{system}"
    );
    let journal = h.fakes.journal.entries();
    assert!(
        journal.contains(
            &"sh pick 'Rewrite' 'Fix only' 'More polite' 'Summarize' 'Custom instruction...'"
                .to_string()
        ),
        "{journal:?}"
    );
    // The window the text came from gets the focus back before the paste.
    assert!(
        journal.ends_with(&[
            "dispatch hl.dsp.focus({ window = \"address:0x1\" })".to_string(),
            r#"clipboard = text/plain "Could you send it now, please?""#.to_string(),
            "key CTRL+V".to_string(),
            "clipboard cleared".to_string(),
        ]),
        "{journal:?}"
    );

    // The rewrite went over the selection: pressing again without a new
    // one asks for one instead of rewriting the old text a second time.
    assert!(h.trigger("hotkey:SUPER+ALT+J").await.ok);
    h.settle().await;
    assert_eq!(h.errors(), ["Select some text first"]);
    assert_eq!(h.fakes.llm.requests().len(), 1);
    assert!(h.events.send(Event::SelectionChanged).await.is_ok());

    // The last mode used comes first; cancelling the menu asks nothing.
    h.fakes.journal.clear();
    h.fakes.shell.fail(Some(1), "");
    assert!(h.trigger("hotkey:SUPER+ALT+J").await.ok);
    h.settle().await;
    assert_eq!(
        h.fakes.journal.entries(),
        ["sh pick 'Rewrite' 'More polite' 'Fix only' 'Summarize' 'Custom instruction...'"]
    );
    assert_eq!(h.fakes.llm.requests().len(), 1);

    // A show mode notifies and leaves the text alone.
    h.fakes.shell.answer("Summarize\n");
    h.fakes.llm.answer(" A request to send it. ");
    assert!(h.trigger("hotkey:SUPER+ALT+J").await.ok);
    h.settle().await;
    assert!(
        h.titles().ends_with(&["Summarize".to_string()]),
        "{:?}",
        h.titles()
    );
    assert!(
        !h.fakes
            .journal
            .entries()
            .contains(&"key CTRL+V".to_string())
    );

    // A custom instruction comes from the prompt.
    h.fakes.shell.answer("Custom instruction...\n");
    h.fakes.shell.answer("in French\n");
    h.fakes.llm.answer("Envoie-le maintenant.");
    assert!(h.trigger("hotkey:SUPER+ALT+J").await.ok);
    h.settle().await;
    let requests = h.fakes.llm.requests();
    assert_eq!(requests.len(), 3);
    assert!(
        requests[2]
            .system
            .clone()
            .unwrap_or_default()
            .contains("in French")
    );
    assert_eq!(h.errors(), ["Select some text first"]);
}

#[tokio::test]
async fn the_ai_text_modes_must_have_a_label_and_an_instruction() {
    let h = Harness::start(&[]).await;
    h.install_builtin("ai-text");
    let saved = h
        .save(&[(
            "rules.d/ai.lua",
            "om.use('ai-text').setup({ modes = { { 'Only a label' } } })",
        )])
        .await;
    assert!(!saved.ok);
    assert!(
        h.errors()
            .iter()
            .any(|e| e.contains("mode 1 needs a label and an instruction")),
        "{:?}",
        h.errors()
    );
}
