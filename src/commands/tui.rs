use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::commands::report;
use crate::commands::verify::collect_verification;
use crate::evidence::browser::{load_trace_catalog, TraceCatalog, TraceSummary, TraceTab};
use crate::evidence::store::{FilesystemTraceStore, TraceStoreConfig};

#[cfg(feature = "tui")]
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
#[cfg(feature = "tui")]
use crossterm::execute;
#[cfg(feature = "tui")]
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
#[cfg(feature = "tui")]
use ratatui::backend::CrosstermBackend;
#[cfg(feature = "tui")]
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
#[cfg(feature = "tui")]
use ratatui::style::{Color, Modifier, Style};
#[cfg(feature = "tui")]
use ratatui::text::{Line, Span};
#[cfg(feature = "tui")]
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs, Wrap};
#[cfg(feature = "tui")]
use ratatui::{Frame, Terminal};

const HELP_FOOTER: &str =
    "q quit | \u{2191}/\u{2193} move | Tab switch | / filter | Enter details | v verify | r report | a archive | u restore";

#[cfg(feature = "tui")]
pub fn execute(trace_root: PathBuf) -> Result<()> {
    let mut app = TraceBrowserState::load(trace_root)?;
    app.run()
}

#[derive(Debug, Clone)]
pub struct TraceBrowserState {
    trace_root: PathBuf,
    catalog: TraceCatalog,
    tab: TraceTab,
    selected_index: usize,
    scroll_offset: usize,
    viewport_rows: usize,
    filter: String,
    filter_mode: bool,
    detail: DetailView,
    status: String,
}

#[derive(Debug, Clone)]
enum DetailView {
    Help,
    Text(String),
}

impl TraceBrowserState {
    pub fn load(trace_root: PathBuf) -> Result<Self> {
        let catalog = load_trace_catalog(&trace_root)?;

        Ok(Self {
            trace_root,
            catalog,
            tab: TraceTab::Active,
            selected_index: 0,
            scroll_offset: 0,
            viewport_rows: 0,
            filter: String::new(),
            filter_mode: false,
            detail: DetailView::Help,
            status: "Ready".to_string(),
        })
    }

    pub fn refresh(&mut self) -> Result<()> {
        self.catalog = load_trace_catalog(&self.trace_root)?;
        self.clamp_selection_and_scroll();
        Ok(())
    }

    pub fn visible_traces(&self) -> Vec<&TraceSummary> {
        self.catalog.filtered_for_tab(self.tab, &self.filter)
    }

    pub fn selected_trace(&self) -> Option<&TraceSummary> {
        self.visible_traces().get(self.selected_index).copied()
    }

    pub fn filter_text(&self) -> &str {
        &self.filter
    }

    pub fn active_tab(&self) -> TraceTab {
        self.tab
    }

    pub fn selection(&self) -> usize {
        self.selected_index
    }

    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    pub fn local_selected_index(&self) -> Option<usize> {
        if self.visible_traces().is_empty() {
            None
        } else {
            Some(self.selected_index.saturating_sub(self.scroll_offset))
        }
    }

    pub fn set_viewport_rows(&mut self, rows: usize) {
        self.viewport_rows = rows;
        self.clamp_selection_and_scroll();
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn status_text(&self) -> String {
        format!("{} | {}", self.filter_line(), self.status_line())
    }

    pub fn keys_text(&self) -> &'static str {
        HELP_FOOTER
    }

    pub fn footer_text(&self) -> String {
        format!("{}\n{}", self.status_text(), self.keys_text())
    }

    pub fn empty_state_message(&self) -> Option<String> {
        if !self.visible_traces().is_empty() {
            return None;
        }

        if self.filter.trim().is_empty() {
            Some(match self.tab {
                TraceTab::Active => "No active traces found".to_string(),
                TraceTab::Archived => "No archived traces found".to_string(),
            })
        } else {
            Some("No traces match filter".to_string())
        }
    }

    pub fn detail_text(&self) -> Option<&str> {
        match &self.detail {
            DetailView::Help => None,
            DetailView::Text(text) => Some(text.as_str()),
        }
    }

    pub fn set_filter(&mut self, filter: impl Into<String>) {
        self.filter = filter.into();
        self.clamp_selection_and_scroll();
    }

    pub fn switch_tab(&mut self) {
        self.tab = self.tab.toggle();
        self.detail = DetailView::Help;
        self.clamp_selection_and_scroll();
    }

