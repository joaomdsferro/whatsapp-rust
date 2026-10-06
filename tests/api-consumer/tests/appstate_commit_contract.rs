#[path = "../../../wacore/tests/support/appstate_commit_contract.rs"]
mod contract;

#[test]
fn downstream_consumer_commits_patch_as_one_operation() {
    futures::executor::block_on(contract::assert_patch_commit_contract(
        &wacore::store::InMemoryBackend::new(),
    ));
}
