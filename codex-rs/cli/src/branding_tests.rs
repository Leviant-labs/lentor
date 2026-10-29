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
