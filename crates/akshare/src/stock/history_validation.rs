use crate::{Error, Result};
use chrono::NaiveDate;
pub(crate) fn date_range(start: &str, end: &str) -> Result<(NaiveDate, NaiveDate)> {
    let parse = |s: &str| {
        NaiveDate::parse_from_str(s, "%Y%m%d")
            .or_else(|_| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
            .map_err(|e| Error::invalid_input(e.to_string()))
    };
    let start = parse(start)?;
    let end = parse(end)?;
    if start > end || (end - start).num_days() > 730 {
        return Err(Error::invalid_input(
            "history range must be at most 731 calendar days; paginate longer histories",
        ));
    }
    Ok((start, end))
}
pub(crate) fn number(v: &serde_json::Value) -> Result<f64> {
    let n = v
        .as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .ok_or_else(|| Error::decode("missing or invalid history number"))?;
    if !n.is_finite() {
        return Err(Error::decode("non-finite history number"));
    }
    Ok(n)
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn validate_bar(
    date: &str,
    open: f64,
    close: f64,
    high: f64,
    low: f64,
    volume: f64,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<()> {
    let date =
        NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|e| Error::decode(e.to_string()))?;
    if !(start..=end).contains(&date)
        || low <= 0.
        || high < open.max(close)
        || low > open.min(close)
        || volume < 0.
        || high >= 100_000_000.
    {
        return Err(Error::decode("invalid OHLC, volume or requested date"));
    }
    Ok(())
}
