use super::{
    benchmark_recipes, commands, connection_matches, connection_model_choices, connection_port,
    copy_to_clipboard, harness_snippets, learn, report, save_config, save_connection_favorites,
    theme_matches, App, CommandAction, ExpandedPane, FocusPanel, ModelTab, PanelAction, Popup,
    Screen, Theme,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InputControl {
    Continue,
    Quit,
}

fn apply_command(app: &mut App, action: CommandAction) {
    match action {
        CommandAction::Home => app.show_screen(Screen::Home),
        CommandAction::Measure => app.show_screen(Screen::Measure),
        CommandAction::System => app.show_screen(Screen::System),
        CommandAction::Learn => app.show_screen(Screen::Learn),
        CommandAction::Customize => app.show_screen(Screen::Customize),
        CommandAction::Bloat => app.show_screen(Screen::Bloat),
        CommandAction::Models => {
            app.model_tab = ModelTab::Local;
            app.popup = Popup::Models;
            app.popup_sel = 0;
        }
        CommandAction::HuggingFace => {
            app.model_tab = ModelTab::HuggingFace;
            app.popup = Popup::Models;
            app.popup_sel = 0;
            app.check_huggingface();
        }
        CommandAction::Serve => toggle_server(app),
        CommandAction::Connect => {
            app.connect_model = connection_model_choices(app)
                .into_iter()
                .next()
                .unwrap_or_else(|| "model-id".into());
            app.connect_query.clear();
            app.popup = Popup::Connect;
            app.popup_sel = 0;
        }
        CommandAction::Recipes => {
            app.popup = Popup::Benchmarks;
            app.popup_sel = 0;
        }
        CommandAction::Benchmark => app.start_benchmark(false),
        CommandAction::Sweep => app.start_benchmark(true),
        CommandAction::Publish => app.popup = Popup::Publish,
        CommandAction::LocalAi => {
            app.model_tab = ModelTab::LocalAi;
            app.popup = Popup::Models;
            app.popup_sel = 0;
        }
        CommandAction::LocalAiRefresh => {
            app.model_tab = ModelTab::LocalAi;
            app.popup = Popup::Models;
            app.popup_sel = 0;
            app.refresh_local_ai();
        }
        CommandAction::Panels => {
            app.popup = Popup::Panels;
            app.popup_sel = 0;
        }
        CommandAction::Walkthrough => app.start_onboarding(),
    }
    app.dirty = true;
}

fn finish_onboarding(app: &mut App, open_destination: bool) {
    app.cfg.onboarding.completed = true;
    let save_error = save_config(&app.cfg).err();
    app.popup = Popup::None;

    if open_destination {
        match app.onboarding_sel {
            0 => {
                app.model_tab = ModelTab::Local;
                app.popup = Popup::Models;
                app.popup_sel = 0;
            }
            1 => {
                app.connect_model = connection_model_choices(app)
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| "model-id".into());
                app.connect_query.clear();
                app.popup = Popup::Connect;
                app.popup_sel = 0;
            }
            2 => app.show_screen(Screen::Measure),
            _ => app.show_screen(Screen::Learn),
        }
    }

    let message = if let Some(error) = save_error {
        format!("walkthrough closed but completion was not saved: {error}")
    } else if open_destination {
        "walkthrough complete | no usage telemetry is sent".into()
    } else {
        "walkthrough skipped | rerun it from Setup or the command palette".into()
    };
    app.set_status(message);
}

fn toggle_server(app: &mut App) {
    if app.server.running() {
        app.server.stop();
        app.set_status("server stopped".into());
    } else if app.online {
        app.set_status(format!(
            "server already served on :{} | use m to load a model",
            app.port
        ));
    } else if let Some(choice) = app.server.available.first().cloned() {
        app.choose_model(choice);
    } else {
        app.set_status("no model targets found | press m for catalog".into());
    }
}

