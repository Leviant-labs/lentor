use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn empty_overload_message_preserves_model_snapshot() {
    let (mut chat, mut rx, mut op_rx) = make_chatwidget_manual(Some("gpt-5.2")).await;
    chat.set_model("gpt-5.2");
    while rx.try_recv().is_ok() {}
    while op_rx.try_recv().is_ok() {}

    handle_error(&mut chat, "", Some(CodexErrorInfo::ServerOverloaded));

    let mut rendered = String::new();
    while let Ok(event) = rx.try_recv() {
        match event {
            AppEvent::InsertHistoryCell(cell) => {
                rendered.push_str(&lines_to_single_string(&cell.display_lines(/*width*/ 80)));
            }
            AppEvent::UpdateModel(model) => assert_eq!(model, "gpt-5.2"),
            _ => {}
        }
    }
    while let Ok(event) = op_rx.try_recv() {
        if let Op::OverrideTurnContext { model, .. } = event {
            assert!(model.is_none(), "overload must not change the model");
        }
    }
    assert_eq!(chat.current_model(), "gpt-5.2");
    insta::assert_snapshot!("empty_overload_message_preserves_model", rendered);
}
