use super::RouteRequest;

/// Builds a collision-resistant, stable session key for a routed conversation.
#[must_use]
pub fn stable_session_key(request: &RouteRequest) -> String {
    format!(
        "channel:{}:account:{}:peer:{}:guild:{}:team:{}",
        encode_component(&request.channel),
        encode_optional_component(request.account_id.as_deref()),
        encode_optional_component(request.peer_id.as_deref()),
        encode_optional_component(request.guild_id.as_deref()),
        encode_optional_component(request.team_id.as_deref()),
    )
}

fn encode_optional_component(value: Option<&str>) -> String {
    value.map_or_else(|| "~".to_owned(), encode_component)
}

fn encode_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(hex_digit(byte >> 4));
            encoded.push(hex_digit(byte & 0x0f));
        }
    }
    encoded
}

fn hex_digit(value: u8) -> char {
    match value {
        0..=9 => char::from(b'0' + value),
        10..=15 => char::from(b'A' + (value - 10)),
        _ => unreachable!("nibble must be in range"),
    }
}

#[cfg(test)]
mod tests {
    use super::stable_session_key;
    use crate::routing::RouteRequest;

    #[test]
    fn keeps_absent_and_delimiter_containing_components_distinct() {
        let missing_peer = RouteRequest {
            channel: "desktop".to_owned(),
            ..RouteRequest::default()
        };
        let literal_peer = RouteRequest {
            channel: "desktop".to_owned(),
            peer_id: Some("~:peer".to_owned()),
            ..RouteRequest::default()
        };

        let missing_key = stable_session_key(&missing_peer);
        let literal_key = stable_session_key(&literal_peer);

        assert_ne!(missing_key, literal_key);
        assert_eq!(
            literal_key,
            "channel:desktop:account:~:peer:%7E%3Apeer:guild:~:team:~"
        );
    }
}
