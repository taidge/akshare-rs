#![cfg(feature = "mod-stock")]
use akshare::AkShareClient;
use serde_json::json;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{path, query_param},
};

#[tokio::test]
async fn bse_directory_parses_jsonp_and_fetches_every_page() {
    let server = MockServer::start().await;
    for page in 0..2 {
        Mock::given(path("/nqxxController/nqxxCnzq.do"))
            .and(wiremock::matchers::body_string_contains(format!("page={page}")))
            .respond_with(ResponseTemplate::new(200).set_body_string(format!("null({});",json!([{"number":page,"totalPages":2,"totalElements":2,"content":[{"xxzqdm":format!("92000{page}"),"xxzqjc":"样例"}]}])))).expect(1).mount(&server).await;
    }
    let items = AkShareClient::with_mock(server.uri())
        .stock_info_bj_name_code()
        .await
        .unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[1]["xxzqdm"], "920001");
}

#[tokio::test]
async fn szse_directory_reads_workbook_instead_of_returning_byte_count() {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, body) in [
        (
            "[Content_Types].xml",
            r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#,
        ),
        (
            "xl/workbook.xml",
            r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="A股列表" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
        ),
        (
            "xl/worksheets/sheet1.xml",
            r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>A股代码</t></is></c><c r="B1" t="inlineStr"><is><t>A股简称</t></is></c><c r="C1" t="inlineStr"><is><t>A股上市日期</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>000001</t></is></c><c r="B2" t="inlineStr"><is><t>平安银行</t></is></c><c r="C2" t="inlineStr"><is><t>1991-04-03</t></is></c></row></sheetData></worksheet>"#,
        ),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    let bytes = zip.finish().unwrap().into_inner();
    let server = MockServer::start().await;
    Mock::given(path("/api/report/ShowReport"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
        .expect(1)
        .mount(&server)
        .await;
    let rows = AkShareClient::with_mock(server.uri())
        .stock_info_sz_name_code("A股列表")
        .await
        .unwrap();
    assert_eq!(rows[0]["A股代码"], "000001");
    assert_eq!(rows[0]["A股简称"], "平安银行");
}

#[tokio::test]
#[ignore = "requires public exchange endpoints"]
async fn live_complete_exchange_directory() {
    let rows = AkShareClient::new()
        .stock_info_a_code_name_exchanges()
        .await
        .unwrap();
    assert!(rows.len() > 5000);
    for exchange in ["SH", "SZ", "BJ"] {
        let items = rows
            .iter()
            .filter(|r| r.market == exchange)
            .collect::<Vec<_>>();
        println!("{exchange}: {}, first={:?}", items.len(), items.first());
        assert!(items.len() > 100);
        assert!(items.iter().all(|r| {
            r.listed_on
                .as_ref()
                .is_some_and(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok())
        }));
    }
}

#[tokio::test]
async fn sourced_history_reports_fallback_and_respects_source_pin() {
    let server = MockServer::start().await;
    Mock::given(path("/appstock/app/fqkline/get"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"code":1})))
        .mount(&server)
        .await;
    Mock::given(path("/api/qt/stock/kline/get"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"rc":0,"data":{"code":"600000","market":1,"klines":[]}})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let client = AkShareClient::with_mock(server.uri());
    assert_eq!(
        client
            .stock_zh_a_hist_sourced("600000.SH", "20261101", "20261110", None)
            .await
            .unwrap()
            .source,
        "eastmoney"
    );
    assert!(
        client
            .stock_zh_a_hist_sourced("600000.SH", "20261101", "20261110", Some("tencent"))
            .await
            .is_err()
    );
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
        .stock_info_a_code_name_em()
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
            .stock_info_a_code_name_em()
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
