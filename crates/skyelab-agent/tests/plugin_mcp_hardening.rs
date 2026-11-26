//! Acceptance: a plugin that can't be loaded safely is left out with a warning naming it, and never breaks the launch
//! (SkyeLab's own server and the other plugins still load). Nothing runs the real `claude`.
#![cfg(unix)]

use skyelab_agent::{Event, Launch, start_with_bin};
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::path::{Path, PathBuf};
use std::time::Duration;

struct Fixture {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    cfg: PathBuf,
    dir: PathBuf,
    grant: PathBuf,
    installed: Vec<String>,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(tmp.path()).expect("canonical");
    let dir = root.join("project");
    std::fs::create_dir(&dir).expect("mkdir");
    let grant = root.join("grant.json");
    std::fs::write(
        &grant,
        r#"{"mcpServers":{"skyelab":{"command":"/bin/skyelab"}}}"#,
    )
    .expect("grant");
    let cfg = root.join("cfg");
    std::fs::create_dir_all(cfg.join("plugins")).expect("mkdir");
    Fixture {
        _tmp: tmp,
        root,
        cfg,
        dir,
        grant,
        installed: Vec::new(),
    }
}

/// Plugin `key` installed (user scope) at `install`, with `files` (path in the plugin, content).
fn plugin(f: &mut Fixture, key: &str, install: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let p = install.join(path);
        std::fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
        std::fs::write(p, text).expect("write");
    }
    f.installed.push(format!(
        r#""{key}":[{{"scope":"user","installPath":"{}"}}]"#,
        install.display()
    ));
}

/// The launch report's names and warning.
fn run(f: &Fixture) -> (Vec<String>, String) {
    let keys: Vec<String> = f
        .installed
        .iter()
        .map(|e| format!("{}:true", e.split(':').next().expect("key")))
        .collect();
    std::fs::write(
        f.cfg.join("settings.json"),
        format!(r#"{{"enabledPlugins":{{{}}}}}"#, keys.join(",")),
    )
    .expect("settings");
    std::fs::write(
        f.cfg.join("plugins/installed_plugins.json"),
        format!(r#"{{"version":2,"plugins":{{{}}}}}"#, f.installed.join(",")),
    )
    .expect("installed");
    let config = AccountsConfig {
        accounts: vec![Account {
            id: "work".into(),
            label: "work".into(),
            config_dir: f.cfg.clone(),
            provider: Provider::Claude,
            gh_config_dir: None,
            ssh_agent: false,
        }],
        bindings: vec![Binding::new(f.dir.clone(), "work".into())],
    };
    let launch = Launch {
        account: &config.accounts[0],
        dir: &f.dir,
        resume: None,
        prompt: "hi",
        mcp_config: Some(&f.grant),
        model: None,
        effort: None,
        attachments: &[],
        mode: Default::default(),
    };
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let session = start_with_bin(&bin, &config, &launch).expect("start");
    loop {
        match session.events().recv_timeout(Duration::from_secs(60)) {
            Ok(Event::Mcp {
                servers,
                launch: true,
                warning,
            }) => {
                let mut names: Vec<String> = servers.into_iter().map(|s| s.name).collect();
                names.sort();
                return (names, warning.unwrap_or_default());
            }
            Ok(_) => {}
            Err(e) => panic!("no launch report: {e}"),
        }
    }
}

fn cache(f: &Fixture, rest: &str) -> PathBuf {
    f.cfg.join("plugins/cache").join(rest)
}

const GOOD: (&str, &str) = (
    ".mcp.json",
    r#"{"mcpServers":{"ok":{"command":"/bin/ok"}}}"#,
);

#[test]
fn an_inline_server_that_is_not_an_object_is_left_out() {
    let mut f = fixture();
    let bad = cache(&f, "m/bad/1");
    plugin(
        &mut f,
        "bad@m",
        &bad,
        &[(
            ".claude-plugin/plugin.json",
            r#"{"mcpServers":{"x":"oops"}}"#,
        )],
    );
    let good = cache(&f, "m/good/1");
    plugin(&mut f, "good@m", &good, &[GOOD]);
    let (names, warning) = run(&f);
    assert_eq!(names, ["plugin:good:ok", "skyelab"]);
    assert!(warning.contains("bad@m"), "{warning}");
}

#[test]
fn a_server_file_the_manifest_names_must_exist() {
    let mut f = fixture();
    let p = cache(&f, "m/named/1");
    plugin(
        &mut f,
        "named@m",
        &p,
        &[(
            ".claude-plugin/plugin.json",
            r#"{"mcpServers":"./servers.json"}"#,
        )],
    );
    let (names, warning) = run(&f);
    assert_eq!(names, ["skyelab"]);
    assert!(
        warning.contains("named@m") && warning.contains("servers.json"),
        "{warning}"
    );
}

#[test]
fn a_server_file_outside_the_plugin_is_refused() {
    let mut f = fixture();
    std::fs::write(
        f.dir.join(".mcp.json"),
        r#"{"mcpServers":{"repo":{"command":"/bin/evil"}}}"#,
    )
    .expect("repo");
    let p = cache(&f, "m/escape/1");
    let outside = format!("{}/.mcp.json", f.dir.display());
    let manifest = format!(r#"{{"mcpServers":["../../../../../project/.mcp.json","{outside}"]}}"#);
    plugin(
        &mut f,
        "escape@m",
        &p,
        &[(".claude-plugin/plugin.json", &manifest)],
    );
    let (names, warning) = run(&f);
    assert_eq!(names, ["skyelab"], "a repo's .mcp.json must never load");
    assert!(warning.contains("escape@m"), "{warning}");
}

#[test]
fn an_install_outside_a_plugin_cache_is_refused() {
    let mut f = fixture();
    // A local checkout an agent could write to: not where Claude Code installs plugins.
    let checkout = f.root.join("project/my-plugin");
    plugin(&mut f, "local@m", &checkout, &[GOOD]);
    // Another config dir's cache is fine: a real installed_plugins.json points there.
    let other = f.root.join("other-config/plugins/cache/m/fine/1");
    plugin(&mut f, "fine@m", &other, &[GOOD]);
    let (names, warning) = run(&f);
    assert_eq!(names, ["plugin:fine:ok", "skyelab"]);
    assert!(warning.contains("local@m"), "{warning}");
}

#[test]
fn two_plugins_with_one_name_are_said() {
    let mut f = fixture();
    let a = cache(&f, "m1/twin/1");
    plugin(&mut f, "twin@m1", &a, &[GOOD]);
    let b = cache(&f, "m2/twin/1");
    plugin(&mut f, "twin@m2", &b, &[GOOD]);
    let (names, warning) = run(&f);
    assert_eq!(names, ["plugin:twin:ok", "skyelab"]);
    assert!(warning.contains("plugin:twin:ok"), "{warning}");
}
