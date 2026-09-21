//! Lock-free process statistics, matching Flowseal's minute diagnostics.

use std::sync::atomic::{AtomicU64, Ordering};

pub static STATS: Stats = Stats::new();

pub struct Stats {
    connections_total: AtomicU64,
    connections_active: AtomicU64,
    connections_ws: AtomicU64,
    connections_tcp_fallback: AtomicU64,
    connections_cfproxy: AtomicU64,
    connections_fronting: AtomicU64,
    connections_bad: AtomicU64,
    connections_masked: AtomicU64,
    ws_errors: AtomicU64,
    bytes_up: AtomicU64,
    bytes_down: AtomicU64,
    pool_hits: AtomicU64,
    pool_misses: AtomicU64,
    cf_pool_hits: AtomicU64,
    cf_pool_misses: AtomicU64,
}

pub struct ActiveConnection;

impl Drop for ActiveConnection {
    fn drop(&mut self) {
        let _ =
            STATS
                .connections_active
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |active| {
                    Some(active.saturating_sub(1))
                });
    }
}

impl Stats {
    const fn new() -> Self {
        Self {
            connections_total: AtomicU64::new(0),
            connections_active: AtomicU64::new(0),
            connections_ws: AtomicU64::new(0),
            connections_tcp_fallback: AtomicU64::new(0),
            connections_cfproxy: AtomicU64::new(0),
            connections_fronting: AtomicU64::new(0),
            connections_bad: AtomicU64::new(0),
            connections_masked: AtomicU64::new(0),
            ws_errors: AtomicU64::new(0),
            bytes_up: AtomicU64::new(0),
            bytes_down: AtomicU64::new(0),
            pool_hits: AtomicU64::new(0),
            pool_misses: AtomicU64::new(0),
            cf_pool_hits: AtomicU64::new(0),
            cf_pool_misses: AtomicU64::new(0),
        }
    }

    pub fn reset(&self) {
        for counter in [
            &self.connections_total,
            &self.connections_active,
            &self.connections_ws,
            &self.connections_tcp_fallback,
            &self.connections_cfproxy,
            &self.connections_fronting,
            &self.connections_bad,
            &self.connections_masked,
            &self.ws_errors,
            &self.bytes_up,
            &self.bytes_down,
            &self.pool_hits,
            &self.pool_misses,
            &self.cf_pool_hits,
            &self.cf_pool_misses,
        ] {
            counter.store(0, Ordering::Relaxed);
        }
    }

    pub fn connection_started(&self) -> ActiveConnection {
        self.connections_total.fetch_add(1, Ordering::Relaxed);
        self.connections_active.fetch_add(1, Ordering::Relaxed);
        ActiveConnection
    }

    pub fn ws(&self) {
        self.connections_ws.fetch_add(1, Ordering::Relaxed);
    }

    pub fn tcp_fallback(&self) {
        self.connections_tcp_fallback
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn cfproxy(&self) {
        self.connections_cfproxy.fetch_add(1, Ordering::Relaxed);
    }

    pub fn fronting(&self) {
        self.connections_fronting.fetch_add(1, Ordering::Relaxed);
    }

    pub fn bad(&self) {
        self.connections_bad.fetch_add(1, Ordering::Relaxed);
    }

    pub fn masked(&self) {
        self.connections_masked.fetch_add(1, Ordering::Relaxed);
    }

    pub fn ws_error(&self) {
        self.ws_errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn traffic(&self, up: u64, down: u64) {
        self.bytes_up.fetch_add(up, Ordering::Relaxed);
        self.bytes_down.fetch_add(down, Ordering::Relaxed);
    }

    pub fn pool_hit(&self) {
        self.pool_hits.fetch_add(1, Ordering::Relaxed);
    }

    pub fn pool_miss(&self) {
        self.pool_misses.fetch_add(1, Ordering::Relaxed);
    }

    pub fn cf_pool_hit(&self) {
        self.cf_pool_hits.fetch_add(1, Ordering::Relaxed);
    }

    pub fn cf_pool_miss(&self) {
        self.cf_pool_misses.fetch_add(1, Ordering::Relaxed);
    }

    pub fn summary(&self) -> String {
        let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        let pool_hits = load(&self.pool_hits);
        let pool_total = pool_hits + load(&self.pool_misses);
        let cf_pool_hits = load(&self.cf_pool_hits);
        let cf_pool_total = cf_pool_hits + load(&self.cf_pool_misses);
        let ratio = |hits, total| {
            if total == 0 {
                "n/a".to_string()
            } else {
                format!("{hits}/{total}")
            }
        };

        format!(
            "total={} active={} ws={} tcp_fb={} cf={} front={} bad={} masked={} err={} pool={} cf_pool={} up={} down={}",
            load(&self.connections_total),
            load(&self.connections_active),
            load(&self.connections_ws),
            load(&self.connections_tcp_fallback),
            load(&self.connections_cfproxy),
            load(&self.connections_fronting),
            load(&self.connections_bad),
            load(&self.connections_masked),
            load(&self.ws_errors),
            ratio(pool_hits, pool_total),
            ratio(cf_pool_hits, cf_pool_total),
            human_bytes(load(&self.bytes_up)),
            human_bytes(load(&self.bytes_down)),
        )
    }
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_matches_flowseal_field_layout() {
        let stats = Stats::new();
        stats.pool_hit();
        stats.traffic(2048, 1024);

        assert_eq!(
            stats.summary(),
            "total=0 active=0 ws=0 tcp_fb=0 cf=0 front=0 bad=0 masked=0 err=0 pool=1/1 cf_pool=n/a up=2.0 KiB down=1.0 KiB"
        );
    }
}
