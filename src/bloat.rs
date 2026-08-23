use crate::platform;
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Instant,
};

const MAX_SCAN_DEPTH: usize = 7;
const MAX_SCAN_FILES: usize = 5_000;
const MAX_SCAN_DIRECTORIES: usize = 1_000;
const MAX_READ_BYTES: u64 = 256 * 1024;
const MAX_ARTIFACT_ENTRIES: usize = 100_000;
const MAX_FINDINGS: usize = 160;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confidence {
    Deterministic,
    Advisory,
}

impl Confidence {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Deterministic => "deterministic",
            Self::Advisory => "advisory",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    SafeToRemove,
    Review,
}

impl Disposition {
    pub const fn label(self) -> &'static str {
        match self {
            Self::SafeToRemove => "SAFE REMOVE",
            Self::Review => "REVIEW",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub code: &'static str,
    pub title: String,
    pub evidence: String,
    pub action: String,
    pub confidence: Confidence,
    pub disposition: Disposition,
    pub reclaim_bytes: u64,
    relative_path: Option<PathBuf>,
}

impl Finding {
    pub fn relative_path(&self) -> Option<&Path> {
        self.relative_path.as_deref()
    }

    pub const fn can_remove(&self) -> bool {
        matches!(self.disposition, Disposition::SafeToRemove) && self.relative_path.is_some()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RuntimeSnapshot {
    pub loaded_endpoints: usize,
    pub swap_mib: f64,
    pub prompt_tokens: u32,
    pub prefix_hits: u64,
    pub prefix_partial_hits: u64,
    pub probe_ports: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScanDepth {
    #[default]
    Quick,
    Deep,
}

impl ScanDepth {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Quick => "quick",
            Self::Deep => "deep",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Report {
    pub depth: ScanDepth,
    pub findings: Vec<Finding>,
    pub scanned_files: usize,
    pub scanned_directories: usize,
    pub truncated: bool,
}

pub enum ScannerEvent {
    ScanCompleted { findings: usize },
    RemovalCompleted(Result<String, String>),
}

pub struct Scanner {
    root: PathBuf,
    report: Report,
    runtime: RuntimeSnapshot,
    scan_rx: Option<mpsc::Receiver<Report>>,
    requested_depth: ScanDepth,
    removal_rx: Option<mpsc::Receiver<Result<String, String>>>,
    completed_at: Option<Instant>,
}

impl Scanner {
    pub fn new(root: PathBuf) -> Self {
        let mut scanner = Self {
            root,
            report: Report::default(),
            runtime: RuntimeSnapshot::default(),
            scan_rx: None,
            requested_depth: ScanDepth::Quick,
            removal_rx: None,
            completed_at: None,
        };
        scanner.refresh();
        scanner
    }

    pub fn runtime_only(root: PathBuf) -> Self {
        Self {
            root,
            report: Report::default(),
            runtime: RuntimeSnapshot::default(),
            scan_rx: None,
            requested_depth: ScanDepth::Quick,
            removal_rx: None,
            completed_at: None,
        }
    }

    pub fn refresh(&mut self) -> bool {
        self.refresh_at(ScanDepth::Quick)
    }

    pub fn refresh_deep(&mut self) -> bool {
        self.refresh_at(ScanDepth::Deep)
    }

    fn refresh_at(&mut self, depth: ScanDepth) -> bool {
        if self.scan_rx.is_some() {
            return false;
        }
        self.requested_depth = depth;
        let root = self.root.clone();
        let (tx, rx) = mpsc::channel();
        self.scan_rx = Some(rx);
        thread::spawn(move || {
            let _ = tx.send(scan_project(&root, depth));
        });
        true
    }

    pub fn update_runtime(&mut self, snapshot: RuntimeSnapshot) {
        self.runtime = snapshot;
    }

    pub fn scanning(&self) -> bool {
        self.scan_rx.is_some()
    }

    pub fn removing(&self) -> bool {
        self.removal_rx.is_some()
    }

    pub fn findings(&self) -> Vec<Finding> {
        let mut findings = runtime_findings(self.runtime);
        findings.extend(self.report.findings.clone());
        findings.sort_by_key(|finding| match finding.disposition {
            Disposition::SafeToRemove => 0,
            Disposition::Review => 1,
        });
        findings
    }

    pub fn scan_age_seconds(&self) -> Option<u64> {
        self.completed_at
            .map(|completed| completed.elapsed().as_secs())
    }

    pub fn scan_summary(&self) -> String {
        if self.scanning() {
            format!("{} scan running", self.requested_depth.label())
        } else {
            format!(
                "{} | {} files / {} directories{}",
                self.report.depth.label(),
                self.report.scanned_files,
                self.report.scanned_directories,
                if self.report.truncated {
                    " / bounded"
                } else {
                    ""
                }
            )
        }
    }

    pub fn remove(&mut self, finding: &Finding) -> Result<(), String> {
        if self.removal_rx.is_some() {
            return Err("a removal is already running".into());
        }
        if !finding.can_remove() {
            return Err("this finding is review-only".into());
        }
        let relative = finding
            .relative_path
            .clone()
            .ok_or_else(|| "the artifact path is unavailable".to_string())?;
        let root = self.root.clone();
        let (tx, rx) = mpsc::channel();
        self.removal_rx = Some(rx);
        thread::spawn(move || {
            let result = remove_safe_artifact(&root, &relative);
            let _ = tx.send(result);
        });
        Ok(())
    }

    pub fn poll(&mut self) -> Vec<ScannerEvent> {
        let mut events = Vec::new();
        let scan = match self.scan_rx.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(report)) => Some(report),
            Some(Err(mpsc::TryRecvError::Disconnected)) => Some(Report::default()),
            _ => None,
        };
        if let Some(report) = scan {
            self.scan_rx = None;
            let count = report.findings.len();
            self.report = report;
            self.completed_at = Some(Instant::now());
            events.push(ScannerEvent::ScanCompleted { findings: count });
        }

        let removal = match self.removal_rx.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(result)) => Some(result),
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                Some(Err("the removal worker stopped".into()))
            }
            _ => None,
        };
        if let Some(result) = removal {
            self.removal_rx = None;
            events.push(ScannerEvent::RemovalCompleted(result));
            self.refresh();
        }
        events
    }
}

