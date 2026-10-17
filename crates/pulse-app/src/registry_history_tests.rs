#![allow(clippy::unwrap_used)]
use super::tests::{config, session};
use super::*;

#[tokio::test]
async fn history_page_requires_current_window_page_and_binding() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let other = registry.open_window().unwrap();
    let page = registry.activate_launch_page(owner).unwrap();
    let other_page = registry.activate_launch_page(other).unwrap();
    let (handle, task, _driver) = session();
    let id = registry.register(owner, handle).unwrap();
    assert!(registry.history_page(&page, id, None, None).await.is_err());
    let search = pulse_term::HistorySearchRequest {
        query: "needle".into(),
        match_case: false,
        regex: true,
        whole_word: true,
        backwards: true,
        alternate: false,
        cursor: None,
        revision: None,
    };
    assert!(
        registry
            .history_search(&page, id, search.clone())
            .await
            .is_err()
    );
    let _binding = registry.attach_page_v2(&page, id, config()).await.unwrap();
    let history = registry.history_page(&page, id, None, None).await.unwrap();
    assert_eq!(history.total_rows, 0);
    assert!(
        registry
            .history_search(&page, id, search.clone())
            .await
            .unwrap()
            .complete
    );
    assert!(
        registry
            .history_search(&other_page, id, search.clone())
            .await
            .is_err()
    );
    assert!(
        registry
            .history_page(&other_page, id, None, None)
            .await
            .is_err()
    );
    let _new_page = registry.activate_launch_page(owner).unwrap();
    assert!(registry.history_page(&page, id, None, None).await.is_err());
    assert!(registry.history_search(&page, id, search).await.is_err());
    registry.shutdown().unwrap();
    task.join().await.unwrap();
}

#[tokio::test]
async fn ended_history_keeps_original_page_and_window_authorization() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let other = registry.open_window().unwrap();
    let page = registry.activate_launch_page(owner).unwrap();
    let other_page = registry.activate_launch_page(other).unwrap();
    let (handle, task, driver) = session();
    let id = registry.register(owner, handle).unwrap();
    driver
        .output
        .send(
            b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix\r\nseven\r\neight\r\nEOF needle\x1b[?1049h\x1b[HALT".to_vec(),
        )
        .await
        .unwrap();
    drop(driver.output);
    task.join().await.unwrap();
    let _binding = registry.attach_page_v2(&page, id, config()).await.unwrap();
    let request = pulse_term::HistorySearchRequest {
        query: "EOF needle".into(),
        match_case: false,
        regex: true,
        whole_word: true,
        backwards: true,
        alternate: false,
        cursor: None,
        revision: None,
    };
    assert!(
        registry
            .history_search(&page, id, request.clone())
            .await
            .unwrap()
            .matched_row
            .is_some()
    );
    assert!(
        registry
            .history_page(&page, id, None, None)
            .await
            .unwrap()
            .total_rows
            > 0
    );
    assert!(
        registry
            .history_search(&other_page, id, request.clone())
            .await
            .is_err()
    );
    assert!(
        registry
            .history_page(&other_page, id, None, None)
            .await
            .is_err()
    );
    let mut alternate = request.clone();
    alternate.alternate = true;
    alternate.query = "ALT".into();
    let found = registry
        .history_search(&page, id, alternate.clone())
        .await
        .unwrap();
    assert!(found.alternate);
    assert!(found.matched_row.is_some());
    assert!(
        registry
            .history_search(&other_page, id, alternate.clone())
            .await
            .is_err()
    );
    let current = registry.activate_launch_page(owner).unwrap();
    assert!(
        registry
            .history_search(&page, id, alternate.clone())
            .await
            .is_err()
    );
    assert!(
        registry
            .history_search(&page, id, request.clone())
            .await
            .is_err()
    );
    assert!(registry.history_page(&page, id, None, None).await.is_err());
    let _rebound = registry
        .attach_page_v2(&current, id, config())
        .await
        .unwrap();
    assert!(
        registry
            .history_search(&current, id, request)
            .await
            .unwrap()
            .matched_row
            .is_some()
    );
    assert!(
        registry
            .history_search(&current, id, alternate)
            .await
            .unwrap()
            .matched_row
            .is_some()
    );
    registry.shutdown().unwrap();
}
