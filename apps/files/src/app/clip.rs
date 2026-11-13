//! Copy, Cut and Paste through the Windows clipboard, so files go to and come from any app.
//! What is on the clipboard is followed by a listener (see `transfer::clip::listen`), which
//! enables Paste and fades items that were cut until they are pasted or the clipboard changes.

use super::Msg;
use shellman_core::transfer::{self, clip, Payload, COPY, MOVE};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The clipboard as the window sees it.
pub(super) struct Clip {
    state: Arc<Mutex<clip::State>>,
    /// The sequence number `cut` was worked out for.
    seen: Option<u32>,
    /// The keys of the paths that were cut (see `transfer::path_key`).
    cut: HashSet<String>,
    files: usize,
    can_paste: bool,
}

impl Clip {
    /// Start following the clipboard; the window is drawn again whenever it changes.
    pub(super) fn new(ctx: &egui::Context) -> Self {
        let state = Arc::new(Mutex::new(clip::State::default()));
        let (shared, ctx) = (state.clone(), ctx.clone());
        clip::listen(Arc::new(move || {
            let st = clip::state();
            *shared.lock().unwrap() = st;
            ctx.request_repaint();
        }));
        Self {
            state,
            seen: None,
            cut: HashSet::new(),
            files: 0,
            can_paste: false,
        }
    }

    /// Take in a change, once per change.
    pub(super) fn poll(&mut self) {
        let st = self.state.lock().unwrap();
        if self.seen == Some(st.seq) {
            return;
        }
        self.seen = Some(st.seq);
        self.cut = if st.cut() {
            st.offer.files.iter().map(|p| transfer::path_key(p)).collect()
        } else {
            HashSet::new()
        };
        self.files = st.offer.files.len();
        self.can_paste = st.can_paste();
    }

    pub(super) fn is_cut(&self, p: &Path) -> bool {
        !self.cut.is_empty() && self.cut.contains(&transfer::path_key(p))
    }

    pub(super) fn can_paste(&self) -> bool {
        self.can_paste
    }

    pub(super) fn has_files(&self) -> bool {
        self.files > 0
    }

    /// How many files are on it and whether they were cut, for the command bar.
    pub(super) fn summary(&self) -> Option<(usize, bool)> {
        (self.files > 0).then_some((self.files, !self.cut.is_empty()))
    }

    fn files(&self) -> Vec<PathBuf> {
        self.state.lock().unwrap().offer.files.clone()
    }
}

impl super::FileMan {
    /// Copy (or cut) the selection to the clipboard, as Explorer does.
    pub(super) fn clip_put(&mut self, cut: bool) {
        let paths = self.selected_paths();
        if paths.is_empty() {
            return;
        }
        let n = paths.len();
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            if let Err(e) = clip::put(paths, cut) {
                let _ = tx.send(Msg::Toast(format!("Couldn't use the clipboard: {e}")));
                ctx.request_repaint();
            }
        });
        let verb = if cut { "Cut" } else { "Copied" };
        self.toast(format!("{verb} {}", super::text::count(n, "item")));
    }

    /// Paste into the folder shown: files (moved after a cut, else copied), or else a picture as
    /// a new PNG, or else text as a new .txt, each selected for renaming.
    pub(super) fn paste(&mut self) {
        let Some(dir) = self.tabs[self.active].loc.dir().map(Path::to_path_buf) else {
            return;
        };
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let (payload, effect, seq) = clip::payload();
            let msg = match payload {
                Payload::Files(paths) => Msg::PasteFiles { paths, effect, seq, dir },
                Payload::Image(png) => Msg::Created(write_new(&dir, "Pasted image.png", &png)),
                Payload::Text(text) => Msg::Created(write_new(&dir, "Pasted text.txt", text.as_bytes())),
                Payload::Nothing => Msg::Toast("Nothing to paste".into()),
            };
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
    }

    /// Files read from the clipboard arrived: move them after a cut (and then clear the
    /// clipboard, as Explorer does), else copy them.
    pub(super) fn paste_files(&mut self, paths: Vec<PathBuf>, effect: u32, seq: u32, dir: PathBuf) {
        let cut = effect & MOVE != 0 && effect & COPY == 0;
        let effect = if cut { MOVE } else { COPY };
        self.transfer_files(paths, &dir, effect, cut.then_some(seq));
    }

    /// Shortcuts to the clipboard's files, in the folder shown.
    pub(super) fn paste_shortcut(&mut self) {
        let Some(dir) = self.tabs[self.active].loc.dir().map(Path::to_path_buf) else {
            return;
        };
        let files = self.clip.files();
        if !files.is_empty() {
            self.make_links(files, &dir);
        }
    }
}

/// A new file in `dir` named after `name` (numbered when taken).
pub(super) fn write_new(dir: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let path = shellman_core::fsops::unique_file(dir, name);
    std::fs::write(&path, bytes).map(|()| path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_files_are_numbered_before_their_extension() {
        let dir = std::env::temp_dir().join(format!("files-paste-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let a = write_new(&dir, "Pasted image.png", b"1").unwrap();
        let b = write_new(&dir, "Pasted image.png", b"2").unwrap();
        let c = write_new(&dir, "Pasted image.png", b"3").unwrap();
        assert_eq!(a.file_name().unwrap(), "Pasted image.png");
        assert_eq!(b.file_name().unwrap(), "Pasted image (2).png");
        assert_eq!(c.file_name().unwrap(), "Pasted image (3).png");
        assert_eq!(std::fs::read(&b).unwrap(), b"2");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
