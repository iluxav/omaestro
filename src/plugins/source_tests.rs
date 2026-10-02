//! Tests of `plugins::source`.
use super::*;

fn official() -> Official {
    Official {
        repo: OFFICIAL_REPO.to_string(),
        dir: OFFICIAL_DIR.to_string(),
    }
}

fn git(url: &str, path: Option<&str>, reference: Option<&str>) -> Source {
    Source::Git {
        url: url.to_string(),
        path: path.map(str::to_string),
        reference: reference.map(str::to_string),
    }
}

#[test]
fn every_way_of_naming_a_plugin() {
    let o = official();
    let parse = |spec: &str| parse(spec, None, None, &o).unwrap();
    assert_eq!(
        parse("window-halves"),
        git(OFFICIAL_REPO, Some("plugins/window-halves"), None)
    );
    assert_eq!(
        parse("you/thing"),
        git("https://github.com/you/thing", None, None)
    );
    assert_eq!(
        parse("you/plugins/clock"),
        git("https://github.com/you/plugins", Some("clock"), None)
    );
    assert_eq!(
        parse("https://github.com/you/plugins/tree/main/clock/"),
        git(
            "https://github.com/you/plugins",
            Some("clock"),
            Some("main")
        )
    );
    assert_eq!(
        parse("https://github.com/you/thing.git"),
        git("https://github.com/you/thing", None, None)
    );
    assert_eq!(
        parse("github.com/iluxav/omaestro/plugins/panel"),
        git(
            "https://github.com/iluxav/omaestro",
            Some("plugins/panel"),
            None
        )
    );
    assert_eq!(
        parse("git@example.com:me/thing.git"),
        git("git@example.com:me/thing.git", None, None)
    );
    // Flags win over what the spec says.
    assert_eq!(
        super::parse(
            "https://git.example.com/x.git",
            Some("v2"),
            Some("clock"),
            &o
        )
        .unwrap(),
        git("https://git.example.com/x.git", Some("clock"), Some("v2"))
    );
}

#[test]
fn bad_specs_say_why() {
    let o = official();
    let err = |spec: &str| parse(spec, None, None, &o).unwrap_err().to_string();
    assert!(err("you/plugins/../etc").contains("not a directory inside"));
    assert!(err("https://github.com/you").contains("does not name a repository"));
    assert!(err("bad name").contains("not a plugin name"));
    assert!(err("./does-not-exist-anywhere").contains("is not a directory"));
    assert!(err("").contains("no plugin given"));
}

#[test]
fn names_and_descriptions() {
    let name = |source: Source| source.name().unwrap();
    assert_eq!(
        name(git(OFFICIAL_REPO, Some("plugins/panel"), None)),
        "panel"
    );
    assert_eq!(
        name(git("https://github.com/you/om-thing.git", None, None)),
        "om-thing"
    );
    assert_eq!(
        name(git("git@example.com:me/thing.git", None, None)),
        "thing"
    );
    assert_eq!(
        git(OFFICIAL_REPO, Some("plugins/panel"), Some("v1")).describe(),
        "github.com/iluxav/omaestro/plugins/panel@v1"
    );
    assert_eq!(
        git("git@example.com:me/thing.git", Some("clock"), None).describe(),
        "git@example.com:me/thing clock"
    );
}

#[test]
fn version_requirements() {
    assert_eq!(
        required_version("-- clock\n-- requires om >= 0.3.1\nlocal M = {}"),
        Some((0, 3, 1))
    );
    assert_eq!(required_version("local M = {}"), None);
    assert!(check_requirement("x", "-- requires om >= 0.0.1").is_ok());
    let err = check_requirement("x", "-- requires om >= 99.0.0")
        .unwrap_err()
        .to_string();
    assert!(err.starts_with("x needs om 99.0.0 or newer"), "{err}");
}
