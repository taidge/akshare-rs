//! Pure Rust port of Sina's type-139 calendar bitstream (AKShare hk_js_decode R).
//! No JavaScript evaluation and no weekday-only calendar approximation.
use crate::{Error, Result};
use chrono::{Duration, NaiveDate};

struct Bits {
    values: Vec<u8>,
    position: usize,
}
impl Bits {
    fn read(&mut self, width: usize) -> Result<i64> {
        if width > 30 || self.position + width > self.values.len() * 6 {
            return Err(Error::decode("truncated Sina calendar bitstream"));
        }
        let mut value = 0;
        for offset in 0..width {
            value |=
                i64::from((self.values[self.position / 6] >> (self.position % 6)) & 1) << offset;
            self.position += 1;
        }
        Ok(value)
    }
    fn signed_length_change(&mut self) -> Result<i64> {
        let positive = self.read(1)?;
        let mut magnitude = 1;
        while self.read(1)? != 0 {
            magnitude += 1;
            if magnitude > 10 {
                return Err(Error::decode("invalid Sina calendar run length"));
            }
        }
        Ok(magnitude * (2 * positive - 1))
    }
}
pub(crate) fn parse_calendar(body: &str) -> Result<Vec<String>> {
    if body.len() > 100_000 {
        return Err(Error::decode("Sina calendar too large"));
    }
    let (name, value) = body
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .split_once('=')
        .ok_or_else(|| Error::decode("invalid Sina calendar assignment"))?;
    if !matches!(name.trim(), "var datelist" | "var klc_td_sh") {
        return Err(Error::decode("unexpected Sina calendar identity"));
    }
    let encoded: String = serde_json::from_str(value.trim())
        .map_err(|_| Error::decode("invalid Sina calendar string"))?;
    if encoded.contains('-') {
        // Accept the older explicit-date protocol, validating every date (also useful to mock).
        let mut dates = encoded
            .split(',')
            .map(|s| {
                NaiveDate::parse_from_str(s, "%Y-%m-%d")
                    .map(|d| d.to_string())
                    .map_err(|_| Error::decode("invalid Sina calendar date"))
            })
            .collect::<Result<Vec<_>>>()?;
        dates.sort();
        if dates.windows(2).any(|w| w[0] == w[1]) {
            return Err(Error::decode("duplicate Sina calendar date"));
        }
        return Ok(dates);
    }
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let values = encoded
        .bytes()
        .map(|c| {
            alphabet
                .iter()
                .position(|v| *v == c)
                .map(|v| v as u8)
                .ok_or_else(|| Error::decode("invalid Sina calendar alphabet"))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut bits = Bits {
        values,
        position: 0,
    };
    if bits.read(12)? != 139 || (63 ^ bits.read(6)?) > 1 {
        return Err(Error::decode("unsupported Sina calendar codec"));
    }
    let mut day = bits.read(18)? - 1;
    let end = bits.read(18)?;
    if end < day || end - day > 100_000 {
        return Err(Error::decode("invalid Sina calendar bounds"));
    }
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).expect("epoch");
    let mut remaining = -1;
    let mut length = 0_i64;
    let mut dates = vec![];
    while day < end {
        day += 1;
        let weekday = day.rem_euclid(7);
        if weekday == 3 || weekday == 4 {
            day += 5 - weekday;
        }
        let date = epoch
            .checked_add_signed(Duration::days(7657 + day))
            .ok_or_else(|| Error::decode("Sina calendar date overflow"))?;
        if remaining <= 0 {
            if bits.read(1)? != 0 {
                length += bits.signed_length_change()?;
            }
            if !(0..=10).contains(&length) {
                return Err(Error::decode("invalid Sina calendar code width"));
            }
            remaining = bits.read((3 * length) as usize)? + 1;
            if dates.is_empty() {
                dates.push(date.to_string());
                remaining -= 1;
            }
        } else {
            dates.push(date.to_string());
        }
        remaining -= 1;
    }
    // Documented source omission, also corrected by upstream AKShare.
    let missing = "1992-05-04".to_owned();
    if dates.first().is_some_and(|d| d <= &missing)
        && dates.last().is_some_and(|d| d >= &missing)
        && !dates.contains(&missing)
    {
        dates.push(missing);
        dates.sort();
    }
    if dates.len() < 2 {
        return Err(Error::decode("empty Sina calendar"));
    }
    Ok(dates)
}
