//use std::intrinsics::{unchecked_add, unchecked_sub};
use std::ptr::{addr_of, addr_of_mut};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

/// [`NS_PER_SEC`]  The number of nanoseconds in each second is equal to one billion nanoseconds.
const NS_PER_SEC: i64 = 1_000_000_000;

/// [`INIT_CALIBRATE_NANOS`] The default initial calibration sampling duration is 300 milliseconds.
pub const INIT_CALIBRATE_NANOS: i64 = 300000000;

/// [`CALIBRATE_INTERVAL_NANOS`] The default clock calibration period is 3 seconds.
pub const CALIBRATE_INTERVAL_NANOS: i64 = 3 * NS_PER_SEC;

#[inline(always)]
fn unchecked_add<T: std::ops::Add<Output = T> + Copy>(a: T, b: T) -> T {
    unsafe { std::mem::transmute::<T, T>(a + b) }
}

#[inline(always)]
fn unchecked_sub<T: std::ops::Sub<Output = T> + Copy>(a: T, b: T) -> T {
    unsafe { std::mem::transmute::<T, T>(a - b) }
}

/// [`PARAM_SEQ`] Global optimistic lock, used to detect whether global parameters have changed or whether global state (such as BASE_NS, BASE_TSC, NS_PER_TSC) has been modified by other threads during the calculation process.
#[repr(align(64))]
struct Sequence(AtomicUsize);

impl Sequence {
    pub(crate) const fn new() -> Self {
        Self(AtomicUsize::new(0))
    }
    pub(crate) fn read(&self, order: Ordering) -> usize {
        self.0.load(order)
    }

    pub(crate) fn write(&self, val: usize, order: Ordering) {
        self.0.store(val, order);
    }
}

static mut PARAM_SEQ: Sequence = const { Sequence::new() };

/// [`NS_PER_TSC`] Indicates the number of nanoseconds per clock cycle.
static mut NS_PER_TSC: f64 = 0.0;

/// [`BASE_TSC`] Benchmark TSC timestamp, used to calculate relative time.
static mut BASE_TSC: i64 = 0;

/// [`BASE_NS`] Benchmark nanosecond error, used to reduce the error between TSC timestamp and nanosecond timestamp conversion.
static mut BASE_NS: i64 = 0;

/// [`CALIBATE_INTERVAL_NS`] Calibrate Clock Cycle
static mut CALIBATE_INTERVAL_NS: i64 = 0;

/// [`BASE_NS_ERR`] Benchmark nanosecond error, used to reduce the error between TSC timestamp and nanosecond timestamp conversion.
static mut BASE_NS_ERR: i64 = 0;

/// [`NEXT_CALIBRATE_TSC`]  The TSC timestamp for the next clock calibration is used to determine whether clock calibration is necessary.
static mut NEXT_CALIBRATE_TSC: i64 = 0;

