//! Count actual inference requests against a local mock endpoint, never a paid model.
#![cfg(unix)]
use anyhow::Result;
use codex_protocol::protocol::EventMsg;
use core_test_support::responses::{
    ev_assistant_message, ev_completed, ev_function_call, ev_response_created, mount_sse_sequence,
    sse,
};
use core_test_support::test_codex::{TestCodexHarness, test_codex};
use core_test_support::wait_for_event;
use serde_json::json;
use std::time::Duration;

async fn setup(
    exit_code: i32,
    expect_completion: bool,
) -> Result<(TestCodexHarness, core_test_support::responses::ResponseMock)> {
    let harness = TestCodexHarness::with_builder(test_codex().with_model("gpt-5.4")).await?;
    let arguments = json!({
        "cmd": format!("while [ ! -f release-background ]; do sleep 0.1; done; echo BACKGROUND_TEST_DONE; exit {exit_code}"),
        "login": false,
        "background": { "label": "test task" }
    });
    let mut responses = vec![
        sse(vec![
            ev_response_created("start"),
            ev_function_call("background-start", "exec_command", &arguments.to_string()),
            ev_completed("start"),
        ]),
        sse(vec![
            ev_assistant_message("waiting", "Task started; waiting without polling."),
            ev_completed("waiting"),
        ]),
        sse(vec![
            ev_assistant_message("resumed", "Task completion received."),
            ev_completed("resumed"),
        ]),
    ];
    if !expect_completion {
        responses.truncate(2);
    }
    let requests = mount_sse_sequence(harness.server(), responses).await;
    harness
        .submit("Start a background task and finish your turn while waiting.")
        .await?;
    Ok((harness, requests))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn background_completion_wakes_once_without_polling() -> Result<()> {
    for exit_code in [0, 7] {
        let (harness, requests) = setup(exit_code, true).await?;
        assert_eq!(requests.requests().len(), 2);
        let tasks = harness.test().codex.background_tasks().await;
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].status, "running");
        tokio::time::sleep(Duration::from_millis(500)).await;
        // Reading state or waiting does not invoke inference.
        assert_eq!(requests.requests().len(), 2);
        std::fs::write(harness.cwd().join("release-background"), "go")?;
        tokio::time::timeout(
            Duration::from_secs(20),
            wait_for_event(&harness.test().codex, |e| {
                matches!(e, EventMsg::TurnComplete(_))
            }),
        )
        .await?;
        assert_eq!(requests.requests().len(), 3);
        let tasks = harness.test().codex.background_tasks().await;
        assert_eq!(tasks[0].exit_code, Some(exit_code));
        assert_eq!(
            tasks[0].status,
            if exit_code == 0 {
                "succeeded"
            } else {
                "failed"
            }
        );
        assert!(tasks[0].output.contains("BACKGROUND_TEST_DONE"));
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(requests.requests().len(), 3);
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn background_cancellation_does_not_wake_model() -> Result<()> {
    let (harness, requests) = setup(0, false).await?;
    let tasks = harness.test().codex.background_tasks().await;
    assert!(
        harness
            .test()
            .codex
            .terminate_background_terminal(tasks[0].process_id.parse()?)
            .await
    );
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(requests.requests().len(), 2);
    assert_eq!(
        harness.test().codex.background_tasks().await[0].status,
        "cancelled"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn background_monitor_alerts_once_then_completes() -> Result<()> {
    let harness = TestCodexHarness::with_builder(test_codex().with_model("gpt-5.4")).await?;
    let arguments = json!({
        "cmd": "while [ ! -f alert-background ]; do sleep 0.1; done; printf 'READY\\nREADY\\n'; while [ ! -f release-background ]; do sleep 0.1; done; echo FINISHED",
        "login": false,
        "background": { "label": "monitor test", "monitor": true, "notify_pattern": "^READY$" }
    });
    let requests = mount_sse_sequence(
        harness.server(),
        vec![
            sse(vec![
                ev_function_call("monitor-start", "exec_command", &arguments.to_string()),
                ev_completed("start"),
            ]),
            sse(vec![
                ev_assistant_message("waiting", "Waiting."),
                ev_completed("waiting"),
            ]),
            sse(vec![
                ev_assistant_message("alert", "Ready notification received."),
                ev_completed("alert"),
            ]),
            sse(vec![
                ev_assistant_message("done", "Completion received."),
                ev_completed("done"),
            ]),
        ],
    )
    .await;
    harness.submit("Run a background monitor.").await?;
    assert_eq!(requests.requests().len(), 2);
    std::fs::write(harness.cwd().join("alert-background"), "go")?;
    tokio::time::timeout(
        Duration::from_secs(20),
        wait_for_event(&harness.test().codex, |e| {
            matches!(e, EventMsg::TurnComplete(_))
        }),
    )
    .await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        requests.requests().len(),
        3,
        "Duplicate lines must not cause another turn"
    );
    assert_eq!(
        harness.test().codex.background_tasks().await[0].status,
        "running"
    );
    std::fs::write(harness.cwd().join("release-background"), "go")?;
    tokio::time::timeout(
        Duration::from_secs(20),
        wait_for_event(&harness.test().codex, |e| {
            matches!(e, EventMsg::TurnComplete(_))
        }),
    )
    .await?;
    assert_eq!(requests.requests().len(), 4);
    assert_eq!(
        harness.test().codex.background_tasks().await[0].status,
        "succeeded"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "Five-minute acceptance test; run explicitly after the focused suite"]
async fn background_five_minute_idle_has_zero_inference_requests() -> Result<()> {
    let (harness, requests) = setup(0, true).await?;
    tokio::time::sleep(Duration::from_secs(300)).await;
    assert_eq!(
        requests.requests().len(),
        2,
        "No inference requests during the five-minute wait"
    );
    std::fs::write(harness.cwd().join("release-background"), "go")?;
    tokio::time::timeout(
        Duration::from_secs(20),
        wait_for_event(&harness.test().codex, |e| {
            matches!(e, EventMsg::TurnComplete(_))
        }),
    )
    .await?;
    assert_eq!(
        requests.requests().len(),
        3,
        "Exactly one completion continuation"
    );
    Ok(())
}
