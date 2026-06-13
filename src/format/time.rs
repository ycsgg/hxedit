#[derive(Clone, Copy)]
pub struct FractionPrecision {
    pub digits: usize,
    pub units: u32,
    pub label: &'static str,
}

pub const MICROSECOND: FractionPrecision = FractionPrecision {
    digits: 6,
    units: 1_000_000,
    label: "microsecond",
};

pub const NANOSECOND: FractionPrecision = FractionPrecision {
    digits: 9,
    units: 1_000_000_000,
    label: "nanosecond",
};

pub fn format_unix_utc(seconds: u32) -> String {
    let (year, month, day, hour, minute, second) = split_unix_seconds(seconds);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

pub fn format_unix_utc_fraction(
    seconds: u32,
    fraction: u32,
    precision: FractionPrecision,
    fraction_field_name: &str,
) -> String {
    if fraction >= precision.units {
        return format!(
            "invalid timestamp (ts_sec=0x{seconds:08x}, {fraction_field_name}=0x{fraction:08x})"
        );
    }

    let (year, month, day, hour, minute, second) = split_unix_seconds(seconds);
    let width = precision.digits;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{fraction:0width$}Z")
}

pub fn parse_unix_utc(input: &str) -> Result<u32, String> {
    let (seconds, _) = parse_timestamp(input, None)?;
    Ok(seconds)
}

pub fn parse_unix_utc_fraction(
    input: &str,
    precision: FractionPrecision,
) -> Result<(u32, u32), String> {
    parse_timestamp(input, Some(precision))
}

pub fn format_dos_datetime(time: u16, date: u16) -> String {
    let day = u32::from(date & 0x001f);
    let month = u32::from((date >> 5) & 0x000f);
    let year = i32::from((date >> 9) & 0x007f) + 1980;
    let second = u32::from(time & 0x001f) * 2;
    let minute = u32::from((time >> 5) & 0x003f);
    let hour = u32::from((time >> 11) & 0x001f);

    if hour > 23 || minute > 59 || second > 59 || !valid_civil_date(year, month, day) {
        return format!("invalid DOS datetime (time=0x{time:04x}, date=0x{date:04x})");
    }

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02} (DOS local)")
}

pub fn encode_dos_datetime_le(input: &str) -> Result<Vec<u8>, String> {
    let (time, date) = parse_dos_datetime(input)?;
    let mut bytes = Vec::with_capacity(4);
    bytes.extend_from_slice(&time.to_le_bytes());
    bytes.extend_from_slice(&date.to_le_bytes());
    Ok(bytes)
}

pub fn parse_dos_datetime(input: &str) -> Result<(u16, u16), String> {
    let input = strip_parenthesized_suffix(input.trim());
    let (year, month, day, hour, minute, second) =
        parse_datetime_components(input, "DOS datetime")?;
    if !(1980..=2107).contains(&year) {
        return Err("DOS datetime year must be in 1980..=2107".into());
    }
    if second % 2 != 0 {
        return Err("DOS datetime stores seconds with two-second precision".into());
    }

    let time = ((hour as u16) << 11) | ((minute as u16) << 5) | ((second as u16) / 2);
    let date = (((year - 1980) as u16) << 9) | ((month as u16) << 5) | day as u16;
    Ok((time, date))
}

fn parse_timestamp(
    input: &str,
    precision: Option<FractionPrecision>,
) -> Result<(u32, u32), String> {
    let input = strip_parenthesized_suffix(input.trim());
    if input.is_empty() {
        return Err("timestamp is empty".into());
    }
    if input
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b'.')
    {
        return parse_epoch_timestamp(input, precision);
    }
    parse_iso_timestamp(input, precision)
}

fn parse_epoch_timestamp(
    input: &str,
    precision: Option<FractionPrecision>,
) -> Result<(u32, u32), String> {
    let (seconds, fraction) = input.split_once('.').unwrap_or((input, ""));
    if seconds.is_empty() {
        return Err("timestamp seconds are missing".into());
    }
    let seconds = seconds
        .parse::<u32>()
        .map_err(|err| format!("invalid timestamp seconds: {err}"))?;
    let fraction = parse_fraction(fraction, precision)?;
    Ok((seconds, fraction))
}

fn parse_iso_timestamp(
    input: &str,
    precision: Option<FractionPrecision>,
) -> Result<(u32, u32), String> {
    let input = input
        .strip_suffix('Z')
        .or_else(|| input.strip_suffix('z'))
        .unwrap_or(input);
    let (date, time) = input
        .split_once('T')
        .or_else(|| input.split_once(' '))
        .ok_or_else(|| "expected UTC timestamp like 2024-07-03T09:46:40Z".to_owned())?;

    let (year, month, day) = parse_date(date)?;
    let (hms, fraction) = time.split_once('.').unwrap_or((time, ""));
    let (hour, minute, second) = parse_time(hms)?;
    if !valid_civil_date(year, month, day) {
        return Err("timestamp date is invalid".into());
    }

    let days = days_from_civil(year, month, day)
        .ok_or_else(|| "timestamp date is out of range".to_owned())?;
    let total_seconds = days
        .checked_mul(86_400)
        .and_then(|value| value.checked_add((hour * 3_600 + minute * 60 + second) as i64))
        .ok_or_else(|| "timestamp seconds are out of range".to_owned())?;
    if !(0..=u32::MAX as i64).contains(&total_seconds) {
        return Err("Unix timestamp seconds must fit in u32".into());
    }
    let fraction = parse_fraction(fraction, precision)?;
    Ok((total_seconds as u32, fraction))
}