fn runtime_findings(snapshot: RuntimeSnapshot) -> Vec<Finding> {
    let mut findings = Vec::new();
    if snapshot.loaded_endpoints > 1 {
        findings.push(review(
            "RUNTIME_DUPLICATE_ENDPOINTS",
            "more than one inference endpoint is loaded",
            format!(
                "{} loaded endpoints may duplicate weights",
                snapshot.loaded_endpoints
            ),
            "stop the duplicate before comparing speed or memory",
            Confidence::Deterministic,
        ));
    }
    if snapshot.swap_mib > 100.0 {
        findings.push(review(
            "RUNTIME_SWAP",
            "swap is active",
            format!("{:.0} MiB swap is currently in use", snapshot.swap_mib),
            "reduce context or stop idle model servers before benchmarking",
            Confidence::Deterministic,
        ));
    }
    if snapshot.prompt_tokens >= 8_000 {
        findings.push(review(
            "RUNTIME_CONTEXT",
            "the active context is large",
            format!(
                "{} prompt tokens are in the current request",
                snapshot.prompt_tokens
            ),
            "summarise or build a compact evidence bundle before synthesis",
            Confidence::Advisory,
        ));
    }
    if snapshot.prefix_partial_hits >= 3 && snapshot.prefix_hits == 0 {
        findings.push(review(
            "RUNTIME_PREFIX_REUSE",
            "prefix reuse is not paying back",
            format!(
                "{} partial hits and no complete hits were reported",
                snapshot.prefix_partial_hits
            ),
            "keep stable system and context prefixes together, then measure again",
            Confidence::Advisory,
        ));
    }
    if snapshot.probe_ports > 4 {
        findings.push(review(
            "RUNTIME_PROBE_BREADTH",
            "the endpoint probe list is wider than the common path",
            format!("{} localhost ports are polled", snapshot.probe_ports),
            "remove runtimes you do not use from Tokoro setup",
            Confidence::Deterministic,
        ));
    }
    findings
}

pub fn quick_scan(root: &Path) -> Report {
    scan_project(root, ScanDepth::Quick)
}

pub fn deep_scan(root: &Path) -> Report {
    scan_project(root, ScanDepth::Deep)
}

