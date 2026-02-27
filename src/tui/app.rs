use std::collections::VecDeque;

use crate::oauth::UsageInfo;
use crate::proxy::RequestLog;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusedPanel {
    #[default]
    Usage,
    Logs,
}

#[derive(Debug)]
pub struct App {
    pub proxy_addr: String,
    pub proxy_running: bool,
    pub usage: Vec<UsageInfo>,
    pub logs: VecDeque<RequestLog>,
    pub log_scroll: usize,
    pub focused_panel: FocusedPanel,
    pub status_message: Option<String>,
}

impl App {
    pub fn new(proxy_addr: String) -> Self {
        Self {
            proxy_addr,
            proxy_running: false,
            usage: Vec::new(),
            logs: VecDeque::with_capacity(1_000),
            log_scroll: 0,
            focused_panel: FocusedPanel::Usage,
            status_message: None,
        }
    }

    pub fn push_log(&mut self, log: RequestLog) {
        if self.logs.len() >= 1_000 {
            self.logs.pop_front();
        }
        self.logs.push_back(log);
    }

    pub fn scroll_up(&mut self) {
        self.log_scroll = self.log_scroll.saturating_sub(1);
    }

    pub fn scroll_down(&mut self) {
        let max_scroll = self.logs.len().saturating_sub(1);
        self.log_scroll = (self.log_scroll + 1).min(max_scroll);
    }

    pub fn cycle_focus(&mut self) {
        self.focused_panel = match self.focused_panel {
            FocusedPanel::Usage => FocusedPanel::Logs,
            FocusedPanel::Logs => FocusedPanel::Usage,
        };
    }
}
