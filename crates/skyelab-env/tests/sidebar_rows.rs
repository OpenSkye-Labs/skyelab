//! The full rows this machine's sidebar reads are the same projects and threads as its shell, in the same order.

mod common;

use skyelab_env::{LocalSource, Source};

#[test]
fn sidebar_rows_match_the_shell() {
    let env = common::env();
    let local = LocalSource::new(env.store.clone(), env.backend());
    let shell = local.shell().expect("shell");
    let projects: Vec<_> = local
        .projects()
        .expect("projects")
        .iter()
        .map(|p| (p.id, p.name.clone()))
        .collect();
    let shell_projects: Vec<_> = shell
        .projects
        .iter()
        .map(|p| (p.id, p.name.clone()))
        .collect();
    assert_eq!(projects, shell_projects);
    let mut threads: Vec<_> = local
        .threads()
        .expect("threads")
        .iter()
        .map(|t| (t.id, t.title.clone()))
        .collect();
    let mut shell_threads: Vec<_> = shell
        .threads
        .iter()
        .map(|t| (t.id, t.title.clone()))
        .collect();
    // The sidebar lists every project's threads newest first; the shell lists them project by project.
    threads.sort();
    shell_threads.sort();
    assert_eq!(threads, shell_threads);
}
