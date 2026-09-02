mod miniapp;

use mahayana_plugin_runtime::{ExternalReleaseManifest, PluginInstaller};
use mahayana_product::MahayanaProductClient;
use mahayana_test_driver_protocol::{TestDriverBackend, TestDriverError, TestDriverMethod};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const TEST_DRIVER_ROOT_ENV: &str = "MAHAYANA_TEST_DRIVER_ROOT";
const TEST_DRIVER_ROOT_BASENAME: &str = "mahayana-test-driver";
const PREFERRED_PLUGIN_RUNTIMES: &[&str] = &["local-web", "plugin-package", "web", "js"];

pub(crate) struct ProductBackend {
    product: MahayanaProductClient,
    root: PathBuf,
}

impl ProductBackend {
    pub(crate) fn from_environment() -> Result<Self, TestDriverError> {
        let root = test_driver_root_from_environment()?;
        std::fs::create_dir_all(&root).map_err(|error| {
            TestDriverError::new(
                "test_profile_unavailable",
                format!("failed to create isolated test profile: {error}"),
            )
        })?;
        ensure_safe_test_root(&root)?;
        let product = new_product_client(&root);
        Ok(Self { product, root })
    }

    fn reset_profile(&mut self) -> Result<Value, TestDriverError> {
        ensure_safe_test_root(&self.root)?;
        if self.root.exists() {
            std::fs::remove_dir_all(&self.root).map_err(|error| {
                TestDriverError::new(
                    "test_profile_reset_failed",
                    format!("failed to remove isolated test profile: {error}"),
                )
            })?;
        }
        std::fs::create_dir_all(&self.root).map_err(|error| {
            TestDriverError::new(
                "test_profile_reset_failed",
                format!("failed to recreate isolated test profile: {error}"),
            )
        })?;
        self.product = new_product_client(&self.root);
        Ok(json!({
            "reset": true,
            "profileKind": "isolated-test-driver",
        }))
    }

    fn list_installed_plugins(&self) -> Result<Value, TestDriverError> {
        Ok(json!({
            "source": "mahayana-plugin-runtime-active-receipts",
            "plugins": list_active_plugin_receipts(&self.root)?,
        }))
    }

