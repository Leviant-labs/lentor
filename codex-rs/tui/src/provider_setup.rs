use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::BottomPaneView;
use crate::bottom_pane::ViewCompletion;
use crate::render::renderable::Renderable;
use codex_app_server_protocol::ModelProviderConfigureParams;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

const MAX_INPUT_BYTES: usize = 4096;

#[derive(Clone)]
pub(crate) struct ProviderSetupForm {
    endpoint: String,
    api_key: String,
    field: Field,
    error: Option<String>,
}

#[derive(Clone, Copy)]
enum Field {
    Endpoint,
    ApiKey,
}

pub(crate) enum ProviderSetupAction {
    Cancel,
    Save(ModelProviderConfigureParams),
}

impl ProviderSetupForm {
    pub(crate) fn new(endpoint: String) -> Self {
        Self { endpoint, api_key: String::new(), field: Field::Endpoint, error: None }
    }

    pub(crate) fn paste(&mut self, text: &str) {
        let input = match self.field { Field::Endpoint => &mut self.endpoint, Field::ApiKey => &mut self.api_key };
        for ch in text.chars().filter(|ch| !ch.is_control()) {
            if input.len() + ch.len_utf8() > MAX_INPUT_BYTES { break; }
            input.push(ch);
        }
    }

    pub(crate) fn key(&mut self, event: KeyEvent) -> Option<ProviderSetupAction> {
        if event.kind == KeyEventKind::Release { return None; }
        self.error = None;
        match event.code {
            KeyCode::Esc => return Some(ProviderSetupAction::Cancel),
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => {
                self.field = match self.field { Field::Endpoint => Field::ApiKey, Field::ApiKey => Field::Endpoint };
            }
            KeyCode::Enter if matches!(self.field, Field::Endpoint) => self.field = Field::ApiKey,
            KeyCode::Enter => {
                if self.endpoint.trim().is_empty() || self.api_key.trim().is_empty() {
                    self.error = Some("Enter an API endpoint and API key.".to_string());
                } else {
                    return Some(ProviderSetupAction::Save(ModelProviderConfigureParams {
                        provider_id: "custom-api".to_string(),
                        name: "Custom API".to_string(),
                        base_url: self.endpoint.trim().to_string(),
                        api_key: self.api_key.trim().into(),
                        model: None,
                    }));
                }
            }
            KeyCode::Backspace => {
                match self.field { Field::Endpoint => &mut self.endpoint, Field::ApiKey => &mut self.api_key }.pop();
            }
            KeyCode::Char('u') if event.modifiers.contains(KeyModifiers::CONTROL) => {
                match self.field { Field::Endpoint => &mut self.endpoint, Field::ApiKey => &mut self.api_key }.clear();
            }
            KeyCode::Char(ch) if !event.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                let mut utf8 = [0; 4];
                self.paste(ch.encode_utf8(&mut utf8));
            }
            _ => {}
        }
        None
    }
}

impl Renderable for ProviderSetupForm {
    fn desired_height(&self, _width: u16) -> u16 { 11 }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let endpoint_marker = if matches!(self.field, Field::Endpoint) { "> " } else { "  " };
        let key_marker = if matches!(self.field, Field::ApiKey) { "> " } else { "  " };
        let endpoint = if self.endpoint.is_empty() { "https://provider.example/v1" } else { &self.endpoint };
        let key = if self.api_key.is_empty() { "(enter key)".to_string() } else { "•".repeat(self.api_key.chars().count().min(48)) };
        let mut lines: Vec<Line<'_>> = vec![
            "Connect an API provider".bold().into(),
            "".into(),
            format!("{endpoint_marker}API base URL").cyan().into(),
            format!("  {endpoint}").into(),
            "".into(),
            format!("{key_marker}API key").cyan().into(),
            format!("  {key}").into(),
            "".into(),
        ];
        if let Some(error) = &self.error {
            lines.extend(textwrap::wrap(error, usize::from(area.width.max(1))).into_iter().map(|line| Line::from(line.into_owned().red())));
        } else {
            lines.push("Keys are saved in your system credential store.".dim().into());
        }
        lines.push("Tab switch field · Enter continue/save · Esc cancel".dim().into());
        Paragraph::new(lines).render(area, buf);
    }
}

pub(crate) struct ProviderSetupView {
    form: ProviderSetupForm,
    events: AppEventSender,
    completion: Option<ViewCompletion>,
}

impl ProviderSetupView {
    pub(crate) fn new(endpoint: String, events: AppEventSender) -> Self {
        Self { form: ProviderSetupForm::new(endpoint), events, completion: None }
    }
}

impl Renderable for ProviderSetupView {
    fn desired_height(&self, width: u16) -> u16 { self.form.desired_height(width) }
    fn render(&self, area: Rect, buf: &mut Buffer) { self.form.render(area, buf); }
}

impl BottomPaneView for ProviderSetupView {
    fn handle_key_event(&mut self, event: KeyEvent) {
        match self.form.key(event) {
            Some(ProviderSetupAction::Cancel) => self.completion = Some(ViewCompletion::Cancelled),
            Some(ProviderSetupAction::Save(params)) => {
                self.events.send(AppEvent::ConfigureModelProvider(params));
                self.completion = Some(ViewCompletion::Accepted);
            }
            None => {}
        }
    }
    fn handle_paste(&mut self, pasted: String) -> bool { self.form.paste(&pasted); true }
    fn is_complete(&self) -> bool { self.completion.is_some() }
    fn completion(&self) -> Option<ViewCompletion> { self.completion }
}

#[cfg(test)]
#[path = "provider_setup_tests.rs"]
mod tests;
