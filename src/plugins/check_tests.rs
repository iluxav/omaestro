use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use super::*;
use crate::testutil::TempDir;

fn plugin(dir: &Path, files: &[(&str, &str)]) {
    for (rel, text) in files {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

fn checked(files: &[(&str, &str)]) -> Result<Shape> {
    static RUN: AtomicU32 = AtomicU32::new(0);
    let tmp = TempDir::new(&format!(
        "plugin-check-{}",
        RUN.fetch_add(1, Ordering::Relaxed)
    ));
    plugin(tmp.path(), files);
    check("demo", tmp.path())
}

fn refusal(files: &[(&str, &str)]) -> String {
    format!("{:#}", checked(files).unwrap_err())
}

#[test]
fn a_module_with_setup_passes_and_one_without_says_so() {
    let with =
        "local M = {}\nfunction M.setup(opts) om.hotkey('X', function() end) end\nreturn M\n";
    assert_eq!(
        checked(&[("init.lua", with)]).unwrap(),
        Shape { setup: true }
    );
    assert_eq!(
        checked(&[("init.lua", "return {}")]).unwrap(),
        Shape { setup: false }
    );
    assert_eq!(
        checked(&[("init.lua", "return function() end")]).unwrap(),
        Shape { setup: false }
    );
}

#[test]
fn a_syntax_or_load_error_is_refused_with_its_file_and_line() {
    assert_eq!(
        refusal(&[("init.lua", "local M = {}\nfunction M.setup(\nreturn M\n")]),
        "demo does not load: demo/init.lua:3: <name> or '...' expected near 'return'"
    );
    assert_eq!(
        refusal(&[("init.lua", "local M = {}\nlocal x = nil + 1\nreturn M\n")]),
        "demo does not load: demo/init.lua:2: attempt to perform arithmetic on a nil value"
    );
    assert_eq!(
        refusal(&[("init.lua", "error('broken on purpose')")]),
        "demo does not load: demo/init.lua:1: broken on purpose"
    );
}

#[test]
fn the_top_level_may_reach_for_om_and_other_globals() {
    let init = r#"
        local HOME = os.getenv("HOME") or ""
        local lib = om.config_dir .. "/lib"
        local last = om.store.get("x") or 1
        if om.version and om.version >= "0.1" then end
        local data = some_library.decode("{}")
        function om.extra() end
        return { setup = function() end }
    "#;
    assert!(checked(&[("init.lua", init)]).unwrap().setup);
}

#[test]
fn nothing_the_plugin_does_reaches_the_machine() {
    let tmp = TempDir::new("plugin-check-effects");
    let marker = tmp.path().join("touched");
    let marker = marker.to_string_lossy();
    let init = format!(
        "io.open('{marker}', 'w')\nos.execute('touch {marker}')\nos.remove('{marker}')\n\
         dofile('{marker}')\nreturn {{ setup = function() end }}\n"
    );
    plugin(&tmp.path().join("p"), &[("init.lua", &init)]);
    assert!(check("demo", &tmp.path().join("p")).unwrap().setup);
    assert!(!tmp.path().join("touched").exists());

    // A precompiled chunk is not loaded.
    let refused = refusal(&[
        ("init.lua", "return require('demo.bin')"),
        ("bin.lua", "\x1bLua\x54\x00"),
    ]);
    assert!(
        refused.contains("attempt to load a binary chunk"),
        "{refused}"
    );
}

#[test]
fn a_loop_at_the_top_level_runs_out_of_budget() {
    assert_eq!(
        refusal(&[("init.lua", "while true do end")]),
        "demo does not load: it does not finish loading (a loop at its top level?)"
    );
    // Inside a coroutine too.
    let refused = refusal(&[(
        "init.lua",
        "local co = coroutine.wrap(function() while true do end end)\nco()\n",
    )]);
    assert!(refused.contains("does not finish loading"), "{refused}");
}

#[test]
fn its_own_modules_load_from_its_files_and_stay_inside_it() {
    let files = [
        (
            "init.lua",
            "local util = require('demo.util')\nlocal deep = require('demo.parts')\n\
             return { setup = util.setup, deep = deep }\n",
        ),
        ("util.lua", "return { setup = function() end }"),
        ("parts/init.lua", "return 1"),
    ];
    assert!(checked(&files).unwrap().setup);

    let broken = [
        ("init.lua", "return require('demo.util')"),
        ("util.lua", "return {\n"),
    ];
    let refused = refusal(&broken);
    assert!(refused.contains("demo/util.lua:"), "{refused}");

    assert!(
        refusal(&[("init.lua", "return require('demo.missing')")])
            .contains("module 'demo.missing' not found in demo")
    );
    // Dots cannot climb out of the plugin's directory.
    assert!(inside(Path::new("/p"), "/etc/passwd").is_none());
    assert!(inside(Path::new("/p"), "../x.lua").is_none());
    assert_eq!(
        inside(Path::new("/p"), "a/b.lua"),
        Some(Path::new("/p/a/b.lua").to_path_buf())
    );
}

#[test]
fn every_shipped_plugin_and_the_template_pass() {
    let shipped = Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins");
    for entry in std::fs::read_dir(&shipped).unwrap() {
        let dir = entry.unwrap().path();
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        let shape = check(&name, &dir).unwrap_or_else(|err| panic!("{err:#}"));
        assert!(shape.setup, "{name} has no setup");
    }
    let template = super::super::scaffold::init_template("demo");
    assert!(checked(&[("init.lua", &template)]).unwrap().setup);
}
