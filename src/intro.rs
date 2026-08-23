use crate::{
    platform,
    settings::{expand_home, IntroConfig, Theme},
};
use ratatui::{
    layout::{Alignment, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
    Frame,
};
use std::{
    fs,
    io::{self, IsTerminal},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const BUILTIN_SOUND: &[u8] = include_bytes!("../assets/intro/tokoro-ident.wav");
const MAX_CUSTOM_BYTES: u64 = 64 * 1024;
const MAX_CUSTOM_FRAMES: usize = 32;
const MAX_CUSTOM_WIDTH: usize = 160;
const MAX_CUSTOM_HEIGHT: usize = 48;

#[derive(Clone)]
enum Visual {
    CursorThreshold,
    Custom(Vec<Vec<String>>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Motion {
    Full,
    Reduced,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Search(usize),
    Opening,
    Final,
    Custom(usize),
}

pub(crate) struct Session {
    started: Instant,
    duration: Duration,
    motion: Motion,
    slogan: String,
    visual: Visual,
}

impl Session {
    pub(crate) fn new(config: &IntroConfig) -> Self {
        let visual = if config.style == "custom" {
            load_custom_frames(&config.frames_path)
                .map(Visual::Custom)
                .unwrap_or(Visual::CursorThreshold)
        } else {
            Visual::CursorThreshold
        };
        let motion = match config.motion.as_str() {
            "reduced" => Motion::Reduced,
            "none" => Motion::None,
            _ => Motion::Full,
        };
        Self {
            started: Instant::now(),
            duration: Duration::from_millis(config.duration_ms.clamp(250, 5_000)),
            motion,
            slogan: config
                .slogan
                .trim()
                .chars()
                .filter(|character| !character.is_control())
                .take(80)
                .collect(),
            visual,
        }
    }

    pub(crate) fn finished(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.started) >= self.duration
    }

    pub(crate) fn frame_key(&self, now: Instant) -> usize {
        match self.phase_at(now.saturating_duration_since(self.started)) {
            Phase::Search(step) => step,
            Phase::Opening => 7,
            Phase::Final => 8,
            Phase::Custom(index) => 100 + index,
        }
    }

    pub(crate) fn render(&self, frame: &mut Frame<'_>, theme: &Theme) {
        self.render_elapsed(
            frame,
            theme,
            Instant::now().saturating_duration_since(self.started),
        );
    }

    fn render_elapsed(&self, frame: &mut Frame<'_>, theme: &Theme, elapsed: Duration) {
        let area = frame.area();
        frame.render_widget(Clear, area);
        let mut background = Style::default().fg(theme.fg);
        if let Some(color) = theme.bg {
            background = background.bg(color);
        }
        frame.render_widget(Block::default().style(background), area);

        if area.width < 28 || area.height < 9 {
            frame.render_widget(
                Paragraph::new("[_] TOKORO")
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(theme.fg)),
                centered(area, area.width.min(18), 1),
            );
            return;
        }

        let phase = self.phase_at(elapsed);
        match &self.visual {
            Visual::CursorThreshold => {
                render_cursor_threshold(frame, area, phase, &self.slogan, theme)
            }
            Visual::Custom(frames) => {
                let index = match phase {
                    Phase::Custom(index) => index.min(frames.len().saturating_sub(1)),
                    _ => frames.len().saturating_sub(1),
                };
                render_custom(frame, area, &frames[index], &self.slogan, theme);
            }
        }
    }

    fn phase_at(&self, elapsed: Duration) -> Phase {
        let ratio = elapsed.as_secs_f64() / self.duration.as_secs_f64().max(0.001);
        if let Visual::Custom(frames) = &self.visual {
            let last = frames.len().saturating_sub(1);
            let index = match self.motion {
                Motion::None => last,
                Motion::Reduced => {
                    if ratio < 0.5 {
                        0
                    } else {
                        last
                    }
                }
                Motion::Full => ((ratio * frames.len() as f64) as usize).min(last),
            };
            return Phase::Custom(index);
        }

        match self.motion {
            Motion::None => Phase::Final,
            Motion::Reduced => {
                if ratio < 0.45 {
                    Phase::Opening
                } else {
                    Phase::Final
                }
            }
            Motion::Full => {
                const CLAIMS: [f64; 6] = [0.065, 0.139, 0.213, 0.287, 0.361, 0.435];
                if ratio >= 0.680 {
                    Phase::Final
                } else if ratio >= 0.510 {
                    Phase::Opening
                } else {
                    Phase::Search(CLAIMS.iter().filter(|claim| ratio >= **claim).count())
                }
            }
        }
    }
}

pub(crate) fn should_run(config: &IntroConfig) -> bool {
    config.enabled
        && config.duration_ms > 0
        && io::stdin().is_terminal()
        && io::stdout().is_terminal()
        && std::env::var("TERM").map_or(true, |term| term != "dumb")
        && std::env::var_os("CI").is_none()
}

pub(crate) fn play_sound(config: &IntroConfig) {
    let sound = config.sound.trim();
    if sound.is_empty() || sound == "off" {
        return;
    }
    let path = if matches!(sound, "tokoro" | "freedom") {
        match write_builtin_sound() {
            Some(path) => path,
            None => return,
        }
    } else {
        let path = expand_home(sound);
        if !path.is_file() {
            return;
        }
        path
    };
    thread::spawn(move || {
        let _ = playback_command(&path).and_then(|mut child| child.wait().map(|_| ()));
    });
}

fn write_builtin_sound() -> Option<PathBuf> {
    let path = platform::cache_home()
        .join("tokoro")
        .join("audio")
        .join("tokoro-ident.wav");
    if path
        .metadata()
        .ok()
        .is_some_and(|metadata| metadata.len() == BUILTIN_SOUND.len() as u64)
    {
        return Some(path);
    }
    fs::create_dir_all(path.parent()?).ok()?;
    fs::write(&path, BUILTIN_SOUND).ok()?;
    Some(path)
}

#[cfg(target_os = "macos")]
fn playback_command(path: &Path) -> io::Result<std::process::Child> {
    Command::new("afplay")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

#[cfg(target_os = "linux")]
fn playback_command(path: &Path) -> io::Result<std::process::Child> {
    let player = ["pw-play", "paplay", "aplay"]
        .into_iter()
        .find(|command| platform::command_exists(command))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no audio player"))?;
    Command::new(player)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

#[cfg(target_os = "windows")]
fn playback_command(path: &Path) -> io::Result<std::process::Child> {
    let player = if platform::command_exists("powershell") {
        "powershell"
    } else {
        "pwsh"
    };
    let escaped = path.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$p = New-Object System.Media.SoundPlayer '{}'; $p.PlaySync()",
        escaped
    );
    Command::new(player)
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

fn load_custom_frames(configured_path: &str) -> Option<Vec<Vec<String>>> {
    if configured_path.trim().is_empty() {
        return None;
    }
    let path = expand_home(configured_path);
    let metadata = path.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MAX_CUSTOM_BYTES {
        return None;
    }
    parse_custom_frames(&fs::read_to_string(path).ok()?).ok()
}

fn parse_custom_frames(content: &str) -> Result<Vec<Vec<String>>, &'static str> {
    let mut frames = Vec::new();
    let mut current = Vec::new();
    for line in content.lines() {
        if line == "---" {
            if current.is_empty() {
                return Err("empty frame");
            }
            frames.push(std::mem::take(&mut current));
        } else {
            if !line
                .chars()
                .all(|character| character == ' ' || character.is_ascii_graphic())
            {
                return Err("frames must contain printable ASCII");
            }
            current.push(line.to_string());
        }
    }
    if !current.is_empty() {
        frames.push(current);
    }
    if frames.is_empty() || frames.len() > MAX_CUSTOM_FRAMES {
        return Err("invalid frame count");
    }
    let height = frames[0].len();
    let width = frames[0]
        .first()
        .map(|line| line.chars().count())
        .unwrap_or(0);
    if height == 0 || height > MAX_CUSTOM_HEIGHT || width == 0 || width > MAX_CUSTOM_WIDTH {
        return Err("invalid frame dimensions");
    }
    if frames.iter().any(|frame| {
        frame.len() != height || frame.iter().any(|line| line.chars().count() != width)
    }) {
        return Err("all frames must have equal dimensions");
    }
    Ok(frames)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Debris,
    Foreground,
    Cursor,
}

#[derive(Clone, Copy)]
struct Cell {
    character: char,
    role: Role,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            character: ' ',
            role: Role::Debris,
        }
    }
}

fn render_cursor_threshold(
    frame: &mut Frame<'_>,
    area: Rect,
    phase: Phase,
    slogan: &str,
    theme: &Theme,
) {
    let width = usize::from(area.width.saturating_sub(4).min(76));
    let available_height = area
        .height
        .saturating_sub(if slogan.is_empty() { 2 } else { 4 });
    let height = usize::from(available_height.min(15));
    let mut grid = debris_grid(width, height);
    match phase {
        Phase::Search(step) => draw_search(&mut grid, step),
        Phase::Opening => draw_threshold(&mut grid, false),
        Phase::Final | Phase::Custom(_) => draw_threshold(&mut grid, true),
    }
    let lines = styled_lines(&grid, theme);
    let grid_area = centered(area, width as u16, height as u16);
    frame.render_widget(Paragraph::new(lines), grid_area);
    render_slogan(frame, area, grid_area, slogan, theme);
}

fn render_custom(
    frame: &mut Frame<'_>,
    area: Rect,
    source: &[String],
    slogan: &str,
    theme: &Theme,
) {
    let width = source
        .first()
        .map(|line| line.chars().count())
        .unwrap_or(0)
        .min(usize::from(area.width.saturating_sub(2)));
    let height = source.len().min(usize::from(area.height.saturating_sub(2)));
    let lines = source
        .iter()
        .take(height)
        .map(|line| Line::styled(line.chars().take(width).collect::<String>(), theme.fg))
        .collect::<Vec<_>>();
    let grid_area = centered(area, width as u16, height as u16);
    frame.render_widget(Paragraph::new(lines), grid_area);
    render_slogan(frame, area, grid_area, slogan, theme);
}

fn render_slogan(frame: &mut Frame<'_>, area: Rect, grid: Rect, slogan: &str, theme: &Theme) {
    if slogan.is_empty() || grid.bottom().saturating_add(2) >= area.bottom() {
        return;
    }
    let slogan_area = Rect::new(area.x, grid.bottom().saturating_add(1), area.width, 1);
    frame.render_widget(
        Paragraph::new(slogan)
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme.dim)),
        slogan_area,
    );
}