fn parse_datetime_components(
    input: &str,
    name: &str,
) -> Result<(i32, u32, u32, u32, u32, u32), String> {
    let (date, time) = input
        .split_once('T')
        .or_else(|| input.split_once(' '))
        .ok_or_else(|| format!("expected {name} like 2024-07-03T09:46:40"))?;
    if time.contains('.') {
        return Err(format!("{name} does not support fractional seconds"));
    }
    let (year, month, day) = parse_date(date)?;
    let (hour, minute, second) = parse_time(time)?;
    if !valid_civil_date(year, month, day) {
        return Err(format!("{name} date is invalid"));
    }
    Ok((year, month, day, hour, minute, second))
}

fn parse_date(date: &str) -> Result<(i32, u32, u32), String> {
    if date.len() != 10
        || date.as_bytes().get(4) != Some(&b'-')
        || date.as_bytes().get(7) != Some(&b'-')
    {
        return Err("expected date as YYYY-MM-DD".into());
    }
    let year = parse_decimal_range(&date[0..4], "year", 0, 9999)? as i32;
    let month = parse_decimal_range(&date[5..7], "month", 1, 12)?;
    let day = parse_decimal_range(&date[8..10], "day", 1, 31)?;
    Ok((year, month, day))
}

fn parse_time(time: &str) -> Result<(u32, u32, u32), String> {
    if time.len() != 8
        || time.as_bytes().get(2) != Some(&b':')
        || time.as_bytes().get(5) != Some(&b':')
    {
        return Err("expected time as HH:MM:SS".into());
    }
    let hour = parse_decimal_range(&time[0..2], "hour", 0, 23)?;
    let minute = parse_decimal_range(&time[3..5], "minute", 0, 59)?;
    let second = parse_decimal_range(&time[6..8], "second", 0, 59)?;
    Ok((hour, minute, second))
}

fn parse_decimal_range(input: &str, name: &str, min: u32, max: u32) -> Result<u32, String> {
    if input.is_empty() || !input.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("invalid {name}"));
    }
    let value = input
        .parse::<u32>()
        .map_err(|err| format!("invalid {name}: {err}"))?;
    if !(min..=max).contains(&value) {
        return Err(format!("{name} must be in {min}..={max}"));
    }
    Ok(value)
}

fn parse_fraction(input: &str, precision: Option<FractionPrecision>) -> Result<u32, String> {
    if input.is_empty() {
        return Ok(0);
    }
    if !input.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("timestamp fraction must contain only digits".into());
    }

    let Some(precision) = precision else {
        if input.bytes().all(|byte| byte == b'0') {
            return Ok(0);
        }
        return Err("fractional seconds are not supported for this field".into());
    };

    if input.len() > precision.digits {
        return Err(format!(
            "{} timestamps accept at most {} fractional digits",
            precision.label, precision.digits
        ));
    }
    let mut value = input
        .parse::<u32>()
        .map_err(|err| format!("invalid timestamp fraction: {err}"))?;
    for _ in input.len()..precision.digits {
        value *= 10;
    }
    Ok(value)
}

fn split_unix_seconds(seconds: u32) -> (i32, u32, u32, i64, i64, i64) {
    let days = seconds as i64 / 86_400;
    let second_of_day = seconds as i64 % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = second_of_day / 3_600;
    let minute = (second_of_day % 3_600) / 60;
    let second = second_of_day % 60;
    (year, month, day, hour, minute, second)
}

fn valid_civil_date(year: i32, month: u32, day: u32) -> bool {
    let Some(days) = days_from_civil(year, month, day) else {
        return false;
    };
    civil_from_days(days) == (year, month, day)
}

fn strip_parenthesized_suffix(input: &str) -> &str {
    input
        .split_once(" (")
        .map(|(head, _)| head.trim_end())
        .unwrap_or(input)
}

fn days_from_civil(year: i32, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let year = year as i64 - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let month = month as i64;
    let day = day as i64;
    let month_prime = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * month_prime + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    (year as i32, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_utc_roundtrips_seconds_and_fraction() {
        assert_eq!(format_unix_utc(1000), "1970-01-01T00:16:40Z");
        assert_eq!(
            format_unix_utc_fraction(1000, 123, MICROSECOND, "ts_usec"),
            "1970-01-01T00:16:40.000123Z"
        );
        assert_eq!(parse_unix_utc("1970-01-01T00:16:40Z").unwrap(), 1000);
        assert_eq!(
            parse_unix_utc_fraction("1000.000123", MICROSECOND).unwrap(),
            (1000, 123)
        );
    }

    #[test]
    fn dos_datetime_roundtrips() {
        let (time, date) = parse_dos_datetime("2024-07-03T09:46:40").unwrap();
        assert_eq!(
            format_dos_datetime(time, date),
            "2024-07-03T09:46:40 (DOS local)"
        );
        assert_eq!(
            encode_dos_datetime_le("2024-07-03T09:46:40 (DOS local)").unwrap(),
            [time.to_le_bytes(), date.to_le_bytes()].concat()
        );
    }

    #[test]
    fn dos_datetime_rejects_odd_seconds() {
        assert!(parse_dos_datetime("2024-07-03T09:46:41").is_err());
    }
}