fn scan_project(root: &Path, depth: ScanDepth) -> Report {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut report = Report {
        depth,
        ..Report::default()
    };
    let mut queue = vec![(root.clone(), 0_usize)];
    let mut head = 0_usize;
    let mut instruction_files: Vec<(PathBuf, String)> = Vec::new();
    let mut skills: Vec<(String, PathBuf, String)> = Vec::new();
    let mut agent_launches = 0_usize;
    let mut verification_markers = 0_usize;

    while head < queue.len() && report.findings.len() < MAX_FINDINGS {
        if report.scanned_directories >= MAX_SCAN_DIRECTORIES
            || report.scanned_files >= MAX_SCAN_FILES
        {
            report.truncated = true;
            break;
        }
        let (directory, depth) = queue[head].clone();
        head += 1;
        report.scanned_directories += 1;
        let mut entries = match fs::read_dir(&directory) {
            Ok(entries) => entries.filter_map(Result::ok).collect::<Vec<_>>(),
            Err(_) => continue,
        };
        entries.sort_by_key(|entry| entry.file_name());

        for entry in entries {
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = path.strip_prefix(&root).unwrap_or(&path).to_path_buf();

            if metadata.is_dir() {
                if is_ignored_directory(&name) {
                    continue;
                }
                if safe_directory(&path, &name) {
                    let bytes = directory_size(&path);
                    report.findings.push(safe_remove(
                        "GENERATED_DIRECTORY",
                        format!("generated artifact: {}", display_relative(&relative)),
                        format!("reconstructible directory uses {}", format_bytes(bytes)),
                        "press d twice in Bloat to remove it",
                        bytes,
                        relative,
                    ));
                    continue;
                }
                if depth < MAX_SCAN_DEPTH {
                    queue.push((path, depth + 1));
                }
                continue;
            }

            if !metadata.is_file() {
                continue;
            }
            report.scanned_files += 1;
            if safe_file(&name) {
                report.findings.push(safe_remove(
                    "GENERATED_FILE",
                    format!("generated artifact: {}", display_relative(&relative)),
                    format!("reconstructible file uses {}", format_bytes(metadata.len())),
                    "press d twice in Bloat to remove it",
                    metadata.len(),
                    relative.clone(),
                ));
            }
            if transient_work_artifact(&name) {
                report.findings.push(review(
                    "TRANSIENT_WORK_ARTIFACT",
                    format!("possible transient agent artifact: {}", display_relative(&relative)),
                    "the filename describes work state rather than product documentation".into(),
                    "review its unique content, then remove it manually if the product no longer needs it",
                    Confidence::Advisory,
                ));
            }
            if credential_shaped(&name) || metadata.len() > MAX_READ_BYTES {
                continue;
            }
            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            if source_extension(extension) {
                if let Ok(text) = fs::read_to_string(&path) {
                    let lines = text.lines().count();
                    if lines >= 2_500 {
                        report.findings.push(review(
                            "SOURCE_CONCENTRATION",
                            format!("source concentration: {}", display_relative(&relative)),
                            format!("{} lines live behind one source-file interface", lines),
                            "deepen around a domain seam; do not split by arbitrary line count",
                            Confidence::Advisory,
                        ));
                    }
                    if inside_scripts(&relative) {
                        agent_launches += count_agent_launches(&text);
                        verification_markers += count_verification_markers(&text);
                    }
                }
            }
            if instruction_name(&name) {
                if let Ok(text) = fs::read_to_string(&path) {
                    let lines = text.lines().count();
                    if metadata.len() >= 32 * 1024 || lines >= 500 {
                        report.findings.push(review(
                            "INSTRUCTION_CONTEXT_SIZE",
                            format!("large instruction context: {}", display_relative(&relative)),
                            format!("{} lines / {}", lines, format_bytes(metadata.len())),
                            "move detail behind explicit references and keep first-turn rules short",
                            Confidence::Advisory,
                        ));
                    }
                    instruction_files.push((relative.clone(), text));
                }
            }
            if name == "SKILL.md" && skill_root(&relative) {
                if let Ok(text) = fs::read_to_string(&path) {
                    let skill_name = frontmatter_value(&text, "name")
                        .unwrap_or_else(|| display_relative(&relative));
                    skills.push((skill_name, relative, normalize_text(&text)));
                }
            }
        }
    }

    add_duplicate_instruction_findings(&mut report, &instruction_files);
    add_duplicate_skill_findings(&mut report, &skills);
    if depth == ScanDepth::Deep {
        add_deep_agent_findings(&mut report);
    }
    if agent_launches > 1 && verification_markers == 0 {
        report.findings.push(review(
            "AGENT_LAUNCH_WITHOUT_GATE",
            "repeated agent launches have no detected return gate",
            format!(
                "{} literal agent launch markers and no build/test/gate marker",
                agent_launches
            ),
            "add one deterministic verification command after agent results return",
            Confidence::Advisory,
        ));
    }
    if report.truncated {
        report.findings.push(review(
            "SCAN_BOUNDED",
            "the project scan reached its safety bound",
            format!(
                "stopped at {} files / {} directories",
                report.scanned_files, report.scanned_directories
            ),
            "narrow the project root instead of increasing the bound blindly",
            Confidence::Deterministic,
        ));
    }
    report.findings.truncate(MAX_FINDINGS);
    report
}