    pub fn move_selection(&mut self, offset: isize) {
        let len = self.visible_traces().len();
        if len == 0 {
            self.selected_index = 0;
            self.scroll_offset = 0;
            return;
        }

        let len_isize = len as isize;
        let mut next = self.selected_index as isize + offset;
        if next < 0 {
            next = 0;
        }
        if next >= len_isize {
            next = len_isize - 1;
        }
        self.selected_index = next as usize;
        self.clamp_selection_and_scroll();
    }

    pub fn inspect_selected(&mut self) -> Result<()> {
        let summary = self
            .selected_trace()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no trace selected"))?;
        self.detail = DetailView::Text(build_summary_text(&summary));
        self.status = format!("Inspected {}", summary.trace_id);
        Ok(())
    }

    pub fn verify_selected(&mut self) -> Result<()> {
        let summary = self
            .selected_trace()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no trace selected"))?;
        let store = FilesystemTraceStore::new(TraceStoreConfig::new(&self.trace_root));
        let verification = collect_verification(&store, &summary.trace_id)?;
        self.detail = DetailView::Text(verification.render_text());
        self.status = if verification.all_ok() {
            "Verification: OK".to_string()
        } else {
            format!(
                "Verification failed: {}",
                verification
                    .first_failure_reason()
                    .unwrap_or_else(|| "verification failed".to_string())
            )
        };
        Ok(())
    }

    pub fn report_selected(&mut self) -> Result<()> {
        let summary = self
            .selected_trace()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no trace selected"))?;
        let store = FilesystemTraceStore::new(TraceStoreConfig::new(&self.trace_root));
        let result = (|| -> Result<(PathBuf, String)> {
            let resolved = store.resolve_trace(&summary.trace_id)?;
            let manifest = store.load_manifest_at(&resolved.paths)?;
            let report_path = resolved.paths.root.join("report.md");
            let report_text =
                report::build_report(&resolved.paths.root, &manifest, &resolved.paths)?;

            fs::write(&report_path, report_text)
                .with_context(|| format!("failed to write {}", report_path.display()))?;

            let rendered = fs::read_to_string(&report_path)
                .with_context(|| format!("failed to read {}", report_path.display()))?;

            Ok((report_path, rendered))
        })();

        match result {
            Ok((report_path, rendered)) => {
                self.detail = DetailView::Text(rendered);
                self.status = format!("Report written: {}", report_path.display());
                Ok(())
            }
            Err(err) => {
                self.detail = DetailView::Text(format!("Error: {err}"));
                self.status = format!("Error: {err}");
                Err(err)
            }
        }
    }

    pub fn archive_selected(&mut self) -> Result<()> {
        let trace_id = self
            .selected_trace()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no trace selected"))?
            .trace_id
            .clone();

        let store = FilesystemTraceStore::new(TraceStoreConfig::new(&self.trace_root));
        match store.archive_trace(&trace_id) {
            Ok(_) => {
                self.refresh()?;
                self.status = format!("Archived trace: {trace_id}");
                Ok(())
            }
            Err(err) => {
                self.status = format!("Error: {err}");
                Err(err)
            }
        }
    }

    pub fn restore_selected(&mut self) -> Result<()> {
        let trace_id = self
            .selected_trace()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no trace selected"))?
            .trace_id
            .clone();

        let store = FilesystemTraceStore::new(TraceStoreConfig::new(&self.trace_root));
        match store.restore_trace(&trace_id) {
            Ok(_) => {
                self.refresh()?;
                self.status = format!("Restored trace: {trace_id}");
                Ok(())
            }
            Err(err) => {
                self.status = format!("Error: {err}");
                Err(err)
            }
        }
    }

    pub fn push_filter_char(&mut self, ch: char) {
        self.filter.push(ch);
        self.clamp_selection_and_scroll();
    }

    pub fn pop_filter_char(&mut self) {
        self.filter.pop();
        self.clamp_selection_and_scroll();
    }

    pub fn enter_filter_mode(&mut self) {
        self.filter_mode = true;
        self.status = "Editing filter".to_string();
    }

