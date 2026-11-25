//! Full-screen two-column dashboard. Actions run in the normal terminal, then the dashboard resumes.

use std::io::{self, Write};
use std::sync::atomic::Ordering;

use anyhow::Result;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::crossterm::{cursor, execute, terminal};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, HighlightSpacing, List, ListItem, ListState, Padding, Paragraph, Wrap};

use crate::commands::{self, RemoteState};
use crate::config::Config;
use crate::interactive::{self, Action};
use crate::{git, ui};

const PALETTE: [Color; 6] = [
    Color::Green,
    Color::Magenta,
    Color::Yellow,
    Color::Blue,
    Color::Cyan,
    Color::Red,
];
const ACCENT: Color = Color::Cyan;

enum Entry {
    Header(&'static str),
    Item {
        key: char,
        label: &'static str,
        desc: &'static str,
        action: Action,
    },
}

const fn item(key: char, label: &'static str, desc: &'static str, action: Action) -> Entry {
    Entry::Item {
        key,
        label,
        desc,
        action,
    }
}

const GIT: &[Entry] = &[
    Entry::Header("Changes"),
    item(
        's',
        "Status",
        "Show the working tree status (git status).",
        Action::Status,
    ),
    item(
        'c',
        "Commit",
        "Stage everything and commit with a message.",
        Action::Commit,
    ),
    item(
        'u',
        "Pull",
        "Pull from the upstream branch (fixes remotes first).",
        Action::Pull,
    ),
    item(
        'p',
        "Push",
        "Push the current branch (fixes remotes first, sets upstream if needed).",
        Action::Push,
    ),
    Entry::Header("History"),
    item(
        'b',
        "Branch",
        "Switch to an existing branch or create a new one.",
        Action::Branch,
    ),
    item('g', "Log", "Show the last 25 commits as a graph.", Action::Log),
    Entry::Header("Remotes"),
    item(
        'a',
        "Add remote",
        "Add a remote; owner/repo is expanded to the profile's SSH alias.",
        Action::RemoteAdd,
    ),
    item(
        'e',
        "Edit remote",
        "Change a remote URL (rewritten for the repository profile).",
        Action::RemoteEdit,
    ),
    item(
        'x',
        "Delete remote",
        "Remove a remote from this repository.",
        Action::RemoteDelete,
    ),
    Entry::Header("Identity"),
    item(
        'r',
        "Use profile here",
        "Set commit identity and SSH account for this repository.",
        Action::UseHere,
    ),
];

const ACCOUNTS: &[Entry] = &[
    Entry::Header("Overview"),
    item(
        'w',
        "Who am I",
        "Show identity, remotes and profiles in detail.",
        Action::Whoami,
    ),
    item(
        'o',
        "Clone",
        "Clone owner/repo or a URL with a chosen profile.",
        Action::Clone,
    ),
    item(
        'f',
        "Default profile",
        "Choose the profile used for new clones.",
        Action::Default,
    ),
    Entry::Header("Profiles"),
    item('n', "Add profile", "Create a new account profile.", Action::ProfileAdd),
    item(
        'm',
        "Edit profile",
        "Change name, email, username or SSH settings of a profile.",
        Action::ProfileEdit,
    ),
    item(
        'd',
        "Remove profile",
        "Delete a profile (SSH keys are kept).",
        Action::ProfileRemove,
    ),
    Entry::Header("SSH"),
    item(
        't',
        "Test",
        "Check which GitHub account each SSH key authenticates as.",
        Action::SshTest,
    ),
    item(
        'y',
        "Sync config",
        "Write gg-managed Host entries to ~/.ssh/config.",
        Action::SshSync,
    ),
    item(
        'k',
        "Generate key",
        "Create an ed25519 key for a profile.",
        Action::SshKeygen,
    ),
    item(
        'v',
        "Public key",
        "Print a profile's public key to paste into GitHub.",
        Action::SshPubkey,
    ),
    Entry::Header("Other"),
    item(
        'i',
        "Setup wizard",
        "Detect SSH hosts and create profiles step by step.",
        Action::Setup,
    ),
    item('q', "Quit", "Leave gg.", Action::Quit),
];

struct RepoInfo {
    name: String,
    branch: String,
    ahead_behind: Option<(u32, u32)>,
    dirty: usize,
    profile: Option<String>,
    author: String,
    remotes: Vec<(String, String, RemoteState)>,
}

impl RepoInfo {
    fn load(cfg: &Config) -> Option<Self> {
        let root = git::repo_root(None)?;
        let name = root.rsplit('/').next().unwrap_or(&root).to_string();
        let branch = git::get(None, &["branch", "--show-current"])
            .or_else(|| git::get(None, &["rev-parse", "--short", "HEAD"]).map(|h| format!("detached {h}")))
            .unwrap_or_else(|| "no commits".into());
        let ahead_behind = git::get(None, &["rev-list", "--left-right", "--count", "@{u}...HEAD"]).and_then(|s| {
            let mut it = s.split_whitespace().filter_map(|n| n.parse().ok());
            let behind = it.next()?;
            Some((it.next()?, behind))
        });
        let dirty = git::get(None, &["status", "--porcelain"])
            .map(|s| s.lines().count())
            .unwrap_or(0);
        let author = match (
            git::get(None, &["config", "user.name"]),
            git::get(None, &["config", "user.email"]),
        ) {
            (Some(n), Some(e)) => format!("{n} <{e}>"),
            (n, e) => n.or(e).unwrap_or_else(|| "no author configured".into()),
        };
        let remotes = git::remotes(None)
            .into_iter()
            .map(|r| {
                let url = git::get(None, &["remote", "get-url", &r]).unwrap_or_default();
                let state = commands::remote_state(cfg, &url);
                (r, url, state)
            })
            .collect();
        Some(Self {
            name,
            branch,
            ahead_behind,
            dirty,
            profile: commands::identity_profile(cfg, None),
            author,
            remotes,
        })
    }
}

struct App {
    cfg: Config,
    repo: Option<RepoInfo>,
    col: usize,
    states: [ListState; 2],
    message: Option<Line<'static>>,
}

fn columns() -> [&'static [Entry]; 2] {
    [GIT, ACCOUNTS]
}

fn first_item(entries: &[Entry]) -> usize {
    entries
        .iter()
        .position(|e| matches!(e, Entry::Item { .. }))
        .unwrap_or(0)
}

fn color_of(cfg: &Config, name: &str) -> Color {
    PALETTE[ui::profile_index(cfg, name)]
}

fn badge(cfg: &Config, name: &str) -> Span<'static> {
    Span::styled(format!("● {name}"), Style::new().fg(color_of(cfg, name)).bold())
}

