//! Synthetic characterization of failure paths, not a stronger delivery contract.
use super::*;
use diesel::{Connection, RunQueryDsl, SqliteConnection};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use wacore::types::events::{ChannelEventHandler, InboundMessage};

#[derive(Default)]
struct Hook {
    fail: AtomicBool,
    pause_first: AtomicBool,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
    attempts: AtomicUsize,
    committed: AtomicUsize,
}

#[async_trait::async_trait]
impl crate::types::durability_hook::InboundDurabilityHook for Hook {
    async fn on_messages(&self, _: Arc<Client>, items: &[InboundMessage]) -> anyhow::Result<()> {
        self.attempts.fetch_add(items.len(), Ordering::SeqCst);
        if self.pause_first.swap(false, Ordering::SeqCst) {
            self.entered.notify_one();
            self.release.notified().await;
        }
        anyhow::ensure!(
            !self.fail.load(Ordering::SeqCst),
            "synthetic consumer failure"
        );
        self.committed.fetch_add(items.len(), Ordering::SeqCst);
        Ok(())
    }
}

struct Fixture {
    client: Arc<Client>,
    transport: Arc<crate::transport::mock::CapturingMockTransport>,
    sql: SqliteConnection,
    hook: Arc<Hook>,
    events: async_channel::Receiver<Arc<Event>>,
    stanza: Arc<OwnedNodeRef>,
    info: MessageInfo,
}

impl Fixture {
    async fn new(name: &str, drain: bool) -> Self {
        use crate::socket::NoiseSocket;
        use crate::transport::mock::CapturingMockTransportFactory;
        use wacore::handshake::NoiseCipher;
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let db = format!(
            "file:a09_{}_{}_{}?mode=memory&cache=shared",
            name,
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        );
        let backend = Arc::new(SqliteStore::open(&db).await.unwrap());
        let sql = SqliteConnection::establish(&db).unwrap();
        let pm = Arc::new(PersistenceManager::new(backend).await.unwrap());
        let factory = CapturingMockTransportFactory::new();
        let transport = factory.transport();
        let (client, _) = Client::builder()
            .with_runtime_arc(Arc::new(crate::runtime_impl::TokioRuntime))
            .with_persistence_manager(pm)
            .with_transport_factory_arc(Arc::new(factory))
            .with_http_client_arc(Arc::new(MockHttpClient))
            .build()
            .await
            .unwrap()
            .into_parts();
        *client.noise_socket.lock().unwrap() = Some(Arc::new(NoiseSocket::new(
            client.runtime.clone(),
            transport.clone(),
            NoiseCipher::new(&[0; 32]).unwrap(),
            NoiseCipher::new(&[0; 32]).unwrap(),
        )));
        client.set_connected_for_test(true);
        seed_test_pn(&client).await;
        if !drain {
            client.enter_live_mode_for_tests();
        }
        let hook = Arc::new(Hook::default());
        client
            .inbound_durability_hook
            .set(hook.clone())
            .ok()
            .unwrap();
        let (handler, events) = ChannelEventHandler::new();
        client.core.event_bus.subscribe_handler(handler).detach();
        let (bundle, own) = bobs_prekey_bundle(&client).await;
        let mut peer = AlicePeer::new("12025550123:7@s.whatsapp.net").await;
        peer.install_bob_session(&own.to_protocol_address(), &bundle)
            .await;
        let mut message = wa::Message::default();
        message.conversation = Some("synthetic retained body".into());
        let ciphertext = peer
            .encrypt(
                &own.to_protocol_address(),
                &MessageUtils::encode_and_pad(&message),
            )
            .await;
        let enc = enc_payload_from_ciphertext(&ciphertext);
        let stanza = node_to_arc(
            NodeBuilder::new("message")
                .attr("from", &peer.jid)
                .attr("id", name)
                .attr("type", "text")
                .attr("t", wacore::time::now_secs().to_string())
                .children([NodeBuilder::new("enc")
                    .attr("type", enc.enc_type.as_wire_str())
                    .attr("v", "2")
                    .bytes(enc.ciphertext.to_vec())
                    .build()])
                .build(),
        );
        let info = client.parse_message_info(stanza.get()).await.unwrap();
        Self {
            client,
            transport,
            sql,
            hook,
            events,
            stanza,
            info,
        }
    }