    pub fn exit_filter_mode(&mut self) {
        self.filter_mode = false;
        self.clamp_selection_and_scroll();
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Result<bool> {
        if self.filter_mode {
            return self.handle_filter_key(key);
        }

        match key.code {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Char('j') | KeyCode::Down => self.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_selection(-1),
            KeyCode::Tab => self.switch_tab(),
            KeyCode::Enter => self.inspect_selected()?,
            KeyCode::Char('v') => self.verify_selected()?,
            KeyCode::Char('r') => self.report_selected()?,
            KeyCode::Char('a') => {
                if let Some(trace) = self.selected_trace() {
                    if !trace.archived {
                        self.archive_selected()?;
                    } else {
                        self.status = "Error: selected trace is archived".to_string();
                    }
                }
            }
            KeyCode::Char('u') => {
                if let Some(trace) = self.selected_trace() {
                    if trace.archived {
                        self.restore_selected()?;
                    } else {
                        self.status = "Error: selected trace is active".to_string();
                    }
                }
            }
            KeyCode::Char('/') => self.enter_filter_mode(),
            _ => {}
        }

        Ok(false)
    }

    fn handle_filter_key(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                self.exit_filter_mode();
                self.status = "Filter cancelled".to_string();
            }
            KeyCode::Enter => {
                self.exit_filter_mode();
                self.status = format!("Filter applied: {}", self.filter);
            }
            KeyCode::Backspace => {
                self.pop_filter_char();
            }
            KeyCode::Char(c) => {
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT {
                    self.push_filter_char(c);
                }
            }
            _ => {}
        }

        Ok(false)
    }

    fn clamp_selection_and_scroll(&mut self) {
        let len = self.visible_traces().len();
        if len == 0 {
            self.selected_index = 0;
            self.scroll_offset = 0;
            return;
        }

        if self.selected_index >= len {
            self.selected_index = len - 1;
        }

        let viewport_rows = self.viewport_rows.max(1).min(len);
        let max_offset = len.saturating_sub(viewport_rows);

        if self.scroll_offset > max_offset {
            self.scroll_offset = max_offset;
        }

        if self.selected_index < self.scroll_offset {
            self.scroll_offset = self.selected_index;
        } else if self.selected_index >= self.scroll_offset + viewport_rows {
            self.scroll_offset = self.selected_index + 1 - viewport_rows;
        }

        if self.scroll_offset > max_offset {
            self.scroll_offset = max_offset;
        }
    }

    #[cfg(feature = "tui")]
    pub fn run(&mut self) -> Result<()> {
        enable_raw_mode().context("tracebox tui requires an interactive terminal")?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)
            .context("tracebox tui requires an interactive terminal")?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let run_result = self.event_loop(&mut terminal);

        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        run_result
    }

    #[cfg(feature = "tui")]
    fn event_loop(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        loop {
            terminal.draw(|frame| self.draw(frame))?;

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if self.handle_key(key)? {
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    #[cfg(feature = "tui")]
    fn draw(&mut self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(10),
                Constraint::Min(6),
                Constraint::Length(4),
            ])
            .split(frame.area());

        let tab_titles = ["Active", "Archived"]
            .into_iter()
            .map(|title| Line::from(Span::styled(title, Style::default().fg(Color::Cyan))))
            .collect::<Vec<_>>();

        let tabs = Tabs::new(tab_titles)
            .select(match self.tab {
                TraceTab::Active => 0,
                TraceTab::Archived => 1,
            })
            .block(Block::default().borders(Borders::ALL).title("Tracebox"))
            .highlight_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );
        frame.render_widget(tabs, chunks[0]);

        self.viewport_rows = chunks[1].height.saturating_sub(3) as usize;
        self.clamp_selection_and_scroll();

        let traces = self.visible_traces();

        if let Some(message) = self.empty_state_message() {
            let empty = Paragraph::new(message)
                .alignment(Alignment::Center)
                .block(Block::default().borders(Borders::ALL).title("Traces"))
                .wrap(Wrap { trim: true });
            frame.render_widget(empty, chunks[1]);
        } else {
            let header = Row::new(vec![
                Cell::from("TRACE ID"),
                Cell::from("STATUS"),
                Cell::from("EXIT"),
                Cell::from("DURATION"),
                Cell::from("COMMAND"),
            ])
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            );

            let local_selected = self
                .local_selected_index()
                .unwrap_or(0)
                .min(self.viewport_rows.saturating_sub(1));

            let rows = traces
                .iter()
                .skip(self.scroll_offset)
                .take(self.viewport_rows)
                .enumerate()
                .map(|(idx, trace)| {
                    let style = if idx == local_selected {
                        Style::default().fg(Color::Black).bg(Color::Yellow)
                    } else {
                        Style::default()
                    };

                    Row::new(vec![
                        Cell::from(trace.trace_id.clone()),
                        Cell::from(if trace.archived { "ARCHIVED" } else { "ACTIVE" }),
                        Cell::from(
                            trace
                                .exit_code
                                .map(|code| code.to_string())
                                .unwrap_or_else(|| "-".to_string()),
                        ),
                        Cell::from(format!("{}ms", trace.duration_ms)),
                        Cell::from(trace.command.clone()),
                    ])
                    .style(style)
                });

            let table = Table::new(
                rows,
                [
                    Constraint::Length(42),
                    Constraint::Length(10),
                    Constraint::Length(8),
                    Constraint::Length(10),
                    Constraint::Min(20),
                ],
            )
            .header(header)
            .block(Block::default().borders(Borders::ALL).title("Traces"))
            .column_spacing(1);

            frame.render_widget(table, chunks[1]);
        }

        let detail_text = self
            .detail_text()
            .map(|text| text.to_string())
            .unwrap_or_else(|| {
                self.selected_trace()
                    .map(build_summary_text)
                    .unwrap_or_else(|| "No trace selected".to_string())
            });

        let detail = Paragraph::new(detail_text)
            .block(Block::default().borders(Borders::ALL).title("Detail"))
            .wrap(Wrap { trim: false });
        frame.render_widget(detail, chunks[2]);

        let footer = Paragraph::new(self.footer_text())
            .block(Block::default().borders(Borders::ALL).title("Footer"))
            .wrap(Wrap { trim: true });
        frame.render_widget(footer, chunks[3]);
    }
}

