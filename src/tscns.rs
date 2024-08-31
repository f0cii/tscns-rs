extern crate crossbeam_utils;

use std::ptr::addr_of_mut;
use crossbeam_utils::CachePadded;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime};

/// [`NS_PER_SEC`] 全局常量定义了每秒的纳秒数, 即一秒等于10亿纳秒
const NS_PER_SEC: u64 = 1_000_000_000;

/// [`INIT_CALIBRATE_NANOS`] 默认首次校准采样时间间隔 20ms
pub const INIT_CALIBRATE_NANOS: u64 = 20000000;

/// [`CALIBRATE_INTERVAL_NANOS`] 默认时钟校准周期 3s
pub const CALIBRATE_INTERVAL_NANOS: u64 = 3 * NS_PER_SEC;

/// [`PARAM_SEQ`] 全局乐观锁用于检测计算过程中全局参数是否发生变化, 全局状态（如BASE_NS，BASE_TSC，NS_PER_TSC）是否被其他线程修改
static mut PARAM_SEQ: CachePadded<AtomicUsize> = CachePadded::new(AtomicUsize::new(0));

/// [`NS_PER_TSC`] 表示每个时钟周期(Time Stamp Counter)的纳秒数, 即时钟周期的纳秒数
static mut NS_PER_TSC: f64 = 0.0;

/// [`BASE_TSC`] 基准TSC时间点, ((当前TSC时间点 - BASE_TSC) * NS_PER_TSC) 即是每个时钟周期的纳秒数
static mut BASE_TSC: u64 = 0;

/// [`BASE_NS`] 基准纳秒
static mut BASE_NS: u64 = 0;

/// [`CALIBATE_INTERVAL_NS`] 校准时钟周期
static mut CALIBATE_INTERVAL_NS: u64 = 0;

/// [`BASE_NS_ERR`] 基准纳秒误差, 用于减小TSC时间戳转换成纳秒时间戳的误差
static mut BASE_NS_ERR: u64 = 0;

/// [`NEXT_CALIBRATE_TSC`] 下一个时钟周期, 用于判断是否需要进行时钟校准
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
        while read_sys_nanos() < expire_ns {  // 自旋等待，直到当前系统时间超过校准周期结束时间
            std::thread::yield_now();
        }

        let (delayed_tsc, delayed_ns) = sync_time();
        // 计算初始每个时钟周期的纳秒数, 将两个纳秒时间戳的差值除以两个TSC时间戳的差值，可以更准确地表示TSC每个tick的纳秒数。
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
    { // 当前时间应该超过下一次校准时间
        return;
    }
    let (tsc, ns) = sync_time();
    let calculated_ns = tsc2ns(tsc);
    let ns_err = calculated_ns.checked_sub(ns).unwrap_or_else(|| 0);
    // let ns_err = (calculated_ns - ns);    // 计算当前tsc时间戳转换成纳秒时间戳的误差
    let expected_err_at_next_calibration = ns_err + (ns_err - unsafe { BASE_NS_ERR }) * unsafe { CALIBATE_INTERVAL_NS } / (ns - unsafe { BASE_NS } + unsafe { BASE_NS_ERR });
    let new_ns_per_tsc = unsafe { NS_PER_TSC } * (1.0 - (expected_err_at_next_calibration as f64) / unsafe { CALIBATE_INTERVAL_NS } as f64);    // 计算新的每个时钟周期的纳秒数
    save_param(tsc, calculated_ns, ns, new_ns_per_tsc);
}

/// 用于获取当前cpu频率GHz为单位的
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


/// 将tsc时间戳转换成纳秒时间戳
fn tsc2ns(tsc: u64) -> u64 {
    loop {
        let before_seq = unsafe {
            let param_seq_ref = &*addr_of_mut!(PARAM_SEQ);
            param_seq_ref.load(Ordering::Acquire) & !1
        };
        std::sync::atomic::fence(Ordering::AcqRel);
        // 计算从基准时间点到当前时间点的TSC间隔然后转换成纳秒数， 初始基准纳秒+间隔纳秒数=当前纳秒数
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

/// 获取当前系统纳秒时间戳
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
        // *addr_of_mut!(BASE_NS) = base_ns - sys_ns; // 计算基准纳秒数的误差
        *addr_of_mut!(NEXT_CALIBRATE_TSC) = base_tsc + ((CALIBATE_INTERVAL_NS - 1000) as f64 / new_ns_per_tsc) as u64; // 计算下一次校准的时钟周期
        let param_seq_ref = &*addr_of_mut!(PARAM_SEQ);
        let seq = param_seq_ref.load(Ordering::Relaxed);
        let param_seq = &mut *addr_of_mut!(PARAM_SEQ);
        param_seq.store(seq + 1, Ordering::Release);

        std::sync::atomic::fence(Ordering::AcqRel); //原子屏障分隔、确保在该原子屏障之前执行的读写操作都被完成。
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
    for i in 1..=N {    // 获取采样
        ns[i] = read_sys_nanos();
        tsc[i] = read_tsc();
    }

    let j: usize;
    // 如果是Windows系统,这里会去除样本数据中连续相同的时间戳以减小误差
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