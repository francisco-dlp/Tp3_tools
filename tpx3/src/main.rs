use timepix3::errorlib::Tp3ErrorKind;
use timepix3::auxiliar::*;
use timepix3::tdclib::*;
use timepix3::{speclib, spimlib, spimlib::SpimKind};


fn connect_and_loop() -> Result<u8, Tp3ErrorKind> {
    
    let (my_settings, mut pack, ns) = Settings::create_settings([192, 168, 199, 11], 8088)?;

    match my_settings.mode {
        0 if my_settings.bin => {
            let frame_tdc = PeriodicTdcRef::new(TdcType::TdcOneRisingEdge, &mut pack, None)?;
            let np_tdc = NonPeriodicTdcRef::new(TdcType::TdcTwoRisingEdge, &mut pack, None)?;
            speclib::run_spectrum(pack, ns, my_settings, frame_tdc, np_tdc, speclib::Live1D)?;
            Ok(my_settings.mode)
        },
        0 if !my_settings.bin => {
            let frame_tdc = PeriodicTdcRef::new(TdcType::TdcOneRisingEdge, &mut pack, None)?;
            let np_tdc = NonPeriodicTdcRef::new(TdcType::TdcTwoRisingEdge, &mut pack, None)?;
            speclib::run_spectrum(pack, ns, my_settings, frame_tdc, np_tdc, speclib::Live2D)?;
            Ok(my_settings.mode)
        },
        1 if my_settings.bin => {
            let frame_tdc = PeriodicTdcRef::new(TdcType::TdcOneRisingEdge, &mut pack, None)?;
            let laser_tdc = SingleTriggerPeriodicTdcRef::new(TdcType::TdcTwoRisingEdge, &mut pack, None)?;
            speclib::run_spectrum(pack, ns, my_settings, frame_tdc, laser_tdc, speclib::LiveTR1D)?;
            Ok(my_settings.mode)
        },
        1 if !my_settings.bin => {
            let frame_tdc = PeriodicTdcRef::new(TdcType::TdcOneRisingEdge, &mut pack, None)?;
            let laser_tdc = SingleTriggerPeriodicTdcRef::new(TdcType::TdcTwoRisingEdge, &mut pack, None)?;
            speclib::run_spectrum(pack, ns, my_settings, frame_tdc, laser_tdc, speclib::LiveTR2D)?;
            Ok(my_settings.mode)
        },
        2 => {
            let spim_tdc = PeriodicTdcRef::new(TdcType::TdcOneFallingEdge, &mut pack, Some(my_settings.yspim_size))?;
            let np_tdc = NonPeriodicTdcRef::new(TdcType::TdcTwoRisingEdge, &mut pack, None)?;
            let measurement = spimlib::Live::new();
            spimlib::build_spim(pack, ns, my_settings, spim_tdc, np_tdc, measurement)?;
            Ok(my_settings.mode)
        },
        6 => {
            let frame_tdc = PeriodicTdcRef::new(TdcType::TdcOneRisingEdge, &mut pack, None)?;
            let np_tdc = NonPeriodicTdcRef::new(TdcType::TdcTwoRisingEdge, &mut pack, None)?;
            speclib::run_spectrum(pack, ns, my_settings, frame_tdc, np_tdc, speclib::FastChrono)?;
            Ok(my_settings.mode)
        },
        7 => {
            let frame_tdc = PeriodicTdcRef::new(TdcType::TdcOneRisingEdge, &mut pack, None)?;
            let np_tdc = NonPeriodicTdcRef::new(TdcType::TdcTwoRisingEdge, &mut pack, None)?;
            speclib::run_spectrum(pack, ns, my_settings, frame_tdc, np_tdc, speclib::Chrono)?;
            Ok(my_settings.mode)
        },
        _ => Err(Tp3ErrorKind::MiscModeNotImplemented(my_settings.mode)),
    }
}

fn main() {
    let mut log_file = simple_log::start().unwrap();
    loop {
        match connect_and_loop() {
            Ok(val) => {
                simple_log::ok(&mut log_file, val).unwrap();
            },
            Err(e) => {
                println!("Error in measurement. Error message: {:?}.", e);
                simple_log::error(&mut log_file, e).unwrap();
            },
        }
    }
}
