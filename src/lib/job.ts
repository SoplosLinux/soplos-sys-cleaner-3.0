import { api } from "./api";

interface JobStatus {
  running: boolean;
  success: boolean;
  output: string;
}

// Starts a job (any command returning a job id from commands/jobs.rs) and
// polls it until it finishes, returning the final status with its output.
export async function runJobDetailed(command: string, args?: Record<string, unknown>): Promise<JobStatus> {
  const jobId = await api.invoke<number>(command, args);
  while (true) {
    const status = await api.invoke<JobStatus>("poll_job", { jobId });
    if (!status.running) return status;
    await new Promise((resolve) => setTimeout(resolve, 300));
  }
}

export async function runJob(command: string, args?: Record<string, unknown>): Promise<boolean> {
  return (await runJobDetailed(command, args)).success;
}
