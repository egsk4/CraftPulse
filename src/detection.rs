use crate::config::ActivityRule;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use sysinfo::{ProcessesToUpdate, System};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DetectedApp {
    pub name: String,
    pub process_id: u32,
    pub document: Option<String>,
}

/// Largest idle timeout accepted by the settings API (one day).
pub const MAX_IDLE_SECS: u64 = 86_400;

/// Seconds without input; u64::MAX means this session cannot report idle time.
pub static LAST_IDLE_SECONDS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(u64::MAX);

fn is_idle(seconds: Option<u64>, threshold: u64) -> bool {
    threshold > 0 && seconds.is_some_and(|seconds| seconds >= threshold)
}

/// Once idle, activity in another app must not make ArtCraft look active.
#[cfg(any(windows, test))]
fn windows_idle_status(
    seconds: Option<u64>,
    threshold: u64,
    was_idle: bool,
    previous_app: Option<u32>,
    current_app: Option<u32>,
    artcraft_focused: bool,
) -> bool {
    threshold > 0
        && current_app.is_some()
        && (is_idle(seconds, threshold)
            || (was_idle && previous_app == current_app && !artcraft_focused))
}

/// (normalized name, app name, process id, start time)
type Candidate<'a> = (String, &'a str, u32, u64);
type UiTabs = (Vec<(String, Option<bool>)>, String, Option<String>);

#[derive(Default)]
pub struct ProcessDetector {
    system: System,
    previous: HashSet<(String, u32, u64)>,
    current_pid: Option<u32>,
    /// Activity inferred from the app's open dialogs, e.g. "Exporting".
    pub activity: Option<String>,
    /// True when there has been no keyboard or mouse input for a while.
    pub idle: bool,
    pub idle_after_secs: u64,
    pub preferred_app: Option<String>,
    pub selection_reason: &'static str,
    pub idle_overrides: std::collections::BTreeMap<String, u64>,
    /// A streaming or recording app (OBS and similar) is running.
    pub streaming: bool,
    /// Raw visible window titles of the chosen app (for the activity log).
    pub titles: Vec<String>,
    uia_pid: Option<u32>,
    uia_checked: Option<Instant>,
    uia_doc: Option<String>,
    uia_note: String,
    uia_tabs: Vec<String>,
    uia_activity: Option<String>,
    extra_rules: Vec<ActivityRule>,
    /// Running processes that look like ArtCraft apps but aren't recognised.
    pub unknown: Vec<String>,
}

// Keep one logical app while retaining the window-owning helper processes.
fn process_root(
    mut pid: u32,
    names: &HashMap<u32, String>,
    parents: &HashMap<u32, Option<u32>>,
) -> u32 {
    for _ in 0..names.len() {
        match parents.get(&pid).copied().flatten() {
            Some(parent) if names.get(&parent) == names.get(&pid) => pid = parent,
            _ => break,
        }
    }
    pid
}

fn pinned_candidate<'a>(
    active: &[Candidate<'a>],
    name: &str,
    previous_pid: Option<u32>,
) -> Option<Candidate<'a>> {
    active
        .iter()
        .filter(|candidate| candidate.1.eq_ignore_ascii_case(name))
        .max_by_key(|candidate| (Some(candidate.2) == previous_pid, candidate.3))
        .cloned()
}

impl ProcessDetector {
    /// User-defined "selected button -> activity" rules from config.toml.
    pub fn set_rules(&mut self, rules: &[ActivityRule]) {
        if self.extra_rules != rules {
            self.extra_rules = rules.to_vec();
        }
    }

