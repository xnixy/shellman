//! Reading a toast's XML (the payload an app hands Windows): its text, logo, buttons and reply
//! box. Pure; everything shellman does not show (audio, hero images, progress bars, selection
//! boxes, scenarios) is ignored.

use super::model::{Action, Activation, Reply};
use std::path::PathBuf;

/// What shellman takes from a toast's XML.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Parsed {
    pub launch: String,
    pub launch_kind: Activation,
    /// The binding's text lines in order, without the attribution: the first is the title.
    pub texts: Vec<String>,
    pub attribution: Option<String>,
    /// `appLogoOverride`, when it is a file on disk.
    pub logo: Option<PathBuf>,
    /// Buttons shown on the toast (not context-menu entries, not Windows' own), without the
    /// reply's send button.
    pub actions: Vec<Action>,
    pub reply: Option<Reply>,
}

/// Parse a toast's XML; None if it is not XML with a `toast` root.
pub fn parse(xml: &str) -> Option<Parsed> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let toast = doc.root_element();
    if !toast.has_tag_name("toast") {
        return None;
    }
    let mut out = Parsed {
        launch: toast.attribute("launch").unwrap_or_default().to_owned(),
        launch_kind: Activation::parse(toast.attribute("activationType")),
        ..Default::default()
    };
    if let Some(binding) = binding(&toast) {
        for node in binding.descendants().filter(|n| n.is_element()) {
            match node.tag_name().name() {
                "text" => {
                    let text = node.text().unwrap_or_default().trim().to_owned();
                    if node.attribute("placement") == Some("attribution") {
                        out.attribution = Some(text).filter(|t| !t.is_empty());
                    } else if !text.is_empty() {
                        out.texts.push(text);
                    }
                }
                "image" if node.attribute("placement") == Some("appLogoOverride") => {
                    out.logo = node.attribute("src").and_then(local_file);
                }
                _ => {}
            }
        }
    }
    if let Some(actions) = toast.children().find(|n| n.has_tag_name("actions")) {
        read_actions(&actions, &mut out);
    }
    Some(out)
}

/// The generic binding, or the first binding there is.
fn binding<'a>(toast: &roxmltree::Node<'a, 'a>) -> Option<roxmltree::Node<'a, 'a>> {
    let visual = toast.children().find(|n| n.has_tag_name("visual"))?;
    let bindings = || visual.children().filter(|n| n.has_tag_name("binding"));
    bindings()
        .find(|b| b.attribute("template").is_some_and(|t| t.eq_ignore_ascii_case("ToastGeneric")))
        .or_else(|| bindings().next())
}

fn read_actions(actions: &roxmltree::Node, out: &mut Parsed) {
    // Text boxes first: a button names the box it sends.
    let boxes: Vec<(String, String)> = actions
        .children()
        .filter(|n| n.has_tag_name("input") && n.attribute("type").is_none_or(|t| t.eq_ignore_ascii_case("text")))
        .filter_map(|n| {
            let id = n.attribute("id")?.to_owned();
            Some((id, n.attribute("placeHolderContent").unwrap_or_default().to_owned()))
        })
        .collect();
    for node in actions.children().filter(|n| n.has_tag_name("action")) {
        let kind = Activation::parse(node.attribute("activationType"));
        if kind == Activation::System || node.attribute("placement") == Some("contextMenu") {
            continue;
        }
        let action = Action {
            label: node.attribute("content").unwrap_or_default().to_owned(),
            args: node.attribute("arguments").unwrap_or_default().to_owned(),
            kind,
            input: node.attribute("hint-inputId").map(str::to_owned),
        };
        let sends = action.input.as_ref().and_then(|id| boxes.iter().find(|(b, _)| b == id));
        match sends {
            Some((id, placeholder)) if out.reply.is_none() => {
                out.reply = Some(Reply { id: id.clone(), placeholder: placeholder.clone(), send: action });
            }
            _ if !action.label.is_empty() => out.actions.push(action),
            _ => {}
        }
    }
}

/// A `src` that names a file on this machine: a plain path or a `file:` URI. Packaged apps'
/// `ms-appx:` and web images are not files shellman can read.
pub fn local_file(src: &str) -> Option<PathBuf> {
    let src = src.trim();
    let path = match src.get(..8) {
        Some(p) if p.eq_ignore_ascii_case("file:///") => src[8..].replace('/', "\\"),
        _ if src.contains("://") || src.starts_with("ms-") => return None,
        _ => src.to_owned(),
    };
    let path = percent_decode(&path);
    let p = PathBuf::from(&path);
    p.is_absolute().then_some(p)
}

