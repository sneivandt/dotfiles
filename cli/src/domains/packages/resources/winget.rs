//! Winget package provider.

use std::collections::HashSet;

use anyhow::{Context as _, Result};

use super::package::{InstalledPackages, PackageManager, PackageProvider};
use crate::engine::ResourceChange;
use crate::infra::exec::{CommandSpec, Executor};

#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Inventory {
    sources: Vec<InventorySource>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct InventorySource {
    packages: Vec<InventoryPackage>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct InventoryPackage {
    package_identifier: String,
}

pub(super) fn parse_winget_ids(json: &str) -> Result<HashSet<String>> {
    let inventory: Inventory = serde_json::from_str(json.trim_start_matches('\u{feff}'))
        .context("could not parse winget export inventory")?;
    let mut ids = HashSet::new();
    for package in inventory
        .sources
        .into_iter()
        .flat_map(|source| source.packages)
    {
        anyhow::ensure!(
            !package.package_identifier.trim().is_empty(),
            "winget export inventory contains an empty package identifier"
        );
        ids.insert(package.package_identifier);
    }
    Ok(ids)
}

#[cfg(test)]
pub(crate) fn mock_inventory(spec: &CommandSpec, ids: &[&str]) {
    assert_eq!(spec.program(), "winget");
    let args = spec.arguments();
    assert_eq!(args.len(), 5);
    assert_eq!(args[0], "export");
    assert_eq!(args[1], "--output");
    assert_eq!(args[3], "--accept-source-agreements");
    assert_eq!(args[4], "--disable-interactivity");
    assert!(spec.is_checked());
    let output = std::path::Path::new(&args[2]);
    std::fs::write(
        output,
        serde_json::json!({
            "Sources": [{"Packages": ids.iter().map(|id| {
                serde_json::json!({"PackageIdentifier": id})
            }).collect::<Vec<_>>()}]
        })
        .to_string(),
    )
    .unwrap();
}

/// Winget exit codes this provider reacts to.
///
/// winget reports failures as `HRESULT`s, which arrive here as negative `i32`s.
/// Only the codes that change our behaviour are named; everything else falls
/// through to the generic failure path.
mod exit_code {
    /// `APPINSTALLER_CLI_ERROR_NO_APPLICABLE_INSTALLER` (`0x8A15_0010`).
    ///
    /// The package has no installer matching the requested scope, so a
    /// `--scope user` attempt should be retried without the scope filter.
    pub(super) const NO_APPLICABLE_INSTALLER: i32 = -1_978_335_216;

    /// `APPINSTALLER_CLI_ERROR_COMMAND_REQUIRES_ADMIN` (`0x8A15_0019`).
    pub(super) const COMMAND_REQUIRES_ADMIN: i32 = -1_978_335_207;

    /// `APPINSTALLER_CLI_ERROR_INSTALL_CANCELLED_BY_USER` (`0x8A15_010C`).
    pub(super) const INSTALL_CANCELLED_BY_USER: i32 = -1_978_334_964;

    /// `APPINSTALLER_CLI_ERROR_INSTALL_BLOCKED_BY_POLICY` (`0x8A15_010F`).
    pub(super) const INSTALL_BLOCKED_BY_POLICY: i32 = -1_978_334_961;

    /// `APPINSTALLER_CLI_ERROR_PACKAGE_ALREADY_INSTALLED` (`0x8A15_0061`).
    pub(super) const PACKAGE_ALREADY_INSTALLED: i32 = -1_978_335_135;

    /// `APPINSTALLER_CLI_ERROR_UPDATE_NOT_APPLICABLE` (`0x8A15_002B`).
    ///
    /// `winget install` switches to upgrading when it finds an installed
    /// package. No applicable upgrade still satisfies our presence-only goal.
    pub(super) const UPDATE_NOT_APPLICABLE: i32 = -1_978_335_189;

    /// `APPINSTALLER_CLI_ERROR_INSTALL_ALREADY_INSTALLED` (`0x8A15_010D`).
    pub(super) const INSTALL_ALREADY_INSTALLED: i32 = -1_978_334_963;

    /// `APPINSTALLER_CLI_ERROR_INSTALL_REBOOT_REQUIRED_TO_FINISH` (`0x8A15_0109`).
    pub(super) const INSTALL_REBOOT_REQUIRED_TO_FINISH: i32 = -1_978_334_967;
}

/// How the package task should treat a winget exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallVerdict {
    /// Installation made progress but requires a reboot to finish.
    PendingReboot,
    /// Winget reports the package was already present despite list discovery.
    AlreadyInstalled,
    /// The requested scope has no installer; retry without `--scope`.
    RetryWithoutScope,
    /// Expected on an unelevated run; report it rather than failing the task.
    Skipped(&'static str),
    /// Anything else, reported with winget's own output.
    Failed,
}

/// Classify a winget exit code into the action the caller should take.
///
/// Recognising "already installed" as success keeps installs idempotent when
/// inventory discovery under-reports a package, and recognising the privilege codes
/// lets an unelevated run degrade to a skip instead of a hard failure.
const fn classify_install(code: Option<i32>) -> InstallVerdict {
    match code {
        Some(exit_code::NO_APPLICABLE_INSTALLER) => InstallVerdict::RetryWithoutScope,
        Some(
            exit_code::PACKAGE_ALREADY_INSTALLED
            | exit_code::INSTALL_ALREADY_INSTALLED
            | exit_code::UPDATE_NOT_APPLICABLE,
        ) => InstallVerdict::AlreadyInstalled,
        Some(exit_code::INSTALL_REBOOT_REQUIRED_TO_FINISH) => InstallVerdict::PendingReboot,
        Some(exit_code::COMMAND_REQUIRES_ADMIN) => InstallVerdict::Skipped(
            "requires administrator; re-run `dotfiles update --only packages` from an elevated shell",
        ),
        Some(exit_code::INSTALL_CANCELLED_BY_USER) => {
            InstallVerdict::Skipped("installer elevation was declined")
        }
        Some(exit_code::INSTALL_BLOCKED_BY_POLICY) => {
            InstallVerdict::Skipped("blocked by system policy")
        }
        _ => InstallVerdict::Failed,
    }
}

/// Build the `winget install` argument list for `name`.
///
/// `--disable-interactivity` suppresses winget's own prompts so a run cannot
/// stall on a hidden question; it does not suppress an installer's UAC prompt.
/// `--no-upgrade` keeps a missed inventory entry or concurrent install from
/// turning this presence-only request into an application upgrade.
fn install_args<'a>(name: &'a str, scope: Option<&'a str>) -> Vec<&'a str> {
    let mut args = vec![
        "install",
        "--id",
        name,
        "--exact",
        "--no-upgrade",
        "--source",
        "winget",
        "--accept-source-agreements",
        "--accept-package-agreements",
        "--disable-interactivity",
    ];
    if let Some(scope) = scope {
        args.push("--scope");
        args.push(scope);
    }
    args
}

/// Winget provider for Windows packages.
#[derive(Debug, Clone, Copy)]
pub(super) struct WingetProvider;

impl PackageProvider for WingetProvider {
    fn name(&self) -> &'static str {
        "winget"
    }

    fn query_installed(&self, executor: &dyn Executor) -> Result<InstalledPackages> {
        let staging = crate::infra::fs::TempGuard::create_unique_dir_with_mode(
            &std::env::temp_dir(),
            "dotfiles-winget",
            "inventory",
            0o700,
        )
        .context("creating temporary winget inventory directory")?;
        let output = staging.path().join("packages.json");
        executor
            .execute(
                CommandSpec::new("winget")
                    .arg("export")
                    .arg("--output")
                    .arg(&output)
                    .args(&["--accept-source-agreements", "--disable-interactivity"]),
            )
            .context("querying installed winget packages")?;
        let json = std::fs::read_to_string(&output).context("reading winget export inventory")?;
        parse_winget_ids(&json).map(|ids| InstalledPackages::new(PackageManager::Winget, ids))
    }

    /// Install `name`, preferring a per-user installer.
    ///
    /// Trying `--scope user` first means the common case installs without any
    /// UAC prompt at all. Packages that only ship a machine-scope installer
    /// report `NO_APPLICABLE_INSTALLER`, and are retried unscoped so winget can
    /// raise its own elevation prompt for just that package.
    fn install(&self, name: &str, executor: &dyn Executor) -> Result<ResourceChange> {
        self.install_with_reboot_status(name, executor)
            .map(|(change, _)| change)
    }

    fn install_with_reboot_status(
        &self,
        name: &str,
        executor: &dyn Executor,
    ) -> Result<(ResourceChange, bool)> {
        let mut result = executor.execute(
            CommandSpec::new("winget")
                .args(&install_args(name, Some("user")))
                .unchecked(),
        )?;

        if !result.success && classify_install(result.code) == InstallVerdict::RetryWithoutScope {
            result = executor.execute(
                CommandSpec::new("winget")
                    .args(&install_args(name, None))
                    .unchecked(),
            )?;
        }

        if result.success {
            return Ok((ResourceChange::Applied, false));
        }

        match classify_install(result.code) {
            InstallVerdict::PendingReboot => Ok((ResourceChange::Applied, true)),
            InstallVerdict::AlreadyInstalled => Ok((ResourceChange::AlreadyCorrect, false)),
            InstallVerdict::Skipped(reason) => {
                Ok((ResourceChange::unusable(format!("{name}: {reason}")), false))
            }
            InstallVerdict::RetryWithoutScope | InstallVerdict::Failed => {
                let status = match result.code {
                    Some(code) if code < 0 => format!("exit {code}, {code:#010X}"),
                    Some(code) => format!("exit {code}"),
                    None => "no exit code".to_string(),
                };
                let reason = format!("winget install failed ({status})");
                let reason = match (result.stdout.trim(), result.stderr.trim()) {
                    ("", "") => reason,
                    ("", text) | (text, "") => format!("{reason}: {text}"),
                    (stdout, stderr) => format!("{reason}: {stdout}\n{stderr}"),
                };
                Ok((ResourceChange::unusable(reason), false))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::exec::{ExecError, ExecResult, MockExecutor};

    fn assert_install_command(spec: &CommandSpec, scoped: bool) {
        let mut expected = vec![
            "install",
            "--id",
            "Git.Git",
            "--exact",
            "--no-upgrade",
            "--source",
            "winget",
            "--accept-source-agreements",
            "--accept-package-agreements",
            "--disable-interactivity",
        ];
        if scoped {
            expected.extend(["--scope", "user"]);
        }
        assert_eq!(spec.program(), "winget");
        assert_eq!(spec.arguments(), expected);
        assert!(
            !spec.is_checked(),
            "HRESULTs are classified by the provider"
        );
        assert_eq!(spec.working_dir(), None);
    }

    fn install_with_result(result: ExecResult) -> ResourceChange {
        install_with_results([result])
    }

    fn install_with_results(results: impl IntoIterator<Item = ExecResult>) -> ResourceChange {
        let mut mock = MockExecutor::new();
        let mut sequence = mockall::Sequence::new();
        for (index, result) in results.into_iter().enumerate() {
            mock.expect_execute()
                .once()
                .in_sequence(&mut sequence)
                .return_once(move |spec| {
                    assert_install_command(&spec, index == 0);
                    Ok(result)
                });
        }
        WingetProvider.install("Git.Git", &mock).unwrap()
    }

    #[test]
    fn export_inventory_preserves_full_ids_across_sources_and_optional_fields() {
        let json = r#"{
            "Sources": [
                {"Packages": [
                    {"PackageIdentifier": "Microsoft.VisualStudioCode.Insiders", "Version": "1.0"},
                    {"PackageIdentifier": "Git.Git"}
                ], "SourceDetails": {"Name": "winget"}},
                {"Packages": [{"PackageIdentifier": "Git.Git"}]}
            ]
        }"#;
        assert_eq!(
            parse_winget_ids(json).unwrap(),
            HashSet::from([
                "Microsoft.VisualStudioCode.Insiders".into(),
                "Git.Git".into()
            ])
        );
        assert_eq!(
            parse_winget_ids(&format!("\u{feff}{json}")).unwrap(),
            parse_winget_ids(json).unwrap()
        );
        for empty in [r#"{"Sources":[]}"#, r#"{"Sources":[{"Packages":[]}]}"#] {
            assert!(parse_winget_ids(empty).unwrap().is_empty());
        }
    }

    #[test]
    fn export_inventory_rejects_incomplete_or_invalid_inventory() {
        for json in [
            "",
            "not json",
            "{}",
            r#"{"Sources":null}"#,
            r#"{"Sources":[{}]}"#,
            r#"{"Sources":[{"Packages":[{}]}]}"#,
            r#"{"Sources":[{"Packages":[{"PackageIdentifier":""}]}]}"#,
        ] {
            assert!(parse_winget_ids(json).is_err(), "{json}");
        }
    }

    #[test]
    fn winget_inventory_ignores_localized_console_output_and_cleans_export() {
        let output_path = std::sync::Arc::new(std::sync::Mutex::new(None));
        let captured = std::sync::Arc::clone(&output_path);
        let mut mock = MockExecutor::new();
        mock.expect_execute().once().returning(move |spec| {
            mock_inventory(&spec, &["Git.Git"]);
            *captured.lock().unwrap() = Some(std::path::PathBuf::from(&spec.arguments()[2]));
            Ok(ExecResult::success("Имя  ИД  Версия\nЭкспорт завершён"))
        });

        let ids = WingetProvider.query_installed(&mock).unwrap();

        assert_eq!(
            ids,
            InstalledPackages::new(PackageManager::Winget, ["Git.Git".into()].into())
        );
        assert!(
            !output_path
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .parent()
                .unwrap()
                .exists()
        );
    }

    #[test]
    fn winget_export_success_without_output_is_not_empty_inventory() {
        let mut mock = MockExecutor::new();
        mock.expect_execute()
            .once()
            .returning(|_| Ok(ExecResult::success("")));
        let error = WingetProvider.query_installed(&mock).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("reading winget export inventory")
        );
    }

    #[test]
    fn winget_unrecognized_exit_preserves_diagnostics_without_retrying() {
        for (code, status) in [
            (Some(73), "exit 73"),
            (Some(-1), "exit -1, 0xFFFFFFFF"),
            (None, "no exit code"),
        ] {
            for (stdout, stderr, detail) in [
                ("", "", ""),
                (" No package found \n", "", ": No package found"),
                (
                    " Installer output \n",
                    " Access denied \n",
                    ": Installer output\nAccess denied",
                ),
            ] {
                assert_eq!(
                    install_with_result(ExecResult::failure(stdout, stderr, code)),
                    ResourceChange::unusable(format!("winget install failed ({status}){detail}")),
                    "exit={code:?}, stderr={stderr:?}",
                );
            }
        }
    }

    #[test]
    fn winget_install_prefers_user_scope() {
        assert_eq!(
            install_with_result(ExecResult::success("")),
            ResourceChange::Applied,
        );
    }

    #[test]
    fn winget_scope_retry_preserves_the_final_installer_outcome() {
        for (result, expected) in [
            (ExecResult::success(""), ResourceChange::Applied),
            (
                ExecResult::failure(
                    "No available upgrade found. No newer package versions are available.",
                    "",
                    Some(-1_978_335_189),
                ),
                ResourceChange::AlreadyCorrect,
            ),
            (
                ExecResult::failure(
                    "",
                    "no applicable installer",
                    Some(exit_code::NO_APPLICABLE_INSTALLER),
                ),
                ResourceChange::unusable(format!(
                    "winget install failed (exit {}, 0x8A150010): no applicable installer",
                    exit_code::NO_APPLICABLE_INSTALLER
                )),
            ),
        ] {
            assert_eq!(
                install_with_results([
                    ExecResult::failure(
                        "No user-scope installer",
                        "no applicable installer",
                        Some(exit_code::NO_APPLICABLE_INSTALLER)
                    ),
                    result,
                ]),
                expected,
            );
        }
    }

    #[test]
    fn winget_scope_retry_reports_elevation_requirement_as_unmet_work() {
        let ResourceChange::Skipped { reason, kind } = install_with_results([
            ExecResult::failure("", "", Some(exit_code::NO_APPLICABLE_INSTALLER)),
            ExecResult::failure("", "", Some(exit_code::COMMAND_REQUIRES_ADMIN)),
        ]) else {
            panic!("expected a skip");
        };
        assert!(
            kind.is_failure(),
            "an admin-required skip must count as a failure"
        );
        assert!(
            reason.starts_with("Git.Git: requires administrator"),
            "{reason}"
        );
        assert!(reason.contains("--only packages"), "{reason}");
    }

    #[test]
    fn winget_install_preserves_already_installed_as_already_correct() {
        for code in [
            exit_code::PACKAGE_ALREADY_INSTALLED,
            exit_code::INSTALL_ALREADY_INSTALLED,
            exit_code::UPDATE_NOT_APPLICABLE,
        ] {
            assert_eq!(
                install_with_result(ExecResult::failure("", "", Some(code))),
                ResourceChange::AlreadyCorrect,
                "exit code {code} should not inflate the applied count",
            );
        }
    }

    #[test]
    fn winget_distinguishes_a_pending_reboot_from_completed_installation() {
        for (result, pending) in [
            (ExecResult::success(""), false),
            (
                ExecResult::failure("", "", Some(exit_code::INSTALL_REBOOT_REQUIRED_TO_FINISH)),
                true,
            ),
        ] {
            let mut mock = MockExecutor::new();
            mock.expect_execute().once().return_once(move |spec| {
                assert_install_command(&spec, true);
                Ok(result)
            });
            let resource = crate::domains::packages::resources::package::PackageResource::new(
                "Git.Git".into(),
                PackageManager::Winget,
                std::sync::Arc::new(MockExecutor::new()),
            );
            let report = WingetProvider
                .install_missing(&[&resource], &mock, &|_| {})
                .unwrap();
            assert_eq!(report.applied_count(), 1);
            let expected: &[&str] = if pending { &["Git.Git"] } else { &[] };
            assert_eq!(report.pending_reboot(), expected);
        }
    }

    #[test]
    fn winget_declined_elevation_is_unmet_work() {
        assert_eq!(
            install_with_result(ExecResult::failure(
                "",
                "",
                Some(exit_code::INSTALL_CANCELLED_BY_USER),
            )),
            ResourceChange::unusable("Git.Git: installer elevation was declined"),
        );
    }

    #[test]
    fn winget_install_preserves_transport_errors_without_retrying() {
        for retry in [false, true] {
            for (label, error) in [
                (
                    "spawn",
                    ExecError::spawn("winget", std::io::Error::other("fixture spawn failure")),
                ),
                (
                    "cancelled",
                    ExecError::Cancelled {
                        command: "winget".into(),
                        result: ExecResult::failure("partial installer output", "", None),
                    },
                ),
                (
                    "timeout",
                    ExecError::TimedOut {
                        command: "winget".into(),
                        timeout: std::time::Duration::from_secs(30),
                        result: ExecResult::failure("partial installer output", "", None),
                    },
                ),
            ] {
                let expected = error.to_string();
                let expected_kind = std::mem::discriminant(&error);
                let mut mock = MockExecutor::new();
                let mut sequence = mockall::Sequence::new();
                if retry {
                    mock.expect_execute()
                        .once()
                        .in_sequence(&mut sequence)
                        .returning(|spec| {
                            assert_install_command(&spec, true);
                            Ok(ExecResult::failure(
                                "",
                                "",
                                Some(exit_code::NO_APPLICABLE_INSTALLER),
                            ))
                        });
                }
                mock.expect_execute()
                    .once()
                    .in_sequence(&mut sequence)
                    .return_once(move |spec| {
                        assert_install_command(&spec, !retry);
                        Err(error)
                    });
                let observed = WingetProvider.install("Git.Git", &mock).unwrap_err();
                assert_eq!(observed.to_string(), expected, "{label}, retry={retry}");
                assert_eq!(
                    std::mem::discriminant(observed.downcast_ref::<ExecError>().unwrap()),
                    expected_kind,
                    "{label}, retry={retry}",
                );
            }
        }
    }

    #[test]
    fn winget_policy_block_is_unmet_work_not_an_applied_install() {
        assert_eq!(
            install_with_result(ExecResult::failure(
                "",
                "",
                Some(exit_code::INSTALL_BLOCKED_BY_POLICY),
            )),
            ResourceChange::unusable("Git.Git: blocked by system policy"),
        );
    }
}
