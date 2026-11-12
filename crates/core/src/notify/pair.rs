//! Putting a notification together from the platform's two halves. The listener gives each
//! notification's id, app, time and text, but not its XML; the app's toast history gives the XML
//! (launch arguments, buttons, reply box, logo) but no id. They are paired by app and text. Pure.

use super::model::Toast;
use super::xml::{self, Parsed};
use std::collections::HashMap;
use std::time::SystemTime;

/// A notification as the listener reports it.
#[derive(Clone, PartialEq, Debug)]
pub struct Listed {
    pub id: u32,
    pub app_id: String,
    pub app_name: String,
    pub texts: Vec<String>,
    pub time: SystemTime,
}

/// Build the toasts, oldest first, from the listener's entries and each app's history XML.
/// A notification whose XML is not found still shows, with its text and nothing to press.
pub fn assemble(mut listed: Vec<Listed>, history: &HashMap<String, Vec<String>>) -> Vec<Toast> {
    listed.sort_by_key(|l| (l.time, l.id));
    // Each app's payloads, parsed once; a payload pairs with one notification at most.
    let mut parsed: HashMap<&str, Vec<Option<Parsed>>> = history
        .iter()
        .map(|(app, xmls)| (app.as_str(), xmls.iter().map(|x| xml::parse(x)).collect()))
        .collect();
    listed
        .into_iter()
        .map(|l| {
            let found = parsed.get_mut(l.app_id.as_str()).and_then(|list| take_match(list, &l.texts));
            build(l, found)
        })
        .collect()
}

/// The first unused payload with the same text, else the first with the same title.
fn take_match(list: &mut [Option<Parsed>], texts: &[String]) -> Option<Parsed> {
    let same = list.iter().position(|p| p.as_ref().is_some_and(|p| p.texts == texts));
    let title = || {
        list.iter()
            .position(|p| p.as_ref().is_some_and(|p| !texts.is_empty() && p.texts.first() == texts.first()))
    };
    let i = same.or_else(title)?;
    list[i].take()
}

fn build(l: Listed, parsed: Option<Parsed>) -> Toast {
    let p = parsed.unwrap_or_default();
    // The listener's text is what Windows itself shows; fall back on the XML's.
    let texts = if l.texts.is_empty() { p.texts } else { l.texts };
    let mut lines = texts.into_iter();
    let title = lines.next().unwrap_or_default();
    Toast {
        id: l.id,
        app_name: if l.app_name.is_empty() { l.app_id.clone() } else { l.app_name },
        app_id: l.app_id,
        app_icon: p.logo,
        title,
        body: lines.collect(),
        attribution: p.attribution,
        time: l.time,
        launch: p.launch,
        launch_kind: p.launch_kind,
        actions: p.actions,
        reply: p.reply,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn listed(id: u32, app: &str, texts: &[&str], secs: u64) -> Listed {
        Listed {
            id,
            app_id: app.into(),
            app_name: format!("{app} name"),
            texts: texts.iter().map(|t| t.to_string()).collect(),
            time: SystemTime::UNIX_EPOCH + Duration::from_secs(secs),
        }
    }

    fn toast_xml(texts: &[&str], launch: &str) -> String {
        let t: String = texts.iter().map(|t| format!("<text>{t}</text>")).collect();
        format!(r#"<toast launch="{launch}"><visual><binding template="ToastGeneric">{t}</binding></visual></toast>"#)
    }

    #[test]
    fn pairs_by_app_and_text_and_sorts_oldest_first() {
        let list = vec![listed(9, "b", &["Hi", "there"], 20), listed(4, "a", &["Done"], 10)];
        let mut history = HashMap::new();
        history.insert("a".to_string(), vec![toast_xml(&["Other"], "x"), toast_xml(&["Done"], "a-launch")]);
        history.insert("b".to_string(), vec![toast_xml(&["Hi", "there"], "b-launch")]);
        let toasts = assemble(list, &history);
        assert_eq!(toasts.iter().map(|t| t.id).collect::<Vec<_>>(), [4, 9]);
        assert_eq!(toasts[0].launch, "a-launch");
        assert_eq!((toasts[1].title.as_str(), toasts[1].body.as_slice()), ("Hi", &["there".to_string()][..]));
        assert_eq!(toasts[1].launch, "b-launch");
        assert_eq!(toasts[1].app_name, "b name");
    }

    #[test]
    fn identical_toasts_each_take_their_own_payload() {
        let list = vec![listed(1, "a", &["Same"], 1), listed(2, "a", &["Same"], 2)];
        let mut history = HashMap::new();
        history.insert("a".to_string(), vec![toast_xml(&["Same"], "first"), toast_xml(&["Same"], "second")]);
        let toasts = assemble(list, &history);
        assert_eq!((toasts[0].launch.as_str(), toasts[1].launch.as_str()), ("first", "second"));
    }

    #[test]
    fn a_changed_body_still_pairs_by_title() {
        let list = vec![listed(1, "a", &["Download", "3 of 3"], 1)];
        let mut history = HashMap::new();
        history.insert("a".to_string(), vec![toast_xml(&["Download", "1 of 3"], "dl")]);
        let toasts = assemble(list, &history);
        assert_eq!(toasts[0].launch, "dl");
        // The listener's text wins.
        assert_eq!(toasts[0].body, ["3 of 3"]);
    }

    #[test]
    fn without_xml_it_shows_its_text_and_has_nothing_to_press() {
        let toasts = assemble(vec![listed(1, "a", &["Only text"], 1)], &HashMap::new());
        assert_eq!(toasts[0].title, "Only text");
        assert!(toasts[0].launch.is_empty() && toasts[0].actions.is_empty() && !toasts[0].has_reply());
    }
}
