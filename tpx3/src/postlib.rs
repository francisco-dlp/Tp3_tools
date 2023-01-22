pub mod coincidence {

    use std::fs::OpenOptions;
    use crate::spimlib::SPIM_PIXELS;
    use crate::packetlib::{Packet, TimeCorrectedPacketEELS as Pack, packet_change};
    use crate::tdclib::{TdcControl, TdcType, PeriodicTdcRef, NonPeriodicTdcRef};
    use crate::postlib::isi_box;
    use crate::errorlib::Tp3ErrorKind;
    use crate::clusterlib::cluster::ClusterCorrection;
    use std::io;
    use std::io::prelude::*;
    use std::fs;
    use std::convert::TryInto;
    use crate::clusterlib::cluster::{SingleElectron, CollectionElectron};
    use crate::auxiliar::ConfigAcquisition;
    use crate::auxiliar::value_types::*;
    use indicatif::{ProgressBar, ProgressStyle};

    const BUFFER_SIZE: usize = 512_000_000; //Buffer size reading files when using TP3 and IsiBox
    const ISI_BUFFER_SIZE: usize = 512_000_000; //Buffer size reading files when using TP3 and IsiBox
    const ISI_TP3_MAX_DIF: u64 = 1_000; //In units of 640 Mhz;
    const PHOTON_LIST_STEP: usize = 10;
    
    fn as_bytes<T>(v: &[T]) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                v.as_ptr() as *const u8,
                v.len() * std::mem::size_of::<T>())
        }
    }

    fn output_data<T>(data: &[T], name: &str) {
        let mut tfile = OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(name).unwrap();
        tfile.write_all(as_bytes(data)).unwrap();
        println!("Outputting data under {:?} name. Vector len is {}", name, data.len());
    }

    //Non-standard data types 
    pub struct ElectronData<T> {
        time: Vec<TIME>,
        channel: Vec<u8>,
        rel_time: Vec<i16>,
        //corrected_rel_time: Vec<i16>,
        //fully_corrected_rel_time: Vec<i16>,
        double_photon_rel_time: Vec<i16>,
        g2_time: Vec<Option<i16>>,
        x: Vec<u16>,
        y: Vec<u16>,
        tot: Vec<u16>,
        cluster_size: Vec<u16>,
        spectrum: Vec<usize>,
        corr_spectrum: Vec<usize>,
        is_spim: bool,
        spim_size: (POSITION, POSITION),
        spim_index: Vec<POSITION>,
        spim_tdc: Option<PeriodicTdcRef>,
        remove_clusters: T,
        overflow_electrons: COUNTER,
        file: String,
    }

    impl<T: ClusterCorrection> ElectronData<T> {
        fn add_electron(&mut self, val: SingleElectron) {
            self.spectrum[val.x() as usize] += 1;
        }

        fn add_spim_line(&mut self, pack: &Pack) {
            if let Some(spim_tdc) = &mut self.spim_tdc {
                spim_tdc.upt(pack.tdc_time_norm(), pack.tdc_counter());
            }
        }

        fn estimate_overflow(&self, pack: &Pack) -> Option<TIME> {
            if let Some(spim_tdc) = self.spim_tdc {
                let val = spim_tdc.estimate_time();
                if val > pack.tdc_time() + Pack::electron_overflow() {
                    return Some(val / Pack::tdc_overflow());
                }
                else {return Some(0)}
            }
            None
        }


        fn add_coincident_electron(&mut self, val: SingleElectron, photon: (TIME, COUNTER, Option<i16>)) {
            self.corr_spectrum[val.x() as usize] += 1; //Adding the electron
            self.corr_spectrum[SPIM_PIXELS as usize-1] += 1; //Adding the photon
            self.time.push(val.time());
            self.g2_time.push(photon.2);
            self.tot.push(val.tot());
            self.x.push(val.x().try_into().unwrap());
            self.y.push(val.y().try_into().unwrap());
            self.channel.push(photon.1.try_into().unwrap());
            self.rel_time.push(val.relative_time_from_abs_tdc(photon.0).try_into().unwrap());
            //self.corrected_rel_time.push(val.corrected_relative_time_from_abs_tdc(photon.0).try_into().unwrap());
            //self.fully_corrected_rel_time.push(val.fully_corrected_relative_time_from_abs_tdc(photon.0).try_into().unwrap());
            self.cluster_size.push(val.cluster_size().try_into().unwrap());
            match val.get_or_not_spim_index(self.spim_tdc, self.spim_size.0, self.spim_size.1) {
                Some(index) => self.spim_index.push(index),
                None => self.spim_index.push(POSITION::MAX),
            }
            //if let Some(index) = val.get_or_not_spim_index(self.spim_tdc, self.spim_size.0, self.spim_size.1) {
            //    self.spim_index.push(index);
            //}
        }
        
        fn add_events(&mut self, mut temp_edata: TempElectronData, temp_tdc: &mut TempTdcData, time_delay: TIME, time_width: TIME) {
            let _ntotal = temp_tdc.tdc.len();
            let nphotons = temp_tdc.tdc.iter().
                filter(|(_time, channel, _dt)| *channel != 16 && *channel != 24).
                count();
            let mut min_index = temp_tdc.min_index;
            //println!("Total supplementary events: {}. Photons: {}. Minimum size of the array: {}.", ntotal, nphotons, min_index);

            match temp_tdc.tdc_type {
                TempTdcDataType::FromTP3 => {
                    temp_tdc.sort();
                    if temp_edata.electron.check_if_overflow() {self.overflow_electrons += 1;}
                },
                TempTdcDataType::FromIsiBox => {
                    if temp_edata.electron.correct_electron_time(self.overflow_electrons) {self.overflow_electrons += 1;}
                },
            }

            temp_edata.electron.sort();
            temp_edata.electron.try_clean(0, &self.remove_clusters);

            self.spectrum[SPIM_PIXELS as usize-1]=nphotons; //Adding photons to the last pixel

            //let mut photon_vec = temp_tdc.tdc.iter().filter(|ph| ph.1 != 16 && ph.1 != 24).collect::<Vec<_>>();
            
            let mut first_corr_photon = 0;
            for val in temp_edata.electron.values() {
                self.add_electron(*val);
                let mut photons_per_electron = 0;
                let mut index = 0;
                let mut index_to_increase = None;
                for ph in &temp_tdc.clean_tdc[min_index..] {
                    if ((ph.0/6) < val.time() + time_delay + time_width) && (val.time() + time_delay < (ph.0/6) + time_width) {
                        self.add_coincident_electron(*val, *ph);
                        if index_to_increase.is_none() {
                            index_to_increase = Some(index)
                        }
                        photons_per_electron += 1;
                        if photons_per_electron == 2 {
                            self.double_photon_rel_time.push(val.relative_time_from_abs_tdc(first_corr_photon).try_into().unwrap());
                            self.double_photon_rel_time.push(val.relative_time_from_abs_tdc(ph.0).try_into().unwrap());
                        }
                        first_corr_photon = ph.0;

                    }
                    if (ph.0/6) > val.time() + time_delay + 10_000 {break;}
                    index += 1;
                }
                if let Some(increase) = index_to_increase {
                    min_index += increase / PHOTON_LIST_STEP;
                }
            }
            temp_tdc.min_index = min_index;

            //println!("Number of coincident electrons: {:?}. Last photon real time is {:?}. Last relative time is {:?}.", self.x.len(), self.time.iter().last(), self.rel_time.iter().last());
        }

        fn prepare_spim(&mut self, spim_tdc: PeriodicTdcRef) {
            assert!(self.is_spim);
            self.spim_tdc = Some(spim_tdc);
        }

        pub fn new(my_config: ConfigAcquisition<T>) -> Self {
            Self {
                time: Vec::new(),
                channel: Vec::new(),
                rel_time: Vec::new(),
                //corrected_rel_time: Vec::new(),
                //fully_corrected_rel_time: Vec::new(),
                double_photon_rel_time: Vec::new(),
                g2_time: Vec::new(),
                x: Vec::new(),
                y: Vec::new(),
                tot: Vec::new(),
                cluster_size: Vec::new(),
                spectrum: vec![0; SPIM_PIXELS as usize],
                corr_spectrum: vec![0; SPIM_PIXELS as usize],
                is_spim: my_config.is_spim,
                spim_size: (my_config.xspim, my_config.yspim),
                spim_index: Vec::new(),
                spim_tdc: None,
                remove_clusters: my_config.correction_type,
                overflow_electrons: 0,
                file: my_config.file,
            }
        }
        
        pub fn output_corr_spectrum(&self, bin: bool) {
            let out: String = match bin {
                true => {
                    let mut spec: Vec<usize> = vec![0; SPIM_PIXELS as usize];
                    for val in self.corr_spectrum.chunks_exact(SPIM_PIXELS as usize) {
                        spec.iter_mut().zip(val.iter()).map(|(a, b)| *a += b).count();
                    }
                    spec.iter().map(|x| x.to_string()).collect::<Vec<String>>().join(", ")
                },
                false => {
                    self.corr_spectrum.iter().map(|x| x.to_string()).collect::<Vec<String>>().join(", ")
                },
            };
            fs::write("cspec.txt", out).unwrap();
        }
        
        pub fn output_spectrum(&self, bin: bool) {
            let out: String = match bin {
                true => {
                    let mut spec: Vec<usize> = vec![0; SPIM_PIXELS as usize];
                    for val in self.spectrum.chunks_exact(SPIM_PIXELS as usize) {
                        spec.iter_mut().zip(val.iter()).map(|(a, b)| *a += b).count();
                    }
                    spec.iter().map(|x| x.to_string()).collect::<Vec<String>>().join(", ")
                },
                false => {
                    self.spectrum.iter().map(|x| x.to_string()).collect::<Vec<String>>().join(", ")
                },
            };
            fs::write("spec.txt", out).unwrap();
        }

        pub fn output_relative_time(&self) {
            output_data(&self.rel_time, "tH.txt");
            output_data(&self.double_photon_rel_time, "double_tH.txt");
            /*
            output_data(&self.corrected_rel_time, "corrected_tH.txt");
            output_data(&self.fully_corrected_rel_time, "fully_corrected_tH.txt");
            */
        }
        
        pub fn output_time(&self) {
            output_data(&self.time, "tabsH.txt");
        }
        
        pub fn output_g2_time(&self) {
            let vec = self.g2_time.iter().map(|x| {
                match x {
                    None => -5_000,
                    Some(x) => *x,
                }
            }).collect::<Vec<i16>>();
            output_data(&vec, "g2tH.txt");
        }
        
        pub fn output_channel(&self) {
            output_data(&self.channel, "channel.txt");
        }
        
        pub fn output_dispersive(&self) {
            output_data(&self.x, "xH.txt");
        }
        
        pub fn output_non_dispersive(&self) {
            output_data(&self.y, "yH.txt");
        }
        
        pub fn output_spim_index(&self) {
            output_data(&self.spim_index, "si.txt");
            /*
            println!("Outputting each spim index value under si name. Vector len is {}", self.spim_index.len());
            let out: String = self.spim_index.iter().map(|x| x.to_string()).collect::<Vec<String>>().join(", ");
            fs::write("si.txt", out).unwrap();
            */
        }

        pub fn output_cluster_size(&self) {
            output_data(&self.cluster_size, "cs.txt");
            /*
            let out: String = self.cluster_size.iter().map(|x| x.to_string()).collect::<Vec<String>>().join(", ");
            fs::write("cs.txt", out).unwrap();
            */
        }

        pub fn output_tot(&self) {
            output_data(&self.tot, "tot.txt");
        }
            
    }

    enum TempTdcDataType {
        FromTP3,
        FromIsiBox,
    }

    pub struct TempTdcData {
        tdc: Vec<(TIME, COUNTER, Option<i16>)>, //The absolute time, the channel and the g2_dT
        clean_tdc: Vec<(TIME, COUNTER, Option<i16>)>,
        min_index: usize,
        tdc_type: TempTdcDataType,
    }

    impl TempTdcData {
        fn new() -> Self {
            Self {
                tdc: Vec::new(),
                clean_tdc: Vec::new(),
                min_index: 0,
                tdc_type: TempTdcDataType::FromTP3,
            }
        }

        fn new_from_isilist(list: isi_box::IsiList) -> Self {
            let vec_list = list.get_timelist_with_tp3_tick();
            Self {
                tdc: vec_list,
                clean_tdc: Vec::new(),
                min_index: 0,
                tdc_type: TempTdcDataType::FromIsiBox,
            }
        }

        fn get_vec_len(&self) -> usize {
            self.tdc.len()
        }

        fn correct_tdc(&mut self, val: &mut IsiBoxCorrectVector) {
            self.tdc.iter_mut().zip(val.0.iter_mut()).
                filter(|((_time, _channel, _dt), corr)| corr.is_some()).
                for_each(|((time, _channel, _dt), corr)| {
                *time += corr.unwrap();
                //*time = *time - (*time / (Pack::electron_overflow() * 6)) * (Pack::electron_overflow() * 6);
                *corr = Some(0);
            });
            //println!("{:?}", self.tdc.get(0..100));
        }
        
        pub fn get_sync(&self) -> Vec<(usize, TIME)> {
            self.tdc.iter().
                enumerate().
                filter(|(_index, (_time, channel, _dt))| *channel == 16).
                map(|(index, (time, _channel, _dt))| (index, *time)).
                collect::<Vec<_>>()
        }

        fn add_tdc(&mut self, my_pack: &Pack, channel: COUNTER) {
            self.tdc.push((my_pack.tdc_time_abs_norm(), channel, None));
        }

        fn sort(&mut self) {
            self.tdc.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());
            self.clean_tdc = self.tdc.iter().filter(|ph| ph.1 != 16 && ph.1 != 24).cloned().collect::<Vec<_>>();
        }
    }

    struct IsiBoxCorrectVector(Vec<Option<TIME>>, usize);

    impl IsiBoxCorrectVector {
        #[inline]
        fn add_offset(&mut self, max_index: usize, value: TIME) {
            //self.0.iter_mut().enumerate().filter(|(index, x)| x.is_none() && *index <= max_index).for_each(|(index, x)| *x = Some(value));
            self.0[self.1..max_index+1].iter_mut().filter(|x| x.is_none()).for_each(|x| *x = Some(value));
            self.1 = max_index
        }
    }


    pub struct TempElectronData {
        pub electron: CollectionElectron, //Time, X, Y and ToT and Time difference (for Spim positioning)
        pub min_index: usize,
    }

    impl TempElectronData {
        fn new() -> Self {
            Self {
                electron: CollectionElectron::new(),
                min_index: 0,
            }
        }
    }

            

    pub fn search_coincidence<T: ClusterCorrection>(coinc_data: &mut ElectronData<T>) -> io::Result<()> {

        let mut file0 = fs::File::open(&coinc_data.file)?;
        let progress_size = file0.metadata().unwrap().len() as u64;
        let spim_tdc: Box<dyn TdcControl> = if coinc_data.is_spim {
            if coinc_data.spim_size.0 == 0 || coinc_data.spim_size.1 == 0 {
                panic!("Spim mode is on. X and Y pixels must be greater than 0.");
            }
            let temp = PeriodicTdcRef::new(TdcType::TdcOneFallingEdge, &mut file0, Some(coinc_data.spim_size.1)).expect("Could not create period TDC reference.");
            coinc_data.prepare_spim(temp);
            Box::new(temp)
        } else {
            Box::new(NonPeriodicTdcRef::new(TdcType::TdcOneFallingEdge, &mut file0, None).expect("Could not create non periodic TDC reference."))
        };
        let np_tdc = NonPeriodicTdcRef::new(TdcType::TdcTwoRisingEdge, &mut file0, None).expect("Could not create non periodic (photon) TDC reference.");

        let mut ci = 0;
        let mut file = fs::File::open(&coinc_data.file)?;
        let mut buffer: Vec<u8> = vec![0; BUFFER_SIZE];
        let mut total_size = 0;
        
        let bar = ProgressBar::new(progress_size);
        bar.set_style(ProgressStyle::with_template("[{elapsed_precise}] {bar:40.white/black} {percent}% {pos:>7}/{len:7} [ETA: {eta}] Searching electron photon coincidences")
                      .unwrap()
                      .progress_chars("=>-"));
        
        while let Ok(size) = file.read(&mut buffer) {
            if size == 0 {println!("Finished Reading."); break;}
            total_size += size;
            bar.inc(BUFFER_SIZE as u64);
            //println!("MB Read: {}", total_size / 1_000_000 );
            let mut temp_edata = TempElectronData::new();
            let mut temp_tdc = TempTdcData::new();
            //let mut packet_chunks = buffer[0..size].chunks_exact(8);
            buffer[0..size].chunks_exact(8).for_each(|pack_oct| {
                match *pack_oct {
                    [84, 80, 88, 51, nci, _, _, _] => {ci=nci;},
                    _ => {
                        let packet = Pack { chip_index: ci, data: packet_change(pack_oct)[0] };
                        match packet.id() {
                            6 if packet.tdc_type() == np_tdc.id() => {
                                temp_tdc.add_tdc(&packet, 0);
                            },
                            6 if packet.tdc_type() == spim_tdc.id() => {
                                coinc_data.add_spim_line(&packet);
                            },
                            11 => {
                                let se = SingleElectron::new(&packet, coinc_data.spim_tdc);
                                temp_edata.electron.add_electron(se);
                            },
                            _ => {}, //println!("{}", packet.tdc_type());},
                        };
                    },
                };
            });
        coinc_data.add_events(temp_edata, &mut temp_tdc, 104, 40);
        //println!("Time elapsed: {:?}", start.elapsed());

        }
        println!("Total number of bytes read {}", total_size);
        Ok(())
    }
    
    pub fn correct_coincidence_isi<T: ClusterCorrection>(file2: &str, coinc_data: &mut ElectronData<T>, jump_tp3_tdc: usize) -> Result<(TempTdcData, usize), Tp3ErrorKind> {
    
        //TP3 configurating TDC Ref
        let mut file0 = fs::File::open(&coinc_data.file).unwrap();
        let progress_size = file0.metadata().unwrap().len();
        let mut spim_tdc = PeriodicTdcRef::new(TdcType::TdcOneFallingEdge, &mut file0, Some(coinc_data.spim_size.1)).expect("Could not create period TDC reference.");
        coinc_data.prepare_spim(spim_tdc);
        let _begin_tp3_time = spim_tdc.begin_frame;
        let mut tp3_tdc_counter = 0;
    
        //IsiBox loading file & setting up synchronization
        let f = fs::File::open(file2).unwrap();
        let temp_list = isi_box::get_channel_timelist(f, coinc_data.spim_size, spim_tdc.pixel_time(coinc_data.spim_size.0) * 15_625 / 10_000);
        println!("***IsiBox***: Selected pixel time is (ns): {}.", spim_tdc.pixel_time(coinc_data.spim_size.0) * 15_625 / 10_000);
        let _begin_isi_time = temp_list.start_time;
        let mut temp_tdc = TempTdcData::new_from_isilist(temp_list);
        let tdc_vec = temp_tdc.get_sync();
        let _isi_tdc_counter = tdc_vec.len();
        let mut tdc_iter = tdc_vec.iter();

        let mut counter_jump_tp3_tdc = 0;
        
        let bar = ProgressBar::new(progress_size);
        bar.set_style(ProgressStyle::with_template("[{elapsed_precise}] {bar:40.white/black} {percent}% {pos:>7}/{len:7} [ETA: {eta}] Correcting IsiBox values")
                      .unwrap()
                      .progress_chars("=>-"));
        
        let mut correct_vector = IsiBoxCorrectVector(vec![None; temp_tdc.get_vec_len()], 0);
        
        let mut offset = 0;
        let mut ci = 0;
        let mut file = fs::File::open(&coinc_data.file).unwrap();
        let mut buffer: Vec<u8> = vec![0; ISI_BUFFER_SIZE];
        let mut total_size = 0;
        let mut quit = false;
        let ignore_lines = 0;
        let mut counter_ignore_lines = 0;
        
        while let Ok(size) = file.read(&mut buffer) {
            if size == 0 {break;}
            if quit {break;}
            //if (total_size / 1_000_000) > 10_000 {break;}
            total_size += size;
            bar.inc(ISI_BUFFER_SIZE as u64);
            buffer[0..size].chunks_exact(8).for_each(|pack_oct| {
                match *pack_oct {
                    [84, 80, 88, 51, nci, _, _, _] => {ci=nci;},
                    _ => {
                        let packet = Pack { chip_index: ci, data: packet_change(pack_oct)[0] };
                        match packet.id() {
                            6 if packet.tdc_type() == spim_tdc.id() => {

                                //This jumps timepix3 TDCs based on the value given to jump_tp3_tdc
                                if jump_tp3_tdc > counter_jump_tp3_tdc {
                                    counter_jump_tp3_tdc += 1;
                                    return;
                                }
                                
                                //This jumps IsiBox lines when the Timepix3 raw data loses TDCs
                                tp3_tdc_counter = spim_tdc.counter();
                                spim_tdc.upt(packet.tdc_time_abs(), packet.tdc_counter());
                                let tp3_values_to_skip = (spim_tdc.counter() - tp3_tdc_counter - 2) / 2;

                                if spim_tdc.counter() != 0 {
                                    for _ in 0..tp3_values_to_skip {
                                        let _val = tdc_iter.next().unwrap();
                                    }
                                }
                                





                                
                                coinc_data.add_spim_line(&packet);
                                let of = coinc_data.estimate_overflow(&packet).unwrap();
                                let isi_val = tdc_iter.next().unwrap();
                                let tdc_val = packet.tdc_time_abs() + of * Pack::tdc_overflow() * 6;
                                let mut t_dif = tdc_val - isi_val.1;
                                
                                if ignore_lines > counter_ignore_lines {
                                    counter_ignore_lines += 1;
                                    return;
                                }
                                
                                
                                //Sometimes the estimative time does not work, underestimating it.
                                //This tries to recover it out by adding a single offset;
                                if isi_val.1 > tdc_val {
                                    let of = of + 1;
                                    let tdc_val = packet.tdc_time_abs() + of * Pack::tdc_overflow() * 6;
                                    t_dif = tdc_val - isi_val.1;
                                } else {
                                    //Sometimes the estimative time does not work, overestimating it.
                                    //This tries to recover it out by removing a single offset
                                    if (offset != 0) && ((t_dif > offset + ISI_TP3_MAX_DIF) || (offset > t_dif + ISI_TP3_MAX_DIF)) {
                                        let of = of - 1;
                                        let tdc_val = packet.tdc_time_abs() + of * Pack::tdc_overflow() * 6;
                                        t_dif = tdc_val - isi_val.1;
                                    }
                                };

                                //println!("{} and {} and {} and {} and {} and {} and {}", offset, t_dif, isi_val.1, packet.tdc_time_abs(), tdc_val, of, packet.tdc_counter());
                                
                                if (offset != 0) && ((t_dif > offset + ISI_TP3_MAX_DIF) || (offset > t_dif + ISI_TP3_MAX_DIF)) {
                                    //println!("***IsiBox***: Possibly problem in acquiring TDC in both TP3 and IsiBox. Values for debug (Time difference, TDC, Isi, Packet_tdc, overflow, current offset) are: {} and {} and {} and {} and {} and {}", t_dif, tdc_val, isi_val.1, packet.tdc_time_abs(), of, offset);
                                    //println!("{:?}", isi_val);
                                    //println!("{:?}", tdc_iter.next().unwrap());
                                    //panic!("program is over");
                                    quit = true;
                                } else {
                                    //Note here that a bad one will be skipped but the next one
                                    //will try to fix it because the min_index of
                                    //'IsiBoxCorrectorVector' won't be setted in the bad
                                    //interaction.
                                    correct_vector.add_offset(isi_val.0, t_dif);
                                }

                                offset = t_dif;
                     
                            },
                            11 => {},
                            _ => {},
                        };
                    },
                };
            });
        temp_tdc.correct_tdc(&mut correct_vector);
        }
        //If less than 50% of the file is read, it considers it was an issue and thus a retry must
        //be performed.
        if (total_size * 100 / progress_size as usize) < 50 {
            println!("***IsiBox***: IsiBox values not corrected. Retrying with a different condition.");
            return Err(Tp3ErrorKind::IsiBoxCouldNotSync);
        } else {
            println!("***IsiBox***: IsiBox values corrected.");
        }
        temp_tdc.sort();
        Ok((temp_tdc, total_size))
    }

    pub fn search_coincidence_isi<T: ClusterCorrection>(file2: &str, coinc_data: &mut ElectronData<T>) -> io::Result<()> {
    
        //TP3 configurating TDC Ref
        let mut file0 = fs::File::open(&coinc_data.file)?;
        let progress_size = file0.metadata().unwrap().len() as u64;
        let spim_tdc = PeriodicTdcRef::new(TdcType::TdcOneFallingEdge, &mut file0, Some(coinc_data.spim_size.1)).expect("Could not create period TDC reference.");
        //coinc_data.prepare_spim(spim_tdc);
    
        
        let (mut temp_tdc, max_total_size) = match correct_coincidence_isi(file2, coinc_data, 0) {
            Ok((tt, mts)) => (tt, mts),
            Err(_) => correct_coincidence_isi(file2, coinc_data, 1).unwrap(),
        };
        
        //let (mut temp_tdc, max_total_size) = correct_coincidence_isi(file1, file2, coinc_data, 0).unwrap();

        let mut ci = 0;
        let mut file = fs::File::open(&coinc_data.file)?;
        let mut buffer: Vec<u8> = vec![0; ISI_BUFFER_SIZE];
        let mut total_size = 0;
        
        let bar = ProgressBar::new(progress_size);
        bar.set_style(ProgressStyle::with_template("[{elapsed_precise}] {bar:40.white/black} {percent}% {pos:>7}/{len:7} [ETA: {eta}] Searching electron photon coincidences")
                      .unwrap()
                      .progress_chars("=>-"));
        
        while let Ok(size) = file.read(&mut buffer) {
            if size == 0 {break;}
            if total_size >= max_total_size {break;}
            total_size += size;
            bar.inc(ISI_BUFFER_SIZE as u64);
            //println!("MB Read: {}", total_size / 1_000_000 );
            //if (total_size / 1_000_000) > 2_000 {break;}
            let mut temp_edata = TempElectronData::new();
            buffer[0..size].chunks_exact(8).for_each(|pack_oct| {
                match *pack_oct {
                    [84, 80, 88, 51, nci, _, _, _] => {ci=nci;},
                    _ => {
                        let packet = Pack { chip_index: ci, data: packet_change(pack_oct)[0] };
                        match packet.id() {
                            6 if packet.tdc_type() == spim_tdc.id() => {
                                coinc_data.add_spim_line(&packet);
                            },
                            11 => {
                                let se = SingleElectron::new(&packet, coinc_data.spim_tdc);
                                temp_edata.electron.add_electron(se);
                            },
                            _ => {}, //println!("{}", packet.tdc_type());},
                        };
                    },
                };
            });
        coinc_data.add_events(temp_edata, &mut temp_tdc, 78, 50); //Fast start (NIM)
        //coinc_data.add_events(temp_edata, &mut temp_tdc, 87, 100); //Slow start (TTL)
        }
        println!("***IsiBox***: Coincidence search is over.");
        Ok(())
    }
}

