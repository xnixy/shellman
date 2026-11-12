//! The Store (packaged) apps of the current user, from the package manager, and removing one.
//! Frameworks, resource packages, system packages and packages with nothing to start are left
//! out, as Windows' own list does, so what shows is what a person installed.

use super::act::Progress;
use super::app::{App, Kind};
use super::format;
use std::path::{Path, PathBuf};
use windows::core::HSTRING;
use windows::ApplicationModel::{Package, PackageSignatureKind};
use windows_future::AsyncOperationProgressHandler;
use windows::Management::Deployment::{DeploymentProgress, DeploymentResult, PackageManager};

/// The packaged apps of the current user. Needs a COM apartment on the calling thread.
pub fn list(exists: &dyn Fn(&Path) -> bool) -> Vec<App> {
    let Ok(manager) = PackageManager::new() else { return Vec::new() };
    let Ok(packages) = manager.FindPackagesByUserSecurityId(&HSTRING::new()) else { return Vec::new() };
    packages.into_iter().filter_map(|p| read(&p, exists)).collect()
}

fn read(p: &Package, exists: &dyn Fn(&Path) -> bool) -> Option<App> {
    if p.IsFramework().ok()? || p.IsResourcePackage().ok()? || p.SignatureKind().ok()? == PackageSignatureKind::System {
        return None;
    }
    let name = p.DisplayName().ok()?.to_string();
    if name.trim().is_empty() || name.starts_with("ms-resource:") {
        return None;
    }
    // Something to start: a package that is only a component has no app entry.
    if p.GetAppListEntries().ok()?.Size().ok()? == 0 {
        return None;
    }
    let id = p.Id().ok()?;
    let v = id.Version().ok()?;
    let full_name = id.FullName().ok()?.to_string();
    Some(App {
        id: format!("pkg|{full_name}"),
        name,
        version: format!("{}.{}.{}.{}", v.Major, v.Minor, v.Build, v.Revision),
        publisher: p.PublisherDisplayName().map(|s| s.to_string()).unwrap_or_default(),
        size_kb: None,
        date: p.InstalledDate().ok().and_then(|d| format::date_from_ticks(d.UniversalTime)),
        icon: p.Logo().ok().and_then(|uri| uri.Path().ok()).and_then(|path| logo_file(&path.to_string(), exists)),
        kind: Kind::Packaged { full_name },
    })
}

/// The file behind a package's logo URI. The manifest names `Assets\Logo.png`, but the files on
/// disk carry a scale or size (`Logo.scale-200.png`), so when the plain name is not there the
/// first sibling that starts with its stem is used.
pub fn logo_file(uri_path: &str, exists: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    // A file URI's path reads "/C:/dir/x.png".
    let trimmed = percent_decode(uri_path);
    let trimmed = trimmed.trim_start_matches('/');
    let path = PathBuf::from(trimmed.replace('/', "\\"));
    if exists(&path) {
        return Some(path);
    }
    let (dir, stem) = (path.parent()?, path.file_stem()?.to_str()?.to_lowercase());
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let n = p.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_lowercase();
            n.starts_with(&format!("{stem}.")) && n.ends_with(".png") && !n.contains("contrast") && !n.contains("altform")
        })
        .collect();
    found.sort();
    found.into_iter().next()
}

/// "%20" and the like back to the characters they stand for.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let hex = |c: u8| (c as char).to_digit(16);
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match (b[i], b.get(i + 1).and_then(|c| hex(*c)), b.get(i + 2).and_then(|c| hex(*c))) {
            (b'%', Some(hi), Some(lo)) => {
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            _ => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Remove an app for the current user, telling `progress` the percentage as it goes. Blocks.
pub fn remove(full_name: &str, progress: Progress) -> Result<(), String> {
    let manager = PackageManager::new().map_err(|e| e.message())?;
    let op = manager.RemovePackageAsync(&HSTRING::from(full_name)).map_err(|e| e.message())?;
    let _ = op.SetProgress(&AsyncOperationProgressHandler::<DeploymentResult, DeploymentProgress>::new(move |_, p| {
        progress(p.percentage);
        Ok(())
    }));
    let result = op.get().map_err(|e| e.message())?;
    if result.ExtendedErrorCode().is_ok_and(|c| c.is_err()) {
        let text = result.ErrorText().map(|t| t.to_string()).unwrap_or_default();
        return Err(if text.is_empty() { "Windows could not remove the app.".into() } else { text });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_logo_path_that_exists_is_used_as_it_is() {
        let p = logo_file("/C:/Program Files/WindowsApps/X/Assets/Logo.png", &|_| true).unwrap();
        assert_eq!(p, PathBuf::from(r"C:\Program Files\WindowsApps\X\Assets\Logo.png"));
    }

    #[test]
    fn a_uri_path_is_decoded() {
        let p = logo_file("/C:/Program%20Files/A%26B/x.png", &|_| true).unwrap();
        assert_eq!(p, PathBuf::from(r"C:\Program Files\A&B\x.png"));
    }

    #[test]
    fn a_missing_logo_with_no_siblings_is_none() {
        assert_eq!(logo_file("/C:/definitely/not/here/Logo.png", &|_| false), None);
    }
}