/// `%20` and friends, as `file:` URIs carry them.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    // As Chromium-based browsers send them (taken from this machine's store).
    const BROWSER: &str = r#"<toast launch="0|2|Default|Helium.X|0|chrome-extension://a/|a-item" displayTimestamp="2026-11-12T20:09:11Z">
 <visual>
  <binding template="ToastGeneric">
   <text>Login saved</text>
   <text>"GIPHY" was saved in "Personal".</text>
   <image placement="appLogoOverride" src="C:\Users\X\AppData\Local\Helium\Notification Resources\ae.tmp" hint-crop="none"/>
  </binding>
 </visual>
 <actions>
  <action content="Go to notification settings" placement="contextMenu" activationType="foreground" arguments="2|2"/>
 </actions>
</toast>"#;

    const CHAT: &str = r#"<toast launch="action=open&amp;n=1" activationType="foreground">
<visual><binding template="ToastGeneric"><text>nix in #general</text><text>are you on tonight?</text><text>we need a fourth</text><text placement="attribution">via Discord</text></binding></visual>
<actions><input id="reply" type="text" placeHolderContent="Reply"/><input id="pick" type="selection"/>
<action content="Send" arguments="action=reply" activationType="background" hint-inputId="reply"/>
<action content="Mute" arguments="action=mute"/>
<action content="" arguments="dismiss" activationType="system"/></actions>
</toast>"#;

    #[test]
    fn a_browser_toast_has_its_text_logo_and_no_buttons() {
        let p = parse(BROWSER).unwrap();
        assert_eq!(p.texts, ["Login saved", "\"GIPHY\" was saved in \"Personal\"."]);
        assert_eq!(p.launch, "0|2|Default|Helium.X|0|chrome-extension://a/|a-item");
        assert_eq!(p.launch_kind, Activation::Foreground);
        assert_eq!(p.logo, Some(PathBuf::from(r"C:\Users\X\AppData\Local\Helium\Notification Resources\ae.tmp")));
        // The context-menu entry is not a button.
        assert!(p.actions.is_empty() && p.reply.is_none());
    }

    #[test]
    fn a_chat_toast_has_a_reply_box_and_a_button() {
        let p = parse(CHAT).unwrap();
        assert_eq!(p.texts, ["nix in #general", "are you on tonight?", "we need a fourth"]);
        assert_eq!(p.attribution.as_deref(), Some("via Discord"));
        assert_eq!(p.launch, "action=open&n=1");
        let reply = p.reply.unwrap();
        assert_eq!((reply.id.as_str(), reply.placeholder.as_str()), ("reply", "Reply"));
        assert_eq!(reply.send.args, "action=reply");
        assert_eq!(reply.send.kind, Activation::Background);
        // The send button is the reply's, the system one is left out.
        assert_eq!(p.actions.len(), 1);
        assert_eq!((p.actions[0].label.as_str(), p.actions[0].args.as_str()), ("Mute", "action=mute"));
    }

    #[test]
    fn a_button_naming_a_missing_box_is_just_a_button() {
        let xml = r#"<toast><visual><binding template="ToastGeneric"><text>t</text></binding></visual>
            <actions><action content="Go" arguments="go" hint-inputId="nope"/></actions></toast>"#;
        let p = parse(xml).unwrap();
        assert!(p.reply.is_none());
        assert_eq!(p.actions.len(), 1);
    }

    #[test]
    fn old_templates_and_protocol_launches() {
        let xml = r#"<toast launch="https://example.com/x" activationType="protocol"><visual><binding template="ToastText02"><text id="1">Hello</text><text id="2"> there </text></binding></visual></toast>"#;
        let p = parse(xml).unwrap();
        assert_eq!(p.texts, ["Hello", "there"]);
        assert_eq!(p.launch_kind, Activation::Protocol);
    }

    #[test]
    fn not_a_toast() {
        assert!(parse("<tile/>").is_none());
        assert!(parse("not xml").is_none());
    }

    #[test]
    fn image_sources_that_are_files() {
        assert_eq!(local_file("file:///C:/a%20b/c.png"), Some(PathBuf::from(r"C:\a b\c.png")));
        assert_eq!(local_file(r"C:\x\y.png"), Some(PathBuf::from(r"C:\x\y.png")));
        assert_eq!(local_file("ms-appx:///Assets/logo.png"), None);
        assert_eq!(local_file("https://example.com/a.png"), None);
        assert_eq!(local_file("relative.png"), None);
    }
}
