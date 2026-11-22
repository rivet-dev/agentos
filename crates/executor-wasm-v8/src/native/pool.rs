//! Guest threads with their isolate already created.
//!
//! Creating an isolate costs about a millisecond, most of a short command's
//! setup. V8 ties an isolate's stack limits to the thread that created it, so
//! the pool parks threads, each holding a fresh isolate, and a launch hands
//! its guest to one of them. Every isolate still runs exactly one guest and is
//! disposed with it; nothing is shared between guests.

use agentos_executor_v8_runtime::isolate::{create_isolate, GuestIsolate};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

/// Parked guest threads kept ready. Each holds an idle isolate.
const DEFAULT_WARM_ISOLATES: usize = 4;
/// Upper bound for the operator knob, so a typo cannot park unbounded threads.
const MAX_WARM_ISOLATES: usize = 64;
pub(super) const WARM_ISOLATES_ENV: &str = "AGENTOS_V8_WASM_WARM_ISOLATES";

pub(super) type Guest = Box<dyn FnOnce(GuestIsolate) + Send>;

struct Pool {
    heap_limit_mb: u32,
    capacity: usize,
    parked: Mutex<Vec<flume::Sender<Guest>>>,
    /// Parked plus still-starting threads, so refills never exceed capacity.
    reserved: AtomicUsize,
}

fn pool(heap_limit_mb: u32) -> &'static Pool {
    static POOL: OnceLock<Pool> = OnceLock::new();
    POOL.get_or_init(|| {
        let capacity = match std::env::var(WARM_ISOLATES_ENV) {
            Ok(value) => match value.parse::<usize>() {
                Ok(count) if count <= MAX_WARM_ISOLATES => count,
                _ => {
                    eprintln!(
                        "WARN_AGENTOS_V8_WASM_WARM_ISOLATES: {WARM_ISOLATES_ENV}={value:?} is not 0..={MAX_WARM_ISOLATES}; using {DEFAULT_WARM_ISOLATES}"
                    );
                    DEFAULT_WARM_ISOLATES
                }
            },
            Err(_) => DEFAULT_WARM_ISOLATES,
        };
        Pool {
            heap_limit_mb,
            capacity,
            parked: Mutex::new(Vec::new()),
            reserved: AtomicUsize::new(0),
        }
    })
}

/// Run `guest` on a thread with an isolate of `heap_limit_mb`: a parked warm
/// thread when one matches, otherwise a new thread.
pub(super) fn launch(name: String, heap_limit_mb: u32, guest: Guest) -> std::io::Result<()> {
    let pool = pool(heap_limit_mb);
    let mut guest = guest;
    if pool.heap_limit_mb == heap_limit_mb {
        while let Some(parked) = pool.take() {
            match parked.send(guest) {
                Ok(()) => {
                    pool.refill();
                    return Ok(());
                }
                // The parked thread is gone (it failed to start); try another.
                Err(flume::SendError(returned)) => guest = returned,
            }
        }
        pool.refill();
    }
    // AGENTOS_THREAD_SITE: admitted-v8-wasm-guest-executor
    std::thread::Builder::new().name(name).spawn(move || {
        guest(create_isolate(Some(heap_limit_mb)));
    })?;
    Ok(())
}

impl Pool {
    fn take(&self) -> Option<flume::Sender<Guest>> {
        let parked = self.parked.lock().ok()?.pop()?;
        self.reserved.fetch_sub(1, Ordering::AcqRel);
        Some(parked)
    }

    /// Claim one of `capacity` parked-or-starting slots.
    fn reserve_slot(&self) -> bool {
        let mut reserved = self.reserved.load(Ordering::Acquire);
        while reserved < self.capacity {
            match self.reserved.compare_exchange_weak(
                reserved,
                reserved + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(current) => reserved = current,
            }
        }
        false
    }

    /// Start threads until `capacity` are parked or starting.
    fn refill(&self) {
        while self.reserve_slot() {
            let (sender, receiver) = flume::bounded::<Guest>(1);
            let heap_limit_mb = self.heap_limit_mb;
            // AGENTOS_THREAD_SITE: bounded-v8-wasm-warm-guest
            let spawned = std::thread::Builder::new()
                .name(String::from("agentos-v8-wasm-warm"))
                .spawn(move || {
                    let isolate = create_isolate(Some(heap_limit_mb));
                    // A dropped sender means the process no longer wants it.
                    if let Ok(guest) = receiver.recv() {
                        guest(isolate);
                    }
                });
            match spawned {
                Ok(_) => match self.parked.lock() {
                    Ok(mut parked) => parked.push(sender),
                    Err(_) => {
                        self.reserved.fetch_sub(1, Ordering::AcqRel);
                        eprintln!("ERR_AGENTOS_V8_WASM_WARM_POOL_POISONED: warm isolate pool lock is poisoned");
                        return;
                    }
                },
                Err(error) => {
                    self.reserved.fetch_sub(1, Ordering::AcqRel);
                    eprintln!("ERR_AGENTOS_V8_WASM_WARM_SPAWN: could not start a warm isolate thread: {error}");
                    return;
                }
            }
        }
    }
}
