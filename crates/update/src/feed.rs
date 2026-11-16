//! What the release feed offers: GitHub's "latest release" answer, read down to what updating
//! needs. Drafts and pre-releases never come from `/releases/latest`; they are refused here too.

use crate::version::Version;
use serde::Deserialize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    /// The release's page (its notes).
    pub page: String,
    pub exe_name: String,
    pub exe_url: String,
    pub exe_size: u64,
    pub sig_url: String,
}

#[derive(Deserialize)]
struct Raw {
    tag_name: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// The setup exe's name for a version.
pub fn exe_name(v: &Version) -> String {
    format!("shellman-{}.{}.{}-setup.exe", v.major, v.minor, v.patch)
}

pub fn parse(json: &str) -> Result<Release, String> {
    let raw: Raw = serde_json::from_str(json).map_err(|e| format!("the release feed could not be read ({e})"))?;
    if raw.draft || raw.prerelease {
        return Err("the newest release is a draft or a pre-release".into());
    }
    let version = Version::from_tag(&raw.tag_name).ok_or_else(|| format!("the newest release has an unknown tag '{}'", raw.tag_name))?;
    let name = exe_name(&version);
    let sig = format!("{name}.sig");
    let exe = raw.assets.iter().find(|a| a.name.eq_ignore_ascii_case(&name)).ok_or_else(|| format!("release {} has no {name}", raw.tag_name))?;
    let sig_url = raw.assets.iter().find(|a| a.name.eq_ignore_ascii_case(&sig)).map(|a| a.browser_download_url.clone()).ok_or_else(|| format!("release {} has no signature ({sig})", raw.tag_name))?;
    let page = if raw.html_url.starts_with("https://") { raw.html_url } else { crate::RELEASES_PAGE.to_string() };
    Ok(Release { version, page, exe_name: name, exe_url: exe.browser_download_url.clone(), exe_size: exe.size, sig_url })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "tag_name": "v0.2.0", "html_url": "https://github.com/xnixy/shellman/releases/tag/v0.2.0",
      "draft": false, "prerelease": false, "body": "notes",
      "assets": [
        {"name": "shellman-0.2.0-setup.exe.sig", "browser_download_url": "https://github.com/xnixy/shellman/releases/download/v0.2.0/shellman-0.2.0-setup.exe.sig", "size": 300},
        {"name": "shellman-0.2.0-setup.exe", "browser_download_url": "https://github.com/xnixy/shellman/releases/download/v0.2.0/shellman-0.2.0-setup.exe", "size": 31000000}
      ]}"#;

    #[test]
    fn reads_githubs_answer() {
        let r = parse(SAMPLE).unwrap();
        assert_eq!(r.version, Version::new(0, 2, 0));
        assert_eq!(r.exe_name, "shellman-0.2.0-setup.exe");
        assert_eq!(r.exe_size, 31_000_000);
        assert!(r.sig_url.ends_with(".exe.sig"));
        assert!(r.page.ends_with("/tag/v0.2.0"));
    }

    #[test]
    fn refuses_what_it_cannot_use() {
        assert!(parse(&SAMPLE.replace(r#""prerelease": false"#, r#""prerelease": true"#)).is_err());
        assert!(parse(&SAMPLE.replace(r#""draft": false"#, r#""draft": true"#)).is_err());
        assert!(parse(&SAMPLE.replace("\"v0.2.0\"", "\"nightly\"")).is_err());
        assert!(parse(&SAMPLE.replace("shellman-0.2.0-setup.exe.sig\"", "other.sig\"")).unwrap_err().contains("signature"));
        assert!(parse("{}").is_err());
        assert!(parse("not json").is_err());
        let r = parse(&SAMPLE.replace("https://github.com/xnixy/shellman/releases/tag/v0.2.0", "javascript:x")).unwrap();
        assert_eq!(r.page, crate::RELEASES_PAGE, "only an https page is linked");
    }
}
