use git2::{Repository, Signature};
use revisar::diff::{Kind, Snapshot};
use std::{fs, path::Path};

fn fixture() -> (tempfile::TempDir, Repository) {
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
    fs::create_dir_all(&target).unwrap();
    let dir = tempfile::tempdir_in(target).unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    repo.config()
        .unwrap()
        .set_bool("core.autocrlf", false)
        .unwrap();
    (dir, repo)
}

fn stage(repo: &Repository, path: &str) {
    let mut index = repo.index().unwrap();
    index.add_path(Path::new(path)).unwrap();
    index.write().unwrap();
}

fn commit(repo: &Repository) {
    let oid = repo.index().unwrap().write_tree().unwrap();
    let tree = repo.find_tree(oid).unwrap();
    let author = Signature::now("Fixture", "fixture@example.invalid").unwrap();
    let parent = repo.head().ok().map(|h| h.peel_to_commit().unwrap());
    repo.commit(
        Some("HEAD"),
        &author,
        &author,
        "Fixture",
        &tree,
        &parent.iter().collect::<Vec<_>>(),
    )
    .unwrap();
}

#[test]
fn combines_staged_unstaged_untracked_and_is_read_only() {
    let (dir, repo) = fixture();
    fs::write(dir.path().join("a.rs"), "one\ntwo\nthree\n").unwrap();
    stage(&repo, "a.rs");
    commit(&repo);
    fs::write(dir.path().join("a.rs"), "staged\ntwo\nthree\n").unwrap();
    stage(&repo, "a.rs");
    fs::write(dir.path().join("a.rs"), "staged\nunstaged\nthree\n").unwrap();
    fs::write(dir.path().join("new file.ts"), "hello\n").unwrap();
    fs::write(dir.path().join(".gitignore"), "ignored\n").unwrap();
    fs::write(dir.path().join("ignored"), "secret\n").unwrap();
    let before = fs::read(dir.path().join(".git/index")).unwrap();
    let s = Snapshot::load(dir.path()).unwrap();
    assert_eq!(
        s.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
        vec![".gitignore", "a.rs", "new file.ts"]
    );
    let rows = &s.files[1].rows;
    assert!(
        rows.iter()
            .any(|r| r.kind == Kind::Add && r.text == "staged")
    );
    assert!(
        rows.iter()
            .any(|r| r.kind == Kind::Add && r.text == "unstaged")
    );
    assert_eq!(before, fs::read(dir.path().join(".git/index")).unwrap());
    assert!(s.unchanged().unwrap());
    fs::write(dir.path().join("a.rs"), "later\n").unwrap();
    assert!(!s.unchanged().unwrap());
}

#[test]
fn unborn_empty_binary_symlink_and_unusual_names() {
    let (dir, repo) = fixture();
    fs::write(dir.path().join("staged"), "before\n").unwrap();
    stage(&repo, "staged");
    fs::write(dir.path().join("staged"), "after\n").unwrap();
    fs::write(dir.path().join("empty"), "").unwrap();
    fs::write(dir.path().join("binary"), [0, 1, 2, 3]).unwrap();
    for name in ["line\nbreak", ":(glob)*", "-dash", "a 'quote' 界"] {
        fs::write(dir.path().join(name), "hello\n").unwrap();
    }
    std::os::unix::fs::symlink("missing-target", dir.path().join("link")).unwrap();
    let s = Snapshot::load(dir.path()).unwrap();
    assert!(s.head.is_empty());
    assert_eq!(s.files.len(), 8);
    assert!(s.files.iter().all(|f| f.status == 'A'));
    let staged = s.files.iter().find(|f| f.path == "staged").unwrap();
    assert!(
        staged
            .rows
            .iter()
            .any(|r| r.text == "after" && r.new == Some(1))
    );
    let binary = s.files.iter().find(|f| f.path == "binary").unwrap();
    assert!(binary.rows.iter().any(|r| r.text.contains("Binary files")));
    assert!(
        s.files
            .iter()
            .find(|f| f.path == "link")
            .unwrap()
            .rows
            .iter()
            .any(|r| r.text == "missing-target")
    );
    assert!(s.unchanged().unwrap());
    fs::write(dir.path().join("binary"), [0, 9, 2, 3]).unwrap();
    assert!(!s.unchanged().unwrap());
}