    pub fn detect(&mut self, supported: &[&str]) -> Option<DetectedApp> {
        refresh_processes(&mut self.system);
        let mut active: Vec<Candidate> = Vec::new();
        let mut unknown: Vec<String> = Vec::new();
        let mut streaming = false;
        let mut parents: std::collections::HashMap<u32, Option<u32>> =
            std::collections::HashMap::new();
        for process in self.system.processes().values() {
            let process_name = process.name().to_string_lossy();
            let lower_name = process_name.to_lowercase();
            streaming |= is_streaming_process(&lower_name);
            let names = process_names(process);
            let matched = supported
                .iter()
                .find(|candidate| names.iter().any(|name| matches_app(name, candidate)));
            if matched.is_none()
                && lower_name.contains("craft")
                && !lower_name.contains("craftpulse")
            {
                unknown.push(process_name.to_string());
            }
            if let Some(name) = matched {
                active.push((
                    name.to_lowercase(),
                    *name,
                    process.pid().as_u32(),
                    process.start_time(),
                ));
                parents.insert(
                    process.pid().as_u32(),
                    process.parent().map(|pid| pid.as_u32()),
                );
            }
        }
        // An app that starts helper processes of the same name counts once:
        // keep the one that was not started by another process of the same app.
        let names_by_pid: std::collections::HashMap<u32, String> = active
            .iter()
            .map(|(name, _, pid, _)| (*pid, name.clone()))
            .collect();
        let roots: HashMap<u32, u32> = active
            .iter()
            .map(|(_, _, pid, _)| (*pid, process_root(*pid, &names_by_pid, &parents)))
            .collect();
        let mut families: HashMap<u32, Vec<u32>> = HashMap::new();
        for (&pid, &root) in &roots {
            families.entry(root).or_default().push(pid);
        }
        active.retain(|(_, _, pid, _)| roots.get(pid) == Some(pid));
        let current = active
            .iter()
            .map(|(name, _, pid, start)| (name.clone(), *pid, *start))
            .collect::<HashSet<_>>();

        // Prefer the ArtCraft app whose window you are actually using.
        let foreground = foreground_process_id();
        let focused = foreground.and_then(|fpid| {
            let root = roots.get(&fpid).copied().unwrap_or(fpid);
            active.iter().find(|(_, _, pid, _)| *pid == root).cloned()
        });
        let artcraft_focused = focused.is_some();
        let focused_app_pid = focused.as_ref().map(|candidate| candidate.2);
        let chosen = if let Some(name) = &self.preferred_app {
            pinned_candidate(&active, name, self.current_pid)
        } else if artcraft_focused {
            focused
        } else if cfg!(any(target_os = "windows", target_os = "linux")) {
            // Another (non-ArtCraft) window has focus: stay on the last app
            // while it is still running, otherwise pick the newest one.
            self.current_pid
                .and_then(|cpid| active.iter().find(|(_, _, pid, _)| *pid == cpid).cloned())
                .or_else(|| newest_candidate(&active, &self.previous))
        } else {
            newest_candidate(&active, &self.previous)
        };
        self.selection_reason = if chosen.is_none() {
            if self.preferred_app.is_some() {
                "Pinned app is not running or is disabled"
            } else {
                "No enabled supported app detected"
            }
        } else if self.preferred_app.is_some() {
            "Pinned in settings"
        } else if chosen.as_ref().map(|candidate| candidate.2) == focused_app_pid {
            "Focused ArtCraft window"
        } else if cfg!(any(windows, target_os = "linux"))
            && chosen.as_ref().map(|candidate| candidate.2) == self.current_pid
        {
            "Previous app is still running"
        } else {
            "Newest running ArtCraft app"
        };
        self.streaming = streaming;
        unknown.sort();
        unknown.dedup();
        self.unknown = unknown;
        self.previous = current;
        let threshold = chosen
            .as_ref()
            .and_then(|(_, name, _, _)| self.idle_overrides.get(&name.to_ascii_lowercase()))
            .copied()
            .unwrap_or(self.idle_after_secs);
        let idle_for = idle_seconds();
        LAST_IDLE_SECONDS.store(
            idle_for.unwrap_or(u64::MAX),
            std::sync::atomic::Ordering::Relaxed,
        );
        #[cfg(windows)]
        {
            self.idle = windows_idle_status(
                idle_for,
                threshold,
                self.idle,
                self.current_pid,
                chosen.as_ref().map(|(_, _, pid, _)| *pid),
                chosen
                    .as_ref()
                    .is_some_and(|candidate| Some(candidate.2) == focused_app_pid),
            );
        }
        #[cfg(not(windows))]
        {
            self.idle = is_idle(idle_for, threshold);
        }

        match chosen {
            Some((_, name, process_id, _)) => {
                self.current_pid = Some(process_id);
                let mut family = families
                    .remove(&process_id)
                    .unwrap_or_else(|| vec![process_id]);
                family.sort_unstable();
                if let Some(fpid) = foreground.filter(|pid| family.contains(pid)) {
                    family.retain(|pid| *pid != fpid);
                    family.insert(0, fpid);
                }
                let ui_pid = foreground
                    .filter(|pid| family.contains(pid))
                    .unwrap_or(process_id);
                let mut title_document = None;
                let mut activity = None;
                let mut titles = Vec::new();
                for pid in family {
                    let (document, action, window_titles) = window_info(pid, name);
                    title_document = title_document.or(document);
                    activity = activity.or(action);
                    titles.extend(window_titles);
                }
                // Reading the accessibility tree is slow, so only do it every 2 seconds.
                let uia_due = self.uia_pid != Some(ui_pid)
                    || self
                        .uia_checked
                        .is_none_or(|at| at.elapsed() >= Duration::from_secs(2));
                if uia_due {
                    if self.uia_pid != Some(ui_pid) {
                        self.uia_tabs.clear();
                        self.uia_doc = None;
                        self.uia_activity = None;
                    }
                    let rules: Vec<(String, String)> = self
                        .extra_rules
                        .iter()
                        .filter(|rule| rule.app.eq_ignore_ascii_case(name))
                        .map(|rule| (rule.element.to_lowercase(), rule.activity.clone()))
                        .chain(
                            BUILTIN_RULES
                                .iter()
                                .filter(|(app, _, _)| app.eq_ignore_ascii_case(name))
                                .map(|(_, element, activity)| {
                                    (element.to_lowercase(), activity.to_string())
                                }),
                        )
                        .collect();
                    let (tabs, note, ui_activity) = tab_candidates_via_uia(ui_pid, name, &rules);
                    self.uia_activity = ui_activity;
                    let names: Vec<String> = tabs.iter().map(|(name, _)| name.clone()).collect();
                    let selected = tabs
                        .iter()
                        .find(|(_, selected)| *selected == Some(true))
                        .map(|(name, _)| name.clone());
                    // Without selection info: a newly appeared tab is the one just opened,
                    // otherwise keep the current one while it still exists.
                    let doc = selected
                        .or_else(|| {
                            if self.uia_tabs.is_empty() {
                                None
                            } else {
                                names
                                    .iter()
                                    .rev()
                                    .find(|name| !self.uia_tabs.contains(*name))
                                    .cloned()
                            }
                        })
                        .or_else(|| {
                            self.uia_doc.clone().filter(|doc| {
                                names.contains(doc) || name.eq_ignore_ascii_case("LightCraft")
                            })
                        })
                        .or_else(|| names.last().cloned());
                    self.uia_note = match &doc {
                        Some(doc) => format!("{note}; current: {doc}"),
                        None => note,
                    };
                    self.uia_tabs = names;
                    self.uia_doc = doc;
                    self.uia_checked = Some(Instant::now());
                    self.uia_pid = Some(ui_pid);
                }
                let document = self.uia_doc.clone().or(title_document);
                if !self.uia_note.is_empty() {
                    titles.push(self.uia_note.clone());
                }
                self.activity = activity.or_else(|| self.uia_activity.clone());
                self.titles = titles;
                Some(DetectedApp {
                    name: name.to_string(),
                    process_id,
                    document,
                })
            }
            None => {
                self.current_pid = None;
                self.activity = None;
                self.titles = Vec::new();
                None
            }
        }
    }
}

