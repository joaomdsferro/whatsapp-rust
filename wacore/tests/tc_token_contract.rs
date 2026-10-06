#![cfg(not(target_arch = "wasm32"))]
#[path = "support/tc_token_contract.rs"]
mod contract;
use wacore::store::{InMemoryBackend, traits::ProtocolStore};

#[tokio::test]
async fn memory_preserves_both_token_fields_and_store_isolation() {
    let a = InMemoryBackend::new();
    let b = InMemoryBackend::new();
    b.store_received_tc_token("100001@lid", b"other-device", 10)
        .await
        .unwrap();
    b.touch_tc_token_sender_timestamp("100001@lid", 20)
        .await
        .unwrap();
    contract::assert_tc_token_contract(&a).await;
    let other = b.get_tc_token("100001@lid").await.unwrap().unwrap();
    assert_eq!(other.token, b"other-device");
    assert_eq!(other.token_timestamp, 10);
    assert_eq!(other.sender_timestamp, Some(20));
    assert!(b.get_tc_token("race-0@lid").await.unwrap().is_none());
}
