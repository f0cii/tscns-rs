mod tscns;

pub use tscns::{
    read_nanos, read_tsc, calibrate, init, get_tsc_ghz, INIT_CALIBRATE_NANOS, CALIBRATE_INTERVAL_NANOS,
};

