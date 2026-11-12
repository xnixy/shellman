//! What the helper may be asked to do. A fixed list, parsed strictly from the command line:
//! no paths, commands or registry keys are ever taken from it. To add an operation: one variant
//! here, its words in `parse` and `to_args`, and one arm in the helper's handler.

/// One thing the helper will do as administrator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// Turn a sound device on or off (`sound-device <enable|disable> <endpoint-id>`).
    SoundDevice { enable: bool, id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadArgs(pub &'static str);

/// Whether `s` is exactly an MMDevice endpoint id: `{0.0.N.00000000}.{guid}` with N 0 (output)
/// or 1 (input) and the guid as 8-4-4-4-12 hex digits.
pub fn is_endpoint_id(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("{0.0.") else { return false };
    let mut chars = rest.chars();
    if !matches!(chars.next(), Some('0' | '1')) {
        return false;
    }
    let Some(rest) = chars.as_str().strip_prefix(".00000000}.{") else { return false };
    let Some(guid) = rest.strip_suffix('}') else { return false };
    let groups: Vec<&str> = guid.split('-').collect();
    groups.len() == 5
        && groups.iter().zip([8, 4, 4, 4, 12]).all(|(g, n)| g.len() == n && g.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The operation a command line (without the program name) asks for.
pub fn parse(args: &[String]) -> Result<Op, BadArgs> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["sound-device", state, id] => {
            let enable = match *state {
                "enable" => true,
                "disable" => false,
                _ => return Err(BadArgs("expected enable or disable")),
            };
            if !is_endpoint_id(id) {
                return Err(BadArgs("not an audio endpoint id"));
            }
            Ok(Op::SoundDevice { enable, id: (*id).to_owned() })
        }
        ["sound-device", ..] => Err(BadArgs("sound-device takes exactly a state and an endpoint id")),
        [] => Err(BadArgs("no operation")),
        _ => Err(BadArgs("unknown operation")),
    }
}

impl Op {
    /// The command-line words for this operation; `parse` of them gives it back. Every word is
    /// validated, so none needs quoting.
    pub fn to_args(&self) -> Vec<String> {
        match self {
            Op::SoundDevice { enable, id } => {
                vec!["sound-device".into(), if *enable { "enable" } else { "disable" }.into(), id.clone()]
            }
        }
    }

    /// The operation for a sound device, or None when `id` is not an endpoint id.
    pub fn sound_device(enable: bool, id: &str) -> Option<Op> {
        is_endpoint_id(id).then(|| Op::SoundDevice { enable, id: id.to_owned() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "{0.0.0.00000000}.{8a4f1c2e-0b3d-4e5f-9a6b-7c8d9e0f1a2b}";

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_known_shapes_are_endpoint_ids() {
        assert!(is_endpoint_id(ID));
        assert!(is_endpoint_id("{0.0.1.00000000}.{8A4F1C2E-0B3D-4E5F-9A6B-7C8D9E0F1A2B}"));
    }

    #[test]
    fn anything_else_is_not() {
        for bad in [
            "",
            "{0.0.2.00000000}.{8a4f1c2e-0b3d-4e5f-9a6b-7c8d9e0f1a2b}",
            "{0.0.0.00000001}.{8a4f1c2e-0b3d-4e5f-9a6b-7c8d9e0f1a2b}",
            "{0.0.0.00000000}.{8a4f1c2e-0b3d-4e5f-9a6b-7c8d9e0f1a2}",
            "{0.0.0.00000000}.{8a4f1c2e-0b3d-4e5f-9a6b-7c8d9e0f1a2g}",
            "{0.0.0.00000000}.{8a4f1c2e0b3d4e5f9a6b7c8d9e0f1a2b}",
            "{0.0.0.00000000}.{8a4f1c2e-0b3d-4e5f-9a6b-7c8d9e0f1a2b} ",
            "{0.0.0.00000000}.{8a4f1c2e-0b3d-4e5f-9a6b-7c8d9e0f1a2b}x",
            "{0.0.00.00000000}.{8a4f1c2e-0b3d-4e5f-9a6b-7c8d9e0f1a2b}",
            "C:\\Windows\\notepad.exe",
            "HKLM\\SOFTWARE\\x",
        ] {
            assert!(!is_endpoint_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn sound_device_parses_strictly() {
        assert_eq!(parse(&args(&["sound-device", "enable", ID])), Ok(Op::SoundDevice { enable: true, id: ID.into() }));
        assert_eq!(parse(&args(&["sound-device", "disable", ID])), Ok(Op::SoundDevice { enable: false, id: ID.into() }));
        assert!(parse(&args(&["sound-device", "toggle", ID])).is_err());
        assert!(parse(&args(&["sound-device", "Enable", ID])).is_err());
        assert!(parse(&args(&["sound-device", "enable", "x"])).is_err());
        assert!(parse(&args(&["sound-device", "enable"])).is_err());
        assert!(parse(&args(&["sound-device", "enable", ID, "extra"])).is_err());
        assert!(parse(&args(&["sound-device"])).is_err());
        assert!(parse(&args(&[])).is_err());
        assert!(parse(&args(&["run", "cmd.exe"])).is_err());
        assert!(parse(&args(&["--sound-device", "enable", ID])).is_err());
    }

    #[test]
    fn to_args_round_trips() {
        for enable in [true, false] {
            let op = Op::SoundDevice { enable, id: ID.into() };
            assert_eq!(parse(&op.to_args()), Ok(op));
        }
        assert!(Op::sound_device(true, "nope").is_none());
    }
}