fn add_deep_agent_findings(report: &mut Report) {
    let home = platform::home_dir();
    scan_codex_config(report, &home.join(".codex/config.toml"));
    scan_claude_settings(report, &home.join(".claude/settings.json"));
    scan_global_skills(report, &home);
    scan_usage_shapes(report, &home);
}

fn scan_codex_config(report: &mut Report, path: &Path) {
    let Some(text) = read_named_text(path) else {
        return;
    };
    let Ok(value) = toml::from_str::<toml::Value>(&text) else {
        return;
    };
    if let Some(effort) = value
        .get("model_reasoning_effort")
        .and_then(toml::Value::as_str)
        .filter(|effort| matches!(*effort, "high" | "xhigh"))
    {
        report.findings.push(review(
            "GLOBAL_EFFORT_PIN",
            "a global reasoning-effort pin may over-provision routine work",
            format!("Codex default reasoning effort is {effort}"),
            "route effort by task and verify accepted output before keeping the change",
            Confidence::Advisory,
        ));
    }
    let servers = value
        .get("mcp_servers")
        .and_then(toml::Value::as_table)
        .map_or(0, toml::map::Map::len);
    if servers >= 6 {
        report.findings.push(review(
            "MCP_BREADTH",
            "the global MCP registry is broad",
            format!("{} named MCP servers are configured", servers),
            "keep a small task-specific toolset; count alone does not prove loaded schema cost",
            Confidence::Advisory,
        ));
    }
}

fn scan_claude_settings(report: &mut Report, path: &Path) {
    let Some(text) = read_named_text(path) else {
        return;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return;
    };
    let plugins = value
        .get("enabledPlugins")
        .and_then(serde_json::Value::as_object)
        .map(|plugins| {
            plugins
                .values()
                .filter(|enabled| enabled.as_bool() == Some(true))
                .count()
        })
        .unwrap_or(0);
    if plugins >= 12 {
        report.findings.push(review(
            "PLUGIN_BREADTH",
            "many agent plugins are enabled at once",
            format!("{} Claude plugins are enabled", plugins),
            "disable plugins outside the common workflow, then compare task outcomes",
            Confidence::Advisory,
        ));
    }
    let (hooks, synchronous) = value
        .get("hooks")
        .map(count_hook_leaves)
        .unwrap_or_default();
    if synchronous > 0 {
        report.findings.push(review(
            "SYNCHRONOUS_HOOKS",
            "agent lifecycle hooks can block the critical path",
            format!(
                "{} of {} hook leaves are not explicitly async",
                synchronous, hooks
            ),
            "make non-gating hooks asynchronous; keep verification gates synchronous",
            Confidence::Advisory,
        ));
    }
}

fn count_hook_leaves(value: &serde_json::Value) -> (usize, usize) {
    match value {
        serde_json::Value::Array(values) => values.iter().fold((0, 0), |acc, value| {
            let next = count_hook_leaves(value);
            (acc.0 + next.0, acc.1 + next.1)
        }),
        serde_json::Value::Object(object) => {
            let is_leaf = object.contains_key("command") || object.contains_key("type");
            if is_leaf {
                (
                    1,
                    usize::from(
                        object.get("async").and_then(|value| value.as_bool()) != Some(true),
                    ),
                )
            } else {
                object.values().fold((0, 0), |acc, value| {
                    let next = count_hook_leaves(value);
                    (acc.0 + next.0, acc.1 + next.1)
                })
            }
        }
        _ => (0, 0),
    }
}

