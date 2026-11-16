//! The Linux distributions of WSL as places for a file list: Windows serves each one's files
//! at `\\wsl.localhost\<name>` (and, the older spelling, `\\wsl$\<name>`).
//!
//! Which ones there are comes from the registry (`terminal::profiles::wsl_names`), never from
//! `wsl.exe` and never from asking `\\wsl.localhost` itself: touching a distribution's files
//! starts it, which takes seconds, so nothing here does. Only opening one does, on the worker
//! that lists the folder.

use std::path::{Component, Path, PathBuf, Prefix};

/// One distribution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Distro {
    pub name: String,
    /// `\\wsl.localhost\<name>\`.
    pub root: PathBuf,
}

impl Distro {
    pub fn new(name: &str) -> Distro {
        Distro { name: name.to_owned(), root: PathBuf::from(format!(r"\\wsl.localhost\{name}\")) }
    }
}

/// The distributions installed for this user, the default one first. Only the registry is read.
pub fn distros() -> Vec<Distro> {
    crate::terminal::profiles::wsl_names().iter().map(|n| Distro::new(n)).collect()
}

/// The distribution a path lies in, when it is one of WSL's: `Debian` for
/// `\\wsl.localhost\Debian\home` and `\\wsl$\Debian`. Decided from the spelling alone.
pub fn distro_of(path: &Path) -> Option<String> {
    let Some(Component::Prefix(prefix)) = path.components().next() else { return None };
    let (Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share)) = prefix.kind() else { return None };
    let server = server.to_string_lossy();
    let wsl = server.eq_ignore_ascii_case("wsl.localhost") || server.eq_ignore_ascii_case("wsl$");
    (wsl && !share.is_empty()).then(|| share.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_distro_s_root_is_its_share() {
        let d = Distro::new("Debian");
        assert_eq!(d.root, Path::new(r"\\wsl.localhost\Debian"));
        // The root has no parent: "up" from it leaves the distribution.
        assert_eq!(d.root.parent(), None);
        assert_eq!(Path::new(r"\\wsl.localhost\Debian\home").parent(), Some(d.root.as_path()));
    }

    #[test]
    fn paths_in_a_distro_are_told_by_their_spelling() {
        assert_eq!(distro_of(Path::new(r"\\wsl.localhost\Debian\home\x")).as_deref(), Some("Debian"));
        assert_eq!(distro_of(Path::new(r"\\WSL.LOCALHOST\Ubuntu-24.04")).as_deref(), Some("Ubuntu-24.04"));
        assert_eq!(distro_of(Path::new(r"\\wsl$\Debian\etc")).as_deref(), Some("Debian"));
        assert_eq!(distro_of(Path::new(r"\\nas\share\x")), None);
        assert_eq!(distro_of(Path::new(r"C:\wsl.localhost\Debian")), None);
        assert_eq!(distro_of(Path::new("")), None);
    }
}
