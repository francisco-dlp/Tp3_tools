pub mod coincidence {

    use crate::packetlib::{Packet, PacketEELS as Pack};
    use crate::tdclib::TdcType;
    use std::io;
    use std::io::prelude::*;
    use std::fs;
    use rayon::prelude::*;
    //use std::time::Instant;

    const TIME_WIDTH: usize = 50;
    const TIME_DELAY: usize = 160;
    const MIN_LEN: usize = 50; // This is the minimal TDC vec size. It reduces over time.
    //const EXC: (usize, usize) = (5, 3); //This controls how TDC vec reduces. (20, 5) means if correlation is got in the time index >20, the first 5 items are erased.
    const CLUSTER_DET:usize = 50;

    pub struct ElectronData {
        pub time: Vec<usize>,
        pub rel_time: Vec<isize>,
        pub x: Vec<usize>,
        pub y: Vec<usize>,
        pub tot: Vec<u16>,
        pub cluster_size: Vec<usize>,
        pub spectrum: Vec<usize>,
        pub corr_spectrum: Vec<usize>,
    }

    impl ElectronData {
        fn add_electron(&mut self, val: (usize, usize, usize, u16)) {
            self.spectrum[val.1 + 1024 * val.2] += 1;
        }

        fn add_coincident_electron(&mut self, val: (usize, usize, usize, u16), photon_time: usize) {
            self.corr_spectrum[val.1 + 1024*val.2] += 1;
            self.time.push(val.0);
            self.rel_time.push(val.0 as isize - photon_time as isize);
            self.x.push(val.1);
            self.y.push(val.2);
            self.tot.push(val.3);
        }

        fn add_events(&mut self, mut temp_edata: TempElectronData, mut temp_tdc: TempTdcData) {
            temp_edata.sort();
            temp_tdc.sort();
            let nelectrons = temp_edata.electron.len();
            let nphotons = temp_tdc.tdc.len();
            
            let mut cs = temp_edata.remove_clusters();
            let nclusters = cs.len();
            self.cluster_size.append(&mut cs);
        
            println!("Electron events: {}. Number of clusters: {}, Number of photons: {}", nelectrons, nclusters, nphotons);

            for val in temp_edata.electron {
                self.add_electron(val);
                if let Some(pht) = temp_tdc.check(val.0) {
                    self.add_coincident_electron(val, pht);
                }
            }
            
            /*
            for val in temp_tdc.tdc {
                //self.add_electron(val);
                if let Some(ele) = temp_edata.check(val) {
                    self.add_coincident_electron(ele, val);
                }
            }
            */

            println!("Number of coincident electrons: {:?}", self.x.len());
        }

        pub fn new() -> Self {
            Self {
                time: Vec::new(),
                rel_time: Vec::new(),
                x: Vec::new(),
                y: Vec::new(),
                tot: Vec::new(),
                cluster_size: Vec::new(),
                spectrum: vec![0; 1024*256],
                corr_spectrum: vec![0; 1024*256],
            }
        }

        pub fn output_corr_spectrum(&self, bin: bool) {
            let out: String = match bin {
                true => {
                    let mut spec: Vec<usize> = vec![0; 1024];
                    for val in self.corr_spectrum.chunks_exact(1024) {
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
                    let mut spec: Vec<usize> = vec![0; 1024];
                    for val in self.spectrum.chunks_exact(1024) {
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
            println!("Outputting relative time under tH name. Vector len is {}", self.rel_time.len());
            let out: String = self.rel_time.iter().map(|x| x.to_string()).collect::<Vec<String>>().join(", ");
            fs::write("tH.txt", out).unwrap();
        }

        pub fn output_cluster_size(&self) {
            let out: String = self.cluster_size.iter().map(|x| x.to_string()).collect::<Vec<String>>().join(", ");
            fs::write("cs.txt", out).unwrap();
        }

        pub fn output_tot(&self, sum_cluster: bool) {
            let out: String = match sum_cluster {
                false => {
                    self.tot.iter().map(|x| x.to_string()).collect::<Vec<String>>().join(", ")
                },
                true => {
                    self.tot.iter().zip(self.cluster_size.iter()).map(|(tot, cs)| (*tot as usize * cs).to_string()).collect::<Vec<String>>().join(", ")
                },
            };
            fs::write("tot.txt", out).unwrap();
        }

            
    }

    pub struct TempTdcData {
        pub tdc: Vec<usize>,
        pub min_index: usize,
    }

    impl TempTdcData {
        fn new() -> Self {
            Self {
                tdc: Vec::new(),
                min_index: 0,
            }
        }

        fn add_tdc(&mut self, my_pack: &Pack) {
            self.tdc.push(my_pack.tdc_time_norm() - TIME_DELAY);
        }

        fn sort(&mut self) {
            self.tdc.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());
        }

        fn check(&mut self, value: usize) -> Option<usize> {
            
            let result = self.tdc[self.min_index..self.min_index+MIN_LEN].iter()
                .enumerate()
                .find(|(_, x)| ((**x as isize - value as isize).abs() as usize) < TIME_WIDTH);


            match result {
                Some((index, pht_value)) => {
                    if index > MIN_LEN/10  && self.tdc.len()>self.min_index + MIN_LEN + index {
                        self.min_index += index/2;
                    }
                    Some(*pht_value)
                },
                None => None,
            }
        }
    }



    pub struct TempElectronData {
        pub electron: Vec<(usize, usize, usize, u16)>,
        pub min_index: usize,
    }

    impl TempElectronData {
        fn new() -> Self {
            Self {
                electron: Vec::new(),
                min_index: 0,
            }
        }

        fn remove_clusters(&mut self) -> Vec<usize> {
            let mut nelist:Vec<(usize, usize, usize, u16)> = Vec::new();
            let mut cs_list: Vec<usize> = Vec::new();

            let mut last: (usize, usize, usize, u16) = self.electron[0];
            let mut cluster_vec: Vec<(usize, usize, usize, u16)> = Vec::new();
            for x in &self.electron {
                if x.0 > last.0 + CLUSTER_DET || (x.1 as isize - last.1 as isize).abs() > 2 || (x.2 as isize - last.2 as isize).abs() > 2 {
                    let cluster_size: usize = cluster_vec.len();
                    let t_mean:usize = cluster_vec.iter().map(|&(t, _, _, _)| t).sum::<usize>() / cluster_size as usize;
                    let x_mean:usize = cluster_vec.iter().map(|&(_, x, _, _)| x).sum::<usize>() / cluster_size;
                    let y_mean:usize = cluster_vec.iter().map(|&(_, _, y, _)| y).sum::<usize>() / cluster_size;
                    let tot_mean: u16 = (cluster_vec.iter().map(|&(_, _, _, tot)| tot as usize).sum::<usize>() / cluster_size) as u16;
                    //println!("{:?} and {}", cluster_vec, t_mean);
                    nelist.push((t_mean, x_mean, y_mean, tot_mean));
                    cs_list.push(cluster_size);
                    cluster_vec = Vec::new();
                }
                last = *x;
                cluster_vec.push(*x);
            }
            self.electron = nelist;
            cs_list
        }


        fn add_electron(&mut self, my_pack: &Pack) {
            self.electron.push((my_pack.electron_time(), my_pack.x(), my_pack.y(), my_pack.tot()));
        }

        fn sort(&mut self) {
            self.electron.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());
        }
        
        fn check(&mut self, value: usize) -> Option<(usize, usize, usize, u16)> {
            
            let result = self.electron[self.min_index..self.min_index+MIN_LEN].iter()
                .enumerate()
                .find(|(_, val)| ((val.0 as isize - value as isize).abs() as usize) < TIME_WIDTH);


            match result {
                Some((index, ele_value)) => {
                    if index > MIN_LEN/10  && self.electron.len()>self.min_index + MIN_LEN + index {
                        self.min_index += index/2; // - MIN_LEN/10 ;
                    }
                    Some(*ele_value)
                },
                None => None,
            }
        }
    }
            

    pub fn search_coincidence(file: &str, coinc_data: &mut ElectronData) -> io::Result<()> {
        
        let mytdc = TdcType::TdcTwoRisingEdge;
        let mut ci = 0;

        let mut file = fs::File::open(file)?;
        let mut buffer: Vec<u8> = vec![0; 256_000_000];
        let mut total_size = 0;
        
        while let Ok(size) = file.read(&mut buffer) {
            if size == 0 {println!("Finished Reading."); break;}
            total_size += size;
            println!("MB Read: {}", total_size / 1_000_000 );
            let mut temp_edata = TempElectronData::new();
            let mut temp_tdc = TempTdcData::new();
            let mut packet_chunks = buffer[0..size].chunks_exact(8);
            while let Some(pack_oct) = packet_chunks.next() {
                match pack_oct {
                    &[84, 80, 88, 51, nci, _, _, _] => {ci=nci as usize;},
                    _ => {
                        let packet = Pack { chip_index: ci, data: pack_oct };
                        match packet.id() {
                            6 if packet.tdc_type() == mytdc.associate_value() => {
                                temp_tdc.add_tdc(&packet);
                            },
                            11 => {
                                temp_edata.add_electron(&packet);
                            },
                            _ => {},
                        };
                    },
                };
            }
        coinc_data.add_events(temp_edata, temp_tdc);
        }
        println!("Total number of bytes read {}", total_size);
        Ok(())
    }
}

/*
pub mod time_resolved {
    use crate::packetlib::{Packet, PacketEELS as Pack};
    use crate::tdclib::{TdcControl, TdcType, PeriodicTdcRef};
    use std::io::prelude::*;
    use std::fs;

    #[derive(Debug)]
    pub enum ErrorType {
        OutOfBounds,
        FolderDoesNotExist,
        FolderNotCreated,
        ScanOutofBounds,
        MinGreaterThanMax,
    }

    const VIDEO_TIME: f64 = 0.000005;

    pub trait TimeTypes {
        fn prepare(&mut self, file: &mut fs::File);
        fn add_packet(&mut self, packet: &Pack);
        fn add_tdc(&mut self, packet: &Pack);
        fn output(&self) -> Result<(), ErrorType>;
        fn display_info(&self) -> Result<(), ErrorType>;
    }

    pub struct TimeSet {
        pub set: Vec<Box<dyn TimeTypes>>,
    }

    /// This enables spectral analysis in a certain spectral window.
    pub struct TimeSpectral {
        pub spectra: Vec<[usize; 1024]>,
        pub initial_time: Option<f64>,
        pub interval: usize,
        pub counter: Vec<usize>,
        pub min: usize,
        pub max: usize,
        pub folder: String,
    }

    impl TimeTypes for TimeSpectral {
        fn prepare(&mut self, _file: &mut fs::File) {
        }

        fn add_packet(&mut self, packet: &Pack) {
            self.initial_time = match self.initial_time {
                Some(t) => {Some(t)},
                None => {Some(packet.electron_time())},
            };

            if let Some(offset) = self.initial_time {
                let vec_index = ((packet.electron_time()-offset) * 1.0e9) as usize / self.interval;
                while self.spectra.len() < vec_index+1 {
                    self.spectra.push([0; 1024]);
                    self.counter.push(0);
                }
                if packet.x()>self.min && packet.x()<self.max {
                    self.spectra[vec_index][packet.x()] += 1;
                    self.counter[vec_index] += 1;
                };
            }
        }

        fn add_tdc(&mut self, _packet: &Pack) {
        }
        
        fn output(&self) -> Result<(), ErrorType> {
            if let Err(_) = fs::read_dir(&self.folder) {
                if let Err(_) = fs::create_dir(&self.folder) {
                    return Err(ErrorType::FolderNotCreated);
                }
            }
            /*
            let mut entries = match fs::read_dir(&self.folder) {
                Ok(e) => e,
                Err(_) => return Err(ErrorType::FolderDoesNotExist),
            };
            
            while let Some(x) = entries.next() {
                let path = x.unwrap().path();
                let dir = path.to_str().unwrap();
                fs::remove_file(dir).unwrap();
            };
            */
            let mut folder: String = String::from(&self.folder);
            folder.push_str("\\");
            folder.push_str(&(self.spectra.len()).to_string());
            folder.push_str("_");
            folder.push_str(&self.min.to_string());
            folder.push_str("_");
            folder.push_str(&self.max.to_string());

            let out = self.spectra.iter().flatten().map(|x| x.to_string()).collect::<Vec<String>>().join(", ");
            if let Err(_) = fs::write(&folder, out) {
                return Err(ErrorType::FolderDoesNotExist);
            }
            
            folder.push_str("_");
            folder.push_str("counter");

            let out = self.counter.iter().map(|x| x.to_string()).collect::<Vec<String>>().join(", ");
            if let Err(_) = fs::write(folder, out) {
                return Err(ErrorType::FolderDoesNotExist);
            }
            Ok(())
        }

        fn display_info(&self) -> Result<(), ErrorType> {
            let number = self.counter.iter().sum::<usize>();
            println!("Total number of spectra are: {}. Total number of electrons are: {}. Electrons / spectra is {}. First electron detected at {:?}", self.spectra.len(), number, number / self.spectra.len(), self.initial_time);
            Ok(())
        }
    }

    
    impl TimeSpectral {

        pub fn new(interval: usize, xmin: usize, xmax: usize, folder: String) -> Result<Self, ErrorType> {
            if xmax>1024 {return Err(ErrorType::OutOfBounds)}
            Ok(Self {
                spectra: Vec::new(),
                interval: interval,
                counter: Vec::new(),
                initial_time: None,
                min: xmin,
                max: xmax,
                folder: folder,
            })
        }
    }

    /// This enables spatial+spectral analysis in a certain spectral window.
    pub struct TimeSpectralSpatial {
        pub spectra: Vec<Vec<usize>>,
        pub initial_time: Option<f64>,
        pub cycle_counter: f64, //Electron overflow counter
        pub cycle_trigger: bool, //Electron overflow control
        pub interval: usize, //time interval you want to form spims
        pub counter: Vec<usize>,
        pub min: usize,
        pub max: usize,
        pub folder: String,
        pub spimx: usize,
        pub spimy: usize,
        pub scanx: Option<usize>,
        pub scany: Option<usize>,
        pub line_offset: usize,
        pub is_image: bool,
        pub is_spim: bool,
        pub spec_bin: Option<usize>,
        pub tdc_periodic: Option<PeriodicTdcRef>,
        pub tdc_type: TdcType,
    }
    
    impl TimeTypes for TimeSpectralSpatial {
        fn prepare(&mut self, file: &mut fs::File) {
            self.tdc_periodic = match self.tdc_periodic {
                None => {
                    let val = Some(PeriodicTdcRef::new(self.tdc_type, file));
                    val
                },
                Some(val) => Some(val),
            };
        }
        
        fn add_packet(&mut self, packet: &Pack) {
            //Getting Initial Time
            self.initial_time = match self.initial_time {
                Some(t) => {Some(t)},
                None => {Some(packet.electron_time())},
            };

            //Correcting Electron Time
            let el = packet.electron_time();
            if el > 26.7 && self.cycle_trigger {
                self.cycle_counter += 1.0;
                self.cycle_trigger = false;
            }
            else if el > 0.1 && packet.electron_time() < 13.0 && !self.cycle_trigger {
                self.cycle_trigger = true;
            }
            let corrected_el = if !self.cycle_trigger && (el + self.cycle_counter * Pack::electron_reset_time()) > ((0.5 + self.cycle_counter) * Pack::electron_reset_time()) {
                el
            } else {
                el + self.cycle_counter * Pack::electron_reset_time()
            };

            //Creating the array using the electron corrected time. Note that you dont need to use
            //it in the 'spim_detector' if you synchronize the clocks.
            if let Some(offset) = self.initial_time {
                let vec_index = ((corrected_el-offset) * 1.0e9) as usize / self.interval;
                while self.spectra.len() < vec_index+1 {
                    self.expand_data();
                    self.counter.push(0);
                }
                match self.spim_detector(packet.electron_time() - VIDEO_TIME) {
                    Some(array_pos) if packet.x()>self.min && packet.x()<self.max => {
                        self.append_electron(vec_index, array_pos, packet.x());
                        self.counter[vec_index] += 1;
                    },
                    _ => {},
                };
            }
        }

        fn add_tdc(&mut self, packet: &Pack) {
            //Synchronizing clocks using two different approaches. It is always better to use a
            //multiple of 2 and use the FPGA counter.
            match &mut self.tdc_periodic {
                Some(my_tdc_periodic) if packet.tdc_type() == self.tdc_type.associate_value() => {
                    my_tdc_periodic.upt(packet.tdc_time_norm(), packet.tdc_counter());
                    if  (my_tdc_periodic.counter / 2) % (self.spimy) == 0 {
                        my_tdc_periodic.begin_frame = my_tdc_periodic.time();
                    }
                },
                _ => {},
            };
        }

        fn output(&self) -> Result<(), ErrorType> {
            if let Err(_) = fs::read_dir(&self.folder) {
                if let Err(_) = fs::create_dir(&self.folder) {
                    return Err(ErrorType::FolderNotCreated);
                }
            }
            
            let mut folder: String = String::from(&self.folder);
            folder.push_str("\\");
            folder.push_str(&(self.spectra.len()).to_string());
            folder.push_str("_");
            folder.push_str(&self.min.to_string());
            folder.push_str("_");
            folder.push_str(&self.max.to_string());
            if !self.is_image && !self.is_spim {
                folder.push_str("_");
                folder.push_str(&self.scanx.unwrap().to_string());
                folder.push_str("_");
                folder.push_str(&self.scany.unwrap().to_string());
                folder.push_str("_");
                folder.push_str(&self.spec_bin.unwrap().to_string());
            } else {
                if self.is_image {folder.push_str("_spim");}
                else {folder.push_str("_spimComplete");}
            }


            let out = self.spectra.iter().flatten().map(|x| x.to_string()).collect::<Vec<String>>().join(", ");
            if let Err(_) = fs::write(&folder, out) {
                return Err(ErrorType::FolderDoesNotExist);
            }
         
            if !self.is_image && !self.is_spim {
                folder.push_str("_");
                folder.push_str("counter");
                let out = self.counter.iter().map(|x| x.to_string()).collect::<Vec<String>>().join(", ");
                if let Err(_) = fs::write(folder, out) {
                    return Err(ErrorType::FolderDoesNotExist);
                }
            }

            Ok(())
        }

        fn display_info(&self) -> Result<(), ErrorType> {
            let number = self.counter.iter().sum::<usize>();
            println!("Total number of spims are: {}. Total number of electrons are: {}. Electrons / spim are {}. First electron detected at {:?}. TDC period (us) is {}. TDC low time (us) is {}. Output is image: {}. Scanx, Scany and Spec_bin is {:?}, {:?} and {:?} (must be all None is is_image). Is a complete spim: {}.", self.spectra.len(), number, number / self.spectra.len(), self.initial_time, self.tdc_periodic.expect("TDC periodic is None during display_info.").period*1e6, self.tdc_periodic.expect("TDC periodic is None during display_info.").low_time*1e6, self.is_image, self.scanx, self.scany, self.spec_bin, self.is_spim);
            Ok(())
        }
    }
    
    impl TimeSpectralSpatial {

        pub fn new(interval: usize, xmin: usize, xmax: usize, spimx: usize, spimy: usize, lineoffset: usize, scan_parameters: Option<(usize, usize, usize)>, tdc_type: TdcType, folder: String) -> Result<Self, ErrorType> {
            if xmax>1024 {return Err(ErrorType::OutOfBounds)};
            if xmin>xmax {return Err(ErrorType::MinGreaterThanMax)};
            let (is_image, is_spim) = match scan_parameters {
                None if (xmin==0 && xmax==1024)  => (false, true),
                Some(_) => (false, false),
                _ => (true, false),
            };
            
            let (scanx, scany, spec_bin) = match scan_parameters {
                Some((x, y, bin)) => {
                    if x>spimx || y>spimy {
                        return Err(ErrorType::ScanOutofBounds)
                    };
                    (Some(x), Some(y), Some(bin))
                },
                None => {
                    (None, None, None)
                },
            };

            Ok(Self {
                spectra: Vec::new(),
                interval: interval,
                counter: Vec::new(),
                initial_time: None,
                cycle_counter: 0.0,
                cycle_trigger: true,
                min: xmin,
                max: xmax,
                spimx: spimx,
                spimy: spimy,
                scanx: scanx,
                scany: scany,
                line_offset: lineoffset,
                is_image: is_image,
                is_spim: is_spim,
                spec_bin: spec_bin,
                folder: folder,
                tdc_periodic: None,
                tdc_type: tdc_type,
            })
        }

        fn spim_detector(&self, ele_time: f64) -> Option<usize> {
            if let Some(tdc_periodic) = self.tdc_periodic {
                let begin = tdc_periodic.begin;
                let interval = tdc_periodic.low_time;
                let period = tdc_periodic.period;
               
                let ratio = (ele_time - begin) / period;
                let ratio_inline = ratio.fract();
                if ratio_inline > interval / period || ratio_inline.is_sign_negative() {
                    None
                } else {
                    let line = (ratio as usize + self.line_offset) % self.spimy;
                    let xpos = (self.spimx as f64 * ratio_inline / (interval / period)) as usize;
                    let result = line * self.spimx + xpos;
                    match (self.scanx, self.scany, self.spec_bin) {
                        (None, None, None) => Some(result),
                        (Some(posx), Some(posy), Some(spec_bin)) if (posx as isize-xpos as isize).abs()<spec_bin as isize && (posy as isize-line as isize).abs()<spec_bin as isize => Some(result),
                        _ => None,
                    }
                }
            } else {None}
        }

        fn expand_data(&mut self) {
            if self.is_spim {
                self.spectra.push(vec![0; self.spimx*self.spimy*1024]);
            } else {
                if self.is_image {
                    self.spectra.push(vec![0; self.spimx*self.spimy]);
                } else {
                    self.spectra.push(vec![0; 1024]);
                }
            }
        }

        fn append_electron(&mut self, vec_index: usize, array_pos: usize, x: usize) {
            if self.is_spim {
                self.spectra[vec_index][array_pos*1024+x] += 1;
            } else {
                if self.is_image {
                    self.spectra[vec_index][array_pos] += 1;
                } else {
                    self.spectra[vec_index][x] += 1;
                }
            }
        }
    }



    pub fn analyze_data(file: &str, data: &mut TimeSet) {

        for each in data.set.iter_mut() {
            let mut file = fs::File::open(file).expect("Could not open desired file.");
            each.prepare(&mut file);
        }

        let mut file = fs::File::open(file).expect("Could not open desired file.");
        let mut buffer: Vec<u8> = Vec::new();
        file.read_to_end(&mut buffer).expect("Could not write file on buffer.");

        let mut ci = 0usize;
        let mut packet_chunks = buffer.chunks_exact(8);

        while let Some(pack_oct) = packet_chunks.next() {
            match pack_oct {
                &[84, 80, 88, 51, nci, _, _, _] => {ci = nci as usize},
                _ => {
                    let packet = Pack{chip_index: ci, data: pack_oct};
                    match packet.id() {
                        6 => {
                            for each in data.set.iter_mut() {
                                each.add_tdc(&packet);
                            }
                        },
                        11 => {
                            for each in data.set.iter_mut() {
                                each.add_packet(&packet);
                            }
                        },
                        _ => {},
                    };
                },
            };
        };
    }
}
*/
