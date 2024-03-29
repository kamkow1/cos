use bit_field::BitField;
use lazy_static::lazy_static;
use spin::Mutex;
use x86_64::instructions::port::{Port, PortReadOnly, PortWriteOnly};
use core::hint::spin_loop;
use core::convert::TryInto;
use core::fmt::Debug;
use alloc::vec::Vec;
use alloc::string::String;
use crate::println;

pub const BLOCK_SIZE: usize = 512;

#[derive(Debug)]
#[repr(u16)]
enum Command {
    Read = 0x20,
    Write = 0x30,
    Identify = 0xEC,
}

enum IdentifyResponse {
    Ata([u16; 256]),
    Atapi,
    Sata,
    None,
}

#[repr(usize)]
#[derive(Clone, Copy)]
enum Status {
    Error = 0,
    DataRequest = 3,
    DeviceReady = 6,
    Busy = 7,
}

pub struct Bus {
    id: u8,
    irq: u8,
    data: Port<u16>,
    error: PortReadOnly<u8>,
    features: PortWriteOnly<u8>,
    sector_count: Port<u8>,
    lba_low: Port<u8>,
    lba_mid: Port<u8>,
    lba_high: Port<u8>,
    drive: Port<u8>,
    status: PortReadOnly<u8>,
    command: PortWriteOnly<u8>,
    alt_status: PortReadOnly<u8>,
    control: PortWriteOnly<u8>,
    drive_addr: PortReadOnly<u8>,
}

impl Bus {
    pub fn new(id: u8, io_base: u16, ctrl_base: u16, irq: u8) -> Self {
        Self {
            id,
            irq,
            data: Port::new(io_base),
            error: PortReadOnly::new(io_base + 1),
            features: PortWriteOnly::new(io_base + 1),
            sector_count: Port::new(io_base + 2),
            lba_low: Port::new(io_base + 3),
            lba_mid: Port::new(io_base + 4),
            lba_high: Port::new(io_base + 5),
            drive: Port::new(io_base + 6),
            command: PortWriteOnly::new(io_base + 7),
            status: PortReadOnly::new(io_base + 7),
            alt_status: PortReadOnly::new(ctrl_base),
            control: PortWriteOnly::new(ctrl_base),
            drive_addr: PortReadOnly::new(ctrl_base + 1),
        }
    }

    fn status(&mut self) -> u8 {
        unsafe { self.alt_status.read() }
    }

    fn check_floating_bus(&mut self) -> Result<(), ()> {
        match self.status() {
            0xFF | 0x7F => Err(()),
            _ => Ok(()),
        }
    }

    fn clear_interrupt(&mut self) -> u8 {
        unsafe { self.status.read() }
    }

    fn lba_mid(&mut self) -> u8 {
        unsafe { self.lba_mid.read() }
    }

    fn lba_high(&mut self) -> u8 {
        unsafe { self.lba_high.read() }
    }

    fn read_data(&mut self) -> u16 {
        unsafe { self.data.read() }
    }

    fn write_data(&mut self, data: u16) {
        unsafe { self.data.write(data) }
    }

    fn is_error(&mut self) -> bool {
        self.status().get_bit(Status::Error as usize)
    }

    fn poll(&mut self, bit: Status, value: bool) -> Result<(), ()> {
        while self.status().get_bit(bit as usize) != value {
            spin_loop();
        }
        Ok(())
    }

    fn select_drive(&mut self, drive: u8) -> Result<(), ()> {
        self.poll(Status::Busy, false)?;
        self.poll(Status::DataRequest, false)?;
        unsafe {
            // bit 4 -> device
            // bit 5 -> 1
            // bit 7 -> 1
            self.drive.write(0xA0 | (drive << 4))
        }
        self.poll(Status::Busy, false)?;
        self.poll(Status::DataRequest, false)?;
        Ok(())
    }

    fn write_command_args(&mut self, drive: u8, block: u32) -> Result<(), ()> {
        let lba = true;
        let mut bytes = block.to_le_bytes();
        bytes[3].set_bit(4, drive > 0);
        bytes[3].set_bit(5, true);
        bytes[3].set_bit(6, lba);
        bytes[3].set_bit(7, true);
        unsafe {
            self.sector_count.write(1);
            self.lba_low.write(bytes[0]);
            self.lba_mid.write(bytes[1]);
            self.lba_high.write(bytes[2]);
            self.drive.write(bytes[3]);
        }
        Ok(())
    }

    fn write_command(&mut self, cmd: Command) -> Result<(), ()> {
        unsafe { self.command.write(cmd as u8) }
        self.status();
        self.clear_interrupt();
        if self.status() == 0 {
            return Err(())
        }
        if self.is_error() {
            return Err(())
        }
        self.poll(Status::Busy, false)?;
        self.poll(Status::DataRequest, true)?;
        Ok(())
    }

