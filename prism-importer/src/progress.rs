use indicatif::{ProgressBar, ProgressStyle};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

pub struct ImportProgress {
    bar: ProgressBar,
    imported: AtomicU64,
    failed: AtomicU64,
    start: Instant,
}

impl ImportProgress {
    pub fn new(total: u64) -> Self {
        let bar = ProgressBar::new(total);
        bar.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({per_sec}) ETA: {eta}"
            )
            .unwrap()
            .progress_chars("#>-"),
        );

        Self {
            bar,
            imported: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            start: Instant::now(),
        }
    }

    pub fn inc(&self, count: u64) {
        self.imported.fetch_add(count, Ordering::Relaxed);
        self.bar.inc(count);
    }

    pub fn inc_failed(&self, count: u64) {
        self.failed.fetch_add(count, Ordering::Relaxed);
        self.bar.inc(count);
    }

    pub fn finish(&self) {
        let imported = self.imported.load(Ordering::Relaxed);
        let failed = self.failed.load(Ordering::Relaxed);
        let elapsed = self.start.elapsed();

        self.bar.finish_with_message(format!(
            "Done! Imported {} documents in {:.1}s ({} failed)",
            imported,
            elapsed.as_secs_f64(),
            failed
        ));
    }

    pub fn imported(&self) -> u64 {
        self.imported.load(Ordering::Relaxed)
    }

    pub fn failed(&self) -> u64 {
        self.failed.load(Ordering::Relaxed)
    }
}
