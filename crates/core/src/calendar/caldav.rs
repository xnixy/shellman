//! CalDAV (RFC 4791) on the wire: the request bodies the widget sends and the multistatus
//! answers it reads. Pure; the requests themselves go out in `client`.

use serde::{Deserialize, Serialize};

pub const DAV: &str = "DAV:";
pub const CALDAV: &str = "urn:ietf:params:xml:ns:caldav";
pub const APPLE: &str = "http://apple.com/ns/ical/";

/// Who the account is: the principal's URL.
pub const PRINCIPAL_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:"><d:prop><d:current-user-principal/></d:prop></d:propfind>"#;

/// Where its calendars live.
pub const HOME_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><c:calendar-home-set/></d:prop></d:propfind>"#;

/// The calendars in the home: kind, name, colour, what they hold.
pub const LIST_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:a="http://apple.com/ns/ical/"><d:prop><d:resourcetype/><d:displayname/><a:calendar-color/><c:supported-calendar-component-set/></d:prop></d:propfind>"#;

/// The events of a calendar that touch `[from, to)`, given as `YYYYMMDDTHHMMSSZ`.
pub fn query_body(from: &str, to: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><d:getetag/><c:calendar-data/></d:prop><c:filter><c:comp-filter name="VCALENDAR"><c:comp-filter name="VEVENT"><c:time-range start="{from}" end="{to}"/></c:comp-filter></c:comp-filter></c:filter></c:calendar-query>"#
    )
}

/// One calendar of the account.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CalendarRef {
    /// Its URL, absolute.
    pub href: String,
    pub name: String,
    /// `#RRGGBB`, when the server says.
    pub color: Option<String>,
}

/// One `<response>`: its href and the properties that came back with status 200.
#[derive(Debug, Default)]
pub struct Response<'a> {
    pub href: String,
    props: Vec<roxmltree::Node<'a, 'a>>,
}

impl<'a> Response<'a> {
    fn prop(&self, ns: &str, name: &str) -> Option<roxmltree::Node<'a, 'a>> {
        self.props.iter().copied().find(|n| n.tag_name().namespace() == Some(ns) && n.tag_name().name() == name)
    }

    /// The text of a property.
    pub fn text(&self, ns: &str, name: &str) -> Option<String> {
        let n = self.prop(ns, name)?;
        let t: String = n.descendants().filter(|d| d.is_text()).filter_map(|d| d.text()).collect();
        let t = t.trim().to_owned();
        (!t.is_empty()).then_some(t)
    }

    /// The `<href>` inside a property (`current-user-principal`, `calendar-home-set`).
    pub fn href_in(&self, ns: &str, name: &str) -> Option<String> {
        let n = self.prop(ns, name)?;
        n.descendants()
            .find(|d| d.tag_name().namespace() == Some(DAV) && d.tag_name().name() == "href")
            .and_then(|h| h.text())
            .map(|t| t.trim().to_owned())
    }

    /// Whether a property holds a child element `ns:name` (`resourcetype` holding `calendar`).
    pub fn has_child(&self, prop_ns: &str, prop: &str, ns: &str, name: &str) -> bool {
        self.prop(prop_ns, prop)
            .is_some_and(|p| p.descendants().any(|d| d.tag_name().namespace() == Some(ns) && d.tag_name().name() == name))
    }

