# Local validation

The executable-level five-minute test ran against custom Codex `0.163.0-bg.1`
with an isolated `CODEX_HOME`, a real app-server, real shell processes, and a local
mock inference endpoint. It did not use an OpenAI account or paid inference.

| Event | Inference requests |
| --- | ---: |
| Initial command launch and assistant finishing its turn | 2 |
| 300 seconds waiting with the client disconnected | 0 |
| Completion while disconnected | 1 |
| Duplicate completion requests | 0 |

The client reconnected during the wait and observed the running task. It then
disconnected again, released the command, and reconnected after the automatic
continuation. The task record reported `succeeded`, exit code `0`, and the
completion request contained the command's final output.

This measures request counts, not token savings for a particular real model.
A real completion continuation consumes tokens; the runtime wait does not invoke
the model. A live GitHub Actions deployment was not required for this test.

Reproduction instructions are in [background-tasks.md](background-tasks.md).

The focused Rust integration binary also passed all three short tests:
success/nonzero exit wake once; cancellation does not wake; duplicate monitor
lines produce one intermediate alert and a separate completion notification.
Four deterministic CI-watcher tests passed for success, terminal failure,
transient network errors, and persistent monitoring errors.

Additional checks passed: 82 UI tests selected by `background`, 26 execution UI
tests, 16 slash-command menu tests, and 6 core unit tests. The core tests cover
queued completions waiting for an idle conversation, batching, cross-thread
isolation, bounded UTF-8 output, and monitor validation/deduplication. The
protocol serialization and generated TypeScript fixture checks also passed.

The final executable was rebuilt successfully and assembled with its runtime
resources. A two-second disconnected-client smoke test also passed against that
complete package. The user subsequently confirmed the installed feature worked.

## Publication validation

The contribution branch removes the custom version suffix and machine-specific
installation scripts. Its Rust implementation and tests are identical to the
validated local build. The original validation used Rust 1.98.1 on Linux; the
upstream toolchain declaration is retained unchanged. Cross-platform and declared
Rust 1.95.0 toolchain compatibility have not been independently verified.

The publication rerun passed the focused core tests (6), short integration tests
(3), background UI selection (82), execution UI selection (26), slash-command
selection (16), and CI-watcher tests (4). These selections can overlap; the counts
are not a claim of that many unique feature tests. Rust formatting and whitespace
checks passed. The Rust tests reuse the validated compiled binaries after checking
that all 54 implementation, schema, and test files match the publication branch
byte for byte; documentation and version/install packaging differ.

The five-minute executable acceptance test was repeated from the publication
checkout using the validated complete package. It passed with 2 initial requests,
0 requests during 300 seconds of waiting, and 1 completion request while the
client was disconnected. The mock endpoint consumed no paid model tokens.

After rebuilding the protocol test binary to embed the current cached exports,
all four schema fixture checks passed: TypeScript, JSON, stable precomputed
exports, and experimental precomputed exports. The background-terminal response
serialization round-trip also passed. An initial cached-binary check used stale
embedded TypeScript timestamps; rebuilding resolved it without source changes.
