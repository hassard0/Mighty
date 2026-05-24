use sdust_runtime::timer::with_deadline;
use std::time::Duration;

#[tokio::test]
async fn deadline_fires() {
    let res = with_deadline(Some(Duration::from_millis(20)), async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        42_i32
    })
    .await;
    assert!(res.is_err());
}

#[tokio::test]
async fn deadline_none_passes_through() {
    let res = with_deadline(None, async { 7_i32 }).await.unwrap();
    assert_eq!(res, 7);
}

#[tokio::test]
async fn deadline_returns_value_when_fast() {
    let res = with_deadline(Some(Duration::from_secs(1)), async { 9_i32 })
        .await
        .unwrap();
    assert_eq!(res, 9);
}
