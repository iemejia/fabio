//! Shared UTC/RFC 3339 boundaries. Use the maintained `time` crate already in
//! the Azure SDK dependency graph; never enable its unsafe local-offset mode.

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub fn parse_rfc3339(value: &str) -> Result<OffsetDateTime, time::error::Parse> {
    OffsetDateTime::parse(value, &Rfc3339)
}

pub fn format_rfc3339(value: OffsetDateTime) -> String {
    value
        .format(&Rfc3339)
        .expect("UTC application timestamps are valid RFC 3339")
}

pub fn now_rfc3339() -> String {
    format_rfc3339(OffsetDateTime::now_utc())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_fractions_and_legacy_cache_timestamps_preserve_instants() {
        let utc = parse_rfc3339("2026-11-18T12:34:56.123456700Z").unwrap();
        assert_eq!(
            parse_rfc3339("2026-11-18T18:04:56.1234567+05:30").unwrap(),
            utc
        );
        assert_eq!(parse_rfc3339(&format_rfc3339(utc)).unwrap(), utc);
        assert_eq!(
            parse_rfc3339("2026-11-18T12:34:56+00:00").unwrap(),
            parse_rfc3339("2026-11-18T12:34:56Z").unwrap()
        );
        assert!(parse_rfc3339(&now_rfc3339()).is_ok());
    }

    #[test]
    fn malformed_timestamps_are_not_valid_instants() {
        for value in [
            "",
            "invalid",
            "2026-02-29T00:00:00Z",
            "2026-11-18",
            "2026-11-18T12:00:00",
            "2026-11-18T24:00:00Z",
        ] {
            assert!(parse_rfc3339(value).is_err(), "{value}");
        }
    }
}
