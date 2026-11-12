use agentos_resource_accounting::{Reservation, ResourceClass, ResourceLedger, ResourceLimit};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::{
    future::Future,
    task::{Context, Poll, Wake, Waker},
};

#[derive(Default)]
struct CountWakes(AtomicUsize);

impl Wake for CountWakes {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn ancestor_release_wakes_a_grandchild_waiting_for_capacity() {
    let root = Arc::new(ResourceLedger::root(
        "process",
        [(
            ResourceClass::Sockets,
            ResourceLimit::new(1, "test.sockets"),
        )],
    ));
    let child = Arc::new(ResourceLedger::child("vm", [], Arc::clone(&root)));
    let leaf = ResourceLedger::child("execution", [], child);
    let held = root.reserve(ResourceClass::Sockets, 1).unwrap();
    let mut pending = Box::pin(leaf.reserve_when_available(ResourceClass::Sockets, 1));
    let wakes = Arc::new(CountWakes::default());
    let waker = Waker::from(Arc::clone(&wakes));
    let mut context = Context::from_waker(&waker);
    assert!(pending.as_mut().poll(&mut context).is_pending());
    assert_eq!(wakes.0.load(Ordering::SeqCst), 0);
    drop(held);
    assert!(
        wakes.0.load(Ordering::SeqCst) > 0,
        "ancestor release must schedule the parked descendant"
    );
    let Poll::Ready(result) = pending.as_mut().poll(&mut context) else {
        panic!("an ancestor release must wake the descendant waiter");
    };
    drop(result.unwrap());
    assert!(root.is_zero() && leaf.is_zero());
}

#[test]
fn unspecified_limit_still_cannot_overflow_the_counter() {
    let ledger = ResourceLedger::root("process", []);
    let held = ledger.reserve(ResourceClass::Sockets, usize::MAX).unwrap();
    assert!(!ledger.capacity_available(ResourceClass::Sockets, 1));
    assert!(ledger.reserve(ResourceClass::Sockets, 1).is_err());
    drop(held);
    assert!(ledger.is_zero() && ledger.integrity_ok());
}

#[test]
fn duplicate_requests_cannot_overflow_an_admission_total() {
    let ledger = ResourceLedger::root(
        "process",
        [(
            ResourceClass::Sockets,
            ResourceLimit::new(usize::MAX, "test.sockets"),
        )],
    );
    let result = ledger.reserve_all(&[
        (ResourceClass::Sockets, usize::MAX),
        (ResourceClass::Sockets, 1),
    ]);
    assert!(
        result.is_err(),
        "overflow must reject before changing counters"
    );
    assert!(ledger.is_zero());
    assert!(ledger.integrity_ok());
}

#[test]
fn generated_admission_and_release_traces_match_a_hierarchical_model() {
    const CLASSES: [ResourceClass; 2] = [ResourceClass::Sockets, ResourceClass::BufferedBytes];
    const PARENTS: [Option<usize>; 4] = [None, Some(0), Some(1), Some(0)];
    const LIMITS: [[usize; 2]; 4] = [[97, 121], [71, 83], [43, 61], [53, 79]];

    // Seeds are stable replay inputs. Each trace combines duplicate-resource
    // batches, ancestor rejection, zero-sized admissions and arbitrary releases.
    for seed in 1..=32_u64 {
        let mut state = seed;
        let mut random = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as usize
        };
        let mut ledgers: Vec<Arc<ResourceLedger>> = Vec::new();
        for index in 0..4 {
            let limits = CLASSES.into_iter().enumerate().map(|(class, resource)| {
                (
                    resource,
                    ResourceLimit::new(LIMITS[index][class], "test.capacity"),
                )
            });
            ledgers.push(Arc::new(match PARENTS[index] {
                Some(parent) => ResourceLedger::child(
                    format!("scope-{index}"),
                    limits,
                    Arc::clone(&ledgers[parent]),
                ),
                None => ResourceLedger::root("process", limits),
            }));
        }
        let mut used = [[0_usize; 2]; 4];
        let mut live: Vec<(usize, usize, usize, Reservation)> = Vec::new();
        for step in 0..512 {
            if !live.is_empty() && random() % 3 == 0 {
                let index = random() % live.len();
                let (scope, class, amount, reservation) = live.swap_remove(index);
                drop(reservation);
                let mut ancestor = Some(scope);
                while let Some(index) = ancestor {
                    used[index][class] -= amount;
                    ancestor = PARENTS[index];
                }
            } else {
                let scope = random() % ledgers.len();
                let requests: Vec<_> = (0..1 + random() % 3)
                    .map(|_| (random() % CLASSES.len(), random() % 25))
                    .collect();
                let mut expected = used;
                let mut fits = true;
                for &(class, amount) in &requests {
                    let mut ancestor = Some(scope);
                    while let Some(index) = ancestor {
                        expected[index][class] += amount;
                        fits &= expected[index][class] <= LIMITS[index][class];
                        ancestor = PARENTS[index];
                    }
                }
                let actual = ledgers[scope].reserve_all(
                    &requests
                        .iter()
                        .map(|&(class, amount)| (CLASSES[class], amount))
                        .collect::<Vec<_>>(),
                );
                assert_eq!(
                    actual.is_ok(),
                    fits,
                    "seed={seed} step={step} scope={scope} requests={requests:?}"
                );
                if let Ok(reservations) = actual {
                    used = expected;
                    for ((class, amount), reservation) in requests.into_iter().zip(reservations) {
                        live.push((scope, class, amount, reservation));
                    }
                }
            }
            for (scope, ledger) in ledgers.iter().enumerate() {
                for (class, resource) in CLASSES.into_iter().enumerate() {
                    assert_eq!(
                        ledger.usage(resource).used,
                        used[scope][class],
                        "seed={seed} step={step} scope={scope} class={class}"
                    );
                }
                assert!(
                    ledger.integrity_ok(),
                    "seed={seed} step={step} scope={scope}"
                );
            }
        }
        drop(live);
        assert!(ledgers
            .iter()
            .all(|ledger| ledger.is_zero() && ledger.integrity_ok()));
    }
}
