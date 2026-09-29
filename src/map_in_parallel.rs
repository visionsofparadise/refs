use std::sync::atomic::{AtomicUsize, Ordering};

pub fn map_in_parallel<T: Sync, R: Send>(items: &[T], work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = AtomicUsize::new(0);

    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(items.len().max(1));

    let mut results: Vec<(usize, R)> = std::thread::scope(|threads| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                threads.spawn(|| {
                    let mut results = Vec::new();

                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);

                        let Some(item) = items.get(index) else {
                            return results;
                        };

                        results.push((index, work(item)));
                    }
                })
            })
            .collect();

        handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap())
            .collect()
    });

    results.sort_unstable_by_key(|(index, _)| *index);

    results.into_iter().map(|(_, result)| result).collect()
}