fn quick_bloat_scan(app: &mut App) {
    let started = app.bloat.refresh();
    app.set_status(if started {
        "quick scan started".into()
    } else {
        "a Bloat scan is already running".into()
    });
}

fn move_process_selection(app: &mut App, backwards: bool) {
    let count = app.interference.offenders.len();
    if count == 0 {
        app.interference.selected = 0;
        return;
    }
    app.interference.selected = app.interference.selected.min(count - 1);
    app.interference.selected = if backwards {
        app.interference
            .selected
            .checked_sub(1)
            .unwrap_or(count - 1)
    } else {
        (app.interference.selected + 1) % count
    };
}

fn apply_selected_panel_action(app: &mut App) {
    let Some(panel) = app.expanded_panel else {
        return;
    };
    let actions = app.panel_actions(panel);
    let Some(item) = actions.get(app.expanded_action_sel).copied() else {
        app.set_status("no action is available for this reading".into());
        return;
    };
    if !item.enabled {
        app.set_status(format!(
            "{} is not available for the current state",
            item.label
        ));
        return;
    }
    match item.action {
        PanelAction::Command(action) => apply_command(app, action),
        PanelAction::PreviousRequest => app.select_request(true),
        PanelAction::NextRequest => app.select_request(false),
        PanelAction::PreviousProcess => move_process_selection(app, true),
        PanelAction::NextProcess => move_process_selection(app, false),
        PanelAction::TogglePressurePause => {
            app.interference.paused = !app.interference.paused;
            app.set_status(if app.interference.paused {
                "process list paused; system totals remain live".into()
            } else {
                "process list resumed".into()
            });
        }
        PanelAction::TerminateProcess => app.try_kill_selected(),
        PanelAction::QuickBloatScan => quick_bloat_scan(app),
        PanelAction::CreateEvalFixture => app.create_eval_fixture_from_selected(),
    }
}

