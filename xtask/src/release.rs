//! `cargo xtask release --version X.Y.Z [--publish]`.
//!
//! 1. Refuses a dirty tree, a version that is not newer than the last `v*` tag, and a tag or
//!    GitHub Release that already exists.
//! 2. Reads the signing key from `SHELLMAN_RELEASE_SIGNING_KEY` (hex of the 32-byte seed) and
//!    refuses unless it is the pair of the public key compiled into shellman
//!    (`shellman_update_core::sig::PUBLIC_KEY`). The key is in 1Password only; run this under
//!    `op run --environment <id> -- cargo xtask release ...`, which puts it in this process's
//!    environment and nowhere else.
//! 3. `cargo build --release --workspace` stamped `X.Y.Z <commit date>`, then
//!    `shellman-installer --pack` into `target\dist\shellman-X.Y.Z-setup.exe`.
//! 4. Signs it (`.sig` beside it), then checks the result exactly as the updater will: signature
//!    against the compiled-in key, file name, size, SHA-256, and the payload's own hash.
//! 5. Writes release notes from the commit subjects since the last tag.
//! 6. With `--publish` only: `gh release create vX.Y.Z` with the exe and its signature, on the
//!    current commit (which must already be on GitHub). Without it, prints that command.
//!
//! `--test-key` signs with a throwaway key so the whole path can be tried without 1Password; such
//! a release is never published (the updater would refuse it anyway).

use shellman_update_core::{feed, payload, sig, version::Version};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The 1Password Environment `shellman/release-signing` (made by `cargo xtask keygen`); its id
/// is not a secret.
const ENVIRONMENT_ID: &str = "ojhbajp6vr6dp33bdkrjjgldci";

