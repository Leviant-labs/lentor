use anyhow::Result;
use predicates::str::contains;
use std::path::Path;
use tempfile::TempDir;

fn codex_command(codex_home: &Path) -> Result<assert_cmd::Command> {
    let mut cmd = assert_cmd::Command::new(codex_utils_cargo_bin::cargo_bin("lentor")?);
    cmd.env("LENTOR_HOME", codex_home);
    cmd.current_dir(codex_home);
    Ok(cmd)
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn update_does_not_start_interactive_prompt() -> Result<()> {
    let codex_home = TempDir::new()?;

    let assertion = codex_command(codex_home.path())?
        .arg("--no-daemon")
        .arg("update")
        .assert()
        .failure()
        .stderr(contains("`lentor update` is not available in debug builds"));

    insta::assert_snapshot!(
        "lentor_update_unavailable_debug",
        String::from_utf8_lossy(&assertion.get_output().stderr)
    );
    Ok(())
}
