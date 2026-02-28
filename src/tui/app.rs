use std::collections::VecDeque;

use crate::agent::UsageInfo;
use crate::model_router::RequestLog;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActivePage {
    #[default]
    Usage,
    Router,
}

#[derive(Debug)]
pub struct App {
    pub router_addr: String,
    pub router_running: bool,
    pub usage: Vec<UsageInfo>,
    pub logs: VecDeque<RequestLog>,
    pub log_scroll: usize,
    pub active_page: ActivePage,
    pub status_message: Option<String>,
}

impl App {
    pub fn new(router_addr: String) -> Self {
        Self {
            router_addr,
            router_running: false,
            usage: Vec::new(),
            logs: VecDeque::with_capacity(1_000),
            log_scroll: 0,
            active_page: ActivePage::Usage,
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
        if self.active_page == ActivePage::Router {
            self.log_scroll = self.log_scroll.saturating_sub(1);
        }
    }

    pub fn scroll_down(&mut self) {
        if self.active_page == ActivePage::Router {
            let max_scroll = self.logs.len().saturating_sub(1);
            self.log_scroll = (self.log_scroll + 1).min(max_scroll);
        }
    }

    pub fn next_tab(&mut self) {
        self.active_page = match self.active_page {
            ActivePage::Usage => ActivePage::Router,
            ActivePage::Router => ActivePage::Usage,
        };
    }
}
