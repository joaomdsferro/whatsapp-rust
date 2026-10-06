#![cfg(not(target_family = "wasm"))]
#[path = "support/storage_fixture.rs"]
mod fixture;

use diesel::connection::SimpleConnection;
use wacore::appstate::{hash::HashState, processor::AppStateMutationMAC};
use wacore::store::{error::StoreError, traits::AppSyncStore};
use whatsapp_rust_sqlite_storage::SqliteDatabase;

#[tokio::test]
async fn patch_write_failure_rolls_back_cursor_and_both_mac_changes() {
    for statement in [
        "INSERT ON app_state_versions",
        "DELETE ON app_state_mutation_macs",
        "INSERT ON app_state_mutation_macs",
    ] {
        let fixture = fixture::Fixture::new();
        let db = SqliteDatabase::open(&fixture.url(), Default::default())
            .await
            .unwrap();
        let store = db.provision_device(1).await.unwrap();
        let other = db.create_device().await.unwrap();
        let before = HashState {
            version: 1,
            hash: [1; 128],
            ..Default::default()
        };
        let after = HashState {
            version: 2,
            hash: [2; 128],
            ..Default::default()
        };
        let old = AppStateMutationMAC {
            index_mac: vec![1; 32],
            value_mac: vec![11; 32],
        };
        let new = AppStateMutationMAC {
            index_mac: vec![2; 32],
            value_mac: vec![22; 32],
        };
        for scope in [&store, &other] {
            scope
                .commit_patch("regular", before.clone(), &[], std::slice::from_ref(&old))
                .await
                .unwrap();
        }
        let trigger = format!(
            "CREATE TRIGGER fail_commit BEFORE {statement} BEGIN SELECT RAISE(ABORT, 'synthetic commit failure'); END;"
        );
        db.shared()
            .run(move |conn| {
                conn.batch_execute(&trigger)
                    .map_err(|e| StoreError::Database(Box::new(e)))
            })
            .await
            .unwrap();
        let err = store
            .commit_patch(
                "regular",
                after.clone(),
                std::slice::from_ref(&old.index_mac),
                std::slice::from_ref(&new),
            )
            .await
            .unwrap_err();
        let source = std::error::Error::source(&err).expect("database error keeps its cause");
        assert!(source.downcast_ref::<diesel::result::Error>().is_some());
        assert!(
            source.to_string().contains("synthetic commit failure"),
            "{source}"
        );
        for scope in [&store, &other] {
            let held = scope.get_version("regular").await.unwrap().unwrap();
            assert_eq!(held.version, before.version, "{statement}");
            assert_eq!(held.hash, before.hash);
            assert_eq!(
                scope
                    .get_mutation_mac("regular", &old.index_mac)
                    .await
                    .unwrap(),
                Some(old.value_mac.clone())
            );
            assert!(
                scope
                    .get_mutation_mac("regular", &new.index_mac)
                    .await
                    .unwrap()
                    .is_none()
            );
        }
        db.shared()
            .run(|conn| {
                conn.batch_execute("DROP TRIGGER fail_commit;")
                    .map_err(|e| StoreError::Database(Box::new(e)))
            })
            .await
            .unwrap();
        store
            .commit_patch(
                "regular",
                after,
                std::slice::from_ref(&old.index_mac),
                std::slice::from_ref(&new),
            )
            .await
            .unwrap();
        assert_eq!(
            store.get_version("regular").await.unwrap().unwrap().version,
            2
        );
        assert!(
            store
                .get_mutation_mac("regular", &old.index_mac)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            store
                .get_mutation_mac("regular", &new.index_mac)
                .await
                .unwrap(),
            Some(new.value_mac)
        );
        assert_eq!(
            other.get_version("regular").await.unwrap().unwrap().version,
            1
        );
    }
}