fn debris_grid(width: usize, height: usize) -> Vec<Vec<Cell>> {
    let mut grid = vec![vec![Cell::default(); width]; height];
    let debris = ['.', ':', '+', '0', '1', '_', '/', '\\'];
    let mut seed = (width as u64)
        .wrapping_mul(419)
        .wrapping_add((height as u64).wrapping_mul(97))
        .wrapping_add(92_601);
    for row in grid.iter_mut().take(height.saturating_sub(2)).skip(1) {
        for cell in row.iter_mut().take(width.saturating_sub(1)).skip(1) {
            let sample = random(&mut seed);
            if sample % 1_000 < 125 {
                cell.character = debris[(random(&mut seed) as usize) % debris.len()];
            }
        }
    }
    grid
}

fn random(seed: &mut u64) -> u32 {
    *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (*seed >> 16) as u32
}

fn source_positions(width: usize, height: usize) -> [(usize, usize); 6] {
    const RATIOS: [(f64, f64); 6] = [
        (0.11, 0.19),
        (0.84, 0.16),
        (0.24, 0.76),
        (0.87, 0.72),
        (0.39, 0.34),
        (0.68, 0.65),
    ];
    RATIOS.map(|(x, y)| {
        (
            ((x * width as f64) as usize).clamp(1, width.saturating_sub(2)),
            ((y * height as f64) as usize).clamp(1, height.saturating_sub(3)),
        )
    })
}