fn scan_global_skills(report: &mut Report, home: &Path) {
    let roots = [
        home.join(".claude/skills"),
        home.join(".claude/plugins/cache"),
        home.join(".pi/agent/skills"),
    ];
    let mut skills = Vec::new();
    for root in roots {
        collect_skills(&root, &mut skills);
        if skills.len() >= 500 {
            break;
        }
    }
    if skills.len() >= 80 {
        report.findings.push(review(
            "SKILL_REGISTRY_BREADTH",
            "the installed skill registry is broad",
            format!("{} local SKILL.md registrations were found", skills.len()),
            "remove unused registrations only after confirming each harness search root",
            Confidence::Advisory,
        ));
    }
    add_duplicate_skill_findings(report, &skills);
}

fn collect_skills(root: &Path, skills: &mut Vec<(String, PathBuf, String)>) {
    if !root.is_dir() || skills.len() >= 500 {
        return;
    }
    let mut queue = vec![(root.to_path_buf(), 0_usize)];
    let mut head = 0_usize;
    while head < queue.len() && skills.len() < 500 {
        let (directory, depth) = queue[head].clone();
        head += 1;
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() && depth < 5 {
                queue.push((path, depth + 1));
            } else if metadata.is_file()
                && path.file_name().and_then(|name| name.to_str()) == Some("SKILL.md")
                && metadata.len() <= MAX_READ_BYTES
            {
                if let Ok(text) = fs::read_to_string(&path) {
                    let name =
                        frontmatter_value(&text, "name").unwrap_or_else(|| "unnamed-skill".into());
                    skills.push((name, PathBuf::new(), normalize_text(&text)));
                }
            }
        }
    }
}

#[derive(Default)]
struct UsageShape {
    files: usize,
    records: usize,
    max_input_tokens: u64,
    duplicate_message_ids: usize,
    repeated_tool_names: usize,
    tool_events: usize,
    seen_message_ids: HashSet<String>,
    previous_tool: Option<String>,
}

fn scan_usage_shapes(report: &mut Report, home: &Path) {
    let roots = [
        home.join(".pi/agent/sessions"),
        home.join(".codex/sessions"),
        home.join(".claude/projects"),
    ];
    let mut files = Vec::new();
    for root in roots {
        collect_jsonl_files(&root, &mut files);
    }
    files.sort_by_key(|path| {
        std::cmp::Reverse(
            fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .ok(),
        )
    });
    files.truncate(24);

    let mut shape = UsageShape::default();
    for path in files {
        scan_usage_file(&path, &mut shape);
        if shape.records >= 20_000 {
            break;
        }
    }
    if shape.max_input_tokens >= 100_000 {
        report.findings.push(review(
            "SESSION_CONTEXT_BLOAT",
            "recent agent usage exposes very large input counters",
            format!(
                "maximum observed input counter was {} tokens across {} bounded files",
                shape.max_input_tokens, shape.files
            ),
            "compare full history with a compact evidence bundle on the same accepted task",
            Confidence::Advisory,
        ));
    }
    if shape.duplicate_message_ids > 0 {
        report.findings.push(review(
            "DUPLICATE_SESSION_EVENTS",
            "recent agent history contains duplicate message identifiers",
            format!(
                "{} duplicate identifiers across {} bounded records",
                shape.duplicate_message_ids, shape.records
            ),
            "deduplicate ingestion before attributing token or tool volume",
            Confidence::Deterministic,
        ));
    }
    if shape.repeated_tool_names >= 5 {
        report.findings.push(review(
            "REPEATED_TOOL_SEQUENCE",
            "recent agent history contains repeated adjacent tool classes",
            format!(
                "{} repeated adjacent names across {} tool events",
                shape.repeated_tool_names, shape.tool_events
            ),
            "inspect the task locally and add a stop condition or deterministic workflow step",
            Confidence::Advisory,
        ));
    }
}

