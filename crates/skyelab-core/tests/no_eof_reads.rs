//! A short-lived child's output is read through `skyelab_core::process` (temp files, read after the child exits),
//! never through a pipe read to EOF, which any process that inherited the pipe can hold open forever. This scans
//! every crate's src/. The one exception is the agent's own stdin/stdout, which stream for the session's life.

use std::path::Path;

/// (file, line) pairs allowed to pipe: the agent session's streaming stdin and stdout.
const STREAMING: &[(&str, &str)] = &[
    ("skyelab-agent/src/lib.rs", ".stdin(Stdio::piped())"),
    ("skyelab-agent/src/lib.rs", ".stdout(Stdio::piped())"),
];
const PIPE_READS: &[&str] = &[".output()", "wait_with_output", "Stdio::piped()"];

fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_short_lived_child_output_is_read_from_a_pipe() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut found = Vec::new();
    for entry in std::fs::read_dir(&crates).expect("crates dir").flatten() {
        let mut files = Vec::new();
        rust_files(&entry.path().join("src"), &mut files);
        for file in files {
            let stem = file
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            // The helper itself, and test code compiled into src/.
            if stem == "process"
                || stem == "tests"
                || stem.ends_with("_test")
                || stem.ends_with("_tests")
            {
                continue;
            }
            let text = std::fs::read_to_string(&file).expect("read source");
            // Unit tests at the bottom of a file are test code too.
            let code = text.split("#[cfg(test)]").next().unwrap_or_default();
            let rel = file
                .strip_prefix(&crates)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            for (n, line) in code.lines().enumerate() {
                let streaming = STREAMING
                    .iter()
                    .any(|(f, l)| rel.ends_with(f) && line.trim() == *l);
                if !streaming && PIPE_READS.iter().any(|p| line.contains(p)) {
                    found.push(format!("{}:{}: {}", file.display(), n + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "read these through skyelab_core::process::output:\n{}",
        found.join("\n")
    );
}
