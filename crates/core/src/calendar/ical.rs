//! iCalendar (RFC 5545) text: unfolded lines, properties with parameters, nested components,
//! and the value types the widget needs (dates, date-times, durations, text). Pure.

use super::time::{days_from_civil, wall, DAY};

#[derive(Clone, Debug, PartialEq)]
pub struct Prop {
    /// Upper case: `DTSTART`.
    pub name: String,
    /// Names upper case, values unquoted: `("TZID", "Europe/London")`.
    pub params: Vec<(String, String)>,
    pub value: String,
}

impl Prop {
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Component {
    /// Upper case: `VEVENT`.
    pub name: String,
    pub props: Vec<Prop>,
    pub children: Vec<Component>,
}

impl Component {
    pub fn prop(&self, name: &str) -> Option<&Prop> {
        self.props.iter().find(|p| p.name == name)
    }

    pub fn props_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Prop> + 'a {
        self.props.iter().filter(move |p| p.name == name)
    }

    pub fn text(&self, name: &str) -> Option<String> {
        self.prop(name).map(|p| unescape(&p.value))
    }

    /// Every descendant component named `name`, depth first.
    pub fn find_all<'a>(&'a self, name: &str, out: &mut Vec<&'a Component>) {
        for c in &self.children {
            if c.name == name {
                out.push(c);
            }
            c.find_all(name, out);
        }
    }
}

/// Lines with their folds undone (a line starting with a space or tab continues the one before).
fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = line.strip_prefix([' ', '\t']) {
            if let Some(last) = lines.last_mut() {
                last.push_str(rest);
                continue;
            }
        }
        if !line.is_empty() {
            lines.push(line.to_owned());
        }
    }
    lines
}

/// One content line: `NAME;P1=a;P2="b:c":value`.
fn parse_line(line: &str) -> Option<Prop> {
    // The name ends at the first ';' or ':'; parameter values may hold ':' inside quotes.
    let mut it = line.char_indices();
    let mut name = String::new();
    let mut sep = None;
    for (i, c) in it.by_ref() {
        if c == ';' || c == ':' {
            sep = Some((i, c));
            break;
        }
        name.push(c);
    }
    let (mut at, mut c) = sep?;
    let mut params = Vec::new();
    while c == ';' {
        // A parameter: NAME=value[,value], the value possibly quoted.
        let (mut pname, mut value) = (String::new(), String::new());
        let (mut in_value, mut quoted, mut next) = (false, false, None);
        for (j, ch) in it.by_ref() {
            if !in_value {
                if ch == '=' {
                    in_value = true;
                } else {
                    pname.push(ch);
                }
                continue;
            }
            match ch {
                '"' => quoted = !quoted,
                ';' | ':' if !quoted => {
                    next = Some((j, ch));
                    break;
                }
                _ => value.push(ch),
            }
        }
        params.push((pname.trim().to_ascii_uppercase(), value));
        (at, c) = next?;
    }
    Some(Prop { name: name.trim().to_ascii_uppercase(), params, value: line[at + 1..].to_owned() })
}

/// How deep components may nest. Real calendars go three deep (VCALENDAR, VEVENT, VALARM);
/// deeper ones are skipped with all they hold, so a hostile body of 100,000 nested BEGINs
/// cannot build a tree whose walk or drop overflows the stack.
pub const MAX_DEPTH: usize = 16;

/// The components in `text` (usually one `VCALENDAR`). Anything malformed is skipped, never
/// fatal: a calendar from any server must not take the widget down. Components nested deeper
/// than [`MAX_DEPTH`] are left out.
pub fn parse(text: &str) -> Vec<Component> {
    let mut stack: Vec<Component> = Vec::new();
    let mut top = Vec::new();
    // BEGINs past the depth limit not yet ENDed: everything until their END is ignored.
    let mut skipped = 0usize;
    for line in unfold(text) {
        let Some(prop) = parse_line(&line) else { continue };
        match prop.name.as_str() {
            "BEGIN" if skipped > 0 || stack.len() >= MAX_DEPTH => skipped += 1,
            "BEGIN" => stack.push(Component { name: prop.value.trim().to_ascii_uppercase(), ..Default::default() }),
            "END" if skipped > 0 => skipped -= 1,
            _ if skipped > 0 => {}
            "END" => {
                let Some(done) = stack.pop() else { continue };
                match stack.last_mut() {
                    Some(parent) => parent.children.push(done),
                    None => top.push(done),
                }
            }
            _ => {
                if let Some(current) = stack.last_mut() {
                    current.props.push(prop);
                }
            }
        }
    }
    top
}