fn collect_jsonl_files(root: &Path, files: &mut Vec<PathBuf>) {
    if !root.is_dir() || files.len() >= 200 {
        return;
    }
    let mut queue = vec![(root.to_path_buf(), 0_usize)];
    let mut head = 0_usize;
    while head < queue.len() && files.len() < 200 {
        let (directory, depth) = queue[head].clone();
        head += 1;
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() && depth < 5 {
                queue.push((path, depth + 1));
            } else if metadata.is_file()
                && path.extension().and_then(|extension| extension.to_str()) == Some("jsonl")
                && metadata.len() <= 20 * 1024 * 1024
            {
                files.push(path);
            }
        }
    }
}

fn scan_usage_file(path: &Path, shape: &mut UsageShape) {
    let Ok(file) = fs::File::open(path) else {
        return;
    };
    shape.files += 1;
    shape.previous_tool = None;
    for line in BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .take(5_000)
    {
        if shape.records >= 20_000 {
            break;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        shape.records += 1;
        inspect_usage_value(&value, shape);
    }
}

fn inspect_usage_value(value: &serde_json::Value, shape: &mut UsageShape) {
    match value {
        serde_json::Value::Object(object) => {
            for key in ["input_tokens", "inputTokens", "input"] {
                if let Some(tokens) = object.get(key).and_then(serde_json::Value::as_u64) {
                    shape.max_input_tokens = shape.max_input_tokens.max(tokens);
                }
            }
            if object.get("type").and_then(serde_json::Value::as_str) == Some("message") {
                if let Some(id) = object.get("id").and_then(serde_json::Value::as_str) {
                    if !shape.seen_message_ids.insert(id.to_string()) {
                        shape.duplicate_message_ids += 1;
                    }
                }
            }
            let tool_type = object.get("type").and_then(serde_json::Value::as_str);
            if matches!(tool_type, Some("toolCall" | "tool_call" | "tool_use")) {
                if let Some(name) = object.get("name").and_then(serde_json::Value::as_str) {
                    shape.tool_events += 1;
                    if shape.previous_tool.as_deref() == Some(name) {
                        shape.repeated_tool_names += 1;
                    }
                    shape.previous_tool = Some(name.to_string());
                }
            }
            for nested in object.values() {
                inspect_usage_value(nested, shape);
            }
        }
        serde_json::Value::Array(values) => {
            for nested in values {
                inspect_usage_value(nested, shape);
            }
        }
        _ => {}
    }
}

fn read_named_text(path: &Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_READ_BYTES {
        return None;
    }
    fs::read_to_string(path).ok()
}

fn add_duplicate_instruction_findings(report: &mut Report, files: &[(PathBuf, String)]) {
    let mut directives: HashMap<String, Vec<String>> = HashMap::new();
    for (path, text) in files {
        for line in text.lines() {
            let normalized = normalize_directive(line);
            if normalized.len() >= 24 && directive_shaped(line) {
                directives
                    .entry(normalized)
                    .or_default()
                    .push(display_relative(path));
            }
        }
    }
    let duplicated = directives.values().filter(|paths| paths.len() > 1).count();
    if duplicated > 0 {
        let file_count = directives
            .values()
            .filter(|paths| paths.len() > 1)
            .flat_map(|paths| paths.iter())
            .collect::<std::collections::HashSet<_>>()
            .len();
        report.findings.push(review(
            "DUPLICATE_INSTRUCTIONS",
            "instruction context repeats exact directives",
            format!(
                "{} repeated directives across {} files",
                duplicated, file_count
            ),
            "keep each rule authoritative in one file and reference it elsewhere",
            Confidence::Deterministic,
        ));
    }
}

fn add_duplicate_skill_findings(report: &mut Report, skills: &[(String, PathBuf, String)]) {
    let mut names: HashMap<String, usize> = HashMap::new();
    let mut bodies: HashMap<&str, usize> = HashMap::new();
    for (name, _, body) in skills {
        *names.entry(name.to_lowercase()).or_default() += 1;
        *bodies.entry(body.as_str()).or_default() += 1;
    }
    let duplicate_names = names.values().filter(|count| **count > 1).count();
    let duplicate_bodies = bodies.values().filter(|count| **count > 1).count();
    if duplicate_names > 0 || duplicate_bodies > 0 {
        report.findings.push(review(
            "DUPLICATE_SKILLS",
            "the project has duplicate skill registrations",
            format!(
                "{} duplicate names / {} exact duplicate bodies across {} skills",
                duplicate_names,
                duplicate_bodies,
                skills.len()
            ),
            "remove or consolidate only after confirming which skill root the harness loads",
            Confidence::Deterministic,
        ));
    }
}

fn safe_remove(
    code: &'static str,
    title: String,
    evidence: String,
    action: &'static str,
    reclaim_bytes: u64,
    relative_path: PathBuf,
) -> Finding {
    Finding {
        code,
        title,
        evidence,
        action: action.into(),
        confidence: Confidence::Deterministic,
        disposition: Disposition::SafeToRemove,
        reclaim_bytes,
        relative_path: Some(relative_path),
    }
}

fn review(
    code: &'static str,
    title: impl Into<String>,
    evidence: String,
    action: &'static str,
    confidence: Confidence,
) -> Finding {
    Finding {
        code,
        title: title.into(),
        evidence,
        action: action.into(),
        confidence,
        disposition: Disposition::Review,
        reclaim_bytes: 0,
        relative_path: None,
    }
}

fn remove_safe_artifact(root: &Path, relative: &Path) -> Result<String, String> {
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err("refused a path outside the selected project".into());
    }
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let target = root.join(relative);
    let metadata = fs::symlink_metadata(&target).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("refused to remove a symlink".into());
    }
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "artifact name is unavailable".to_string())?;
    let parent = target
        .parent()
        .ok_or_else(|| "artifact parent is unavailable".to_string())?;
    let allowed = if metadata.is_dir() {
        safe_directory(&target, name)
    } else {
        safe_file(name)
    };
    if !allowed || !parent.starts_with(&root) || target == root {
        return Err("artifact no longer satisfies the safe-removal rule".into());
    }
    if metadata.is_dir() {
        fs::remove_dir_all(&target).map_err(|error| error.to_string())?;
    } else {
        fs::remove_file(&target).map_err(|error| error.to_string())?;
    }
    Ok(format!("removed {}", display_relative(relative)))
}