    fn login_test_account(&self, params: &Value) -> Result<Value, TestDriverError> {
        reject_inline_test_account_token(params)?;
        let token = std::env::var("MAHAYANA_TEST_ACCOUNT_TOKEN")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                TestDriverError::new(
                    "test_account_token_missing",
                    "MAHAYANA_TEST_ACCOUNT_TOKEN is required for loginTestAccount",
                )
            })?;
        self.product
            .store_test_account_session(&token)
            .map_err(|error| {
                TestDriverError::new("product_backend_error", error.to_string()).with_details(
                    json!({
                        "operation": TestDriverMethod::LoginTestAccount.as_str(),
                        "tokenSource": "environment",
                    }),
                )
            })?;
        Ok(json!({
            "loggedIn": true,
            "accountKind": "test",
            "tokenSource": "environment",
        }))
    }

    fn marketplace_search(&self, params: &Value) -> Result<Value, TestDriverError> {
        let (query, platform) = marketplace_search_args(params)?;
        self.product
            .marketplace_browse(Some(query), Some(platform))
            .map_err(|error| {
                TestDriverError::new("product_backend_error", error.to_string()).with_details(
                    json!({
                        "operation": TestDriverMethod::MarketplaceSearch.as_str(),
                        "platform": platform,
                    }),
                )
            })
    }

    fn install_external_marketplace_plugin(
        &self,
        params: &Value,
        require_existing: bool,
        operation: TestDriverMethod,
    ) -> Result<Value, TestDriverError> {
        let (plugin_id, requested_version, platform) = plugin_release_args(params)?;
        let listing = self
            .product
            .marketplace_browse(Some(plugin_id), Some(platform))
            .map_err(|error| {
                TestDriverError::new("product_backend_error", error.to_string())
                    .with_details(json!({"operation": operation.as_str(), "platform": platform}))
            })?;
        let plugin = listing
            .get("plugins")
            .and_then(Value::as_array)
            .and_then(|plugins| {
                plugins.iter().find(|plugin| {
                    plugin.get("pluginId").and_then(Value::as_str) == Some(plugin_id)
                })
            })
            .ok_or_else(|| {
                TestDriverError::new(
                    "marketplace_plugin_not_found",
                    format!("approved marketplace plugin {plugin_id} was not found for {platform}"),
                )
            })?;
        let version = requested_version
            .map(str::to_string)
            .or_else(|| {
                plugin
                    .get("latestVersion")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .ok_or_else(|| {
                TestDriverError::new(
                    "marketplace_version_missing",
                    "marketplace entry has no installable version",
                )
            })?;
        let metadata = self
            .product
            .marketplace_release_metadata(plugin_id, &version)
            .map_err(|error| {
                TestDriverError::new("product_backend_error", error.to_string()).with_details(
                    json!({
                        "operation": operation.as_str(),
                        "pluginId": plugin_id,
                        "version": version,
                    }),
                )
            })?;
        if metadata.get("pluginId").and_then(Value::as_str) != Some(plugin_id)
            || metadata.get("version").and_then(Value::as_str) != Some(version.as_str())
        {
            return Err(TestDriverError::new(
                "marketplace_release_mismatch",
                "marketplace release metadata does not match requested plugin/version",
            ));
        }
        let marketplace_source = metadata.get("source").cloned().unwrap_or(Value::Null);
        let release_value = metadata.get("releaseManifest").cloned().ok_or_else(|| {
            TestDriverError::new(
                "external_release_required",
                "test-driver installation requires an external release manifest; bundled/legacy packages are rejected",
            )
        })?;
        let release = serde_json::from_value::<ExternalReleaseManifest>(release_value.clone())
            .map_err(|error| {
                TestDriverError::new(
                    "invalid_external_release",
                    format!("marketplace external release manifest is invalid: {error}"),
                )
            })?;
        release.validate().map_err(|error| {
            TestDriverError::new(
                "invalid_external_release",
                format!("marketplace external release validation failed: {error}"),
            )
        })?;
        if release.plugin_id != plugin_id || release.version != version {
            return Err(TestDriverError::new(
                "external_release_mismatch",
                "external release identity does not match marketplace metadata",
            ));
        }

        let installer = plugin_installer(&self.root)?;
        let previous = installer.active(plugin_id).map_err(|error| {
            TestDriverError::new(
                "plugin_state_read_failed",
                format!("failed to read active plugin receipt: {error}"),
            )
        })?;
        if require_existing && previous.is_none() {
            return Err(TestDriverError::new(
                "plugin_not_installed",
                format!("plugin {plugin_id} must be installed before plugin.update"),
            ));
        }
        let receipt = installer
            .install(&release, platform, PREFERRED_PLUGIN_RUNTIMES)
            .map_err(|error| {
                TestDriverError::new(
                    "plugin_install_failed",
                    format!("verified external plugin installation failed: {error}"),
                )
            })?;
        Ok(json!({
            "installed": true,
            "updated": previous.is_some(),
            "platform": platform,
            "source": "online-external-marketplace",
            "marketplaceSource": marketplace_source,
            "releaseVersion": version,
            "previousReceipt": previous,
            "receipt": receipt,
            "releaseManifest": release_value,
        }))
    }
}

impl TestDriverBackend for ProductBackend {
    fn backend_name(&self) -> &'static str {
        "mahayana-product-core"
    }

    fn execute(
        &mut self,
        method: TestDriverMethod,
        params: Value,
        correlation_id: &str,
    ) -> Result<Value, TestDriverError> {
        match method {
            TestDriverMethod::ResetProfile => self.reset_profile(),
            TestDriverMethod::LoginTestAccount => self.login_test_account(&params),
            TestDriverMethod::MarketplaceSearch => self.marketplace_search(&params),
            TestDriverMethod::PluginInstall => self.install_external_marketplace_plugin(
                &params,
                false,
                TestDriverMethod::PluginInstall,
            ),
            TestDriverMethod::PluginUpdate => self.install_external_marketplace_plugin(
                &params,
                true,
                TestDriverMethod::PluginUpdate,
            ),
            TestDriverMethod::PluginList => self.list_installed_plugins(),
            TestDriverMethod::MiniappOpen => {
                miniapp::open_miniapp(&self.product, &self.root, &params, correlation_id)
            }
            TestDriverMethod::MiniappChat => {
                miniapp::chat_miniapp(&self.product, &self.root, &params, correlation_id)
            }
            TestDriverMethod::ActionsDescribe => {
                miniapp::describe_actions(&self.product, &self.root, &params, correlation_id)
            }
            TestDriverMethod::ActionsInvoke => {
                miniapp::invoke_action(&self.product, &self.root, &params, correlation_id)
            }
            other => Err(TestDriverError::new(
                "product_backend_not_wired",
                format!(
                    "{} is not yet wired to its Mahayana product-core operation",
                    other.as_str()
                ),
            )),
        }
    }
}

