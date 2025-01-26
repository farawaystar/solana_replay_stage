use {
    crate::{
        consensus::{
            heaviest_subtree_fork_choice::HeaviestSubtreeForkChoice,
            progress_map::{ForkProgress, ProgressMap, PropagatedStats},
        },
        repair::{
            ancestor_hashes_service::AncestorHashesReplayUpdateSender,
            cluster_slot_state_verifier::*,
        },
    },
    solana_entry::entry::VerifyRecyclers,
    rayon::ThreadPool,
    solana_ledger::{
        block_error::BlockError,
        blockstore::Blockstore,
        blockstore_processor::{
            self, BlockstoreProcessorError, ConfirmationProgress,
            ReplaySlotStats, TransactionStatusSender,
        },
        entry_notifier_service::EntryNotifierSender,
    },
    solana_rpc::{
        optimistically_confirmed_bank_tracker::*,
        rpc_subscriptions::RpcSubscriptions,
        slot_status_notifier::SlotStatusNotifier,
    },
    solana_rpc_client_api::response::SlotUpdate,
    solana_runtime::{
        bank::Bank,
        commitment::BlockCommitmentCache,
        installed_scheduler_pool::BankWithScheduler,
        prioritization_fee_cache::PrioritizationFeeCache,
        vote_sender_types::ReplayVoteSender,
    },
    solana_sdk::{
        clock::*,
        pubkey::Pubkey,
        signature::Keypair,
        timing::timestamp,
    },
    std::{
        collections::HashSet,
        result,
        sync::{
        atomic::{AtomicBool, AtomicU64},
            Arc, RwLock,
        },
    },
};

#[allow(clippy::too_many_arguments)]
fn replay_blockstore_into_bank(
    bank: &BankWithScheduler,
    blockstore: &Blockstore,
    replay_tx_thread_pool: &ThreadPool,
    replay_stats: &RwLock<ReplaySlotStats>,
    replay_progress: &RwLock<ConfirmationProgress>,
    transaction_status_sender: Option<&TransactionStatusSender>,
    entry_notification_sender: Option<&EntryNotifierSender>,
    replay_vote_sender: &ReplayVoteSender,
    verify_recyclers: &VerifyRecyclers,
    log_messages_bytes_limit: Option<usize>,
    prioritization_fee_cache: &PrioritizationFeeCache,
) -> result::Result<usize, BlockstoreProcessorError> {
    let mut w_replay_stats = replay_stats.write().unwrap();
    let mut w_replay_progress = replay_progress.write().unwrap();
    let tx_count_before = w_replay_progress.num_txs;
    // All errors must lead to marking the slot as dead, otherwise,
    // the `check_slot_agrees_with_cluster()` called by `replay_active_banks()`
    // will break!
    blockstore_processor::confirm_slot(
        blockstore,
        bank,
        replay_tx_thread_pool,
        &mut w_replay_stats,
        &mut w_replay_progress,
        false,
        transaction_status_sender,
        entry_notification_sender,
        Some(replay_vote_sender),
        verify_recyclers,
        false,
        log_messages_bytes_limit,
        prioritization_fee_cache,
    )?;
    let tx_count_after = w_replay_progress.num_txs;
    let tx_count = tx_count_after - tx_count_before;
    Ok(tx_count)
}