/// Streaming and screen-recording apps that mean an audience may be watching.
const STREAMING_PROCESSES: [&str; 8] = [
    "obs64",
    "obs32",
    "obs",
    "streamlabs",
    "xsplit.core",
    "twitch studio",
    "cpthost",
    "medal",
];

fn is_streaming_process(lower_name: &str) -> bool {
    let name = lower_name.strip_suffix(".exe").unwrap_or(lower_name);
    STREAMING_PROCESSES
        .iter()
        .any(|candidate| name == *candidate || name.starts_with("streamlabs"))
}

fn newest_candidate<'a>(
    active: &[Candidate<'a>],
    previous: &HashSet<(String, u32, u64)>,
) -> Option<Candidate<'a>> {
    active
        .iter()
        .filter(|(name, _, pid, start)| !previous.contains(&(name.clone(), *pid, *start)))
        .max_by_key(|(_, _, pid, start)| (*start, *pid))
        .or_else(|| {
            active
                .iter()
                .max_by_key(|(_, _, pid, start)| (*start, *pid))
        })
        .cloned()
}

/// (app, name of a button/tab, activity shown while it is selected).
/// Earlier entries win. Add your own under `[[rules]]` in config.toml.
const BUILTIN_RULES: [(&str, &str, &str); 15] = [
    ("LightCraft", "Compare", "Comparing photos"),
    ("LightCraft", "Survey", "Surveying photos"),
    ("LightCraft", "People", "Organising people"),
    ("LightCraft", "Photo Grid", "Browsing the library"),
    ("LightCraft", "Square Grid", "Browsing the library"),
    ("LightCraft", "Crop & Rotate", "Cropping a photo"),
    ("LightCraft", "Remove (H)", "Retouching a photo"),
    ("LightCraft", "Masking (M)", "Masking a photo"),
    ("LightCraft", "Red Eye", "Fixing red eye"),
    ("LightCraft", "Versions", "Managing versions"),
    ("LightCraft", "History & Activity", "Reviewing edit history"),
    ("LightCraft", "Keywords", "Tagging photos"),
    ("LightCraft", "Info (I)", "Viewing photo info"),
    ("LightCraft", "Presets", "Applying presets"),
    ("LightCraft", "Edit (E)", "Editing a photo"),
];

/// Window-title prefixes that tell us what the user is doing.
const DIALOG_ACTIVITIES: [(&str, &str); 7] = [
    ("export", "Exporting"),
    ("render", "Rendering"),
    ("save as", "Saving"),
    ("save", "Saving"),
    ("print", "Printing"),
    ("import", "Importing"),
    ("open", "Opening a file"),
];

pub fn dialog_activity(title: &str) -> Option<&'static str> {
    let lower = title.trim().to_lowercase();
    DIALOG_ACTIVITIES
        .iter()
        .find(|(keyword, _)| lower.starts_with(keyword))
        .map(|(_, activity)| *activity)
}

/// The main document window is the one whose title mentions the app name.
#[allow(dead_code)]
fn pick_document_title<'a>(titles: &'a [String], app_name: &str) -> Option<&'a str> {
    let app = app_name.to_lowercase();
    titles
        .iter()
        .find(|title| title.to_lowercase().contains(&app))
        .or_else(|| titles.iter().find(|title| dialog_activity(title).is_none()))
        .or_else(|| titles.first())
        .map(String::as_str)
}

/// Dialog windows (titles that don't mention the app name) reveal the activity.
#[allow(dead_code)]
fn activity_from_titles(titles: &[String], app_name: &str) -> Option<String> {
    let app = app_name.to_lowercase();
    titles
        .iter()
        .filter(|title| !title.to_lowercase().contains(&app))
        .find_map(|title| dialog_activity(title))
        .map(str::to_string)
}

#[cfg(target_os = "windows")]
fn window_info(pid: u32, app_name: &str) -> (Option<String>, Option<String>, Vec<String>) {
    let titles = window_titles_for_process(pid);
    let document =
        pick_document_title(&titles, app_name).and_then(|title| strip_title(title, app_name));
    let activity = activity_from_titles(&titles, app_name);
    (document, activity, titles)
}

