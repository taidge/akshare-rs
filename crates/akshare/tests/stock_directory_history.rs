#![cfg(feature = "mod-stock")]
use akshare::AkShareClient;
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{path, query_param},
};

#[tokio::test]
async fn sourced_history_reports_fallback_and_respects_source_pin() {
    let server=MockServer::start().await;
    Mock::given(path("/appstock/app/fqkline/get")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"code":1}))).mount(&server).await;
    Mock::given(path("/api/qt/stock/kline/get")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"rc":0,"data":{"code":"600000","market":1,"klines":[]}}))).expect(1).mount(&server).await;
    let client=AkShareClient::with_mock(server.uri());
    assert_eq!(client.stock_zh_a_hist_sourced("600000.SH","20261101","20261110",None).await.unwrap().source,"eastmoney");
    assert!(client.stock_zh_a_hist_sourced("600000.SH","20261101","20261110",Some("tencent")).await.is_err());
}

#[tokio::test]
async fn directory_paginates_and_preserves_exchange_listing_and_industry() {
    let server = MockServer::start().await;
    for (filter, page, total, rows) in [
        (
            "m:1+t:2,m:1+t:23",
            "1",
            3,
            json!([{"f12":"600000","f13":1,"f14":"浦发银行","f26":19991110,"f100":"银行"},{"f12":"600001","f13":1,"f14":"样例一"}]),
        ),
        (
            "m:1+t:2,m:1+t:23",
            "2",
            3,
            json!([{"f12":"600002","f13":1,"f14":"样例二"}]),
        ),
        (
            "m:0+t:6,m:0+t:80",
            "1",
            1,
            json!([{"f12":"000001","f13":0,"f14":"平安银行"}]),
        ),
        (
            "m:0+t:81+s:2048",
            "1",
            2,
            json!([{"f12":"810011","f13":0,"f14":"优机定转"},{"f12":"920130","f13":0,"f14":"立方控股"}]),
        ),
    ] {
        Mock::given(path("/api/qt/clist/get"))
            .and(query_param("fs", filter))
            .and(query_param("pn", page))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"rc":0,"data":{"total":total,"diff":rows}})),
            )
            .expect(1)
            .mount(&server)
            .await;
    }
    let items = AkShareClient::with_mock(server.uri())
        .stock_info_a_code_name()
        .await
        .unwrap();
    assert_eq!(items.len(), 5);
    assert_eq!(items[0].listed_on.as_deref(), Some("1999-11-10"));
    assert_eq!(items[0].industry.as_deref(), Some("银行"));
    assert_eq!(items.last().unwrap().market, "BJ");
}

#[tokio::test]
async fn directory_rejects_repeated_pages_instead_of_returning_partial_market() {
    let server = MockServer::start().await;
    Mock::given(path("/api/qt/clist/get"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"rc":0,"data":{"total":2,"diff":[{"f12":"600000","f13":1,"f14":"浦发银行"}]}}),
        ))
        .mount(&server)
        .await;
    assert!(
        AkShareClient::with_mock(server.uri())
            .stock_info_a_code_name()
            .await
            .is_err()
    );
}

#[tokio::test]
async fn tencent_accepts_numeric_strings_and_normalizes_documented_dates() {
    let server = MockServer::start().await;
    Mock::given(path("/appstock/app/fqkline/get"))
        .and(query_param("param","sh600000,day,2026-11-01,2026-11-10,640,"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"code":0,"data":{"sh600000":{"day":[["2026-11-10","10.10","10.20","10.30","10.00","123.01",{"corporate_action":true}]]}}}))).mount(&server).await;
    let bars = AkShareClient::with_mock(server.uri())
        .stock_zh_a_hist_tx("600000.SH", "20261101", "20261110", "")
        .await
        .unwrap();
    assert_eq!(bars[0].close, 10.2);
    assert_eq!(bars[0].volume, 123.01);
}

#[tokio::test]
async fn history_rejects_adjustment_mismatch_and_invalid_prices() {
    for row in [
        json!({"qfqday":[["2026-11-10","10","11","12","9","123"]]}),
        json!({"day":[["2026-11-10","bad","11","12","9","123"]]}),
        json!({"day":[["2026-11-10","10","11","8","9","123"]]}),
    ] {
        let server = MockServer::start().await;
        Mock::given(path("/appstock/app/fqkline/get"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"code":0,"data":{"sh600000":row}})),
            )
            .mount(&server)
            .await;
        assert!(
            AkShareClient::with_mock(server.uri())
                .stock_zh_a_hist_tx("600000.SH", "20261101", "20261110", "")
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn eastmoney_only_supports_beijing_and_does_not_silently_change_source() {
    let server = MockServer::start().await;
    Mock::given(path("/api/qt/stock/kline/get")).and(query_param("secid","0.920130")).and(query_param("fqt","0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"rc":0,"data":{"code":"920130","market":0,"klines":["2026-11-10,10.10,10.20,10.30,10.00,123.01,0,0,0,0,0"]}}))).expect(1).mount(&server).await;
    let client = AkShareClient::with_mock(server.uri());
    assert_eq!(
        client
            .stock_zh_a_hist_em("920130", "daily", "", "20261101", "20261110")
            .await
            .unwrap()[0]
            .volume,
        123.01
    );
    assert!(
        client
            .stock_zh_a_hist_tx("920130.BJ", "20261101", "20261110", "")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn explicit_empty_range_is_valid_but_wrong_identity_is_rejected() {
    for (code, success) in [("600000", true), ("600001", false)] {
        let server = MockServer::start().await;
        Mock::given(path("/api/qt/stock/kline/get"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"rc":0,"data":{"code":code,"market":1,"klines":[]}})),
            )
            .mount(&server)
            .await;
        let result = AkShareClient::with_mock(server.uri())
            .stock_zh_a_hist_em("600000.SH", "daily", "", "20261101", "20261110")
            .await;
        assert_eq!(result.is_ok(), success);
    }
}

#[tokio::test]
async fn transient_failure_is_retried_without_returning_empty_history() {
    let server = MockServer::start().await;
    let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let calls = count.clone();
    Mock::given(path("/appstock/app/fqkline/get"))
        .respond_with(move |_: &wiremock::Request| {
            if calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                ResponseTemplate::new(503)
            } else {
                ResponseTemplate::new(200)
                    .set_body_json(json!({"code":0,"data":{"sh600000":{"day":[]}}}))
            }
        })
        .mount(&server)
        .await;
    assert!(
        AkShareClient::with_mock(server.uri())
            .stock_zh_a_hist_tx("600000.SH", "20261101", "20261110", "")
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 2);
}
