use super::*;
use std::collections::BTreeMap;

fn repo_source() -> Source {
    Source::at(&Path::new(env!("CARGO_MANIFEST_DIR")).join("..\\..\\integrations\\firefox")).expect("integration")
}

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("shellman-ff-look-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Every file below `dir` with its bytes, and every folder (as an empty entry).
fn tree(dir: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Option<Vec<u8>>>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let rel = e.path().strip_prefix(base).unwrap().to_string_lossy().into_owned();
            if e.file_type().unwrap().is_dir() {
                out.insert(rel, None);
                walk(base, &e.path(), out);
            } else {
                out.insert(rel, Some(std::fs::read(e.path()).unwrap()));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// A Firefox folder with the loader in it (what Settings or the helper would have written).
fn firefox_with_loader(name: &str) -> PathBuf {
    let ff = temp(name);
    std::fs::create_dir_all(ff.join("defaults\\pref")).unwrap();
    std::fs::write(ff.join("firefox.exe"), b"").unwrap();
    loader::write(&ff).unwrap();
    ff
}

#[test]
fn remove_puts_a_used_profile_back_exactly() {
    let src = repo_source();
    let p = temp("used");
    std::fs::create_dir_all(p.join("chrome\\img")).unwrap();
    std::fs::write(p.join("chrome\\userChrome.css"), b"/* mine */\r\n").unwrap();
    std::fs::write(p.join("chrome\\img\\a.png"), b"png").unwrap();
    // No line break at the end: install has to add one, and removal takes it out again.
    std::fs::write(p.join("user.js"), b"user_pref(\"mine\", 1);").unwrap();
    std::fs::write(p.join("prefs.js"), b"// prefs\n").unwrap();
    let before = tree(&p);

    let ff = firefox_with_loader("used-ff");
    let done = install(&p, &src, Some(&ff)).unwrap();
    assert!(!done.updated);
    assert_eq!(status(&p, Some(&src)), Status::Installed { scripts: true });
    assert_eq!(std::fs::read(p.join("chrome\\userChrome.css")).unwrap(), std::fs::read(src.chrome("userChrome.css")).unwrap());
    assert!(uses_scripts(&p));
    // The backup holds what was replaced.
    assert_eq!(std::fs::read(done.backup.join("chrome\\userChrome.css")).unwrap(), b"/* mine */\r\n");
    // And state.json is what install.ps1 writes, read back.
    let st = read_state(&p).unwrap();
    assert!(st.had_chrome && st.had_user_js && st.user_js_break);
    assert_eq!(st.autoconfig.as_ref().unwrap()[0].sha256, sha256_hex(loader::CONFIG_JS.as_bytes()));

    let removed = remove(&p).unwrap();
    assert_eq!(removed.scripts_for.as_deref(), Some(ff.as_path()));
    assert_eq!(tree(&p), before);
    let _ = std::fs::remove_dir_all(&p);
    let _ = std::fs::remove_dir_all(&ff);
}

#[test]
fn remove_from_a_fresh_profile_leaves_it_as_it_was() {
    let src = repo_source();
    let p = temp("fresh");
    std::fs::write(p.join("times.json"), b"{}").unwrap();
    let before = tree(&p);
    install(&p, &src, None).unwrap();
    assert_eq!(status(&p, Some(&src)), Status::Installed { scripts: false });
    assert!(!read_state(&p).unwrap().had_chrome);
    remove(&p).unwrap();
    assert_eq!(tree(&p), before);
    assert_eq!(remove(&p).unwrap(), Removed { nothing: true, scripts_for: None });
    let _ = std::fs::remove_dir_all(&p);
}

#[test]
fn update_brings_changed_files_back_keeps_the_backup_and_later_prefs() {
    let src = repo_source();
    let p = temp("update");
    install(&p, &src, None).unwrap();
    let backup = read_state(&p).unwrap().backup;
    std::fs::write(p.join("chrome\\shellman-look.css"), b"old").unwrap();
    assert_eq!(status(&p, Some(&src)), Status::Outdated { scripts: false });
    let mut u = std::fs::read(p.join("user.js")).unwrap();
    u.extend_from_slice(b"user_pref(\"later\", 1);\r\n");
    std::fs::write(p.join("user.js"), &u).unwrap();

    let again = install(&p, &src, None).unwrap();
    assert!(again.updated);
    assert_eq!(read_state(&p).unwrap().backup, backup);
    assert_eq!(status(&p, Some(&src)), Status::Installed { scripts: false });
    remove(&p).unwrap();
    // The pref added after install stays; the file install created stays because it is not blank.
    assert_eq!(std::fs::read(p.join("user.js")).unwrap(), b"user_pref(\"later\", 1);\r\n");
    let _ = std::fs::remove_dir_all(&p);
}

#[test]
fn scripts_switch_on_and_off_with_an_update() {
    let src = repo_source();
    let p = temp("scripts");
    let ff = firefox_with_loader("scripts-ff");
    install(&p, &src, None).unwrap();
    install(&p, &src, Some(&ff)).unwrap();
    assert_eq!(status(&p, Some(&src)), Status::Installed { scripts: true });
    // The loader going (a Firefox reinstall) makes it outdated.
    loader::remove(&ff).unwrap();
    assert_eq!(status(&p, Some(&src)), Status::Outdated { scripts: true });
    loader::write(&ff).unwrap();
    let off = install(&p, &src, None).unwrap();
    assert_eq!(off.scripts_dropped.as_deref(), Some(ff.as_path()));
    assert!(!p.join("chrome").join(SCRIPTS_DIR).exists());
    assert_eq!(status(&p, Some(&src)), Status::Installed { scripts: false });
    let _ = std::fs::remove_dir_all(&p);
    let _ = std::fs::remove_dir_all(&ff);
}

#[test]
fn scripts_need_the_loader_first() {
    let src = repo_source();
    let p = temp("noloader");
    let ff = temp("noloader-ff");
    assert!(matches!(install(&p, &src, Some(&ff)), Err(Refused::Other(_))));
    assert!(!p.join(BACKUP_ROOT).exists(), "nothing written");
    let _ = std::fs::remove_dir_all(&p);
    let _ = std::fs::remove_dir_all(&ff);
}

#[test]
fn nothing_changes_while_firefox_holds_the_profile() {
    use std::os::windows::fs::OpenOptionsExt;
    let src = repo_source();
    let p = temp("inuse");
    std::fs::write(p.join("parent.lock"), b"").unwrap();
    let lock = std::fs::OpenOptions::new().read(true).write(true).share_mode(0).open(p.join("parent.lock")).unwrap();
    assert_eq!(install(&p, &src, None), Err(Refused::InUse));
    assert!(!p.join("chrome").exists() && !p.join("user.js").exists());
    drop(lock);
    install(&p, &src, None).unwrap();
    let lock = std::fs::OpenOptions::new().read(true).write(true).share_mode(0).open(p.join("parent.lock")).unwrap();
    assert_eq!(remove(&p), Err(Refused::InUse));
    drop(lock);
    remove(&p).unwrap();
    let _ = std::fs::remove_dir_all(&p);
}

#[test]
fn powershells_state_file_is_read_and_its_unknown_fields_kept() {
    let ps = "\u{feff}{\r\n    \"backup\":  \"20261113-150102\",\r\n    \"hadChrome\":  false,\r\n    \"hadUserJs\":  true,\r\n    \"files\":  [\r\n                  \"userChrome.css\"\r\n              ],\r\n    \"installed\":  \"2026-11-13T15:01:02.1234567-08:00\",\r\n    \"firefoxDir\":  \"C:\\\\Program Files\\\\Mozilla Firefox\",\r\n    \"autoconfig\":  [\r\n                       {\r\n                           \"path\":  \"config.js\",\r\n                           \"sha256\":  \"AB\",\r\n                           \"backedUp\":  false\r\n                       }\r\n                   ],\r\n    \"scripts\":  [\"shellman-live.sys.mjs\", \"shellman_glass.dll\"],\r\n    \"future\":  7\r\n}";
    let st = State::parse(ps.as_bytes()).expect("parsed");
    assert_eq!(st.backup, "20261113-150102");
    assert!(st.had_user_js && !st.had_chrome && st.has_scripts());
    assert_eq!(st.scripts.as_ref().unwrap()[1], "shellman_glass.dll");
    let back: serde_json::Value = serde_json::from_slice(&serde_json::to_vec(&st).unwrap()).unwrap();
    assert_eq!(back["future"], 7);
    assert_eq!(back["hadUserJs"], true);
    assert_eq!(back["autoconfig"][0]["backedUp"], false);
}

#[test]
fn a_powershell_install_over_another_autoconfig_is_left_to_powershell() {
    let src = repo_source();
    let p = temp("forced");
    install(&p, &src, None).unwrap();
    let mut st = read_state(&p).unwrap();
    st.firefox_dir = Some("C:\\ff".into());
    st.autoconfig = Some(vec![Autoconfig { path: "config.js".into(), sha256: "AB".into(), backed_up: true }]);
    write_state(&p, &st).unwrap();
    assert!(matches!(remove(&p), Err(Refused::Other(_))));
    assert!(p.join(BACKUP_ROOT).exists());
    let _ = std::fs::remove_dir_all(&p);
}
