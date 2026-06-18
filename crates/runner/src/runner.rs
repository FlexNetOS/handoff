use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

use crate::config::TuiConfig;
use crate::data;

/// Messages sent from the worker thread to the TUI.
#[derive(Debug)]
pub enum ImplUpdate {
    Progress { completed: u32, total: u32 },
    Finished { success: bool },
    Stalled,
    Error(String),
}

/// State for a running implementation process.
pub struct ImplState {
    pub change_name: String,
    pub completed: u32,
    pub total: u32,
    pub log_path: PathBuf,
    pub receiver: Receiver<ImplUpdate>,
    pub cancel_flag: Arc<AtomicBool>,
    pub child_handle: Arc<Mutex<Option<Child>>>,
}

/// State for a batch implementation run across multiple changes.
///
/// Tracks which changes to run (in topological order), the current position,
/// and which changes have completed, failed, or been skipped.
pub struct BatchImplState {
    /// Topologically sorted change names to execute.
    pub queue: Vec<String>,
    /// Index of the currently running change in `queue`.
    pub current_index: usize,
    /// Changes that completed successfully.
    pub completed: HashSet<String>,
    /// Changes that failed during execution.
    pub failed: HashSet<String>,
    /// Changes skipped because a dependency failed or was skipped.
    pub skipped: HashSet<String>,
    /// Map of change name to its dependencies (used for failure propagation).
    pub deps: HashMap<String, Vec<String>>,
}

impl BatchImplState {
    /// Create a new batch state from a topologically sorted queue and their dependencies.
    pub fn new(queue: Vec<String>, deps: HashMap<String, Vec<String>>) -> Self {
        Self {
            queue,
            current_index: 0,
            completed: HashSet::new(),
            failed: HashSet::new(),
            skipped: HashSet::new(),
            deps,
        }
    }

    /// Returns the total number of changes in the batch.
    pub fn total(&self) -> usize {
        self.queue.len()
    }

    /// Check whether a change should be skipped because one of its
    /// dependencies (transitively) has failed or been skipped.
    pub fn should_skip(&self, change_name: &str) -> bool {
        let mut visited = HashSet::new();
        self.has_failed_dependency(change_name, &mut visited)
    }

    fn has_failed_dependency(&self, change_name: &str, visited: &mut HashSet<String>) -> bool {
        if !visited.insert(change_name.to_string()) {
            return false;
        }
        if let Some(dep_list) = self.deps.get(change_name) {
            for dep in dep_list {
                if self.failed.contains(dep) || self.skipped.contains(dep) {
                    return true;
                }
                if self.has_failed_dependency(dep, visited) {
                    return true;
                }
            }
        }
        false
    }

    /// Advance to the next change after the current one has finished or failed.
    ///
    /// - If `success` is true, the current change is added to `completed`.
    /// - If `success` is false, the current change is added to `failed`.
    ///
    /// Then advances `current_index`, skipping any changes whose dependencies
    /// have failed. Returns the name of the next change to run, or `None` if
    /// the batch is finished.
    pub fn advance(&mut self, success: bool) -> Option<String> {
        if let Some(current) = self.queue.get(self.current_index).cloned() {
            if success {
                self.completed.insert(current);
            } else {
                self.failed.insert(current);
            }
        }
        self.current_index += 1;

        // Skip changes whose dependencies failed
        while self.current_index < self.queue.len() {
            let name = &self.queue[self.current_index];
            if self.should_skip(name) {
                self.skipped.insert(name.clone());
                self.current_index += 1;
            } else {
                break;
            }
        }

        self.queue.get(self.current_index).cloned()
    }
}

#[cfg(test)]
impl BatchImplState {
    /// Returns the name of the currently running change, if any.
    pub fn current_change(&self) -> Option<&str> {
        self.queue.get(self.current_index).map(|s| s.as_str())
    }

    /// Returns true if the batch has finished (all changes processed).
    pub fn is_finished(&self) -> bool {
        self.current_index >= self.queue.len()
    }
}

/// Stop a running implementation by setting the cancel flag and killing the
/// active child process.
pub fn stop_implementation(state: &ImplState) {
    state.cancel_flag.store(true, Ordering::Relaxed);
    if let Ok(mut handle) = state.child_handle.lock()
        && let Some(ref mut child) = *handle
        && let Err(e) = child.kill()
    {
        eprintln!("Failed to kill child process {}: {}", child.id(), e);
    }
}

/// Start an implementation runner for the given change.
///
/// Spawns a worker thread that loops through unfinished tasks in tasks.md,
/// invoking the configured command for each one. Command and prompt are
/// driven by `TuiConfig`. Output is redirected to a log file. Progress
/// updates are sent via the mpsc channel stored in the returned `ImplState`.
pub fn start_implementation(change_name: &str, config: &TuiConfig) -> ImplState {
    let tasks_path = PathBuf::from("openspec/changes")
        .join(change_name)
        .join("tasks.md");
    let log_path = PathBuf::from("openspec/changes")
        .join(change_name)
        .join("implementation.log");

    let (tx, rx) = mpsc::channel();
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let child_handle: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));

    // Read initial progress
    let (completed, total) = match data::parse_task_progress(&tasks_path) {
        Ok(res) => res,
        Err(e) => {
            let msg = format!("Failed to read task progress: {}", e);
            if tx.send(ImplUpdate::Error(msg.clone())).is_err() {
                eprintln!("Intent: {}", msg);
            }
            (0, 0) // Fallback for struct initialization, but thread will handle error
        }
    };

    let worker_cancel = cancel_flag.clone();
    let worker_child = child_handle.clone();
    let worker_log_path = log_path.clone();
    let worker_change_name = change_name.to_string();
    let worker_config = config.clone();

    std::thread::spawn(move || {
        implementation_loop(
            &worker_change_name,
            &tasks_path,
            &worker_log_path,
            &tx,
            &worker_cancel,
            &worker_child,
            &worker_config,
        );
    });

    ImplState {
        change_name: change_name.to_string(),
        completed,
        total,
        log_path,
        receiver: rx,
        cancel_flag,
        child_handle,
    }
}

fn write_task_header(
    log_path: &Path,
    task_number: u32,
    total: u32,
    task_text: &str,
) -> Result<(), std::io::Error> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    writeln!(
        file,
        "──────────────────────────────────────────────────────────────"
    )?;
    writeln!(file, "Task {}/{}: {}", task_number, total, task_text)?;
    writeln!(
        file,
        "──────────────────────────────────────────────────────────────"
    )?;
    Ok(())
}

fn write_run_header(log_path: &Path, change_name: &str) -> Result<(), std::io::Error> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    writeln!(
        file,
        "══════════════════════════════════════════════════════════════"
    )?;
    writeln!(file, "IMPLEMENTATION RUN STARTED")?;
    writeln!(file, "Time: {}", timestamp)?;
    writeln!(file, "Change: {}", change_name)?;
    writeln!(
        file,
        "══════════════════════════════════════════════════════════════"
    )?;
    Ok(())
}

