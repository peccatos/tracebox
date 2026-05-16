use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::commands::report;
use crate::evidence::browser::{load_trace_catalog, TraceCatalog, TraceSummary, TraceTab};
use crate::evidence::integrity::sha256_file;
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
use ratatui::layout::{Constraint, Direction, Layout};
#[cfg(feature = "tui")]
use ratatui::style::{Color, Modifier, Style};
#[cfg(feature = "tui")]
use ratatui::text::{Line, Span};
#[cfg(feature = "tui")]
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs, Wrap};
#[cfg(feature = "tui")]
use ratatui::{Frame, Terminal};

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
    selection: usize,
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
            selection: 0,
            filter: String::new(),
            filter_mode: false,
            detail: DetailView::Help,
            status: "Ready".to_string(),
        })
    }

    pub fn refresh(&mut self) -> Result<()> {
        self.catalog = load_trace_catalog(&self.trace_root)?;
        self.clamp_selection();
        Ok(())
    }

    pub fn visible_traces(&self) -> Vec<&TraceSummary> {
        self.catalog.filtered_for_tab(self.tab, &self.filter)
    }

    pub fn selected_trace(&self) -> Option<&TraceSummary> {
        self.visible_traces().get(self.selection).copied()
    }

    pub fn filter_text(&self) -> &str {
        &self.filter
    }

    pub fn active_tab(&self) -> TraceTab {
        self.tab
    }

    pub fn selection(&self) -> usize {
        self.selection
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn detail_text(&self) -> Option<&str> {
        match &self.detail {
            DetailView::Help => None,
            DetailView::Text(text) => Some(text.as_str()),
        }
    }

    pub fn set_filter(&mut self, filter: impl Into<String>) {
        self.filter = filter.into();
        self.selection = 0;
    }

    pub fn switch_tab(&mut self) {
        self.tab = self.tab.toggle();
        self.selection = 0;
        self.detail = DetailView::Help;
    }

    pub fn move_selection(&mut self, offset: isize) {
        let traces = self.visible_traces();

        if traces.is_empty() {
            self.selection = 0;
            return;
        }

        let len = traces.len() as isize;
        let mut next = self.selection as isize + offset;
        if next < 0 {
            next = 0;
        }
        if next >= len {
            next = len - 1;
        }
        self.selection = next as usize;
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
        let verify = build_verify_text(&self.trace_root, &summary)?;
        self.detail = DetailView::Text(verify);
        self.status = format!("Verified {}", summary.trace_id);
        Ok(())
    }

    pub fn report_selected(&mut self) -> Result<()> {
        let summary = self
            .selected_trace()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no trace selected"))?;
        let store = FilesystemTraceStore::new(TraceStoreConfig::new(&self.trace_root));
        let resolved = store.resolve_trace(&summary.trace_id)?;
        let manifest = store.load_manifest_at(&resolved.paths)?;
        let report_path = resolved.paths.root.join("report.md");
        let report_text = report::build_report(&resolved.paths.root, &manifest, &resolved.paths)?;

        fs::write(&report_path, report_text)
            .with_context(|| format!("failed to write {}", report_path.display()))?;

        let rendered = fs::read_to_string(&report_path)
            .with_context(|| format!("failed to read {}", report_path.display()))?;

        self.detail = DetailView::Text(rendered);
        self.status = format!("Report written: {}", report_path.display());
        Ok(())
    }

    pub fn archive_selected(&mut self) -> Result<()> {
        let trace_id = self
            .selected_trace()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no trace selected"))?
            .trace_id
            .clone();

        let store = FilesystemTraceStore::new(TraceStoreConfig::new(&self.trace_root));
        store.archive_trace(&trace_id)?;
        self.refresh()?;
        self.status = format!("Archived {trace_id}");
        Ok(())
    }

    pub fn restore_selected(&mut self) -> Result<()> {
        let trace_id = self
            .selected_trace()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no trace selected"))?
            .trace_id
            .clone();

        let store = FilesystemTraceStore::new(TraceStoreConfig::new(&self.trace_root));
        store.restore_trace(&trace_id)?;
        self.refresh()?;
        self.status = format!("Restored {trace_id}");
        Ok(())
    }

    pub fn push_filter_char(&mut self, ch: char) {
        self.filter.push(ch);
        self.selection = 0;
    }

    pub fn pop_filter_char(&mut self) {
        self.filter.pop();
        self.selection = 0;
    }

    pub fn enter_filter_mode(&mut self) {
        self.filter_mode = true;
        self.status = "Filter mode".to_string();
    }

    pub fn exit_filter_mode(&mut self) {
        self.filter_mode = false;
        self.clamp_selection();
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
                        self.status = "Selected trace is archived".to_string();
                    }
                }
            }
            KeyCode::Char('u') => {
                if let Some(trace) = self.selected_trace() {
                    if trace.archived {
                        self.restore_selected()?;
                    } else {
                        self.status = "Selected trace is active".to_string();
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

    fn clamp_selection(&mut self) {
        let len = self.visible_traces().len();
        if len == 0 {
            self.selection = 0;
        } else if self.selection >= len {
            self.selection = len - 1;
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
    fn draw(&self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(10),
                Constraint::Length(10),
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

        let traces = self.visible_traces();
        let selected = self.selection.min(traces.len().saturating_sub(1));

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

        let rows = traces.iter().enumerate().map(|(idx, trace)| {
            let style = if idx == selected {
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

        let detail_text = self
            .detail_text()
            .map(|text| text.to_string())
            .unwrap_or_else(|| {
                self.selected_trace()
                    .map(build_summary_text)
                    .unwrap_or_else(|| "No trace selected".to_string())
            });

        let bottom = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(6), Constraint::Length(2)])
            .split(chunks[2]);

        let detail = Paragraph::new(detail_text)
            .block(Block::default().borders(Borders::ALL).title("Detail"))
            .wrap(Wrap { trim: false });
        frame.render_widget(detail, bottom[0]);

        let filter_label = "Filter: ";
        let filter_text = if self.filter_mode {
            format!("{filter_label}{}_", self.filter)
        } else if self.filter.is_empty() {
            "Filter: <empty>".to_string()
        } else {
            format!("Filter: {}", self.filter)
        };

        let help = if self.filter_mode {
            "Enter apply | Esc cancel | Backspace delete"
        } else {
            "q quit | j/k or arrows move | Tab switch tab | Enter inspect | v verify | r report | a archive | u restore | / filter"
        };

        let footer = Paragraph::new(format!("{}    {}", filter_text, help)).block(
            Block::default()
                .borders(Borders::ALL)
                .title(self.status.clone()),
        );
        frame.render_widget(footer, bottom[1]);
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

fn build_verify_text(trace_root: &PathBuf, trace: &TraceSummary) -> Result<String> {
    let store = FilesystemTraceStore::new(TraceStoreConfig::new(trace_root));
    let resolved = store.resolve_trace(&trace.trace_id)?;
    let manifest = store.load_manifest_at(&resolved.paths)?;
    let mut lines = Vec::new();

    lines.push(format!("Trace ID: {}", trace.trace_id));
    lines.push(format!("Trace path: {}", resolved.paths.root.display()));

    let manifest_hash = manifest_sha256_status(&resolved.paths)?;
    lines.push(format!(
        "Manifest: {}",
        manifest_hash.unwrap_or_else(|| "unavailable".to_string())
    ));

    let stdout_hash = verify_artifact(&resolved.paths.stdout, &manifest.integrity.stdout_sha256)?;
    let stderr_hash = verify_artifact(&resolved.paths.stderr, &manifest.integrity.stderr_sha256)?;

    lines.push(format!("stdout: {stdout_hash}"));
    lines.push(format!("stderr: {stderr_hash}"));

    lines.push("Status: OK".to_string());

    Ok(lines.join("\n"))
}

fn manifest_sha256_status(paths: &crate::evidence::store::TracePaths) -> Result<Option<String>> {
    if !paths.manifest_sha256.exists() {
        return Ok(None);
    }

    let expected = fs::read_to_string(&paths.manifest_sha256)
        .with_context(|| format!("failed to read {}", paths.manifest_sha256.display()))?
        .trim()
        .to_string();

    if expected.is_empty() {
        return Ok(Some("FAILED (empty manifest.sha256)".to_string()));
    }

    let actual = sha256_file(&paths.manifest)?;

    if actual == expected {
        Ok(Some("OK".to_string()))
    } else {
        Ok(Some(format!(
            "FAILED (expected {expected}, actual {actual})"
        )))
    }
}

fn verify_artifact(path: &Path, expected: &str) -> Result<String> {
    let actual = sha256_file(path)?;
    if actual == expected {
        Ok("OK".to_string())
    } else {
        Ok(format!("FAILED (expected {expected}, actual {actual})"))
    }
}

#[cfg(all(test, feature = "tui"))]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use tempfile::TempDir;

    fn create_trace(
        workspace: &std::path::Path,
        trace_id: &str,
        archived: bool,
    ) -> Result<PathBuf> {
        let root = if archived {
            workspace.join(".traces/archive").join(trace_id)
        } else {
            workspace.join(".traces").join(trace_id)
        };
        fs::create_dir_all(&root)?;
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
                "pty": null,
            },
            "integrity": {
                "stdout_sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                "stderr_sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                "pty_sha256": null,
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
        fs::write(root.join("manifest.sha256"), "dummy\n")?;
        Ok(root)
    }

    #[test]
    fn loads_active_and_archived_traces_and_skips_reserved_dirs() -> Result<()> {
        let temp = TempDir::new()?;
        fs::create_dir_all(temp.path().join(".traces/archive"))?;
        fs::create_dir_all(temp.path().join(".traces/reports"))?;

        let active = create_trace(temp.path(), "trc_active", false)?;
        let archived = create_trace(temp.path(), "trc_archived", true)?;

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
        let trace_path = create_trace(temp.path(), "trc_action", false)?;
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
