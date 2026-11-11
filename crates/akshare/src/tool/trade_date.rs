//! Trade calendar data from Sina Finance.

use crate::client::AkShareClient;
use crate::error::Result;
use crate::types::MacroDataPoint;

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

impl AkShareClient {
    /// Historical trade calendar from Sina Finance.
    ///
    /// Downloads the A-share trading calendar from Sina Finance.
    /// Returns a list of trading dates.
    pub async fn tool_trade_date_hist(&self) -> Result<Vec<MacroDataPoint>> {
        let url = "https://finance.sina.com.cn/realstock/company/klc_td_sh.txt";
        let body = self.get(url).send().await?.text().await?;

        Ok(crate::sina_calendar::parse_calendar(&body)?
            .into_iter()
            .map(|date| MacroDataPoint {
                date,
                value: 1.0,
                name: "Trade Date".into(),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_trade_date_parsing() {
        // Simulate Sina response format
        let body = r#"var klc_td_sh="2024-01-02,2024-01-03,2024-01-04";"#;
        let dates_str = body
            .split_once('=')
            .and_then(|(_, r)| r.split_once(';'))
            .map_or("", |(s, _)| s.trim_matches('"'));
        let dates: Vec<&str> = dates_str.split(',').collect();
        assert_eq!(dates.len(), 3);
        assert_eq!(dates[0], "2024-01-02");
    }
}