#[cfg(target_os = "linux")]
fn window_info(pid: u32, app_name: &str) -> (Option<String>, Option<String>, Vec<String>) {
    let titles = crate::linux::titles_for(pid);
    let document =
        pick_document_title(&titles, app_name).and_then(|title| strip_title(title, app_name));
    let activity = activity_from_titles(&titles, app_name);
    (document, activity, titles)
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn window_info(pid: u32, app_name: &str) -> (Option<String>, Option<String>, Vec<String>) {
    (window_title_for_process(pid, app_name), None, Vec::new())
}

/// Turns a tab label like "Poster.png @ 78.9% (RGB/8)*" into "Poster.png".
#[allow(dead_code)]
fn clean_tab_name(name: &str) -> String {
    let without_zoom = match regex::Regex::new(r" @ \d+(?:\.\d+)?%.*$") {
        Ok(pattern) => pattern.replace(name, "").into_owned(),
        Err(_) => name.to_string(),
    };
    without_zoom
        .trim()
        .trim_end_matches(|ch: char| ch == '*' || ch == '•' || ch.is_whitespace())
        .to_string()
}

/// Asks an accessibility element whether it is the selected/active one, trying
/// the selection, toggle and legacy-accessibility patterns in turn.
#[cfg(target_os = "windows")]
fn element_selected(
    element: &windows::Win32::UI::Accessibility::IUIAutomationElement,
) -> Option<bool> {
    use windows::Win32::UI::Accessibility::{
        IUIAutomationLegacyIAccessiblePattern, IUIAutomationSelectionItemPattern,
        IUIAutomationTogglePattern, ToggleState_On, UIA_LegacyIAccessiblePatternId,
        UIA_SelectionItemPatternId, UIA_TogglePatternId,
    };
    unsafe {
        if let Ok(pattern) = element
            .GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId)
        {
            if let Ok(flag) = pattern.CurrentIsSelected() {
                return Some(flag.as_bool());
            }
        }
        if let Ok(pattern) =
            element.GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId)
        {
            if let Ok(state) = pattern.CurrentToggleState() {
                return Some(state == ToggleState_On);
            }
        }
        if let Ok(pattern) = element.GetCurrentPatternAs::<IUIAutomationLegacyIAccessiblePattern>(
            UIA_LegacyIAccessiblePatternId,
        ) {
            if let Ok(state) = pattern.CurrentState() {
                // STATE_SYSTEM_SELECTED (0x2), PRESSED (0x8), CHECKED (0x10)
                return Some(state & 0x1A != 0);
            }
        }
    }
    None
}

