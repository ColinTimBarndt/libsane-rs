fn main() -> Result<(), libsane::Error> {
    let (sane, version) = libsane::Sane::init_no_auth()?;

    println!("Version: {version}");
    println!("Lib Version: {}", libsane::LIB_VERSION);

    let devices = sane.get_devices_as_boxed_slice(true)?;

    println!("{devices:#?}");

    Ok(())
}