/// Text with its escapes undone: `\n`, `\,`, `\;`, `\\`.
pub fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// A DATE or DATE-TIME value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DateValue {
    /// A whole day (all-day events), as days since 1970.
    Date(i64),
    /// A local time of no zone ("floating"): the viewer's own zone applies.
    Floating(i64),
    /// An instant, as seconds since 1970 UTC.
    Utc(i64),
    /// A wall time in the named zone.
    Zoned(i64, String),
}

impl DateValue {
    pub fn is_date(&self) -> bool {
        matches!(self, DateValue::Date(_))
    }
}

/// `20261112`, `20261112T140000`, `20261112T140000Z`.
fn parse_one(s: &str, tzid: Option<&str>, date_only: bool) -> Option<DateValue> {
    let s = s.trim();
    let num = |a: usize, b: usize| s.get(a..b).and_then(|t| t.parse::<u32>().ok());
    let (y, m, d) = (num(0, 4)? as i64, num(4, 6)?, num(6, 8)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    if date_only || s.len() == 8 {
        return Some(DateValue::Date(days_from_civil(y, m, d)));
    }
    if s.as_bytes().get(8) != Some(&b'T') {
        return None;
    }
    let (hh, mm, ss) = (num(9, 11)?, num(11, 13)?, num(13, 15)?);
    let w = wall(y, m, d, hh.min(23), mm.min(59), ss.min(60).min(59));
    if s.ends_with('Z') || s.ends_with('z') {
        return Some(DateValue::Utc(w));
    }
    Some(match tzid.filter(|t| !t.is_empty()) {
        Some(t) => DateValue::Zoned(w, t.trim_start_matches('/').to_owned()),
        None => DateValue::Floating(w),
    })
}

/// A property's date or date-time value (the first of a list).
pub fn date_value(p: &Prop) -> Option<DateValue> {
    date_values(p).into_iter().next()
}

/// A property's comma-separated dates (EXDATE, RDATE). PERIOD values are taken by their start.
pub fn date_values(p: &Prop) -> Vec<DateValue> {
    let date_only = p.param("VALUE").is_some_and(|v| v.eq_ignore_ascii_case("DATE"));
    p.value
        .split(',')
        .filter_map(|v| parse_one(v.split('/').next().unwrap_or(v), p.param("TZID"), date_only))
        .collect()
}

/// A DURATION in seconds: `PT1H30M`, `P1D`, `-PT15M`, `P2W`.
pub fn duration(s: &str) -> Option<i64> {
    let s = s.trim();
    let (sign, rest) = match s.as_bytes().first()? {
        b'-' => (-1, &s[1..]),
        b'+' => (1, &s[1..]),
        _ => (1, s),
    };
    let rest = rest.strip_prefix(['P', 'p'])?;
    let mut total = 0i64;
    let mut n = String::new();
    let mut seen = false;
    for c in rest.chars() {
        match c.to_ascii_uppercase() {
            '0'..='9' => n.push(c),
            'T' => {}
            unit => {
                let v: i64 = n.parse().ok()?;
                n.clear();
                seen = true;
                total += v * match unit {
                    'W' => 7 * DAY,
                    'D' => DAY,
                    'H' => 3600,
                    'M' => 60,
                    'S' => 1,
                    _ => return None,
                };
            }
        }
    }
    (seen && n.is_empty()).then_some(sign * total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn depth(c: &Component) -> usize {
        1 + c.children.iter().map(depth).max().unwrap_or(0)
    }

    #[test]
    fn deep_nesting_is_cut_off_and_cannot_overflow() {
        let n = 100_000;
        let mut text = String::from("BEGIN:VCALENDAR\r\n");
        text.push_str(&"BEGIN:X\r\n".repeat(n));
        text.push_str("SUMMARY:deep\r\n");
        text.push_str(&"END:X\r\n".repeat(n));
        text.push_str("BEGIN:VEVENT\r\nUID:after\r\nSUMMARY:Still read\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n");
        let comps = parse(&text);
        assert_eq!(comps.len(), 1);
        assert!(depth(&comps[0]) <= MAX_DEPTH, "{}", depth(&comps[0]));
        let mut events = Vec::new();
        comps[0].find_all("VEVENT", &mut events);
        assert_eq!(events.len(), 1, "the event after the deep part is still there");
        assert_eq!(events[0].text("UID").as_deref(), Some("after"));
        let mut xs = Vec::new();
        comps[0].find_all("X", &mut xs);
        assert!(xs.len() < MAX_DEPTH);
        assert!(xs.iter().all(|x| x.prop("SUMMARY").is_none()), "the cut-off part's lines are not given to an ancestor");
        // Dropping it is no trouble either (it is shallow).
        drop(comps);
    }

    #[test]
    fn folded_lines_params_and_nesting() {
        let text = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nSUMMARY:Design re\r\n view\\, with \\\"notes\\\"\r\nDTSTART;TZID=\"Europe/Copenhagen\";X-A=\"a:b\":20261112T140000\r\nLOCATION:Nørrebrogade 34\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let cal = parse(text);
        assert_eq!(cal.len(), 1);
        let ev = &cal[0].children[0];
        assert_eq!(ev.name, "VEVENT");
        assert_eq!(ev.text("SUMMARY").unwrap(), "Design review, with \"notes\"");
        let start = ev.prop("DTSTART").unwrap();
        assert_eq!(start.param("TZID"), Some("Europe/Copenhagen"));
        assert_eq!(start.param("X-A"), Some("a:b"));
        assert_eq!(start.value, "20261112T140000");
        assert_eq!(date_value(start), Some(DateValue::Zoned(wall(2026, 11, 12, 14, 0, 0), "Europe/Copenhagen".into())));
        assert_eq!(ev.text("LOCATION").unwrap(), "Nørrebrogade 34");
    }

    #[test]
    fn date_kinds_and_lists() {
        let p = |line: &str| parse_line(line).unwrap();
        assert_eq!(date_value(&p("DTSTART;VALUE=DATE:20261112")), Some(DateValue::Date(20_769)));
        assert_eq!(date_value(&p("DTSTART:20261112")), Some(DateValue::Date(20_769)));
        assert_eq!(date_value(&p("DTSTART:20261112T090000Z")), Some(DateValue::Utc(wall(2026, 11, 12, 9, 0, 0))));
        assert_eq!(date_value(&p("DTSTART:20261112T090000")), Some(DateValue::Floating(wall(2026, 11, 12, 9, 0, 0))));
        let ex = date_values(&p("EXDATE;TZID=Europe/London:20261112T090000,20261119T090000"));
        assert_eq!(ex.len(), 2);
        assert_eq!(date_value(&p("DTSTART:garbage")), None);
        assert_eq!(date_value(&p("DTSTART:20261340")), None);
    }

    #[test]
    fn durations() {
        assert_eq!(duration("PT1H30M"), Some(5400));
        assert_eq!(duration("P1D"), Some(DAY));
        assert_eq!(duration("P2W"), Some(14 * DAY));
        assert_eq!(duration("-PT15M"), Some(-900));
        assert_eq!(duration("P1DT2H"), Some(DAY + 7200));
        assert_eq!(duration("PT"), None);
        assert_eq!(duration("1H"), None);
    }

    #[test]
    fn junk_never_panics() {
        for s in ["", "BEGIN", "END:VEVENT", ":::", "A;B", "A;B=\"unterminated:x", "BEGIN:VEVENT\nDTSTART;TZID=:2026"] {
            let _ = parse(s);
        }
    }
}
