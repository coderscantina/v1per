//! The generator is reproducible and windows are independent of how a run is split.

use v1per_workload::{HOUR_MS, Manifest, Workload};

const START_MS: i64 = 1_790_812_800_000; // 2026-10-01T00:00:00Z

#[test]
fn same_seed_gives_the_same_results_however_the_window_is_split() {
    let workload = Workload::new(Manifest::default());
    let whole = workload.generate(START_MS, START_MS + HOUR_MS);
    let again = Workload::new(Manifest::default()).generate(START_MS, START_MS + HOUR_MS);
    assert_eq!(whole, again);

    let mut split = workload.generate(START_MS, START_MS + HOUR_MS / 3);
    let rest = workload.generate(START_MS + HOUR_MS / 3, START_MS + HOUR_MS);
    split.heartbeats.extend(rest.heartbeats);
    split.statuses.extend(rest.statuses);
    split
        .heartbeats
        .sort_by_key(|h| (h.time_ms, h.monitor, h.probe));
    split.statuses.sort_by_key(|s| (s.time_ms, s.monitor));
    assert_eq!(whole, split);

    let other = Workload::new(Manifest {
        seed: 7,
        ..Manifest::default()
    })
    .generate(START_MS, START_MS + HOUR_MS);
    assert_ne!(whole.heartbeats, other.heartbeats);
}

#[test]
fn the_mix_matches_the_release_gate() {
    let workload = Workload::new(Manifest::default());
    let monitors = workload.monitors();
    assert_eq!(monitors.len(), 2_000);
    assert_eq!(
        monitors.iter().filter(|m| m.interval_ms == 20_000).count(),
        400
    );
    let multi = monitors.iter().filter(|m| m.probes.len() == 3).count();
    assert!(
        (150..250).contains(&multi),
        "{multi} monitors on three probes"
    );

    // One check per monitor and interval: 400 * 180 + 1,600 * 60 an hour.
    let hour = workload.generate(START_MS, START_MS + HOUR_MS);
    assert_eq!(hour.statuses.len(), 400 * 180 + 1_600 * 60);
}