impl App {
    fn new() -> Result<Self> {
        let cfg = Config::load()?;
        let repo = RepoInfo::load(&cfg);
        let mut states = [ListState::default(), ListState::default()];
        for (s, entries) in states.iter_mut().zip(columns()) {
            s.select(Some(first_item(entries)));
        }
        let col = if repo.is_some() { 0 } else { 1 };
        Ok(Self {
            cfg,
            repo,
            col,
            states,
            message: None,
        })
    }

    fn refresh(&mut self) -> Result<()> {
        self.cfg = Config::load()?;
        self.repo = RepoInfo::load(&self.cfg);
        if self.repo.is_none() {
            self.col = 1;
        }
        Ok(())
    }

    fn enabled(&self, col: usize) -> bool {
        col == 1 || self.repo.is_some()
    }

    fn selected(&self) -> Option<&'static Entry> {
        let i = self.states[self.col].selected()?;
        columns()[self.col].get(i)
    }

    fn switch(&mut self) {
        let other = 1 - self.col;
        if self.enabled(other) {
            self.col = other;
        } else {
            self.message = Some(Line::from(
                " Git commands need a repository: cd into one or clone it.".yellow(),
            ));
        }
    }

    fn step(&mut self, delta: isize) {
        let entries = columns()[self.col];
        let n = entries.len() as isize;
        let mut i = self.states[self.col].selected().unwrap_or(0) as isize;
        for _ in 0..n {
            i = (i + delta).rem_euclid(n);
            if matches!(entries[i as usize], Entry::Item { .. }) {
                break;
            }
        }
        self.states[self.col].select(Some(i as usize));
    }

    fn jump(&mut self, end: bool) {
        let entries = columns()[self.col];
        let i = if end {
            entries
                .iter()
                .rposition(|e| matches!(e, Entry::Item { .. }))
                .unwrap_or(0)
        } else {
            first_item(entries)
        };
        self.states[self.col].select(Some(i));
    }

    /// Selects the item bound to a hotkey and returns its action.
    fn hotkey(&mut self, c: char) -> Option<Action> {
        for col in 0..2 {
            for (i, e) in columns()[col].iter().enumerate() {
                if let Entry::Item { key, action, .. } = e
                    && *key == c
                {
                    if !self.enabled(col) {
                        self.message = Some(Line::from(
                            " Git commands need a repository: cd into one or clone it.".yellow(),
                        ));
                        return None;
                    }
                    self.col = col;
                    self.states[col].select(Some(i));
                    return Some(*action);
                }
            }
        }
        None
    }
}

