//! End-to-end: spawn the real binary, speak MCP over stdio with the rmcp client.
//! Runs in fixture mode (YC_MCP_FIXTURE_DIR) so results are synthetic and labelled `data_mode: "fixture"`.

use rmcp::{ServiceExt, model::CallToolRequestParams, transport::TokioChildProcess};
use serde_json::{Value, json};

async fn call(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &'static str,
    args: Value,
) -> (bool, Value) {
    let r = client
        .call_tool(
            CallToolRequestParams::new(name).with_arguments(args.as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    (r.is_error.unwrap_or(false), r.structured_content.unwrap())
}

#[tokio::test]
async fn initialize_list_and_call_over_stdio() {
    let cache = tempfile::tempdir().unwrap();
    let mut cmd = tokio::process::Command::new(env!("CARGO_BIN_EXE_yc-cashflow-mcp"));
    cmd.env(
        "YC_MCP_FIXTURE_DIR",
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"),
    )
    .env("YC_MCP_CACHE_DIR", cache.path())
    .env("YC_MCP_MAX_RESULTS", "5");
    let client = ().serve(TokioChildProcess::new(cmd).unwrap()).await.unwrap();

    let info = client.peer_info().unwrap();
    assert_eq!(info.server_info.as_ref().unwrap().name, "yc-cashflow-mcp");

    let mut names: Vec<String> = client
        .list_all_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "get_yc_company",
            "rank_yc_prospects",
            "search_yc_companies",
            "source_status"
        ]
    );

    let (err, v) = call(&client, "source_status", json!({})).await;
    assert!(!err);
    assert_eq!(v["dataset"]["loaded"], false);

    let (err, v) = call(&client, "rank_yc_prospects", json!({"limit": 3})).await;
    assert!(!err);
    assert_eq!(v["data_mode"], "fixture");
    assert_eq!(v["returned"], 3);
    assert_eq!(v["results"][0]["slug"], "fixture-docsmith");
    assert_eq!(v["results"][1]["slug"], "fixture-pyagents");
    assert_eq!(
        v["results"][0]["unknowns"]["serbia_hiring_eligibility"],
        "unknown"
    );
    assert!(
        v["results"][0]["derived"]["explanation"]
            .as_array()
            .unwrap()
            .len()
            > 1
    );
    assert!(v["attribution"].as_str().unwrap().contains("yc-oss"));

    let (err, v) = call(
        &client,
        "search_yc_companies",
        json!({"query": "documentation", "hiring_only": true}),
    )
    .await;
    assert!(!err);
    assert_eq!(v["total_matches"], 1);
    assert_eq!(
        v["results"][0]["links"]["yc_page"],
        "https://www.ycombinator.com/companies/fixture-docsmith"
    );

    let (err, v) = call(&client, "get_yc_company", json!({"slug": "fixture-nulls"})).await;
    assert!(!err);
    assert_eq!(v["found"], true);
    assert!(v["company"]["links"]["website"].is_null());

    let (err, v) = call(&client, "get_yc_company", json!({"slug": "does-not-exist"})).await;
    assert!(!err);
    assert_eq!(v["result"], "not_found");

    let (err, v) = call(&client, "search_yc_companies", json!({"limit": 500})).await;
    assert!(err);
    assert_eq!(v["error"]["category"], "invalid_arguments");

    let (err, v) = call(&client, "get_yc_company", json!({"slug": "../etc/passwd"})).await;
    assert!(err);
    assert_eq!(v["error"]["category"], "invalid_arguments");

    let (_, v) = call(&client, "source_status", json!({})).await;
    assert_eq!(v["dataset"]["records"], 8);
    assert_eq!(v["dataset"]["data_mode"], "fixture");
    assert_eq!(
        v["dataset"]["duplicate_slugs_dropped"],
        json!(["fixture-pyagents"])
    );

    client.cancel().await.unwrap();
}
