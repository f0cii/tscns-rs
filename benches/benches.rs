use criterion::{criterion_group, criterion_main, Criterion};

use tscns::read_nanos;
use std::sync::atomic::{AtomicBool};

fn bench_tsc_avg(c: &mut Criterion) {
    tscns::init(tscns::INIT_CALIBRATE_NANOS, tscns::CALIBRATE_INTERVAL_NANOS);
    let running = std::sync::Arc::new(AtomicBool::new(true));
    let thread_running = running.clone();
    std::thread::spawn(move || {
        while thread_running.load(std::sync::atomic::Ordering::Acquire) {
            tscns::calibrate();
            std::thread::sleep(std::time::Duration::from_nanos(tscns::CALIBRATE_INTERVAL_NANOS as u64));
        }
    });

    c.bench_function("tsc_avg", |b| b.iter(|| {
        let _ns = read_nanos();
    }));
    running.store(false, std::sync::atomic::Ordering::Release);
}

criterion_group!(benches, bench_tsc_avg);
criterion_main!(benches);