    fn buffer_failure(&mut self, enabled: bool) {
        let query = if enabled {
            "CREATE TRIGGER fail_pending BEFORE INSERT ON pending_inbound_messages BEGIN SELECT RAISE(FAIL, 'synthetic pending write failure'); END"
        } else {
            "DROP TRIGGER fail_pending"
        };
        diesel::sql_query(query).execute(&mut self.sql).unwrap();
    }

    async fn receive(&self) {
        self.client
            .clone()
            .handle_incoming_message(self.stanza.clone())
            .await;
        crate::test_utils::wait_for_outbound_tasks(&self.client).await;
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while self.client.signal_flush_worker_alive() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("coalesced Signal flush must settle");
    }

    async fn pending(&self) -> Option<Vec<u8>> {
        self.client
            .persistence_manager
            .backend()
            .get_pending_inbound(
                &self.info.source.chat.to_string(),
                &self.info.source.sender.to_string(),
                &self.info.id,
            )
            .await
            .unwrap()
    }

    fn receipts(&self) -> usize {
        let frames = self.transport.sent();
        assert_eq!(
            message_acks_for(&frames, &self.info.id),
            0,
            "ordinary DM/group delivery uses a receipt, not a transport ack"
        );
        delivery_receipts_for(&frames, &self.info.id)
    }
    fn published(&self) -> Vec<String> {
        message_texts_for_id(&self.events, &self.info.id)
    }
    async fn persisted_session(&self) -> bool {
        self.client
            .persistence_manager
            .backend()
            .get_session(self.info.source.sender.to_protocol_address().as_str())
            .await
            .unwrap()
            .is_some()
    }
}

#[tokio::test]
async fn a09_live_buffer_failure_then_exact_ciphertext_redelivery_recovers_body() {
    let mut f = Fixture::new("A09_LIVE_BUFFER", false).await;
    assert!(!f.persisted_session().await);
    f.buffer_failure(true);
    f.receive().await;
    assert_eq!(f.receipts(), 0);
    assert_eq!(f.hook.attempts.load(Ordering::SeqCst), 0);
    assert!(f.pending().await.is_none());
    assert!(f.published().is_empty());
    assert_eq!(f.client.inbound_commit_batch.pending_stats().0, 0);
    assert!(
        f.persisted_session().await,
        "live receive persists the advanced session despite missing buffer"
    );
    f.buffer_failure(false);
    f.receive().await;
    assert_eq!(
        f.receipts(),
        1,
        "the original plaintext is committed before its receipt"
    );
    assert_eq!(f.hook.committed.load(Ordering::SeqCst), 1);
    assert_eq!(f.published(), ["synthetic retained body"]);
}

#[tokio::test]
async fn a09_drain_buffer_failure_retains_body_then_recovers() {
    let mut f = Fixture::new("A09_DRAIN_BUFFER", true).await;
    f.buffer_failure(true);
    f.receive().await;
    assert!(
        !f.client
            .flush_inbound_commits_under_permit(true, None, None)
            .await
    );
    assert_eq!(f.receipts(), 0);
    assert_eq!(f.hook.attempts.load(Ordering::SeqCst), 0);
    assert!(f.pending().await.is_none());
    assert!(f.published().is_empty());
    assert_eq!(f.client.inbound_commit_batch.pending_stats().0, 1);
    assert!(
        !f.persisted_session().await,
        "drain must retain the ratchet in cache until the row exists"
    );
    f.buffer_failure(false);
    assert!(
        f.client
            .flush_inbound_commits_under_permit(true, None, None)
            .await
    );
    crate::test_utils::wait_for_outbound_tasks(&f.client).await;
    assert_eq!(f.receipts(), 1);
    assert_eq!(f.hook.committed.load(Ordering::SeqCst), 1);
    assert_eq!(f.published(), ["synthetic retained body"]);
    assert!(f.pending().await.is_none());
    assert!(f.persisted_session().await);
    assert!(!f.client.inbound_commit_batch.is_active());
    f.receive().await;
    assert_eq!(f.receipts(), 2);
    assert_eq!(f.hook.committed.load(Ordering::SeqCst), 1);
    assert!(f.published().is_empty());
}

