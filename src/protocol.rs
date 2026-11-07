use serde::{Deserialize, Deserializer};

#[derive(Debug, PartialEq)]
pub enum HostMessage {
    Answer { name: String, value: String },
    Kill,
}

#[derive(Deserialize)]
enum Tagged {
    Answer { name: String, value: String },
}

impl<'de> Deserialize<'de> for HostMessage {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let is_kill = match &value {
            serde_json::Value::String(s) => s == "Kill",
            serde_json::Value::Object(map) => map.len() == 1 && map.contains_key("Kill"),
            _ => false,
        };
        if is_kill {
            return Ok(HostMessage::Kill);
        }
        match serde_json::from_value(value).map_err(serde::de::Error::custom)? {
            Tagged::Answer { name, value } => Ok(HostMessage::Answer { name, value }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_is_accepted_in_every_form() {
        for json in [r#""Kill""#, r#"{"Kill":null}"#, r#"{"Kill":{}}"#] {
            assert_eq!(serde_json::from_str::<HostMessage>(json).unwrap(), HostMessage::Kill, "{json}");
        }
    }

    #[test]
    fn answer_is_parsed() {
        let msg: HostMessage = serde_json::from_str(r#"{"Answer":{"name":"source","value":"abc"}}"#).unwrap();
        assert_eq!(msg, HostMessage::Answer { name: "source".into(), value: "abc".into() });
        assert!(serde_json::from_str::<HostMessage>(r#""Bogus""#).is_err());
    }
}