fn build_summary_text(trace: &TraceSummary) -> String {
    format!(
        "Trace ID: {}\nStatus: {}\nExit code: {}\nDuration: {}ms\nCommand: {}\nPath: {}",
        trace.trace_id,
        if trace.archived { "ARCHIVED" } else { "ACTIVE" },
        trace
            .exit_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "-".to_string()),
        trace.duration_ms,
        trace.command,
        trace.trace_path.display()
    )
}

impl TraceBrowserState {
    fn status_line(&self) -> String {
        if self.status.is_empty() {
            "Status: none".to_string()
        } else {
            format!("Status: {}", self.status)
        }
    }

    fn filter_line(&self) -> String {
        if self.filter_mode {
            format!("Filter: {}_", self.filter)
        } else if self.filter.is_empty() {
            "Filter: none".to_string()
        } else {
            format!("Filter: {}", self.filter)
        }
    }
}

#[cfg(all(test, feature = "tui"))]
mod tests {
    use super::*;
    use crate::commands::verify::collect_verification;
    use crate::evidence::integrity::sha256_file;
    use crate::evidence::store::{FilesystemTraceStore, TraceStoreConfig};
    use serde_json::json;
    use std::fs;
    use tempfile::TempDir;

    #[derive(Clone, Copy)]
    enum PtyFixture {
        None,
        Mismatch,
    }

