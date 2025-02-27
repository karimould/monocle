mod backend_workers;
mod search_results;
mod ui;
use zellij_tile::prelude::*;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use backend_workers::{FileContentsWorker, FileNameWorker, MessageToPlugin, MessageToSearch};
use search_results::{ResultsOfSearch, SearchResult};

pub const ROOT: &str = "/host";
pub const CURRENT_SEARCH_TERM: &str = "/data/current_search_term";

register_plugin!(State);
register_worker!(FileNameWorker, file_name_search_worker, FILE_NAME_WORKER);
register_worker!(
    FileContentsWorker,
    file_contents_search_worker,
    FILE_CONTENTS_WORKER
);

#[derive(Default)]
struct State {
    search_term: String,
    file_name_search_results: Vec<SearchResult>,
    file_contents_search_results: Vec<SearchResult>,
    loading: bool,
    loading_animation_offset: u8,
    should_open_floating: bool,
    search_filter: SearchType,
    display_rows: usize,
    display_columns: usize,
    displayed_search_results: (usize, Vec<SearchResult>), // usize is selected index
}

impl ZellijPlugin for State {
    fn load(&mut self) {
        self.loading = true;
        subscribe(&[
            EventType::Key,
            EventType::Mouse,
            EventType::CustomMessage,
            EventType::Timer,
            EventType::FileSystemCreate,
            EventType::FileSystemUpdate,
            EventType::FileSystemDelete,
        ]);
        post_message_to(
            "file_name_search",
            &serde_json::to_string(&MessageToSearch::ScanFolder).unwrap(),
            "",
        );
        post_message_to(
            "file_contents_search",
            &serde_json::to_string(&MessageToSearch::ScanFolder).unwrap(),
            "",
        );
        self.loading = true;
        set_timeout(0.5); // for displaying loading animation
    }

    fn update(&mut self, event: Event) -> bool {
        let mut should_render = false;
        match event {
            Event::Timer(_elapsed) => {
                if self.loading {
                    set_timeout(0.5);
                    self.progress_animation();
                    should_render = true;
                }
            }
            Event::CustomMessage(message, payload) => match serde_json::from_str(&message) {
                Ok(MessageToPlugin::UpdateFileNameSearchResults) => {
                    if let Ok(results_of_search) = serde_json::from_str::<ResultsOfSearch>(&payload)
                    {
                        self.update_file_name_search_results(results_of_search);
                        should_render = true;
                    }
                }
                Ok(MessageToPlugin::UpdateFileContentsSearchResults) => {
                    if let Ok(results_of_search) = serde_json::from_str::<ResultsOfSearch>(&payload)
                    {
                        self.update_file_contents_search_results(results_of_search);
                        should_render = true;
                    }
                }
                Ok(MessageToPlugin::DoneScanningFolder) => {
                    self.loading = false;
                    should_render = true;
                }
                Err(e) => eprintln!("Failed to deserialize custom message: {:?}", e),
            },
            Event::Key(key) => {
                self.handle_key(key);
                should_render = true;
            }
            Event::FileSystemCreate(paths) => {
                let paths: Vec<String> = paths
                    .iter()
                    .map(|p| p.to_string_lossy().to_string())
                    .collect();
                post_message_to(
                    "file_name_search",
                    &serde_json::to_string(&MessageToSearch::FileSystemCreate).unwrap(),
                    &serde_json::to_string(&paths).unwrap(),
                );
                post_message_to(
                    "file_contents_search",
                    &serde_json::to_string(&MessageToSearch::FileSystemCreate).unwrap(),
                    &serde_json::to_string(&paths).unwrap(),
                );
            }
            Event::FileSystemUpdate(paths) => {
                let paths: Vec<String> = paths
                    .iter()
                    .map(|p| p.to_string_lossy().to_string())
                    .collect();
                post_message_to(
                    "file_name_search",
                    &serde_json::to_string(&MessageToSearch::FileSystemUpdate).unwrap(),
                    &serde_json::to_string(&paths).unwrap(),
                );
                post_message_to(
                    "file_contents_search",
                    &serde_json::to_string(&MessageToSearch::FileSystemUpdate).unwrap(),
                    &serde_json::to_string(&paths).unwrap(),
                );
            }
            Event::FileSystemDelete(paths) => {
                let paths: Vec<String> = paths
                    .iter()
                    .map(|p| p.to_string_lossy().to_string())
                    .collect();
                post_message_to(
                    "file_name_search",
                    &serde_json::to_string(&MessageToSearch::FileSystemDelete).unwrap(),
                    &serde_json::to_string(&paths).unwrap(),
                );
                post_message_to(
                    "file_contents_search",
                    &serde_json::to_string(&MessageToSearch::FileSystemDelete).unwrap(),
                    &serde_json::to_string(&paths).unwrap(),
                );
            }
            _ => {
                eprintln!("Unknown event: {}", event.to_string());
            }
        }
        should_render
    }
    fn render(&mut self, rows: usize, cols: usize) {
        self.change_size(rows, cols);
        print!("{}", self);
    }
}

