//! What a generated commit message or pull request is written from: a diff, the branch's commits and the repo's PR
//! template, cut to a size an agent prompt can carry. Only paths inside the thread's folder are read, and untracked
//! files only when the user picked them (or picked nothing, which commits everything listed).

use std::io::Read as _;
use std::path::{Path, PathBuf};

use crate::{Error, Result, git, lossy, repo_root, rev_parse, status};

/// The most diff text sent to the agent; past it the diff is cut and [`TRUNCATED`] says so.
pub const MAX_DIFF: usize = 60 << 10;
pub const TRUNCATED: &str = "[diff truncated]";
/// The most of the commit list and of a PR template sent.
const MAX_COMMITS: usize = 8 << 10;
const MAX_TEMPLATE: usize = 8_000;

/// GitHub's single-file PR template places, in the order GitHub looks. shortcut: a template folder
/// (`PULL_REQUEST_TEMPLATE/`) is not read; add it if a user relies on one.
const TEMPLATES: &[&str] = &[
    ".github/pull_request_template.md",
    ".github/PULL_REQUEST_TEMPLATE.md",
    ".github/pull_request_template.txt",
    ".github/PULL_REQUEST_TEMPLATE.txt",
    "pull_request_template.md",
    "PULL_REQUEST_TEMPLATE.md",
    "pull_request_template.txt",
    "PULL_REQUEST_TEMPLATE.txt",
    "docs/pull_request_template.md",
    "docs/PULL_REQUEST_TEMPLATE.md",
    "docs/pull_request_template.txt",
    "docs/PULL_REQUEST_TEMPLATE.txt",
];

/// Diff flags whose output a user's config can't change.
const DIFF: &[&str] = &[
    "-c",
    "core.quotePath=false",
    "-c",
    "diff.suppressBlankEmpty=false",
    "--literal-pathspecs",
    "diff",
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--ignore-submodules=dirty",
    "--no-relative",
    "-M",
    "--submodule=short",
    "--src-prefix=a/",
    "--dst-prefix=b/",
    "--stat",
    "--patch",
];

/// The repo root and `dir` relative to it (empty for the root itself).
fn scope(dir: &Path) -> Result<(PathBuf, PathBuf)> {
    let root = repo_root(dir)?;
    let canon = |p: &Path| std::fs::canonicalize(p).map_err(Error::Io);
    let rel = canon(dir)?
        .strip_prefix(canon(&root)?)
        .map(Path::to_owned)
        .map_err(|_| Error::Invalid(format!("{} is outside its repo", dir.display())))?;
    Ok((root, rel))
}

/// The folder as a pathspec: `.` (from the root) for the whole repo.
fn folder_spec(rel: &Path) -> String {
    if rel.as_os_str().is_empty() {
        ".".into()
    } else {
        rel.to_string_lossy().into_owned()
    }
}

/// shortcut: git's whole output is read before the cap; stream and stop at the cap if huge tracked diffs get slow.
/// What Commit would commit, as `diff --stat --patch` text plus the picked untracked files' contents: `paths` (repo
/// root relative, as the Changes tab lists them) or, when empty, everything changed. Only paths inside `dir`.
pub fn commit_diff(dir: &Path, paths: &[PathBuf]) -> Result<String> {
    let (root, rel) = scope(dir)?;
    let inside: Vec<&PathBuf> = paths.iter().filter(|p| p.starts_with(&rel)).collect();
    if !paths.is_empty() && inside.is_empty() {
        return Ok(String::new());
    }
    let specs: Vec<String> = if paths.is_empty() {
        vec![folder_spec(&rel)]
    } else {
        inside
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect()
    };
    // A repo with no commits yet is compared against the empty tree, hashed in the repo's own format (sha1 or sha256).
    let head = if rev_parse(&root, "HEAD").is_ok() {
        "HEAD".to_owned()
    } else {
        lossy(&git(&root, &["hash-object", "-t", "tree", "--stdin"])?)
            .trim()
            .to_owned()
    };
    let args = [DIFF, &["--end-of-options", &head, "--"]].concat();
    let mut out = lossy(&git(
        &root,
        &[
            &args[..],
            &specs.iter().map(String::as_str).collect::<Vec<_>>(),
        ]
        .concat(),
    )?);
    for file in status(&root)?.untracked {
        let picked = if paths.is_empty() {
            file.starts_with(&rel)
        } else {
            inside.iter().any(|p| **p == file)
        };
        if picked && out.len() < MAX_DIFF {
            untracked(&root, &file, &mut out);
        }
    }
    Ok(cap(out, MAX_DIFF, TRUNCATED))
}