fn new_product_client(root: &Path) -> MahayanaProductClient {
    MahayanaProductClient::new_with_default_api_base_url(
        root.join("session.json"),
        root.join("product-surface.json"),
    )
}

fn plugin_installer(root: &Path) -> Result<PluginInstaller, TestDriverError> {
    PluginInstaller::new(root.join("plugins")).map_err(|error| {
        TestDriverError::new(
            "plugin_runtime_unavailable",
            format!("failed to initialize Mahayana plugin installer: {error}"),
        )
    })
}

fn list_active_plugin_receipts(root: &Path) -> Result<Vec<Value>, TestDriverError> {
    let plugins_root = root.join("plugins");
    if !plugins_root.exists() {
        return Ok(Vec::new());
    }
    let installer = plugin_installer(root)?;
    let entries = std::fs::read_dir(&plugins_root).map_err(|error| {
        TestDriverError::new(
            "plugin_state_read_failed",
            format!("failed to enumerate installed plugins: {error}"),
        )
    })?;
    let mut receipts = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            TestDriverError::new(
                "plugin_state_read_failed",
                format!("failed to enumerate installed plugins: {error}"),
            )
        })?;
        if !entry
            .file_type()
            .map_err(|error| {
                TestDriverError::new(
                    "plugin_state_read_failed",
                    format!("failed to inspect installed plugin state: {error}"),
                )
            })?
            .is_dir()
        {
            continue;
        }
        let plugin_id = entry.file_name().into_string().map_err(|_| {
            TestDriverError::new(
                "plugin_state_read_failed",
                "installed plugin directory is not valid UTF-8",
            )
        })?;
        if let Some(pointer) = installer.active(&plugin_id).map_err(|error| {
            TestDriverError::new(
                "plugin_state_read_failed",
                format!("failed to read active receipt for {plugin_id}: {error}"),
            )
        })? {
            receipts.push(serde_json::to_value(pointer).map_err(|error| {
                TestDriverError::new(
                    "plugin_state_read_failed",
                    format!("failed to serialize active receipt for {plugin_id}: {error}"),
                )
            })?);
        }
    }
    receipts.sort_by(|left, right| {
        left.get("pluginId")
            .and_then(Value::as_str)
            .cmp(&right.get("pluginId").and_then(Value::as_str))
    });
    Ok(receipts)
}

fn plugin_release_args(params: &Value) -> Result<(&str, Option<&str>, &str), TestDriverError> {
    let plugin_id = required_trimmed(
        params,
        "pluginId",
        "plugin.install/update requires pluginId",
    )?;
    let requested_version = params
        .get("version")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let platform = params
        .get("platform")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("ios");
    Ok((plugin_id, requested_version, platform))
}

fn test_driver_root_from_environment() -> Result<PathBuf, TestDriverError> {
    let root = std::env::var_os(TEST_DRIVER_ROOT_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| {
            TestDriverError::new(
                "test_profile_root_missing",
                format!("{TEST_DRIVER_ROOT_ENV} must point to an isolated test profile"),
            )
        })?;
    ensure_safe_test_root(&root)?;
    Ok(root)
}

