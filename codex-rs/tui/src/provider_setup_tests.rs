use super::*;
use pretty_assertions::assert_eq;

fn rendered(form: &ProviderSetupForm, width: u16) -> String {
    let mut buffer = Buffer::empty(Rect::new(/*x*/ 0, /*y*/ 0, width, /*height*/ 11));
    form.render(buffer.area, &mut buffer);
    buffer.content.chunks(usize::from(width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>().trim_end().to_string())
        .collect::<Vec<_>>().join("\n")
}

#[test]
fn provider_setup_masks_key_and_submits_endpoint() {
    let mut form = ProviderSetupForm::new("https://gateway.example/v1".to_string());
    form.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    form.paste("secret-provider-key");
    let display = rendered(&form, /*width*/ 70);
    assert!(!display.contains("secret-provider-key"));
    insta::assert_snapshot!("provider_setup_key_entry", display);
    let Some(ProviderSetupAction::Save(params)) = form.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) else { panic!("expected provider configuration"); };
    assert_eq!(params, ModelProviderConfigureParams {
        provider_id: "custom-api".to_string(), name: "Custom API".to_string(),
        base_url: "https://gateway.example/v1".to_string(), api_key: "secret-provider-key".into(), model: None,
    });
}

#[test]
fn provider_setup_empty_key_has_visible_error() {
    let mut form = ProviderSetupForm::new("https://gateway.example/v1".to_string());
    form.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(form.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)).is_none());
    insta::assert_snapshot!("provider_setup_missing_key", rendered(&form, /*width*/ 70));
}

#[test]
fn provider_setup_narrow_entry() {
    insta::assert_snapshot!("provider_setup_narrow", rendered(&ProviderSetupForm::new(String::new()), /*width*/ 36));
}
