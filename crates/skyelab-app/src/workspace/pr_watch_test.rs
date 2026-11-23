//! Acceptance: which PRs are read, and that a read runs only under the thread's own account's GitHub login.

use super::{PrEntry, PrView, due, read_pr};
use skyelab_core::{Account, AccountsConfig, Binding};
use skyelab_git::{Checks, PrState, PrStatus, Review};
use std::path::{Path, PathBuf};

const URL: &str = "https://github.com/acme/demo/pull/7";

fn entry(state: Option<PrState>, checked_ms: i64) -> PrEntry {
    PrEntry {
        url: URL.into(),
        view: match state {
            Some(state) => PrView::Status(PrStatus {
                state,
                checks: Checks::None,
                review: Review::None,
                failed: Vec::new(),
            }),
            None => PrView::SignIn,
        },
        checked_ms,
    }
}

#[test]
fn pr_watch_never_reads_merged_or_closed_prs() {
    let min = 60_000;
    for done in [PrState::Merged, PrState::Closed] {
        assert!(
            !due(None, Some(&entry(Some(done), 0)), URL, 99 * min, min),
            "{done:?}"
        );
        // The store's state counts too: an ended PR isn't read after a restart.
        assert!(!due(Some(done.as_str()), None, URL, 0, min), "{done:?}");
    }
}

#[test]
fn pr_watch_reads_open_prs_when_their_last_read_is_old() {
    let min = 60_000;
    assert!(due(None, None, URL, 0, min), "never read");
    assert!(
        due(Some("open"), None, URL, 0, min),
        "read before the restart"
    );
    for open in [PrState::Open, PrState::Draft] {
        assert!(!due(None, Some(&entry(Some(open), 0)), URL, min - 1, min));
        assert!(due(None, Some(&entry(Some(open), 0)), URL, min, min));
    }
    // "Sign in" and errors are asked again on the same schedule (a login may have happened).
    assert!(!due(None, Some(&entry(None, 0)), URL, 1, min));
    assert!(due(None, Some(&entry(None, 0)), URL, min, min));
    // A relinked thread starts over, even when the old PR was merged.
    let merged = entry(Some(PrState::Merged), 0);
    assert!(due(
        None,
        Some(&merged),
        "https://github.com/acme/demo/pull/8",
        1,
        min
    ));
}

#[cfg(unix)] // the fake gh is a shell script
mod reads {
    use super::*;

    fn fake_gh() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-git/tests/fixtures/fake-gh.sh")
    }

    struct Setup {
        _tmp: tempfile::TempDir,
        project: PathBuf,
        gh_acme: PathBuf,
        gh_other: PathBuf,
        config: AccountsConfig,
    }

    /// A repo bound to "acme"; "acme" and "other" each have their own gh dir, both logged in to the fake.
    fn setup(acme_gh: bool) -> Setup {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = std::fs::canonicalize(tmp.path()).expect("canonical");
        let project = root.join("demo");
        std::fs::create_dir_all(&project).expect("mkdir");
        let ok = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&project)
            .status()
            .expect("git");
        assert!(ok.success());
        let (gh_acme, gh_other) = (root.join("gh-acme"), root.join("gh-other"));
        let mut config = AccountsConfig::default();
        for (id, gh) in [("acme", &gh_acme), ("other", &gh_other)] {
            std::fs::create_dir_all(gh).expect("mkdir");
            config
                .add_account(Account {
                    id: id.into(),
                    label: id.into(),
                    config_dir: root.join(format!(".claude-{id}")),
                    provider: Default::default(),
                    gh_config_dir: (id != "acme" || acme_gh).then(|| gh.clone()),
                    ssh_agent: false,
                })
                .expect("account");
        }
        config
            .add_binding(Binding::new(project.clone(), "acme".into()))
            .expect("bind");
        Setup {
            _tmp: tmp,
            project,
            gh_acme,
            gh_other,
            config,
        }
    }

    fn log_in(gh: &Path) {
        std::fs::write(gh.join("fake-login"), "octo").expect("login");
        std::fs::write(gh.join("hosts.yml"), "github.com:\n    user: octo\n").expect("hosts");
    }

    fn calls(gh: &Path) -> usize {
        std::fs::read_to_string(gh.join("fake-gh.prview")).map_or(0, |t| t.lines().count())
    }

    #[test]
    fn pr_watch_reads_under_the_threads_account_only() {
        let s = setup(true);
        log_in(&s.gh_acme);
        log_in(&s.gh_other);
        let view = read_pr(&s.config, &s.project, "acme", URL, &fake_gh());
        assert!(
            matches!(&view, PrView::Status(st) if st.state == PrState::Open),
            "{view:?}"
        );
        assert_eq!((calls(&s.gh_acme), calls(&s.gh_other)), (1, 0));
        // A thread recorded under another account than the folder's binding is refused, as Push refuses it.
        let view = read_pr(&s.config, &s.project, "other", URL, &fake_gh());
        assert!(matches!(view, PrView::Failed(_)), "{view:?}");
        assert_eq!((calls(&s.gh_acme), calls(&s.gh_other)), (1, 0));
    }

    #[test]
    fn pr_watch_never_calls_gh_without_a_login() {
        // No gh dir set for the account: sign in, nothing run.
        let s = setup(false);
        log_in(&s.gh_other);
        assert_eq!(
            read_pr(&s.config, &s.project, "acme", URL, &fake_gh()),
            PrView::SignIn
        );
        assert_eq!((calls(&s.gh_acme), calls(&s.gh_other)), (0, 0));
        // A gh dir nobody logged in to: sign in, nothing run.
        let s = setup(true);
        assert_eq!(
            read_pr(&s.config, &s.project, "acme", URL, &fake_gh()),
            PrView::SignIn
        );
        assert!(!s.gh_acme.join("fake-gh.fullenv").exists(), "gh never ran");
    }
}
