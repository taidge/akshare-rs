//! Stock info utilities from Eastmoney.

use crate::client::AkShareClient;
use crate::error::Error;
use crate::error::Result;
use std::collections::HashSet;

/// 股票信息查询 (from Eastmoney search)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StockInfo {
    pub code: String,
    pub name: String,
    pub market: String,
    pub industry: Option<String>,
    #[serde(default)]
    pub listed_on: Option<String>,
}

impl AkShareClient {
    /// 东方财富-股票信息查询
    pub async fn stock_info_a_code_name(&self) -> Result<Vec<StockInfo>> {
        let mut items = Vec::new();
        for (exchange, filter) in [
            ("SH", "m:1+t:2,m:1+t:23"),
            ("SZ", "m:0+t:6,m:0+t:80"),
            ("BJ", "m:0+t:81+s:2048"),
        ] {
            let mut seen = HashSet::new();
            let mut expected = None;
            for page in 1..=150 {
                let value = self
                    .stock_json(
                        "https://push2.eastmoney.com/api/qt/clist/get",
                        &[
                            ("pn", page.to_string()),
                            ("pz", "100".into()),
                            ("po", "0".into()),
                            ("np", "1".into()),
                            ("fltt", "2".into()),
                            ("invt", "2".into()),
                            ("fid", "f12".into()),
                            ("fs", filter.into()),
                            ("fields", "f12,f13,f14,f26,f100".into()),
                        ],
                    )
                    .await?;
                if value["rc"].as_i64() != Some(0) {
                    return Err(Error::upstream("directory status error"));
                }
                let total = value["data"]["total"]
                    .as_u64()
                    .ok_or_else(|| Error::decode("missing directory total"))?
                    as usize;
                if total > 15000 || expected.is_some_and(|n| n != total) {
                    return Err(Error::upstream("directory total changed or exceeds limit"));
                }
                expected = Some(total);
                if total == 0 {
                    break;
                }
                let rows = value["data"]["diff"]
                    .as_array()
                    .ok_or_else(|| Error::decode("missing directory page"))?;
                if rows.is_empty() {
                    return Err(Error::decode("incomplete directory pagination"));
                }
                for row in rows {
                    let code = row["f12"]
                        .as_str()
                        .ok_or_else(|| Error::decode("missing stock code"))?;
                    let name = row["f14"]
                        .as_str()
                        .ok_or_else(|| Error::decode("missing stock name"))?;
                    if code.len() != 6
                        || !code.bytes().all(|b| b.is_ascii_digit())
                        || !seen.insert(code.to_owned())
                    {
                        return Err(Error::decode("duplicate or invalid directory code"));
                    }
                    if row["f13"].as_i64() != Some(if exchange == "SH" { 1 } else { 0 }) {
                        return Err(Error::decode("directory exchange mismatch"));
                    }
                    if exchange == "BJ"
                        && (code.starts_with("81")
                            || name.contains("定转")
                            || !(code.starts_with("920")
                                || code.starts_with('4')
                                || code.starts_with('8')))
                    {
                        continue;
                    }
                    let listed_on = row["f26"]
                        .as_i64()
                        .and_then(|d| {
                            chrono::NaiveDate::parse_from_str(&d.to_string(), "%Y%m%d").ok()
                        })
                        .map(|d| d.to_string());
                    items.push(StockInfo {
                        code: code.into(),
                        name: name.into(),
                        market: exchange.into(),
                        industry: row["f100"]
                            .as_str()
                            .filter(|s| *s != "-")
                            .map(str::to_owned),
                        listed_on,
                    });
                }
                if seen.len() > total {
                    return Err(Error::decode("directory count exceeds total"));
                }
                if seen.len() == total {
                    break;
                }
            }
            if Some(seen.len()) != expected {
                return Err(Error::decode("incomplete directory pagination"));
            }
        }
        Ok(items)
    }
}
