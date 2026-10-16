// Shared job tracking, reused by every cleaner: a job is any command run on
// a background thread with its stdout/stderr streamed line by line, so the
// frontend can poll live output and the final status. Privileged work is a
// job whose command is `pkexec <self> --run-ops <file>`; see privileged.rs.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::Serialize;

static NEXT_JOB_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobStatus {
    pub running: bool,
    pub success: bool,
    pub output: String,
}

static JOBS: Mutex<Option<HashMap<u64, JobStatus>>> = Mutex::new(None);

fn jobs() -> std::sync::MutexGuard<'static, Option<HashMap<u64, JobStatus>>> {
    let mut guard = JOBS.lock().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    guard
}

fn append_output_line(id: u64, line: &str) {
    if let Some(job) = jobs().as_mut().unwrap().get_mut(&id) {
        job.output.push_str(line);
        job.output.push('\n');
    }
}

/// Starts a command on a plain OS thread and immediately returns a job id;
/// the command handler itself never blocks. stdout/stderr are streamed line
/// by line into the job's output as they arrive, so the frontend's
/// poll_job() sees live progress instead of only revealing output once the
/// whole process finishes.
pub(crate) fn start_job(program: &str, args: Vec<String>) -> u64 {
    let id = NEXT_JOB_ID.fetch_add(1, Ordering::SeqCst);
    jobs().as_mut().unwrap().insert(id, JobStatus { running: true, success: false, output: String::new() });

    let program = program.to_string();
    std::thread::spawn(move || {
        let child = Command::new(&program).args(&args).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();

        let mut child = match child {
            Ok(child) => child,
            Err(e) => {
                jobs().as_mut().unwrap().insert(id, JobStatus { running: false, success: false, output: e.to_string() });
                return;
            }
        };

        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        let stdout_reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().flatten() {
                append_output_line(id, &line);
            }
        });
        let stderr_reader = std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().flatten() {
                append_output_line(id, &line);
            }
        });

        let exit_status = child.wait();
        let _ = stdout_reader.join();
        let _ = stderr_reader.join();

        let success = exit_status.map(|status| status.success()).unwrap_or(false);
        if let Some(job) = jobs().as_mut().unwrap().get_mut(&id) {
            job.running = false;
            job.success = success;
        }
    });

    id
}

pub(crate) fn failed_job(message: String) -> u64 {
    let id = NEXT_JOB_ID.fetch_add(1, Ordering::SeqCst);
    jobs().as_mut().unwrap().insert(id, JobStatus { running: false, success: false, output: message });
    id
}

#[tauri::command]
pub fn poll_job(job_id: u64) -> JobStatus {
    let mut guard = jobs();
    let map = guard.as_mut().unwrap();
    match map.get(&job_id) {
        // The frontend stops polling as soon as it sees `running: false`, so
        // this is the one and only time it reads a finished job's status —
        // safe to drop it here instead of leaving it in the map forever.
        Some(status) if !status.running => map.remove(&job_id).unwrap(),
        Some(status) => status.clone(),
        None => JobStatus { running: false, success: false, output: "Unknown job".to_string() },
    }
}
