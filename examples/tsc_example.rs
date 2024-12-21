use std::time::Duration;
use tscns::{CALIBRATE_INTERVAL_NANOS, INIT_CALIBRATE_NANOS};

pub struct TscTime;

impl TscTime {
    pub fn now(&self) -> i64 {
        tscns::read_nanos()
    }
}

impl Default for TscTime {
    fn default() -> Self {
        tscns::init(INIT_CALIBRATE_NANOS, CALIBRATE_INTERVAL_NANOS);
        Self {}
    }
}

// impl FormatTime for TscTime {
//     fn format_time(&self, w: &mut Writer<'_>) -> std::fmt::Result {
//         let micros = (self.now() as f64 * 1e-3) as i64;
//         let us = micros as f64 % 1000000_f64;
//         let sys_time = std::time::UNIX_EPOCH + Duration::from_micros(micros as u64);
//         let datetime = chrono::DateTime::<chrono::Local>::from(sys_time); // 将 SystemTime 转换为 DateTime<Local> 类型
//         write!(
//             w,
//             "{}",
//             format!(
//                 "{}.{:0>6}",
//                 datetime.format("%Y-%m-%d %H:%M:%S").to_string(),
//                 us
//             )
//         )
//     }
// }

fn main() {
    tscns::init(tscns::INIT_CALIBRATE_NANOS, tscns::CALIBRATE_INTERVAL_NANOS);

    std::thread::spawn(move || {
        tscns::calibrate();
        std::thread::sleep(Duration::from_nanos(tscns::CALIBRATE_INTERVAL_NANOS as u64));
    });

    let micros = (tscns::read_nanos() as f64 * 1e-3) as i64;
    let us = micros as f64 % 1000000_f64;
    let sys_time = std::time::UNIX_EPOCH + Duration::from_micros(micros as u64);
    let datetime = chrono::DateTime::<chrono::Local>::from(sys_time);

    println!(
        "{}",
        format!(
            "{}.{:0>6}",
            datetime.format("%Y-%m-%d %H:%M:%S").to_string(),
            us
        )
    );

    // output: 2024-04-18 11:34:23.004812

    // for _ in 0..10 {
    //     let _ns = tscns::read_nanos();
    //     println!("now ns: {}", _ns);
    // }

    println!("cpu {} GHz", tscns::get_tsc_ghz());
}
