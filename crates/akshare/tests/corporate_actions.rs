#![cfg(feature = "mod-stock")]
use akshare::AkShareClient;
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, query_param},
};
#[tokio::test]
async fn implemented_events_require_complete_pages_and_matching_security() {
    for (security, count, success) in [
        ("600036.SH", 1, true),
        ("600519.SH", 1, false),
        ("600036.SH", 2, false),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(query_param("reportName","RPT_SHAREBONUS_DET")).and(query_param("filter","(SECUCODE=\"600036.SH\")(ASSIGN_PROGRESS=\"实施分配\")")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true,"code":0,"result":{"pages":1,"count":count,"data":[{"SECUCODE":security,"SECURITY_CODE":&security[..6],"EX_DIVIDEND_DATE":"2026-07-10","PRETAX_BONUS_RMB":10.03}]}}))).expect(1).mount(&server).await;
        let result = AkShareClient::with_mock(server.uri())
            .stock_implemented_distributions_em("600036.SH")
            .await;
        assert_eq!(result.is_ok(), success);
    }
}
#[tokio::test]
#[ignore = "live public read-only implemented distribution contract"]
async fn live_implemented_distribution_contract() {
    let rows = AkShareClient::new()
        .stock_implemented_distributions_em("600036.SH")
        .await
        .unwrap();
    assert!(rows.len() > 20);
    let latest = rows
        .iter()
        .find(|r| {
            r["EX_DIVIDEND_DATE"]
                .as_str()
                .is_some_and(|d| d.starts_with("2026-07-10"))
        })
        .unwrap();
    assert_eq!(latest["PRETAX_BONUS_RMB"], 10.03);
    assert!(latest.get("PAY_DATE").is_none());
    println!("implemented={} latest={latest}", rows.len());
}