pub fn run() -> Result<()> {
    let mut app = App::new()?;
    interactive::DASHBOARD.store(true, Ordering::SeqCst);
    let original = interactive::save_tty();
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &mut app, original);
    ratatui::restore();
    interactive::restore_tty(original);
    interactive::DASHBOARD.store(false, Ordering::SeqCst);
    result
}

fn event_loop(terminal: &mut DefaultTerminal, app: &mut App, original: Option<libc::termios>) -> Result<()> {
    loop {
        terminal.draw(|f| draw(f, app))?;
        let Event::Key(key) = event::read()? else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        let action = match handle_key(app, key) {
            Some(Action::Quit) => return Ok(()),
            Some(a) => a,
            None => continue,
        };
        let label = match app.selected() {
            Some(Entry::Item { label, .. }) => *label,
            _ => "gg",
        };
        run_suspended(terminal, app, action, label, original)?;
    }
}

fn handle_key(app: &mut App, key: KeyEvent) -> Option<Action> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Some(Action::Quit);
    }
    match key.code {
        KeyCode::Esc => Some(Action::Quit),
        KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
            app.switch();
            None
        }
        KeyCode::Up | KeyCode::Char('K') => {
            app.step(-1);
            None
        }
        KeyCode::Down | KeyCode::Char('J') => {
            app.step(1);
            None
        }
        KeyCode::Home => {
            app.jump(false);
            None
        }
        KeyCode::End => {
            app.jump(true);
            None
        }
        KeyCode::Enter => match app.selected() {
            Some(Entry::Item { action, .. }) => Some(*action),
            _ => None,
        },
        KeyCode::Char(c) => app.hotkey(c.to_ascii_lowercase()),
        _ => None,
    }
}

fn run_suspended(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    action: Action,
    label: &str,
    original: Option<libc::termios>,
) -> Result<()> {
    terminal::disable_raw_mode()?;
    interactive::restore_tty(original);
    execute!(io::stdout(), terminal::LeaveAlternateScreen, cursor::Show)?;
    println!("{}\n", ui::banner(label));

    let outcome = Config::load().and_then(|mut cfg| interactive::run_action(&mut cfg, action));
    // Child programs (ssh-keygen, git, editors) may die mid-prompt with echo turned off.
    interactive::restore_tty(original);
    let pause = match &outcome {
        Ok(shown) => *shown,
        Err(e) => {
            eprintln!("{}", ui::fail(format!("{e:#}")));
            true
        }
    };
    if pause {
        print!("\n{}", ui::dim("Press any key to return to gg…"));
        io::stdout().flush()?;
        terminal::enable_raw_mode()?;
        loop {
            if let Event::Key(k) = event::read()?
                && k.kind == KeyEventKind::Press
            {
                break;
            }
        }
    }

    execute!(io::stdout(), terminal::EnterAlternateScreen)?;
    terminal::enable_raw_mode()?;
    terminal.clear()?;
    app.refresh()?;
    app.message = Some(match outcome {
        Ok(true) => Line::from(vec![" ✓ ".green().bold(), Span::raw(format!("{label} finished"))]),
        Ok(false) => Line::from(vec![" · ".dark_gray(), format!("{label} cancelled").dark_gray()]),
        Err(e) => Line::from(vec![" ✗ ".red().bold(), Span::raw(format!("{label}: {e:#}"))]),
    });
    Ok(())
}

