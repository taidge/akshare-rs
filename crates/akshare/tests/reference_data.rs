#![cfg(feature = "mod-stock")]
use akshare::AkShareClient;
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path, query_param},
};

#[tokio::test]
async fn financial_tables_paginate_all_rows_and_keep_reporting_components_separate() {
    let server = MockServer::start().await;
    for (page, date) in [(1, "2025-12-31"), (2, "2025-09-30")] {
        Mock::given(method("GET")).and(path("/api/data/v1/get")).and(query_param("reportName","RPT_LICO_FN_CPD")).and(query_param("pageNumber",page.to_string())).respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true,"code":0,"result":{"pages":2,"count":2,"data":[{"SECUCODE":"600036.SH","SECURITY_CODE":"600036","REPORTDATE":format!("{date} 00:00:00"),"TOTAL_OPERATE_INCOME":null}]}}))).expect(1).mount(&server).await;
    }
    for report in ["RPT_DMSK_FN_BALANCE", "RPT_DMSK_FN_CASHFLOW"] {
        Mock::given(method("GET")).and(query_param("reportName",report)).respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true,"code":0,"result":{"pages":1,"count":1,"data":[{"SECUCODE":"600036.SH","SECURITY_CODE":"600036","REPORT_DATE":"2025-12-31 00:00:00","TOTAL_EQUITY":123,"NETCASH_OPERATE":-45}]}}))).expect(1).mount(&server).await;
    }
    let rows = AkShareClient::with_mock(server.uri())
        .stock_financial_reference_em("600036.SH")
        .await
        .unwrap();
    assert_eq!(rows.summary.len(), 2);
    assert_eq!(rows.balance.len(), 1);
    assert!(rows.summary[0]["TOTAL_OPERATE_INCOME"].is_null());
    assert_eq!(rows.cashflow[0]["NETCASH_OPERATE"], -45);
}
#[tokio::test]
async fn financial_identity_duplicate_periods_and_incomplete_pages_are_rejected() {
    for (count, code, date) in [
        (1, "600519.SH", "2025-12-31"),
        (2, "600036.SH", "2025-12-31"),
        (1, "600036.SH", "not-date"),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true,"code":0,"result":{"pages":1,"count":count,"data":[{"SECUCODE":code,"SECURITY_CODE":"600036","REPORTDATE":date}]}}))).mount(&server).await;
        assert!(
            AkShareClient::with_mock(server.uri())
                .stock_financial_reference_em("600036.SH")
                .await
                .is_err()
        );
    }
    let server = MockServer::start().await;
    Mock::given(method("GET")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true,"code":0,"result":{"pages":1,"count":2,"data":[{"SECUCODE":"600036.SH","SECURITY_CODE":"600036","REPORTDATE":"2025-12-31"},{"SECUCODE":"600036.SH","SECURITY_CODE":"600036","REPORTDATE":"2025-12-31"}]}}))).mount(&server).await;
    assert!(
        AkShareClient::with_mock(server.uri())
            .stock_financial_reference_em("600036.SH")
            .await
            .is_err()
    );
}
#[tokio::test]
async fn factor_and_calendar_http_preserve_actual_source_protocols() {
    let server = MockServer::start().await;
    Mock::given(path("/realstock/company/sh600036/hfq.js"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"var sh600036hfq={"total":1,"data":[{"d":"1900-01-01","f":"1.0"}]};"#,
        ))
        .mount(&server)
        .await;
    Mock::given(path("/realstock/company/klc_td_sh.txt"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(include_str!("fixtures/sina-calendar.js")),
        )
        .mount(&server)
        .await;
    Mock::given(path("/realstock/company/bj920130/hfq.js"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"var bj920130hfq={"total":1,"data":[{"d":"1900-01-01","f":"1.0"}]};"#,
        ))
        .mount(&server)
        .await;
    let client = AkShareClient::with_mock(server.uri());
    assert_eq!(
        client
            .stock_adjustment_factors_sina("600036.SH")
            .await
            .unwrap()[0]
            .factor,
        1.
    );
    assert_eq!(
        client
            .stock_adjustment_factors_sina("920130.BJ")
            .await
            .unwrap()[0]
            .factor,
        1.
    );
    assert_eq!(
        client.stock_trade_calendar_sina().await.unwrap().len(),
        8797
    );
    assert!(
        client
            .stock_financial_reference_em("600036.SH\").injection")
            .await
            .is_err()
    );
}
#[tokio::test]
#[ignore = "live public Eastmoney/Sina reference data, paced and read-only"]
async fn live_reference_financial_factors_calendar_contract() {
    let client = AkShareClient::new();
    let tables = client
        .stock_financial_reference_em("600036.SH")
        .await
        .unwrap();
    assert!(tables.summary.len() > 80);
    assert!(!tables.balance.is_empty() && !tables.cashflow.is_empty());
    let factors = client
        .stock_adjustment_factors_sina("600036.SH")
        .await
        .unwrap();
    assert!(factors.len() > 20);
    let beijing = client
        .stock_adjustment_factors_sina("920130.BJ")
        .await
        .unwrap();
    assert!(beijing.len() >= 3 && beijing.iter().any(|p| p.date == "2024-06-12"));
    let dates = client.stock_trade_calendar_sina().await.unwrap();
    assert!(dates.len() > 8500);
    println!(
        "financials summary={} balance={} cashflow={}; factors={} latest={:?}; calendar={} {}..{}",
        tables.summary.len(),
        tables.balance.len(),
        tables.cashflow.len(),
        factors.len(),
        factors.last(),
        dates.len(),
        dates.first().unwrap(),
        dates.last().unwrap()
    );
}
