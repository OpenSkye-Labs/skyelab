use super::*;

const HOST: &str = "box.tail1234.ts.net";
const NODE: &str = "nodeA";

fn sample() -> Cached {
    let project = Project {
        id: 1,
        name: "app".into(),
        path: "/remote/app".into(),
        created_at: 5,
    };
    let thread = Thread {
        id: 7,
        project_id: 1,
        title: "fix the login bug".into(),
        account_id: "work".into(),
        status: "idle".into(),
        ..Thread::default()
    };
    Cached::new(HOST, NODE, vec![project], vec![thread])
}

fn only_file(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .expect("list")
        .map(|e| e.expect("entry").path())
        .collect()
}

#[test]
fn a_saved_snapshot_loads_back_for_its_host_and_node() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let dir = tmp.path().join("remote-shells");
    let cached = sample();
    save(&dir, &cached).expect("save");
    assert_eq!(load(&dir, HOST, Some(NODE)), Some(cached.clone()));
    assert_eq!(
        load(&dir, "BOX.tail1234.ts.net", Some(NODE)),
        Some(cached),
        "the host matches whatever its case"
    );
    // Atomic: only the final file, no temp file left behind.
    assert_eq!(only_file(&dir), vec![file(&dir, HOST)]);
}

#[test]
fn another_node_another_host_or_no_pin_loads_nothing() {
    let tmp = tempfile::tempdir().expect("temp dir");
    save(tmp.path(), &sample()).expect("save");
    assert_eq!(load(tmp.path(), HOST, Some("nodeB")), None);
    assert_eq!(load(tmp.path(), HOST, None), None);
    assert_eq!(load(tmp.path(), "other.tail1234.ts.net", Some(NODE)), None);
    // A file renamed onto another host's name still names its own host inside.
    std::fs::rename(
        file(tmp.path(), HOST),
        file(tmp.path(), "other.tail1234.ts.net"),
    )
    .expect("rename");
    assert_eq!(load(tmp.path(), "other.tail1234.ts.net", Some(NODE)), None);
}

#[test]
fn broken_other_schema_or_oversized_files_load_nothing() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let path = file(tmp.path(), HOST);
    std::fs::write(&path, "{not json").expect("write");
    assert_eq!(load(tmp.path(), HOST, Some(NODE)), None);

    let mut other = serde_json::to_value(sample()).expect("json");
    other["schema"] = 2.into();
    std::fs::write(&path, other.to_string()).expect("write");
    assert_eq!(load(tmp.path(), HOST, Some(NODE)), None);

    // A valid payload padded past the cap.
    let mut big = serde_json::to_string(&sample()).expect("json");
    big.push_str(&" ".repeat(MAX_BYTES as usize));
    std::fs::write(&path, big).expect("write");
    assert_eq!(load(tmp.path(), HOST, Some(NODE)), None);
}

#[test]
fn delete_removes_the_file_and_a_missing_one_is_fine() {
    let tmp = tempfile::tempdir().expect("temp dir");
    save(tmp.path(), &sample()).expect("save");
    delete(tmp.path(), HOST).expect("delete");
    assert!(!file(tmp.path(), HOST).exists());
    delete(tmp.path(), HOST).expect("a second delete");
    assert_eq!(load(tmp.path(), HOST, Some(NODE)), None);
}

#[test]
fn the_file_name_is_a_hash_inside_the_dir() {
    let dir = Path::new("/cache");
    let path = file(dir, "../../etc/passwd");
    assert_eq!(path.parent(), Some(dir));
    let name = path
        .file_name()
        .expect("name")
        .to_string_lossy()
        .into_owned();
    assert_eq!(name.len(), 64 + ".json".len());
    assert_eq!(file(dir, " Box.Tail1234.ts.net "), file(dir, HOST));
}

#[cfg(unix)]
#[test]
fn the_dir_and_file_are_owner_only() {
    use std::os::unix::fs::PermissionsExt as _;
    let tmp = tempfile::tempdir().expect("temp dir");
    let dir = tmp.path().join("a/remote-shells");
    save(&dir, &sample()).expect("save");
    let mode = |p: &Path| std::fs::metadata(p).expect("stat").permissions().mode() & 0o777;
    assert_eq!(mode(&dir), 0o700);
    assert_eq!(mode(&file(&dir, HOST)), 0o600);
}