fn safe_directory(path: &Path, name: &str) -> bool {
    match name {
        "target" => path
            .parent()
            .is_some_and(|parent| parent.join("Cargo.toml").is_file()),
        "node_modules" => path
            .parent()
            .is_some_and(|parent| parent.join("package.json").is_file()),
        ".next" | ".nuxt" | ".turbo" | ".parcel-cache" => path
            .parent()
            .is_some_and(|parent| parent.join("package.json").is_file()),
        "__pycache__" | ".pytest_cache" | ".mypy_cache" | ".ruff_cache" => true,
        _ => false,
    }
}

fn safe_file(name: &str) -> bool {
    name == ".DS_Store" || name.ends_with(".pyc") || name.ends_with(".pyo")
}

fn is_ignored_directory(name: &str) -> bool {
    matches!(
        name,
        ".git" | ".hg" | ".svn" | ".venv" | "venv" | "vendor" | "Pods" | ".idea"
    )
}

fn transient_work_artifact(name: &str) -> bool {
    matches!(
        name.to_ascii_uppercase().as_str(),
        "HANDOFF.MD"
            | "STATUS.MD"
            | "PROGRESS.MD"
            | "TECH-DEBT.MD"
            | "ARCHITECTURE-REVIEW.MD"
            | "EXPLORATION.MD"
            | "IMPLEMENTATION-NOTES.MD"
    )
}

fn credential_shaped(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == ".env"
        || name.starts_with(".env.")
        || [
            "credential",
            "secret",
            "token",
            "auth",
            "private-key",
            "apikey",
        ]
        .iter()
        .any(|marker| name.contains(marker))
}

fn instruction_name(name: &str) -> bool {
    matches!(name, "AGENTS.md" | "CLAUDE.md" | "CONTEXT.md")
}

fn skill_root(path: &Path) -> bool {
    let text = path.to_string_lossy();
    text.contains("/.agents/skills/")
        || text.starts_with(".agents/skills/")
        || text.contains("/.claude/skills/")
        || text.starts_with(".claude/skills/")
        || text.contains("/skills/")
        || text.starts_with("skills/")
}

fn source_extension(extension: &str) -> bool {
    matches!(
        extension,
        "rs" | "py" | "js" | "jsx" | "ts" | "tsx" | "swift" | "go" | "sh" | "bash"
    )
}