pub mod isi_box {
    //use rand_distr::{Normal, Distribution};
    //use rand::{thread_rng};
    use std::fs::OpenOptions;
    use std::io::{Read, Write};
    use crate::spimlib::VIDEO_TIME;
    use crate::tdclib::isi_box::CHANNELS;
    use crate::auxiliar::value_types::*;
    use indicatif::{ProgressBar, ProgressStyle};
    
    const ISI_CHANNEL_SHIFT: [u32; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

    fn as_bytes<T>(v: &[T]) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                v.as_ptr() as *const u8,
                v.len() * std::mem::size_of::<T>())
        }
    }
    
    fn as_int(v: &[u8]) -> &[u32] {
        unsafe {
            std::slice::from_raw_parts(
                v.as_ptr() as *const u32,
                //v.len() )
                v.len() * std::mem::size_of::<u8>() / std::mem::size_of::<u32>())
        }
    }

    fn add_overflow(data: u64, value: u64) -> u64
    {
        (data + value) % 67108864
    }
    
    fn subtract_overflow(data: u64, value: u64) -> u64
    {
        if data > value {
            (data - value) % 67108864
        } else {
            (data + 67108864 - value) % 67108864
        }
    }
 
    type IsiListType = (TIME, u32, Option<u32>, Option<u32>, Option<i16>);
    struct IsiListVec(Vec<IsiListType>);
    struct IsiListVecg2(Vec<(i64, u32, Option<u32>, Option<u32>)>);

    pub struct IsiList {
        data_raw: IsiListVec, //Time, channel, spim index, spim frame, dT
        x: u32,
        y: u32,
        pixel_time: u32,
        pub counter: u32,
        pub overflow: u32,
        last_time: u32,
        pub start_time: Option<u32>,
        line_time: Option<u32>,
    }


    impl IsiList {
        fn increase_counter(&mut self, data: u32) {
  
            if let Some(line_time) = self.line_time {
                let mut time_dif = data as i32 - self.last_time as i32;
                
                if time_dif < 0 {
                    time_dif += 67108864;
                }


                //Modulus of the time_dif relative to the line time
                let fractional = time_dif as u32 - (time_dif as u32 / line_time) * line_time;

                //If modulus > 1% or smaller than 99% of the line time, we have found an issue
                if fractional > line_time / 100 && fractional < (line_time * 99) / 100 {
                }


                if (time_dif - line_time as i32).abs() > 10 {
                    //println!("***IsiBox***: Probable line issue. Raw time is {}. Diference relative to the last time is {}. The spim frame is {:?}. Line counter is {}. Line time is {}. Last time is {}. Abs time is {}.", data, time_dif, self.spim_frame(), self.counter, line_time, self.last_time, self.get_abs_time(data));
                }
            }
            
            if data < self.last_time {self.overflow+=1;}
            self.last_time = data;
            self.counter += 1;
        }

        fn add_event(&mut self, channel: u32, data: u32) {
            //self.data.0.push((self.get_abs_time(data), channel, self.spim_index(data), self.spim_frame(), None));
            let data = if channel < 16 {
                ISI_CHANNEL_SHIFT[channel as usize] + data
            } else {
                data
            };
            self.data_raw.0.push((data as u64, channel, None, None, None));
        }

        fn determine_line_time(&mut self) {
            let iter = self.scan_iterator().
                filter_map(|(val1, val2)| {
                    if val2.1.0 > val1.1.0 {
                        Some((val2.1.0 - val1.1.0) as u32)
                    } else {None}
                });

            let mut line = u32::MAX;
            for val in iter {
                if line == val {
                    break;
                }
                line = val;
            };
            println!("***IsiBox***: Line time is (units of 120 ps): {}", line);
            self.line_time = Some(line);
        }

        fn check_for_issues(&mut self) {
            //Check if there is an issue in the first scan. This is very rare but can happens sometimes.
            let iter = self.scan_iterator().
                filter(|(val1, val2)| ((subtract_overflow(val2.1.0, val1.1.0) > self.line_time.unwrap() as u64 + 1_000) || (subtract_overflow(val2.1.0, val1.1.0) < self.line_time.unwrap() as u64 - 1_000))).
                collect::<Vec<_>>();
            for val in iter {
                if val.0.0 == 0 { //First value is literally a bad scan line
                    //Removing the bad vector
                    self.data_raw.0.remove(0);
                    break;
                }
            }
            
            let progress_size = 100;
            let bar = ProgressBar::new(progress_size);
            bar.set_style(ProgressStyle::with_template("[{elapsed_precise}] {bar:40.white/black} {percent}% {pos:>7}/{len:7} [ETA: {eta}] Checking for issues in the IsiBox data")
                          .unwrap()
                          .progress_chars("=>-"));

            for _ in 0..progress_size {
                bar.inc(1);
                let iter = self.scan_iterator().
                    filter(|(val1, val2)| ((subtract_overflow(val2.1.0, val1.1.0) > self.line_time.unwrap() as u64 + 1_000) || (subtract_overflow(val2.1.0, val1.1.0) < self.line_time.unwrap() as u64 - 1_000))).
                    collect::<Vec<_>>();
                //println!("***IsiBox***: Start of a correction cycle. The size is {}.", iter.len());
                let mut number_of_insertions = 0;
                if iter.is_empty() {
                    //println!("***IsiBox***: values successfully corrected."); 
                    break;}
                for val in iter {
                    //println!("{:?}", val);
                    self.data_raw.0.insert(val.1.0+number_of_insertions, (subtract_overflow(val.1.1.0, self.line_time.unwrap() as u64), val.1.1.1, val.1.1.2, val.1.1.3, val.1.1.4));
                    number_of_insertions += 1;
                }
            }
            println!("***IsiBox***: reference values corrected.");
        }

        fn correct_data(&mut self) {
            let mut counter = 0;
            let mut last_time = 0;
            let mut overflow = 0;
            let low = (self.x * self.pixel_time) as u64;
            let y = self.y;
            let x = self.x;

            
            let spim_index = |data: u64, ct: u32, lt: u64| -> Option<u32> {
                let line = ct % y;
                let time = if data > VIDEO_TIME * 13 + lt {
                    data - lt
                } else {
                    data + 67108864 - lt
                };

                if time > low {return None;}
                let column = ((time as u64 * x as u64) / low) as u32;
                let index = line * x + column;
                
                Some(index)
            };
            

            self.data_raw.0.iter_mut().for_each(|x| {
                //Correction time
                let raw_time = x.0;
                if x.0 > last_time {
                    x.0 += overflow as TIME * 67108864;
                } else {
                    x.0 += (overflow + 1) as TIME  * 67108864;
                };
                //Correcting spim index
                x.2 = spim_index(raw_time, counter, last_time);
                //Correcting spim frame
                x.3 = Some(counter / y);

                //If it is a scan signal
                if x.1 == 16 {
                    if raw_time < last_time {
                        overflow+=1;
                    }
                    counter += 1;
                    last_time = raw_time;
                }


            });
        }

        
        fn scan_iterator(&self) -> impl Iterator<Item=((usize, IsiListType), (usize, IsiListType))> + '_ {
            let iter1 = self.data_raw.0.iter().
                cloned().
                enumerate().
                filter(|(_index, (_time, channel, _spim_index, _spim_frame, _dt))| *channel == 16);
            
            let mut iter2 = self.data_raw.0.iter().
                cloned().
                enumerate().
                filter(|(_index, (_time, channel, _spim_index, _spim_frame, _dt))| *channel == 16);

            let _advance = iter2.next();

            iter1.zip(iter2)
        }
        
        pub fn get_timelist_with_tp3_tick(&self) -> Vec<(TIME, COUNTER, Option<i16>)> {
            let first = self.data_raw.0.iter().
                filter(|(_time, channel, _spim_index, _spim_frame, _dt)| *channel == 16).
                map(|(time, _channel, _spim_index, _spim_frame, _dt)| (*time * 1200 * 6) / 15625).
                next().
                unwrap();
            
            self.data_raw.0.iter().
                map(|(time, channel, _spim_index, _spim_frame, dt)| (((*time * 1200 * 6) / 15625) - first, *channel, *dt)).
                //map(|(time, channel)| (time - (time / 103_079_215_104) * 103_079_215_104, channel)).
                collect::<Vec<_>>()
            
        }

        fn output_spim(&self) {
            let spim_vec = self.data_raw.0.iter().
                filter(|(_time, channel, _spim_index, _spim_frame, _dt)| *channel != 16 && *channel != 24).
                filter(|(_time, _channel, spim_index, spim_frame, _dt)| spim_index.is_some() && spim_frame.is_some()).
                map(|(_time, channel, spim_index, _spim_frame, _dt)| spim_index.unwrap() * CHANNELS as u32 + channel).collect::<Vec<u32>>();
 
            let index_vec = self.data_raw.0.iter().
                filter(|(_time, channel, _spim_index, _spim_frame, _dt)| *channel != 16 && *channel != 24).
                filter(|(_time, _channel, spim_index, spim_frame, _dt)| spim_index.is_some() && spim_frame.is_some()).
                map(|(_time, _channel, _spim_index, spim_frame, _dt)| spim_frame.unwrap()).collect::<Vec<u32>>();

            let mut tfile = OpenOptions::new()
                .write(true)
                .truncate(true)
                .create(true)
                .open("isi_si_complete.txt").expect("Could not output SI index.");
            tfile.write_all(as_bytes(&spim_vec)).expect("Could not write time to file.");
            
            let mut tfile = OpenOptions::new()
                .write(true)
                .truncate(true)
                .create(true)
                .open("isi_si_complete_frame.txt").expect("Could not output SI frame.");
            tfile.write_all(as_bytes(&index_vec)).expect("Could not write time to file.");
        }

        fn search_coincidence(&mut self, ch1: u32, ch2: u32) {
            let progress_size = self.data_raw.0.len() as u64;
            let vec1_len = self.data_raw.0.iter().filter(|(_time, channel, _spim_index, _spim_frame, _dt)| *channel == ch1).count();
            let mut vec2 = self.data_raw.0.iter().filter(|(_time, channel, _spim_index, _spim_frame, _dt)| *channel == ch2).cloned().collect::<Vec<_>>();
            let iter1 = self.data_raw.0.iter_mut();
            let mut min_index = 0;
            let mut corr = 0;


            let mut new_list = IsiListVecg2(Vec::new());
            let bar = ProgressBar::new(progress_size);
            bar.set_style(ProgressStyle::with_template("[{elapsed_precise}] {bar:40.white/black} {percent}% {pos:>7}/{len:7} [ETA: {eta}] Searching photon coincidences")
                          .unwrap()
                          .progress_chars("=>-"));
        
            for val1 in iter1 {
                bar.inc(1);
                if val1.1 != ch1 {continue;}
                let mut index = 0;
                for val2 in &mut vec2[min_index..] {
                    let dt = val2.0 as i64 - val1.0 as i64;
                    if dt.abs() < 5_000 {

                        val1.4 = Some(dt as i16);
                        val2.4 = Some(dt as i16);

                        corr+=1;
                        new_list.0.push((dt, val2.1, val2.2, val2.3));
                        min_index += index / 10;
                    }
                    if dt > 100_000 {break;}
                    index += 1;
                }
            }

            self.data_raw.0.iter_mut().
                filter(|(_time, channel, _spim_index, _spim_frame, _dt)| *channel == ch2).
                zip(vec2.iter()).
                for_each(|(ph21, ph22)| {
                    ph21.4 = ph22.4;
                    assert_eq!(ph21.1, ph22.1);
                });

            
            let dt_vec = new_list.0.iter().
                filter(|(_time, _channel, spim_index, spim_frame)| spim_index.is_some() && spim_frame.is_some()).
                map(|(dtime, _channel, _spim_index, _spim_frame)| *dtime).
                collect::<Vec<i64>>();
            
            println!("***IsiBox***: Size of the (first/second) channel: ({} / {}). Number of coincidences: {}. Number of output coincidences: {}. Ratio: {} %.", vec1_len, vec2.len(), corr, dt_vec.len(), dt_vec.len() as f32 * 100.0 / vec1_len as f32);
            
            let spim_index_vec = new_list.0.iter().
                filter(|(_time, _channel, spim_index, spim_frame)| spim_index.is_some() && spim_frame.is_some()).
                map(|(_dtime, _channel, spim_index, _spim_frame)| spim_index.unwrap()).
                collect::<Vec<u32>>();

            let mut tfile = OpenOptions::new()
                .write(true)
                .truncate(true)
                .create(true)
                .open("isi_g2.txt").expect("Could not output time histogram.");
            tfile.write_all(as_bytes(&dt_vec)).expect("Could not write time to file.");
            
            let mut tfile = OpenOptions::new()
                .write(true)
                .truncate(true)
                .create(true)
                .open("isi_g2_index.txt").expect("Could not output time histogram.");
            tfile.write_all(as_bytes(&spim_index_vec)).expect("Could not write time to file.");
            
        }
    }

    //Pixel time must be in nanoseconds.
    pub fn get_channel_timelist<V>(mut data: V, spim_size: (POSITION, POSITION), pixel_time: TIME) -> IsiList 
        where V: Read
        {
            let mut list = IsiList{data_raw: IsiListVec(Vec::new()), x: spim_size.0, y: spim_size.1, pixel_time: (pixel_time * 83_333 / 10_000) as u32, counter: 0, overflow: 0, last_time: 0, start_time: None, line_time: None};
            let mut buffer = [0; 256_000];
            while let Ok(size) = data.read(&mut buffer) {
                if size == 0 {println!("***IsiBox***: Finished reading file."); break;}
                buffer.chunks_exact(4).for_each( |x| {
                    let channel = (as_int(x)[0] & 0xFC000000) >> 27;
                    let _overflow = (as_int(x)[0] & 0x04000000) >> 26;
                    let time = as_int(x)[0] & 0x03FFFFFF;
                    list.add_event(channel, time);
                    if channel == 16 {
                        list.increase_counter(time);
                    }
                })
            }
            list.determine_line_time();
            list.check_for_issues();
            list.correct_data();
            list.output_spim();
            list.search_coincidence(0, 12);
            list
        }
}

