use core::str;
use crate::ata;
use crate::{print, println};
use crate::KERNEL_SIZE;

const SUPERBLOCK_ADDR: u32 = (KERNEL_SIZE / ata::BLOCK_SIZE) as u32;
const SIGNATURE: &[u8; 8] = b"COS FSYS";

struct SuperBlock<'a> {
    signature: &'a [u8; 8],
}

impl SuperBlock<'_> {
    fn new() -> Self {
        Self {
            signature: SIGNATURE,
        }
    }

    /// Checks if the drive is already formatted and contains the signature
    fn check_ata(bus: u8, dsk: u8) -> bool {
        let mut buf = [0u8; ata::BLOCK_SIZE];
        if ata::read_ata(bus, dsk, SUPERBLOCK_ADDR, &mut buf).is_err() {
            println!("FS: signature is \"{}\" ({:?}), looking for \"{}\"",
                    str::from_utf8(&buf[0..8]).unwrap(),
                    &buf[0..8],
                    str::from_utf8(SIGNATURE).unwrap());
            return false;
        }
        &buf[0..8] == SIGNATURE
    }

    /// Writes superblock data to the drive
    fn write(&self, bus: u8, dsk: u8) -> Result<(), ()> {
        let mut buf = [0u8; ata::BLOCK_SIZE];

        buf[..8].clone_from_slice(self.signature);

        ata::write_ata(bus, dsk, SUPERBLOCK_ADDR, &buf)?;
        Ok(())
    }
}

/// Sets up the COS Filesystem on a drive
/// Call if a drive is unformatted
fn format_ata(drive: &ata::Drive) {
    let sb = SuperBlock::new();
    sb.write(drive.bus, drive.dsk).expect("FS: Failed to write super block");
    println!("FS: Wrote super block");
}

/// Initializes the COS Filesystem for a mounted drive
pub fn init(drive: &ata::Drive) {
    println!("FS: Initializing the filesystem");

    println!("FS: Checking ATA...");
    let check = SuperBlock::check_ata(drive.bus, drive.dsk);

    if !check {
        println!("FS: Drive not formatted. Formatting...");
        format_ata(drive);
    } else {
        println!("FS: Drive is ok. No need to format");
    }

}
