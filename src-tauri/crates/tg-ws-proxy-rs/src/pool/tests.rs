use super::*;

#[test]
fn direct_refill_is_reserved_before_a_task_can_be_spawned() {
    let pool = WsPool::new(4, Duration::from_secs(60));
    let key = (2, true);

    assert!(pool.reserve_refill(key));
    for _ in 0..1_000 {
        assert!(!pool.reserve_refill(key));
    }
    assert_eq!(pool.refilling.lock().unwrap().len(), 1);
}

#[test]
fn cloudflare_refill_is_reserved_before_a_task_can_be_spawned() {
    let pool = WsPool::new(4, Duration::from_secs(60));
    let key = (CfTier::Worker, 2, true);

    assert!(pool.reserve_cf_refill(key));
    for _ in 0..1_000 {
        assert!(!pool.reserve_cf_refill(key));
    }
    assert_eq!(pool.cf_refilling.lock().unwrap().len(), 1);
}

#[test]
fn failed_refill_backs_off_until_a_success_resets_it() {
    let pool = WsPool::new(4, Duration::from_secs(120));
    let key = (4, false);

    pool.report_refill_failure(key);
    assert!(!pool.reserve_refill(key));

    pool.report_success(key.0, key.1);
    assert!(pool.reserve_refill(key));
}

#[test]
fn refill_backoff_is_capped_at_one_hour() {
    let pool = WsPool::new(4, Duration::from_secs(120));
    let key = (2, true);

    for _ in 0..32 {
        pool.report_refill_failure(key);
    }

    let remaining =
        pool.refill_after.lock().unwrap()[&key].saturating_duration_since(Instant::now());
    assert!(remaining <= REFILL_BACKOFF_MAX);
    assert!(remaining > Duration::from_secs(3500));
}

#[tokio::test]
async fn a_direct_miss_burst_spawns_one_refill_task() {
    let pool = Arc::new(WsPool::new(4, Duration::from_secs(60)));

    for _ in 0..1_000 {
        pool.schedule_refill(2, true, "203.0.113.10", false);
    }

    assert_eq!(pool.refill_task_spawns.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn a_cloudflare_prefetch_burst_spawns_one_refill_task() {
    let pool = Arc::new(WsPool::new(4, Duration::from_secs(60)));

    for _ in 0..1_000 {
        pool.cf_prefetch(CfTarget {
            tier: CfTier::Worker,
            dc: 2,
            is_media: true,
            dst: "149.154.167.51".to_string(),
            domain: "worker.example".to_string(),
            skip_tls_verify: false,
            connect_timeout: Duration::from_secs(1),
        });
    }

    assert_eq!(pool.cf_refill_task_spawns.load(Ordering::Relaxed), 1);
}