struct Args {
    version: Version,
    publish: bool,
    test_key: bool,
    allow_dirty: bool,
    skip_build: bool,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut a = Args { version: Version::new(0, 0, 0), publish: false, test_key: false, allow_dirty: false, skip_build: false };
    let mut version = None;
    let mut it = args.iter();
    while let Some(x) = it.next() {
        match x.as_str() {
            "--version" => version = Some(it.next().ok_or("--version needs X.Y.Z")?.trim_start_matches('v').to_string()),
            "--publish" => a.publish = true,
            "--test-key" => a.test_key = true,
            "--allow-dirty" => a.allow_dirty = true,
            "--skip-build" => a.skip_build = true,
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    let v = version.ok_or("give --version X.Y.Z")?;
    a.version = Version::parse(&v).ok_or(format!("'{v}' is not X.Y.Z"))?;
    if a.publish && (a.test_key || a.allow_dirty || a.skip_build) {
        return Err("--publish cannot be combined with --test-key, --allow-dirty or --skip-build".into());
    }
    Ok(a)
}

pub fn run(args: &[String]) -> i32 {
    match parse(args).and_then(|a| release(&a)) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("release: {e}");
            1
        }
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

/// Run a program in the repo; its trimmed stdout, or why not.
fn out(prog: &str, args: &[&str]) -> Result<String, String> {
    let o = Command::new(prog).args(args).current_dir(root()).output().map_err(|e| format!("cannot run {prog}: {e}"))?;
    if !o.status.success() {
        return Err(format!("{prog} {} failed: {}", args.join(" "), String::from_utf8_lossy(&o.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn step(s: &str) {
    println!("\n== {s}");
}

/// The newest `vX.Y.Z` tag reachable from HEAD.
fn last_tag() -> Option<(String, Version)> {
    let t = out("git", &["describe", "--tags", "--abbrev=0", "--match", "v[0-9]*"]).ok()?;
    Version::from_tag(&t).map(|v| (t, v))
}

fn key(test: bool) -> Result<([u8; 32], [u8; 32]), String> {
    if test {
        let mut seed = [0u8; 32];
        getrandom::getrandom(&mut seed).map_err(|e| e.to_string())?;
        let public = sig::public_of(&seed);
        println!("signing with a THROWAWAY key (--test-key): this release cannot be published or installed");
        // Only a debug build of shellman-update, given this, would accept it (local tests).
        println!("throwaway public key, for SHELLMAN_UPDATE_TEST_KEY: {}", shellman_update_core::hex(&public));
        return Ok((seed, public));
    }
    let text = std::env::var("SHELLMAN_RELEASE_SIGNING_KEY").map_err(|_| {
        format!("SHELLMAN_RELEASE_SIGNING_KEY is not set. Run this under 1Password's CLI (`op`):\n  op run --environment {ENVIRONMENT_ID} -- cargo xtask release ...")
    })?;
    let seed: [u8; 32] = shellman_update_core::unhex(&text).and_then(|v| v.try_into().ok()).ok_or("SHELLMAN_RELEASE_SIGNING_KEY is not 64 hex digits")?;
    let public = sig::public_of(&seed);
    if public != sig::PUBLIC_KEY {
        return Err(format!(
            "the signing key does not match the public key compiled into shellman ({}); refusing to sign",
            sig::fingerprint(&sig::PUBLIC_KEY)
        ));
    }
    println!("signing key matches the compiled-in public key {}", sig::fingerprint(&public));
    Ok((seed, public))
}

fn release(a: &Args) -> Result<(), String> {
    let v = a.version;
    let tag = format!("v{v}");
    step(&format!("checks for {tag}"));
    let dirty = out("git", &["status", "--porcelain"])?;
    if !dirty.is_empty() {
        if a.allow_dirty {
            println!("the tree has uncommitted changes (--allow-dirty: a dry run only)");
        } else {
            return Err("the tree has uncommitted changes; commit or stash them first".into());
        }
    }
    let head = out("git", &["rev-parse", "HEAD"])?;
    let date = out("git", &["log", "-1", "--format=%cs"])?;
    let last = last_tag();
    match &last {
        Some((t, lv)) if !v.replaces(lv) => return Err(format!("{v} is not newer than the last release tag {t}")),
        Some((t, _)) => println!("last release tag: {t}"),
        None => println!("no release tag yet: this is the first release"),
    }
    if out("git", &["rev-parse", "-q", "--verify", &format!("refs/tags/{tag}")]).is_ok() {
        return Err(format!("tag {tag} already exists here"));
    }
    if !out("git", &["ls-remote", "--tags", "origin", &format!("refs/tags/{tag}")])?.is_empty() {
        return Err(format!("tag {tag} already exists on GitHub"));
    }
    if out("gh", &["release", "view", &tag, "--repo", shellman_update_core::REPO, "--json", "tagName"]).is_ok() {
        return Err(format!("a GitHub Release {tag} already exists"));
    }
    let (seed, public) = key(a.test_key)?;

    let root = root();
    let release_dir = root.join("target").join("release");
    let dist = root.join("target").join("dist");
    let exe_name = feed::exe_name(&v);
    let exe = dist.join(&exe_name);
    let stamp = format!("{v} {date}");
    if a.skip_build {
        step("build skipped (--skip-build)");
    } else {
        step(&format!("cargo build --release --workspace (stamp '{stamp}')"));
        let ok = Command::new("cargo")
            .args(["build", "--release", "--workspace"])
            .env("SHELLMAN_BUILD_STAMP", &stamp)
            .current_dir(&root)
            .status()
            .map_err(|e| format!("cannot run cargo: {e}"))?
            .success();
        if !ok {
            return Err("the build failed".into());
        }
    }

    step(&format!("pack {exe_name}"));
    let _ = std::fs::create_dir_all(&dist);
    let packer = release_dir.join("shellman-installer.exe");
    let ok = Command::new(&packer)
        .arg("--pack")
        .arg(&release_dir)
        .arg("--out")
        .arg(&exe)
        .status()
        .map_err(|e| format!("cannot run {}: {e}", packer.display()))?
        .success();
    if !ok || !exe.is_file() {
        return Err("packing failed".into());
    }

    step("sign and check as the updater will");
    let size = std::fs::metadata(&exe).map_err(|e| e.to_string())?.len();
    let sha = shellman_update_core::sha256_file(&exe).map_err(|e| e.to_string())?;
    let st = sig::Statement { version: v.to_string(), file: exe_name.clone(), size, sha256: sha.clone() };
    let sig_text = sig::sign(&st, &seed);
    let sig_path = dist.join(format!("{exe_name}.sig"));
    std::fs::write(&sig_path, &sig_text).map_err(|e| format!("cannot write {}: {e}", sig_path.display()))?;
    let checked = sig::verify(&sig_text, &[public])?;
    checked.matches(&exe_name, size, &sha)?;
    if Version::parse(&checked.version) != Some(v) {
        return Err("the signed version is not the release's".into());
    }
    payload::check(&exe)?;
    if !a.test_key {
        // What the installed updater trusts, not just the key in hand.
        sig::verify(&sig_text, &[sig::PUBLIC_KEY])?;
    }
    println!("{exe_name}: {size} bytes, sha256 {sha}");
    println!("signature: {} (key {})", sig_path.display(), sig::fingerprint(&public));

    step("release notes");
    let notes = notes(&v, last.as_ref().map(|(t, _)| t.as_str()))?;
    let notes_path = dist.join(format!("release-notes-{v}.md"));
    std::fs::write(&notes_path, &notes).map_err(|e| e.to_string())?;
    println!("{}", notes.trim_end());

    let title = format!("shellman {v}");
    let gh_args = vec![
        "release".to_string(),
        "create".into(),
        tag.clone(),
        exe.display().to_string(),
        sig_path.display().to_string(),
        "--repo".into(),
        shellman_update_core::REPO.into(),
        "--target".into(),
        head.clone(),
        "--title".into(),
        title,
        "--notes-file".into(),
        notes_path.display().to_string(),
    ];
    if !a.publish {
        step("dry run: nothing was published");
        println!("with --publish this would run:\n  gh {}", gh_args.iter().map(|s| if s.contains(' ') { format!("\"{s}\"") } else { s.clone() }).collect::<Vec<_>>().join(" "));
        return Ok(());
    }
    step("publish");
    // The release's tag is made on GitHub at this commit: it must be there already.
    if out("git", &["branch", "-r", "--contains", &head])?.is_empty() {
        return Err(format!("commit {} is not on GitHub yet; push it first", &head[..12]));
    }
    let ok = Command::new("gh").args(&gh_args).current_dir(&root).status().map_err(|e| format!("cannot run gh: {e}"))?.success();
    if !ok {
        return Err("gh release create failed".into());
    }
    println!("published {tag}: https://github.com/{}/releases/tag/{tag}", shellman_update_core::REPO);
    Ok(())
}

fn notes(v: &Version, since: Option<&str>) -> Result<String, String> {
    let mut s = String::new();
    match since {
        Some(t) => {
            let log = out("git", &["log", "--no-merges", "--format=- %s", &format!("{t}..HEAD")])?;
            s.push_str(&format!("Changes since {t}:\n\n{}\n", if log.is_empty() { "- (no changes)" } else { &log }));
        }
        None => s.push_str("The first release of shellman.\n"),
    }
    s.push_str(&format!(
        "\nDownload `shellman-{v}-setup.exe` and run it. An installed shellman updates itself (Settings > About > Update automatically).\n\
         Each setup exe is signed: `shellman-{v}-setup.exe.sig` is checked against the key built into shellman before anything runs.\n"
    ));
    Ok(s)
}
