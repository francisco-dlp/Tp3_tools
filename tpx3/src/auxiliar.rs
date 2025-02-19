//!`auxiliar` is a collection of tools to set acquisition conditions.
use crate::errorlib::Tp3ErrorKind;
use std::net::{TcpListener, TcpStream, SocketAddr};
use std::io::{Read, Write};
use std::fs::File;
//use std::{fs::{File, OpenOptions, create_dir_all}, path::Path};

const CONFIG_SIZE: usize = 16;

///Configures the detector for acquisition. Each new measurement must send 20 bytes
///containing instructions.
struct BytesConfig {
    pub data: [u8; CONFIG_SIZE],
}

impl BytesConfig {
    ///Set binning mode for 1x4 detector. `\x00` for unbinned and `\x01` for binned. Panics otherwise. Byte[0].
    fn bin(&self) -> Result<bool, Tp3ErrorKind> {
        match self.data[0] {
            0 => {
                println!("Bin is False.");
                Ok(false)
            },
            1 => {
                println!("Bin is True.");
                Ok(true)
            },
            _ => Err(Tp3ErrorKind::SetBin),
        }
    }

    ///Set bytedepth. `\x00` for 1, `\x01` for 2 and `\x02` for 4. Panics otherwise. Byte[1].
    fn bytedepth(&self) -> Result<usize, Tp3ErrorKind> {
        match self.data[1] {
            0 => {
                println!("Bitdepth is 8.");
                Ok(1)
            },
            1 => {
                println!("Bitdepth is 16.");
                Ok(2)
            },
            2 => {
                println!("Bitdepth is 32.");
                Ok(4)
            },
            _ => Err(Tp3ErrorKind::SetByteDepth),
        }
    }

    ///Sums all arriving data. `\x00` for False, `\x01` for True. Panics otherwise. Byte[2].
    fn cumul(&self) -> Result<bool, Tp3ErrorKind> {
        match self.data[2] {
            0 => {
                println!("Cumulation mode is OFF.");
                Ok(false)
            },
            1 => {
                println!("Cumulation mode is ON.");
                Ok(true)
            },
            _ => Err(Tp3ErrorKind::SetCumul),
        }
    }

    ///Acquisition Mode. `\x00` for normal, `\x01` for spectral image and `\x02` for time-resolved. Panics otherwise. Byte[2..4].
    fn mode(&self) -> Result<u8, Tp3ErrorKind> {
        println!("Mode is: {}", self.data[3]);
        Ok(self.data[3])
        
        /*match self.data[3] {
            0 => {
                println!("Mode is Focus/Cumul.");
                Ok(self.data[3])
            },
            1 => {
                println!("Entering in time resolved mode (Focus/Cumul).");
                Ok(self.data[3])
            },
            2 => {
                println!("Entering in Spectral Image (SpimTP).");
                Ok(self.data[3])
            },
            3 => {
                println!("Entering in time resolved mode (SpimTP).");
                Ok(self.data[3])
            },
            4 => {
                println!("Entering in Spectral Image [TDC Mode] (SpimTP).");
                Ok(self.data[3])
            },
            5 => {
                println!("Entering in Spectral Image [Save Locally] (SpimTP).");
                Ok(self.data[3])
            },
            6 => {
                println!("Entering in Chrono Mode.");
                Ok(self.data[3])
            },
            _ => Err(Tp3ErrorKind::SetMode),
        }*/
    }


    ///X spim size. Must be sent with 2 bytes in big-endian mode. Byte[4..6]
    fn xspim_size(&self) -> usize {
        let x = (self.data[4] as usize)<<8 | (self.data[5] as usize);
        println!("X Spim size is: {}.", x);
        x
    }
    
    ///Y spim size. Must be sent with 2 bytes in big-endian mode. Byte[6..8]
    fn yspim_size(&self) -> usize {
        let y = (self.data[6] as usize)<<8 | (self.data[7] as usize);
        println!("Y Spim size is: {}.", y);
        y
    }
    