async fn hook_failure_recovers(drain: bool, corrupt: bool) {
    let mut f = Fixture::new(
        if corrupt {
            "A09_CORRUPT"
        } else if drain {
            "A09_DRAIN_HOOK"
        } else {
            "A09_LIVE_HOOK"
        },
        drain,
    )
    .await;
    f.hook.fail.store(true, Ordering::SeqCst);
    f.receive().await;
    if drain {
        assert!(
            f.client
                .flush_inbound_commits_under_permit(true, None, None)
                .await
        );
    }
    crate::test_utils::wait_for_outbound_tasks(&f.client).await;
    assert_eq!(f.receipts(), 0);
    assert_eq!(f.hook.attempts.load(Ordering::SeqCst), 1);
    let original = f
        .pending()
        .await
        .expect("failed hook retains original bytes");
    assert!(f.persisted_session().await);
    assert!(f.published().is_empty());
    if corrupt {
        diesel::sql_query("UPDATE pending_inbound_messages SET message = X'FF'")
            .execute(&mut f.sql)
            .unwrap();
    }
    f.hook.fail.store(false, Ordering::SeqCst);
    if drain {
        assert!(!f.client.inbound_commit_batch.reset());
        f.client.swap_message_semaphore(1);
        f.client
            .offline_sync_completed
            .store(false, Ordering::Release);
    }
    f.receive().await;
    if drain {
        assert_eq!(f.receipts(), 0, "replay during drain waits for its batch");
        assert!(
            f.client
                .flush_inbound_commits_under_permit(true, None, None)
                .await
        );
        crate::test_utils::wait_for_outbound_tasks(&f.client).await;
    }
    if corrupt {
        assert_eq!(
            f.receipts(),
            0,
            "corruption cannot acknowledge an uncommitted message"
        );
        assert_eq!(
            f.pending().await.as_deref(),
            Some(&[0xff][..]),
            "retain exact bytes for diagnosis or repair"
        );
        assert_eq!(f.hook.attempts.load(Ordering::SeqCst), 1);
        assert_eq!(f.hook.committed.load(Ordering::SeqCst), 0);
        assert!(f.published().is_empty());
        f.client
            .persistence_manager
            .backend()
            .store_pending_inbound(
                &f.info.source.chat.to_string(),
                &f.info.source.sender.to_string(),
                &f.info.id,
                &original,
            )
            .await
            .unwrap();
        f.receive().await;
    }
    assert_eq!(f.receipts(), 1);
    assert!(f.pending().await.is_none());
    assert_eq!(f.hook.committed.load(Ordering::SeqCst), 1);
    assert_eq!(f.published(), ["synthetic retained body"]);
}

#[tokio::test]
async fn a09_live_hook_failure_replays_buffer() {
    hook_failure_recovers(false, false).await;
}
#[tokio::test]
async fn a09_drain_hook_failure_replays_buffer() {
    hook_failure_recovers(true, false).await;
}
#[tokio::test]
async fn a09_corrupt_replay_row_is_retained_until_repaired() {
    hook_failure_recovers(false, true).await;
}

#[tokio::test]
async fn a09_old_pending_row_survives_startup_and_regular_sweep() {
    let mut f = Fixture::new("A09_RETENTION", false).await;
    f.hook.fail.store(true, Ordering::SeqCst);
    f.receive().await;
    assert_eq!(f.receipts(), 0);
    assert!(f.pending().await.is_some());
    diesel::sql_query("UPDATE pending_inbound_messages SET inserted_at = inserted_at - 691200")
        .execute(&mut f.sql)
        .unwrap();
    f.client.run_startup_retention_cleanup().await;
    assert!(
        f.pending().await.is_some(),
        "startup gives old rows a replay opportunity"
    );
    f.client.run_retention_cleanup(0).await;
    assert!(
        f.pending().await.is_some(),
        "elapsed time cannot establish consumer commit"
    );
    f.hook.fail.store(false, Ordering::SeqCst);
    f.receive().await;
    assert_eq!(f.receipts(), 1);
    assert_eq!(f.hook.committed.load(Ordering::SeqCst), 1);
    assert_eq!(f.published(), ["synthetic retained body"]);
}

