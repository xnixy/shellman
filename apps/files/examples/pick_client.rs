//! A tiny app that asks Files for a file, for trying the picker by hand.
//!
//! ```text
//! cargo build -p shellman-files
//! cargo run -p shellman-files --example pick_client -- <open|many|save|folder> [options]
//!   --dir <folder>      open here (the app's SetFolder)
//!   --name <name>       the name to start with
//!   --title <title>     the window's title
//!   --owner <hwnd>      a window to centre over and belong to (decimal or 0x hex)
//!   --hidden            show hidden items
//!   --confirm           check the choice before the picker closes (as OnFileOk does)
//!   --veto-first        with --confirm: refuse the first choice, to try choosing again
//! ```
//!
//! It writes the request to %TEMP%, runs `shellman-files --pick`, prints the picker's process
//! id as soon as it starts, then the response and the exit code.

use shellman_pick::{FileType, Mode, Request, Verdict};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

fn main() {
    let mut args = std::env::args().skip(1);
    let mode = match args.next().as_deref() {
        Some("open") => Mode::Open,
        Some("many") => Mode::OpenMany,
        Some("save") => Mode::Save,
        Some("folder") => Mode::Folder,
        _ => {
            eprintln!("usage: pick_client <open|many|save|folder> [--dir D] [--name N] [--title T] [--owner HWND] [--hidden] [--confirm] [--veto-first]");
            std::process::exit(2);
        }
    };
    let mut req = Request {
        mode,
        must_exist: matches!(mode, Mode::Open | Mode::OpenMany),
        overwrite_prompt: mode == Mode::Save,
        client: "pick_client.exe".into(),
        host_pid: std::process::id(),
        ..Default::default()
    };
    if mode != Mode::Folder {
        req.types = vec![
            FileType::parse("Text files (*.txt; *.md)", "*.txt;*.md"),
            FileType::parse("Images (*.png; *.jpg)", "*.png;*.jpg;*.jpeg"),
            FileType::parse("All files (*.*)", "*.*"),
        ];
    }
    if mode == Mode::Save {
        req.default_ext = "txt".into();
        req.file_name = "Untitled".into();
    }
    let mut veto_first = false;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--dir" => req.folder = args.next().map(PathBuf::from),
            "--name" => req.file_name = args.next().unwrap_or_default(),
            "--title" => req.title = args.next().unwrap_or_default(),
            "--owner" => req.owner = args.next().map_or(0, |s| parse_hwnd(&s)),
            "--hidden" => req.show_hidden = true,
            "--confirm" => req.confirm = true,
            "--veto-first" => veto_first = true,
            other => eprintln!("ignored: {other}"),
        }
    }

    let exe = std::env::var_os("SHELLMAN_PICKER_EXE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            // target\<profile>\examples\pick_client.exe -> target\<profile>\shellman-files.exe
            let me = std::env::current_exe().unwrap();
            me.parent().and_then(|d| d.parent()).unwrap().join("shellman-files.exe")
        });
    if !exe.exists() {
        eprintln!("{} is missing: run `cargo build -p shellman-files` first", exe.display());
        std::process::exit(2);
    }
    let path = shellman_pick::new_request_path(&std::env::temp_dir(), std::process::id(), 1);
    shellman_pick::write_json(&path, &req).expect("write the request");
    let response = shellman_pick::response_path(&path);
    let verdict = shellman_pick::verdict_path(&path);
    let _ = std::fs::remove_file(&response);

    let mut child = Command::new(&exe).arg("--pick").arg(&path).spawn().expect("start the picker");
    println!("picker pid {}", child.id());
    let _ = std::io::stdout().flush();

    let mut vetoed = false;
    let mut checked = None;
    let status = loop {
        if let Some(status) = child.try_wait().expect("wait") {
            break status;
        }
        if req.confirm {
            if let Some(resp) = shellman_pick::read_response(&response) {
                if resp.ok {
                    let _ = std::fs::remove_file(&response);
                    println!("checking: {:?}", resp.paths);
                    checked = Some(resp.clone());
                    let v = if veto_first && !vetoed {
                        vetoed = true;
                        Verdict::Veto
                    } else {
                        Verdict::Ok
                    };
                    println!("verdict: {}", v.as_str());
                    shellman_pick::write_verdict(&verdict, v).expect("write the verdict");
                }
            }
        }
        std::thread::sleep(Duration::from_millis(30));
    };
    let code = status.code().unwrap_or(-1);
    println!("exit code {code}");
    match shellman_pick::read_response(&response).or(checked) {
        Some(r) => println!("{}", serde_json::to_string_pretty(&r).unwrap()),
        None => println!("(no response file)"),
    }
    for p in [&path, &response, &verdict] {
        let _ = std::fs::remove_file(p);
    }
}

fn parse_hwnd(s: &str) -> i64 {
    match s.strip_prefix("0x") {
        Some(hex) => i64::from_str_radix(hex, 16).unwrap_or(0),
        None => s.parse().unwrap_or(0),
    }
}
