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
    /// Complete A-share directory from the three exchanges, falling back to a fully
    /// paginated Eastmoney directory only when the exchange catalog is unavailable.
    pub async fn stock_info_a_code_name(&self) -> Result<Vec<StockInfo>> {
        match self.stock_info_a_code_name_exchanges().await {
            Ok(items) => Ok(items),
            Err(primary) => self.stock_info_a_code_name_em().await.map_err(|fallback| {
                Error::upstream(format!(
                    "exchange catalog: {primary}; Eastmoney fallback: {fallback}"
                ))
            }),
        }
    }
    pub async fn stock_info_a_code_name_exchanges(&self) -> Result<Vec<StockInfo>> {
        let listed_date = |raw: String| {
            chrono::NaiveDate::parse_from_str(&raw, "%Y%m%d")
                .or_else(|_| chrono::NaiveDate::parse_from_str(&raw, "%Y-%m-%d"))
                .map(|d| d.to_string())
                .map_err(|e| Error::decode(e.to_string()))
        };
        let mut items = Vec::new();
        for board in ["主板A股", "科创板"] {
            let rows = self.stock_info_sh_name_code(board).await?;
            if rows.is_empty() {
                return Err(Error::decode("SSE directory empty"));
            }
            for row in rows {
                let field = |key: &str| {
                    row[key]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .ok_or_else(|| Error::decode(format!("missing SSE {key}")))
                };
                items.push(StockInfo {
                    code: field("A_STOCK_CODE")?,
                    name: field("SEC_NAME_CN")?,
                    market: "SH".into(),
                    industry: None,
                    listed_on: Some(listed_date(field("LIST_DATE")?)?),
                });
            }
        }
        for row in self.stock_info_sz_name_code("A股列表").await? {
            let field = |key: &str| {
                row[key]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .ok_or_else(|| Error::decode(format!("missing SZSE {key}")))
            };
            let raw = field("A股代码")?;
            let code = format!("{:0>6}", raw.split('.').next().unwrap_or(&raw));
            items.push(StockInfo {
                code,
                name: field("A股简称")?,
                market: "SZ".into(),
                industry: Some(field("所属行业")?),
                listed_on: Some(listed_date(field("A股上市日期")?)?),
            });
        }
        for row in self.stock_info_bj_name_code().await? {
            let field = |key: &str| {
                row[key]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .ok_or_else(|| Error::decode(format!("missing BSE {key}")))
            };
            let raw = field("xxgprq")?;
            let date = chrono::NaiveDate::parse_from_str(&raw, "%Y%m%d")
                .map_err(|e| Error::decode(e.to_string()))?;
            items.push(StockInfo {
                code: field("xxzqdm")?,
                name: field("xxzqjc")?,
                market: "BJ".into(),
                industry: Some(field("xxhyzl")?),
                listed_on: Some(date.to_string()),
            });
        }
        let mut seen = HashSet::new();
        for item in &items {
            if item.code.len() != 6
                || !item.code.bytes().all(|b| b.is_ascii_digit())
                || !seen.insert((&item.code, &item.market))
            {
                return Err(Error::decode(
                    "invalid or duplicate exchange directory symbol",
                ));
            }
        }
        if items.len() < 4000 {
            return Err(Error::decode("incomplete exchange directory"));
        }
        Ok(items)
    }

    /// 东方财富-股票信息查询
    pub async fn stock_info_a_code_name_em(&self) -> Result<Vec<StockInfo>> {
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