#[tokio::test]
async fn a09_pending_read_error_withholds_ack_and_recovers_original_body() {
    let mut f = Fixture::new("A09_READ_ERROR", false).await;
    f.hook.fail.store(true, Ordering::SeqCst);
    f.receive().await;
    assert_eq!(f.receipts(), 0);
    assert!(f.pending().await.is_some());
    diesel::sql_query("ALTER TABLE pending_inbound_messages RENAME TO saved_pending")
        .execute(&mut f.sql)
        .unwrap();
    f.hook.fail.store(false, Ordering::SeqCst);
    f.receive().await;
    assert_eq!(f.receipts(), 0);
    assert_eq!(f.hook.attempts.load(Ordering::SeqCst), 1);
    assert!(f.published().is_empty());
    diesel::sql_query("ALTER TABLE saved_pending RENAME TO pending_inbound_messages")
        .execute(&mut f.sql)
        .unwrap();
    f.receive().await;
    assert_eq!(f.receipts(), 1);
    assert_eq!(f.hook.committed.load(Ordering::SeqCst), 1);
    assert_eq!(f.published(), ["synthetic retained body"]);
}

async fn group_buffer_failure(drain: bool) {
    let mut f = Fixture::new(
        if drain {
            "A09_GROUP_DRAIN"
        } else {
            "A09_GROUP_LIVE"
        },
        drain,
    )
    .await;
    let group: Jid = "120363000000000091@g.us".parse().unwrap();
    let mut peer = joined_group_sender(&f.client, "777000000000091:7@lid", &group).await;
    let mut message = wa::Message::default();
    message.conversation = Some("synthetic retained group body".into());
    let ciphertext = peer
        .encrypt_group_message(&group, &MessageUtils::encode_and_pad(&message))
        .await;
    f.stanza = group_skmsg_stanza(&group, &peer.jid, &f.info.id, ciphertext);
    f.info = f.client.parse_message_info(f.stanza.get()).await.unwrap();
    f.buffer_failure(true);
    f.receive().await;
    if drain {
        assert!(
            !f.client
                .flush_inbound_commits_under_permit(true, None, None)
                .await
        );
    }
    assert_eq!(f.receipts(), 0);
    assert_eq!(f.hook.attempts.load(Ordering::SeqCst), 0);
    assert!(f.pending().await.is_none());
    assert!(f.published().is_empty());
    f.buffer_failure(false);
    if drain {
        assert!(
            f.client
                .flush_inbound_commits_under_permit(true, None, None)
                .await
        );
        crate::test_utils::wait_for_outbound_tasks(&f.client).await;
        assert_eq!(f.hook.committed.load(Ordering::SeqCst), 1);
        assert_eq!(f.published(), ["synthetic retained group body"]);
    } else {
        f.receive().await;
        assert_eq!(f.hook.committed.load(Ordering::SeqCst), 1);
        assert_eq!(f.published(), ["synthetic retained group body"]);
    }
    assert_eq!(f.receipts(), 1);
}

#[tokio::test]
async fn a09_group_live_buffer_failure_recovers_body_on_redelivery() {
    group_buffer_failure(false).await;
}
#[tokio::test]
async fn a09_group_drain_buffer_failure_retains_body_for_recovery() {
    group_buffer_failure(true).await;
}

