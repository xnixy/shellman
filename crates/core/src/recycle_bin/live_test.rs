//! Runs against the real bin, so it is ignored by default: `cargo test -p shellman-core
//! recycle_bin::live -- --ignored --nocapture`. It recycles files it made itself in a temp folder
//! and only ever restores or deletes those, found by their original path. It never empties the bin.

use super::*;
use crate::fsops::{shell_op, Op};

fn ours(root: &std::path::Path) -> Vec<BinItem> {
    list().unwrap().into_iter().filter(|i| i.original.starts_with(root)).collect()
}

#[test]
#[ignore]
fn recycle_list_restore_delete() {
    let root = std::env::temp_dir().join(format!("shellman-bin-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("sub")).unwrap();
    let (a, b, dir) = (root.join("keep.txt"), root.join("drop.bin"), root.join("sub"));
    std::fs::write(&a, b"hello").unwrap();
    std::fs::write(&b, vec![7u8; 3000]).unwrap();
    std::fs::write(dir.join("inner.txt"), b"x").unwrap();
    let before_total = list().unwrap().len();

    assert_eq!(shell_op(0, Op::Recycle, &[a.clone(), b.clone(), dir.clone()], None), Ok(true));
    let mine = ours(&root);
    for i in &mine {
        println!("{:?}  from {}  at {}  {} B dir={}  stored {}", i.name, i.original.display(), i.deleted_text, i.size, i.is_dir, i.path.display());
    }
    assert_eq!(mine.len(), 3);
    assert_eq!(list().unwrap().len(), before_total + 3);
    let find = |name: &str| mine.iter().find(|i| i.name == name).unwrap().clone();
    assert_eq!(find("keep.txt").original, a);
    assert_eq!(find("drop.bin").size, 3000);
    assert!(find("sub").is_dir);
    assert!(state().unwrap().items >= 3);

    restore(0, &[find("keep.txt").handle, find("sub").handle]).unwrap();
    assert_eq!(std::fs::read(&a).unwrap(), b"hello");
    assert!(dir.join("inner.txt").exists());
    delete(0, &[find("drop.bin").handle]).unwrap();
    assert!(!b.exists());
    assert!(ours(&root).is_empty());
    assert_eq!(list().unwrap().len(), before_total);
    std::fs::remove_dir_all(&root).unwrap();
}

/// Removes what an interrupted run of the test above left behind: only items that were deleted
/// from a `shellman-bin-test-*` folder in the temp directory.
#[test]
#[ignore]
fn clean_up_leftovers() {
    let temp = std::env::temp_dir();
    let mine: Vec<Handle> = list()
        .unwrap()
        .into_iter()
        .filter(|i| i.original.strip_prefix(&temp).is_ok_and(|r| r.to_string_lossy().starts_with("shellman-bin-test-")))
        .map(|i| i.handle)
        .collect();
    println!("removing {} leftovers", mine.len());
    delete(0, &mine).unwrap();
}