pub mod ntime_resolved {
    use std::fs::OpenOptions;
    use crate::packetlib::{Packet, PacketEELS as Pack, packet_change};
    use crate::tdclib::{TdcControl, TdcType, PeriodicTdcRef};
    use std::io::prelude::*;
    use crate::clusterlib::cluster::{SingleElectron, CollectionElectron};
    use crate::clusterlib::cluster::ClusterCorrection;
    use std::convert::TryInto;
    use std::time::Instant;
    use std::fs;
    use crate::auxiliar::{value_types::*, ConfigAcquisition};

    #[derive(Debug)]
    pub enum ErrorType {
        OutOfBounds,
        FolderDoesNotExist,
        FolderNotCreated,
        ScanOutofBounds,
        MinGreaterThanMax,
    }

    /// This enables spatial+spectral analysis in a certain spectral window.
    pub struct TimeSpectralSpatial<T> {
        spectra: Vec<POSITION>, //Main data,
        return_spectra: Vec<POSITION>, //Main data from flyback,
        indices: Vec<u16>, //indexes from main scan
        return_indices: Vec<u16>, //indexes from flyback
        ensemble: CollectionElectron, //A collection of single electrons,
        spimx: POSITION, //The horinzontal axis of the spim,
        spimy: POSITION, //The vertical axis of the spim,
        tdc_periodic: Option<PeriodicTdcRef>, //The periodic tdc. Can be none if xspim and yspim <= 1,
        spim_tdc_type: TdcType, //The tdc type for the spim,
        extra_tdc_type: TdcType, //The tdc type for the external,
        remove_clusters: T,
        file: String,
    }