    ///X scan size. Must be sent with 2 bytes in big-endian mode. Byte[8..10]
    fn xscan_size(&self) -> usize {
        let x = (self.data[8] as usize)<<8 | (self.data[9] as usize);
        println!("X Scan size is: {}.", x);
        x
    }
    
    ///Y scan size. Must be sent with 2 bytes in big-endian mode. Byte[10..12]
    fn yscan_size(&self) -> usize {
        let y = (self.data[10] as usize)<<8 | (self.data[11] as usize);
        println!("Y Scan size is: {}.", y);
        y
    }
    
    ///Time delay. Must be sent with 2 bytes in big-endian mode. Byte[12..14]
    fn time_delay(&self) -> usize {
        let td = (self.data[12] as usize)<<8 | (self.data[13] as usize);
        println!("Time delay is (ns): {}.", td);
        td
    }
    
    ///Time width. Must be sent with 2 bytes in big-endian mode. Byte[14..16].
    fn time_width(&self) -> usize {
        let tw = (self.data[14] as usize)<<8 | (self.data[15] as usize);
        println!("Time delay is (ns): {}.", tw);
        tw
    }
    
    ///Convenience method. Returns the ratio between scan and spim size in X.
    fn spimoverscanx(&self) -> Result<usize, Tp3ErrorKind> {
        let xspim = (self.data[4] as usize)<<8 | (self.data[5] as usize);
        let xscan = (self.data[8] as usize)<<8 | (self.data[9] as usize);
        if xspim == 0 {return Err(Tp3ErrorKind::SetXSize);}
        let var = xscan / xspim;
        match var {
            0 => {
                println!("Xratio is: 1.");
                Ok(1)
            },
            _ => {
                println!("Xratio is: {}.", var);
                Ok(var)
            },
        }
    }
    
    ///Convenience method. Returns the ratio between scan and spim size in Y.
    fn spimoverscany(&self) -> Result<usize, Tp3ErrorKind> {
        let yspim = (self.data[6] as usize)<<8 | (self.data[7] as usize);
        let yscan = (self.data[10] as usize)<<8 | (self.data[11] as usize);
        if yspim == 0 {return Err(Tp3ErrorKind::SetYSize);}
        let var = yscan / yspim;
        match var {
            0 => {
                println!("Yratio is: 1.");
                Ok(1)
            },
            _ => {
                println!("Yratio is: {}.", var);
                Ok(var)
            },
        }
    }


    ///Create Settings struct from BytesConfig
    fn create_settings(&self) -> Result<Settings, Tp3ErrorKind> {
        let my_set = Settings {
            bin: self.bin()?,
            bytedepth: self.bytedepth()?,
            cumul: self.cumul()?,
            mode: self.mode()?,
            xspim_size: self.xspim_size(),
            yspim_size: self.yspim_size(),
            xscan_size: self.xscan_size(),
            yscan_size: self.yscan_size(),
            time_delay: self.time_delay(),
            time_width: self.time_width(),
            spimoverscanx: self.spimoverscanx()?,
            spimoverscany: self.spimoverscany()?,
        };
        Ok(my_set)
    }

}


struct DebugIO {}
impl Write for DebugIO {
    fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
        Ok(0)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}


///`Settings` contains all relevant parameters for a given acquistion
#[derive(Copy, Clone, Debug)]
pub struct Settings {
    pub bin: bool,
    pub bytedepth: usize,
    pub cumul: bool,
    pub mode: u8,
    pub xspim_size: usize,
    pub yspim_size: usize,
    pub xscan_size: usize,
    pub yscan_size: usize,
    pub time_delay: usize,
    pub time_width: usize,
    pub spimoverscanx: usize,
    pub spimoverscany: usize,
}

impl Settings {

