//! The saved font choice. A family name is interned so the choice stays `Copy`, like the rest of
//! `Appearance` (the apps copy it around); the few names ever picked are kept for the process.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::sync::Mutex;

/// The family "Default" means when it is installed (shown as "SF Pro").
pub const SF_DEFAULT: &str = "SF Pro Text";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FamilyName(&'static str);

impl FamilyName {
    pub fn new(name: &str) -> Self {
        static TABLE: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
        let mut table = TABLE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(known) = table.iter().find(|k| **k == name) {
            return Self(known);
        }
        let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
        table.push(leaked);
        Self(leaked)
    }

    pub fn as_str(&self) -> &'static str {
        self.0
    }
}

impl Serialize for FamilyName {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.0)
    }
}

impl<'de> Deserialize<'de> for FamilyName {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self::new(&String::deserialize(d)?))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum FontChoice {
    /// SF Pro when installed, else Segoe UI Variable.
    #[default]
    Default,
    Named(FamilyName),
}

impl FontChoice {
    pub fn named(name: &str) -> Self {
        Self::Named(FamilyName::new(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_shared_and_round_trip() {
        assert_eq!(FamilyName::new("Inter"), FamilyName::new("Inter"));
        let json = serde_json::to_string(&FontChoice::named("Inter")).unwrap();
        assert_eq!(json, r#"{"Named":"Inter"}"#);
        assert_eq!(serde_json::from_str::<FontChoice>(&json).unwrap(), FontChoice::named("Inter"));
        assert_eq!(serde_json::from_str::<FontChoice>("\"Default\"").unwrap(), FontChoice::Default);
    }
}
