//! The build stamp. Every exe gets it in its Win32 version resource (`ProductVersion`), from its
//! own `build.rs` through [`apply`], so `shellman-setup status` can read it from a file without
//! running it. `shellman-setup --version` prints [`STAMP`].

/// `<crate version>+<git describe> <commit date>`; see `build.rs`.
pub const STAMP: &str = include_str!(concat!(env!("OUT_DIR"), "/stamp.txt"));

/// Put the stamp into an exe's version resource (call before `compile`).
pub fn apply(res: &mut winresource::WindowsResource) {
    res.set("ProductName", "shellman");
    res.set("ProductVersion", STAMP);
    res.set("FileVersion", STAMP);
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_stamp_starts_with_the_crate_version() {
        assert!(super::STAMP.starts_with(env!("CARGO_PKG_VERSION")) || std::env::var("SHELLMAN_BUILD_STAMP").is_ok());
    }
}