    ///Create Settings structure reading from a TCP.
    pub fn create_settings(host_computer: [u8; 4], port: u16) -> Result<(Settings, Box<dyn misc::TimepixRead + Send>, Box<dyn Write + Send>), Tp3ErrorKind> {
    
        let mut _sock_vec: Vec<TcpStream> = Vec::new();
        
        let addrs = [
            SocketAddr::from((host_computer, port)),
            SocketAddr::from(([127, 0, 0, 1], port)),
        ];
        
        let pack_listener = TcpListener::bind("127.0.0.1:8098").expect("Could not bind to TP3.");
        let ns_listener = TcpListener::bind(&addrs[..]).expect("Could not bind to NS.");
        println!("Packet Tcp socket connected at: {:?}", pack_listener);
        println!("Nionswift Tcp socket connected at: {:?}", ns_listener);

        let debug: bool = match ns_listener.local_addr() {
            Ok(val) if val == addrs[1] => true,
            _ => false,
        };

        let (mut ns_sock, ns_addr) = ns_listener.accept().expect("Could not connect to Nionswift.");
        println!("Nionswift connected at {:?} and {:?}.", ns_addr, ns_sock);
        
        let mut cam_settings = [0_u8; CONFIG_SIZE];
        let my_config = {
            match ns_sock.read(&mut cam_settings){
                Ok(size) => {
                    println!("Received {} bytes from NS.", size);
                    BytesConfig{data: cam_settings}
                },
                Err(_) => panic!("Could not read cam initial settings."),
            }
        };
        let my_settings = my_config.create_settings()?;
        println!("Received settings is {:?}. Mode is {}.", cam_settings, my_settings.mode);

        match debug {
            false => {
                let (pack_sock, packet_addr) = pack_listener.accept().expect("Could not connect to TP3.");
                println!("Localhost TP3 detected at {:?} and {:?}.", packet_addr, pack_sock);
                Ok((my_settings, Box::new(pack_sock), Box::new(ns_sock)))
            },
            true => {
                let file = match File::open("bin/Data/raw000000.tpx3") {
                    Ok(file) => file,
                    Err(_) => return Err(Tp3ErrorKind::SetNoReadFile),
                };
                println!("Debug mode. Will one file a single time.");
                Ok((my_settings, Box::new(file), Box::new(ns_sock)))
            },
        }

    }

    fn create_spec_debug_settings(_config: &ConfigAcquisition) -> Settings  {
        Settings {
            bin: false,
            bytedepth: 4,
            cumul: false,
            mode: 00,
            xspim_size: 512,
            yspim_size: 512,
            xscan_size: 512,
            yscan_size: 512,
            time_delay: 0,
            time_width: 1000,
            spimoverscanx: 1,
            spimoverscany: 1,
        }
    }
    
    fn create_spim_debug_settings(config: &ConfigAcquisition) -> Settings  {
        Settings {
            bin: true,
            bytedepth: 4,
            cumul: false,
            mode: 2,
            xspim_size: config.xspim,
            yspim_size: config.yspim,
            xscan_size: config.xspim,
            yscan_size: config.yspim,
            time_delay: 0,
            time_width: 1000,
            spimoverscanx: 1,
            spimoverscany: 1,
        }
    }

    
    pub fn create_debug_settings(config: &ConfigAcquisition) -> Result<(Settings, Box<dyn misc::TimepixRead + Send>, Box<dyn Write + Send>), Tp3ErrorKind> {
    
        let my_settings = match config.is_spim {
            true => Settings::create_spim_debug_settings(config),
            false => Settings::create_spec_debug_settings(config),
        };
        
        println!("Received settings is {:?}. Mode is {}.", my_settings, my_settings.mode);

        let in_file = match File::open(&config.file) {
            Ok(file) => file,
            Err(_) => return Err(Tp3ErrorKind::SetNoReadFile),
        };

        println!("Spectra Debug mode. Will one file a single time.");
        Ok((my_settings, Box::new(in_file), Box::new(DebugIO{})))
    }
    
}

///`ConfigAcquisition` is used for post-processing, where reading external TPX3 files is necessary.
#[derive(Debug)]
pub struct ConfigAcquisition {
    pub file: String,
    pub is_spim: bool,
    pub xspim: usize,
    pub yspim: usize,
}

