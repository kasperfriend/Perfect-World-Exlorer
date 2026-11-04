//! Lossless terrain paging: XOR byte planes on disk, a bounded decoded hot set
//! in RAM. Source floats are never quantized. Navigation and mesh workers share
//! one cache, and cursors pin just their current block outside the hot set.
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};
const DEFAULT_BUDGET: usize = 2 * 1024 * 1024;
static NEXT: AtomicU64 = AtomicU64::new(0);
#[derive(Clone)]
struct Record {
    offset: u64,
    size: usize,
    hash: [u8; 32],
}
struct Page {
    heights: Vec<f32>,
    normals: Vec<[i8; 3]>,
}
impl Page {
    fn bytes(&self) -> usize {
        self.heights.capacity() * 4
            + self.normals.capacity() * 3
            + std::mem::size_of::<Page>()
            + 2 * std::mem::size_of::<usize>()
    }
}
struct Entry {
    page: Arc<Page>,
    used: u64,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub resident_bytes: usize,
    pub peak_bytes: usize,
    pub budget_bytes: usize,
    pub reads: usize,
    pub evictions: usize,
    pub metadata_bytes: usize,
}
struct Hot {
    pages: HashMap<usize, Entry>,
    clock: u64,
    stats: Stats,
}
struct Disk {
    file: File,
    path: PathBuf,
    side: usize,
    records: Vec<Record>,
    hot: Hot,
}
enum Data {
    Memory(Vec<f32>),
    Disk(Mutex<Disk>),
}
struct Store {
    len: usize,
    width: usize,
    side: usize,
    data: Data,
}
#[derive(Clone)]
pub struct Heights(Arc<Store>);
pub struct Normals(Arc<Store>);
pub struct Reader {
    store: Arc<Store>,
    current: Option<(usize, Arc<Page>)>,
}
impl From<Vec<f32>> for Heights {
    fn from(values: Vec<f32>) -> Self {
        Self(Arc::new(Store {
            len: values.len(),
            width: values.len(),
            side: 1,
            data: Data::Memory(values),
        }))
    }
}
impl Heights {
    pub fn reader(&self) -> Reader {
        Reader {
            store: Arc::clone(&self.0),
            current: None,
        }
    }
    pub fn at(&self, index: usize) -> f32 {
        self.reader().height(index)
    }
    pub fn stats(&self) -> Stats {
        match &self.0.data {
            Data::Memory(v) => Stats {
                resident_bytes: v.len() * 4,
                ..Stats::default()
            },
            Data::Disk(d) => {
                let d = d.lock().unwrap();
                Stats {
                    metadata_bytes: d.records.len() * std::mem::size_of::<Record>(),
                    ..d.hot.stats
                }
            }
        }
    }
    /// Release even the hot set after an exhaustive initialization/export scan.
    pub fn trim(&self) {
        if let Data::Disk(d) = &self.0.data {
            let mut d = d.lock().unwrap();
            d.hot.pages.clear();
            d.hot.stats.resident_bytes = 0;
        }
    }
}
impl Normals {
    pub fn at(&self, index: usize) -> [i8; 3] {
        Reader {
            store: Arc::clone(&self.0),
            current: None,
        }
        .normal(index)
    }
}
impl Reader {
    fn page(&mut self, index: usize) -> Result<usize, String> {
        if index >= self.store.len {
            return Err(format!("terrain cell {index} out of bounds"));
        }
        let width = self.store.width;
        let side = self.store.side;
        let x = index % width;
        let y = index / width;
        let id = (y / side) * (width / side) + x / side;
        let local = (y % side) * side + x % side;
        if let Some((last, _)) = &self.current {
            if *last == id {
                return Ok(local);
            }
        }
        let Data::Disk(disk) = &self.store.data else {
            return Err("memory fixture has no disk pages".into());
        };
        let mut d = disk
            .lock()
            .map_err(|_| "terrain page cache lock poisoned")?;
        d.hot.clock += 1;
        let clock = d.hot.clock;
        if let Some(entry) = d.hot.pages.get_mut(&id) {
            entry.used = clock;
            let page = Arc::clone(&entry.page);
            self.current = Some((id, page));
            return Ok(local);
        }
        let record = d.records[id].clone();
        let mut packed = vec![0; record.size];
        d.file
            .seek(SeekFrom::Start(record.offset))
            .and_then(|_| d.file.read_exact(&mut packed))
            .map_err(|e| format!("terrain cache {}: {e}", d.path.display()))?;
        let cells = d.side * d.side;
        let mut bytes = Vec::with_capacity(cells * 7);
        ZlibDecoder::new(packed.as_slice())
            .take((cells * 7 + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("terrain cache block {id}: {e}; restart to rebuild cache"))?;
        if bytes.len() != cells * 7 || <[u8; 32]>::from(Sha256::digest(&bytes)) != record.hash {
            return Err(format!(
                "corrupt terrain cache block {id}; restart to rebuild cache"
            ));
        }
        let page = Arc::new(decode(&bytes, d.side));
        let size = page.bytes();
        while d.hot.stats.resident_bytes + size > d.hot.stats.budget_bytes
            && !d.hot.pages.is_empty()
        {
            let victim = *d.hot.pages.iter().min_by_key(|(_, e)| e.used).unwrap().0;
            let removed = d.hot.pages.remove(&victim).unwrap();
            d.hot.stats.resident_bytes -= removed.page.bytes();
            d.hot.stats.evictions += 1;
        }
        if size <= d.hot.stats.budget_bytes {
            d.hot.pages.insert(
                id,
                Entry {
                    page: Arc::clone(&page),
                    used: clock,
                },
            );
            d.hot.stats.resident_bytes += size;
            d.hot.stats.peak_bytes = d.hot.stats.peak_bytes.max(d.hot.stats.resident_bytes);
        }
        d.hot.stats.reads += 1;
        self.current = Some((id, page));
        Ok(local)
    }
    pub fn cell(&mut self, index: usize) -> Result<(f32, [i8; 3]), String> {
        if let Data::Memory(values) = &self.store.data {
            return values
                .get(index)
                .copied()
                .map(|h| (h, [0, 127, 0]))
                .ok_or_else(|| format!("terrain cell {index} out of bounds"));
        }
        let local = self.page(index)?;
        let page = &self.current.as_ref().unwrap().1;
        Ok((page.heights[local], page.normals[local]))
    }
    pub fn height(&mut self, index: usize) -> f32 {
        if let Data::Memory(values) = &self.store.data {
            return values[index];
        }
        let local = self
            .page(index)
            .expect("lossless terrain backing store read failed");
        self.current.as_ref().unwrap().1.heights[local]
    }
    pub fn normal(&mut self, index: usize) -> [i8; 3] {
        if let Data::Memory(_) = &self.store.data {
            assert!(index < self.store.len);
            return [0, 127, 0];
        }
        let local = self
            .page(index)
            .expect("lossless terrain backing store read failed");
        self.current.as_ref().unwrap().1.normals[local]
    }
}
// Deinterleaving plus row-local XOR exposes equal float exponent bytes and
// correlated normals to the compressor without discarding a single source bit.
fn encode(heights: &[f32], normals: &[[i8; 3]], side: usize) -> Vec<u8> {
    let cells = side * side;
    let mut out = vec![0; cells * 7];
    for plane in 0..7 {
        for y in 0..side {
            let mut previous = 0;
            for x in 0..side {
                let i = y * side + x;
                let value = if plane < 4 {
                    heights[i].to_bits().to_le_bytes()[plane]
                } else {
                    normals[i][plane - 4] as u8
                };
                out[plane * cells + i] = value ^ previous;
                previous = value;
            }
        }
    }
    out
}
fn decode(bytes: &[u8], side: usize) -> Page {
    let cells = side * side;
    let mut raw = bytes.to_vec();
    for plane in 0..7 {
        for y in 0..side {
            let mut previous = 0;
            for x in 0..side {
                let i = plane * cells + y * side + x;
                raw[i] ^= previous;
                previous = raw[i];
            }
        }
    }
    let mut heights = Vec::with_capacity(cells);
    let mut normals = Vec::with_capacity(cells);
    for i in 0..cells {
        heights.push(f32::from_bits(u32::from_le_bytes([
            raw[i],
            raw[cells + i],
            raw[2 * cells + i],
            raw[3 * cells + i],
        ])));
        normals.push([
            raw[4 * cells + i] as i8,
            raw[5 * cells + i] as i8,
            raw[6 * cells + i] as i8,
        ]);
    }
    Page { heights, normals }
}
pub struct Builder {
    file: Option<File>,
    path: PathBuf,
    root: PathBuf,
    width: usize,
    height: usize,
    side: usize,
    records: Vec<Option<Record>>,
    hash: Sha256,
}
impl Builder {
    pub fn new(root: &Path, width: usize, height: usize, side: usize) -> Result<Self, String> {
        if side == 0
            || side > 256
            || width == 0
            || height == 0
            || width % side != 0
            || height % side != 0
            || width
                .checked_mul(height)
                .is_none_or(|cells| cells > 100_000_000)
            || (width / side)
                .checked_mul(height / side)
                .is_none_or(|pages| pages > 1_000_000)
        {
            return Err("invalid paged terrain dimensions".into());
        }
        fs::create_dir_all(root).map_err(|e| format!("terrain cache {}: {e}", root.display()))?;
        let path = root.join(format!(
            "session-{}-{}-{}.tmp",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        let mut header = b"PWTGRID1".to_vec();
        for v in [width, height, side] {
            header.extend_from_slice(&(v as u64).to_le_bytes());
        }
        file.write_all(&header).map_err(|e| e.to_string())?;
        let mut hash = Sha256::new();
        hash.update(&header);
        Ok(Self {
            file: Some(file),
            path,
            root: root.to_path_buf(),
            width,
            height,
            side,
            records: vec![None; (width / side) * (height / side)],
            hash,
        })
    }
    pub fn block(
        &mut self,
        bx: usize,
        by: usize,
        heights: &[f32],
        normals: &[[i8; 3]],
    ) -> Result<(), String> {
        if bx >= self.width / self.side
            || by >= self.height / self.side
            || heights.len() != self.side * self.side
            || normals.len() != heights.len()
        {
            return Err("invalid terrain cache block".into());
        }
        let id = by * (self.width / self.side) + bx;
        if self.records[id].is_some() {
            return Err("duplicate terrain cache block".into());
        }
        let raw = encode(heights, normals, self.side);
        let hash = Sha256::digest(&raw).into();
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(&raw).map_err(|e| e.to_string())?;
        let packed = encoder.finish().map_err(|e| e.to_string())?;
        let file = self.file.as_mut().unwrap();
        let mut header = (id as u64).to_le_bytes().to_vec();
        header.extend_from_slice(&(packed.len() as u64).to_le_bytes());
        file.write_all(&header)
            .and_then(|_| file.write_all(&packed))
            .map_err(|e| format!("terrain cache write: {e}"))?;
        self.hash.update(&header);
        self.hash.update(&packed);
        let offset = file.stream_position().map_err(|e| e.to_string())? - packed.len() as u64;
        self.records[id] = Some(Record {
            offset,
            size: packed.len(),
            hash,
        });
        Ok(())
    }
    pub fn finish(mut self, budget: usize) -> Result<(Heights, Normals), String> {
        if self.records.iter().any(Option::is_none) {
            return Err("incomplete terrain page cache".into());
        }
        let file = self.file.take().unwrap();
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        let digest = self.hash.clone().finalize();
        let name = format!("{:x}.pages", digest);
        let target = self.root.join(name);
        // Never trust existence, timestamps or client filenames as identity.
        // Stream-verify old content before reusing an immutable cache object.
        let valid = File::open(&target).ok().is_some_and(|mut f| {
            let mut hash = Sha256::new();
            let mut buf = [0u8; 65536];
            loop {
                match f.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => hash.update(&buf[..n]),
                    Err(_) => return false,
                }
            }
            hash.finalize() == digest
        });
        if valid {
            fs::remove_file(&self.path).map_err(|e| e.to_string())?;
            self.path = target;
        } else if fs::rename(&self.path, &target).is_ok() {
            self.path = target;
        }
        // On Windows a corrupt old file may be held open by another process.
        // In that case keep our verified new session file, never overwrite it.
        let file = File::open(&self.path).map_err(|e| e.to_string())?;
        let records = std::mem::take(&mut self.records)
            .into_iter()
            .map(Option::unwrap)
            .collect();
        let disk_bytes = file.metadata().map_err(|e| e.to_string())?.len();
        eprintln!("Lossless terrain paging: {} cells, {:.2} MiB source grids -> {:.2} MiB disk; decoded hot-set budget {:.2} MiB (plus one block per reader)",
            self.width*self.height,(self.width*self.height*7) as f64/1048576.,disk_bytes as f64/1048576.,budget as f64/1048576.);
        let store = Arc::new(Store {
            len: self.width * self.height,
            width: self.width,
            side: self.side,
            data: Data::Disk(Mutex::new(Disk {
                file,
                path: self.path.clone(),
                side: self.side,
                records,
                hot: Hot {
                    pages: HashMap::new(),
                    clock: 0,
                    stats: Stats {
                        budget_bytes: budget,
                        ..Stats::default()
                    },
                },
            })),
        });
        Ok((Heights(Arc::clone(&store)), Normals(store)))
    }
}
impl Drop for Builder {
    fn drop(&mut self) {
        if self.file.take().is_some() {
            let _ = fs::remove_file(&self.path);
        }
    }
}
pub fn budget() -> usize {
    std::env::var("PW_TERRAIN_RAM_MIB")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .map(|n| n.clamp(1, 256) * 1024 * 1024)
        .unwrap_or(DEFAULT_BUDGET)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "pw-terrain-pages-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn height(x: usize, y: usize) -> f32 {
        ((x as i32 - 37) * 127 + (y as i32 - 13) * 31) as f32 / 17.
    }
    fn normal(x: usize, y: usize) -> [i8; 3] {
        [x as i8, (x ^ y) as i8, y as i8]
    }
    fn fixture(root: &Path, budget: usize, change: bool) -> (Heights, Normals) {
        let mut b = Builder::new(root, 32, 24, 4).unwrap();
        // Source order need not be the final row-major grid order.
        for bx in (0..8).rev() {
            for by in 0..6 {
                let mut h = Vec::new();
                let mut n = Vec::new();
                for y in 0..4 {
                    for x in 0..4 {
                        h.push(height(bx * 4 + x, by * 4 + y) + if change { 1. } else { 0. });
                        n.push(normal(bx * 4 + x, by * 4 + y));
                    }
                }
                b.block(bx, by, &h, &n).unwrap();
            }
        }
        b.finish(budget).unwrap()
    }
    #[test]
    fn codec_preserves_all_float_and_signed_normal_bits() {
        let bits = [
            0, 0x80000000, 0x7fc12345, 0x7f800000, 0xff800000, 0x3f800001, 0x00000001, 0xffffffff,
        ];
        let h: Vec<_> = (0..16)
            .map(|i| f32::from_bits(bits[i % bits.len()]))
            .collect();
        let n: Vec<_> = (0..16).map(|i| [i as i8 - 8, -128, 127]).collect();
        let p = decode(&encode(&h, &n, 4), 4);
        assert_eq!(
            p.heights.iter().map(|f| f.to_bits()).collect::<Vec<_>>(),
            h.iter().map(|f| f.to_bits()).collect::<Vec<_>>()
        );
        assert_eq!(p.normals, n);
    }
    #[test]
    fn paging_is_exact_across_edges_evictions_and_shared_views() {
        let dir = Temp::new();
        let (h, n) = fixture(&dir.0, 4 * 4 * 7 * 2, false);
        let mut reader = h.reader();
        for pass in 0..3 {
            for i in 0..32 * 24 {
                let index = (i * 173 + pass * 47) % (32 * 24);
                let x = index % 32;
                let y = index / 32;
                assert_eq!(reader.height(index).to_bits(), height(x, y).to_bits());
                assert_eq!(reader.normal(index), normal(x, y));
            }
        }
        assert_eq!(n.at(0), normal(0, 0));
        let stats = h.stats();
        assert!(stats.evictions > 100);
        assert!(stats.reads > 100);
        assert!(stats.peak_bytes <= stats.budget_bytes);
        assert!(stats.resident_bytes <= 224);
        h.trim();
        assert_eq!(h.stats().resident_bytes, 0);
        assert_eq!(h.at(767).to_bits(), height(31, 23).to_bits());
    }
    #[test]
    fn current_block_cursor_does_not_lock_or_read_again() {
        let dir = Temp::new();
        let (h, _) = fixture(&dir.0, 224, false);
        let mut r = h.reader();
        for _ in 0..100 {
            for y in 0..4 {
                for x in 0..4 {
                    r.height(y * 32 + x);
                    r.normal(y * 32 + x);
                }
            }
        }
        assert_eq!(h.stats().reads, 1);
    }
    #[test]
    fn cache_identity_reuse_source_changes_and_corruption_repair() {
        let dir = Temp::new();
        drop(fixture(&dir.0, 224, false));
        let paths = || {
            fs::read_dir(&dir.0)
                .unwrap()
                .map(|p| p.unwrap().path())
                .collect::<Vec<_>>()
        };
        assert_eq!(paths().len(), 1);
        drop(fixture(&dir.0, 224, false));
        assert_eq!(paths().len(), 1);
        let path = paths().pop().unwrap();
        fs::write(&path, b"damaged").unwrap();
        let (h, n) = fixture(&dir.0, 224, false);
        assert_eq!(h.at(0), height(0, 0));
        drop((h, n));
        let (h, _) = fixture(&dir.0, 224, true);
        assert_eq!(h.at(0), height(0, 0) + 1.);
        assert_eq!(paths().len(), 2);
    }
    #[test]
    fn damaged_page_never_silently_becomes_geometry() {
        let dir = Temp::new();
        let (h, _) = fixture(&dir.0, 224, false);
        let Data::Disk(d) = &h.0.data else { panic!() };
        let d = d.lock().unwrap();
        let path = d.path.clone();
        let offset = d.records[0].offset;
        drop(d);
        let mut f = OpenOptions::new().write(true).open(path).unwrap();
        f.seek(SeekFrom::Start(offset)).unwrap();
        f.write_all(&[0xff; 8]).unwrap();
        drop(f);
        assert!(h.reader().page(0).is_err());
    }
    #[test]
    fn shared_readers_stay_exact_under_concurrent_eviction() {
        let dir = Temp::new();
        let (h, _) = fixture(&dir.0, 224, false);
        std::thread::scope(|s| {
            for thread in 0..4 {
                let h = h.clone();
                s.spawn(move || {
                    let mut r = h.reader();
                    for i in 0..500 {
                        let index = (i * 31 + thread * 47) % 768;
                        assert_eq!(
                            r.height(index).to_bits(),
                            height(index % 32, index / 32).to_bits()
                        );
                    }
                });
            }
        });
        assert!(h.stats().peak_bytes <= 224);
    }
    #[test]
    fn incomplete_duplicate_and_out_of_bounds_builds_fail_cleanly() {
        let dir = Temp::new();
        let mut b = Builder::new(&dir.0, 8, 8, 4).unwrap();
        assert!(b.block(2, 0, &[0.; 16], &[[0; 3]; 16]).is_err());
        b.block(0, 0, &[0.; 16], &[[0; 3]; 16]).unwrap();
        assert!(b.block(0, 0, &[0.; 16], &[[0; 3]; 16]).is_err());
        assert!(b.finish(224).is_err());
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 0);
        assert!(Builder::new(&dir.0, 7, 8, 4).is_err());
    }
}
