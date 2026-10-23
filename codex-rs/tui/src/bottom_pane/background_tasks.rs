//! Live task inspector. Direct server requests refresh this view without model turns.
use super::BottomPaneView;
use super::CancellationEvent;
use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::render::renderable::Renderable;
use codex_protocol::ThreadId;
use codex_protocol::background_tasks::BackgroundTaskInfo;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;
use std::sync::Arc;
use std::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[derive(Default)]
pub(crate) struct TaskViewState {
    pub tasks: Vec<BackgroundTaskInfo>,
    pub error: Option<String>,
}

pub(crate) struct BackgroundTasksView {
    pub state: Arc<Mutex<TaskViewState>>,
    pub cancel: CancellationToken,
    pub events: AppEventSender,
    pub thread_id: ThreadId,
    selected: usize,
    selected_id: Option<String>,
    detail: bool,
    complete: bool,
}

impl BackgroundTasksView {
    pub(crate) fn new(
        state: Arc<Mutex<TaskViewState>>,
        cancel: CancellationToken,
        events: AppEventSender,
        thread_id: ThreadId,
    ) -> Self {
        Self {
            state,
            cancel,
            events,
            thread_id,
            selected: 0,
            selected_id: None,
            detail: false,
            complete: false,
        }
    }
}
impl Drop for BackgroundTasksView {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
impl BottomPaneView for BackgroundTasksView {
    fn is_complete(&self) -> bool {
        self.complete
    }
    fn prefer_esc_to_handle_key_event(&self) -> bool {
        true
    }
    fn on_ctrl_c(&mut self) -> CancellationEvent {
        self.complete = true;
        self.cancel.cancel();
        CancellationEvent::Handled
    }
    fn pre_draw_tick(&mut self, _now: std::time::Instant) -> bool {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(index) = state
            .tasks
            .iter()
            .position(|t| Some(&t.id) == self.selected_id.as_ref())
        {
            self.selected = index;
        }
        self.selected = self.selected.min(state.tasks.len().saturating_sub(1));
        self.selected_id = state.tasks.get(self.selected).map(|t| t.id.clone());
        false
    }
    fn handle_key_event(&mut self, key: KeyEvent) {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(index) = state
            .tasks
            .iter()
            .position(|t| Some(&t.id) == self.selected_id.as_ref())
        {
            self.selected = index;
        }
        match key.code {
            KeyCode::Esc if self.detail => self.detail = false,
            KeyCode::Esc | KeyCode::Char('q') => {
                self.complete = true;
                self.cancel.cancel();
            }
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(state.tasks.len().saturating_sub(1))
            }
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Enter => self.detail = !self.detail,
            KeyCode::Char('s') => {
                if let Some(task) = state
                    .tasks
                    .get(self.selected)
                    .filter(|t| t.status == "running")
                {
                    self.events.send(AppEvent::StopBackgroundTask {
                        thread_id: self.thread_id,
                        process_id: task.process_id.clone(),
                    });
                }
            }
            _ => {}
        }
        self.selected_id = state.tasks.get(self.selected).map(|t| t.id.clone());
    }
}
impl Renderable for BackgroundTasksView {
    fn desired_height(&self, _width: u16) -> u16 {
        20
    }
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut lines = vec![Line::from(
            "Background tasks · live · no model calls".bold(),
        )];
        if let Some(error) = &state.error {
            lines.push(Line::from(error.clone().red()));
        }
        if self.detail {
            if let Some(task) = state.tasks.get(self.selected) {
                lines.push(Line::from(format!(
                    "{} · {} · {} · exit {:?}",
                    task.process_id, task.label, task.status, task.exit_code
                )));
                lines.push(Line::from(task.command.clone()));
                let available = area.height.saturating_sub(6) as usize;
                let output: Vec<_> = task.output.lines().collect();
                lines.extend(
                    output
                        .iter()
                        .skip(output.len().saturating_sub(available))
                        .map(|line| Line::from((*line).to_owned())),
                );
            }
        } else {
            if state.tasks.is_empty() {
                lines.push(Line::from(
                    "No registered tasks. Start a command with background options.",
                ));
            }
            let available = area.height.saturating_sub(3) as usize;
            let start = self.selected.saturating_sub(available.saturating_sub(1));
            for (index, task) in state.tasks.iter().enumerate().skip(start).take(available) {
                let elapsed = task
                    .completed_at
                    .unwrap_or_else(|| chrono::Utc::now().timestamp())
                    - task.started_at;
                let line = format!(
                    "{} {} · {} · {} · {}s · {}",
                    if index == self.selected { "›" } else { " " },
                    task.process_id,
                    task.kind,
                    task.status,
                    elapsed,
                    task.label
                );
                lines.push(if index == self.selected {
                    Line::from(line.cyan().bold())
                } else {
                    Line::from(line)
                });
            }
        }
        lines.push(Line::from(
            "↑↓ select · Enter output/back · s stop watcher/process · Esc close".dim(),
        ));
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn task(id: &str) -> BackgroundTaskInfo {
        BackgroundTaskInfo {
            id: id.into(),
            process_id: id.into(),
            label: format!("Build {id}"),
            kind: "shell".into(),
            status: "running".into(),
            command: "make".into(),
            started_at: 0,
            completed_at: None,
            exit_code: None,
            output: "building".into(),
        }
    }

    #[test]
    fn background_task_selection_survives_refresh_and_stop_targets_selected_task() {
        let state = Arc::new(Mutex::new(TaskViewState {
            tasks: vec![task("1"), task("2")],
            error: None,
        }));
        let cancel = CancellationToken::new();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut view = BackgroundTasksView::new(
            Arc::clone(&state),
            cancel.clone(),
            AppEventSender::new(sender),
            ThreadId::new(),
        );
        view.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        state.lock().unwrap().tasks.insert(0, task("3"));
        view.pre_draw_tick(std::time::Instant::now());
        view.handle_key_event(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert!(
            matches!(receiver.try_recv().unwrap(), AppEvent::StopBackgroundTask { process_id, .. } if process_id == "2")
        );
        view.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(view.detail);
        view.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!view.is_complete());
        view.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(view.is_complete());
        assert!(cancel.is_cancelled());
    }
}
