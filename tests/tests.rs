use std::thread;

#[test]
fn it_works() {
    tscns::init(tscns::INIT_CALIBRATE_NANOS, tscns::CALIBRATE_INTERVAL_NANOS);
    thread::spawn(|| {
        loop {
            tscns::calibrate();
            thread::sleep(std::time::Duration::from_secs(1));
        }
    });


    let mut count = 100;

    while count > 0 {
        let now = tscns::read_nanos();
        println!("ns: {}", now);
        thread::sleep(std::time::Duration::from_micros(100));
        count -= 1;
    }

    println!("cpu {}GHz", tscns::get_tsc_ghz());
}