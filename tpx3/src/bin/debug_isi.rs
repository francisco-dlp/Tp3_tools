use timepix3::tdclib::isi_box;
use std::{thread, time};

fn main() {
    loop {
        println!("Starting new debug session...");
        let mut handler = isi_box::IsiBoxHandler::new(17);
        handler.bind_and_connect();
        handler.configure_scan_parameters(32, 32, 8334);
        handler.configure_measurement_type(false);
        let time = time::Duration::from_millis(100);
        //handler.start_index_threads();
        handler.start_counter_threads();
        for _ in 0..5 {
            thread::sleep(time);
            handler.send_counter_to_external_socket();
        }
    }
}