/// # Examples
/// ```
/// tscns::init(tscns::INIT_CALIBRATE_NANOS, tscns::CALIBRATE_INTERVAL_NANOS);
/// ```
pub fn init(init_calibrate_ns: i64, calibrate_interval_ns: i64) {
    unsafe {
        addr_of_mut!(CALIBATE_INTERVAL_NS).write_volatile(calibrate_interval_ns);
        let (base_tsc, base_ns) = sync_time();
        let expire_ns = unchecked_add(base_ns, init_calibrate_ns);
        while read_sys_nanos() < expire_ns {
            // Spin wait until the current system time exceeds the end time of the calibration period.
            std::thread::yield_now();
        }

        let (delayed_tsc, delayed_ns) = sync_time();
        // Calculate the number of nanoseconds for each clock cycle initially,
        // dividing the difference between two nanosecond timestamps by the difference between two TSC timestamps
        // can more accurately represent the number of nanoseconds per tick of the TSC.
        let init_ns_per_tsc =
            unchecked_sub(delayed_ns, base_ns) as f64 / unchecked_sub(delayed_tsc, base_tsc) as f64;
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
pub fn read_nanos() -> i64 {
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
    if read_tsc() < (unsafe { addr_of!(NEXT_CALIBRATE_TSC).read_volatile() }) {
        // The current time should be beyond the next calibration time.
        return;
    }
    let (tsc, ns) = sync_time();
    let calculated_ns = tsc2ns(tsc);
    // Calculate the error in converting the current TSC timestamp to a nanosecond timestamp.
    // If `ns_err` is a negative value, it indicates that the time converted by TSC is "slower" than the actual system time.
    // When `ns_err` is a negative value, it will cause NS_PER_TSC to increase. This means that we need to increase the number of
    // nanoseconds corresponding to each TSC cycle to catch up with the actual system time.
    unsafe {
        let ns_err = unchecked_sub(calculated_ns, ns);
        let expected_err_at_next_calibration = ns_err
            + (unchecked_sub(ns_err, addr_of!(BASE_NS_ERR).read_volatile()))
                * addr_of!(CALIBATE_INTERVAL_NS).read_volatile()
                / (unchecked_add(
                    unchecked_sub(ns, addr_of!(BASE_NS).read_volatile()),
                    addr_of!(BASE_NS_ERR).read_volatile(),
                ));

        let new_ns_per_tsc = addr_of!(NS_PER_TSC).read_volatile()
            * (1.0
                - (expected_err_at_next_calibration as f64)
                    / addr_of!(CALIBATE_INTERVAL_NS).read_volatile() as f64); // Calculate the number of nanoseconds for each new clock cycle.
        save_param(tsc, calculated_ns, ns, new_ns_per_tsc);
    }
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
    1.0 / unsafe { addr_of!(NS_PER_TSC).read_volatile() }
}

/// Convert tsc timestamp to nanosecond timestamp
pub fn tsc2ns(tsc: i64) -> i64 {
    loop {
        let before_seq = unsafe { (&*addr_of_mut!(PARAM_SEQ)).read(Ordering::Acquire) & !1 };
        std::sync::atomic::fence(Ordering::AcqRel);
        // Calculate the TSC interval from the baseline time to the current time point and convert it into nanoseconds.
        // Add the initial baseline nanoseconds to the interval nanoseconds to obtain the current nanoseconds.
        let ns = unsafe {
            // BASE_NS + ((tsc - BASE_TSC) as f64 * NS_PER_TSC) as i64
            let diff_tsc = unchecked_sub(tsc, addr_of!(BASE_TSC).read_volatile()) as f64;
            // the rust bug Cannot MulUnchecked non-integer type f64
            // let diff_ns = unchecked_mul(diff_tsc, NS_PER_TSC) as i64;
            let diff_ns = (diff_tsc * addr_of!(NS_PER_TSC).read_volatile()) as i64;
            unchecked_add(addr_of!(BASE_NS).read_volatile(), diff_ns)
        };
        std::sync::atomic::fence(Ordering::AcqRel);
        let after_seq = unsafe { (&*addr_of_mut!(PARAM_SEQ)).read(Ordering::Acquire) };
        if before_seq == after_seq {
            return ns;
        }
    }
}

/// Get the current system nanosecond timestamp.
fn read_sys_nanos() -> i64 {
    let now = SystemTime::now();
    let result = now.duration_since(SystemTime::UNIX_EPOCH);
    match result {
        Ok(duration) => duration.as_nanos() as i64,
        Err(_) => 0,
    }
}

/// Update static global variables inside the module
fn save_param(base_tsc: i64, base_ns: i64, sys_ns: i64, new_ns_per_tsc: f64) {
    unsafe {
        addr_of_mut!(BASE_NS).write_volatile(unchecked_sub(base_ns, sys_ns)); // Compute benchmark nanosecond error.

        // base_tsc + ((CALIBATE_INTERVAL_NS - 1000) as f64 / new_ns_per_tsc) as i64;
        addr_of_mut!(NEXT_CALIBRATE_TSC).write_volatile(unchecked_add(
            base_tsc,
            (unchecked_sub(addr_of!(CALIBATE_INTERVAL_NS).read_volatile(), 1000) as f64
                / new_ns_per_tsc) as i64,
        ));

        let seq = (&*addr_of!(PARAM_SEQ)).read(Ordering::Relaxed);
        (&mut *addr_of_mut!(PARAM_SEQ)).write(unchecked_add(seq, 1), Ordering::Release);

        std::sync::atomic::fence(Ordering::AcqRel); // Atomic barrier separation ensures that all read and write operations executed before the atomic barrier are completed.
        addr_of_mut!(BASE_TSC).write_volatile(base_tsc);
        addr_of_mut!(BASE_NS).write_volatile(base_ns);
        addr_of_mut!(NS_PER_TSC).write_volatile(new_ns_per_tsc);
        std::sync::atomic::fence(Ordering::AcqRel);
        (&mut *addr_of_mut!(PARAM_SEQ)).write(unchecked_add(seq, 2), Ordering::Release);
    }
}

/// Internal function to synchronize the tsc and system time
fn sync_time() -> (i64, i64) {
    const N: usize = if cfg!(windows) { 15 } else { 3 };

    let mut tsc: [i64; N + 1] = [0; N + 1];
    let mut ns: [i64; N + 1] = [0; N + 1];

    tsc[0] = read_tsc();
    for i in 1..=N {
        // Get Sampling
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
pub fn read_tsc() -> i64 {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        std::arch::x86_64::_rdtsc() as i64
    }
    #[cfg(target_arch = "x86")]
    unsafe {
        std::arch::x86::_rdtsc() as i64
    }

    #[cfg(target_arch = "aarch64")]
    {
        let tsc: i64;
        unsafe {
            std::arch::asm!("mrs {}, cntvct_el0", out(reg) tsc);
        }
        tsc
    }

    #[cfg(target_arch = "riscv64")]
    {
        let tsc: i64;
        unsafe {
            asm!("rdtime {}", out(reg) tsc);
        }
        tsc
    }

    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")))]
    read_sys_nanos()
}