impl App {
    pub(crate) fn handle_key(&mut self, k: KeyEvent) -> InputControl {
        self.dirty = true;
        match self.popup {
            Popup::Onboarding => match k.code {
                KeyCode::Char('q') => return InputControl::Quit,
                KeyCode::Esc | KeyCode::Char('s' | 'S') => finish_onboarding(self, false),
                KeyCode::Left | KeyCode::Backspace if self.onboarding_step > 0 => {
                    self.onboarding_step -= 1;
                }
                KeyCode::Up | KeyCode::Char('k') if self.onboarding_step == 1 => {
                    self.onboarding_sel = self.onboarding_sel.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') if self.onboarding_step == 1 => {
                    if self.onboarding_sel + 1 < super::ONBOARDING_CHOICES.len() {
                        self.onboarding_sel += 1;
                    }
                }
                KeyCode::Char(choice @ '1'..='4') if self.onboarding_step == 1 => {
                    self.onboarding_sel = choice as usize - '1' as usize;
                }
                KeyCode::Enter if self.onboarding_step < 2 => {
                    self.onboarding_step += 1;
                }
                KeyCode::Enter => finish_onboarding(self, true),
                _ => {}
            },
            Popup::Command => match k.code {
                KeyCode::Esc => self.popup = Popup::None,
                KeyCode::Up | KeyCode::Char('k') => {
                    self.command_sel = self.command_sel.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let count = commands::matches(&self.command_query).len();
                    if self.command_sel + 1 < count {
                        self.command_sel += 1;
                    }
                }
                KeyCode::Backspace => {
                    self.command_query.pop();
                    self.command_sel = 0;
                }
                KeyCode::Enter => {
                    let matches = commands::matches(&self.command_query);
                    if let Some(index) = matches.get(self.command_sel) {
                        let action = commands::catalog()[*index].action;
                        self.popup = Popup::None;
                        apply_command(self, action);
                    }
                }
                KeyCode::Char(ch) if !ch.is_control() => {
                    self.command_query.push(ch);
                    self.command_sel = 0;
                }
                _ => {}
            },
            Popup::Panels => match k.code {
                KeyCode::Esc => self.popup = Popup::None,
                KeyCode::Up | KeyCode::Char('k') => {
                    self.popup_sel = self.popup_sel.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if self.popup_sel + 1 < self.cfg.layout.panels.len() {
                        self.popup_sel += 1;
                    }
                }
                KeyCode::Char(' ') | KeyCode::Enter => {
                    if let Some(name) = self.cfg.layout.panels.get(self.popup_sel).cloned() {
                        self.cfg.layout.toggle_panel(&name);
                    }
                }
                KeyCode::Char('s') | KeyCode::Char('S') => {
                    match save_config(&self.cfg) {
                        Ok(()) => self.set_status("panel layout saved".into()),
                        Err(error) => self.set_status(format!("panel layout not saved: {}", error)),
                    }
                    self.popup = Popup::None;
                }
                _ => {}
            },
            Popup::Themes => match k.code {
                KeyCode::Esc => self.popup = Popup::None,
                KeyCode::Up | KeyCode::Char('k') => {
                    self.popup_sel = self.popup_sel.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let count = theme_matches(self).len();
                    if self.popup_sel + 1 < count {
                        self.popup_sel += 1;
                    }
                }
                KeyCode::Backspace => {
                    self.theme_query.pop();
                    self.popup_sel = 0;
                }
                KeyCode::Enter => {
                    let matches = theme_matches(self);
                    if let Some(index) = matches.get(self.popup_sel) {
                        let selected = self.theme_choices[*index].clone();
                        self.cfg.theme.name = if selected == "auto" {
                            String::new()
                        } else {
                            selected.clone()
                        };
                        self.theme = Theme::load(&self.cfg.theme);
                        match save_config(&self.cfg) {
                            Ok(()) => self.set_status(format!("theme: {}", selected)),
                            Err(error) => self.set_status(format!("theme not saved: {}", error)),
                        }
                    }
                    self.popup = Popup::None;
                }
                KeyCode::Char(ch) if !ch.is_control() => {
                    self.theme_query.push(ch);
                    self.popup_sel = 0;
                }
                _ => {}
            },
            Popup::Publish => match k.code {
                KeyCode::Esc => self.popup = Popup::None,
                KeyCode::Char('1') => match copy_to_clipboard(&report::benchmark_markdown(self)) {
                    Ok(()) => self.set_status("clean benchmark copied".into()),
                    Err(error) => self.set_status(format!("copy failed: {}", error)),
                },
                KeyCode::Char('2') => match report::save_private_report(self) {
                    Ok(report_id) => self.set_status(format!(
                        "private editable report pack saved | id {report_id}"
                    )),
                    Err(error) => self.set_status(format!("save failed: {}", error)),
                },
                KeyCode::Char('3') => match report::capture(self)
                    .and_then(|bundle| report::render_json(&bundle))
                    .and_then(|json| copy_to_clipboard(&json))
                {
                    Ok(()) => self.set_status("checked benchmark JSON copied".into()),
                    Err(error) => self.set_status(format!("copy failed: {error}")),
                },
                _ => {}
            },
            Popup::Benchmarks => match k.code {
                KeyCode::Esc => self.popup = Popup::None,
                KeyCode::Up | KeyCode::Char('k') => {
                    self.popup_sel = self.popup_sel.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let count = benchmark_recipes(self).len();
                    if self.popup_sel + 1 < count {
                        self.popup_sel += 1;
                    }
                }
                KeyCode::Enter => {
                    if let Some(recipe) = benchmark_recipes(self).get(self.popup_sel).cloned() {
                        self.start_benchmark_plan(recipe);
                    }
                    self.popup = Popup::None;
                }
                _ => {}
            },
            Popup::Models => match k.code {
                KeyCode::Esc => self.popup = Popup::None,
                KeyCode::Left | KeyCode::BackTab => {
                    self.model_tab = self.model_tab.previous();
                    self.popup_sel = 0;
                }
                KeyCode::Right | KeyCode::Tab => {
                    self.model_tab = self.model_tab.next();
                    self.popup_sel = 0;
                }
                KeyCode::Char('1') => {
                    self.model_tab = ModelTab::Local;
                    self.popup_sel = 0;
                }
                KeyCode::Char('2') => {
                    self.model_tab = ModelTab::HuggingFace;
                    self.popup_sel = 0;
                    if !self.huggingface.busy() {
                        self.check_huggingface();
                    }
                }
                KeyCode::Char('3') => {
                    self.model_tab = ModelTab::LocalAi;
                    self.popup_sel = 0;
                }
                KeyCode::Char('r') if self.model_tab == ModelTab::HuggingFace => {
                    self.check_huggingface();
                }
                KeyCode::Char('r') if self.model_tab == ModelTab::LocalAi => {
                    self.refresh_local_ai();
                }
                KeyCode::Char('s') if self.model_tab == ModelTab::HuggingFace => {
                    if self.huggingface.restore_starters() {
                        self.popup_sel = 0;
                        self.check_huggingface();
                    }
                }
                KeyCode::Char('f') if self.model_tab == ModelTab::LocalAi => {
                    self.search_huggingface_for_local_ai_selection();
                }
                KeyCode::Char('h') if self.model_tab == ModelTab::LocalAi => {
                    self.model_tab = ModelTab::HuggingFace;
                    self.popup_sel = 0;
                    if !self.huggingface.busy() {
                        self.check_huggingface();
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.popup_sel = self.popup_sel.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let count = match self.model_tab {
                        ModelTab::Local => self.server.available.len(),
                        ModelTab::HuggingFace => self.huggingface.entries().len(),
                        ModelTab::LocalAi => self
                            .local_ai
                            .reading_for(&self.chip, self.total_mem_gb)
                            .map(|reading| reading.recommendations.len())
                            .unwrap_or(0),
                    };
                    if self.popup_sel + 1 < count {
                        self.popup_sel += 1;
                    }
                }
                KeyCode::Enter => match self.model_tab {
                    ModelTab::Local => {
                        if let Some(choice) = self.server.available.get(self.popup_sel).cloned() {
                            self.choose_model(choice);
                            self.popup = Popup::None;
                        }
                    }
                    ModelTab::HuggingFace => self.use_huggingface_selection(),
                    ModelTab::LocalAi => self.copy_local_ai_selection(),
                },
                _ => {}
            },
            Popup::Connect => match k.code {
                KeyCode::Esc => self.popup = Popup::None,
                KeyCode::Up | KeyCode::Char('k') => {
                    self.popup_sel = self.popup_sel.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let count = connection_matches(self).len();
                    if self.popup_sel + 1 < count {
                        self.popup_sel += 1;
                    }
                }
                KeyCode::Char('m') => {
                    let choices = connection_model_choices(self);
                    self.popup_sel = choices
                        .iter()
                        .position(|model| model == &self.connect_model)
                        .unwrap_or(0);
                    self.popup = Popup::ConnectModels;
                }
                KeyCode::Char('f') => {
                    let matches = connection_matches(self);
                    let snippets = harness_snippets(&self.connect_model, connection_port(self));
                    if let Some(index) = matches.get(self.popup_sel) {
                        let name = snippets[*index].0.to_string();
                        if !self.connect_favorites.remove(&name) {
                            self.connect_favorites.insert(name.clone());
                            self.set_status(format!("{} added to connections", name));
                        } else {
                            self.set_status(format!("{} removed from connections", name));
                        }
                        if let Err(error) = save_connection_favorites(&self.connect_favorites) {
                            self.set_status(format!("favorite change not saved: {}", error));
                        }
                    }
                }
                KeyCode::Backspace => {
                    self.connect_query.pop();
                    self.popup_sel = 0;
                }
                KeyCode::Char(ch) if !ch.is_control() => {
                    self.connect_query.push(ch);
                    self.popup_sel = 0;
                }
                KeyCode::Enter => {
                    let matches = connection_matches(self);
                    let snippets = harness_snippets(&self.connect_model, connection_port(self));
                    if let Some(index) = matches.get(self.popup_sel) {
                        let (name, text) = &snippets[*index];
                        match copy_to_clipboard(text) {
                            Ok(()) => self.set_status(format!("{} config copied", name)),
                            Err(e) => self.set_status(format!("copy failed: {}", e)),
                        }
                    }
                    self.popup = Popup::None;
                }
                _ => {}
            },
            Popup::ConnectModels => match k.code {
                KeyCode::Esc => self.popup = Popup::Connect,
                KeyCode::Up | KeyCode::Char('k') => {
                    self.popup_sel = self.popup_sel.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let count = connection_model_choices(self).len();
                    if self.popup_sel + 1 < count {
                        self.popup_sel += 1;
                    }
                }
                KeyCode::Enter => {
                    let choices = connection_model_choices(self);
                    if let Some(model) = choices.get(self.popup_sel).cloned() {
                        self.connect_model = model.clone();
                        self.set_status(format!("connections will use {}", model));
                    }
                    self.popup = Popup::Connect;
                    self.popup_sel = 0;
                }
                _ => {}
            },
            Popup::None => match k.code {
                KeyCode::Char('q') if self.expanded_panel.is_some() => self.collapse_panel(),
                KeyCode::Char('q') if self.screen == Screen::Home => return InputControl::Quit,
                KeyCode::Char('q') => self.show_screen(Screen::Home),
                KeyCode::Esc if self.expanded_panel.is_some() => self.collapse_panel(),
                KeyCode::Esc => {
                    self.show_screen(Screen::Home);
                    self.popup = Popup::None;
                }
                KeyCode::Tab => self.cycle_focus(false),
                KeyCode::BackTab => self.cycle_focus(true),
                KeyCode::Char('j') | KeyCode::Down
                    if self.expanded_pane == ExpandedPane::Guide
                        && self.expanded_panel.is_some() =>
                {
                    self.select_panel_action(false);
                }
                KeyCode::Char('k') | KeyCode::Up
                    if self.expanded_pane == ExpandedPane::Guide
                        && self.expanded_panel.is_some() =>
                {
                    self.select_panel_action(true);
                }
                KeyCode::Enter
                    if self.expanded_pane == ExpandedPane::Guide
                        && self.expanded_panel.is_some() =>
                {
                    apply_selected_panel_action(self);
                }
                KeyCode::Char('j') | KeyCode::Down
                    if self.expanded_pane == ExpandedPane::Content
                        && matches!(
                            self.expanded_panel,
                            Some(FocusPanel::Stages | FocusPanel::History)
                        ) =>
                {
                    self.select_request(false);
                }
                KeyCode::Char('k') | KeyCode::Up
                    if self.expanded_pane == ExpandedPane::Content
                        && matches!(
                            self.expanded_panel,
                            Some(FocusPanel::Stages | FocusPanel::History)
                        ) =>
                {
                    self.select_request(true);
                }
                KeyCode::Char('j') | KeyCode::Down
                    if self.expanded_pane == ExpandedPane::Content
                        && self.expanded_panel == Some(FocusPanel::Pressure) =>
                {
                    move_process_selection(self, false);
                }
                KeyCode::Char('k') | KeyCode::Up
                    if self.expanded_pane == ExpandedPane::Content
                        && self.expanded_panel == Some(FocusPanel::Pressure) =>
                {
                    move_process_selection(self, true);
                }
                KeyCode::Enter
                    if self.expanded_panel.is_none() && self.selected_panel().is_some() =>
                {
                    self.expand_selected_panel();
                }
                KeyCode::Char('/') | KeyCode::Char('k')
                    if k.code == KeyCode::Char('/')
                        || k.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    self.command_query.clear();
                    self.command_sel = 0;
                    self.popup = Popup::Command;
                }
                KeyCode::Char('1') => self.show_screen(Screen::Home),
                KeyCode::Char('2') => self.show_screen(Screen::Measure),
                KeyCode::Char('3') => self.show_screen(Screen::System),
                KeyCode::Char('4') => self.show_screen(Screen::Learn),
                KeyCode::Char('5') => self.show_screen(Screen::Customize),
                KeyCode::Char('6') => self.show_screen(Screen::Bloat),
                KeyCode::Char('b') => self.start_benchmark(false),
                KeyCode::Char('B') => self.start_benchmark(true),
                KeyCode::Char('r') => {
                    self.popup = Popup::Benchmarks;
                    self.popup_sel = 0;
                }
                KeyCode::Char('?') => {
                    self.show_screen(Screen::Learn);
                    self.popup = Popup::None;
                }
                KeyCode::Char('p') => {
                    self.popup = Popup::Publish;
                }
                KeyCode::Char('l') => {
                    self.model_tab = ModelTab::LocalAi;
                    self.popup = Popup::Models;
                    self.popup_sel = 0;
                }
                KeyCode::Char('L') => {
                    self.model_tab = ModelTab::LocalAi;
                    self.popup = Popup::Models;
                    self.popup_sel = 0;
                    self.refresh_local_ai();
                }
                KeyCode::Char('m') => {
                    self.model_tab = ModelTab::Local;
                    self.popup = Popup::Models;
                    self.popup_sel = 0;
                }
                KeyCode::Char('h') => {
                    self.model_tab = ModelTab::HuggingFace;
                    self.popup = Popup::Models;
                    self.popup_sel = 0;
                    self.check_huggingface();
                }
                KeyCode::Char('c') => {
                    self.connect_model = connection_model_choices(self)
                        .into_iter()
                        .next()
                        .unwrap_or_else(|| "model-id".into());
                    self.connect_query.clear();
                    self.popup = Popup::Connect;
                    self.popup_sel = 0;
                }
                KeyCode::Char('s') => toggle_server(self),
                KeyCode::Char('j') | KeyCode::Down if self.screen == Screen::Bloat => {
                    let count = self.bloat.findings().len();
                    if self.bloat_sel + 1 < count {
                        self.bloat_sel += 1;
                    }
                }
                KeyCode::Char('k') | KeyCode::Up if self.screen == Screen::Bloat => {
                    self.bloat_sel = self.bloat_sel.saturating_sub(1);
                }
                KeyCode::Char('g') if self.screen == Screen::Bloat => quick_bloat_scan(self),
                KeyCode::Char('D') if self.screen == Screen::Bloat => {
                    let started = self.bloat.refresh_deep();
                    self.set_status(if started {
                        "deep local agent scan started".into()
                    } else {
                        "a Bloat scan is already running".into()
                    });
                }
                KeyCode::Char('d') if self.screen == Screen::Bloat => {
                    self.try_remove_selected_bloat();
                }
                KeyCode::Char('j') | KeyCode::Down if self.screen == Screen::Learn => {
                    let count = learn::TOPIC_COUNT;
                    if self.learn_sel + 1 < count {
                        self.learn_sel += 1;
                    }
                }
                KeyCode::Char('k') | KeyCode::Up if self.screen == Screen::Learn => {
                    self.learn_sel = self.learn_sel.saturating_sub(1);
                }
                KeyCode::Char('j') | KeyCode::Down if self.screen == Screen::Customize => {
                    self.settings_sel = (self.settings_sel + 1).min(11);
                }
                KeyCode::Char('k') | KeyCode::Up if self.screen == Screen::Customize => {
                    self.settings_sel = self.settings_sel.saturating_sub(1);
                }
                KeyCode::Enter if self.screen == Screen::Customize => {
                    match self.settings_sel {
                        0 => {
                            self.theme_query.clear();
                            self.popup_sel = 0;
                            self.popup = Popup::Themes;
                        }
                        1 => self.cycle_visualization_profile(),
                        2 => {
                            self.cfg.layout.density = match self.cfg.layout.density.as_str() {
                                "compact" => "standard",
                                "standard" => "expanded",
                                _ => "compact",
                            }
                            .into();
                        }
                        3 => {
                            self.cfg.layout.default_view =
                                match self.cfg.layout.default_view.as_str() {
                                    "home" => "measure",
                                    "measure" => "system",
                                    "system" => "learn",
                                    "learn" => "bloat",
                                    "bloat" => "customize",
                                    _ => "home",
                                }
                                .into();
                        }
                        4 => {
                            self.popup = Popup::Panels;
                            self.popup_sel = 0;
                        }
                        5 => self.cfg.observability.cycle_focus(),
                        6 => {
                            self.cfg.observability.cycle_history_samples();
                            self.trim_observability_history();
                        }
                        7 => {
                            self.cfg.observability.cycle_request_retention();
                            self.trim_request_history();
                        }
                        8 => self.cfg.intro.enabled = !self.cfg.intro.enabled,
                        9 => {
                            self.cfg.intro.motion = match self.cfg.intro.motion.as_str() {
                                "full" => "reduced",
                                "reduced" => "none",
                                _ => "full",
                            }
                            .into();
                        }
                        10 => {
                            self.cfg.intro.sound = if self.cfg.intro.sound == "off" {
                                "tokoro"
                            } else {
                                "off"
                            }
                            .into();
                        }
                        11 => self.start_onboarding(),
                        _ => {}
                    }
                    if !matches!(
                        self.popup,
                        Popup::Themes | Popup::Panels | Popup::Onboarding
                    ) {
                        if let Err(error) = save_config(&self.cfg) {
                            self.set_status(format!("setup not saved: {}", error));
                        } else {
                            self.set_status("setup saved".into());
                        }
                    }
                }
                KeyCode::Char('P') if self.screen == Screen::Customize => {
                    self.popup = Popup::Panels;
                    self.popup_sel = 0;
                }
                KeyCode::Char('j') | KeyCode::Down
                    if self.screen == Screen::System
                        && self.selected_panel() == Some(FocusPanel::Pressure) =>
                {
                    move_process_selection(self, false);
                }
                KeyCode::Char('k') | KeyCode::Up
                    if self.screen == Screen::System
                        && self.selected_panel() == Some(FocusPanel::Pressure) =>
                {
                    move_process_selection(self, true);
                }
                KeyCode::Char('x')
                    if self.screen == Screen::System
                        && (self.selected_panel() == Some(FocusPanel::Pressure)
                            || self.expanded_panel == Some(FocusPanel::Pressure)) =>
                {
                    self.try_kill_selected();
                }
                _ => {}
            },
        }
        InputControl::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let mut config = crate::Config::default();
        config.bloat.scan_project = false;
        App::new(config)
    }

