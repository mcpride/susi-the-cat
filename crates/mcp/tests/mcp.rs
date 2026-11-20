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

#[tokio::test]
async fn copy_over_mcp() {
    let dir = tempfile::tempdir().unwrap();
    let json_file = dir.path().join("app.json");
    std::fs::write(&json_file, r#"{"menu":{"open":"Open {0}","close":"Close"}}"#).unwrap();
    let slp = dir.path().join("app.slp");
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    let server = tokio::spawn(async move {
        let s = SusiServer::new(Session::new()).serve(server_io).await.unwrap();
        let _ = s.waiting().await;
    });
    let client = Client.serve(client_io).await.unwrap();
    let json = |t: &str| serde_json::from_str::<Value>(t).unwrap();

    let (e, t) = call(&client, "create_project", json!({"path": slp.to_string_lossy(), "languages": ["en", "de", "de-CH"]})).await;
    assert!(!e, "{t}");
    let (e, t) = call(&client, "add_source", json!({"file": json_file.to_string_lossy()})).await;
    assert!(!e, "{t}");
    let (_, t) = call(&client, "list_rows", json!({})).await;
    let rows = json(&t)["rows"].as_array().unwrap().clone();
    let key = |original: &str| rows.iter().find(|r| r["original"] == original).unwrap()["key"].as_str().unwrap().to_string();
    let (open, close) = (key("Open {0}"), key("Close"));
    let items = json!([{"key": open, "lang": "de", "text": "{0} öffnen"}, {"key": close, "lang": "de", "text": "Schließen"}]);
    assert!(!call(&client, "set_translations", json!({ "items": items })).await.0);

    let (e, t) = call(&client, "copy_translations", json!({"from": "de", "lang": "de-CH"})).await;
    assert!(!e && json(&t)["copied"] == 2, "{t}");
    let items = json!([{"key": close, "lang": "de-CH", "text": "Schliessen"}]);
    assert!(!call(&client, "set_translations", json!({ "items": items })).await.0);
    let (_, t) = call(&client, "copy_translations", json!({"from": "de", "lang": "de-CH"})).await;
    assert_eq!((json(&t)["copied"].clone(), json(&t)["kept"].clone()), (json!(0), json!(1)), "{t}");

    // The original into one row, overwriting; unknown keys are reported per key.
    let (e, t) = call(&client, "copy_translations", json!({"lang": "de-CH", "keys": [close, "nope"], "overwrite": true})).await;
    assert!(!e && json(&t)["copied"] == 1 && json(&t)["errors"].as_array().unwrap().len() == 1, "{t}");
    let (_, t) = call(&client, "get_row", json!({ "key": close })).await;
    assert_eq!(json(&t)["translations"]["de-CH"]["text"], "Close", "{t}");

    for (args, error) in [
        (json!({"from": "de-CH", "lang": "de-CH"}), "different"),
        (json!({"lang": "fr"}), "'fr' is not part of the project"),
        (json!({"lang": "de", "source": 7}), "source 7 does not exist"),
    ] {
        let (e, t) = call(&client, "copy_translations", args).await;
        assert!(e && t.contains(error), "{t}");
    }
    client.cancel().await.unwrap();
    let _ = server.await;
}

#[tokio::test]
async fn screenshots_over_mcp() {
    let dir = tempfile::tempdir().unwrap();
    let json_file = dir.path().join("app.json");
    std::fs::write(&json_file, r#"{"menu":{"open":"Open","close":"Close"}}"#).unwrap();
    let png = dir.path().join("menu.png");
    std::fs::write(&png, susi_core::screenshots::solid_png(120, 80, [10, 20, 30])).unwrap();
    let slp = dir.path().join("app.slp");

    let (server_io, client_io) = tokio::io::duplex(256 * 1024);
    let server = tokio::spawn(async move {
        let s = SusiServer::new(Session::new()).serve(server_io).await.unwrap();
        let _ = s.waiting().await;
    });
    let client = Client.serve(client_io).await.unwrap();
    assert!(!call(&client, "create_project", json!({"path": slp.to_string_lossy(), "languages": ["en", "de"]})).await.0);
    assert!(!call(&client, "add_source", json!({"file": json_file.to_string_lossy()})).await.0);

    let keys = json!([{"key": "*menu.open", "area": [5, 5, 40, 12]}, {"key": "nothing"}]);
    let (e, t) = call(&client, "attach_screenshot", json!({"path": png.to_string_lossy(), "keys": keys, "title": "Menu"})).await;
    assert!(!e, "{t}");
    let v: Value = serde_json::from_str(&t).unwrap();
    assert_eq!((v["id"].as_str(), v["linked_rows"].as_u64()), (Some("menu"), Some(1)), "{t}");
    assert!(v["errors"][0].as_str().unwrap().contains("nothing"), "{t}");

    let (_, t) = call(&client, "list_rows", json!({"has_screenshot": true})).await;
    let v: Value = serde_json::from_str(&t).unwrap();
    assert_eq!(v["total"], 1, "{t}");
    assert_eq!(v["rows"][0]["screenshots"], json!([{"id": "menu", "title": "Menu", "area": [5, 5, 40, 12]}]));

    let res = client.call_tool(CallToolRequestParams::new("get_screenshot").with_arguments(json!({"id": "menu"}).as_object().cloned().unwrap())).await.unwrap();
    assert_ne!(res.is_error, Some(true));
    let image = res.content[0].as_image().expect("image content");
    assert_eq!(image.mime_type, "image/png");
    assert!(image.data.starts_with("iVBORw0KGgo"), "base64 PNG");
    let info = res.content[1].as_text().unwrap().text.clone();
    assert!(info.contains("\"original\": \"Open\"") && info.contains("\"width\": 120"), "{info}");
    let res = client.call_tool(CallToolRequestParams::new("get_screenshot").with_arguments(json!({"id": "nope"}).as_object().cloned().unwrap())).await.unwrap();
    assert_eq!(res.is_error, Some(true));

    assert!(!call(&client, "save_project", json!({})).await.0);
    assert!(dir.path().join("app.files").join("screenshots.json").is_file());
    let (e, t) = call(&client, "validate", json!({"kinds": ["screenshot"]})).await;
    assert!(!e && t.contains("\"count\": 0"), "{t}");
    let (e, t) = call(&client, "project_info", json!({})).await;
    assert!(!e && t.contains("\"rows_with_screenshots\": 1"), "{t}");
    let (e, t) = call(&client, "detach_screenshot", json!({"id": "menu", "keys": [{"key": "*menu.open"}]})).await;
    assert!(!e && t.contains("\"unlinked\": 1"), "{t}");

    client.cancel().await.unwrap();
    let _ = server.await;
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to.join(e.file_name()));
        } else {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}

/// Pictures and binary data (fixture with Sisulizer's translated project).
#[tokio::test]
async fn custom_data_over_mcp() {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/projects/custom-data"), dir.path());
    let slp = dir.path().join("app.slp");
    std::fs::copy(dir.path().join("sisulizer").join("translated.slp"), &slp).unwrap();
    let (server_io, client_io) = tokio::io::duplex(256 * 1024);
    let server = tokio::spawn(async move {
        let s = SusiServer::new(Session::new()).serve(server_io).await.unwrap();
        let _ = s.waiting().await;
    });
    let client = Client.serve(client_io).await.unwrap();
    let json = |t: &str| serde_json::from_str::<Value>(t).unwrap();
    assert!(!call(&client, "open_project", json!({"path": slp.to_string_lossy()})).await.0);

    let (_, t) = call(&client, "get_row", json!({"key": "Logo/Logo"})).await;
    let v = json(&t);
    assert_eq!(
        (v["original"].clone(), v["data"].clone()),
        (Value::Null, json!({"format": "picture", "size": 76, "type": "PNG", "width": 16, "height": 8})),
        "{t}"
    );
    assert_eq!(v["translations"]["de"]["file"], "de\\logo.png", "{t}");
    let (_, t) = call(&client, "get_row", json!({"key": "Blob/Blob"})).await;
    assert_eq!(json(&t)["data"], json!({"format": "binary"}), "{t}");

    let picture = |args: Value| {
        let client = &client;
        async move { client.call_tool(CallToolRequestParams::new("get_picture").with_arguments(args.as_object().cloned().unwrap())).await.unwrap() }
    };
    let res = picture(json!({"key": "Logo/Logo", "lang": "de"})).await;
    assert_ne!(res.is_error, Some(true));
    let image = res.content[0].as_image().expect("image content");
    assert!(image.mime_type == "image/png" && image.data.starts_with("iVBORw0KGgo"));
    assert_eq!(picture(json!({"key": "FileGif/FileGif"})).await.content[0].as_image().unwrap().mime_type, "image/gif");
    assert_eq!(picture(json!({"key": "Blob/Blob"})).await.is_error, Some(true));

    let (e, t) = call(&client, "set_translations", json!({"items": [{"key": "Logo/Logo", "lang": "de", "text": "x"}]})).await;
    assert!(!e && t.contains("load_translation_file"), "{t}");
    let path = dir.path().join("de").join("data.bin");
    let (e, t) = call(&client, "load_translation_file", json!({"key": "Blob/Blob", "lang": "de", "path": path.to_string_lossy()})).await;
    assert!(!e, "{t}");
    assert_eq!(
        json(&t)["translations"]["de"],
        json!({"data": {"format": "binary", "size": 40}, "status": "complete", "file": "de\\data.bin", "date": json(&t)["translations"]["de"]["date"]}),
        "{t}"
    );

    // A changed linked file is read again before building.
    std::fs::copy(dir.path().join("de").join("file.png"), dir.path().join("de").join("logo.png")).unwrap();
    let (e, t) = call(&client, "build", json!({"langs": ["de"]})).await;
    assert!(!e && json(&t)["linked_files"]["updated"] == json!(["Logo/Logo (de)"]), "{t}");
    let (_, t) = call(&client, "validate", json!({"kinds": ["data"]})).await;
    assert!(t.contains("differs from the original"), "{t}");

    client.cancel().await.unwrap();
    let _ = server.await;
}