    fn create_trace(
        workspace: &std::path::Path,
        trace_id: &str,
        archived: bool,
        manifest_sidecar: bool,
        pty_fixture: PtyFixture,
    ) -> Result<PathBuf> {
        let root = if archived {
            workspace.join(".traces/archive").join(trace_id)
        } else {
            workspace.join(".traces").join(trace_id)
        };
        fs::create_dir_all(&root)?;

        let (pty_path, pty_sha256) = match pty_fixture {
            PtyFixture::None => (serde_json::Value::Null, serde_json::Value::Null),
            PtyFixture::Mismatch => {
                fs::write(root.join("pty.log"), "pty\n")?;
                (
                    serde_json::Value::String("pty.log".to_string()),
                    serde_json::Value::String(
                        "0000000000000000000000000000000000000000000000000000000000000000"
                            .to_string(),
                    ),
                )
            }
        };

        let manifest = json!({
            "manifest_version": 1,
            "trace_id": trace_id,
            "context": {},
            "tool_kind": "process",
            "command": ["echo", "hello"],
            "cwd": "/tmp",
            "started_at": "2026-01-01T00:00:00+00:00",
            "finished_at": "2026-01-01T00:00:01+00:00",
            "duration_ms": 1000,
            "exit_code": 0,
            "artifacts": {
                "stdout": "stdout.log",
                "stderr": "stderr.log",
                "pty": pty_path,
            },
            "integrity": {
                "stdout_sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                "stderr_sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                "pty_sha256": pty_sha256,
            },
            "git": {
                "commit_before": null,
                "commit_after": null,
                "branch_before": null,
                "branch_after": null,
                "dirty_before": false,
                "dirty_after": false,
            },
            "workspace": {
                "dirty_before": [],
                "dirty_after": [],
                "changes": {
                    "modified_files": [],
                    "created_files": [],
                    "deleted_files": [],
                },
            },
            "env": [],
        });

        fs::write(
            root.join("manifest.json"),
            format!("{}\n", serde_json::to_string_pretty(&manifest)?),
        )?;
        fs::write(root.join("stdout.log"), "")?;
        fs::write(root.join("stderr.log"), "")?;
        if manifest_sidecar {
            let manifest_hash = sha256_file(&root.join("manifest.json"))?;
            fs::write(root.join("manifest.sha256"), format!("{manifest_hash}\n"))?;
        }
        Ok(root)
    }

    #[test]
    fn loads_active_and_archived_traces_and_skips_reserved_dirs() -> Result<()> {
        let temp = TempDir::new()?;
        fs::create_dir_all(temp.path().join(".traces/archive"))?;
        fs::create_dir_all(temp.path().join(".traces/reports"))?;

        let active = create_trace(temp.path(), "trc_active", false, true, PtyFixture::None)?;
        let archived = create_trace(temp.path(), "trc_archived", true, true, PtyFixture::None)?;

        let catalog = load_trace_catalog(&temp.path().join(".traces"))?;
        assert_eq!(catalog.active.len(), 1);
        assert_eq!(catalog.archived.len(), 1);
        assert_eq!(catalog.active[0].trace_id, "trc_active");
        assert_eq!(catalog.archived[0].trace_id, "trc_archived");
        assert!(!catalog
            .active
            .iter()
            .any(|trace| trace.trace_path.ends_with("archive")));
        assert!(active.exists());
        assert!(archived.exists());

        Ok(())
    }

    #[test]
    fn footer_help_text_is_present_in_state_model() -> Result<()> {
        let temp = TempDir::new()?;
        let state = TraceBrowserState::load(temp.path().join(".traces"))?;

        assert_eq!(state.status_text(), "Filter: none | Status: Ready");
        assert_eq!(state.keys_text(), HELP_FOOTER);
        assert!(state.footer_text().contains("Filter: none | Status: Ready"));
        assert!(state.footer_text().contains("q quit |"));

        Ok(())
    }

    #[test]
    fn empty_active_and_archived_states_do_not_panic() -> Result<()> {
        let temp = TempDir::new()?;
        let mut state = TraceBrowserState::load(temp.path().join(".traces"))?;

        assert_eq!(
            state.empty_state_message(),
            Some("No active traces found".to_string())
        );
        assert_eq!(state.status_text(), "Filter: none | Status: Ready");

        state.switch_tab();
        assert_eq!(
            state.empty_state_message(),
            Some("No archived traces found".to_string())
        );

        Ok(())
    }