    /// The component names a calendar holds; empty when the server did not say.
    pub fn components(&self) -> Vec<String> {
        self.prop(CALDAV, "supported-calendar-component-set")
            .map(|p| {
                p.descendants()
                    .filter(|d| d.tag_name().name() == "comp")
                    .filter_map(|d| d.attribute("name").map(|s| s.to_ascii_uppercase()))
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Read a multistatus document and hand each response to `each`.
pub fn multistatus<T>(xml: &str, mut each: impl FnMut(&Response) -> Option<T>) -> Result<Vec<T>, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| format!("not XML: {e}"))?;
    let root = doc.root_element();
    if root.tag_name().name() != "multistatus" {
        return Err(format!("expected a multistatus, got <{}>", root.tag_name().name()));
    }
    let mut out = Vec::new();
    for resp in root.children().filter(|n| n.tag_name().namespace() == Some(DAV) && n.tag_name().name() == "response") {
        let href = resp
            .children()
            .find(|n| n.tag_name().namespace() == Some(DAV) && n.tag_name().name() == "href")
            .and_then(|n| n.text())
            .map(|t| t.trim().to_owned())
            .unwrap_or_default();
        let mut props = Vec::new();
        for ps in resp.children().filter(|n| n.tag_name().name() == "propstat") {
            let ok = ps
                .children()
                .find(|n| n.tag_name().name() == "status")
                .and_then(|n| n.text())
                .is_none_or(|s| s.contains(" 200"));
            if !ok {
                continue;
            }
            if let Some(prop) = ps.children().find(|n| n.tag_name().name() == "prop") {
                props.extend(prop.children().filter(|n| n.is_element()));
            }
        }
        let r = Response { href, props };
        if let Some(v) = each(&r) {
            out.push(v);
        }
    }
    Ok(out)
}

/// `href` made absolute against `base` (`https://host[:port]/path`).
pub fn absolute(base: &str, href: &str) -> String {
    let href = href.trim();
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_owned();
    }
    let scheme_end = base.find("://").map_or(0, |i| i + 3);
    let host_end = base[scheme_end..].find('/').map_or(base.len(), |i| scheme_end + i);
    if href.starts_with('/') {
        return format!("{}{}", &base[..host_end], href);
    }
    // Relative to the base's folder.
    let dir_end = base.rfind('/').filter(|i| *i >= host_end).map_or(base.len(), |i| i + 1);
    let mut dir = base[..dir_end].to_owned();
    if !dir.ends_with('/') {
        dir.push('/');
    }
    format!("{dir}{href}")
}

/// The principal's URL from a PROPFIND answer.
pub fn principal(xml: &str, base: &str) -> Result<Option<String>, String> {
    Ok(multistatus(xml, |r| r.href_in(DAV, "current-user-principal"))?.into_iter().next().map(|h| absolute(base, &h)))
}

/// The calendar home's URL.
pub fn home(xml: &str, base: &str) -> Result<Option<String>, String> {
    Ok(multistatus(xml, |r| r.href_in(CALDAV, "calendar-home-set"))?.into_iter().next().map(|h| absolute(base, &h)))
}

/// `#RRGGBBAA` or `#RRGGBB` (Apple's calendar-color) as `#RRGGBB`.
fn color(s: &str) -> Option<String> {
    let s = s.trim();
    let hex = s.strip_prefix('#')?;
    (hex.len() >= 6 && hex[..6].chars().all(|c| c.is_ascii_hexdigit())).then(|| format!("#{}", hex[..6].to_ascii_uppercase()))
}

/// The calendars that hold events, from a Depth 1 PROPFIND of the home.
pub fn calendars(xml: &str, base: &str) -> Result<Vec<CalendarRef>, String> {
    multistatus(xml, |r| {
        if !r.has_child(DAV, "resourcetype", CALDAV, "calendar") {
            return None;
        }
        let comps = r.components();
        if !comps.is_empty() && !comps.iter().any(|c| c == "VEVENT") {
            return None;
        }
        let href = absolute(base, &r.href);
        let name = r.text(DAV, "displayname").unwrap_or_else(|| {
            href.trim_end_matches('/').rsplit('/').next().unwrap_or("Calendar").to_owned()
        });
        Some(CalendarRef { href, name, color: r.text(APPLE, "calendar-color").and_then(|c| color(&c)) })
    })
}

/// The iCalendar texts of a calendar-query answer.
pub fn calendar_data(xml: &str) -> Result<Vec<String>, String> {
    multistatus(xml, |r| r.text(CALDAV, "calendar-data"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shaped as Fastmail answers (namespaces as it writes them).
    const PRINCIPAL: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<D:multistatus xmlns:D="DAV:"><D:response><D:href>/dav/</D:href><D:propstat><D:prop><D:current-user-principal><D:href>/dav/principals/user/someone@fastmail.com/</D:href></D:current-user-principal></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response></D:multistatus>"#;

    const HOME: &str = r#"<?xml version="1.0"?><multistatus xmlns="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav"><response><href>/dav/principals/user/someone@fastmail.com/</href><propstat><prop><C:calendar-home-set><href>/dav/calendars/user/someone@fastmail.com/</href></C:calendar-home-set></prop><status>HTTP/1.1 200 OK</status></propstat></response></multistatus>"#;

    const LIST: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav" xmlns:x1="http://apple.com/ns/ical/">
 <d:response><d:href>/dav/calendars/user/someone@fastmail.com/</d:href>
  <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:response><d:href>/dav/calendars/user/someone@fastmail.com/Default/</d:href>
  <d:propstat><d:prop><d:resourcetype><d:collection/><cal:calendar/></d:resourcetype><d:displayname>Personal</d:displayname><x1:calendar-color>#3a87adff</x1:calendar-color>
   <cal:supported-calendar-component-set><cal:comp name="VEVENT"/><cal:comp name="VTODO"/></cal:supported-calendar-component-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:response><d:href>/dav/calendars/user/someone@fastmail.com/work-1234/</d:href>
  <d:propstat><d:prop><d:resourcetype><d:collection/><cal:calendar/></d:resourcetype><d:displayname>Work &amp; projects</d:displayname></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat>
  <d:propstat><d:prop><x1:calendar-color/></d:prop><d:status>HTTP/1.1 404 Not Found</d:status></d:propstat></d:response>
 <d:response><d:href>/dav/calendars/user/someone@fastmail.com/tasks/</d:href>
  <d:propstat><d:prop><d:resourcetype><d:collection/><cal:calendar/></d:resourcetype><d:displayname>Tasks</d:displayname><cal:supported-calendar-component-set><cal:comp name="VTODO"/></cal:supported-calendar-component-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
</d:multistatus>"##;

    const REPORT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<d:multistatus xmlns:d="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
<d:response><d:href>/dav/calendars/user/someone@fastmail.com/Default/a.ics</d:href><d:propstat><d:prop><d:getetag>"1"</d:getetag><C:calendar-data>BEGIN:VCALENDAR&#13;
BEGIN:VEVENT&#13;
UID:a&#13;
SUMMARY:Design review&#13;
DTSTART:20261112T130000Z&#13;
END:VEVENT&#13;
END:VCALENDAR&#13;
</C:calendar-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
<d:response><d:href>/dav/calendars/user/someone@fastmail.com/Default/b.ics</d:href><d:propstat><d:prop><C:calendar-data><![CDATA[BEGIN:VCALENDAR
BEGIN:VEVENT
UID:b
SUMMARY:Dentist
DTSTART:20261112T153000Z
END:VEVENT
END:VCALENDAR
]]></C:calendar-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
</d:multistatus>"#;

    const BASE: &str = "https://caldav.fastmail.com/dav/";

    #[test]
    fn discovery_steps() {
        assert_eq!(principal(PRINCIPAL, BASE).unwrap().unwrap(), "https://caldav.fastmail.com/dav/principals/user/someone@fastmail.com/");
        assert_eq!(home(HOME, BASE).unwrap().unwrap(), "https://caldav.fastmail.com/dav/calendars/user/someone@fastmail.com/");
        let cals = calendars(LIST, BASE).unwrap();
        assert_eq!(cals.len(), 2, "{cals:?}");
        assert_eq!(cals[0].name, "Personal");
        assert_eq!(cals[0].color.as_deref(), Some("#3A87AD"));
        assert_eq!(cals[0].href, "https://caldav.fastmail.com/dav/calendars/user/someone@fastmail.com/Default/");
        assert_eq!(cals[1].name, "Work & projects");
        assert_eq!(cals[1].color, None);
    }

    #[test]
    fn report_gives_the_ical_texts() {
        let data = calendar_data(REPORT).unwrap();
        assert_eq!(data.len(), 2);
        assert!(data[0].contains("SUMMARY:Design review"));
        assert!(data[1].contains("UID:b"));
        let comps = crate::calendar::ical::parse(&data[0]);
        assert_eq!(comps[0].children[0].text("UID").unwrap(), "a");
    }

    #[test]
    fn bad_answers_are_errors_not_panics() {
        assert!(calendars("<html>nope</html>", BASE).is_err());
        assert!(calendars("not xml", BASE).is_err());
        assert_eq!(principal(r#"<multistatus xmlns="DAV:"/>"#, BASE).unwrap(), None);
    }

    #[test]
    fn hrefs_resolve() {
        assert_eq!(absolute("https://h.example:8443/a/b/", "/x/"), "https://h.example:8443/x/");
        assert_eq!(absolute("https://h.example/a/b/", "c/"), "https://h.example/a/b/c/");
        assert_eq!(absolute("https://h.example", "/x"), "https://h.example/x");
        assert_eq!(absolute("https://h.example/a/", "https://o.example/y"), "https://o.example/y");
        assert!(query_body("20261111T000000Z", "20261126T000000Z").contains(r#"start="20261111T000000Z""#));
    }
}