fn inside_scripts(path: &Path) -> bool {
    path.components().any(|part| part.as_os_str() == "scripts")
}

fn count_agent_launches(text: &str) -> usize {
    [
        "codex exec",
        "claude -p",
        "claude --print",
        "pi -p",
        "pi --print",
    ]
    .iter()
    .map(|marker| text.matches(marker).count())
    .sum()
}

fn count_verification_markers(text: &str) -> usize {
    [
        "cargo test",
        "pytest",
        "npm test",
        "pnpm test",
        "worker_gate",
        "return_gate",
        "verify",
    ]
    .iter()
    .map(|marker| text.matches(marker).count())
    .sum()
}

fn directive_shaped(line: &str) -> bool {
    let trimmed = line.trim_start();
    let candidate = trimmed
        .trim_start_matches(['-', '*'])
        .trim_start()
        .to_ascii_lowercase();
    [
        "must ", "never ", "do not ", "always ", "use ", "keep ", "avoid ", "prefer ",
    ]
    .iter()
    .any(|prefix| candidate.starts_with(prefix))
}

fn normalize_directive(line: &str) -> String {
    line.trim()
        .trim_start_matches(['-', '*'])
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn normalize_text(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn frontmatter_value(text: &str, key: &str) -> Option<String> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    for line in lines {
        let line = line.trim();
        if line == "---" {
            break;
        }
        if let Some(value) = line.strip_prefix(&format!("{}:", key)) {
            return Some(value.trim().trim_matches(['\'', '"']).to_string());
        }
    }
    None
}

fn directory_size(root: &Path) -> u64 {
    let mut bytes = 0_u64;
    let mut queue = vec![root.to_path_buf()];
    let mut head = 0_usize;
    let mut entries_seen = 0_usize;
    while head < queue.len() && entries_seen < MAX_ARTIFACT_ENTRIES {
        let directory = queue[head].clone();
        head += 1;
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            entries_seen += 1;
            if entries_seen >= MAX_ARTIFACT_ENTRIES {
                break;
            }
            let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                queue.push(entry.path());
            } else if metadata.is_file() {
                bytes = bytes.saturating_add(metadata.len());
            }
        }
    }
    bytes
}

fn display_relative(path: &Path) -> String {
    let value = path.to_string_lossy();
    if value.is_empty() {
        ".".into()
    } else {
        value.into_owned()
    }
}

pub fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= GIB {
        format!("{:.1} GiB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes / KIB)
    } else {
        format!("{} B", bytes as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_root(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("tokoro-bloat-{name}-{nonce}"));
        fs::create_dir_all(&root).expect("fixture root");
        root
    }

    #[test]
    fn generated_directories_are_the_only_safe_removals() {
        let root = fixture_root("safe");
        fs::write(root.join("Cargo.toml"), "[package]\nname='fixture'\n").expect("manifest");
        fs::create_dir_all(root.join("target/debug")).expect("target");
        fs::write(root.join("target/debug/build.bin"), vec![0_u8; 2048]).expect("artifact");
        fs::write(root.join("HANDOFF.md"), "keep until reviewed").expect("handoff");

        let report = scan_project(&root, ScanDepth::Quick);
        let target = report
            .findings
            .iter()
            .find(|finding| finding.title.contains("target"))
            .expect("target finding");
        assert!(target.can_remove());
        let handoff = report
            .findings
            .iter()
            .find(|finding| finding.title.contains("HANDOFF"))
            .expect("handoff finding");
        assert!(!handoff.can_remove());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn removal_refuses_non_generated_paths() {
        let root = fixture_root("refuse");
        fs::write(root.join("README.md"), "product").expect("readme");
        assert!(remove_safe_artifact(&root, Path::new("README.md")).is_err());
        assert!(root.join("README.md").exists());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn runtime_findings_keep_advice_separate_from_removal() {
        let findings = runtime_findings(RuntimeSnapshot {
            loaded_endpoints: 2,
            swap_mib: 512.0,
            prompt_tokens: 10_000,
            prefix_hits: 0,
            prefix_partial_hits: 4,
            probe_ports: 6,
        });
        assert_eq!(findings.len(), 5);
        assert!(findings.iter().all(|finding| !finding.can_remove()));
    }
}
