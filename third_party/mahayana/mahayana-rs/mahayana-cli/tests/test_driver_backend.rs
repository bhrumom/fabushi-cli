#[path = "../src/bin/mahayana-test-driver/backend.rs"]
mod backend;

use backend::ProductBackend;
use mahayana_test_driver_protocol::{
    TEST_DRIVER_PROTOCOL, TestDriverMethod, TestDriverRequest, TestDriverSession,
};
use serde_json::{Value, json};
use std::path::PathBuf;

fn request(
    request_id: &str,
    correlation_id: &str,
    method: TestDriverMethod,
    params: Value,
) -> TestDriverRequest {
    TestDriverRequest {
        protocol: TEST_DRIVER_PROTOCOL.into(),
        request_id: request_id.into(),
        correlation_id: Some(correlation_id.into()),
        method,
        params,
    }
}

fn live_test_token() -> Option<String> {
    if std::env::var("GITHUB_ACTIONS").ok().as_deref() != Some("true") {
        return None;
    }
    std::env::var("TEST_ACCOUNT_TOKEN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn isolated_root() -> PathBuf {
    std::env::temp_dir()
        .join(format!("mahayana-live-test-driver-{}", std::process::id()))
        .join("mahayana-test-driver")
}

#[test]
fn live_official_global_dharma_is_external_verified_and_persistent() {
    let Some(token) = live_test_token() else {
        eprintln!(
            "live test-driver E2E skipped: TEST_ACCOUNT_TOKEN is unavailable outside the cloud marketplace gate"
        );
        return;
    };

    let root = isolated_root();
    let _ = std::fs::remove_dir_all(root.parent().unwrap());

    // SAFETY: this integration-test process owns these two test-only variables.
    // Other tests in this crate do not mutate or consume either variable.
    unsafe {
        std::env::set_var("MAHAYANA_TEST_DRIVER_ROOT", &root);
        std::env::set_var("MAHAYANA_TEST_ACCOUNT_TOKEN", &token);
    }

    let backend = ProductBackend::from_environment().expect("create isolated product backend");
    let mut session = TestDriverSession::new(backend);

    let reset = session.execute(request(
        "reset-1",
        "corr-reset",
        TestDriverMethod::ResetProfile,
        json!({}),
    ));
    assert!(reset.ok, "reset failed: {reset:?}");

    let pre_list = session.execute(request(
        "pre-list-1",
        "corr-pre-list",
        TestDriverMethod::PluginList,
        json!({}),
    ));
    assert!(pre_list.ok, "pre-install list failed: {pre_list:?}");
    assert_eq!(pre_list.result["plugins"], json!([]));

    let login = session.execute(request(
        "login-1",
        "corr-login",
        TestDriverMethod::LoginTestAccount,
        json!({}),
    ));
    assert!(login.ok, "test-account login failed: {login:?}");
    assert_eq!(login.result["accountKind"], "test");
    assert_eq!(login.result["tokenSource"], "[REDACTED]");

    let search = session.execute(request(
        "search-1",
        "corr-market-global-dharma",
        TestDriverMethod::MarketplaceSearch,
        json!({"query": "全球法布施", "platform": "ios"}),
    ));
    assert!(search.ok, "live marketplace search failed: {search:?}");
    let search_text = serde_json::to_string(&search.result).unwrap();
    assert!(search_text.contains("global-dharma"), "{search_text}");
    assert!(search_text.contains("全球法布施"), "{search_text}");

    let install = session.execute(request(
        "install-1",
        "corr-install-global-dharma",
        TestDriverMethod::PluginInstall,
        json!({"pluginId": "global-dharma", "platform": "ios"}),
    ));
    assert!(install.ok, "live external install failed: {install:?}");
    assert_eq!(install.result["source"], "online-external-marketplace");
    assert_eq!(install.result["receipt"]["pluginId"], "global-dharma");
    assert_eq!(install.result["receipt"]["platform"], "ios");
    assert_eq!(install.result["receipt"]["runtime"], "local-web");
    assert_eq!(
        install.result["releaseVersion"],
        install.result["receipt"]["version"]
    );
    let sha = install.result["receipt"]["sha256"]
        .as_str()
        .expect("receipt sha256");
    assert_eq!(sha.len(), 64);
    assert!(sha.chars().all(|character| character.is_ascii_hexdigit()));
    assert!(
        install.result["receipt"]["sourceUrl"]
            .as_str()
            .is_some_and(|url| url.starts_with("https://"))
    );
    assert_eq!(
        install.result["releaseManifest"]["protocol"],
        "mahayana.external-release.v1"
    );
    assert_eq!(
        install.result["releaseManifest"]["pluginId"],
        "global-dharma"
    );
    assert_eq!(
        install.result["releaseManifest"]["version"],
        install.result["receipt"]["version"]
    );
    assert_eq!(
        install.result["marketplaceSource"]["marketplaceHostsPackage"],
        false
    );
    assert!(
        install.result["marketplaceSource"]["sourceRef"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    assert!(
        install.result["marketplaceSource"]["repository"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    let selected_artifact = install.result["releaseManifest"]["artifacts"]
        .as_array()
        .and_then(|artifacts| {
            artifacts.iter().find(|artifact| {
                artifact["runtime"] == install.result["receipt"]["runtime"]
                    && artifact["sha256"] == install.result["receipt"]["sha256"]
            })
        })
        .expect("installed receipt must match a release-manifest artifact");
    assert!(
        selected_artifact["platforms"]
            .as_array()
            .is_some_and(|platforms| platforms.iter().any(|platform| platform == "ios"))
    );

    let post_list = session.execute(request(
        "post-list-1",
        "corr-post-list",
        TestDriverMethod::PluginList,
        json!({}),
    ));
    assert!(post_list.ok, "post-install list failed: {post_list:?}");
    let installed = post_list.result["plugins"]
        .as_array()
        .expect("installed plugins array");
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0]["pluginId"], "global-dharma");
    assert_eq!(installed[0]["sha256"], install.result["receipt"]["sha256"]);
    assert_eq!(
        installed[0]["sourceUrl"],
        install.result["receipt"]["sourceUrl"]
    );

    let logs = session.execute(request(
        "logs-1",
        "corr-logs",
        TestDriverMethod::LogsQuery,
        json!({"correlationId": "corr-install-global-dharma"}),
    ));
    assert!(logs.ok, "logs query failed: {logs:?}");
    let entries = logs.result["entries"].as_array().expect("log entries");
    assert!(!entries.is_empty());
    assert!(
        entries
            .iter()
            .all(|entry| { entry["correlationId"] == "corr-install-global-dharma" })
    );
    assert!(
        entries
            .iter()
            .any(|entry| { entry["message"] == "test-driver request started" })
    );
    assert!(
        entries
            .iter()
            .any(|entry| { entry["message"] == "test-driver request completed" })
    );

    let events = session.execute(request(
        "events-1",
        "corr-events",
        TestDriverMethod::EventsSubscribe,
        json!({"afterSequence": 0}),
    ));
    assert!(events.ok, "event subscription failed: {events:?}");
    let install_events = events.result["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["correlationId"] == "corr-install-global-dharma")
        .collect::<Vec<_>>();
    assert!(
        install_events
            .iter()
            .any(|event| event["kind"] == "request.started")
    );
    assert!(
        install_events
            .iter()
            .any(|event| event["kind"] == "request.completed")
    );

    drop(session);
    let restarted_backend =
        ProductBackend::from_environment().expect("reopen isolated product backend");
    let mut restarted = TestDriverSession::new(restarted_backend);
    let restart_list = restarted.execute(request(
        "restart-list-1",
        "corr-restart-list",
        TestDriverMethod::PluginList,
        json!({}),
    ));
    assert!(restart_list.ok, "restart list failed: {restart_list:?}");
    let restart_plugins = restart_list.result["plugins"]
        .as_array()
        .expect("restart installed plugins array");
    assert_eq!(restart_plugins.len(), 1);
    assert_eq!(restart_plugins[0]["pluginId"], "global-dharma");
    assert_eq!(
        restart_plugins[0]["sha256"],
        install.result["receipt"]["sha256"]
    );
    assert_eq!(
        restart_plugins[0]["sourceUrl"],
        install.result["receipt"]["sourceUrl"]
    );

    unsafe {
        std::env::remove_var("MAHAYANA_TEST_DRIVER_ROOT");
        std::env::remove_var("MAHAYANA_TEST_ACCOUNT_TOKEN");
    }
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}
