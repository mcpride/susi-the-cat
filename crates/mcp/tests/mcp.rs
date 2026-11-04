//! End-to-end MCP test: rmcp client <-> SusiServer over an in-memory duplex stream.

use rmcp::model::{CallToolRequestParams, ClientConfig};
use rmcp::{ClientHandler, ServiceExt};
use serde_json::{Value, json};
use susi_mcp::{Session, SusiServer};

#[derive(Clone, Default)]
struct Client;

impl ClientHandler for Client {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::default()
    }
}

async fn call(client: &rmcp::service::RunningService<rmcp::RoleClient, Client>, name: &'static str, args: Value) -> (bool, String) {
    let res = client.call_tool(CallToolRequestParams::new(name).with_arguments(args.as_object().cloned().unwrap_or_default())).await.unwrap();
    let text = res.content.first().and_then(|c| c.as_text()).map(|t| t.text.clone()).unwrap_or_default();
    (res.is_error.unwrap_or(false), text)
}

#[tokio::test]
async fn translate_over_mcp() {
    let dir = tempfile::tempdir().unwrap();
    let json_file = dir.path().join("app.json");
    std::fs::write(&json_file, r#"{"menu":{"open":"Open {0}","close":"Close"}}"#).unwrap();
    let slp = dir.path().join("app.slp");

    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let mut session = Session::new();
    session.tm_path = Some(dir.path().join("tm.tmx"));
    let server = tokio::spawn(async move {
        let s = SusiServer::new(session).serve(server_io).await.unwrap();
        let _ = s.waiting().await;
    });
    let client = Client.serve(client_io).await.unwrap();

    let tools = client.list_all_tools().await.unwrap();
    let names: Vec<_> = tools.iter().map(|t| t.name.to_string()).collect();
    for n in ["open_project", "list_rows", "set_translations", "build", "validate", "tm_translate", "export", "import"] {
        assert!(names.iter().any(|x| x == n), "missing tool {n}: {names:?}");
    }

    let (e, t) = call(&client, "list_rows", json!({})).await;
    assert!(e && t.contains("open_project"), "{t}");

    let (e, t) = call(&client, "create_project", json!({"path": slp.to_string_lossy(), "languages": ["en", "de"]})).await;
    assert!(!e, "{t}");
    let (e, t) = call(&client, "add_source", json!({"file": json_file.to_string_lossy()})).await;
    assert!(!e, "{t}");

    let (e, t) = call(&client, "list_rows", json!({"status": ["untranslated"], "langs": ["de"]})).await;
    assert!(!e, "{t}");
    let v: Value = serde_json::from_str(&t).unwrap();
    let rows = v["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{t}");
    let open = rows.iter().find(|r| r["original"] == "Open {0}").unwrap();
    assert_eq!(open["placeholders"], json!(["{0}"]));
    let key = open["key"].as_str().unwrap().to_string();

    let (e, t) = call(&client, "set_translations", json!({"items": [{"key": key, "lang": "de", "text": "Öffnen"}]})).await;
    assert!(!e, "{t}");
    let (e, t) = call(&client, "validate", json!({"langs": ["de"]})).await;
    assert!(!e && t.contains("format"), "placeholder issue expected: {t}");

    let (e, t) = call(&client, "set_translations", json!({"items": [{"key": key, "lang": "de", "text": "{0} öffnen", "status": "translated"}]})).await;
    assert!(!e, "{t}");
    let (e, t) = call(&client, "list_rows", json!({"langs": ["de"], "since": "today", "date_field": "translated"})).await;
    assert!(!e, "{t}");
    let v: Value = serde_json::from_str(&t).unwrap();
    assert_eq!(v["total"], 1, "{t}");
    assert!(v["rows"][0]["translations"]["de"]["date"].is_string(), "{t}");
    let (e, t) = call(&client, "list_rows", json!({"until": "yesterday"})).await;
    assert!(!e && serde_json::from_str::<Value>(&t).unwrap()["total"] == 0, "{t}");
    let (e, t) = call(&client, "list_rows", json!({"since": "next week"})).await;
    assert!(e && t.contains("invalid date"), "{t}");

    let (e, t) = call(&client, "save_project", json!({})).await;
    assert!(!e, "{t}");
    let (e, t) = call(&client, "build", json!({"langs": ["de"]})).await;
    assert!(!e, "{t}");
    let built = std::fs::read_to_string(dir.path().join("de").join("app.json")).unwrap();
    assert!(built.contains("{0} öffnen"), "{built}");

    let (e, t) = call(&client, "get_row", json!({"key": "does-not-exist"})).await;
    assert!(e && t.contains("not found"), "{t}");

    client.cancel().await.unwrap();
    let _ = server.await;
    assert!(slp.exists());
}
