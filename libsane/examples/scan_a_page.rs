use std::io::{BufRead, BufReader, Write};

use libsane::scan::{DecodedImage, DecodedImageFormat, FrameDecoder};

const OUTPUT_FILE: &str = "./page.pam";

/// This example prompts for a scanner device to be used and attempts to scan
/// one page. The file will be saved at `./page.pam` in [Netpbm PAM] file format.
/// PAM is a very simple uncompressed format that doesn't need an external library
/// to be encoded, keeping this code simple.
///
/// To convert this file, you could use FFMPEG as follows:
/// ```sh
/// ffmpeg -i page.pam page.png
/// ```
///
/// [Netpbm PAM]: https://netpbm.sourceforge.net/doc/pam.html#visual
fn main() -> Result<(), libsane::Error> {
    let (sane, version) = libsane::Sane::init_no_auth()?;

    println!("Version: {version}");
    println!("Lib Version: {}", libsane::LIB_VERSION);

    let devices = sane.get_devices_as_boxed_slice(true)?;
    let device_info = ask_for_device(&devices);

    println!("Scanning with device {}", device_info.name());

    let device = sane.connect(device_info.name())?;
    let mut reader = device.scan_blocking();
    let mut buf = Vec::new();

    let mut decoder = FrameDecoder::builder()
        .decode_black_and_white_as_bytes(true)
        .build();

    while let Some(mut frame_reader) = reader.next_frame()? {
        buf.clear();
        frame_reader.read_full_frame(&mut buf)?;
        println!("Read frame: {:#?}", frame_reader.parameters());
        println!("Frame has {} bytes", buf.len());

        decoder
            .write(&buf, frame_reader.parameters())
            .expect("invalid data");
    }

    let image = decoder
        .into_image()
        .expect("not all necessary frames were received");

    if let Err(err) = write_pam_image(&image, OUTPUT_FILE) {
        println!("Failed to write image to {OUTPUT_FILE}: {err}");
    }

    Ok(())
}

fn write_pam_image(image: &DecodedImage, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
    let mut out_file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;

    let (depth, pam_tupletype, maxval) = match image.format {
        DecodedImageFormat::BlackAndWhite => (1, "BLACKANDWHITE", 1u32),
        DecodedImageFormat::Gray { bytes_per_pixel } => {
            (1, "GRAYSCALE", 1 << (bytes_per_pixel * 8))
        }
        DecodedImageFormat::Rgb { bytes_per_channel } => (3, "RGB", 1 << (bytes_per_channel * 8)),
    };
    write!(
        out_file,
        "P7\n\
				WIDTH {width}\n\
				HEIGHT {height}\n\
				DEPTH {depth}\n\
				MAXVAL {maxval}\n\
				TUPLETYPE {pam_tupletype}\n\
				ENDHDR\n",
        width = image.width,
        height = image.height,
    )?;

    out_file.write_all(&image.data)
}

fn ask_for_device(devices: &[libsane::DeviceDescription]) -> &libsane::DeviceDescription {
    if devices.is_empty() {
        println!("No devices available.");
        std::process::exit(0);
    }

    println!("Pick a device from the list:");
    for (i, dev) in devices.iter().enumerate() {
        println!("{}. {} ({})", i + 1, dev.model(), dev.name());
    }
    loop {
        let input = prompt("Device number: ");
        match input.parse() {
            Ok(n) if (1..=devices.len()).contains(&n) => break &devices[n - 1],
            Ok(_) => println!("Not a device. Try again."),
            Err(_) => println!("Not a number. Try again."),
        }
    }
}

fn prompt(msg: &str) -> String {
    print!("{}", msg);
    std::io::stdout().flush().unwrap();
    let line = BufReader::new(std::io::stdin())
        .lines()
        .next()
        .expect("stdin closed");
    line.unwrap()
}
