#![cfg(feature = "mod-stock")]
use akshare::AkShareClient;
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{path, query_param},
};
#[tokio::test]
async fn individual_notice_matches_requested_code_in_multisecurity_articles() {
    let server = MockServer::start().await;
    Mock::given(path("/api/security/ann")).and(query_param("stock_list","600036")).and(query_param("begin_time","2026-11-01"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":1,"data":{"total_hits":1,"list":[{"art_code":"AN202611110001","title":"董事会公告","notice_date":"2026-11-11","codes":[{"stock_code":"03968"},{"stock_code":"600036"}],"columns":[{"column_name":"重大事项"}]}]}}))).expect(1).mount(&server).await;
    let items = AkShareClient::with_mock(server.uri())
        .stock_individual_notice_report("600036", "全部", Some("20261101"), Some("20261111"))
        .await
        .unwrap();
    assert_eq!(items[0]["stock_code"], "600036");
    assert_eq!(items[0]["art_code"], "AN202611110001");
}
#[tokio::test]
async fn wrong_security_missing_pages_and_error_status_do_not_become_empty_success() {
    for payload in [
        json!({"success":0,"data":{"total_hits":0,"list":[]}}),
        json!({"data":{"total_hits":1,"list":[]}}),
        json!({"data":{"total_hits":1,"list":[{"art_code":"AN1","codes":[{"stock_code":"600519"}]}]}}),
        json!({"data":{"total_hits":2,"list":[{"art_code":"AN1","codes":[{"stock_code":"600036"}]}]}}),
        json!({"data":{"total_hits":10001,"list":[]}}),
    ] {
        let server = MockServer::start().await;
        Mock::given(path("/api/security/ann"))
            .respond_with(ResponseTemplate::new(200).set_body_json(payload))
            .mount(&server)
            .await;
        assert!(
            AkShareClient::with_mock(server.uri())
                .stock_individual_notice_report(
                    "600036",
                    "全部",
                    Some("20261101"),
                    Some("20261111")
                )
                .await
                .is_err()
        );
    }
}
#[tokio::test]
async fn invalid_notice_input_is_rejected_without_slicing_or_network_requests() {
    let client = AkShareClient::with_mock("http://127.0.0.1:1".into());
    assert!(
        client
            .stock_individual_notice_report("600036", "全部", Some("中中中中"), None)
            .await
            .is_err()
    );
    assert!(
        client
            .stock_individual_notice_report("600036", "全部", Some("20261112"), Some("20261101"))
            .await
            .is_err()
    );
    assert!(
        client
            .stock_individual_notice_report("600036&other=1", "全部", None, None)
            .await
            .is_err()
    );
}
#[tokio::test]
#[ignore = "requires free public Eastmoney announcement endpoint"]
async fn live_recent_company_notices() {
    let end = chrono::Utc::now().date_naive();
    let begin = end - chrono::Days::new(7);
    let rows = AkShareClient::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .stock_individual_notice_report(
            "600036",
            "全部",
            Some(&begin.format("%Y%m%d").to_string()),
            Some(&end.format("%Y%m%d").to_string()),
        )
        .await
        .unwrap();
    println!(
        "company notice metadata records: {}, first={:?}",
        rows.len(),
        rows.first()
            .map(|r| (&r["title"], &r["notice_date"], &r["stock_code"]))
    );
    assert!(rows.iter().all(|r| r["stock_code"] == "600036"));
}