#[tokio::test]
async fn retained_live_plaintext_survives_real_connection_cleanup() {
    let mut f = Fixture::new("RESET_BUFFER", false).await;
    f.buffer_failure(true);
    f.receive().await;
    assert_eq!(f.client.inbound_commit_batch.retention.stats().0, 1);
    f.client.cleanup_connection_state().await;
    assert_eq!(f.client.inbound_commit_batch.retention.stats().0, 1);
    assert_eq!(f.hook.committed.load(Ordering::SeqCst), 0);
    f.buffer_failure(false);
    f.receive().await;
    assert!(
        f.client
            .flush_inbound_commits_under_permit(true, None, None)
            .await
    );
    assert_eq!(f.hook.committed.load(Ordering::SeqCst), 1);
    assert_eq!(f.published(), ["synthetic retained body"]);
    assert_eq!(f.client.inbound_commit_batch.retention.stats().0, 0);
}

#[tokio::test]
async fn cancelling_receive_waiter_keeps_the_owned_hook_commit() {
    let f = Fixture::new("CANCEL_COMMIT", false).await;
    f.hook.pause_first.store(true, Ordering::SeqCst);
    let task = tokio::spawn(f.client.clone().handle_incoming_message(f.stanza.clone()));
    f.hook.entered.notified().await;
    task.abort();
    let _ = task.await;
    assert!(f.pending().await.is_some());
    assert_eq!(f.receipts(), 0);
    assert_eq!(f.client.inbound_commit_batch.retention.stats().0, 1);
    f.hook.release.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while f.client.inbound_commit_batch.retention.stats().0 != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    crate::test_utils::wait_for_outbound_tasks(&f.client).await;
    assert_eq!(f.hook.committed.load(Ordering::SeqCst), 1);
    assert_eq!(f.receipts(), 1);
    assert_eq!(f.published(), ["synthetic retained body"]);
}

#[tokio::test]
async fn shutdown_is_bounded_while_a_hook_owns_retained_plaintext() {
    let f = Fixture::new("SHUTDOWN_COMMIT", false).await;
    f.hook.pause_first.store(true, Ordering::SeqCst);
    let task = tokio::spawn(f.client.clone().handle_incoming_message(f.stanza.clone()));
    f.hook.entered.notified().await;
    tokio::time::timeout(std::time::Duration::from_secs(3), f.client.shutdown())
        .await
        .unwrap();
    assert!(f.pending().await.is_some());
    assert_eq!(f.client.inbound_commit_batch.retention.stats().0, 1);
    assert_eq!(f.hook.committed.load(Ordering::SeqCst), 0);
    f.hook.release.notify_one();
    task.await.unwrap();
    assert_eq!(f.hook.committed.load(Ordering::SeqCst), 1);
    assert_eq!(f.published(), ["synthetic retained body"]);
}

#[tokio::test]
async fn exhausted_admission_returns_before_decrypt_or_hook() {
    let f = Fixture::new("CAPACITY", false).await;
    let leases: Vec<_> = (0..400)
        .map(|_| f.client.inbound_commit_batch.retention.admit(1).unwrap())
        .collect();
    // Control responses do not acquire inbound message reservations.
    let (tx, rx) = futures::channel::oneshot::channel();
    f.client.response_waiters_guard().insert(
        "CONTROL_WHILE_FULL".to_owned(),
        crate::client::ResponseWaiter::Iq(tx),
    );
    let ack = node_to_arc(
        NodeBuilder::new("ack")
            .attr("id", "CONTROL_WHILE_FULL")
            .attr("from", "s.whatsapp.net")
            .build(),
    );
    assert!(f.client.handle_ack_response_arc(&ack));
    tokio::time::timeout(std::time::Duration::from_millis(100), rx)
        .await
        .unwrap()
        .unwrap();
    let mut cancelled = false;
    tokio::time::timeout(
        std::time::Duration::from_millis(100),
        crate::handlers::message::MessageHandler::handle_inline(
            f.client.clone(),
            f.stanza.clone(),
            &mut cancelled,
        ),
    )
    .await
    .unwrap();
    assert!(cancelled);
    assert!(f.client.connection_shutdown_signal().is_fired());
    assert!(!f.persisted_session().await);
    assert!(f.pending().await.is_none());
    assert_eq!(f.hook.attempts.load(Ordering::SeqCst), 0);
    assert_eq!(f.receipts(), 0);
    drop(leases);
    assert_eq!(f.client.inbound_commit_batch.retention.stats(), (0, 0));
}

