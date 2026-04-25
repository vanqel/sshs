// provides the `random` method
use crate::docker_containers::{render_docker_table, resolve_json_docker_containers, Container};
use crate::keychain::remove_from_keychain;
use crate::{searchable::Searchable, ssh};
use anyhow::Result;
use clap::builder::TypedValueParser;
use crossterm::event::{MouseEvent, MouseEventKind};
use crossterm::{
    cursor::{Hide, Show},
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use itertools::Itertools;
#[allow(clippy::wildcard_imports)]
use ratatui::{prelude::*, widgets::*};
use std::collections::HashMap;
use std::hash::Hash;
use std::{
    cell::RefCell,
    cmp::{max, min},
    io,
    rc::Rc,
};
use strum_macros::Display;
use style::palette::tailwind;
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;
use unicode_width::UnicodeWidthStr;

const INFO_TEXT: &str = "(Esc) quit | (↑) move up | (↓) move down | (enter) select | (SHIFT+D) remove password from keychain  | (SHIFT+C) resolve docker containers \
| (Tab) Switch state | (SHIFT+T) Enable/Disable docker table";
const CONTAINERS_INFO: &str = "docker ps --format json";

#[derive(Clone)]
pub struct AppConfig {
    pub config_paths: Vec<String>,

    pub search_filter: Option<String>,
    pub sort_by_name: bool,
    pub show_proxy_command: bool,

    pub command_template: String,
    pub command_template_on_session_start: Option<String>,
    pub command_template_on_session_end: Option<String>,
    pub exit_after_ssh_session_ends: bool,
    pub command_template_no_password: String,
}
#[derive(Display)]
pub enum StateActive {
    DOCKER,
    HOSTS,
}
pub struct App {
    config: AppConfig,

    search: Input,

    pub table_state_hosts: TableState,
    pub table_state_containers: TableState,
    pub active_state: StateActive,
    pub enabled_docker_table: bool,
    pub hosts: Searchable<ssh::Host>,
    pub docker_containers: HashMap<ssh::Host, Vec<Container>>,
    pub table_columns_constraints: Vec<Constraint>,

    pub palette: tailwind::Palette,
    pub tick: u64,
}

#[derive(PartialEq)]
enum AppKeyAction {
    Ok,
    Stop,
    Continue,
}

impl App {
    /// # Errors
    ///
    /// Will return `Err` if the SSH configuration file cannot be parsed.
    pub fn new(config: &AppConfig) -> Result<App> {
        let mut hosts = Vec::new();

        for path in &config.config_paths {
            let parsed_hosts = match ssh::parse_config(path) {
                Ok(hosts) => hosts,
                Err(err) => {
                    if path == "/etc/ssh/ssh_config" {
                        if let ssh::ParseConfigError::Io(io_err) = &err {
                            // Ignore missing system-wide SSH configuration file
                            if io_err.kind() == std::io::ErrorKind::NotFound {
                                continue;
                            }
                        }
                    }

                    anyhow::bail!("Failed to parse SSH configuration file: {err:?}");
                }
            };

            hosts.extend(parsed_hosts);
        }

        if config.sort_by_name {
            hosts.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        }

        let search_input = config.search_filter.clone().unwrap_or_default();
        let matcher = SkimMatcherV2::default();

        let mut app = App {
            config: config.clone(),
            tick: 0,
            search: search_input.clone().into(),

            table_state_hosts: TableState::default().with_selected(0),
            table_state_containers: TableState::default().with_selected(0),
            enabled_docker_table: false,
            active_state: StateActive::HOSTS,
            table_columns_constraints: Vec::new(),
            palette: tailwind::BLUE,
            docker_containers: HashMap::new(),
            hosts: Searchable::new(
                hosts,
                &search_input,
                move |host: &&ssh::Host, search_value: &str| -> bool {
                    search_value.is_empty()
                        || matcher.fuzzy_match(&host.name, search_value).is_some()
                        || matcher
                            .fuzzy_match(&host.destination, search_value)
                            .is_some()
                        || matcher
                            .fuzzy_match(&host.description, search_value)
                            .is_some()
                        || matcher.fuzzy_match(&host.aliases, search_value).is_some()
                },
            ),
        };
        app.calculate_table_columns_constraints();

        Ok(app)
    }

    /// # Errors
    ///
    /// Will return `Err` if the terminal cannot be configured.
    pub fn start(&mut self) -> Result<()> {
        let stdout = io::stdout().lock();
        let backend = CrosstermBackend::new(stdout);
        let terminal = Rc::new(RefCell::new(Terminal::new(backend)?));

        setup_terminal(&terminal)?;

        // create app and run it
        let res = self.run(&terminal);

        restore_terminal(&terminal)?;

        if let Err(err) = res {
            println!("{err:?}");
        }

        Ok(())
    }

    fn run<B>(&mut self, terminal: &Rc<RefCell<Terminal<B>>>) -> Result<()>
    where
        B: Backend + std::io::Write,
    {
        loop {
            terminal.borrow_mut().draw(|f| ui(f, self))?;

            let ev = event::read()?;

            self.tick += 1;

            if self.tick > 100 {
                self.tick = 0;
            }

            if let Event::Mouse(mouse) = ev {
                if self.tick % 3 != 0 {
                    continue;
                }
                let action = self.on_key_press_mouse(mouse)?;
                if action == AppKeyAction::Ok {
                    continue;
                }
            }

            if let Event::Key(key) = ev {
                if key.kind == KeyEventKind::Press {
                    let action = self.on_key_press(terminal, key)?;
                    match action {
                        AppKeyAction::Ok => continue,
                        AppKeyAction::Stop => break,
                        AppKeyAction::Continue => {}
                    }
                }

                self.search.handle_event(&ev);
                self.hosts.search(self.search.value());

                let selected = self.table_state_hosts.selected().unwrap_or(0);
                if selected >= self.hosts.len() {
                    self.table_state_hosts.select(Some(match self.hosts.len() {
                        0 => 0,
                        _ => self.hosts.len() - 1,
                    }));
                }
            }
        }

        Ok(())
    }

    fn on_key_press_mouse(&mut self, key: MouseEvent) -> Result<AppKeyAction> {
        #[allow(clippy::enum_glob_use)]
        use MouseEventKind::*;

        let active_table_state = match self.active_state {
            StateActive::DOCKER => &mut self.table_state_containers,
            StateActive::HOSTS => &mut self.table_state_hosts,
        };

        match key.kind {
            ScrollUp => active_table_state.select_next(),
            ScrollDown => active_table_state.select_previous(),
            _ => return Ok(AppKeyAction::Continue),
        }

        Ok(AppKeyAction::Ok)
    }
    fn on_key_press<B>(
        &mut self,
        terminal: &Rc<RefCell<Terminal<B>>>,
        key: KeyEvent,
    ) -> Result<AppKeyAction>
    where
        B: Backend + std::io::Write,
    {
        #[allow(clippy::enum_glob_use)]
        use KeyCode::*;

        let active_table_state = match self.active_state {
            StateActive::DOCKER => &mut self.table_state_containers,
            StateActive::HOSTS => &mut self.table_state_hosts,
        };

        let is_shift_pressed = key.modifiers.contains(KeyModifiers::SHIFT);

        if is_shift_pressed {
            if key.code == Char('D') || key.code == Char('d') {
                let selected = active_table_state.selected().unwrap_or(0);
                let host: &ssh::Host = &self.hosts[selected];
                let result = remove_from_keychain(host.name.as_str());
                return Ok(AppKeyAction::Ok);
            }

            if key.code == Char('T') || key.code == Char('t') {
                self.enabled_docker_table = !self.enabled_docker_table;
                return Ok(AppKeyAction::Ok);
            }

            if key.code == Char('C') || key.code == Char('c') {
                self.enabled_docker_table = true;

                let selected = self.table_state_hosts.selected().unwrap_or(0);
                let host: ssh::Host = self.hosts[selected].clone();

                let result = host.run_command_template_on_ssh(CONTAINERS_INFO.to_string().as_str());

                if result.is_ok() {
                    let arr_containers = resolve_json_docker_containers(result.unwrap());

                    if arr_containers.is_err() {
                        println!(
                            "Error resolving ssh containers {}",
                            arr_containers.err().unwrap()
                        );
                        return Ok(AppKeyAction::Continue);
                    }
                    let cont = arr_containers.unwrap();

                    self.docker_containers.insert(host, cont);
                }
                return Ok(AppKeyAction::Ok);
            }
        }

        match key.code {
            Tab => match self.active_state {
                StateActive::DOCKER => {
                    self.active_state = StateActive::HOSTS;
                }
                StateActive::HOSTS => {
                    if self.enabled_docker_table {
                        self.active_state = StateActive::DOCKER;
                    }
                }
            },
            Esc => return Ok(AppKeyAction::Stop),
            Down => active_table_state.select_next(),
            Up => active_table_state.select_previous(),
            Home => active_table_state.select(Some(0)),
            End => active_table_state.select(Some(self.hosts.len() - 1)),
            PageDown => {
                let i = active_table_state.selected().unwrap_or(0);
                let target = min(i.saturating_add(21), self.hosts.len() - 1);

                active_table_state.select(Some(target));
            }
            PageUp => {
                let i = active_table_state.selected().unwrap_or(0);
                let target = max(i.saturating_sub(21), 0);

                active_table_state.select(Some(target));
            }
            Enter => {
                let selected = self.table_state_hosts.selected().unwrap_or(0);

                if selected >= self.hosts.len() {
                    return Ok(AppKeyAction::Ok);
                }

                let host: &ssh::Host = &self.hosts[selected];

                restore_terminal(terminal).expect("Failed to restore terminal");

                if let Some(template) = &self.config.command_template_on_session_start {
                    host.run_command_template(template.as_str())?;
                }

                host.run_connect_command_template()?;

                if let Some(template) = &self.config.command_template_on_session_end {
                    host.run_command_template(template.as_str())?;
                }

                setup_terminal(terminal).expect("Failed to setup terminal");

                if self.config.exit_after_ssh_session_ends {
                    return Ok(AppKeyAction::Stop);
                }
            }
            _ => return Ok(AppKeyAction::Continue),
        }

        Ok(AppKeyAction::Ok)
    }

    fn on_key_press_ctrl(&mut self, key: KeyEvent) -> AppKeyAction {
        #[allow(clippy::enum_glob_use)]
        use KeyCode::*;

        match key.code {
            Char('c') => AppKeyAction::Stop,
            Char('j' | 'n') => {
                self.next();
                AppKeyAction::Ok
            }
            Char('k' | 'p') => {
                self.previous();
                AppKeyAction::Ok
            }
            _ => AppKeyAction::Continue,
        }
    }

    fn next(&mut self) {
        let i = match self.table_state_hosts.selected() {
            Some(i) => {
                if self.hosts.is_empty() || i >= self.hosts.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.table_state_hosts.select(Some(i));
    }

    fn previous(&mut self) {
        let i = match self.table_state_hosts.selected() {
            Some(i) => {
                if self.hosts.is_empty() {
                    0
                } else if i == 0 {
                    self.hosts.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.table_state_hosts.select(Some(i));
    }

    fn calculate_table_columns_constraints(&mut self) {
        let mut lengths = Vec::new();

        let name_len = self
            .hosts
            .iter()
            .map(|d| d.name.as_str())
            .map(UnicodeWidthStr::width)
            .max()
            .unwrap_or(0);
        lengths.push(name_len);

        let description_len = self
            .hosts
            .iter()
            .map(|d| d.description.as_str())
            .map(UnicodeWidthStr::width)
            .max()
            .unwrap_or(0);
        lengths.push(description_len);

        let aliases_len = self
            .hosts
            .non_filtered_iter()
            .map(|d| d.aliases.as_str())
            .map(UnicodeWidthStr::width)
            .max()
            .unwrap_or(0);
        lengths.push(aliases_len);

        let user_len = self
            .hosts
            .non_filtered_iter()
            .map(|d| match &d.user {
                Some(user) => user.as_str(),
                None => "",
            })
            .map(UnicodeWidthStr::width)
            .max()
            .unwrap_or(0);
        lengths.push(user_len);

        let destination_len = self
            .hosts
            .non_filtered_iter()
            .map(|d| d.destination.as_str())
            .map(UnicodeWidthStr::width)
            .max()
            .unwrap_or(0);
        lengths.push(destination_len);

        let port_len = self
            .hosts
            .non_filtered_iter()
            .map(|d| match &d.port {
                Some(port) => port.as_str(),
                None => "",
            })
            .map(UnicodeWidthStr::width)
            .max()
            .unwrap_or(0);
        lengths.push(port_len);

        if self.config.show_proxy_command {
            let proxy_len = self
                .hosts
                .non_filtered_iter()
                .map(|d| match &d.proxy_command {
                    Some(proxy) => proxy.as_str(),
                    None => "",
                })
                .map(UnicodeWidthStr::width)
                .max()
                .unwrap_or(0);
            lengths.push(proxy_len);
        }

        let mut new_constraints = vec![
            // +1 for padding
            Constraint::Length(u16::try_from(lengths[0]).unwrap_or_default() + 1),
        ];
        new_constraints.extend(
            lengths
                .iter()
                .skip(1)
                .map(|len| Constraint::Min(u16::try_from(*len).unwrap_or_default() + 1)),
        );
    }
}

fn setup_terminal<B>(terminal: &Rc<RefCell<Terminal<B>>>) -> Result<()>
where
    B: Backend + std::io::Write,
{
    let mut terminal = terminal.borrow_mut();

    // setup terminal
    enable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        Hide,
        EnterAlternateScreen,
        EnableMouseCapture
    )?;

    Ok(())
}

fn restore_terminal<B>(terminal: &Rc<RefCell<Terminal<B>>>) -> Result<()>
where
    B: Backend + std::io::Write,
{
    let mut terminal = terminal.borrow_mut();
    terminal.clear()?;

    // restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        Show,
        LeaveAlternateScreen,
        DisableMouseCapture,
    )?;

    Ok(())
}

fn ui(f: &mut Frame, app: &mut App) {
    let layout = if app.enabled_docker_table {
        let layout_docker = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Min(2),
            Constraint::Length(3),
        ])
        .split(f.area());

        render_searchbar(f, app, layout_docker[0]);

        render_table(f, app, layout_docker[1]);
        render_docker_table(f, app, layout_docker[2]);

        render_footer(f, app, layout_docker[3]);
        layout_docker
    } else {
        let layout_no_docker = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(f.area());

        render_searchbar(f, app, layout_no_docker[0]);

        render_table(f, app, layout_no_docker[1]);

        render_footer(f, app, layout_no_docker[2]);

        layout_no_docker
    };

    let mut cursor_position = layout[0].as_position();
    cursor_position.x += u16::try_from(app.search.cursor()).unwrap_or_default() + 4;
    cursor_position.y += 1;

    f.set_cursor_position(cursor_position);
}

fn render_searchbar(f: &mut Frame, app: &mut App, area: Rect) {
    let info_footer = Paragraph::new(Line::from(app.search.value())).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(app.palette.c400))
            .border_type(BorderType::Rounded)
            .padding(Padding::horizontal(3)),
    );
    f.render_widget(info_footer, area);
}

