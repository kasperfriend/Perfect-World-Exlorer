#[cfg(windows)]
fn main() {
    use rodio::{Decoder, Source};
    use std::{fs::File, io::BufReader};
    for p in std::env::args_os().skip(1) {
        let d = Decoder::new(BufReader::new(File::open(&p).unwrap())).unwrap();
        let rate = d.sample_rate();
        let channels = d.channels();
        let n = d.take(44100).count();
        println!("{:?}: {rate} Hz {channels} channels {n} decoded samples", p);
    }
    match rodio::OutputStream::try_default() {
        Ok((_s, _h)) => println!("Default audio device available"),
        Err(e) => println!("No audio output device: {e}"),
    }
}
#[cfg(not(windows))]
fn main() {
    println!("Windows audio test only");
}