/// Finds open document tabs through Windows UI Automation. Returns each tab's
/// cleaned name with its "selected" state (if the app reports it), plus a
/// short diagnostic note for the activity log.
///
/// Set the environment variable CRAFTPULSE_DUMP_UIA=1 to also write the
/// accessibility tree to `uia-<app>.txt` in the CraftPulse config folder.
#[cfg(target_os = "windows")]
fn tab_candidates_via_uia(pid: u32, app_name: &str, rules: &[(String, String)]) -> UiTabs {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::{
        Foundation::{BOOL, HWND, LPARAM},
        System::Com::{
            CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
        },
        UI::{
            Accessibility::{
                CUIAutomation, IUIAutomation, TreeScope_Descendants, UIA_DataItemControlTypeId,
                UIA_ListItemControlTypeId, UIA_TabItemControlTypeId,
            },
            WindowsAndMessaging::{
                EnumWindows, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
                IsWindowVisible,
            },
        },
    };
    // (pid, lowercase app name, preferred main window, fallback window)
    type WindowSearch = (u32, String, Option<HWND>, Option<HWND>);
    unsafe extern "system" fn callback(hwnd: HWND, data: LPARAM) -> BOOL {
        let (wanted, needle, preferred, fallback) = &mut *(data.0 as *mut WindowSearch);
        let mut owner = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut owner));
        if owner != *wanted || !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        let length = GetWindowTextLengthW(hwnd);
        if length <= 0 {
            return BOOL(1);
        }
        let mut buffer = vec![0u16; length as usize + 1];
        let count = GetWindowTextW(hwnd, &mut buffer);
        if count <= 0 {
            return BOOL(1);
        }
        buffer.truncate(count as usize);
        let title = OsString::from_wide(&buffer)
            .to_string_lossy()
            .to_lowercase();
        if fallback.is_none() {
            *fallback = Some(hwnd);
        }
        if title.contains(needle.as_str()) {
            *preferred = Some(hwnd);
            return BOOL(0);
        }
        BOOL(1)
    }
    // Prefer the main window (title contains the app name) over dialogs.
    let mut state: WindowSearch = (pid, app_name.to_lowercase(), None, None);
    unsafe {
        let _ = EnumWindows(Some(callback), LPARAM(&mut state as *mut _ as isize));
    }
    let Some(hwnd) = state.2.or(state.3) else {
        return (Vec::new(), "UIA: no window".to_string(), None);
    };
    let (Ok(zoom_pattern), Ok(ext_pattern)) = (
        regex::Regex::new(r"^.+? @ \d+(?:\.\d+)?%"),
        regex::Regex::new(r"\.[A-Za-z0-9]{2,12}[*•\s]*$"),
    ) else {
        return (Vec::new(), String::new(), None);
    };
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let automation: IUIAutomation =
            match CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) {
                Ok(automation) => automation,
                Err(error) => return (Vec::new(), format!("UIA unavailable: {error}"), None),
            };
        let Ok(root) = automation.ElementFromHandle(hwnd) else {
            return (Vec::new(), "UIA: could not read window".to_string(), None);
        };
        let Ok(condition) = automation.CreateTrueCondition() else {
            return (
                Vec::new(),
                "UIA: could not create condition".to_string(),
                None,
            );
        };
        let Ok(elements) = root.FindAll(TreeScope_Descendants, &condition) else {
            return (Vec::new(), "UIA: could not search window".to_string(), None);
        };
        let count = elements.Length().unwrap_or(0);
        let dump = std::env::var_os("CRAFTPULSE_DUMP_UIA").is_some();
        let mut dump_lines: Vec<String> = Vec::new();
        let mut candidates: Vec<(String, Option<bool>)> = Vec::new();
        let mut tab_names: Vec<String> = Vec::new();
        let mut first_names: Vec<String> = Vec::new();
        // LightCraft shows the open photo as the selected filmstrip thumbnail, not a tab.
        let is_lightcraft = app_name.eq_ignore_ascii_case("LightCraft");
        let mut list_checked = 0;
        let mut list_selected: Vec<String> = Vec::new();
        let mut list_names: Vec<String> = Vec::new();
        let mut ui_rule: Option<usize> = None;
        let mut awaiting_file_name = false;
        for index in 0..count {
            let Ok(element) = elements.GetElement(index) else {
                continue;
            };
            let Ok(name) = element.CurrentName() else {
                continue;
            };
            let name = name.to_string();
            if name.trim().is_empty() {
                continue;
            }
            if dump && dump_lines.len() < 800 {
                let kind = element
                    .CurrentControlType()
                    .map(|kind| kind.0)
                    .unwrap_or(-1);
                dump_lines.push(format!(
                    "{kind} | {:?} | {}",
                    element_selected(&element),
                    name.replace('\n', " ")
                ));
            }
            let short = name.chars().count() <= 60;
            let kind = element.CurrentControlType().ok();
            let is_tab = kind.is_some_and(|kind| kind == UIA_TabItemControlTypeId);
            // Which activity rule (if any) is currently active? Earlier rules win.
            if !rules.is_empty() && ui_rule != Some(0) {
                let button_like = kind
                    .is_some_and(|kind| matches!(kind.0, 50000 | 50002 | 50007 | 50013 | 50019));
                if button_like {
                    let lower = name.to_lowercase();
                    if let Some(position) = rules
                        .iter()
                        .position(|(element, _)| lower.starts_with(element.as_str()))
                    {
                        if ui_rule.is_none_or(|best| position < best)
                            && element_selected(&element) == Some(true)
                        {
                            ui_rule = Some(position);
                        }
                    }
                }
            }
            if short && first_names.len() < 6 && !first_names.contains(&name) {
                first_names.push(name.clone());
            }
            if short && is_tab && tab_names.len() < 8 {
                tab_names.push(name.clone());
            }
            if is_lightcraft && list_checked < 200 {
                let is_list_item = kind.is_some_and(|kind| {
                    kind == UIA_ListItemControlTypeId || kind == UIA_DataItemControlTypeId
                });
                if is_list_item {
                    list_checked += 1;
                    if short && list_names.len() < 6 {
                        list_names.push(name.clone());
                    }
                    if element_selected(&element) == Some(true) {
                        list_selected.push(name.trim().to_string());
                    }
                }
            }
            // LightCraft's Info panel lists the file name right after a "File Name"
            // label (a "Rename…" button sits in between).
            if is_lightcraft {
                if awaiting_file_name && kind.is_some_and(|kind| kind.0 == 50020) {
                    candidates.push((name.trim().to_string(), Some(true)));
                    awaiting_file_name = false;
                } else if name == "File Name" {
                    awaiting_file_name = true;
                }
            }
            if zoom_pattern.is_match(&name) || (is_tab && ext_pattern.is_match(&name)) {
                candidates.push((clean_tab_name(&name), element_selected(&element)));
            }
        }
        if dump {
            if let Some(dir) = dirs::config_dir() {
                let dir = dir.join("CraftPulse");
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::fs::write(
                    dir.join(format!("uia-{}.txt", app_name.to_lowercase())),
                    dump_lines.join("\n"),
                );
            }
        }
        if candidates.is_empty() && !list_selected.is_empty() {
            candidates = list_selected
                .iter()
                .map(|name| (name.clone(), Some(true)))
                .collect();
        }
        let note = if is_lightcraft && !candidates.is_empty() {
            "UIA: LightCraft selected photo".to_string()
        } else if is_lightcraft && candidates.is_empty() && !list_names.is_empty() {
            format!("UIA: no selected photo found; list items seen: {list_names:?}")
        } else if is_lightcraft && candidates.is_empty() {
            "UIA: open LightCraft's Info panel (I) to show the photo name".to_string()
        } else if !candidates.is_empty() {
            let known = candidates
                .iter()
                .any(|(_, selected)| *selected == Some(true));
            format!(
                "UIA: {} tab(s), selection {}",
                candidates.len(),
                if known { "known" } else { "unknown" }
            )
        } else if count == 0 {
            "UIA: tree empty (app may not expose accessibility)".to_string()
        } else if !tab_names.is_empty() {
            format!("UIA: no document tab found; tabs seen: {tab_names:?}")
        } else {
            format!("UIA: no tabs found; first elements: {first_names:?}")
        };
        let ui_activity = ui_rule.map(|position| rules[position].1.clone());
        (candidates, note, ui_activity)
    }
}

