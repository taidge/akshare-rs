//! Reference datasets used independently of OHLC prices: reported financials, real factors, calendar.
use crate::{AkShareClient, Error, Result};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FinancialReferenceTables {
    pub summary: Vec<Value>,
    pub balance: Vec<Value>,
    pub cashflow: Vec<Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SinaAdjustmentFactor {
    pub date: String,
    pub factor: f64,
}
fn secucode(symbol: &str) -> Result<(&str, &str)> {
    let (code, market) = symbol
        .split_once('.')
        .ok_or_else(|| Error::invalid_input("expected 600036.SH"))?;
    if code.len() != 6
        || !code.bytes().all(|c| c.is_ascii_digit())
        || !matches!(market, "SH" | "SZ" | "BJ")
    {
        return Err(Error::invalid_input(
            "expected six-digit SH/SZ/BJ stock code",
        ));
    }
    Ok((code, market))
}
async fn bounded_text(mut response: reqwest::Response) -> Result<String> {
    let response_status = response.status();
    if !response_status.is_success() {
        return Err(Error::upstream(format!("reference HTTP {response_status}")));
    }
    const MAX: usize = 8 * 1024 * 1024;
    if response.content_length().is_some_and(|n| n > MAX as u64) {
        return Err(Error::decode("reference response too large"));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > MAX {
            return Err(Error::decode("reference response too large"));
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).map_err(|_| Error::decode("reference response not UTF-8"))
}
fn report_date(row: &Value, key: &str) -> Result<NaiveDate> {
    let raw = row[key]
        .as_str()
        .and_then(|s| s.get(..10))
        .ok_or_else(|| Error::decode("financial report date missing"))?;
    NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .map_err(|_| Error::decode("financial report date invalid"))
}
impl AkShareClient {
    async fn reference_financial_table(
        &self,
        symbol: &str,
        report: &str,
        date_key: &str,
    ) -> Result<Vec<Value>> {
        let filter = if report == "RPT_SHAREBONUS_DET" {
            format!("(SECUCODE=\"{symbol}\")(ASSIGN_PROGRESS=\"实施分配\")")
        } else {
            format!("(SECUCODE=\"{symbol}\")")
        };
        let mut result = Vec::new();
        let mut seen = BTreeSet::new();
        let mut expected = None;
        let mut pages = None;
        for page in 1..=20 {
            let body = bounded_text(
                self.get("https://datacenter-web.eastmoney.com/api/data/v1/get")
                    .query(&[
                        ("reportName", report),
                        ("columns", "ALL"),
                        ("filter", &filter),
                        ("pageNumber", &page.to_string()),
                        ("pageSize", "100"),
                        ("sortTypes", "-1"),
                        ("sortColumns", date_key),
                        ("source", "WEB"),
                        ("client", "WEB"),
                    ])
                    .send()
                    .await?,
            )
            .await?;
            let payload: Value = serde_json::from_str(&body)?;
            if payload["success"] != true || payload["code"] != 0 {
                return Err(Error::upstream(
                    "financial provider returned unsuccessful result",
                ));
            }
            let count = payload["result"]["count"]
                .as_u64()
                .ok_or_else(|| Error::decode("financial count missing"))?;
            let total_pages = payload["result"]["pages"]
                .as_u64()
                .ok_or_else(|| Error::decode("financial page count missing"))?;
            if count > 2000
                || total_pages > 20
                || expected.is_some_and(|n| n != count)
                || pages.is_some_and(|n| n != total_pages)
            {
                return Err(Error::decode(
                    "financial pagination changed or exceeds bounds",
                ));
            }
            expected = Some(count);
            pages = Some(total_pages);
            let rows = payload["result"]["data"]
                .as_array()
                .ok_or_else(|| Error::decode("financial data missing"))?;
            if count == 0 && rows.is_empty() {
                return Ok(vec![]);
            }
            if rows.is_empty() {
                return Err(Error::decode("financial pagination incomplete"));
            }
            for row in rows {
                if row["SECUCODE"].as_str() != Some(symbol)
                    || row["SECURITY_CODE"].as_str() != Some(secucode(symbol)?.0)
                {
                    return Err(Error::decode("financial security identity mismatch"));
                }
                if !seen.insert(report_date(row, date_key)?) {
                    return Err(Error::decode("duplicate financial report period"));
                }
                result.push(row.clone());
            }
            if page as u64 >= total_pages {
                if result.len() as u64 != count {
                    return Err(Error::decode("financial pagination count mismatch"));
                }
                return Ok(result);
            }
            tokio::time::sleep(std::time::Duration::from_millis(180)).await;
        }
        Err(Error::decode("financial pagination exceeded bounds"))
    }
    /// Current reported values, not an archive of original point-in-time vintages.
    /// Complete implemented distributions; actual cash/share settlement dates may be absent.
    pub async fn stock_implemented_distributions_em(&self, symbol: &str) -> Result<Vec<Value>> {
        secucode(symbol)?;
        self.reference_financial_table(symbol, "RPT_SHAREBONUS_DET", "EX_DIVIDEND_DATE")
            .await
    }
    /// Current reported values, not an archive of original point-in-time vintages.
    /// All three reports are paginated and bound to the requested security.
    pub async fn stock_financial_reference_em(
        &self,
        symbol: &str,
    ) -> Result<FinancialReferenceTables> {
        secucode(symbol)?;
        let summary = self
            .reference_financial_table(symbol, "RPT_LICO_FN_CPD", "REPORTDATE")
            .await?;
        if summary.is_empty() {
            return Err(Error::not_found("financial summary unavailable"));
        }
        let balance = self
            .reference_financial_table(symbol, "RPT_DMSK_FN_BALANCE", "REPORT_DATE")
            .await?;
        let cashflow = self
            .reference_financial_table(symbol, "RPT_DMSK_FN_CASHFLOW", "REPORT_DATE")
            .await?;
        Ok(FinancialReferenceTables {
            summary,
            balance,
            cashflow,
        })
    }
    /// Actual sparse Sina hfq factors; never inferred from rounded adjusted prices.
    pub async fn stock_adjustment_factors_sina(
        &self,
        symbol: &str,
    ) -> Result<Vec<SinaAdjustmentFactor>> {
        let (code, market) = secucode(symbol)?;
        let security = format!("{}{code}", market.to_ascii_lowercase());
        let body = bounded_text(
            self.get(&format!(
                "https://finance.sina.com.cn/realstock/company/{security}/hfq.js"
            ))
            .send()
            .await?,
        )
        .await?;
        parse_factors(&body, &security)
    }
    /// Published Sina CN A-share session calendar, including only dates encoded by the provider.
    pub async fn stock_trade_calendar_sina(&self) -> Result<Vec<String>> {
        let body = bounded_text(
            self.get("https://finance.sina.com.cn/realstock/company/klc_td_sh.txt")
                .send()
                .await?,
        )
        .await?;
        crate::sina_calendar::parse_calendar(&body)
    }
}
fn parse_factors(body: &str, security: &str) -> Result<Vec<SinaAdjustmentFactor>> {
    let (name, json) = body
        .trim()
        .trim_end_matches(';')
        .split_once('=')
        .ok_or_else(|| Error::decode("invalid Sina factor assignment"))?;
    if name.trim() != format!("var {security}hfq") {
        return Err(Error::decode("Sina factor security mismatch"));
    }
    let mut stream = serde_json::Deserializer::from_str(json).into_iter::<Value>();
    let value = stream
        .next()
        .ok_or_else(|| Error::decode("Sina factors missing JSON"))??;
    let trailing = json[stream.byte_offset()..]
        .trim()
        .trim_start_matches(';')
        .trim();
    if !trailing.is_empty()
        && !(trailing.starts_with("/*")
            && trailing.ends_with("*/")
            && !trailing[2..trailing.len() - 2].contains("*/"))
    {
        return Err(Error::decode("unexpected content after Sina factors"));
    }
    let rows = value["data"]
        .as_array()
        .ok_or_else(|| Error::decode("Sina factor data missing"))?;
    if value["total"].as_u64() != Some(rows.len() as u64) || rows.is_empty() || rows.len() > 5000 {
        return Err(Error::decode("invalid Sina factor count"));
    }
    let mut result = vec![];
    let mut dates = BTreeSet::new();
    for row in rows {
        let raw = row["d"]
            .as_str()
            .ok_or_else(|| Error::decode("factor date missing"))?;
        let date = NaiveDate::parse_from_str(raw, "%Y-%m-%d")
            .map_err(|_| Error::decode("factor date invalid"))?;
        let factor = row["f"]
            .as_f64()
            .or_else(|| row["f"].as_str().and_then(|s| s.parse().ok()))
            .ok_or_else(|| Error::decode("factor missing"))?;
        if !factor.is_finite() || factor <= 0. || factor > 1e12 || !dates.insert(date) {
            return Err(Error::decode("invalid or duplicate Sina factor"));
        }
        result.push(SinaAdjustmentFactor {
            date: date.to_string(),
            factor,
        });
    }
    result.sort_by(|a, b| a.date.cmp(&b.date));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn factor_protocol_rejects_mismatch_duplicates_bad_values_and_truncation() {
        let body = r#"var sh600036hfq={"total":2,"data":[{"d":"2024-07-11","f":"6.1312692655334000"},{"d":"1900-01-01","f":"1"}]};"#;
        let points = parse_factors(body, "sh600036").unwrap();
        assert!(
            parse_factors(
                &format!("{body}\n/* vendor integrity marker */"),
                "sh600036"
            )
            .is_ok()
        );
        assert!(parse_factors(&format!("{body}evil()"), "sh600036").is_err());
        assert_eq!(points[0].date, "1900-01-01");
        assert!((points[1].factor - 6.1312692655334).abs() < 1e-12);
        assert!(parse_factors(body, "sz600036").is_err());
        for bad in [
            body.replace("\"1\"", "\"NaN\""),
            body.replace("1900-01-01", "2024-07-11"),
            body.replace("\"total\":2", "\"total\":3"),
            body.replace("\"1\"", "\"0\""),
        ] {
            assert!(parse_factors(&bad, "sh600036").is_err());
        }
    }
    #[test]
    fn real_calendar_fixture_matches_upstream_decoder_and_preserves_holidays() {
        use sha2::{Digest, Sha256};
        let dates = crate::sina_calendar::parse_calendar(include_str!(
            "../../tests/fixtures/sina-calendar.js"
        ))
        .unwrap();
        assert_eq!(dates.len(), 8797);
        assert_eq!(dates.first().unwrap(), "1990-12-19");
        assert_eq!(dates.last().unwrap(), "2026-12-31");
        assert!(dates.contains(&"1992-05-04".into()));
        assert!(!dates.contains(&"2026-01-01".into()));
        assert!(!dates.contains(&"2026-05-01".into()));
        assert!(dates.windows(2).all(|w| w[0] < w[1]));
        // Full sequence fingerprint from the upstream AKShare JS decoder, evaluated only during development.
        assert_eq!(
            format!("{:x}", Sha256::digest(serde_json::to_vec(&dates).unwrap())),
            "f6c5474c46aeae51259a415a708dc71101aee47cdccee0809af451411e804b32"
        );
        assert!(crate::sina_calendar::parse_calendar("var datelist=\"LC/AA\";").is_err());
        assert!(crate::sina_calendar::parse_calendar("var other=\"LC/AA\";").is_err());
    }
}
