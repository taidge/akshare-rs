//! Historical data from Eastmoney (stock_zh_a_hist, stock_hk_hist, stock_us_hist, etc.)

use super::types::HistData;
use crate::client::AkShareClient;
use crate::error::{Error, Result};
use crate::market::eastmoney_secid;
use crate::util::parse_csv_line;
use crate::util::parse_f64_safe;

/// Parse raw kline strings into `HistData` records.
fn parse_hist_klines(klines: &[String]) -> Vec<HistData> {
    klines
        .iter()
        .filter_map(|s| {
            let f = parse_csv_line(s);
            if f.len() < 11 {
                return None;
            }
            Some(HistData {
                trade_date: f[0].to_string(),
                open: parse_f64_safe(f[1]),
                close: parse_f64_safe(f[2]),
                high: parse_f64_safe(f[3]),
                low: parse_f64_safe(f[4]),
                volume: parse_f64_safe(f[5]),
                amount: parse_f64_safe(f[6]),
                amplitude_pct: parse_f64_safe(f[7]),
                change_pct: parse_f64_safe(f[8]),
                change_amount: parse_f64_safe(f[9]),
                turnover_rate: parse_f64_safe(f[10]),
            })
        })
        .collect()
}

/// Resolve a period string to a klt code for daily/weekly/monthly.
fn daily_klt(period: &str) -> Result<&'static str> {
    match period {
        "daily" => Ok("101"),
        "weekly" => Ok("102"),
        "monthly" => Ok("103"),
        _ => Err(Error::invalid_input(format!(
            "unsupported period: {period}"
        ))),
    }
}

/// Resolve a period string to a klt code for intraday intervals.
fn minute_klt(period: &str) -> Result<&'static str> {
    match period {
        "1" => Ok("1"),
        "5" => Ok("5"),
        "15" => Ok("15"),
        "30" => Ok("30"),
        "60" => Ok("60"),
        _ => Err(Error::invalid_input(format!(
            "unsupported period: {period}"
        ))),
    }
}

/// Resolve an adjust string to a fqt code.
fn adjust_fqt(adjust: &str) -> &'static str {
    match adjust {
        "qfq" => "1",
        "hfq" => "2",
        _ => "0",
    }
}