/// An untracked regular file as an added-file diff (a symlink or a binary file only by name).
fn untracked(root: &Path, rel: &Path, out: &mut String) {
    let name = rel.display();
    out.push_str(&format!(
        "diff --git a/{name} b/{name}\nnew file (untracked)\n"
    ));
    let path = root.join(rel);
    let is_file = std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_file());
    let mut bytes = Vec::new();
    let read = is_file
        && std::fs::File::open(&path)
            .and_then(|f| f.take((MAX_DIFF + 1) as u64).read_to_end(&mut bytes))
            .is_ok();
    if !read || bytes.contains(&0) {
        out.push_str("(not shown: binary, a link or unreadable)\n");
        return;
    }
    out.push_str(&format!("--- /dev/null\n+++ b/{name}\n"));
    for line in String::from_utf8_lossy(&bytes).lines() {
        out.push('+');
        out.push_str(line);
        out.push('\n');
    }
}

/// `s` cut to `max` bytes at a line end, with `note` on its own line when anything was cut.
fn cap(mut s: String, max: usize, note: &str) -> String {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    let end = s[..end].rfind('\n').map_or(end, |i| i + 1);
    s.truncate(end);
    s.push_str(note);
    s.push('\n');
    s
}

/// What a pull request for the current branch is written from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrContext {
    /// The branch the PR goes into, as git names it (`origin/main`).
    pub base: String,
    /// One `- subject` line per commit on the branch, newest first.
    pub commits: String,
    /// The branch's changes since it left `base`, inside the folder only, cut to [`MAX_DIFF`].
    pub diff: String,
    /// The repo's PR template, read from `HEAD` (a regular file only), cut to 8 KB.
    pub template: Option<String>,
}

/// The branch's commits and diff against its base (`origin/HEAD`, else `origin/main`, `origin/master`, `main`,
/// `master`), and the repo's PR template.
pub fn pr_context(dir: &Path) -> Result<PrContext> {
    let (root, rel) = scope(dir)?;
    let base = base_branch(&root)?;
    let range = format!("{base}..HEAD");
    let log = git(
        &root,
        &[
            "-c",
            "log.showSignature=false",
            "log",
            "--no-color",
            "--no-merges",
            "--format=- %s",
            "-n",
            "200",
            "--end-of-options",
            &range,
            "--",
        ],
    )?;
    let three_dot = format!("{base}...HEAD");
    let spec = folder_spec(&rel);
    let args = [DIFF, &["--end-of-options", &three_dot, "--", &spec]].concat();
    Ok(PrContext {
        commits: cap(lossy(&log), MAX_COMMITS, "[commits truncated]"),
        diff: cap(lossy(&git(&root, &args)?), MAX_DIFF, TRUNCATED),
        template: template(&root)?,
        base,
    })
}

/// The repo's default branch as a local branch: the one `origin/HEAD` names, else `main`, else `master`. A run that
/// must not start from whatever is checked out (a bot fed GitHub text) branches from it.
pub fn default_branch(root: &Path) -> Result<String> {
    let local = crate::branches(root)?.local;
    let head = git(
        root,
        &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"],
    )
    .ok()
    .map(|h| {
        lossy(&h)
            .trim()
            .trim_start_matches("refs/remotes/origin/")
            .to_owned()
    });
    head.into_iter()
        .chain(["main", "master"].map(String::from))
        .find(|b| local.contains(b))
        .ok_or_else(|| {
            Error::Invalid("no default branch (origin/HEAD, main or master) here".into())
        })
}

fn base_branch(root: &Path) -> Result<String> {
    let head = git(
        root,
        &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"],
    )
    .ok();
    let from_remote = head
        .map(|h| {
            lossy(&h)
                .trim()
                .trim_start_matches("refs/remotes/")
                .to_owned()
        })
        .filter(|b| !b.is_empty());
    from_remote
        .into_iter()
        .chain(["origin/main", "origin/master", "main", "master"].map(String::from))
        .find(|b| rev_parse(root, b).is_ok())
        .ok_or_else(|| {
            Error::Invalid("no base branch (origin/HEAD, main or master) to compare with".into())
        })
}

/// The first PR template in `HEAD` that is a regular file.
fn template(root: &Path) -> Result<Option<String>> {
    if rev_parse(root, "HEAD").is_err() {
        return Ok(None);
    }
    for path in TEMPLATES {
        let out = git(
            root,
            &[
                "--literal-pathspecs",
                "ls-tree",
                "-z",
                "--full-tree",
                "HEAD",
                "--",
                path,
            ],
        )?;
        let entry = lossy(&out);
        let Some((meta, _)) = entry.split_once('\t') else {
            continue;
        };
        let mut cols = meta.split(' ');
        let (mode, kind, oid) = (cols.next(), cols.next(), cols.next());
        if !matches!(mode, Some("100644" | "100755")) || kind != Some("blob") {
            continue;
        }
        let Some(oid) = oid.filter(|o| o.chars().all(|c| c.is_ascii_hexdigit())) else {
            continue;
        };
        let text = lossy(&git(root, &["cat-file", "blob", oid])?);
        return Ok(Some(cap(text, MAX_TEMPLATE, "[template truncated]")));
    }
    Ok(None)
}
