//! Managed-install identity is isolated from inherited Codex metadata.

use super::Fixture;
use anyhow::Result;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

const MANAGERS: [&str; 4] = ["NPM", "BUN", "PNPM", "VITE_PLUS"];

#[derive(Default)]
struct ManagedEnvironment<'a> {
    codex_manager: Option<&'a str>,
    lentor_manager: Option<&'a str>,
    codex_root: Option<&'a str>,
    lentor_root: Option<&'a str>,
}

fn installation(fixture: &Fixture, environment: ManagedEnvironment<'_>) -> Result<Value> {
    let mut command = fixture.command()?;
    for family in ["CODEX", "LENTOR"] {
        for name in MANAGERS {
            command.env_remove(format!("{family}_MANAGED_BY_{name}"));
        }
        command.env_remove(format!("{family}_MANAGED_PACKAGE_ROOT"));
    }
    for (family, manager, root) in [
        ("CODEX", environment.codex_manager, environment.codex_root),
        ("LENTOR", environment.lentor_manager, environment.lentor_root),
    ] {
        if let Some(manager) = manager {
            command.env(format!("{family}_MANAGED_BY_{manager}"), "1");
        }
        if let Some(root) = root {
            command.env(format!("{family}_MANAGED_PACKAGE_ROOT"), root);
        }
    }
    let output = command.args(["doctor", "--json"]).output()?;
    let report: Value = serde_json::from_slice(&output.stdout)?;
    Ok(report["checks"]["installation"].clone())
}

fn identity(check: &Value) -> Value {
    json!({
        "context":check["details"]["install context"],
        "root":check["details"]["managed package root"],
        "status":check["status"],
        "managers":{
            "managed by npm":check["details"]["managed by npm"],
            "managed by bun":check["details"]["managed by bun"],
            "managed by pnpm":check["details"]["managed by pnpm"],
            "managed by Vite+":check["details"]["managed by Vite+"],
        }
    })
}

fn check_manager(name: &str, method: &str, detail: &str, old_manager: &str) -> Result<()> {
    let fixture = Fixture::new()?;
    let mut expected = json!({
        "context":"other","root":"not set","status":"ok",
        "managers":{
            "managed by npm":"false","managed by bun":"false",
            "managed by pnpm":"false","managed by Vite+":"false"
        }
    });
    let old_only = installation(&fixture, ManagedEnvironment {
        codex_manager: Some(name), codex_root: Some("old-root"),
        ..Default::default()
    })?;
    assert_eq!(identity(&old_only), expected);
    expected["context"] = method.into();
    expected["root"] = "new-root".into();
    expected["managers"][detail] = "true".into();
    let new_only = installation(&fixture, ManagedEnvironment {
        lentor_manager: Some(name), lentor_root: Some("new-root"),
        ..Default::default()
    })?;
    assert_eq!(identity(&new_only), expected);
    let conflict = installation(&fixture, ManagedEnvironment {
        codex_manager: Some(old_manager), lentor_manager: Some(name),
        codex_root: Some("old-root"), lentor_root: Some("new-root"),
    })?;
    assert_eq!(identity(&conflict), expected);
    Ok(())
}

#[test]
fn npm_environment_is_independent() -> Result<()> {
    check_manager("NPM", "npm", "managed by npm", "BUN")
}

#[test]
fn bun_environment_is_independent() -> Result<()> {
    check_manager("BUN", "bun", "managed by bun", "PNPM")
}

#[test]
fn pnpm_environment_is_independent() -> Result<()> {
    check_manager("PNPM", "pnpm", "managed by pnpm", "VITE_PLUS")
}

#[test]
fn vite_environment_is_independent() -> Result<()> {
    check_manager("VITE_PLUS", "vite+", "managed by Vite+", "NPM")
}

#[test]
fn npm_missing_root_uses_only_lentor_provenance() -> Result<()> {
    let fixture = Fixture::new()?;
    let old_only = installation(&fixture, ManagedEnvironment {
        codex_manager: Some("NPM"), ..Default::default()
    })?;
    assert_eq!(old_only["status"], "ok");
    let new_only = installation(&fixture, ManagedEnvironment {
        lentor_manager: Some("NPM"), ..Default::default()
    })?;
    let conflict = installation(&fixture, ManagedEnvironment {
        codex_manager: Some("BUN"), lentor_manager: Some("NPM"),
        codex_root: Some("old-root"), ..Default::default()
    })?;
    assert_eq!(identity(&conflict), identity(&new_only));
    let mut selected = json!({
        "status":conflict["status"],
        "summary":conflict["summary"],
        "remediation":conflict["remediation"],
    });
    selected.sort_all_objects();
    insta::assert_snapshot!(serde_json::to_string_pretty(&selected)?, @r###"
    {
      "remediation": "Ensure the Lentor launcher provides LENTOR_MANAGED_PACKAGE_ROOT, then rerun lentor doctor.",
      "status": "warning",
      "summary": "npm-managed launch is missing package-root provenance"
    }
    "###);
    Ok(())
}