impl State {
    pub fn handle_key(&mut self, key: Key) {
        match key {
            Key::Down => self.move_search_selection_down(),
            Key::Up => self.move_search_selection_up(),
            Key::Char('\n') => self.open_search_result_in_editor(),
            Key::BackTab => self.open_search_result_in_terminal(),
            Key::Ctrl('f') => {
                self.should_open_floating = !self.should_open_floating;
            }
            Key::Ctrl('r') => self.toggle_search_filter(),
            Key::Esc | Key::Ctrl('c') => {
                if !self.search_term.is_empty() {
                    self.clear_state();
                } else {
                    hide_self();
                }
            }
            _ => self.append_to_search_term(key),
        }
    }
    pub fn update_file_name_search_results(&mut self, mut results_of_search: ResultsOfSearch) {
        if self.search_term == results_of_search.search_term {
            self.file_name_search_results = results_of_search.search_results.drain(..).collect();
            self.update_displayed_search_results();
        }
    }
    pub fn update_file_contents_search_results(&mut self, mut results_of_search: ResultsOfSearch) {
        if self.search_term == results_of_search.search_term {
            self.file_contents_search_results =
                results_of_search.search_results.drain(..).collect();
            self.update_displayed_search_results();
        }
    }
    pub fn change_size(&mut self, rows: usize, cols: usize) {
        self.display_rows = rows;
        self.display_columns = cols;
    }
    pub fn progress_animation(&mut self) {
        if self.loading_animation_offset == u8::MAX {
            self.loading_animation_offset = 0;
        } else {
            self.loading_animation_offset = self.loading_animation_offset.saturating_add(1);
        }
    }
    pub fn number_of_lines_in_displayed_search_results(&self) -> usize {
        self.displayed_search_results
            .1
            .iter()
            .map(|l| l.rendered_height())
            .sum()
    }
    fn move_search_selection_down(&mut self) {
        if self.displayed_search_results.0 < self.max_search_selection_index() {
            self.displayed_search_results.0 += 1;
        }
    }
    fn move_search_selection_up(&mut self) {
        self.displayed_search_results.0 = self.displayed_search_results.0.saturating_sub(1);
    }
    fn open_search_result_in_editor(&mut self) {
        match self.selected_search_result_entry() {
            Some(SearchResult::File { path, .. }) => {
                if self.should_open_floating {
                    open_file_floating(&PathBuf::from(path))
                } else {
                    open_file(&PathBuf::from(path));
                }
            }
            Some(SearchResult::LineInFile {
                path, line_number, ..
            }) => {
                if self.should_open_floating {
                    open_file_with_line_floating(&PathBuf::from(path), line_number);
                } else {
                    open_file_with_line(&PathBuf::from(path), line_number);
                }
            }
            None => eprintln!("Search results not found"),
        }
    }
    fn open_search_result_in_terminal(&mut self) {
        let dir_path_of_result = |path: &str| -> PathBuf {
            let file_path = PathBuf::from(path);
            let mut dir_path = file_path.components();
            dir_path.next_back(); // remove file name to stay with just the folder
            dir_path.as_path().into()
        };
        let selected_search_result_entry = self.selected_search_result_entry();
        if let Some(SearchResult::File { path, .. }) | Some(SearchResult::LineInFile { path, .. }) =
            selected_search_result_entry
        {
            let dir_path = dir_path_of_result(&path);
            if self.should_open_floating {
                open_terminal_floating(&dir_path);
            } else {
                open_terminal(&dir_path);
            }
        }
    }
    fn toggle_search_filter(&mut self) {
        self.search_filter.progress();
        self.send_search_query();
    }
    fn clear_state(&mut self) {
        self.file_name_search_results.clear();
        self.file_contents_search_results.clear();
        self.displayed_search_results = (0, vec![]);
        self.search_term.clear();
    }
    fn append_to_search_term(&mut self, key: Key) {
        match key {
            Key::Char(character) => {
                self.search_term.push(character);
            }
            Key::Backspace => {
                self.search_term.pop();
                if self.search_term.len() == 0 {
                    self.clear_state();
                }
            }
            _ => {}
        }
        self.send_search_query();
    }
    fn send_search_query(&mut self) {
        match std::fs::write(CURRENT_SEARCH_TERM, &self.search_term) {
            Ok(_) => {
                if !self.search_term.is_empty() {
                    post_message_to(
                        "file_name_search",
                        &serde_json::to_string(&MessageToSearch::Search).unwrap(),
                        "",
                    );
                    post_message_to(
                        "file_contents_search",
                        &serde_json::to_string(&MessageToSearch::Search).unwrap(),
                        "",
                    );
                    self.file_name_search_results.clear();
                    self.file_contents_search_results.clear();
                }
            }
            Err(e) => eprintln!("Failed to write search term to HD, aborting search: {}", e),
        }
    }
    fn max_search_selection_index(&self) -> usize {
        self.displayed_search_results.1.len().saturating_sub(1)
    }
    fn update_displayed_search_results(&mut self) {
        if self.search_term.is_empty() {
            self.clear_state();
            return;
        }
        let mut search_results_of_interest = match self.search_filter {
            SearchType::NamesAndContents => {
                let mut all_search_results = self.file_name_search_results.clone();
                all_search_results.append(&mut self.file_contents_search_results.clone());
                all_search_results.sort_by(|a, b| b.score().cmp(&a.score()));
                all_search_results
            }
            SearchType::Names => self.file_name_search_results.clone(),
            SearchType::Contents => self.file_contents_search_results.clone(),
        };
        let mut height_taken_up_by_results = 0;
        let mut displayed_search_results = vec![];
        for search_result in search_results_of_interest.drain(..) {
            if height_taken_up_by_results + search_result.rendered_height()
                > self.rows_for_results()
            {
                break;
            }
            height_taken_up_by_results += search_result.rendered_height();
            displayed_search_results.push(search_result);
        }
        let new_index = self
            .selected_search_result_entry()
            .and_then(|currently_selected_search_result| {
                displayed_search_results
                    .iter()
                    .position(|r| r.is_same_entry(&currently_selected_search_result))
            })
            .unwrap_or(0);
        self.displayed_search_results = (new_index, displayed_search_results);
    }
    fn selected_search_result_entry(&self) -> Option<SearchResult> {
        self.displayed_search_results
            .1
            .get(self.displayed_search_results.0)
            .cloned()
    }
    pub fn rows_for_results(&self) -> usize {
        self.display_rows.saturating_sub(3) // search line and 2 controls lines
    }
}

#[derive(Serialize, Deserialize)]
pub enum SearchType {
    NamesAndContents,
    Names,
    Contents,
}

impl SearchType {
    pub fn progress(&mut self) {
        match &self {
            &SearchType::NamesAndContents => *self = SearchType::Names,
            &SearchType::Names => *self = SearchType::Contents,
            &SearchType::Contents => *self = SearchType::NamesAndContents,
        }
    }
}

impl Default for SearchType {
    fn default() -> Self {
        SearchType::NamesAndContents
    }
}