/// Start an apply-mode runner for the given change.
///
/// Spawns a single subprocess that runs `/opsx:apply <name>` via the
/// configured command. Output is redirected to `implementation.log`.
/// No task tracking or stall detection — only a `Finished` message
/// is sent when the process completes.
pub fn start_apply(change_name: &str, config: &TuiConfig) -> ImplState {
    let log_path = PathBuf::from("openspec/changes")
        .join(change_name)
        .join("implementation.log");

    let (tx, rx) = mpsc::channel();
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let child_handle: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));

    let worker_cancel = cancel_flag.clone();
    let worker_child = child_handle.clone();
    let worker_log_path = log_path.clone();
    let worker_change_name = change_name.to_string();
    let worker_config = config.clone();

    std::thread::spawn(move || {
        apply_run(
            &worker_change_name,
            &worker_log_path,
            &tx,
            &worker_cancel,
            &worker_child,
            &worker_config,
        );
    });

    ImplState {
        change_name: change_name.to_string(),
        completed: 0,
        total: 0,
        log_path,
        receiver: rx,
        cancel_flag,
        child_handle,
    }
}

fn apply_run(
    change_name: &str,
    log_path: &PathBuf,
    tx: &mpsc::Sender<ImplUpdate>,
    cancel_flag: &Arc<AtomicBool>,
    child_handle: &Arc<Mutex<Option<Child>>>,
    config: &TuiConfig,
) {
    if let Err(e) = write_run_header(log_path, change_name) {
        let msg = format!(
            "Failed to write run header to {}: {}",
            log_path.display(),
            e
        );
        if tx.send(ImplUpdate::Error(msg.clone())).is_err() {
            eprintln!("Intent: {}", msg);
        }
    }

    let prompt = format!("/opsx:apply {}", change_name);

    let log_file = match OpenOptions::new().create(true).append(true).open(log_path) {
        Ok(f) => f,
        Err(e) => {
            let msg = format!("Failed to open log file {}: {}", log_path.display(), e);
            if tx.send(ImplUpdate::Error(msg.clone())).is_err() {
                eprintln!("Intent: {}", msg);
            }
            if tx.send(ImplUpdate::Finished { success: false }).is_err() {
                eprintln!("Intent: Finished {{ success: false }}");
            }
            return;
        }
    };
    let stderr_log = match log_file.try_clone() {
        Ok(f) => f,
        Err(e) => {
            let msg = format!(
                "Failed to clone log file handle for {}: {}",
                log_path.display(),
                e
            );
            if tx.send(ImplUpdate::Error(msg.clone())).is_err() {
                eprintln!("Intent: {}", msg);
            }
            if tx.send(ImplUpdate::Finished { success: false }).is_err() {
                eprintln!("Intent: Finished {{ success: false }}");
            }
            return;
        }
    };

    let child_result = match config.build_command(&prompt) {
        Some((binary, args)) => std::process::Command::new(&binary)
            .args(&args)
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(stderr_log))
            .spawn(),
        None => {
            let msg = format!("Failed to build command for prompt: {}", prompt);
            if tx.send(ImplUpdate::Error(msg.clone())).is_err() {
                eprintln!("Intent: {}", msg);
            }
            if tx.send(ImplUpdate::Finished { success: false }).is_err() {
                eprintln!("Intent: Finished {{ success: false }}");
            }
            return;
        }
    };

    let child = match child_result {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("Failed to spawn process: {}", e);
            if tx.send(ImplUpdate::Error(msg.clone())).is_err() {
                eprintln!("Intent: {}", msg);
            }
            if tx.send(ImplUpdate::Finished { success: false }).is_err() {
                eprintln!("Intent: Finished {{ success: false }}");
            }
            return;
        }
    };

    {
        let mut handle = child_handle.lock().unwrap_or_else(|e| e.into_inner());
        *handle = Some(child);
    }

    let exited_ok = loop {
        if cancel_flag.load(Ordering::Relaxed) {
            break false;
        }

        let try_result = {
            let mut handle = child_handle.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(ref mut c) = *handle {
                c.try_wait()
            } else {
                break false;
            }
        };

        match try_result {
            Ok(Some(status)) => break status.success(),
            Ok(None) => {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(_) => break false,
        }
    };

    {
        let mut handle = child_handle.lock().unwrap_or_else(|e| e.into_inner());
        *handle = None;
    }

    if cancel_flag.load(Ordering::Relaxed) {
        return;
    }

    if tx
        .send(ImplUpdate::Finished { success: exited_ok })
        .is_err()
    {
        eprintln!("Intent: Finished {{ success: {} }}", exited_ok);
    }
}

/// Maximum consecutive runs with no task progress before aborting.
const STALL_THRESHOLD: u32 = 3;