fn draw_search(grid: &mut [Vec<Cell>], step: usize) {
    let height = grid.len();
    let width = grid.first().map(Vec::len).unwrap_or(0);
    let letters = ['T', 'O', 'K', 'O', 'R', 'O'];
    for (index, (column, row)) in source_positions(width, height).into_iter().enumerate() {
        let cell = &mut grid[row][column];
        if index < step {
            cell.character = ' ';
        } else if index == step && step < letters.len() {
            cell.character = '#';
            cell.role = Role::Cursor;
        } else {
            cell.character = letters[index];
            cell.role = Role::Foreground;
        }
    }

    let claimed = letters
        .into_iter()
        .enumerate()
        .flat_map(|(index, letter)| [if index < step { letter } else { '_' }, ' '])
        .take(11)
        .collect::<String>();
    let track = format!("[ {claimed} ]");
    write_text(
        grid,
        height / 2,
        width.saturating_sub(track.len()) / 2,
        &track,
        Role::Foreground,
    );
}

fn draw_threshold(grid: &mut [Vec<Cell>], show_word: bool) {
    let height = grid.len();
    let width = grid.first().map(Vec::len).unwrap_or(0);
    let door_width = ((width as f64 * 0.20) as usize).clamp(9, 15) | 1;
    let left = width.saturating_sub(door_width) / 2;
    let right = left + door_width.saturating_sub(1);
    let top = 1;
    let floor = height.saturating_sub(3);

    for (row_index, row) in grid.iter_mut().enumerate() {
        for cell in row.iter_mut().take(right).skip(left + 1) {
            cell.character = ' ';
        }
        if (top..=floor).contains(&row_index) {
            if left > 0 {
                row[left - 1].character = ' ';
            }
            if right + 1 < width {
                row[right + 1].character = ' ';
            }
        }
    }

    write_text(
        grid,
        top,
        left,
        &format!("+{}+", "-".repeat(door_width - 2)),
        Role::Foreground,
    );
    for row in grid.iter_mut().take(floor).skip(top + 1) {
        row[left] = Cell {
            character: '|',
            role: Role::Foreground,
        };
        row[right] = Cell {
            character: '|',
            role: Role::Foreground,
        };
    }
    let floor_row = &mut grid[floor];
    for cell in floor_row.iter_mut().take(left) {
        *cell = Cell {
            character: '-',
            role: Role::Foreground,
        };
    }
    for cell in floor_row.iter_mut().take(width).skip(right + 1) {
        *cell = Cell {
            character: '-',
            role: Role::Foreground,
        };
    }
    floor_row[left] = Cell {
        character: '+',
        role: Role::Foreground,
    };
    floor_row[right] = Cell {
        character: '+',
        role: Role::Foreground,
    };

    if show_word {
        let word = if width < 44 { "TOKORO" } else { "T O K O R O" };
        write_text(
            grid,
            (top + floor) / 2,
            width.saturating_sub(word.len()) / 2,
            word,
            Role::Cursor,
        );
    } else {
        grid[(top + floor) / 2][width / 2] = Cell {
            character: '#',
            role: Role::Cursor,
        };
    }
}

