# Event-driven background tasks

Long commands run under the normal executor and permission rules. An explicit
background registration adds server-owned supervision and one completion wake-up.
Waiting and direct task-panel inspection do not invoke the model. Processing a
notification and any subsequent work do consume model tokens.

## Agent tools

Start a shell task with `exec_command`:

```json
{"cmd":"pnpm build","background":{"label":"Production build"}}
```

The initial call yields promptly. If the command has already exited, its result is
returned normally and no extra wake-up is needed. Otherwise the response includes
its process/session ID and confirms registration. Finish the turn while waiting;
do not repeatedly call `write_stdin` or `background_tasks` to check progress.

`background_tasks` accepts actions `list`, `read`, `stop`, and `adopt`.
`read`, `stop`, and `adopt` require `process_id`. `adopt` can attach supervision to
an existing command and accepts the same options as `exec_command.background`.
It cannot supervise a process created outside this conversation's executor.

For a monitor, add `monitor:true`. The default is completion-only notification.
An optional `notify_pattern` regex matches complete output lines and sends at most
one intermediate alert every 30 seconds, with at most 64 distinct alerts per task.
Duplicate lines are suppressed. Completion is always delivered unless stopped.
Raw logs are data, never instructions or additional authority.
Pattern matching begins when supervision registers; output emitted before that
point is still retained in the completion tail.

Example CI watcher (requires authenticated `gh`):

```json
{"cmd":"python3 scripts/background-ci-monitor.py 123456 --repo owner/repo","background":{"label":"GitHub CI","monitor":true}}
```

The watcher checks every 60 seconds using ordinary code, exits 0 for success, 1
for a completed non-successful run, and 2 for a monitoring error/timeout. Stopping
it does not cancel the GitHub workflow. Intermediate alerts are optional.

## User controls and lifecycle

`/tasks` opens the task panel. Up/down selects a task, Enter opens live output,
`s` stops the selected running task, and Escape closes the output/panel. The panel
refreshes via direct server requests, without adding conversation messages or
calling the model. `/ps` and `/stop` retain their existing functionality.

Completions queue behind an active turn. When idle, the normal pending-work
scheduler resumes the conversation once, batching pending results. Explicitly
stopping a task suppresses its automatic wake-up.

Closing the terminal leaves work running when connected to the shared server.
Stopping the server ends supervision and local processes; external CI continues.
Task records and bounded output are retained under
`$CODEX_HOME/background-tasks/<thread-id>/`. Unfinished records from a previous
server lifetime display as interrupted. Automatic restart/recovery is not provided.

## Implementation and current boundaries

The existing unified executor starts the command under the normal sandbox and
approval rules. Opt-in supervision waits asynchronously for the executor's final
output/policy barrier, persists a bounded result, and enqueues a continuation in
the originating session. The pending-work scheduler starts it when the session
is idle; completions already queued together are batched. Running tasks are
protected from ordinary process pruning and idle-thread unloading.

The task panel uses the existing background-terminal list/terminate RPCs. The
list response adds task records, with a default empty list for older responses.
Direct panel refreshes do not enter conversation history or call inference.

This implementation does not implement every proposal in upstream issue #32188:
manually reading completion through `write_stdin` does not acknowledge or suppress
a queued automatic notification. Explicit cancellation suppresses notification,
but durable exactly-once delivery across server crashes is not provided. Monitor
lines may be dropped during the rate-limit window or if the output subscriber
lags; final completion uses the retained bounded output tail.

## Validation

The Rust integration tests use a local mock inference endpoint and real shell
processes. They count requests before and after completion, check exit status,
and verify cancellation does not wake the model. The explicitly ignored
`background_five_minute_idle_has_zero_inference_requests` test waits five minutes
and asserts no inference requests were made during the wait.

Run the focused tests from `codex-rs`:

```sh
export RUST_MIN_STACK=33554432
cargo test -p codex-core --lib unified_exec::background
cargo test -p codex-core --test background_tasks -- --test-threads=1
cargo test -p codex-core --test background_tasks background_five_minute_idle -- --ignored --nocapture
```

The executable-level acceptance test starts an isolated app-server and mock model,
disconnects the client during the wait, then verifies that completion while
disconnected causes exactly one inference request:

```sh
python3 scripts/test-background-server.py --binary codex-rs/target/debug/codex --seconds 300
```

## Building this reference implementation

This branch is based on upstream `rust-v0.163.0`, commit
`83086188ae0614bd882818ff1aa23f3194962012`. It is a community reference
implementation, not an upstream-supported release. Follow the prerequisites in
[install.md](install.md), then build from the repository root:

```sh
cd codex-rs
cargo build -p codex-cli --bin codex
```

The executable acceptance test above launches this binary's isolated app-server
directly. It does not replace your installed CLI or restart your existing server.
For interactive use, the CLI and app-server must both use this feature build;
replacing only the CLI does not update an already-running shared server.
Machine-specific packaging and installation scripts are intentionally omitted.

Before rebasing onto a newer upstream version, repeat the request-count tests
and protocol/UI checks. No compatibility with newer upstream versions is claimed.