fn pane(title: &str, focused: bool) -> Block<'static> {
    let color = if focused { ACCENT } else { Color::DarkGray };
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(color))
        .title(Line::from(format!(" {title} ")).style(Style::new().fg(color).bold()))
        .padding(Padding::horizontal(1))
}

fn repo_lines(app: &App) -> Vec<Line<'static>> {
    let Some(r) = &app.repo else {
        return vec![
            Line::from("Not inside a Git repository".bold()),
            Line::from(vec!["new clones use ".dark_gray(), default_span(&app.cfg)]),
            Line::from("cd into a repository or press o to clone".dark_gray()),
        ];
    };
    let mut head = vec![r.name.clone().bold(), "  ⎇ ".dark_gray(), r.branch.clone().cyan()];
    if let Some((ahead, behind)) = r.ahead_behind {
        if ahead > 0 {
            head.push(format!("  ↑{ahead}").green());
        }
        if behind > 0 {
            head.push(format!("  ↓{behind}").yellow());
        }
        if ahead == 0 && behind == 0 {
            head.push("  in sync".dark_gray());
        }
    }
    head.push(if r.dirty > 0 {
        format!("  ✎ {} changed", r.dirty).yellow()
    } else {
        "  ✓ clean".green()
    });

    let who = match &r.profile {
        Some(p) => badge(&app.cfg, p),
        None => "⚠ no profile (press r)".yellow().bold(),
    };
    let mut lines = vec![
        Line::from(head),
        Line::from(vec!["as ".dark_gray(), who, "  ".into(), r.author.clone().dark_gray()]),
    ];
    if r.remotes.is_empty() {
        lines.push(Line::from("no remotes (press a to add one)".dark_gray()));
    }
    for (name, url, state) in &r.remotes {
        let tag = match state {
            RemoteState::Profile(p) => {
                let mismatch = r.profile.as_ref().is_some_and(|i| i != p);
                let mut s = vec![Span::styled(format!("✓ {p}"), Style::new().fg(color_of(&app.cfg, p)))];
                if mismatch {
                    s = vec![format!("⚠ {p} ≠ identity").yellow()];
                }
                s
            }
            RemoteState::Direct(h) => vec![format!("⚠ {h} directly").yellow()],
            RemoteState::Unmanaged => vec!["· unmanaged".dark_gray()],
        };
        let mut spans = vec![format!("{name} ").bold(), url.clone().dark_gray(), "  ".into()];
        spans.extend(tag);
        lines.push(Line::from(spans));
    }
    lines
}

fn default_span(cfg: &Config) -> Span<'static> {
    cfg.default
        .as_deref()
        .map(|d| badge(cfg, d))
        .unwrap_or_else(|| "no default".dark_gray())
}

fn profile_lines(app: &App) -> Vec<Line<'static>> {
    let cfg = &app.cfg;
    let mut lines: Vec<Line> = cfg
        .profiles
        .iter()
        .map(|(name, p)| {
            let star = if cfg.default.as_deref() == Some(name) {
                "★ ".yellow()
            } else {
                "  ".into()
            };
            let user = p.username.as_deref().map(|u| format!(" @{u}")).unwrap_or_default();
            let key = if p.key_path().exists() {
                "  ✓ key".green()
            } else {
                "  ✗ key".red()
            };
            Line::from(vec![star, badge(cfg, name), user.dark_gray(), key])
        })
        .collect();
    if lines.is_empty() {
        lines.push(Line::from("no profiles (press n)".dark_gray()));
    }
    lines
}

