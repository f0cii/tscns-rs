extern crate crossbeam_utils;

use std::ptr::addr_of_mut;
use crossbeam_utils::CachePadded;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime};

/// [`NS_PER_SEC`] The number of nanoseconds per second, which is equal to one billion nanoseconds per second.
const NS_PER_SEC: u64 = 1_000_000_000;

/// [`INIT_CALIBRATE_NANOS`] Default initial calibration sampling time interval is 20ms
pub const INIT_CALIBRATE_NANOS: u64 = 20000000;

/// [`CALIBRATE_INTERVAL_NANOS`] Default clock calibration period 3s
pub const CALIBRATE_INTERVAL_NANOS: u64 = 3 * NS_PER_SEC;

/// [`PARAM_SEQ`] The global optimistic lock is used to detect whether global parameters have changed or whether the global state (such as BASE_NS, BASE_TSC, NS_PER_TSC) has been modified by other threads during the calculation process.
static mut PARAM_SEQ: CachePadded<AtomicUsize> = CachePadded::new(AtomicUsize::new(0));

/// [`NS_PER_TSC`] Represents the number of nanoseconds for each clock cycle (Time Stamp Counter), that is, the number of nanoseconds per clock cycle.
static mut NS_PER_TSC: f64 = 0.0;

/// [`BASE_TSC`] Baseline TSC timestamp, ((current TSC timestamp - BASE_TSC) NS_PER_TSC) is the number of nanoseconds per clock cycle.
static mut BASE_TSC: u64 = 0;

/// [`BASE_NS`] Baseline nanosecond
static mut BASE_NS: u64 = 0;

/// [`CALIBATE_INTERVAL_NS`] Calibrate Clock Cycle
static mut CALIBATE_INTERVAL_NS: u64 = 0;

/// [`BASE_NS_ERR`] Benchmark nanosecond error, used to reduce the error between TSC timestamp and nanosecond timestamp conversion.
static mut BASE_NS_ERR: u64 = 0;

/// [`NEXT_CALIBRATE_TSC`] In the next clock cycle, used to determine whether a clock calibration is necessary.
static mut NEXT_CALIBRATE_TSC: u64 = 0;


/// # Examples
/// ```
/// tscns::init(tscns::INIT_CALIBRATE_NANOS, tscns::CALIBRATE_INTERVAL_NANOS);
/// ```
pub fn init(init_calibrate_ns: u64, calibrate_interval_ns: u64) {
    unsafe {
        *addr_of_mut!(CALIBATE_INTERVAL_NS) = calibrate_interval_ns;

        let (base_tsc, base_ns) = sync_time();
        let expire_ns = base_ns + init_calibrate_ns;
        while read_sys_nanos() < expire_ns {
            // Spin wait until the current system time exceeds the end time of the calibration period.
            std::thread::yield_now();
        }

        let (delayed_tsc, delayed_ns) = sync_time();
        // Calculate the number of nanoseconds for each clock cycle initially,
        // dividing the difference between two nanosecond timestamps by the difference between two TSC timestamps
        // can more accurately represent the number of nanoseconds per tick of the TSC.
        let init_ns_per_tsc = (delayed_ns - base_ns) as f64 / (delayed_tsc - base_tsc) as f64;
        save_param(base_tsc, base_ns, base_ns, init_ns_per_tsc);
    }
}

/// # Examples
/// ```
/// tscns::init(tscns::INIT_CALIBRATE_NANOS, tscns::CALIBRATE_INTERVAL_NANOS);
/// tscns::calibrate();
/// let ns = tscns::read_nanos();
/// println!("now ns: {}", ns);
/// ```
#[inline(always)]
pub fn read_nanos() -> u64 {
    tsc2ns(read_tsc())
}


/// # Examples
/// ```
/// use std::thread;
/// use std::sync::atomic::{AtomicBool,Ordering};
/// let running = AtomicBool::new(true);
/// thread::spawn(move || {
///   while running.load(Ordering::Acquire) {
///     tscns::calibrate();
///     thread::sleep(std::time::Duration::from_secs(3));
///   }
/// });
/// ```
pub fn calibrate() {
    if read_tsc() < (unsafe { NEXT_CALIBRATE_TSC })
    {
        // The current time should be beyond the next calibration time.
        return;
    }
    let (tsc, ns) = sync_time();
    let calculated_ns = tsc2ns(tsc);
    let ns_err = calculated_ns.checked_sub(ns).unwrap_or_else(|| 0);
    // let ns_err = (calculated_ns - ns);    // Calculate the error in converting the current TSC timestamp to a nanosecond timestamp.
    let expected_err_at_next_calibration = ns_err + (ns_err - unsafe { BASE_NS_ERR }) * unsafe { CALIBATE_INTERVAL_NS } / (ns - unsafe { BASE_NS } + unsafe { BASE_NS_ERR });
    let new_ns_per_tsc = unsafe { NS_PER_TSC } * (1.0 - (expected_err_at_next_calibration as f64) / unsafe { CALIBATE_INTERVAL_NS } as f64);    // Calculate the number of nanoseconds for each new clock cycle.
    save_param(tsc, calculated_ns, ns, new_ns_per_tsc);
}