fn render_table(f: &mut Frame, app: &mut App, area: Rect) {
    let header_style = Style::default().fg(tailwind::CYAN.c500);
    let selected_style = Style::default().add_modifier(Modifier::REVERSED);

    let mut header_names = vec![
        "Name",
        "Aliases",
        "Description",
        "User",
        "Destination",
        "Port",
    ];
    if app.config.show_proxy_command {
        header_names.push("Proxy");
    }

    let header = header_names
        .iter()
        .copied()
        .map(Cell::from)
        .collect::<Row>()
        .style(header_style)
        .height(1);

    let rows = app.hosts.iter().map(|host| {
        let mut content = vec![
            host.name.clone(),
            host.aliases.clone(),
            host.description.clone(),
            host.user.clone().unwrap_or_default(),
            host.destination.clone(),
            host.port.clone().unwrap_or_default(),
        ];
        if app.config.show_proxy_command {
            content.push(host.proxy_command.clone().unwrap_or_default());
        }

        content
            .iter()
            .map(|content| Cell::from(Text::from(content.to_string())))
            .collect::<Row>()
    });

    let bar = " █ ";
    let t = Table::new(rows, app.table_columns_constraints.clone())
        .header(header)
        .row_highlight_style(selected_style)
        .highlight_symbol(Text::from(vec![
            "".into(),
            bar.into(),
            bar.into(),
            "".into(),
        ]))
        .highlight_spacing(HighlightSpacing::Always)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::new().fg(app.palette.c400))
                .border_type(BorderType::Rounded),
        );

    f.render_stateful_widget(t, area, &mut app.table_state_hosts);
}

fn render_footer(f: &mut Frame, app: &mut App, area: Rect) {
    let info: String =
        "CURRENT_STATE: ".to_string() + &app.active_state.to_string().as_str() + " | " + INFO_TEXT;

    let mut text = Text::default();
    text.extend(vec![Line::from(info.as_str())]);

    let info_footer = Paragraph::new(text).centered().block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(app.palette.c400))
            .border_type(BorderType::Rounded),
    );
    f.render_widget(info_footer, area);
}