async fn encrypted_parts(
    client: &Arc<Client>,
    sender: &str,
    id: &str,
    bodies: &[&str],
) -> Arc<OwnedNodeRef> {
    let (bundle, own) = bobs_prekey_bundle(client).await;
    let mut peer = AlicePeer::new(sender).await;
    peer.install_bob_session(&own.to_protocol_address(), &bundle)
        .await;
    let mut children = Vec::new();
    for body in bodies {
        let mut message = wa::Message::default();
        message.conversation = Some((*body).to_owned());
        let encrypted = peer
            .encrypt(
                &own.to_protocol_address(),
                &MessageUtils::encode_and_pad(&message),
            )
            .await;
        let enc = enc_payload_from_ciphertext(&encrypted);
        children.push(
            NodeBuilder::new("enc")
                .attr("type", enc.enc_type.as_wire_str())
                .attr("v", "2")
                .bytes(enc.ciphertext.to_vec())
                .build(),
        );
    }
    node_to_arc(
        NodeBuilder::new("message")
            .attr("from", &peer.jid)
            .attr("id", id)
            .attr("type", "text")
            .attr("t", wacore::time::now_secs().to_string())
            .children(children)
            .build(),
    )
}

#[tokio::test]
async fn multipart_stanza_commits_all_parts_before_one_receipt() {
    let mut f = Fixture::new("MULTIPART_RECEIVE", false).await;
    f.stanza = encrypted_parts(
        &f.client,
        "12025550125:7@s.whatsapp.net",
        &f.info.id,
        &["first part", "second part"],
    )
    .await;
    f.info = f.client.parse_message_info(f.stanza.get()).await.unwrap();
    f.hook.fail.store(true, Ordering::SeqCst);
    f.receive().await;
    assert_eq!(f.hook.attempts.load(Ordering::SeqCst), 2);
    assert_eq!(f.receipts(), 0);
    assert!(f.pending().await.is_some());
    f.hook.fail.store(false, Ordering::SeqCst);
    f.receive().await;
    assert_eq!(f.hook.committed.load(Ordering::SeqCst), 2);
    assert_eq!(f.published(), ["first part", "second part"]);
    assert_eq!(f.receipts(), 1);
    assert!(f.pending().await.is_none());
}

#[tokio::test]
async fn stalled_hook_does_not_block_an_unrelated_live_chat() {
    let f = Fixture::new("SLOW_CHAT", false).await;
    f.hook.pause_first.store(true, Ordering::SeqCst);
    let task = tokio::spawn(f.client.clone().handle_incoming_message(f.stanza.clone()));
    f.hook.entered.notified().await;
    let other = encrypted_parts(
        &f.client,
        "12025550126:7@s.whatsapp.net",
        "OTHER_CHAT",
        &["independent"],
    )
    .await;
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        f.client.clone().handle_incoming_message(other),
    )
    .await
    .unwrap();
    assert_eq!(f.hook.committed.load(Ordering::SeqCst), 1);
    assert_eq!(f.receipts(), 0, "the stalled stanza is still unconfirmed");
    assert_eq!(
        message_texts_for_id(&f.events, "OTHER_CHAT"),
        ["independent"]
    );
    f.hook.release.notify_one();
    task.await.unwrap();
    assert_eq!(f.hook.committed.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn resident_buffer_failure_recovers_without_another_server_delivery() {
    let mut f = Fixture::new("LOCAL_RECOVERY", false).await;
    f.buffer_failure(true);
    f.receive().await;
    assert_eq!(f.receipts(), 0);
    assert!(f.pending().await.is_none());
    f.buffer_failure(false);
    let published = tokio::time::timeout(std::time::Duration::from_secs(6), async {
        loop {
            let published = f.published();
            if !published.is_empty() {
                break published;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    crate::test_utils::wait_for_outbound_tasks(&f.client).await;
    assert_eq!(published, ["synthetic retained body"]);
    assert_eq!(f.receipts(), 1);
}