    #[test]
    fn typed_navigation_uses_the_same_screen_model() {
        let mut app = app();
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('6'), KeyModifiers::NONE)),
            InputControl::Continue
        );
        assert_eq!(app.screen, Screen::Bloat);
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.screen, Screen::Home);
    }

    #[test]
    fn walkthrough_is_short_selectable_and_skippable_from_every_step() {
        let mut app = app();
        app.start_onboarding();
        assert_eq!(app.popup, Popup::Onboarding);
        assert_eq!(app.onboarding_step, 0);

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.onboarding_step, 1);
        assert_eq!(app.onboarding_sel, 1);

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.onboarding_step, 2);
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
            InputControl::Quit
        );
    }

    #[test]
    fn command_palette_is_a_state_transition_not_a_terminal_side_effect() {
        let mut app = app();
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert_eq!(app.popup, Popup::Command);
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.popup, Popup::None);
    }

    #[test]
    fn setup_opens_a_searchable_theme_picker() {
        let mut app = app();
        app.screen = Screen::Customize;
        app.settings_sel = 0;
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.popup, Popup::Themes);
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        assert_eq!(app.theme_query, "n");
    }

    #[test]
    fn sourced_recommendation_key_opens_its_model_tab() {
        let mut app = app();
        app.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE));
        assert_eq!(app.popup, Popup::Models);
        assert_eq!(app.model_tab, ModelTab::LocalAi);
    }

    #[test]
    fn tab_focuses_panels_and_enter_opens_the_selected_panel() {
        let mut app = app();
        app.show_screen(Screen::Measure);
        assert_eq!(app.selected_panel(), Some(crate::FocusPanel::Performance));

        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.selected_panel(), Some(crate::FocusPanel::Streams));

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.expanded_panel, Some(crate::FocusPanel::Streams));
        assert_eq!(app.expanded_pane, crate::ExpandedPane::Content);

        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.expanded_panel, Some(crate::FocusPanel::Streams));
        assert_eq!(app.expanded_pane, crate::ExpandedPane::Guide);

        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.expanded_panel, Some(crate::FocusPanel::Stages));
        assert_eq!(app.expanded_pane, crate::ExpandedPane::Content);

        app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert_eq!(app.expanded_panel, Some(crate::FocusPanel::Streams));
        assert_eq!(app.expanded_pane, crate::ExpandedPane::Guide);

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.expanded_panel, None);
        assert_eq!(app.screen, Screen::Measure);
    }

    #[test]
    fn expanded_action_rows_are_selectable_and_executable() {
        let mut app = app();
        app.panel_sel = 3;
        app.expand_selected_panel();
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

        assert_eq!(app.expanded_pane, crate::ExpandedPane::Guide);
        assert_eq!(app.expanded_action_sel, 0);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.popup, Popup::Models);
    }

    #[test]
    fn pressure_actions_can_pause_and_resume_the_live_process_list() {
        let mut app = app();
        app.show_screen(Screen::System);
        app.panel_sel = 1;
        app.expand_selected_panel();
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.interference.paused);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!app.interference.paused);
    }

    #[test]
    fn expanded_focus_wraps_through_both_panes_and_visible_panels() {
        let mut app = app();
        app.show_screen(Screen::Measure);
        app.expand_selected_panel();

        app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert_eq!(app.expanded_panel, Some(crate::FocusPanel::History));
        assert_eq!(app.expanded_pane, crate::ExpandedPane::Guide);

        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.expanded_panel, Some(crate::FocusPanel::Performance));
        assert_eq!(app.expanded_pane, crate::ExpandedPane::Content);
    }

    #[test]
    fn panel_focus_skips_hidden_panels() {
        let mut app = app();
        app.cfg.layout.hidden_panels.push("streams".into());
        app.show_screen(Screen::Measure);

        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.selected_panel(), Some(crate::FocusPanel::Stages));
    }

    #[test]
    fn quit_closes_an_expanded_home_panel_before_exiting() {
        let mut app = app();
        app.expand_selected_panel();

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
            InputControl::Continue
        );
        assert_eq!(app.expanded_panel, None);
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
            InputControl::Quit
        );
    }

    #[test]
    fn quit_returns_control_to_the_terminal_adapter() {
        let mut app = app();
        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
            InputControl::Quit
        );
    }

    #[test]
    fn quit_returns_to_home_before_exiting() {
        let mut app = app();
        app.screen = Screen::Measure;

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
            InputControl::Continue
        );
        assert_eq!(app.screen, Screen::Home);

        assert_eq!(
            app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
            InputControl::Quit
        );
    }
}