#[cfg(not(target_os = "windows"))]
fn tab_candidates_via_uia(_pid: u32, _app_name: &str, _rules: &[(String, String)]) -> UiTabs {
    (Vec::new(), String::new(), None)
}

#[cfg(target_os = "windows")]
fn foreground_process_id() -> Option<u32> {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    let mut pid = 0;
    unsafe {
        let hwnd = GetForegroundWindow();
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    (pid != 0).then_some(pid)
}

#[cfg(target_os = "linux")]
fn foreground_process_id() -> Option<u32> {
    crate::linux::focused_pid()
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn foreground_process_id() -> Option<u32> {
    None
}

#[cfg(target_os = "windows")]
fn idle_seconds() -> Option<u64> {
    use windows::Win32::{
        System::SystemInformation::GetTickCount,
        UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
    };
    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    unsafe {
        if !GetLastInputInfo(&mut info).as_bool() {
            return None;
        }
        Some((GetTickCount().wrapping_sub(info.dwTime) / 1000) as u64)
    }
}

#[cfg(target_os = "linux")]
fn idle_seconds() -> Option<u64> {
    crate::linux::idle_seconds()
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn idle_seconds() -> Option<u64> {
    None
}

pub fn process_matches(process: &str, app: &str) -> bool {
    fn without_exe_extension(name: &str) -> &str {
        let name = name.trim();
        name.strip_suffix(".exe")
            .or_else(|| {
                name.get(..name.len().saturating_sub(4)).filter(|_| {
                    name.len() >= 4 && name[name.len() - 4..].eq_ignore_ascii_case(".exe")
                })
            })
            .unwrap_or(name)
    }
    without_exe_extension(process).eq_ignore_ascii_case(without_exe_extension(app))
}

fn matches_app(process: &str, app: &str) -> bool {
    process_matches(process, app) || {
        #[cfg(target_os = "linux")]
        {
            crate::linux::variant_name_matches(process, app)
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }
}

/// Every name a process answers to. On Linux that includes the real executable
/// and the AppImage it came from, because the kernel cuts the process name short.
fn process_names(process: &sysinfo::Process) -> Vec<String> {
    #[allow(unused_mut)]
    let mut names = vec![process.name().to_string_lossy().into_owned()];
    #[cfg(target_os = "linux")]
    names.extend(crate::linux::extra_names(process));
    names
}

#[cfg(target_os = "linux")]
fn refresh_processes(system: &mut System) {
    use sysinfo::{ProcessRefreshKind, UpdateKind};
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_exe(UpdateKind::OnlyIfNotSet)
            .with_cmd(UpdateKind::OnlyIfNotSet)
            .with_environ(UpdateKind::OnlyIfNotSet),
    );
}

#[cfg(not(target_os = "linux"))]
fn refresh_processes(system: &mut System) {
    use sysinfo::{ProcessRefreshKind, UpdateKind};
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
    );
}

#[cfg(target_os = "windows")]
fn window_titles_for_process(pid: u32) -> Vec<String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::{
        Foundation::{BOOL, HWND, LPARAM},
        UI::WindowsAndMessaging::{
            EnumWindows, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
            IsWindowVisible,
        },
    };
    unsafe extern "system" fn callback(hwnd: HWND, data: LPARAM) -> BOOL {
        let (wanted, titles) = &mut *(data.0 as *mut (u32, Vec<String>));
        let mut owner = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut owner));
        if owner == *wanted && IsWindowVisible(hwnd).as_bool() {
            let length = GetWindowTextLengthW(hwnd);
            if length > 0 {
                let mut buffer = vec![0u16; length as usize + 1];
                let count = GetWindowTextW(hwnd, &mut buffer);
                if count > 0 {
                    buffer.truncate(count as usize);
                    titles.push(OsString::from_wide(&buffer).to_string_lossy().into_owned());
                }
            }
        }
        BOOL(1)
    }
    let mut state: (u32, Vec<String>) = (pid, Vec::new());
    unsafe {
        let _ = EnumWindows(Some(callback), LPARAM(&mut state as *mut _ as isize));
    }
    state.1
}