fn ensure_safe_test_root(root: &Path) -> Result<(), TestDriverError> {
    if !root.is_absolute()
        || root.parent().is_none()
        || root.file_name().and_then(|value| value.to_str()) != Some(TEST_DRIVER_ROOT_BASENAME)
    {
        return Err(TestDriverError::new(
            "unsafe_test_profile_root",
            format!(
                "{TEST_DRIVER_ROOT_ENV} must be an absolute path whose final component is {TEST_DRIVER_ROOT_BASENAME}"
            ),
        ));
    }
    match std::fs::symlink_metadata(root) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(TestDriverError::new(
            "unsafe_test_profile_root",
            "test profile root must not be a symbolic link",
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(TestDriverError::new(
            "test_profile_unavailable",
            format!("failed to inspect isolated test profile: {error}"),
        )),
    }
}

fn reject_inline_test_account_token(params: &Value) -> Result<(), TestDriverError> {
    if ["token", "accessToken", "authorization"]
        .iter()
        .any(|key| params.get(*key).is_some())
    {
        return Err(TestDriverError::new(
            "inline_credentials_forbidden",
            "loginTestAccount credentials must come from MAHAYANA_TEST_ACCOUNT_TOKEN, never request params",
        ));
    }
    Ok(())
}

fn marketplace_search_args(params: &Value) -> Result<(&str, &str), TestDriverError> {
    let query = required_trimmed(
        params,
        "query",
        "marketplace.search requires a non-empty query",
    )?;
    let platform = params
        .get("platform")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("ios");
    Ok((query, platform))
}

fn required_trimmed<'a>(
    params: &'a Value,
    key: &str,
    message: &str,
) -> Result<&'a str, TestDriverError> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| TestDriverError::new("invalid_params", message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mahayana_plugin_runtime::InstalledPluginPointer;

    #[test]
    fn marketplace_search_defaults_to_ios_and_trims_query() {
        let params = json!({"query": "  全球法布施  "});
        let (query, platform) = marketplace_search_args(&params).unwrap();
        assert_eq!(query, "全球法布施");
        assert_eq!(platform, "ios");
    }

    #[test]
    fn plugin_release_args_default_to_ios() {
        let params = json!({"pluginId": "global-dharma"});
        let (plugin_id, version, platform) = plugin_release_args(&params).unwrap();
        assert_eq!(plugin_id, "global-dharma");
        assert_eq!(version, None);
        assert_eq!(platform, "ios");
    }

    #[test]
    fn login_test_account_rejects_inline_token_material() {
        let error =
            reject_inline_test_account_token(&json!({"token": "must-not-pass"})).unwrap_err();
        assert_eq!(error.code, "inline_credentials_forbidden");
    }

    #[test]
    fn test_profile_root_requires_absolute_dedicated_basename() {
        assert_eq!(
            ensure_safe_test_root(Path::new("mahayana-test-driver"))
                .unwrap_err()
                .code,
            "unsafe_test_profile_root"
        );
        assert_eq!(
            ensure_safe_test_root(&std::env::temp_dir().join("not-the-driver-root"))
                .unwrap_err()
                .code,
            "unsafe_test_profile_root"
        );
    }

    #[test]
    fn plugin_list_reads_persisted_active_receipt() {
        let parent = std::env::temp_dir().join(format!(
            "mahayana-test-driver-list-unit-{}",
            std::process::id()
        ));
        let root = parent.join(TEST_DRIVER_ROOT_BASENAME);
        let installed_path = root
            .join("plugins")
            .join("global-dharma")
            .join("1.0.0")
            .join("local-web");
        std::fs::create_dir_all(&installed_path).unwrap();
        let pointer = InstalledPluginPointer {
            plugin_id: "global-dharma".into(),
            version: "1.0.0".into(),
            artifact_id: "ios-local-web".into(),
            artifact_sha256: "43de877dc87b5dff306164eb143baad545ef40bea2247f28cbe21616829478be"
                .into(),
            runtime: "local-web".into(),
            entry: Some("index.html".into()),
            requested_permissions: Vec::new(),
            installed_path: installed_path.to_string_lossy().into_owned(),
        };
        let state_dir = root.join("plugins").join("global-dharma");
        std::fs::write(
            state_dir.join("active.json"),
            serde_json::to_vec_pretty(&pointer).unwrap(),
        )
        .unwrap();
        let receipts = list_active_plugin_receipts(&root).unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0]["pluginId"], "global-dharma");
        assert_eq!(receipts[0]["version"], "1.0.0");
        let _ = std::fs::remove_dir_all(parent);
    }
}
