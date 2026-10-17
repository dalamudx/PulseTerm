use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
#[path = "ssh_interactive_peer_tests.rs"]
mod peer;
fn channel() -> (
    Arc<Broker>,
    tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let output = move |packet| tx.send(packet).map_err(|_| VaultOwnerError::Denied);
    (Broker::new(output), rx)
}
fn questions() -> Vec<russh::client::Prompt> {
    vec![russh::client::Prompt {
        prompt: "Test-only verification code".into(),
        echo: false,
    }]
}
#[tokio::test]
async fn exact_round_once_and_count_mismatch_does_not_consume_pending() -> TestResult {
    let (broker, mut packets) = channel();
    let task_broker = broker.clone();
    let task = tokio::spawn(async move {
        task_broker
            .ask(
                "Test server".into(),
                "Test instruction".into(),
                questions(),
                Arc::new(|| true),
            )
            .await
    });
    let packet = tokio::time::timeout(std::time::Duration::from_secs(2), packets.recv())
        .await?
        .ok_or("missing.prompt")?;
    assert_eq!(packet["prompts"][0]["echo"], false);
    let round = Uuid::parse_str(packet["round"].as_str().ok_or("missing.round")?)?;
    assert!(
        broker
            .answer(Uuid::new_v4(), Zeroizing::new(vec!["fixture".into()]))
            .is_err()
    );
    assert!(broker.answer(round, Zeroizing::new(vec![])).is_err());
    broker.answer(round, Zeroizing::new(vec!["fixture".into()]))?;
    assert!(
        broker
            .answer(round, Zeroizing::new(vec!["fixture".into()]))
            .is_err()
    );
    let answers = task.await??;
    assert!(answers.len() == 1 && answers[0] == "fixture");
    Ok(())
}
#[tokio::test]
async fn cancellation_removes_pending_and_revoked_guard_refuses_answers() -> TestResult {
    let (broker, mut packets) = channel();
    let live = Arc::new(AtomicBool::new(true));
    let captured = live.clone();
    let b = broker.clone();
    let task = tokio::spawn(async move {
        b.ask(
            "".into(),
            "".into(),
            questions(),
            Arc::new(move || captured.load(Ordering::Acquire)),
        )
        .await
    });
    let packet = tokio::time::timeout(std::time::Duration::from_secs(2), packets.recv())
        .await?
        .ok_or("missing.prompt")?;
    let round = Uuid::parse_str(packet["round"].as_str().ok_or("missing.round")?)?;
    live.store(false, Ordering::Release);
    assert!(
        broker
            .answer(round, Zeroizing::new(vec!["fixture".into()]))
            .is_err()
    );
    task.abort();
    assert!(task.await.is_err());
    assert!(broker.pending.lock().map_err(|_| "poison")?.is_none());
    assert!(
        broker
            .answer(round, Zeroizing::new(vec!["fixture".into()]))
            .is_err()
    );
    Ok(())
}
#[tokio::test]
async fn text_question_and_answer_budgets_fail_without_emitting_or_consuming() -> TestResult {
    let (broker, mut packets) = channel();
    assert!(
        broker
            .ask("X".repeat(1025), "".into(), questions(), Arc::new(|| true))
            .await
            .is_err()
    );
    assert!(
        broker
            .ask(
                "".into(),
                "".into(),
                (0..9).map(|_| questions().remove(0)).collect(),
                Arc::new(|| true)
            )
            .await
            .is_err()
    );
    assert!(packets.try_recv().is_err());
    let b = broker.clone();
    let task = tokio::spawn(async move {
        b.ask("".into(), "".into(), questions(), Arc::new(|| true))
            .await
    });
    let packet = tokio::time::timeout(std::time::Duration::from_secs(2), packets.recv())
        .await?
        .ok_or("missing.prompt")?;
    let round = Uuid::parse_str(packet["round"].as_str().ok_or("missing.round")?)?;
    assert!(
        broker
            .answer(round, Zeroizing::new(vec!["X".repeat(4097)]))
            .is_err()
    );
    assert!(
        broker
            .answer(round, Zeroizing::new(vec!["\0".into()]))
            .is_err()
    );
    broker.answer(round, Zeroizing::new(vec!["fixture".into()]))?;
    assert!(task.await?.is_ok());
    Ok(())
}