#[cfg(target_os = "macos")]
fn window_title_for_process(pid: u32, app_name: &str) -> Option<String> {
    let script = format!(
        "tell application \"System Events\" to get name of first window of (first application process whose unix id is {pid})"
    );
    std::process::Command::new("osascript")
        .args(["-e", &script])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .and_then(|title| strip_title(title.trim(), app_name))
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn window_title_for_process(_: u32, _: &str) -> Option<String> {
    None
}

pub fn strip_title(title: &str, app_name: &str) -> Option<String> {
    let normalized = title.trim();
    let mut candidate = normalized.to_string();
    if !app_name.is_empty() {
        let suffixes = [
            format!("— {app_name}"),
            format!("– {app_name}"),
            format!("- {app_name}"),
            format!("| {app_name}"),
            app_name.to_string(),
        ];
        for suffix in suffixes {
            if candidate.to_lowercase().ends_with(&suffix.to_lowercase()) {
                let keep = candidate.len() - suffix.len();
                candidate = candidate[..keep].trim_end().to_string();
                break;
            }
        }
    }
    if !app_name.is_empty() {
        for separator in ["— ", "– ", "- ", "| "] {
            let prefix = format!("{app_name} {separator}");
            if candidate.to_lowercase().starts_with(&prefix.to_lowercase()) {
                candidate = candidate[prefix.len()..].trim_start().to_string();
                break;
            }
        }
    }
    let cleaned = candidate.trim().trim_matches('"').trim();
    if cleaned.is_empty() || cleaned.eq_ignore_ascii_case("untitled") {
        None
    } else {
        Some(cleaned.to_string())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn pinning_keeps_the_same_process_and_never_falls_back_to_another_app() {
        let candidates = vec![
            ("wordcraft".into(), "WordCraft", 10, 100),
            ("wordcraft".into(), "WordCraft", 20, 200),
            ("photocraft".into(), "PhotoCraft", 30, 300),
        ];
        assert_eq!(
            super::pinned_candidate(&candidates, "WordCraft", Some(10))
                .unwrap()
                .2,
            10
        );
        assert_eq!(
            super::pinned_candidate(&candidates, "WordCraft", None)
                .unwrap()
                .2,
            20
        );
        assert!(super::pinned_candidate(&candidates, "GridCraft", None).is_none());
    }

    #[test]
    fn helper_windows_keep_the_same_logical_app() {
        let names = [
            (1, "photocraft".into()),
            (2, "photocraft".into()),
            (3, "lightcraft".into()),
            (4, "photocraft".into()),
        ]
        .into();
        let parents = [(1, None), (2, Some(1)), (3, Some(1)), (4, Some(2))].into();
        assert_eq!(super::process_root(4, &names, &parents), 1);
        assert_eq!(super::process_root(3, &names, &parents), 3);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn appimage_payload_is_recognised_from_its_environment() {
        use std::{fs, process::Command};
        let dir = std::env::temp_dir().join(format!("craftpulse-payload-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("payload");
        fs::copy("/bin/sleep", &exe).unwrap();
        let mut child = Command::new(&exe)
            .arg("30")
            .env("APPDIR", &dir)
            .env("APPIMAGE", "/tmp/PhotoCraft-1.2-x86_64.AppImage")
            .spawn()
            .unwrap();
        let mut detector = super::ProcessDetector::default();
        let detected = detector.detect(&["PhotoCraft"]);
        let _ = child.kill();
        let _ = child.wait();
        fs::remove_dir_all(dir).unwrap();
        let detected =
            detected.expect("AppImage payload must match even with a generic executable name");
        assert_eq!(detected.process_id, child.id());
        assert_eq!(detected.name, "PhotoCraft");
    }
    #[test]
    fn windows_idle_survives_discord_input_until_returning_to_artcraft() {
        let app = Some(42);
        let mut idle = super::windows_idle_status(Some(300), 300, false, app, app, false);
        assert!(idle);
        for seconds in [0, 1, 0, 10] {
            idle = super::windows_idle_status(Some(seconds), 300, idle, app, app, false);
            assert!(idle, "input in Discord must not resume ArtCraft activity");
        }
        assert!(!super::windows_idle_status(
            Some(0),
            300,
            idle,
            app,
            app,
            true
        ));
        assert!(super::windows_idle_status(
            Some(300),
            300,
            false,
            app,
            app,
            true
        ));
    }

    #[test]
    fn windows_idle_latch_resets_for_disabled_closed_or_different_apps() {
        let app = Some(42);
        assert!(!super::windows_idle_status(
            Some(0),
            300,
            false,
            app,
            app,
            false
        ));
        assert!(!super::windows_idle_status(
            Some(900),
            0,
            true,
            app,
            app,
            false
        ));
        assert!(!super::windows_idle_status(
            Some(0),
            300,
            true,
            app,
            Some(43),
            false
        ));
        assert!(!super::windows_idle_status(
            Some(900),
            300,
            true,
            app,
            None,
            false
        ));
        assert!(super::windows_idle_status(None, 300, true, app, app, false));
    }

    #[test]
    fn idle_threshold_supports_disabled_unknown_and_boundary() {
        assert!(!super::is_idle(None, 300));
        assert!(!super::is_idle(Some(299), 300));
        assert!(super::is_idle(Some(300), 300));
        assert!(!super::is_idle(Some(1000), 0));
        assert!(super::is_idle(Some(1), 1));
    }
    use super::*;

    #[test]
    fn recognises_streaming_apps() {
        assert!(is_streaming_process("obs64.exe"));
        assert!(is_streaming_process("streamlabs desktop.exe"));
        assert!(!is_streaming_process("photocraft.exe"));
        assert!(!is_streaming_process("obsidian.exe"));
    }

    #[test]
    fn matches_process_with_or_without_extension() {
        assert!(process_matches("PhotoCraft.exe", "photocraft"));
        assert!(process_matches("photocraft", "PHOTOCRAFT.EXE"));
        assert!(!process_matches("PhotoCraftBeta.exe", "PhotoCraft"));
    }

    #[test]
    fn strips_each_supported_title_separator() {
        for sep in [" — ", " – ", " - ", " | "] {
            assert_eq!(
                strip_title(&format!("Project{sep}PhotoCraft"), "PhotoCraft"),
                Some("Project".to_string())
            );
        }
    }

    #[test]
    fn keeps_names_that_only_contain_a_separator() {
        assert_eq!(
            strip_title("The — Project", "PhotoCraft"),
            Some("The — Project".to_string())
        );
    }

    #[test]
    fn cleans_tab_labels() {
        assert_eq!(
            clean_tab_name("Screenshot 2026-10-09 081234.png @ 78.9% (Background, RGB/8)*"),
            "Screenshot 2026-10-09 081234.png"
        );
        assert_eq!(clean_tab_name("Poster.vectorcraft •"), "Poster.vectorcraft");
    }

    #[test]
    fn strips_app_name_prefix() {
        assert_eq!(
            strip_title("PhotoCraft — Poster", "PhotoCraft").as_deref(),
            Some("Poster")
        );
    }

    #[test]
    fn export_dialog_means_exporting() {
        let titles = vec!["Export As".to_string(), "Poster — PhotoCraft".to_string()];
        assert_eq!(
            activity_from_titles(&titles, "PhotoCraft").as_deref(),
            Some("Exporting")
        );
        assert_eq!(
            pick_document_title(&titles, "PhotoCraft"),
            Some("Poster — PhotoCraft")
        );
    }

    #[test]
    fn document_named_export_is_not_a_dialog() {
        let titles = vec!["Export plan — PhotoCraft".to_string()];
        assert_eq!(activity_from_titles(&titles, "PhotoCraft"), None);
    }

    #[test]
    fn document_title_skips_dialog_windows() {
        let titles = vec!["Save As".to_string(), "Logo".to_string()];
        assert_eq!(pick_document_title(&titles, "PhotoCraft"), Some("Logo"));
    }

    macro_rules! app_suffix_tests {
        ($app:literal) => {
            #[test]
            fn em_dash_suffix_is_removed() {
                assert_eq!(
                    strip_title(&format!("Project — {}", $app), $app).as_deref(),
                    Some("Project")
                );
            }
            #[test]
            fn en_dash_suffix_is_removed() {
                assert_eq!(
                    strip_title(&format!("Project – {}", $app), $app).as_deref(),
                    Some("Project")
                );
            }
            #[test]
            fn hyphen_suffix_is_removed() {
                assert_eq!(
                    strip_title(&format!("Project - {}", $app), $app).as_deref(),
                    Some("Project")
                );
            }
            #[test]
            fn pipe_suffix_is_removed() {
                assert_eq!(
                    strip_title(&format!("Project | {}", $app), $app).as_deref(),
                    Some("Project")
                );
            }
            #[test]
            fn em_dash_app_suffix_is_case_insensitive() {
                assert_eq!(
                    strip_title(&format!("Project — {}", $app.to_uppercase()), $app).as_deref(),
                    Some("Project")
                );
            }
            #[test]
            fn en_dash_app_suffix_is_case_insensitive() {
                assert_eq!(
                    strip_title(&format!("Project – {}", $app.to_uppercase()), $app).as_deref(),
                    Some("Project")
                );
            }
            #[test]
            fn hyphen_app_suffix_is_case_insensitive() {
                assert_eq!(
                    strip_title(&format!("Project - {}", $app.to_uppercase()), $app).as_deref(),
                    Some("Project")
                );
            }
            #[test]
            fn pipe_app_suffix_is_case_insensitive() {
                assert_eq!(
                    strip_title(&format!("Project | {}", $app.to_uppercase()), $app).as_deref(),
                    Some("Project")
                );
            }
            #[test]
            fn em_dash_keeps_other_application_suffix() {
                let expected = format!("Project — {}", $app);
                assert_eq!(
                    strip_title(&expected, "OtherCraft").as_deref(),
                    Some(expected.as_str())
                );
            }
            #[test]
            fn en_dash_keeps_other_application_suffix() {
                let expected = format!("Project – {}", $app);
                assert_eq!(
                    strip_title(&expected, "OtherCraft").as_deref(),
                    Some(expected.as_str())
                );
            }
            #[test]
            fn hyphen_keeps_other_application_suffix() {
                let expected = format!("Project - {}", $app);
                assert_eq!(
                    strip_title(&expected, "OtherCraft").as_deref(),
                    Some(expected.as_str())
                );
            }
            #[test]
            fn pipe_keeps_other_application_suffix() {
                let expected = format!("Project | {}", $app);
                assert_eq!(
                    strip_title(&expected, "OtherCraft").as_deref(),
                    Some(expected.as_str())
                );
            }
            #[test]
            fn process_name_matches_without_exe_extension() {
                assert!(process_matches($app, $app));
            }
            #[test]
            fn process_name_matches_with_exe_extension() {
                assert!(process_matches(&format!("{}.exe", $app), $app));
            }
            #[test]
            fn process_name_matching_ignores_case() {
                assert!(process_matches(&$app.to_lowercase(), $app));
            }
            #[test]
            fn process_name_matching_rejects_similar_names() {
                assert!(!process_matches(&format!("{}Beta", $app), $app));
            }
        };
    }

    mod photo_craft {
        use super::*;
        app_suffix_tests!("PhotoCraft");
    }
    mod vector_craft {
        use super::*;
        app_suffix_tests!("VectorCraft");
    }
    mod film_craft {
        use super::*;
        app_suffix_tests!("FilmCraft");
    }
    mod light_craft {
        use super::*;
        app_suffix_tests!("LightCraft");
    }
    mod pdf_craft {
        use super::*;
        app_suffix_tests!("PdfCraft");
    }
    mod effect_craft {
        use super::*;
        app_suffix_tests!("EffectCraft");
    }
    mod design_craft {
        use super::*;
        app_suffix_tests!("DesignCraft");
    }
}
