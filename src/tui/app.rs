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
    pub log_source: String,
    pub log_source_note: Option<String>,
    pub usage: Vec<UsageInfo>,
    pub usage_scroll: usize,
    pub usage_max_scroll: usize,
    pub usage_page_step: usize,
    pub usage_selected_card: Option<usize>,
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
            log_source: "memory".to_string(),
            log_source_note: None,
            usage: Vec::new(),
            usage_scroll: 0,
            usage_max_scroll: 0,
            usage_page_step: 1,
            usage_selected_card: None,
            logs: VecDeque::with_capacity(1_000),
            log_scroll: 0,
            active_page: ActivePage::Usage,
            status_message: None,
        }
    }

    pub fn push_log(&mut self, log: RequestLog) {
        if self.log_scroll > 0 {
            self.log_scroll += 1;
        }
        if self.logs.len() >= 1_000 {
            self.logs.pop_front();
        }
        self.logs.push_back(log);
        let max_scroll = self.logs.len().saturating_sub(1);
        self.log_scroll = self.log_scroll.min(max_scroll);
    }

    pub fn set_usage_scroll_meta(&mut self, max_scroll: usize, page_step: usize) {
        self.usage_max_scroll = max_scroll;
        self.usage_page_step = page_step.max(1);
        self.usage_scroll = self.usage_scroll.min(self.usage_max_scroll);
    }

    pub fn usage_scroll_down(&mut self, step: usize) {
        self.usage_scroll = self
            .usage_scroll
            .saturating_add(step)
            .min(self.usage_max_scroll);
    }

    pub fn usage_scroll_up(&mut self, step: usize) {
        self.usage_scroll = self.usage_scroll.saturating_sub(step);
    }

    pub fn usage_scroll_to_top(&mut self) {
        self.usage_scroll = 0;
    }

    pub fn usage_scroll_to_bottom(&mut self) {
        self.usage_scroll = self.usage_max_scroll;
    }

    pub fn set_usage_selected_card(&mut self, card: Option<usize>) {
        self.usage_selected_card = match (card, self.usage.len()) {
            (_, 0) => None,
            (Some(index), len) => Some(index.min(len - 1)),
            (None, _) => None,
        };
    }

    pub fn clamp_usage_selected_card(&mut self) {
        self.set_usage_selected_card(self.usage_selected_card);
    }

    pub fn cycle_usage_selected_card_forward(&mut self) {
        let len = self.usage.len();
        if len == 0 {
            self.usage_selected_card = None;
            return;
        }
        self.usage_selected_card = Some(match self.usage_selected_card {
            None => 0,
            Some(index) => (index + 1) % len,
        });
    }

    pub fn select_first_usage_card(&mut self) {
        if !self.usage.is_empty() {
            self.usage_selected_card = Some(0);
        }
    }

    pub fn clear_usage_selected_card(&mut self) {
        self.usage_selected_card = None;
    }

    pub fn is_usage_widget_selected(&self) -> bool {
        self.active_page == ActivePage::Usage && self.usage_selected_card.is_some()
    }

    pub fn logs_scroll_up(&mut self) {
        let max_scroll = self.logs.len().saturating_sub(1);
        self.log_scroll = (self.log_scroll + 1).min(max_scroll);
    }

    pub fn logs_scroll_down(&mut self) {
        self.log_scroll = self.log_scroll.saturating_sub(1);
    }

    pub fn next_tab(&mut self) {
        let leaving_usage = self.active_page == ActivePage::Usage;
        self.active_page = match self.active_page {
            ActivePage::Usage => ActivePage::Router,
            ActivePage::Router => ActivePage::Usage,
        };
        if leaving_usage {
            self.usage_selected_card = None;
        }
        if self.active_page == ActivePage::Router {
            self.jump_to_latest_logs();
        }
    }

    pub fn jump_to_latest_logs(&mut self) {
        self.log_scroll = 0;
    }
}

#[cfg(test)]
mod tests {
    use crate::agent::UsageInfo;

    use super::{ActivePage, App};

    #[test]
    fn usage_scroll_is_clamped_by_meta() {
        let mut app = App::new("http://127.0.0.1:8080".to_string());
        app.usage_scroll = 12;
        app.set_usage_scroll_meta(3, 0);
        assert_eq!(app.usage_scroll, 3);
        assert_eq!(app.usage_max_scroll, 3);
        assert_eq!(app.usage_page_step, 1);
    }

    #[test]
    fn usage_scroll_moves_by_step_and_bounds() {
        let mut app = App::new("http://127.0.0.1:8080".to_string());
        app.set_usage_scroll_meta(5, 2);
        app.usage_scroll_down(1);
        assert_eq!(app.usage_scroll, 1);
        app.usage_scroll_down(10);
        assert_eq!(app.usage_scroll, 5);
        app.usage_scroll_up(2);
        assert_eq!(app.usage_scroll, 3);
        app.usage_scroll_up(99);
        assert_eq!(app.usage_scroll, 0);
    }

    #[test]
    fn switching_tabs_keeps_usage_scroll_and_resets_log_position() {
        let mut app = App::new("http://127.0.0.1:8080".to_string());
        app.usage_scroll = 4;
        app.usage = vec![UsageInfo::default()];
        app.usage_selected_card = Some(0);
        app.log_scroll = 7;
        app.active_page = ActivePage::Usage;

        app.next_tab();
        assert_eq!(app.active_page, ActivePage::Router);
        assert_eq!(app.log_scroll, 0);
        assert!(app.usage_selected_card.is_none());

        app.log_scroll = 3;
        app.next_tab();
        assert_eq!(app.active_page, ActivePage::Usage);
        assert_eq!(app.usage_scroll, 4);
        assert!(app.usage_selected_card.is_none());
    }

    #[test]
    fn usage_card_selection_behaves_as_expected() {
        let mut app = App::new("http://127.0.0.1:8080".to_string());
        app.usage = vec![
            UsageInfo::default(),
            UsageInfo::default(),
            UsageInfo::default(),
        ];
        assert!(app.usage_selected_card.is_none());

        app.select_first_usage_card();
        assert_eq!(app.usage_selected_card, Some(0));
        app.cycle_usage_selected_card_forward();
        assert_eq!(app.usage_selected_card, Some(1));
        app.cycle_usage_selected_card_forward();
        assert_eq!(app.usage_selected_card, Some(2));
        app.cycle_usage_selected_card_forward();
        assert_eq!(app.usage_selected_card, Some(0));
        app.set_usage_selected_card(Some(9));
        assert_eq!(app.usage_selected_card, Some(2));
        app.clear_usage_selected_card();
        assert!(app.usage_selected_card.is_none());
    }
}
