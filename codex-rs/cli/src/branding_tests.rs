use super::MultitoolCli;
use clap::CommandFactory;
use clap::Parser;
use clap::error::ErrorKind;
use pretty_assertions::assert_eq;

#[test]
fn lentor_help() {
    insta::assert_snapshot!(MultitoolCli::command().render_help().to_string());
}

#[test]
fn lentor_exec_help() {
    let help = MultitoolCli::command()
        .try_get_matches_from(["lentor", "exec", "--help"])
        .expect_err("help exits before starting a session");
    assert_eq!(help.kind(), ErrorKind::DisplayHelp);
    insta::assert_snapshot!(help.to_string());
}

#[test]
fn lentor_login_help() {
    let help = MultitoolCli::command()
        .try_get_matches_from(["lentor", "login", "--help"])
        .expect_err("help exits before starting login");
    assert_eq!(help.kind(), ErrorKind::DisplayHelp);
    insta::assert_snapshot!(help.to_string());
}

#[cfg(windows)]
#[test]
fn lentor_sandbox_setup_help() {
    insta::assert_snapshot!(
        crate::sandbox_setup::SandboxSetupCommand::command()
            .bin_name("lentor sandbox setup")
            .render_help()
            .to_string()
    );
}

#[test]
fn lentor_version() {
    let result = MultitoolCli::try_parse_from(["lentor", "--version"])
        .expect_err("version flag exits after printing the version");
    assert_eq!(result.kind(), ErrorKind::DisplayVersion);
    insta::assert_snapshot!(
        result
            .to_string()
            .replace(env!("CARGO_PKG_VERSION"), "<VERSION>")
    );
}