#[allow(clippy::too_many_arguments)]
fn mark_dead_slot(
    blockstore: &Blockstore,
    bank: &Bank,
    root: Slot,
    err: &BlockstoreProcessorError,
    rpc_subscriptions: &Arc<RpcSubscriptions>,
    slot_status_notifier: &Option<SlotStatusNotifier>,
    duplicate_slots_tracker: &mut DuplicateSlotsTracker,
    duplicate_confirmed_slots: &DuplicateConfirmedSlots,
    epoch_slots_frozen_slots: &mut EpochSlotsFrozenSlots,
    progress: &mut ProgressMap,
    heaviest_subtree_fork_choice: &mut HeaviestSubtreeForkChoice,
    duplicate_slots_to_repair: &mut DuplicateSlotsToRepair,
    ancestor_hashes_replay_update_sender: &AncestorHashesReplayUpdateSender,
    purge_repair_slot_counter: &mut PurgeRepairSlotCounter,
) {
    // Do not remove from progress map when marking dead! Needed by
    // `process_duplicate_confirmed_slots()`

    // Block producer can abandon the block if it detects a better one
    // while producing. Somewhat common and expected in a
    // network with variable network/machine configuration.
    let is_serious = !matches!(
        err,
        BlockstoreProcessorError::InvalidBlock(BlockError::TooFewTicks)
    );
    let slot = bank.slot();
    if is_serious {
        datapoint_error!(
            "replay-stage-mark_dead_slot",
            ("error", format!("error: {err:?}"), String),
            ("slot", slot, i64)
        );
    } else {
        datapoint_info!(
            "replay-stage-mark_dead_slot",
            ("error", format!("error: {err:?}"), String),
            ("slot", slot, i64)
        );
    }
    progress.get_mut(&slot).unwrap().is_dead = true;
    blockstore
        .set_dead_slot(slot)
        .expect("Failed to mark slot as dead in blockstore");

    blockstore.slots_stats.mark_dead(slot);

    let err = format!("error: {err:?}");

    if let Some(slot_status_notifier) = slot_status_notifier {
        slot_status_notifier
            .read()
            .unwrap()
            .notify_slot_dead(slot, err.clone());
    }

    rpc_subscriptions.notify_slot_update(SlotUpdate::Dead {
        slot,
        err,
        timestamp: timestamp(),
    });

    let dead_state = DeadState::new_from_state(
        slot,
        duplicate_slots_tracker,
        duplicate_confirmed_slots,
        heaviest_subtree_fork_choice,
        epoch_slots_frozen_slots,
    );
    check_slot_agrees_with_cluster(
        slot,
        root,
        blockstore,
        duplicate_slots_tracker,
        epoch_slots_frozen_slots,
        heaviest_subtree_fork_choice,
        duplicate_slots_to_repair,
        ancestor_hashes_replay_update_sender,
        purge_repair_slot_counter,
        SlotStateUpdate::Dead(dead_state),
    );

    // If we previously marked this slot as duplicate in blockstore, let the state machine know
    if !duplicate_slots_tracker.contains(&slot) && blockstore.get_duplicate_slot(slot).is_some()
    {
        let duplicate_state = DuplicateState::new_from_state(
            slot,
            duplicate_confirmed_slots,
            heaviest_subtree_fork_choice,
            || true,
            || None,
        );
        check_slot_agrees_with_cluster(
            slot,
            root,
            blockstore,
            duplicate_slots_tracker,
            epoch_slots_frozen_slots,
            heaviest_subtree_fork_choice,
            duplicate_slots_to_repair,
            ancestor_hashes_replay_update_sender,
            purge_repair_slot_counter,
            SlotStateUpdate::Duplicate(duplicate_state),
        );
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use {
        super::*,
        crate::{
                replay_stage::tests::{ReplayBlockstoreComponents, replay_blockstore_components},
                vote_simulator::{self, VoteSimulator},
        },
        solana_entry::entry::{self, Entry},
        crossbeam_channel::unbounded,
        solana_ledger::{
            blockstore::{entries_to_test_shreds, make_slot_entries, BlockstoreError},
            get_tmp_ledger_path, get_tmp_ledger_path_auto_delete,
            shred::{Shred, ShredFlags, LEGACY_SHRED_DATA_CAPACITY},
        },
        std::{
            fs::remove_dir_all,
            sync::{atomic::AtomicU64, Arc, Mutex, RwLock},
        },
        solana_rpc::{
            rpc_subscriptions::RpcSubscriptions,
            slot_status_notifier::SlotStatusNotifierInterface,
        },
        trees::{tr, Tree},
    };
    
 
#[test]
fn test_dead_fork_invalid_tick_hash_count() {
    let res = check_dead_fork(|_keypair, bank| {
        let blockhash = bank.last_blockhash();
        let slot = bank.slot();
        let hashes_per_tick = bank.hashes_per_tick().unwrap_or(0);
        assert!(hashes_per_tick > 0);

        let too_few_hashes_tick = Entry::new(&blockhash, hashes_per_tick - 1, vec![]);
        entries_to_test_shreds(
            &[too_few_hashes_tick],
            slot,
            slot.saturating_sub(1),
            false,
            0,
            true, // merkle_variant
        )
    });

    if let Err(BlockstoreProcessorError::InvalidBlock(block_error)) = res {
        assert_eq!(block_error, BlockError::InvalidTickHashCount);
    } else {
        panic!();
    }
}

struct SlotStatusNotifierForTest {
    dead_slots: Arc<Mutex<HashSet<Slot>>>,
}

impl SlotStatusNotifierForTest {
    pub fn new(dead_slots: Arc<Mutex<HashSet<Slot>>>) -> Self {
        Self { dead_slots }
    }
}

impl SlotStatusNotifierInterface for SlotStatusNotifierForTest {
    fn notify_slot_confirmed(&self, _slot: Slot, _parent: Option<Slot>) {}

    fn notify_slot_processed(&self, _slot: Slot, _parent: Option<Slot>) {}

    fn notify_slot_rooted(&self, _slot: Slot, _parent: Option<Slot>) {}

    fn notify_first_shred_received(&self, _slot: Slot) {}

    fn notify_completed(&self, _slot: Slot) {}

    fn notify_created_bank(&self, _slot: Slot, _parent: Slot) {}

    fn notify_slot_dead(&self, slot: Slot, _error: String) {
        self.dead_slots.lock().unwrap().insert(slot);
    }
}
    // Given a shred and a fatal expected error, check that replaying that shred causes causes the fork to be
    // marked as dead. Returns the error for caller to verify.
    fn check_dead_fork<F>(shred_to_insert: F) -> result::Result<(), BlockstoreProcessorError>
    where
        F: Fn(&Keypair, Arc<Bank>) -> Vec<Shred>,
    {
        let ledger_path = get_tmp_ledger_path!();
        let (replay_vote_sender, _replay_vote_receiver) = unbounded();
        let res = {
            let ReplayBlockstoreComponents {
                blockstore,
                vote_simulator,
                ..
            } = replay_blockstore_components(Some(tr(0)), 1, None);
            let VoteSimulator {
                mut progress,
                bank_forks,
                mut heaviest_subtree_fork_choice,
                validator_keypairs,
                ..
            } = vote_simulator;

            let bank0 = bank_forks.read().unwrap().get(0).unwrap();
            assert!(bank0.is_frozen());
            assert_eq!(bank0.tick_height(), bank0.max_tick_height());
            let bank1 = Bank::new_from_parent(bank0, &Pubkey::default(), 1);
            bank_forks.write().unwrap().insert(bank1);
            let bank1 = bank_forks.read().unwrap().get_with_scheduler(1).unwrap();
            let bank1_progress = progress
                .entry(bank1.slot())
                .or_insert_with(|| ForkProgress::new(bank1.last_blockhash(), None, None, 0, 0));
            let shreds = shred_to_insert(
                &validator_keypairs.values().next().unwrap().node_keypair,
                bank1.clone(),
            );
            blockstore.insert_shreds(shreds, None, false).unwrap();
            let block_commitment_cache = Arc::new(RwLock::new(BlockCommitmentCache::default()));
            let exit = Arc::new(AtomicBool::new(false));
            let replay_tx_thread_pool = rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .thread_name(|i| format!("solReplayTest{i:02}"))
                .build()
                .expect("new rayon threadpool");
            let res = replay_blockstore_into_bank(
                &bank1,
                &blockstore,
                &replay_tx_thread_pool,
                &bank1_progress.replay_stats,
                &bank1_progress.replay_progress,
                None,
                None,
                &replay_vote_sender,
                &VerifyRecyclers::default(),
                None,
                &PrioritizationFeeCache::new(0u64),
            );
            let max_complete_transaction_status_slot = Arc::new(AtomicU64::default());
            let max_complete_rewards_slot = Arc::new(AtomicU64::default());
            let rpc_subscriptions = Arc::new(RpcSubscriptions::new_for_tests(
                exit,
                max_complete_transaction_status_slot,
                max_complete_rewards_slot,
                bank_forks.clone(),
                block_commitment_cache,
                OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks),
            ));
            let (ancestor_hashes_replay_update_sender, _ancestor_hashes_replay_update_receiver) =
                unbounded();
            let dead_slots = Arc::new(Mutex::new(HashSet::default()));

            let slot_status_notifier: Option<SlotStatusNotifier> = Some(Arc::new(RwLock::new(
                SlotStatusNotifierForTest::new(dead_slots.clone()),
            )));

            if let Err(err) = &res {
                mark_dead_slot(
                    &blockstore,
                    &bank1,
                    0,
                    err,
                    &rpc_subscriptions,
                    &slot_status_notifier,
                    &mut DuplicateSlotsTracker::default(),
                    &DuplicateConfirmedSlots::new(),
                    &mut EpochSlotsFrozenSlots::default(),
                    &mut progress,
                    &mut heaviest_subtree_fork_choice,
                    &mut DuplicateSlotsToRepair::default(),
                    &ancestor_hashes_replay_update_sender,
                    &mut PurgeRepairSlotCounter::default(),
                );
            }
            assert!(dead_slots.lock().unwrap().contains(&bank1.slot()));
            // Check that the erroring bank was marked as dead in the progress map
            assert!(progress
                .get(&bank1.slot())
                .map(|b| b.is_dead)
                .unwrap_or(false));

            // Check that the erroring bank was marked as dead in blockstore
            assert!(blockstore.is_dead(bank1.slot()));
            res.map(|_| ())
        };
        let _ignored = remove_dir_all(ledger_path);
        res
    }
}