impl AkShareClient {
    /// Shared fetch-and-parse logic for Eastmoney kline history.
    async fn hist_inner(
        &self,
        secid: &str,
        klt: &str,
        fqt: &str,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<HistData>> {
        let beg = if start_date.is_empty() {
            "0"
        } else {
            start_date
        };
        let end = if end_date.is_empty() {
            "20500000"
        } else {
            end_date
        };
        let klines = self
            .kline_fetch(secid, klt, fqt, 1_000_000, &[("beg", beg), ("end", end)])
            .await?;
        Ok(parse_hist_klines(&klines))
    }

    /// 东方财富-沪深京A股-历史行情
    pub async fn stock_zh_a_hist(
        &self,
        symbol: &str,
        period: &str,
        adjust: &str,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<HistData>> {
        let secid = eastmoney_secid(symbol)?;
        self.hist_inner(
            &secid,
            daily_klt(period)?,
            adjust_fqt(adjust),
            start_date,
            end_date,
        )
        .await
    }

    /// Eastmoney-only A-share history. Never silently falls back to another vendor.
    /// Prices are yuan; volume is vendor lots (100 shares). Empty verified ranges are valid.
    /// Accepts YYYYMMDD or YYYY-MM-DD; paginate ranges longer than 731 calendar days.
    pub async fn stock_zh_a_hist_em(
        &self,
        symbol: &str,
        period: &str,
        adjust: &str,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<HistData>> {
        let secid = eastmoney_secid(symbol)?;
        let (start, end) = crate::stock::history_validation::date_range(start_date, end_date)?;
        if !matches!(adjust, "" | "qfq" | "hfq") {
            return Err(Error::invalid_input("invalid adjustment"));
        }
        let value = self
            .stock_json(
                "https://push2his.eastmoney.com/api/qt/stock/kline/get",
                &[
                    ("secid", secid.clone()),
                    ("klt", daily_klt(period)?.into()),
                    ("fqt", adjust_fqt(adjust).into()),
                    ("lmt", "640".into()),
                    ("beg", start.format("%Y%m%d").to_string()),
                    ("end", end.format("%Y%m%d").to_string()),
                    ("fields1", "f1,f2,f3,f4,f5,f6".into()),
                    (
                        "fields2",
                        "f51,f52,f53,f54,f55,f56,f57,f58,f59,f60,f61".into(),
                    ),
                ],
            )
            .await?;
        if value["rc"].as_i64() != Some(0) {
            return Err(Error::upstream("eastmoney history status error"));
        }
        let (market, code) = secid
            .split_once('.')
            .ok_or_else(|| Error::invalid_input("invalid secid"))?;
        let data = &value["data"];
        if data["code"].as_str() != Some(code)
            || data["market"].as_i64() != market.parse::<i64>().ok()
        {
            return Err(Error::decode("eastmoney history identity mismatch"));
        }
        let rows = data["klines"]
            .as_array()
            .ok_or_else(|| Error::decode("missing eastmoney klines"))?;
        let mut items = Vec::new();
        for row in rows {
            let fields = parse_csv_line(
                row.as_str()
                    .ok_or_else(|| Error::decode("invalid eastmoney row"))?,
            );
            if fields.len() < 11 {
                return Err(Error::decode("incomplete eastmoney row"));
            }
            let number = |n: usize| {
                crate::stock::history_validation::number(&serde_json::Value::String(
                    fields[n].into(),
                ))
            };
            let item = HistData {
                trade_date: fields[0].into(),
                open: number(1)?,
                close: number(2)?,
                high: number(3)?,
                low: number(4)?,
                volume: number(5)?,
                amount: parse_f64_safe(fields[6]),
                amplitude_pct: parse_f64_safe(fields[7]),
                change_pct: parse_f64_safe(fields[8]),
                change_amount: parse_f64_safe(fields[9]),
                turnover_rate: parse_f64_safe(fields[10]),
            };
            crate::stock::history_validation::validate_bar(
                &item.trade_date,
                item.open,
                item.close,
                item.high,
                item.low,
                item.volume,
                start,
                end,
            )?;
            items.push(item);
        }
        items.sort_by(|a, b| a.trade_date.cmp(&b.trade_date));
        if items.windows(2).any(|p| p[0].trade_date == p[1].trade_date) {
            return Err(Error::decode("duplicate trade date"));
        }
        Ok(items)
    }

    /// 东方财富-沪深A股-分时行情
    pub async fn stock_zh_a_hist_min(
        &self,
        symbol: &str,
        period: &str,
        adjust: &str,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<HistData>> {
        let secid = eastmoney_secid(symbol)?;
        self.hist_inner(
            &secid,
            minute_klt(period)?,
            adjust_fqt(adjust),
            start_date,
            end_date,
        )
        .await
    }

    /// 东方财富-港股-历史行情
    pub async fn stock_hk_hist(
        &self,
        symbol: &str,
        period: &str,
        adjust: &str,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<HistData>> {
        let secid = format!("116.{}", symbol.trim());
        self.hist_inner(
            &secid,
            daily_klt(period)?,
            adjust_fqt(adjust),
            start_date,
            end_date,
        )
        .await
    }

    /// 东方财富-港股-分时行情
    pub async fn stock_hk_hist_min(
        &self,
        symbol: &str,
        period: &str,
        adjust: &str,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<HistData>> {
        let secid = format!("116.{}", symbol.trim());
        self.hist_inner(
            &secid,
            minute_klt(period)?,
            adjust_fqt(adjust),
            start_date,
            end_date,
        )
        .await
    }

    /// 东方财富-美股-历史行情
    pub async fn stock_us_hist(
        &self,
        symbol: &str,
        period: &str,
        adjust: &str,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<HistData>> {
        // US stocks use 105/106 market codes
        let secid = if symbol.contains('.') {
            let parts: Vec<&str> = symbol.split('.').collect();
            if parts.len() == 2 {
                let code = parts[0];
                let suffix = parts[1].to_uppercase();
                let market = match suffix.as_str() {
                    "N" => "106",
                    _ => "105",
                };
                format!("{market}.{code}")
            } else {
                format!("105.{symbol}")
            }
        } else {
            format!("105.{symbol}")
        };
        self.hist_inner(
            &secid,
            daily_klt(period)?,
            adjust_fqt(adjust),
            start_date,
            end_date,
        )
        .await
    }

    /// 东方财富-美股-分时行情
    pub async fn stock_us_hist_min(
        &self,
        symbol: &str,
        period: &str,
        adjust: &str,
        start_date: &str,
        end_date: &str,
    ) -> Result<Vec<HistData>> {
        let secid = format!("105.{symbol}");
        self.hist_inner(
            &secid,
            minute_klt(period)?,
            adjust_fqt(adjust),
            start_date,
            end_date,
        )
        .await
    }
}