    #[test]
    fn filter_by_command_and_trace_id_works() -> Result<()> {
        let temp = TempDir::new()?;
        let root = temp.path().join(".traces");
        fs::create_dir_all(&root)?;
        fs::create_dir_all(root.join("trc_alpha"))?;
        fs::create_dir_all(root.join("trc_beta"))?;
        let alpha_manifest = json!({
            "manifest_version": 1,
            "trace_id": "trc_alpha",
            "context": {},
            "tool_kind": "process",
            "command": ["git", "status"],
            "cwd": "/tmp",
            "started_at": "2026-01-01T00:00:00+00:00",
            "finished_at": "2026-01-01T00:00:01+00:00",
            "duration_ms": 1000,
            "exit_code": 0,
            "artifacts": {"stdout": "stdout.log", "stderr": "stderr.log", "pty": null},
            "integrity": {
                "stdout_sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                "stderr_sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                "pty_sha256": null
            },
            "git": {
                "commit_before": null,
                "commit_after": null,
                "branch_before": null,
                "branch_after": null,
                "dirty_before": false,
                "dirty_after": false
            },
            "workspace": {
                "dirty_before": [],
                "dirty_after": [],
                "changes": {"modified_files": [], "created_files": [], "deleted_files": []}
            },
            "env": []
        });
        fs::write(
            root.join("trc_alpha/manifest.json"),
            format!("{}\n", serde_json::to_string_pretty(&alpha_manifest)?),
        )?;
        fs::write(root.join("trc_alpha/stdout.log"), "")?;
        fs::write(root.join("trc_alpha/stderr.log"), "")?;
        fs::write(root.join("trc_alpha/manifest.sha256"), "dummy\n")?;

        let beta_manifest = json!({
            "manifest_version": 1,
            "trace_id": "trc_beta",
            "context": {},
            "tool_kind": "process",
            "command": ["echo", "needle"],
            "cwd": "/tmp",
            "started_at": "2026-01-02T00:00:00+00:00",
            "finished_at": "2026-01-02T00:00:01+00:00",
            "duration_ms": 1000,
            "exit_code": 0,
            "artifacts": {"stdout": "stdout.log", "stderr": "stderr.log", "pty": null},
            "integrity": {
                "stdout_sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                "stderr_sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                "pty_sha256": null
            },
            "git": {
                "commit_before": null,
                "commit_after": null,
                "branch_before": null,
                "branch_after": null,
                "dirty_before": false,
                "dirty_after": false
            },
            "workspace": {
                "dirty_before": [],
                "dirty_after": [],
                "changes": {"modified_files": [], "created_files": [], "deleted_files": []}
            },
            "env": []
        });
        fs::write(
            root.join("trc_beta/manifest.json"),
            format!("{}\n", serde_json::to_string_pretty(&beta_manifest)?),
        )?;
        fs::write(root.join("trc_beta/stdout.log"), "")?;
        fs::write(root.join("trc_beta/stderr.log"), "")?;
        fs::write(root.join("trc_beta/manifest.sha256"), "dummy\n")?;

        let catalog = load_trace_catalog(&root)?;
        let filtered = catalog.filtered_for_tab(TraceTab::Active, "needle");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].trace_id, "trc_beta");

        let filtered_id = catalog.filtered_for_tab(TraceTab::Active, "trc_alpha");
        assert_eq!(filtered_id.len(), 1);
        assert_eq!(filtered_id[0].trace_id, "trc_alpha");

        Ok(())
    }

    #[test]
    fn no_match_filter_shows_empty_state() -> Result<()> {
        let temp = TempDir::new()?;
        let root = temp.path().join(".traces");
        fs::create_dir_all(&root)?;
        let _ = create_trace(temp.path(), "trc_alpha", false, true, PtyFixture::None)?;

        let mut state = TraceBrowserState::load(root)?;
        state.set_filter("does-not-match");

        assert_eq!(
            state.empty_state_message(),
            Some("No traces match filter".to_string())
        );
        assert_eq!(
            state.status_text(),
            "Filter: does-not-match | Status: Ready"
        );

        Ok(())
    }

    #[test]
    fn detail_text_includes_command_and_path() -> Result<()> {
        let temp = TempDir::new()?;
        let root = temp.path().join(".traces");
        fs::create_dir_all(&root)?;
        let _ = create_trace(temp.path(), "trc_detail", false, true, PtyFixture::None)?;

        let state = TraceBrowserState::load(root)?;
        let mut state = state;
        state.inspect_selected()?;
        let detail = state.detail_text().unwrap_or("");

        assert!(detail.contains("Command: echo hello"));
        assert!(detail.contains("Path:"));

        Ok(())
    }

    #[test]
    fn report_verify_archive_and_restore_set_status_messages() -> Result<()> {
        let temp = TempDir::new()?;
        let root = temp.path().to_path_buf();
        let _trace_path = create_trace(temp.path(), "trc_action", false, true, PtyFixture::None)?;
        let mut state = TraceBrowserState::load(root.join(".traces"))?;

        state.report_selected()?;
        assert!(state.status().starts_with("Report written: "));
        assert!(state.status_text().contains("Status: Report written: "));

        state.verify_selected()?;
        assert_eq!(state.status(), "Verification: OK");
        assert_eq!(
            state.status_text(),
            "Filter: none | Status: Verification: OK"
        );

        state.archive_selected()?;
        assert_eq!(state.status(), "Archived trace: trc_action");
        assert_eq!(
            state.status_text(),
            "Filter: none | Status: Archived trace: trc_action"
        );

        state.switch_tab();
        state.restore_selected()?;
        assert_eq!(state.status(), "Restored trace: trc_action");
        assert_eq!(
            state.status_text(),
            "Filter: none | Status: Restored trace: trc_action"
        );

        Ok(())
    }

    #[test]
    fn verify_selected_fails_without_manifest_sidecar() -> Result<()> {
        let temp = TempDir::new()?;
        let root = temp.path().to_path_buf();
        let _ = create_trace(
            temp.path(),
            "trc_missing_sidecar",
            false,
            false,
            PtyFixture::None,
        )?;
        let mut state = TraceBrowserState::load(root.join(".traces"))?;

        state.verify_selected()?;
        assert!(state.status().starts_with("Verification failed:"));
        assert!(state.detail_text().unwrap_or("").contains("Status: FAILED"));

        Ok(())
    }

    #[test]
    fn verify_selected_fails_when_manifest_sha256_does_not_match() -> Result<()> {
        let temp = TempDir::new()?;
        let root = temp.path().to_path_buf();
        let trace_path = create_trace(
            temp.path(),
            "trc_bad_sidecar",
            false,
            true,
            PtyFixture::None,
        )?;
        fs::write(trace_path.join("manifest.sha256"), "deadbeef\n")?;

        let mut state = TraceBrowserState::load(root.join(".traces"))?;
        state.verify_selected()?;

        assert!(state.status().starts_with("Verification failed:"));
        assert!(state.detail_text().unwrap_or("").contains("Status: FAILED"));

        Ok(())
    }

    #[test]
    fn verify_selected_fails_for_optional_pty_mismatch() -> Result<()> {
        let temp = TempDir::new()?;
        let root = temp.path().to_path_buf();
        let _ = create_trace(
            temp.path(),
            "trc_bad_pty",
            false,
            true,
            PtyFixture::Mismatch,
        )?;

        let mut state = TraceBrowserState::load(root.join(".traces"))?;
        state.verify_selected()?;

        assert!(state.status().starts_with("Verification failed:"));
        assert!(state.detail_text().unwrap_or("").contains("pty.log"));

        Ok(())
    }

    #[test]
    fn verify_selected_matches_cli_verifier_for_intact_trace() -> Result<()> {
        let temp = TempDir::new()?;
        let root = temp.path().to_path_buf();
        let _ = create_trace(temp.path(), "trc_intact", false, true, PtyFixture::None)?;
        let mut state = TraceBrowserState::load(root.join(".traces"))?;

        let store = FilesystemTraceStore::new(TraceStoreConfig::new(root.join(".traces")));
        let cli_result = collect_verification(&store, "trc_intact")?;
        assert!(cli_result.all_ok());

        state.verify_selected()?;
        assert_eq!(state.status(), "Verification: OK");
        assert!(state.detail_text().unwrap_or("").contains("Status: OK"));

        Ok(())
    }

    #[test]
    fn moving_down_beyond_visible_height_increases_scroll_offset() -> Result<()> {
        let temp = TempDir::new()?;
        let root = temp.path().join(".traces");
        fs::create_dir_all(&root)?;
        for idx in 0..5 {
            let trace_id = format!("trc_{idx}");
            let _ = create_trace(temp.path(), &trace_id, false, true, PtyFixture::None)?;
        }

        let mut state = TraceBrowserState::load(root)?;
        state.set_viewport_rows(2);

        state.move_selection(1);
        assert_eq!(state.selected_index(), 1);
        assert_eq!(state.scroll_offset(), 0);

        state.move_selection(1);
        assert_eq!(state.selected_index(), 2);
        assert_eq!(state.scroll_offset(), 1);

        state.move_selection(1);
        assert_eq!(state.selected_index(), 3);
        assert_eq!(state.scroll_offset(), 2);

        Ok(())
    }

    #[test]
    fn moving_up_above_viewport_decreases_scroll_offset() -> Result<()> {
        let temp = TempDir::new()?;
        let root = temp.path().join(".traces");
        fs::create_dir_all(&root)?;
        for idx in 0..5 {
            let trace_id = format!("trc_{idx}");
            let _ = create_trace(temp.path(), &trace_id, false, true, PtyFixture::None)?;
        }

        let mut state = TraceBrowserState::load(root)?;
        state.set_viewport_rows(2);
        state.move_selection(4);
        assert_eq!(state.selected_index(), 4);
        assert_eq!(state.scroll_offset(), 3);

        state.move_selection(-2);
        assert_eq!(state.selected_index(), 2);
        assert_eq!(state.scroll_offset(), 2);

        state.move_selection(-1);
        assert_eq!(state.selected_index(), 1);
        assert_eq!(state.scroll_offset(), 1);

        Ok(())
    }

    #[test]
    fn local_selected_index_is_global_minus_scroll_offset() -> Result<()> {
        let temp = TempDir::new()?;
        let root = temp.path().join(".traces");
        fs::create_dir_all(&root)?;
        for idx in 0..4 {
            let trace_id = format!("trc_{idx}");
            let _ = create_trace(temp.path(), &trace_id, false, true, PtyFixture::None)?;
        }

        let mut state = TraceBrowserState::load(root)?;
        state.set_viewport_rows(2);
        state.move_selection(3);

        assert_eq!(state.selected_index(), 3);
        assert_eq!(state.scroll_offset(), 2);
        assert_eq!(state.local_selected_index(), Some(1));

        Ok(())
    }

    #[test]
    fn filter_switch_archive_restore_and_empty_lists_clamp_scroll_state() -> Result<()> {
        let temp = TempDir::new()?;
        let root = temp.path().join(".traces");
        fs::create_dir_all(&root)?;
        for idx in 0..4 {
            let trace_id = format!("trc_{idx}");
            let _ = create_trace(temp.path(), &trace_id, false, true, PtyFixture::None)?;
        }

        let mut state = TraceBrowserState::load(root.clone())?;
        state.set_viewport_rows(2);
        state.move_selection(3);
        assert_eq!(state.selected_index(), 3);
        assert_eq!(state.scroll_offset(), 2);

        state.set_filter("trc_0");
        assert_eq!(state.selected_index(), 0);
        assert_eq!(state.scroll_offset(), 0);

        state.set_filter("");
        state.switch_tab();
        assert_eq!(state.selected_index(), 0);
        assert_eq!(state.scroll_offset(), 0);

        state = TraceBrowserState::load(root)?;
        state.set_viewport_rows(2);
        state.move_selection(1);
        state.archive_selected()?;
        assert_eq!(state.scroll_offset(), 0);
        assert!(state.selected_index() < state.visible_traces().len().max(1));

        state.switch_tab();
        state.restore_selected()?;
        assert_eq!(state.scroll_offset(), 0);
        assert!(state.selected_index() < state.visible_traces().len().max(1));

        let temp_empty = TempDir::new()?;
        let mut empty_state = TraceBrowserState::load(temp_empty.path().join(".traces"))?;
        empty_state.set_viewport_rows(2);
        assert_eq!(empty_state.selected_index(), 0);
        assert_eq!(empty_state.scroll_offset(), 0);

        Ok(())
    }

    #[test]
    fn missing_traces_directory_does_not_panic() -> Result<()> {
        let temp = TempDir::new()?;
        let catalog = load_trace_catalog(&temp.path().join(".traces"))?;
        assert!(catalog.is_empty());
        Ok(())
    }

    #[test]
    fn archive_and_restore_actions_delegate_to_store_logic() -> Result<()> {
        let temp = TempDir::new()?;
        let root = temp.path().to_path_buf();
        let trace_path = create_trace(temp.path(), "trc_action", false, true, PtyFixture::None)?;
        let mut state = TraceBrowserState::load(root.join(".traces"))?;

        assert_eq!(state.visible_traces().len(), 1);
        assert_eq!(state.selected_trace().unwrap().trace_id, "trc_action");

        state.archive_selected()?;
        assert!(!trace_path.exists());
        assert!(root.join(".traces/archive/trc_action").is_dir());

        state.switch_tab();
        assert_eq!(state.active_tab(), TraceTab::Archived);
        assert_eq!(state.visible_traces().len(), 1);
        assert_eq!(state.selected_trace().unwrap().trace_id, "trc_action");

        state.restore_selected()?;
        assert!(root.join(".traces/trc_action").is_dir());
        assert!(!root.join(".traces/archive/trc_action").exists());

        Ok(())
    }
}
