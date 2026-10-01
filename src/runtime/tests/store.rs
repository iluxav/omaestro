//! `om.json` and `om.store`.

use super::*;

#[tokio::test]
async fn json_encodes_and_decodes() {
    let h = Harness::start(&[]).await;
    assert_eq!(
        h.eval("return om.json.encode({name = 'x', list = {1, 2, {ok = true}}})")
            .await
            .unwrap(),
        [r#"{"list":[1,2,{"ok":true}],"name":"x"}"#]
    );
    assert_eq!(
        h.eval("return om.json.encode({a = 1}, {pretty = true})")
            .await
            .unwrap(),
        ["{\n  \"a\": 1\n}"]
    );
    assert_eq!(
        h.eval("local v = om.json.decode('{\"a\": [1, 2.5, \"s\"], \"b\": null}') return v.a[2], v.a[3], v.b")
            .await
            .unwrap(),
        ["2.5", "s", "nil"]
    );
    let err = h.eval("return om.json.decode('{oops')").await.unwrap_err();
    assert!(err.starts_with("eval:1: json.decode: "), "{err}");
    let err = h.eval("return om.json.encode(print)").await.unwrap_err();
    assert!(err.contains("function"), "{err}");
}

#[tokio::test]
async fn store_persists_across_reloads() {
    let h = Harness::start(&[]).await;
    assert_eq!(
        h.eval("return om.store.get('count')").await.unwrap(),
        ["nil"]
    );
    assert_eq!(
        h.eval("return om.store.get('count', 0)").await.unwrap(),
        ["0"]
    );
    h.eval("om.store.set('count', 3) om.store.set('who', {name = 'x', tags = {'a', 'b'}})")
        .await
        .unwrap();

    // A reload builds a fresh Lua state; the file is what remembers.
    assert!(h.save(&[]).await.ok);
    assert_eq!(
        h.eval("return om.store.get('count') + 1").await.unwrap(),
        ["4"]
    );
    assert_eq!(
        h.eval("local who = om.store.get('who') return who.name, who.tags[2]")
            .await
            .unwrap(),
        ["x", "b"]
    );
    assert_eq!(
        h.eval("return om.json.encode(om.store.all())")
            .await
            .unwrap(),
        [r#"{"count":3,"who":{"name":"x","tags":["a","b"]}}"#]
    );

    h.eval("om.store.set('count', nil)").await.unwrap();
    assert_eq!(
        h.eval("return om.store.get('count')").await.unwrap(),
        ["nil"]
    );
    let path: String = h.eval("return om.store.path").await.unwrap().remove(0);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"who\""), "{text}");
    assert!(!text.contains("\"count\""), "{text}");
}

#[tokio::test]
async fn use_loads_modules_from_the_lib_directory() {
    let h = Harness::start(&[]).await;
    let lib: String = h
        .eval("return om.config_dir .. '/lib'")
        .await
        .unwrap()
        .remove(0);
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::write(
        format!("{lib}/greet.lua"),
        "return { hello = function(n) return 'hi ' .. n end }",
    )
    .unwrap();
    assert_eq!(
        h.eval("return om.use('greet').hello('x')").await.unwrap(),
        ["hi x"]
    );
    assert_eq!(
        h.eval("return require('greet').hello('y')").await.unwrap(),
        ["hi y"]
    );
    let err = h
        .eval("local m = om.use('missing') return m")
        .await
        .unwrap_err();
    assert!(err.contains("module 'missing' not found"), "{err}");
    // With a URL, the clone goes through the shell before the require.
    h.fakes.shell.answer("");
    let err = h
        .eval("local m = om.use('still-missing', 'https://example.com/x.git') return m")
        .await
        .unwrap_err();
    assert!(
        h.fakes
            .journal
            .entries()
            .iter()
            .any(|e| e.starts_with("sh mkdir -p")
                && e.contains("git clone --depth 1 'https://example.com/x.git'")),
        "{:?}",
        h.fakes.journal.entries()
    );
    assert!(err.contains("still-missing"), "{err}");
}

#[tokio::test]
async fn use_expands_the_github_shorthand() {
    let h = Harness::start(&[]).await;
    h.fakes.shell.answer("");
    let _ = h
        .eval("local m = om.use('short', 'iluxav/om-short') return m")
        .await;
    assert!(
        h.fakes
            .journal
            .entries()
            .iter()
            .any(|e| e.contains("git clone --depth 1 'https://github.com/iluxav/om-short'")),
        "{:?}",
        h.fakes.journal.entries()
    );
}

#[tokio::test]
async fn errors_inside_a_lib_module_point_at_its_relative_path() {
    let h = Harness::start(&[]).await;
    let lib = h.config_dir().join("lib").join("boom");
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::write(
        lib.join("init.lua"),
        "local M = {}\nfunction M.setup()\n  error(\"no good\")\nend\nreturn M\n",
    )
    .unwrap();
    let saved = h.save(&[("rules.d/b.lua", "om.use('boom').setup()")]).await;
    assert!(!saved.ok);
    let errors = h.errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].starts_with("lib/boom/init.lua:3: no good"),
        "{errors:?}"
    );
}
