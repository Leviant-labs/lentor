use super::*;
use pretty_assertions::assert_eq;

fn rendered(form: &ProviderSetupForm, width: u16) -> String {
    let mut buffer = Buffer::empty(Rect::new(
        /*x*/ 0, /*y*/ 0, width, /*height*/ 11,
    ));
    form.render(buffer.area, &mut buffer);
    buffer
        .content
        .chunks(usize::from(width))
        .map(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn provider_setup_masks_key_and_submits_endpoint() {
    let mut form = ProviderSetupForm::new("https://gateway.example/v1".to_string());
    form.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    form.paste("secret-provider-key");
    let display = rendered(&form, /*width*/ 70);
    assert!(!display.contains("secret-provider-key"));
    insta::assert_snapshot!("provider_setup_key_entry", display);
    let Some(ProviderSetupAction::Save(params)) =
        form.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
    else {
        panic!("expected provider configuration");
    };
    assert_eq!(
        params,
        ModelProviderConfigureParams {
            provider_id: "custom-api".to_string(),
            name: "Custom API".to_string(),
            base_url: "https://gateway.example/v1".to_string(),
            api_key: "secret-provider-key".into(),
            model: None,
        }
    );
}

#[test]
fn provider_setup_empty_key_has_visible_error() {
    let mut form = ProviderSetupForm::new("https://gateway.example/v1".to_string());
    form.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        form.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .is_none()
    );
    insta::assert_snapshot!("provider_setup_missing_key", rendered(&form, /*width*/ 70));
}

#[test]
fn provider_setup_narrow_entry() {
    insta::assert_snapshot!(
        "provider_setup_narrow",
        rendered(&ProviderSetupForm::new(String::new()), /*width*/ 36)
    );
}

#[tokio::test]
async fn provider_setup_view_submits_redacted_configuration() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut view = ProviderSetupView::new(
        "https://gateway.example/v1".to_string(),
        AppEventSender::new(tx),
    );
    view.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    view.handle_paste("secret-provider-key".to_string());
    view.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let event = rx.try_recv().expect("configuration event");
    let AppEvent::ConfigureModelProvider(params) = event else {
        panic!("expected provider configuration event");
    };
    assert!(!format!("{params:?}").contains("secret-provider-key"));
    assert_eq!(params.base_url, "https://gateway.example/v1");
    assert!(view.is_complete());
}