fn list_widget(app: &App, col: usize, title: &str) -> List<'static> {
    let focused = app.col == col;
    let enabled = app.enabled(col);
    let items: Vec<ListItem> = columns()[col]
        .iter()
        .map(|e| match e {
            Entry::Header(h) => ListItem::new(Line::from(h.to_uppercase().dark_gray().bold())),
            Entry::Item { key, label, .. } => {
                let key_style = if enabled {
                    Style::new().fg(ACCENT).bold()
                } else {
                    Style::new().dark_gray()
                };
                let label_style = if enabled {
                    Style::new()
                } else {
                    Style::new().dark_gray()
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!(" {key} "), key_style),
                    Span::styled(label.to_string(), label_style),
                ]))
            }
        })
        .collect();
    let highlight = if focused {
        Style::new().bg(ACCENT).fg(Color::Black).add_modifier(Modifier::BOLD)
    } else {
        Style::new().add_modifier(Modifier::DIM)
    };
    let mut block = pane(title, focused);
    if !enabled {
        block = block.title_bottom(Line::from(" no repository ").dark_gray());
    }
    List::new(items)
        .block(block)
        .highlight_style(highlight)
        .highlight_symbol(if focused { "▶ " } else { "  " })
        .highlight_spacing(HighlightSpacing::Always)
}

fn draw(f: &mut ratatui::Frame, app: &mut App) {
    let repo = repo_lines(app);
    let profiles = profile_lines(app);
    let top_h = (repo.len().max(profiles.len()) as u16 + 2).clamp(4, 9);
    let body_h = GIT.len().max(ACCOUNTS.len()) as u16 + 2;

    let [title, top, body, info, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(top_h),
        Constraint::Max(body_h),
        Constraint::Length(4),
        Constraint::Length(1),
    ])
    .areas(f.area());

    f.render_widget(
        Line::from(vec![
            " gg ".black().on_cyan().bold(),
            " git account switcher".cyan().bold(),
            "  ·  ".dark_gray(),
            Span::raw(if app.repo.is_some() {
                "repository"
            } else {
                "no repository"
            })
            .dark_gray(),
        ]),
        title,
    );

    let [repo_area, prof_area] =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(top);
    f.render_widget(Paragraph::new(repo).block(pane("Repository", false)), repo_area);
    f.render_widget(Paragraph::new(profiles).block(pane("Profiles", false)), prof_area);

    let [left, right] = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(body);
    let git_list = list_widget(app, 0, "Git");
    let acc_list = list_widget(app, 1, "Accounts & SSH");
    f.render_stateful_widget(git_list, left, &mut app.states[0]);
    f.render_stateful_widget(acc_list, right, &mut app.states[1]);

    draw_info(f, app, info);

    let hint =
        |k: &'static str, d: &'static str| vec![format!(" {k} ").black().on_dark_gray(), format!(" {d}  ").dark_gray()];
    let mut spans = Vec::new();
    for (k, d) in [
        ("←→", "column"),
        ("↑↓", "move"),
        ("⏎", "run"),
        ("key", "shortcut"),
        ("esc", "quit"),
    ] {
        spans.extend(hint(k, d));
    }
    f.render_widget(Line::from(spans), footer);
}

fn draw_info(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let desc = match app.selected() {
        Some(Entry::Item { desc, .. }) => *desc,
        _ => "",
    };
    let mut lines = vec![Line::from(desc.italic())];
    if let Some(m) = &app.message {
        lines.push(m.clone());
    }
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(Color::DarkGray))
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    f.render_widget(block, area);
    // Show the most recent message when only one line fits.
    let skip = lines.len().saturating_sub(inner.height as usize);
    f.render_widget(Paragraph::new(lines.split_off(skip)).wrap(Wrap { trim: true }), inner);
}