impl ConfigAcquisition {
    pub fn file(&self) -> &str {
        &self.file
    }

    pub fn new(args: &[String]) -> Self {
        if args.len() != 4+1 {
            panic!("One must provide 04 ({} detected) arguments (file, is_spim, xspim, yspim).", args.len()-1);
        }
        let file = args[1].clone();
        let is_spim = args[2] == "1";
        let xspim = args[3].parse::<usize>().unwrap();
        let yspim = args[4].parse::<usize>().unwrap();
        let my_config = 
        ConfigAcquisition {
            file,
            is_spim,
            xspim,
            yspim,
        };
        println!("Configuration for the coincidence measurement is {:?}", my_config);
        my_config
    }
}


///`simple_log` is used for post-processing, where reading external TPX3 files is necessary.
pub mod simple_log {
    use chrono::prelude::*;
    use std::{fs::{File, OpenOptions, create_dir_all}, path::Path};
    use std::io::Write;
    use std::io;
    use crate::errorlib::Tp3ErrorKind;

    pub fn start() -> io::Result<File> {
        let dir = Path::new("Microscope/Log/");
        create_dir_all(&dir)?;
        let date = Local::now().format("%Y-%m-%d").to_string() + ".txt";
        let file_path = dir.join(&date);
        let mut file = OpenOptions::new().write(true).truncate(false).create(true).append(true).open(file_path)?;
        let date = Local::now().to_string();
        file.write_all(date.as_bytes())?;
        file.write_all(b" - Starting new loop\n")?;
        Ok(file)
    }

    pub fn ok(file: &mut File, mode: u8) -> io::Result<()> {
        let date = Local::now().to_string();
        file.write_all(date.as_bytes())?;
        file.write_all(b" - OK ")?;
        let mode = format!("{:?}", mode);
        file.write_all(mode.as_bytes())?;
        file.write_all(b"\n")?;
        Ok(())
    }

    pub fn error(file: &mut File, error: Tp3ErrorKind) -> io::Result<()> {
        let date = Local::now().to_string();
        file.write_all(date.as_bytes())?;
        file.write_all(b" - ERROR ")?;
        let error = format!("{:?}", error);
        file.write_all(error.as_bytes())?;
        file.write_all(b"\n")?;
        Ok(())
    }
}

///`misc` are miscellaneous functions.
pub mod misc {
    use std::io::Read;
    use crate::errorlib::Tp3ErrorKind;
    use std::net::TcpStream;
    use std::fs::File;

    pub fn default_read_exact<R: Read + ?Sized>(this: &mut R, mut buf: &mut [u8]) -> Result<usize, Tp3ErrorKind> {
        let mut size = 0;
        while size == 0 || size % 8 != 0 {
            match this.read(buf) {
                Ok(0) => break,
                Ok(n) => {
                    size += n;
                    let tmp = buf;
                    buf = &mut tmp[n..];
                }
                Err(_) => return Err(Tp3ErrorKind::TimepixReadLoop),
            };
        };
        if size != 0 && size % 8 == 0 {
            Ok(size)
        } else {
            Err(Tp3ErrorKind::TimepixReadOver)
        }
    }

        /*
        while !buf.is_empty() {
            match this.read(buf) {
                Ok(0) => break,
                Ok(n) => {
                    let tmp = buf;
                    buf = &mut tmp[n..];
                }
                Err(_) => return Err(Tp3ErrorKind::TimepixRead),
            };
        };
        if buf.is_empty() {
            Ok(())
        } else {
            Err(Tp3ErrorKind::TimepixRead)
        }
    }
    */
    
    ///A modified `Read` trait. Guarantee to read at least 8 bytes.
    pub trait TimepixRead: Read {
        fn read_timepix(&mut self, buf: &mut [u8]) -> Result<usize, Tp3ErrorKind> {
            default_read_exact(self, buf)
        }
    }

    impl<R: Read + ?Sized> TimepixRead for Box<R> {}
    impl TimepixRead for TcpStream {}
    impl TimepixRead for File {}
}