/// Used to obtain the current CPU frequency in GHz units.
/// # Examples
/// ```
/// tscns::init(tscns::INIT_CALIBRATE_NANOS, tscns::CALIBRATE_INTERVAL_NANOS);
/// tscns::calibrate();
/// let ghz = tscns::get_tsc_ghz();
/// println!("cpu {}GHz", ghz);
/// ```
pub fn get_tsc_ghz() -> f64 {
    1.0 / unsafe { NS_PER_TSC }
}


/// Convert tsc timestamp to nanosecond timestamp
fn tsc2ns(tsc: u64) -> u64 {
    loop {
        let before_seq = unsafe {
            let param_seq_ref = &*addr_of_mut!(PARAM_SEQ);
            param_seq_ref.load(Ordering::Acquire) & !1
        };
        std::sync::atomic::fence(Ordering::AcqRel);
        // Calculate the TSC interval from the baseline time to the current time point and convert it into nanoseconds.
        // Add the initial baseline nanoseconds to the interval nanoseconds to obtain the current nanoseconds.
        let ns = unsafe { BASE_NS } + ((tsc - unsafe { BASE_TSC }) as f64 * unsafe { NS_PER_TSC }) as u64;
        std::sync::atomic::fence(Ordering::AcqRel);
        let after_seq = unsafe {
            let param_seq_ref = &*addr_of_mut!(PARAM_SEQ);
            param_seq_ref.load(Ordering::Acquire)
        };
        if before_seq == after_seq {
            return ns;
        }
    }
}

/// Get the current system nanosecond timestamp.
fn read_sys_nanos() -> u64 {
    let now = SystemTime::now();
    let result = now.duration_since(SystemTime::UNIX_EPOCH);
    match result {
        Ok(duration) => duration.as_nanos() as u64,
        Err(_) => 0,
    }
}

/// Update static global variables inside the module
fn save_param(
    base_tsc: u64,
    base_ns: u64,
    sys_ns: u64,
    new_ns_per_tsc: f64,
) {
    unsafe {
        *addr_of_mut!(BASE_NS) = base_ns.checked_sub(sys_ns).unwrap_or_else(|| 0);
        // *addr_of_mut!(BASE_NS) = base_ns - sys_ns; // Error in calculating benchmark nanoseconds.
        *addr_of_mut!(NEXT_CALIBRATE_TSC) = base_tsc + ((CALIBATE_INTERVAL_NS - 1000) as f64 / new_ns_per_tsc) as u64; // Calculate the clock cycle for the next calibration.
        let param_seq_ref = &*addr_of_mut!(PARAM_SEQ);
        let seq = param_seq_ref.load(Ordering::Relaxed);
        let param_seq = &mut *addr_of_mut!(PARAM_SEQ);
        param_seq.store(seq + 1, Ordering::Release);

        std::sync::atomic::fence(Ordering::AcqRel); // Atomic barrier separation ensures that all read and write operations executed before the atomic barrier are completed.
        *addr_of_mut!(BASE_TSC) = base_tsc;
        *addr_of_mut!(BASE_NS) = base_ns;
        *addr_of_mut!(NS_PER_TSC) = new_ns_per_tsc;
        std::sync::atomic::fence(Ordering::AcqRel);

        let param_seq_ref = &mut *addr_of_mut!(PARAM_SEQ);
        param_seq_ref.store(seq + 2, Ordering::Release);
    }
}

/// Internal function to synchronize the tsc and system time
fn sync_time() -> (u64, u64) {
    const N: usize = if cfg!(windows) { 15 } else { 3 };

    let mut tsc: [u64; N + 1] = [0; N + 1];
    let mut ns: [u64; N + 1] = [0; N + 1];

    tsc[0] = read_tsc();
    for i in 1..=N {    // Get Sampling
        ns[i] = read_sys_nanos();
        tsc[i] = read_tsc();
    }

    let j: usize;
    // If it is a Windows system, continuous identical timestamps in the sample data will be removed here to reduce errors.
    #[cfg(windows)]
    {
        j = 1;
        for i in 2..=N {
            if ns[i] == ns[i - 1] {
                continue;
            }
            tsc[j - 1] = tsc[i - 1];
            ns[j] = ns[i];
            j += 1;
        }
        j -= 1;
    }
    #[cfg(not(windows))]
    {
        j = N + 1;
    }

    let mut best = 1;
    for i in 2..j {
        if tsc[i] - tsc[i - 1] < tsc[best] - tsc[best - 1] {
            best = i;
        }
    }
    let tsc_out = (tsc[best] + tsc[best - 1]) >> 1;
    let ns_out = ns[best];
    (tsc_out, ns_out)
}

/// Read tsc count, support x86_64 and aarch64 architecture cpu
#[inline(always)]
fn read_tsc() -> u64 {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        std::arch::x86_64::_rdtsc()
    }
    #[cfg(target_arch = "x86")]
    unsafe {
        std::arch::x86::_rdtsc() as u64
    }

    #[cfg(target_arch = "aarch64")]
    {
        let tsc: u64;
        unsafe {
            std::arch::asm!("mrs {}, cntvct_el0", out(reg) tsc);
        }
        tsc
    }

    #[cfg(target_arch = "riscv64")]
    {
        let tsc: u64;
        unsafe {
            asm!("rdtime {}", out(reg) tsc);
        }
        tsc
    }

    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
    read_sys_nanos()
}