#[test]
fn deletions_renames_modes_and_reversed_staging() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, repo) = fixture();
    for name in ["deleted", "renamed", "mode", "reversed"] {
        fs::write(dir.path().join(name), "original\n").unwrap();
        stage(&repo, name);
    }
    commit(&repo);
    fs::remove_file(dir.path().join("deleted")).unwrap();
    fs::rename(dir.path().join("renamed"), dir.path().join("new-name")).unwrap();
    fs::set_permissions(dir.path().join("mode"), fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(dir.path().join("reversed"), "staged\n").unwrap();
    stage(&repo, "reversed");
    fs::write(dir.path().join("reversed"), "original\n").unwrap();
    let s = Snapshot::load(dir.path()).unwrap();
    assert!(!s.files.iter().any(|f| f.path == "reversed"));
    assert_eq!(
        s.files.iter().find(|f| f.path == "deleted").unwrap().status,
        'D'
    );
    assert_eq!(
        s.files.iter().find(|f| f.path == "renamed").unwrap().status,
        'D'
    );
    assert_eq!(
        s.files
            .iter()
            .find(|f| f.path == "new-name")
            .unwrap()
            .status,
        'A'
    );
    let mode = s.files.iter().find(|f| f.path == "mode").unwrap();
    assert!(mode.rows.iter().any(|r| r.text == "new mode 100755"));
}

#[test]
fn subdirectory_and_worktree_roots_and_head_change() {
    let (dir, repo) = fixture();
    fs::write(dir.path().join("a"), "a\n").unwrap();
    stage(&repo, "a");
    commit(&repo);
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("a"), "b\n").unwrap();
    let s = Snapshot::load(&dir.path().join("sub")).unwrap();
    assert_eq!(s.files.len(), 1);
    stage(&repo, "a");
    commit(&repo);
    assert!(!s.unchanged().unwrap());
    let checkout = dir.path().join("linked");
    repo.worktree("linked", &checkout, None).unwrap();
    fs::write(checkout.join("a"), "worktree\n").unwrap();
    let s = Snapshot::load(&checkout).unwrap();
    assert_eq!(s.files.len(), 1);
    assert!(s.files[0].rows.iter().any(|r| r.text == "worktree"));
}

#[test]
fn configured_diff_programs_are_never_executed() {
    let (dir, repo) = fixture();
    fs::write(dir.path().join("a"), "before\n").unwrap();
    stage(&repo, "a");
    commit(&repo);
    repo.config()
        .unwrap()
        .set_str("diff.external", "/this/must/not/run")
        .unwrap();
    repo.config()
        .unwrap()
        .set_str("diff.test.textconv", "/this/must/not/run")
        .unwrap();
    fs::write(dir.path().join(".gitattributes"), "a diff=test\n").unwrap();
    fs::write(dir.path().join("a"), "after\n").unwrap();
    let s = Snapshot::load(dir.path()).unwrap();
    assert!(
        s.files
            .iter()
            .find(|f| f.path == "a")
            .unwrap()
            .rows
            .iter()
            .any(|r| r.text == "after")
    );
}

#[test]
fn pty_send_cancel_and_signal_cleanup() {
    let (dir, repo) = fixture();
    fs::write(dir.path().join("a.rs"), "before\n").unwrap();
    stage(&repo, "a.rs");
    commit(&repo);
    fs::write(dir.path().join("a.rs"), "after\n").unwrap();
    let status = std::process::Command::new("python3")
        .args(["tests/pty_smoke.py", env!("CARGO_BIN_EXE_revisar")])
        .arg(dir.path())
        .status()
        .unwrap();
    assert!(status.success());
}