fn write_text(grid: &mut [Vec<Cell>], row: usize, start: usize, text: &str, role: Role) {
    let Some(target) = grid.get_mut(row) else {
        return;
    };
    for (offset, character) in text.chars().enumerate() {
        if let Some(cell) = target.get_mut(start + offset) {
            *cell = Cell { character, role };
        }
    }
}

fn styled_lines<'a>(grid: &[Vec<Cell>], theme: &'a Theme) -> Vec<Line<'a>> {
    grid.iter()
        .map(|row| {
            let mut spans = Vec::new();
            let mut run = String::new();
            let mut current_role = row.first().map(|cell| cell.role).unwrap_or(Role::Debris);
            for cell in row {
                if cell.role != current_role && !run.is_empty() {
                    spans.push(Span::styled(
                        std::mem::take(&mut run),
                        style_for(current_role, theme),
                    ));
                    current_role = cell.role;
                }
                run.push(cell.character);
            }
            if !run.is_empty() {
                spans.push(Span::styled(run, style_for(current_role, theme)));
            }
            Line::from(spans)
        })
        .collect()
}

fn style_for(role: Role, theme: &Theme) -> Style {
    match role {
        Role::Debris => Style::default().fg(theme.dim),
        Role::Foreground => Style::default().fg(theme.fg),
        Role::Cursor => Style::default().fg(theme.accent),
    }
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{IntroConfig, ThemeConfig};
    use ratatui::{backend::TestBackend, Terminal};

    fn rendered(config: IntroConfig, width: u16, height: u16, elapsed: Duration) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let session = Session::new(&config);
        let theme = Theme::load(&ThemeConfig::default());
        terminal
            .draw(|frame| session.render_elapsed(frame, &theme, elapsed))
            .expect("intro should render");
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn final_frame_centers_tokoro_at_normal_and_small_sizes() {
        let config = IntroConfig {
            motion: "none".into(),
            ..IntroConfig::default()
        };
        assert!(rendered(config.clone(), 80, 24, Duration::ZERO).contains("T O K O R O"));
        assert!(rendered(config, 20, 5, Duration::ZERO).contains("TOKORO"));
    }

    #[test]
    fn custom_frames_require_equal_printable_ascii_dimensions() {
        let valid = "[   ]\n  #  \n---\n[ T ]\n     \n";
        assert_eq!(parse_custom_frames(valid).expect("valid frames").len(), 2);
        assert!(parse_custom_frames("[ ]\n---\n[  ]\n").is_err());
        assert!(parse_custom_frames("ok\n\u{0007}x\n").is_err());
    }

    #[test]
    fn motion_phases_finish_on_the_threshold() {
        let config = IntroConfig::default();
        let session = Session::new(&config);
        assert_eq!(session.phase_at(Duration::ZERO), Phase::Search(0));
        assert_eq!(session.phase_at(Duration::from_millis(1_060)), Phase::Final);
    }
}