    fn as_bytes<T>(v: &[T]) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                v.as_ptr() as *const u8,
                v.len() * std::mem::size_of::<T>())
        }
    }
    
    impl<T: ClusterCorrection> TimeSpectralSpatial<T> {
        fn prepare(&mut self, file: &mut fs::File) {
            self.tdc_periodic = match self.tdc_periodic {
                None if self.spimx>1 && self.spimy>1 => {
                    Some(PeriodicTdcRef::new(self.spim_tdc_type.clone(), file, Some(self.spimy)).expect("Problem in creating periodic tdc ref."))
                },
                Some(val) => Some(val),
                _ => None,
            };
        }

        fn add_electron(&mut self, packet: &Pack) {
            let se = SingleElectron::new(packet, self.tdc_periodic);
            self.ensemble.add_electron(se);
        }

        fn add_spim_tdc(&mut self, packet: &Pack) {
            //Synchronizing clocks using two different approaches. It is always better to use a multiple of 2 and use the FPGA counter.
            match &mut self.tdc_periodic {
                //Some(my_tdc_periodic) if packet.tdc_type() == self.tdc_type.associate_value() => {
                Some(my_tdc_periodic) => {
                    my_tdc_periodic.upt(packet.tdc_time_norm(), packet.tdc_counter());
                },
                _ => {},
            };
        }
        
        fn add_extra_tdc(&mut self, _packet: &Pack) {
            //self.spectra.push(SPIM_PIXELS);
            //spimlib::get_spimindex(, dt: TIME, spim_tdc: &PeriodicTdcRef, self.spimx, self.spimy;
        }

        fn process(&mut self) -> Result<(), ErrorType> {
            if self.ensemble.try_clean(0, &self.remove_clusters) {
                for val in self.ensemble.values() {
                    if let Some(index) = val.get_or_not_spim_index(self.tdc_periodic, self.spimx, self.spimy) {
                        self.spectra.push(index);
                        self.indices.push((val.spim_slice()).try_into().expect("Exceeded the maximum number of indices"));
                    }
                    
                    if let Some(index) = val.get_or_not_return_spim_index(self.tdc_periodic, self.spimx, self.spimy) {
                        self.return_spectra.push(index);
                        self.return_indices.push((val.spim_slice()).try_into().expect("Exceeded the maximum number of indices"));
                    }
            }
            self.ensemble.clear();

            let mut tfile = OpenOptions::new()
                .append(true)
                .create(true)
                .open("si_complete.txt").expect("Could not output time histogram.");
            tfile.write_all(as_bytes(&self.spectra)).expect("Could not write time to file.");
            
            let mut return_tfile = OpenOptions::new()
                .append(true)
                .create(true)
                .open("si_return_complete.txt").expect("Could not output time histogram.");
            return_tfile.write_all(as_bytes(&self.return_spectra)).expect("Could not write time to file.");
            
            let mut tfile2 = OpenOptions::new()
                .append(true)
                .create(true)
                .open("si_complete_indices.txt").expect("Could not output time histogram.");
            tfile2.write_all(as_bytes(&self.indices)).expect("Could not write time to indices file.");
            
            let mut return_tfile2 = OpenOptions::new()
                .append(true)
                .create(true)
                .open("si_complete_return_indices.txt").expect("Could not output time histogram.");
            return_tfile2.write_all(as_bytes(&self.return_indices)).expect("Could not write time to indices file.");

            self.spectra.clear();
            self.return_spectra.clear();
            self.indices.clear();
            self.return_indices.clear();
            }
            Ok(())
        }
            
        pub fn new(my_config: ConfigAcquisition<T>) -> Result<Self, ErrorType> {

            Ok(Self {
                spectra: Vec::new(),
                return_spectra: Vec::new(),
                indices: Vec::new(),
                return_indices: Vec::new(),
                ensemble: CollectionElectron::new(),
                spimx: my_config.xspim,
                spimy: my_config.yspim,
                tdc_periodic: None,
                spim_tdc_type: TdcType::TdcOneFallingEdge,
                extra_tdc_type: TdcType::TdcTwoRisingEdge,
                remove_clusters: my_config.correction_type,
                file: my_config.file,
                
            })
        }
    }

    pub fn analyze_data<T: ClusterCorrection>(data: &mut TimeSpectralSpatial<T>) {
        let mut prepare_file = fs::File::open(&data.file).expect("Could not open desired file.");
        data.prepare(&mut prepare_file);
        
        let start = Instant::now();
        let mut my_file = fs::File::open(&data.file).expect("Could not open desired file.");
        let mut buffer: Vec<u8> = vec![0; 1_000_000_000];
        
        let mut total_size = 0;
        let mut ci = 0;

        while let Ok(size) = my_file.read(&mut buffer) {
            if size==0 {break;}
            total_size += size;
            buffer[0..size].chunks_exact(8).for_each(|pack_oct| {
                match pack_oct {
                    &[84, 80, 88, 51, nci, _, _, _] => {ci = nci},
                    _ => {
                        let packet = Pack{chip_index: ci, data: packet_change(pack_oct)[0]};
                        match packet.id() {
                            6 if packet.tdc_type() == data.spim_tdc_type.associate_value() => {
                                data.add_spim_tdc(&packet);
                            },
                            6 if packet.tdc_type() == data.extra_tdc_type.associate_value() => {
                                data.add_extra_tdc(&packet);
                            },
                            11 => {
                                data.add_electron(&packet);
                            },
                            _ => {},
                        };
                    },
                };
            });
            data.process().expect("Error in processing");
            println!("File: {:?}. Total number of bytes read (MB): ~ {}", &data.file, total_size/1_000_000);
            println!("Time elapsed: {:?}", start.elapsed());
        };
    }
}