fn implementation_loop(
    change_name: &str,
    tasks_path: &Path,
    log_path: &Path,
    tx: &mpsc::Sender<ImplUpdate>,
    cancel_flag: &Arc<AtomicBool>,
    child_handle: &Arc<Mutex<Option<Child>>>,
    config: &TuiConfig,
) {
    // Write run header before starting the task loop
    if let Err(e) = write_run_header(log_path, change_name) {
        let msg = format!(
            "Failed to write run header to {}: {}",
            log_path.display(),
            e
        );
        if tx.send(ImplUpdate::Error(msg.clone())).is_err() {
            eprintln!("Intent: {}", msg);
        }
    }

    // Stall detection: track consecutive runs without progress
    let mut stall_count: u32 = 0;
    let mut prev_completed: u32 = match data::parse_task_progress(tasks_path) {
        Ok((c, _)) => c,
        Err(e) => {
            let msg = format!("Failed to read task progress: {}", e);
            if tx.send(ImplUpdate::Error(msg.clone())).is_err() {
                eprintln!("Intent: {}", msg);
            }
            0
        }
    };

    // Track loop exit reason
    let mut tasks_complete = false;
    let mut stalled = false;

    loop {
        // Check cancellation
        if cancel_flag.load(Ordering::Relaxed) {
            break;
        }

        // Check if there are unchecked tasks remaining
        let (completed, total) = match data::parse_task_progress(tasks_path) {
            Ok(res) => res,
            Err(e) => {
                let msg = format!("Failed to read task progress: {}", e);
                if tx.send(ImplUpdate::Error(msg.clone())).is_err() {
                    eprintln!("Intent: {}", msg);
                }
                break;
            }
        };
        if completed >= total || total == 0 {
            tasks_complete = true;
            break;
        }

        // Open log file for appending
        let log_file = match OpenOptions::new().create(true).append(true).open(log_path) {
            Ok(f) => f,
            Err(e) => {
                let msg = format!("Failed to open log file {}: {}", log_path.display(), e);
                if tx.send(ImplUpdate::Error(msg.clone())).is_err() {
                    eprintln!("Intent: {}", msg);
                }
                break;
            }
        };
        let stderr_log = match log_file.try_clone() {
            Ok(f) => f,
            Err(e) => {
                let msg = format!(
                    "Failed to clone log file handle for {}: {}",
                    log_path.display(),
                    e
                );
                if tx.send(ImplUpdate::Error(msg.clone())).is_err() {
                    eprintln!("Intent: {}", msg);
                }
                break;
            }
        };

        // Write task header before spawning claude
        let (_, total) = match data::parse_task_progress(tasks_path) {
            Ok(res) => res,
            Err(e) => {
                let msg = format!("Failed to read task progress: {}", e);
                if tx.send(ImplUpdate::Error(msg.clone())).is_err() {
                    eprintln!("Intent: {}", msg);
                }
                break;
            }
        };
        if let Some((task_num, task_text)) = data::next_unchecked_task(tasks_path)
            && let Err(e) = write_task_header(log_path, task_num, total, &task_text)
        {
            let msg = format!(
                "Failed to write task header to {}: {}",
                log_path.display(),
                e
            );
            if tx.send(ImplUpdate::Error(msg.clone())).is_err() {
                eprintln!("Intent: {}", msg);
            }
        }

        let prompt = config.render_prompt(change_name);

        // Spawn process using config-driven command
        let child_result = match config.build_command(&prompt) {
            Some((binary, args)) => std::process::Command::new(&binary)
                .args(&args)
                .stdout(Stdio::from(log_file))
                .stderr(Stdio::from(stderr_log))
                .spawn(),
            None => {
                let msg = format!("Failed to build command for prompt: {}", prompt);
                if tx.send(ImplUpdate::Error(msg.clone())).is_err() {
                    eprintln!("Intent: {}", msg);
                }
                break;
            }
        };

        let child = match child_result {
            Ok(c) => c,
            Err(e) => {
                let msg = format!("Failed to spawn process for task: {}", e);
                if tx.send(ImplUpdate::Error(msg.clone())).is_err() {
                    eprintln!("Intent: {}", msg);
                }
                // Spawn failure counts toward stall detection
                stall_count += 1;
                if stall_count >= STALL_THRESHOLD {
                    stalled = true;
                    break;
                }
                continue;
            }
        };

        // Store child handle so main thread can kill it
        {
            let mut handle = child_handle.lock().unwrap_or_else(|e| e.into_inner());
            *handle = Some(child);
        }

        // Poll for child completion using try_wait so we don't hold the
        // mutex lock. This allows the main thread to lock the mutex and
        // kill the child process for cancellation.
        let _exited_ok = loop {
            if cancel_flag.load(Ordering::Relaxed) {
                break false;
            }

            let try_result = {
                let mut handle = child_handle.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(ref mut c) = *handle {
                    c.try_wait()
                } else {
                    break false;
                }
            };

            match try_result {
                Ok(Some(status)) => break status.success(),
                Ok(None) => {
                    // Process still running, wait briefly before polling again
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(e) => {
                    let msg = format!("Error waiting for process: {}", e);
                    if tx.send(ImplUpdate::Error(msg.clone())).is_err() {
                        eprintln!("Intent: {}", msg);
                    }
                    break false;
                }
            }
        };

        // Clear child handle
        {
            let mut handle = child_handle.lock().unwrap_or_else(|e| e.into_inner());
            *handle = None;
        }

        // If cancelled, stop without stall detection
        if cancel_flag.load(Ordering::Relaxed) {
            break;
        }

        // Re-read progress and check for stall (regardless of exit code)
        let (completed, total) = match data::parse_task_progress(tasks_path) {
            Ok(res) => res,
            Err(e) => {
                let msg = format!("Failed to read task progress: {}", e);
                if tx.send(ImplUpdate::Error(msg.clone())).is_err() {
                    eprintln!("Intent: {}", msg);
                }
                break;
            }
        };

        if completed > prev_completed {
            // Progress was made — reset stall counter
            stall_count = 0;
            prev_completed = completed;
        } else {
            // No progress — increment stall counter
            stall_count += 1;
            if stall_count >= STALL_THRESHOLD {
                stalled = true;
                break;
            }
        }

        if tx.send(ImplUpdate::Progress { completed, total }).is_err() {
            eprintln!("Failed to send Progress update: {}/{}", completed, total);
            break;
        }

        // If all tasks completed, finish
        if completed >= total {
            tasks_complete = true;
            break;
        }
    }

    // Don't send anything if cancelled
    if cancel_flag.load(Ordering::Relaxed) {
        return;
    }

    if stalled {
        if tx.send(ImplUpdate::Stalled).is_err() {
            eprintln!("Intent: Stalled");
        }
        return;
    }

    // Run post-implementation hook if tasks completed successfully
    let mut success = tasks_complete;
    if tasks_complete
        && let Some(post_prompt) = config.render_post_prompt(change_name)
        && let Some((binary, args)) = config.build_command(&post_prompt)
    {
        // Open log file for hook output
        let hook_result = match OpenOptions::new().create(true).append(true).open(log_path) {
            Ok(log_file) => {
                match log_file.try_clone() {
                    Ok(stderr_log) => {
                        match std::process::Command::new(&binary)
                            .args(&args)
                            .stdout(Stdio::from(log_file))
                            .stderr(Stdio::from(stderr_log))
                            .spawn()
                        {
                            Ok(child) => {
                                // Store child handle for cancellation
                                {
                                    let mut handle =
                                        child_handle.lock().unwrap_or_else(|e| e.into_inner());
                                    *handle = Some(child);
                                }
                                // Poll for completion
                                let exited_ok = loop {
                                    if cancel_flag.load(Ordering::Relaxed) {
                                        break false;
                                    }
                                    let try_result = {
                                        let mut handle =
                                            child_handle.lock().unwrap_or_else(|e| e.into_inner());
                                        if let Some(ref mut c) = *handle {
                                            c.try_wait()
                                        } else {
                                            break false;
                                        }
                                    };
                                    match try_result {
                                        Ok(Some(status)) => break status.success(),
                                        Ok(None) => {
                                            std::thread::sleep(std::time::Duration::from_millis(
                                                100,
                                            ));
                                        }
                                        Err(e) => {
                                            let msg =
                                                format!("Error waiting for hook process: {}", e);
                                            if tx.send(ImplUpdate::Error(msg.clone())).is_err() {
                                                eprintln!("Intent: {}", msg);
                                            }
                                            break false;
                                        }
                                    }
                                };
                                // Clear child handle
                                {
                                    let mut handle =
                                        child_handle.lock().unwrap_or_else(|e| e.into_inner());
                                    *handle = None;
                                }
                                Ok(exited_ok)
                            }
                            Err(e) => Err(format!("Failed to spawn hook process: {}", e)),
                        }
                    }
                    Err(e) => Err(format!("Failed to clone log file for hook: {}", e)),
                }
            }
            Err(e) => Err(format!("Failed to open log file for hook: {}", e)),
        };

        match hook_result {
            Ok(ok) => success = ok,
            Err(msg) => {
                if tx.send(ImplUpdate::Error(msg.clone())).is_err() {
                    eprintln!("Intent: {}", msg);
                }
                success = false;
            }
        }
    }

    if tx.send(ImplUpdate::Finished { success }).is_err() {
        eprintln!("Intent: Finished {{ success: {} }}", success);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn test_impl_state_creation() {
        let (tx, rx) = mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let child_handle = Arc::new(Mutex::new(None));

        let state = ImplState {
            change_name: "test-change".to_string(),
            completed: 2,
            total: 5,
            log_path: PathBuf::from("openspec/changes/test-change/implementation.log"),
            receiver: rx,
            cancel_flag: cancel_flag.clone(),
            child_handle: child_handle.clone(),
        };

        assert_eq!(state.change_name, "test-change");
        assert_eq!(state.completed, 2);
        assert_eq!(state.total, 5);
        assert_eq!(
            state.log_path,
            PathBuf::from("openspec/changes/test-change/implementation.log")
        );
        assert!(!state.cancel_flag.load(std::sync::atomic::Ordering::Relaxed));
        assert!(
            state
                .child_handle
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_none()
        );

        // Verify channel works
        tx.send(ImplUpdate::Progress {
            completed: 3,
            total: 5,
        })
        .unwrap();
        let msg = state.receiver.recv().unwrap();
        match msg {
            ImplUpdate::Progress { completed, total } => {
                assert_eq!(completed, 3);
                assert_eq!(total, 5);
            }
            _ => {
                panic!("Expected Progress, got {:?}", msg)
            }
        }
    }

    #[test]
    fn test_impl_update_finished() {
        let (tx, rx) = mpsc::channel();
        tx.send(ImplUpdate::Finished { success: true }).unwrap();
        let msg = rx.recv().unwrap();
        assert!(matches!(msg, ImplUpdate::Finished { success: true }));
    }

    #[test]
    fn test_impl_update_stalled() {
        let (tx, rx) = mpsc::channel();
        tx.send(ImplUpdate::Stalled).unwrap();
        let msg = rx.recv().unwrap();
        assert!(matches!(msg, ImplUpdate::Stalled));
    }

    #[test]
    fn test_impl_update_stalled_after_progress() {
        let (tx, rx) = mpsc::channel();
        tx.send(ImplUpdate::Progress {
            completed: 1,
            total: 5,
        })
        .unwrap();
        tx.send(ImplUpdate::Stalled).unwrap();

        match rx.recv().unwrap() {
            ImplUpdate::Progress { completed, total } => {
                assert_eq!(completed, 1);
                assert_eq!(total, 5);
            }
            _ => panic!("Expected Progress"),
        }
        assert!(matches!(rx.recv().unwrap(), ImplUpdate::Stalled));
    }

    #[test]
    fn test_cancel_flag_shared() {
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let flag_clone = cancel_flag.clone();

        // Simulate main thread setting cancel
        flag_clone.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(cancel_flag.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn test_stop_implementation_sets_cancel_flag() {
        let (_, rx) = mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let child_handle = Arc::new(Mutex::new(None));

        let state = ImplState {
            change_name: "test-change".to_string(),
            completed: 0,
            total: 5,
            log_path: PathBuf::from("openspec/changes/test-change/implementation.log"),
            receiver: rx,
            cancel_flag: cancel_flag.clone(),
            child_handle: child_handle.clone(),
        };

        assert!(!cancel_flag.load(Ordering::Relaxed));
        stop_implementation(&state);
        assert!(cancel_flag.load(Ordering::Relaxed));
    }

    #[test]
    fn test_stop_implementation_kills_child_process() {
        let (_, rx) = mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let child_handle = Arc::new(Mutex::new(None));

        // Spawn a long-running child process to test killing
        let child = std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .expect("failed to spawn sleep process");
        let child_id = child.id();
        *child_handle.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);

        let state = ImplState {
            change_name: "test-change".to_string(),
            completed: 0,
            total: 5,
            log_path: PathBuf::from("openspec/changes/test-change/implementation.log"),
            receiver: rx,
            cancel_flag: cancel_flag.clone(),
            child_handle: child_handle.clone(),
        };

        stop_implementation(&state);

        // Cancel flag should be set
        assert!(cancel_flag.load(Ordering::Relaxed));

        // Child process should have been killed - wait for it to confirm
        if let Some(ref mut child) = *child_handle.lock().unwrap_or_else(|e| e.into_inner()) {
            let status = child.wait().expect("failed to wait on child");
            assert!(!status.success(), "child should have been killed");
        }

        // Verify process is no longer running (check via /proc on linux)
        assert!(
            !std::path::Path::new(&format!("/proc/{child_id}")).exists(),
            "process should no longer exist"
        );
    }

    #[test]
    fn test_stop_implementation_no_child() {
        // stop_implementation should not panic when there is no child process
        let (_, rx) = mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let child_handle: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));

        let state = ImplState {
            change_name: "test-change".to_string(),
            completed: 0,
            total: 5,
            log_path: PathBuf::from("openspec/changes/test-change/implementation.log"),
            receiver: rx,
            cancel_flag: cancel_flag.clone(),
            child_handle,
        };

        stop_implementation(&state);
        assert!(cancel_flag.load(Ordering::Relaxed));
    }

    #[test]
    fn test_child_handle_shared() {
        let child_handle: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
        let handle_clone = child_handle.clone();

        // Verify both references see the same state
        assert!(
            handle_clone
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_none()
        );
        assert!(
            child_handle
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_none()
        );
    }

    #[test]
    fn test_poisoned_mutex_recovery() {
        let mutex = Arc::new(Mutex::new(Some(1)));
        let m_clone = mutex.clone();

        let _ = std::thread::spawn(move || {
            let _guard = m_clone.lock().unwrap();
            panic!("Poisoning the mutex");
        })
        .join();

        assert!(mutex.is_poisoned());

        // This should not panic
        let guard = mutex.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(*guard, Some(1));
    }

    #[test]
    fn test_cancel_flag_stops_loop() {
        // Create a tasks file with uncompleted tasks
        let tmp_dir = tempfile::tempdir().unwrap();
        let dir = tmp_dir.path();
        let change_dir = dir.join("openspec/changes/test-cancel");
        std::fs::create_dir_all(&change_dir).unwrap();
        let tasks_path = change_dir.join("tasks.md");
        std::fs::write(&tasks_path, "- [ ] Task one\n- [ ] Task two\n").unwrap();

        let (tx, rx) = mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(true)); // Pre-set cancel
        let child_handle: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
        let log_path = dir.join("test.log");

        // Run the loop — it should exit immediately due to cancel flag
        implementation_loop(
            "test-cancel",
            &tasks_path,
            &log_path,
            &tx,
            &cancel_flag,
            &child_handle,
            &TuiConfig::default(),
        );

        // The loop should not have sent any Progress message
        // It may or may not send Finished (implementation breaks before sending),
        // but it must not hang or send Progress.
        let mut got_progress = false;
        while let Ok(msg) = rx.try_recv() {
            if matches!(msg, ImplUpdate::Progress { .. }) {
                got_progress = true;
            }
        }
        assert!(
            !got_progress,
            "Loop should not send Progress when cancelled"
        );
    }

    #[test]
    fn test_loop_finishes_when_all_tasks_complete() {
        // Create a tasks file where all tasks are already done
        let tmp_dir = tempfile::tempdir().unwrap();
        let dir = tmp_dir.path();
        let change_dir = dir.join("openspec/changes/test-done");
        std::fs::create_dir_all(&change_dir).unwrap();
        let tasks_path = change_dir.join("tasks.md");
        std::fs::write(&tasks_path, "- [x] Task one\n- [x] Task two\n").unwrap();

        let (tx, rx) = mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let child_handle: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
        let log_path = dir.join("test.log");

        implementation_loop(
            "test-done",
            &tasks_path,
            &log_path,
            &tx,
            &cancel_flag,
            &child_handle,
            &TuiConfig::default(),
        );

        // Should receive Finished { success: true } since all tasks are complete
        let msg = rx.recv().unwrap();
        assert!(matches!(msg, ImplUpdate::Finished { success: true }));
    }

    #[test]
    fn test_loop_sends_stalled_on_repeated_spawn_failure() {
        // Create a tasks file with uncompleted tasks so the loop tries to spawn
        let tmp_dir = tempfile::tempdir().unwrap();
        let dir = tmp_dir.path();
        let change_dir = dir.join("openspec/changes/test-spawn");
        std::fs::create_dir_all(&change_dir).unwrap();
        let tasks_path = change_dir.join("tasks.md");
        std::fs::write(&tasks_path, "- [ ] Task one\n").unwrap();

        let config = TuiConfig {
            command: "definitely-nonexistent-binary-xyz {prompt}".to_string(),
            prompt: "test {name}".to_string(),
            ..Default::default()
        };

        let (tx, rx) = mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let child_handle: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
        let log_path = dir.join("test.log");

        implementation_loop(
            "test-spawn",
            &tasks_path,
            &log_path,
            &tx,
            &cancel_flag,
            &child_handle,
            &config,
        );

        // Should receive 3 Errors then Stalled after 3 consecutive spawn failures
        for _ in 0..3 {
            let msg = rx.recv().unwrap();
            assert!(matches!(msg, ImplUpdate::Error(_)));
        }
        let msg = rx.recv().unwrap();
        assert!(matches!(msg, ImplUpdate::Stalled));
    }

    #[test]
    fn test_config_render_prompt_contains_all_context_references() {
        let config = TuiConfig::default();
        let prompt = config.render_prompt("my-change");
        assert!(
            prompt.contains("openspec/config.yaml"),
            "prompt should reference config.yaml"
        );
        assert!(
            prompt.contains("openspec/changes/my-change/proposal.md"),
            "prompt should reference proposal.md"
        );
        assert!(
            prompt.contains("openspec/changes/my-change/design.md"),
            "prompt should reference design.md"
        );
        assert!(
            prompt.contains("openspec/changes/my-change/specs/"),
            "prompt should reference change specs directory"
        );
        assert!(
            prompt.contains("openspec/specs/"),
            "prompt should reference global specs directory"
        );
        assert!(
            prompt.contains("openspec/changes/my-change/tasks.md"),
            "prompt should reference tasks.md"
        );
        assert!(
            !prompt.contains("{name}"),
            "prompt should not contain unsubstituted {{name}} placeholder"
        );
    }

    #[test]
    fn test_progress_counting_via_channel() {
        // Verify that ImplUpdate::Progress carries correct counts
        let (tx, rx) = mpsc::channel();

        tx.send(ImplUpdate::Progress {
            completed: 3,
            total: 7,
        })
        .unwrap();
        tx.send(ImplUpdate::Progress {
            completed: 5,
            total: 7,
        })
        .unwrap();
        tx.send(ImplUpdate::Finished { success: true }).unwrap();

        match rx.recv().unwrap() {
            ImplUpdate::Progress { completed, total } => {
                assert_eq!(completed, 3);
                assert_eq!(total, 7);
            }
            _ => panic!("Expected Progress"),
        }
        match rx.recv().unwrap() {
            ImplUpdate::Progress { completed, total } => {
                assert_eq!(completed, 5);
                assert_eq!(total, 7);
            }
            _ => panic!("Expected Progress"),
        }
        assert!(matches!(
            rx.recv().unwrap(),
            ImplUpdate::Finished { success: true }
        ));
    }

    #[test]
    fn test_write_run_header_creates_file_with_content() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let dir = tmp_dir.path();
        let log_path = dir.join("implementation.log");

        write_run_header(&log_path, "my-change").unwrap();

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("══"), "Should contain separator lines");
        assert!(
            content.contains("IMPLEMENTATION RUN STARTED"),
            "Should contain run started text"
        );
        assert!(content.contains("Time:"), "Should contain timestamp label");
        assert!(
            content.contains("Change: my-change"),
            "Should contain change name"
        );
    }

    #[test]
    fn test_write_run_header_appends_on_second_call() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let dir = tmp_dir.path();
        let log_path = dir.join("implementation.log");

        write_run_header(&log_path, "change-a").unwrap();
        write_run_header(&log_path, "change-b").unwrap();

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(
            content.contains("Change: change-a"),
            "First run header should be present"
        );
        assert!(
            content.contains("Change: change-b"),
            "Second run header should be present"
        );
        let count = content.matches("IMPLEMENTATION RUN STARTED").count();
        assert_eq!(count, 2, "Should have two run headers");
    }

    #[test]
    fn test_implementation_loop_writes_run_header() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let dir = tmp_dir.path();
        let change_dir = dir.join("openspec/changes/test-header");
        std::fs::create_dir_all(&change_dir).unwrap();
        let tasks_path = change_dir.join("tasks.md");
        std::fs::write(&tasks_path, "- [x] Task one\n- [x] Task two\n").unwrap();
        let log_path = dir.join("test.log");

        let (tx, _rx) = mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let child_handle: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));

        implementation_loop(
            "test-header",
            &tasks_path,
            &log_path,
            &tx,
            &cancel_flag,
            &child_handle,
            &TuiConfig::default(),
        );

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(
            content.contains("IMPLEMENTATION RUN STARTED"),
            "Run header should be written even when all tasks are complete"
        );
        assert!(
            content.contains("Change: test-header"),
            "Run header should contain change name"
        );
    }

    #[test]
    fn test_write_task_header_creates_file_with_content() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let dir = tmp_dir.path();
        let log_path = dir.join("implementation.log");

        write_task_header(&log_path, 3, 7, "Implement the widget").unwrap();

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("──"), "Should contain separator lines");
        assert!(
            content.contains("Task 3/7: Implement the widget"),
            "Should contain task number and description"
        );
    }

    #[test]
    fn test_write_task_header_appends() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let dir = tmp_dir.path();
        let log_path = dir.join("implementation.log");

        write_task_header(&log_path, 1, 3, "First task").unwrap();
        write_task_header(&log_path, 2, 3, "Second task").unwrap();

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(
            content.contains("Task 1/3: First task"),
            "First task header should be present"
        );
        assert!(
            content.contains("Task 2/3: Second task"),
            "Second task header should be present"
        );
    }

    #[test]
    fn test_implementation_loop_writes_task_header() {
        // Create a tasks file with one unchecked task so the loop tries to spawn
        // claude (which will fail in test env), but should still write the task header
        let tmp_dir = tempfile::tempdir().unwrap();
        let dir = tmp_dir.path();
        let change_dir = dir.join("openspec/changes/test-task-hdr");
        std::fs::create_dir_all(&change_dir).unwrap();
        let tasks_path = change_dir.join("tasks.md");
        std::fs::write(&tasks_path, "- [x] Done task\n- [ ] Pending task\n").unwrap();
        let log_path = dir.join("test.log");

        let (tx, _rx) = mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let child_handle: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));

        implementation_loop(
            "test-task-hdr",
            &tasks_path,
            &log_path,
            &tx,
            &cancel_flag,
            &child_handle,
            &TuiConfig::default(),
        );

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(
            content.contains("IMPLEMENTATION RUN STARTED"),
            "Run header should be present"
        );
        assert!(
            content.contains("Task 2/2: Pending task"),
            "Task header should contain task number and description"
        );
        assert!(
            content.contains("──"),
            "Task header should contain separator lines"
        );
    }

    #[test]
    fn test_loop_uses_custom_config_command() {
        // Configure a command that will fail (nonexistent binary), verify it
        // attempts to use the configured command rather than hardcoded claude
        let tmp_dir = tempfile::tempdir().unwrap();
        let dir = tmp_dir.path();
        let change_dir = dir.join("openspec/changes/test-custom");
        std::fs::create_dir_all(&change_dir).unwrap();
        let tasks_path = change_dir.join("tasks.md");
        std::fs::write(&tasks_path, "- [ ] Task one\n").unwrap();
        let log_path = dir.join("test.log");

        let config = TuiConfig {
            command: "nonexistent-binary-xyz {prompt}".to_string(),
            prompt: "custom prompt for {name}".to_string(),
            ..Default::default()
        };

        let (tx, rx) = mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let child_handle: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));

        implementation_loop(
            "test-custom",
            &tasks_path,
            &log_path,
            &tx,
            &cancel_flag,
            &child_handle,
            &config,
        );

        // Should receive 3 Errors then Stalled after 3 consecutive spawn failures
        for _ in 0..3 {
            let msg = rx.recv().unwrap();
            assert!(matches!(msg, ImplUpdate::Error(_)));
        }
        let msg = rx.recv().unwrap();
        assert!(matches!(msg, ImplUpdate::Stalled));
    }

    #[test]
    fn test_loop_uses_custom_config_prompt() {
        // Verify the config prompt template is used for substitution
        let config = TuiConfig {
            command: "echo {prompt}".to_string(),
            prompt: "do something with {name} please".to_string(),
            ..Default::default()
        };
        let rendered = config.render_prompt("my-feature");
        assert_eq!(rendered, "do something with my-feature please");
    }

    #[test]
    fn test_loop_finishes_when_command_empty() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let dir = tmp_dir.path();
        let change_dir = dir.join("openspec/changes/test-empty");
        std::fs::create_dir_all(&change_dir).unwrap();
        let tasks_path = change_dir.join("tasks.md");
        std::fs::write(&tasks_path, "- [ ] Task one\n").unwrap();
        let log_path = dir.join("test.log");

        let config = TuiConfig {
            command: "".to_string(),
            prompt: "test {name}".to_string(),
            ..Default::default()
        };

        let (tx, rx) = mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let child_handle: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));

        implementation_loop(
            "test-empty",
            &tasks_path,
            &log_path,
            &tx,
            &cancel_flag,
            &child_handle,
            &config,
        );

        // Should receive Error then Finished { success: false }
        let msg = rx.recv().unwrap();
        assert!(matches!(msg, ImplUpdate::Error(_)));
        let msg = rx.recv().unwrap();
        assert!(matches!(msg, ImplUpdate::Finished { success: false }));
    }

    // --- Stall detection tests ---

    #[test]
    fn test_loop_sends_stalled_after_three_no_progress_runs() {
        // Use `true` command which exits 0 but doesn't modify tasks
        let tmp_dir = tempfile::tempdir().unwrap();
        let dir = tmp_dir.path();
        let change_dir = dir.join("openspec/changes/test-stall3");
        std::fs::create_dir_all(&change_dir).unwrap();
        let tasks_path = change_dir.join("tasks.md");
        std::fs::write(&tasks_path, "- [ ] Task one\n").unwrap();
        let log_path = dir.join("test.log");

        let config = TuiConfig {
            command: "true {prompt}".to_string(),
            prompt: "test {name}".to_string(),
            ..Default::default()
        };

        let (tx, rx) = mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let child_handle: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));

        implementation_loop(
            "test-stall3",
            &tasks_path,
            &log_path,
            &tx,
            &cancel_flag,
            &child_handle,
            &config,
        );

        // Collect all messages
        let mut messages = vec![];
        while let Ok(msg) = rx.try_recv() {
            messages.push(msg);
        }

        // Should have 2 Progress messages (runs 1 and 2) and then Stalled (run 3)
        assert_eq!(messages.len(), 3);
        assert!(matches!(
            messages[0],
            ImplUpdate::Progress {
                completed: 0,
                total: 1
            }
        ));
        assert!(matches!(
            messages[1],
            ImplUpdate::Progress {
                completed: 0,
                total: 1
            }
        ));
        assert!(matches!(messages[2], ImplUpdate::Stalled));
    }

    #[test]
    fn test_stall_counter_resets_on_progress() {
        // Script marks a task on the 2nd invocation (via counter file)
        let tmp_dir = tempfile::tempdir().unwrap();
        let dir = tmp_dir.path();
        let change_dir = dir.join("openspec/changes/test-stall-reset");
        std::fs::create_dir_all(&change_dir).unwrap();
        let tasks_path = change_dir.join("tasks.md");
        std::fs::write(&tasks_path, "- [ ] Task one\n- [ ] Task two\n").unwrap();
        let log_path = dir.join("test.log");

        let script_path = dir.join("mark_task.sh");
        std::fs::write(
            &script_path,
            "#!/bin/bash\n\
             TASKS_FILE=\"$1\"\n\
             COUNTER_FILE=\"${TASKS_FILE}.counter\"\n\
             count=0\n\
             if [ -f \"$COUNTER_FILE\" ]; then count=$(cat \"$COUNTER_FILE\"); fi\n\
             count=$((count + 1))\n\
             echo $count > \"$COUNTER_FILE\"\n\
             if [ \"$count\" -eq 2 ]; then\n\
                 sed -i '0,/- \\[ \\]/s//- [x]/' \"$TASKS_FILE\"\n\
             fi\n",
        )
        .unwrap();

        let config = TuiConfig {
            command: format!("bash {} {{prompt}}", script_path.display()),
            prompt: tasks_path.to_str().unwrap().to_string(),
            ..Default::default()
        };

        let (tx, rx) = mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let child_handle: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));

        implementation_loop(
            "test-stall-reset",
            &tasks_path,
            &log_path,
            &tx,
            &cancel_flag,
            &child_handle,
            &config,
        );

        let mut messages = vec![];
        while let Ok(msg) = rx.try_recv() {
            messages.push(msg);
        }

        // Expected sequence:
        // Run 1: no progress → Progress(0,2)
        // Run 2: marks task 1 → Progress(1,2) + stall reset
        // Run 3: no progress → Progress(1,2)
        // Run 4: no progress → Progress(1,2)
        // Run 5: no progress → Stalled (stall_count=3)
        // Total: 4 Progress + 1 Stalled = 5 messages
        assert_eq!(
            messages.len(),
            5,
            "Expected 5 messages, got: {}",
            messages.len()
        );

        // Verify progress was detected (proves reset happened)
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, ImplUpdate::Progress { completed: 1, .. }))
        );

        // Last message should be Stalled
        assert!(matches!(messages.last().unwrap(), ImplUpdate::Stalled));
    }

    #[test]
    fn test_loop_continues_past_failed_runs_with_eventual_progress() {
        // Script marks the task on the 3rd invocation — loop should finish, not stall
        let tmp_dir = tempfile::tempdir().unwrap();
        let dir = tmp_dir.path();
        let change_dir = dir.join("openspec/changes/test-stall-cont");
        std::fs::create_dir_all(&change_dir).unwrap();
        let tasks_path = change_dir.join("tasks.md");
        std::fs::write(&tasks_path, "- [ ] Task one\n").unwrap();
        let log_path = dir.join("test.log");

        let script_path = dir.join("mark_task_late.sh");
        std::fs::write(
            &script_path,
            "#!/bin/bash\n\
             TASKS_FILE=\"$1\"\n\
             COUNTER_FILE=\"${TASKS_FILE}.counter\"\n\
             count=0\n\
             if [ -f \"$COUNTER_FILE\" ]; then count=$(cat \"$COUNTER_FILE\"); fi\n\
             count=$((count + 1))\n\
             echo $count > \"$COUNTER_FILE\"\n\
             if [ \"$count\" -ge 3 ]; then\n\
                 sed -i '0,/- \\[ \\]/s//- [x]/' \"$TASKS_FILE\"\n\
             fi\n",
        )
        .unwrap();

        let config = TuiConfig {
            command: format!("bash {} {{prompt}}", script_path.display()),
            prompt: tasks_path.to_str().unwrap().to_string(),
            ..Default::default()
        };

        let (tx, rx) = mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let child_handle: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));

        implementation_loop(
            "test-stall-cont",
            &tasks_path,
            &log_path,
            &tx,
            &cancel_flag,
            &child_handle,
            &config,
        );

        let mut messages = vec![];
        while let Ok(msg) = rx.try_recv() {
            messages.push(msg);
        }

        // Expected sequence:
        // Run 1: no progress → Progress(0,1)
        // Run 2: no progress → Progress(0,1)
        // Run 3: marks task → Progress(1,1) → all done → Finished
        // Total: 3 Progress + 1 Finished = 4 messages
        assert_eq!(
            messages.len(),
            4,
            "Expected 4 messages, got: {}",
            messages.len()
        );

        // Should end with Finished { success: true } (not Stalled)
        assert!(matches!(
            messages.last().unwrap(),
            ImplUpdate::Finished { success: true }
        ));
        assert!(!messages.iter().any(|m| matches!(m, ImplUpdate::Stalled)));
    }

    // --- BatchImplState tests ---

    #[test]
    fn test_batch_impl_state_new() {
        let queue = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let deps = HashMap::new();
        let state = BatchImplState::new(queue.clone(), deps);

        assert_eq!(state.queue, queue);
        assert_eq!(state.current_index, 0);
        assert!(state.completed.is_empty());
        assert!(state.failed.is_empty());
        assert!(state.skipped.is_empty());
    }

    #[test]
    fn test_batch_current_change() {
        let queue = vec!["a".to_string(), "b".to_string()];
        let state = BatchImplState::new(queue, HashMap::new());

        assert_eq!(state.current_change(), Some("a"));
        assert!(!state.is_finished());
    }

    #[test]
    fn test_batch_current_change_empty_queue() {
        let state = BatchImplState::new(vec![], HashMap::new());

        assert_eq!(state.current_change(), None);
        assert!(state.is_finished());
    }

    #[test]
    fn test_batch_total() {
        let queue = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let state = BatchImplState::new(queue, HashMap::new());

        assert_eq!(state.total(), 3);
    }

    #[test]
    fn test_batch_advance_success() {
        let queue = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut state = BatchImplState::new(queue, HashMap::new());

        let next = state.advance(true);
        assert_eq!(next, Some("b".to_string()));
        assert!(state.completed.contains("a"));
        assert_eq!(state.current_index, 1);

        let next = state.advance(true);
        assert_eq!(next, Some("c".to_string()));
        assert!(state.completed.contains("b"));

        let next = state.advance(true);
        assert_eq!(next, None);
        assert!(state.completed.contains("c"));
        assert!(state.is_finished());
    }

    #[test]
    fn test_batch_advance_failure_skips_dependents() {
        // Queue: a -> b -> c (b depends on a, c depends on b)
        let queue = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut deps = HashMap::new();
        deps.insert("b".to_string(), vec!["a".to_string()]);
        deps.insert("c".to_string(), vec!["b".to_string()]);
        let mut state = BatchImplState::new(queue, deps);

        // "a" fails
        let next = state.advance(false);
        // "b" and "c" should be skipped, no next change
        assert_eq!(next, None);
        assert!(state.failed.contains("a"));
        assert!(state.skipped.contains("b"));
        assert!(state.skipped.contains("c"));
        assert!(state.is_finished());
    }

    #[test]
    fn test_batch_advance_failure_continues_independent() {
        // Queue: a, b (independent), c depends on a
        let queue = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let mut deps = HashMap::new();
        deps.insert("c".to_string(), vec!["a".to_string()]);
        let mut state = BatchImplState::new(queue, deps);

        // "a" fails
        let next = state.advance(false);
        // "b" is independent, should be the next
        assert_eq!(next, Some("b".to_string()));
        assert!(state.failed.contains("a"));

        // "b" succeeds
        let next = state.advance(true);
        // "c" depends on "a" which failed, so "c" is skipped
        assert_eq!(next, None);
        assert!(state.completed.contains("b"));
        assert!(state.skipped.contains("c"));
    }

    #[test]
    fn test_batch_should_skip_transitive() {
        // a -> b -> c: if a fails, both b and c should be skipped
        let mut deps = HashMap::new();
        deps.insert("b".to_string(), vec!["a".to_string()]);
        deps.insert("c".to_string(), vec!["b".to_string()]);
        let mut state = BatchImplState::new(vec![], deps);

        state.failed.insert("a".to_string());
        assert!(state.should_skip("b"));
        assert!(state.should_skip("c"));
    }

    #[test]
    fn test_batch_should_skip_no_failed_deps() {
        let mut deps = HashMap::new();
        deps.insert("b".to_string(), vec!["a".to_string()]);
        let state = BatchImplState::new(vec![], deps);

        assert!(!state.should_skip("b"));
    }

    #[test]
    fn test_batch_all_complete_finishes() {
        let queue = vec!["x".to_string()];
        let mut state = BatchImplState::new(queue, HashMap::new());

        let next = state.advance(true);
        assert_eq!(next, None);
        assert!(state.is_finished());
        assert!(state.completed.contains("x"));
    }

    #[test]
    fn test_batch_deps_stored() {
        let mut deps = HashMap::new();
        deps.insert("b".to_string(), vec!["a".to_string()]);
        let state = BatchImplState::new(vec!["a".to_string(), "b".to_string()], deps.clone());

        assert_eq!(state.deps, deps);
    }

    // --- start_apply tests ---

    #[test]
    fn test_start_apply_sets_log_path() {
        let config = TuiConfig {
            command: "echo {prompt}".to_string(),
            ..Default::default()
        };
        let state = start_apply("my-change", &config);

        assert_eq!(
            state.log_path,
            PathBuf::from("openspec/changes/my-change/implementation.log")
        );
        assert_eq!(state.change_name, "my-change");
        assert_eq!(state.completed, 0);
        assert_eq!(state.total, 0);
    }

    #[test]
    fn test_start_apply_cancel_flag_works() {
        let config = TuiConfig {
            // Use sleep to keep process alive long enough to cancel
            command: "sleep {prompt}".to_string(),
            ..Default::default()
        };
        let state = start_apply("test-cancel-apply", &config);

        // Cancel immediately
        stop_implementation(&state);

        // Should not receive any Progress or Stalled messages
        let mut got_progress = false;
        let mut got_stalled = false;
        // Give thread a moment to finish
        std::thread::sleep(std::time::Duration::from_millis(200));
        while let Ok(msg) = state.receiver.try_recv() {
            match msg {
                ImplUpdate::Progress { .. } => got_progress = true,
                ImplUpdate::Stalled => got_stalled = true,
                ImplUpdate::Finished { .. } | ImplUpdate::Error(_) => {}
            }
        }
        assert!(!got_progress, "Apply mode should not send Progress");
        assert!(!got_stalled, "Apply mode should not send Stalled");
    }

    #[test]
    fn test_apply_run_sends_only_finished() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let dir = tmp_dir.path();
        let log_path = dir.join("implementation.log");

        let config = TuiConfig {
            command: "true {prompt}".to_string(),
            ..Default::default()
        };

        let (tx, rx) = mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let child_handle: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));

        apply_run(
            "test-apply",
            &log_path,
            &tx,
            &cancel_flag,
            &child_handle,
            &config,
        );

        let mut messages = vec![];
        while let Ok(msg) = rx.try_recv() {
            messages.push(msg);
        }

        // Should have exactly one Finished message, no Progress or Stalled
        assert_eq!(
            messages.len(),
            1,
            "Expected 1 message, got: {}",
            messages.len()
        );
        assert!(matches!(
            messages[0],
            ImplUpdate::Finished { success: true }
        ));
    }

    #[test]
    fn test_apply_run_no_progress_or_stalled() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let dir = tmp_dir.path();
        let log_path = dir.join("implementation.log");

        // Use a command that fails
        let config = TuiConfig {
            command: "false {prompt}".to_string(),
            ..Default::default()
        };

        let (tx, rx) = mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let child_handle: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));

        apply_run(
            "test-apply-fail",
            &log_path,
            &tx,
            &cancel_flag,
            &child_handle,
            &config,
        );

        let mut messages = vec![];
        while let Ok(msg) = rx.try_recv() {
            messages.push(msg);
        }

        // Only Finished { success: false }, no Progress or Stalled
        assert_eq!(messages.len(), 1);
        assert!(matches!(
            messages[0],
            ImplUpdate::Finished { success: false }
        ));
        assert!(
            !messages
                .iter()
                .any(|m| matches!(m, ImplUpdate::Progress { .. }))
        );
        assert!(!messages.iter().any(|m| matches!(m, ImplUpdate::Stalled)));
    }

    #[test]
    fn test_apply_run_writes_run_header() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let dir = tmp_dir.path();
        let log_path = dir.join("implementation.log");

        let config = TuiConfig {
            command: "true {prompt}".to_string(),
            ..Default::default()
        };

        let (tx, _rx) = mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let child_handle: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));

        apply_run(
            "test-apply-hdr",
            &log_path,
            &tx,
            &cancel_flag,
            &child_handle,
            &config,
        );

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("IMPLEMENTATION RUN STARTED"));
        assert!(content.contains("Change: test-apply-hdr"));
    }

    #[test]
    fn test_apply_run_cancel_sends_nothing() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let dir = tmp_dir.path();
        let log_path = dir.join("implementation.log");

        let config = TuiConfig {
            command: "sleep {prompt}".to_string(),
            ..Default::default()
        };

        let (tx, rx) = mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(true)); // Pre-set cancel
        let child_handle: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));

        apply_run(
            "test-apply-cancel",
            &log_path,
            &tx,
            &cancel_flag,
            &child_handle,
            &config,
        );

        // Should not receive any messages when cancelled before spawn
        let mut messages = vec![];
        while let Ok(msg) = rx.try_recv() {
            messages.push(msg);
        }
        assert!(
            messages.is_empty(),
            "Cancelled apply should send no messages"
        );
    }

    #[test]
    fn test_apply_run_empty_command_sends_finished_false() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let dir = tmp_dir.path();
        let log_path = dir.join("implementation.log");

        let config = TuiConfig {
            command: "".to_string(),
            ..Default::default()
        };

        let (tx, rx) = mpsc::channel();
        let cancel_flag = Arc::new(AtomicBool::new(false));
        let child_handle: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));

        apply_run(
            "test-apply-empty",
            &log_path,
            &tx,
            &cancel_flag,
            &child_handle,
            &config,
        );

        let msg = rx.recv().unwrap();
        assert!(matches!(msg, ImplUpdate::Error(_)));
        let msg = rx.recv().unwrap();
        assert!(matches!(msg, ImplUpdate::Finished { success: false }));
    }
}