    fn setup_pio(&mut self, drive: u8, block: u32) -> Result<(), ()> {
        self.select_drive(drive)?;
        self.write_command_args(drive, block)?;
        Ok(())
    }

    fn read(&mut self, drive: u8, block: u32, buf: &mut [u8]) -> Result<(), ()> {
        self.setup_pio(drive, block)?;
        self.write_command(Command::Read)?;
        for chunk in buf.chunks_mut(2) {
            let data = self.read_data().to_le_bytes();
            chunk.clone_from_slice(&data);
        }
        if self.is_error() {
            Err(())
        } else {
            Ok(())
        }
    }

    fn write(&mut self, drive: u8, block: u32, buf: &[u8]) -> Result<(), ()> {
        self.setup_pio(drive, block)?;
        self.write_command(Command::Write)?;
        for chunk in buf.chunks(2) {
            let data = u16::from_le_bytes(chunk.try_into().unwrap());
            self.write_data(data);
        }
        if self.is_error() {
            Err(())
        } else {
            Ok(())
        }
    }

    fn identify_drive(&mut self, drive: u8) -> Result<IdentifyResponse, ()> {
        if self.check_floating_bus().is_err() {
            return Ok(IdentifyResponse::None);
        }
        self.select_drive(drive)?;
        self.write_command_args(drive, 0)?;
        if self.write_command(Command::Identify).is_err() {
            if self.status() == 0 {
                return Ok(IdentifyResponse::None);
            } else {
                return Err(());
            }
        }
        match (self.lba_mid(), self.lba_high()) {
            (0x00, 0x00) => Ok(IdentifyResponse::Ata([(); 256].map(|_| self.read_data()))),
            (0x14, 0xEB) => Ok(IdentifyResponse::Atapi),
            (0x3C, 0xC3) => Ok(IdentifyResponse::Sata),
            (_, _) => Err(()),
        }
    }

    fn reset(&mut self) {
        unsafe {
            self.control.write(4); // srst
            self.control.write(0);
        }
    }
}

lazy_static! {
    pub static ref BUSES: Mutex<Vec<Bus>> = Mutex::new(Vec::new());
}

#[derive(Clone, Debug)]
pub struct Drive {
    pub bus: u8,
    pub dsk: u8,
    blocks: u32,
    model: String,
    serial: String,
}

impl Drive {
    pub fn open(bus: u8, dsk: u8) -> Option<Self> {
        let mut buses = BUSES.lock();
        if let Ok(IdentifyResponse::Ata(res)) = buses[bus as usize].identify_drive(dsk) {
            let buf = res.map(u16::to_be_bytes).concat();
            let serial = String::from_utf8_lossy(&buf[20..40]).trim().into();
            let model = String::from_utf8_lossy(&buf[54..94]).trim().into();
            let blocks = u32::from_be_bytes(buf[120..124].try_into().unwrap()).rotate_left(16);
            Some(Self { bus, dsk, model, serial, blocks })
        } else {
            None
        }
    }

    pub const fn block_size(&self) -> u32 {
        BLOCK_SIZE as u32
    }

    pub fn block_count(&self) -> u32 {
        self.blocks
    }

    fn human_readable_size(&self) -> (usize, String) {
        let size = self.block_size() as usize;
        let count = self.block_count() as usize;
        let bytes = size * count;
        if bytes >> 20 < 1000 {
            (bytes >> 20, String::from("MB"))
        } else {
            (bytes >> 30, String::from("GB"))
        }
    }
}

pub fn list_drives() -> Vec<Drive> {
    let mut res = Vec::new();
    for bus in 0..2 {
        for dsk in 0..2 {
            if let Some(drive) = Drive::open(bus, dsk) {
                res.push(drive)
            }
        }
    }
    res
}

pub fn read_ata(bus: u8, drive: u8, block: u32, buf: &mut [u8]) -> Result<(), ()> {
    let mut buses = BUSES.lock();
    buses[bus as usize].read(drive, block, buf)
}

pub fn write_ata(bus: u8, drive: u8, block: u32, buf: &[u8]) -> Result<(), ()> {
    let mut buses = BUSES.lock();
    buses[bus as usize].write(drive, block, buf)
}

lazy_static! {
    pub static ref DRIVES: Mutex<Vec<Drive>> = Mutex::new(Vec::new());
}

pub fn init() {
    {
        let mut buses = BUSES.lock();
        buses.push(Bus::new(0, 0x1F0, 0x3F6, 14));
        buses.push(Bus::new(1, 0x170, 0x376, 15));
    }

    *DRIVES.lock() = list_drives();

    for drive in DRIVES.lock().iter() {
        if drive.dsk == 0 {
            println!("ATA: Bootable drive: {:#?}", drive);
        } else {
            println!("ATA: found drive: {:#?}", drive);
        }
    }
}