pub mod calibration {

    use std::fs::OpenOptions;
    use crate::packetlib::{Packet, TimeCorrectedPacketEELS as Pack, packet_change};
    use std::io;
    use std::io::prelude::*;
    use std::fs;
    use std::convert::TryInto;
    use crate::clusterlib::cluster::{SingleElectron, CollectionElectron, ClusterCorrection};
    use indicatif::{ProgressBar, ProgressStyle};
    use rayon::prelude::*;
    
    fn as_bytes<T>(v: &[T]) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                v.as_ptr() as *const u8,
                v.len() * std::mem::size_of::<T>())
        }
    }
    
    fn output_data<T>(data: &[T], name: &str) {
        let mut tfile = OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(name).unwrap();
        tfile.write_all(as_bytes(data)).unwrap();
        println!("Outputting data under {:?} name. Vector len is {}", name, data.len());
    }
    
    pub struct CalibrationData {
        rel_time: Vec<i8>,
        x: Vec<u16>,
        y: Vec<u8>,
        tot: Vec<u16>,
        cluster_size: Vec<u16>,
    }

    impl CalibrationData {
        fn new() -> Self {
            CalibrationData {rel_time: Vec::new(), x: Vec::new(), y: Vec::new(), tot: Vec::new(), cluster_size: Vec::new()}
        }
        fn append_from_collection(&mut self, val: CollectionElectron) {
            for electron in val.iter() {
                self.x.push(electron.x().try_into().unwrap());
                self.y.push(electron.y().try_into().unwrap());
                self.tot.push(electron.tot().try_into().unwrap());
                let electron_time = electron.time() as i64;
                let electron_tot_reference = electron.frame_dt() as i64;
                let time_diference = (electron_time - electron_tot_reference) as i8;
                self.rel_time.push(time_diference);
                self.cluster_size.push(electron.cluster_size().try_into().unwrap());
            }
        }
        pub fn output_relative_calibration_time(&self) {
            output_data(&self.rel_time, "relative_calibration_time.txt");
        }
        pub fn output_x(&self) {
            output_data(&self.x, "relative_calibration_x.txt");
        }
        pub fn output_y(&self) {
            output_data(&self.y, "relative_calibration_y.txt");
        }
        pub fn output_tot(&self) {
            output_data(&self.tot, "relative_calibration_tot.txt");
        }
        pub fn output_cluster_size(&self) {
            output_data(&self.cluster_size, "relative_calibration_cluster_size.txt");
        }
    }

    pub fn calibrate<T: ClusterCorrection>(file: &str, correction_type: &T) -> io::Result<()> {

        let mut ci = 0;
        let mut file = fs::File::open(file)?;
        let progress_size = file.metadata().unwrap().len() as u64;
        let mut buffer: Vec<u8> = vec![0; 512_000_000];
        let mut total_size = 0;
        
        let bar = ProgressBar::new(progress_size);
        bar.set_style(ProgressStyle::with_template("[{elapsed_precise}] {bar:40.white/black} {percent}% {pos:>7}/{len:7} [ETA: {eta}] Searching for clusters and calibrating data")
                      .unwrap()
                      .progress_chars("=>-"));
        
        let mut calibration_data = CalibrationData::new();
        while let Ok(size) = file.read(&mut buffer) {
            let mut temp_electrons = CollectionElectron::new();
            if size == 0 {println!("Finished Reading."); break;}
            total_size += size;
            //if total_size / 1_000_000_000 > 2 {break;}
            bar.inc(512_000_000_u64);
            buffer[0..size].chunks_exact(8).for_each(|pack_oct| {
                match *pack_oct {
                    [84, 80, 88, 51, nci, _, _, _] => {ci=nci;},
                    _ => {
                        let packet = Pack { chip_index: ci, data: packet_change(pack_oct)[0] };
                        if packet.id() == 11 {
                            let se = SingleElectron::new(&packet, None);
                            temp_electrons.add_electron(se);
                            //temp_edata.electron.add_electron(se);
                        }
                    },
                };
            });
            temp_electrons.sort();
            temp_electrons.try_clean(0, correction_type);
            calibration_data.append_from_collection(temp_electrons);
        }
        calibration_data.output_relative_calibration_time();
        calibration_data.output_x();
        calibration_data.output_y();
        calibration_data.output_tot();
        calibration_data.output_cluster_size();
        println!("Total number of bytes read {}", total_size);
        Ok(())
    }
}
