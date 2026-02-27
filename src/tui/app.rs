use std::collections::VecDeque;

use crate::oauth::UsageInfo;
use crate::proxy::RequestLog;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActivePage {
    #[default]
    Usage,
    Proxy,
}

#[derive(Debug)]
pub struct App {
    pub proxy_addr: String,
    pub proxy_running: bool,
    pub usage: Vec<UsageInfo>,
    pub logs: VecDeque<RequestLog>,
    pub log_scroll: usize,
    pub active_page: ActivePage,
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
        if self.active_page == ActivePage::Proxy {
            self.log_scroll = self.log_scroll.saturating_sub(1);
        }
    }

    pub fn scroll_down(&mut self) {
        if self.active_page == ActivePage::Proxy {
            let max_scroll = self.logs.len().saturating_sub(1);
            self.log_scroll = (self.log_scroll + 1).min(max_scroll);
        }
    }

    pub fn next_tab(&mut self) {
        self.active_page = match self.active_page {
            ActivePage::Usage => ActivePage::Proxy,
            ActivePage::Proxy => ActivePage::Usage,
        };
    }
}
