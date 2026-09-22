//! `cw-zlib`: compression for the simulated computers.
//!
//! * [`deflate::State`] is a faithful port of zlib 1.3.1's deflate: the same
//!   match finder, lazy evaluation, block splitting and Huffman construction, so
//!   `zlib.compress` in the simulated `python3` and `zlib.deflateSync` in the
//!   simulated `node` produce the real programs' bytes. Node bundles Chromium's
//!   zlib, which hashes differently ([`deflate::HashVariant::Chromium`]); CPython
//!   links the system zlib ([`deflate::HashVariant::Canonical`]).
//! * [`inflate::Inflater`] decodes zlib, gzip and raw streams, resumably, with
//!   zlib's error messages.
//! * [`brotli_compress`] / [`brotli_decompress`] wrap the `brotli` crate (a port of
//!   Google's encoder and decoder).
//!
//! Everything is pure computation: no host I/O, clock or threads.
pub mod deflate;
pub mod inflate;
pub mod trees;

pub use deflate::{GzHeader, HashVariant, State as Deflater};
pub use inflate::{Inflater, Progress};

/// zlib's flush modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Flush {
    None = 0,
    Partial = 1,
    Sync = 2,
    Full = 3,
    Finish = 4,
    Block = 5,
}
impl Flush {
    pub fn from_i32(v: i32) -> Option<Self> {
        Some(match v {
            0 => Self::None,
            1 => Self::Partial,
            2 => Self::Sync,
            3 => Self::Full,
            4 => Self::Finish,
            5 => Self::Block,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Strategy {
    Default = 0,
    Filtered = 1,
    HuffmanOnly = 2,
    Rle = 3,
    Fixed = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok,
    StreamEnd,
}

/// zlib's error codes, with the message for data errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ZError {
    /// Z_STREAM_ERROR (-2): bad parameters or state.
    Stream,
    /// Z_DATA_ERROR (-3) and zlib's message.
    Data(String),
    /// Z_BUF_ERROR (-5): no progress possible (truncated input when finishing).
    Buf,
    /// Z_NEED_DICT (2), with the Adler-32 of the wanted dictionary.
    NeedDict(u32),
}
impl ZError {
    pub fn code(&self) -> i32 {
        match self {
            Self::Stream => -2,
            Self::Data(_) => -3,
            Self::Buf => -5,
            Self::NeedDict(_) => 2,
        }
    }
    /// zlib's `zError` text / `strm->msg`.
    pub fn message(&self) -> String {
        match self {
            Self::Stream => "stream error".into(),
            Self::Data(m) if !m.is_empty() => m.clone(),
            Self::Data(_) => "data error".into(),
            Self::Buf => "buffer error".into(),
            Self::NeedDict(_) => "need dictionary".into(),
        }
    }
}

/// Adler-32 continuing from `adler` (start with 1).
pub fn adler32(adler: u32, data: &[u8]) -> u32 {
    const BASE: u32 = 65521;
    let mut a = adler & 0xffff;
    let mut b = adler >> 16;
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= BASE;
        b %= BASE;
    }
    (b << 16) | a
}

fn crc_table() -> &'static [u32; 256] {
    static T: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0u32; 256];
        for (n, e) in t.iter_mut().enumerate() {
            let mut c = n as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xedb8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *e = c;
        }
        t
    })
}

/// CRC-32 (IEEE, as zlib and gzip use it) continuing from `crc` (start with 0).
pub fn crc32(crc: u32, data: &[u8]) -> u32 {
    let t = crc_table();
    let mut c = !crc;
    for &b in data {
        c = t[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8);
    }
    !c
}

/// Output buffer sizes a caller gives zlib, one per `deflate` call; the last
/// size repeats. Only stored blocks (level 0) depend on them.
#[derive(Clone, Debug)]
pub struct OutputSchedule(pub Vec<usize>);
impl OutputSchedule {
    /// CPython's `_BlocksOutputBuffer` (32 KiB, 64 KiB, 256 KiB, 1 MiB, …).
    pub fn cpython() -> Self {
        Self(vec![
            32 << 10,
            64 << 10,
            256 << 10,
            1 << 20,
            4 << 20,
            8 << 20,
            16 << 20,
            16 << 20,
            32 << 20,
        ])
    }
    /// Node's fixed `chunkSize` (16 KiB by default).
    pub fn node(chunk: usize) -> Self {
        Self(vec![chunk.max(64)])
    }
    fn at(&self, i: usize) -> usize {
        *self.0.get(i).or(self.0.last()).unwrap_or(&16384)
    }
}

/// Feeds `input` to `d` with `flush`, calling `deflate` again while the output
/// buffer fills up, as zlib's callers do.
pub fn deflate_all(
    d: &mut Deflater,
    input: &[u8],
    flush: Flush,
    schedule: &OutputSchedule,
    call: &mut usize,
) -> Result<Vec<u8>, ZError> {
    let mut out = vec![];
    let mut rest = input.to_vec();
    loop {
        let cap = schedule.at(*call);
        *call += 1;
        let (o, status) = match d.deflate(&rest, cap, flush) {
            Ok(v) => v,
            // Nothing to do (e.g. a repeated flush): zlib callers treat it as done.
            Err(ZError::Buf) => return Ok(out),
            Err(e) => return Err(e),
        };
        let consumed = rest.len() - d.unconsumed();
        rest.drain(..consumed);
        let full = o.len() == cap;
        out.extend(o);
        if status == Status::StreamEnd || (!full && rest.is_empty()) {
            return Ok(out);
        }
    }
}

/// One-shot compression the way a caller with `schedule` output buffers gets it.
pub fn compress(
    data: &[u8],
    level: i32,
    window_bits: i32,
    mem_level: i32,
    strategy: i32,
    hash: HashVariant,
    schedule: &OutputSchedule,
) -> Result<Vec<u8>, ZError> {
    let mut d = Deflater::new(level, window_bits, mem_level, strategy, hash)?;
    let mut call = 0;
    deflate_all(&mut d, data, Flush::Finish, schedule, &mut call)
}

/// One-shot decompression of a complete stream. Returns the output and the
/// bytes that followed the stream.
pub fn decompress(data: &[u8], window_bits: i32) -> Result<(Vec<u8>, Vec<u8>), ZError> {
    let mut inf = Inflater::new(window_bits)?;
    let mut out = vec![];
    match inf.inflate(data, &mut out, usize::MAX)? {
        Progress::End => Ok((out, inf.unconsumed().to_vec())),
        Progress::NeedDict(id) => Err(ZError::NeedDict(id)),
        _ => Err(ZError::Buf),
    }
}

/// Decompresses one gzip member after another, as `gunzip` and Node's
/// `gunzipSync` do; trailing zero padding is ignored.
pub fn gunzip_members(data: &[u8]) -> Result<Vec<u8>, ZError> {
    let mut out = vec![];
    let mut rest = data.to_vec();
    let mut first = true;
    loop {
        if !first && (rest.is_empty() || rest.iter().all(|b| *b == 0)) {
            return Ok(out);
        }
        if !first && !(rest.len() >= 2 && rest[0] == 0x1f && rest[1] == 0x8b) {
            // Trailing garbage after a member is ignored (gzip's own rule).
            return Ok(out);
        }
        let (o, unused) = decompress(&rest, 31)?;
        out.extend(o);
        rest = unused;
        first = false;
    }
}

/// The heap allocator the brotli encoder is given (its `std` feature, which also
/// brings a thread pool, stays off).
#[derive(Clone, Copy, Default)]
struct Heap;
impl<T: Clone + Default> alloc_no_stdlib::Allocator<T> for Heap {
    type AllocatedMemory =
        <alloc_stdlib::StandardAlloc as alloc_no_stdlib::Allocator<T>>::AllocatedMemory;
    fn alloc_cell(&mut self, len: usize) -> Self::AllocatedMemory {
        alloc_stdlib::StandardAlloc::default().alloc_cell(len)
    }
    fn free_cell(&mut self, data: Self::AllocatedMemory) {
        alloc_stdlib::StandardAlloc::default().free_cell(data)
    }
}
impl brotli::enc::BrotliAlloc for Heap {}

/// Brotli compression (quality 0..=11, `lgwin` 10..=24, mode 0 generic, 1 text,
/// 2 font) through the `brotli` crate.
pub fn brotli_compress(
    data: &[u8],
    quality: u32,
    lgwin: u32,
    mode: u32,
    size_hint: usize,
) -> Vec<u8> {
    use brotli::enc::backward_references::BrotliEncoderMode;
    use brotli::{CustomRead, CustomWrite};
    struct Src<'a>(&'a [u8]);
    impl CustomRead<()> for Src<'_> {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
            let n = buf.len().min(self.0.len());
            buf[..n].copy_from_slice(&self.0[..n]);
            self.0 = &self.0[n..];
            Ok(n)
        }
    }
    struct Dst(Vec<u8>);
    impl CustomWrite<()> for Dst {
        fn write(&mut self, data: &[u8]) -> Result<usize, ()> {
            self.0.extend_from_slice(data);
            Ok(data.len())
        }
        fn flush(&mut self) -> Result<(), ()> {
            Ok(())
        }
    }
    let params = brotli::enc::BrotliEncoderParams {
        quality: quality.min(11) as i32,
        lgwin: lgwin.clamp(10, 24) as i32,
        mode: match mode {
            1 => BrotliEncoderMode::BROTLI_MODE_TEXT,
            2 => BrotliEncoderMode::BROTLI_MODE_FONT,
            _ => BrotliEncoderMode::BROTLI_MODE_GENERIC,
        },
        size_hint,
        ..Default::default()
    };
    let mut src = Src(data);
    let mut dst = Dst(vec![]);
    let mut ibuf = vec![0u8; 65536];
    let mut obuf = vec![0u8; 65536];
    let r = brotli::enc::BrotliCompressCustomIo(
        &mut src,
        &mut dst,
        &mut ibuf,
        &mut obuf,
        &params,
        Heap,
        &mut |_, _, _, _| (),
        (),
    );
    match r {
        Ok(_) => dst.0,
        Err(()) => vec![],
    }
}

/// Brotli decompression; `Err` with the decoder's reason on corrupt input.
pub fn brotli_decompress(data: &[u8]) -> Result<Vec<u8>, String> {
    use brotli_decompressor::{BrotliDecompressStream, BrotliResult, BrotliState};
    let mut state = BrotliState::new(
        alloc_stdlib::StandardAlloc::default(),
        alloc_stdlib::StandardAlloc::default(),
        alloc_stdlib::StandardAlloc::default(),
    );
    let mut out = vec![];
    let mut buf = vec![0u8; 65536];
    let mut avail_in = data.len();
    let mut in_off = 0;
    let mut total = 0;
    loop {
        let mut avail_out = buf.len();
        let mut out_off = 0;
        let r = BrotliDecompressStream(
            &mut avail_in,
            &mut in_off,
            data,
            &mut avail_out,
            &mut out_off,
            &mut buf,
            &mut total,
            &mut state,
        );
        out.extend_from_slice(&buf[..out_off]);
        match r {
            BrotliResult::ResultSuccess => return Ok(out),
            BrotliResult::NeedsMoreOutput => continue,
            BrotliResult::NeedsMoreInput => return Err("unexpected end of file".into()),
            BrotliResult::ResultFailure => return Err("Decompression failed".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums() {
        assert_eq!(adler32(1, b"Wikipedia"), 0x11e6_0398);
        assert_eq!(
            crc32(0, b"The quick brown fox jumps over the lazy dog"),
            0x414f_a339
        );
    }

    #[test]
    fn round_trips_every_level_and_wrapper() {
        let mut data = vec![];
        for i in 0..20000u32 {
            data.extend_from_slice(format!("line {} of text {}\n", i % 97, i * 7 % 13).as_bytes());
        }
        data.extend((0..5000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8));
        for hash in [HashVariant::Canonical, HashVariant::Chromium] {
            for level in 0..=9 {
                for wb in [15, -15, 31, 9] {
                    let c = compress(&data, level, wb, 8, 0, hash, &OutputSchedule::node(16384))
                        .unwrap();
                    let (d, rest) = decompress(&c, wb).unwrap();
                    assert!(rest.is_empty());
                    assert_eq!(d, data, "level {level} wb {wb} {hash:?}");
                }
            }
            for strategy in 1..=4 {
                let c =
                    compress(&data, 6, 15, 8, strategy, hash, &OutputSchedule::cpython()).unwrap();
                assert_eq!(decompress(&c, 15).unwrap().0, data);
            }
        }
    }

    #[test]
    fn errors_have_zlib_messages() {
        assert_eq!(
            decompress(b"hello world", 15).unwrap_err(),
            ZError::Data("incorrect header check".into())
        );
        let mut c = compress(
            b"abcabcabc",
            6,
            15,
            8,
            0,
            HashVariant::Canonical,
            &OutputSchedule::cpython(),
        )
        .unwrap();
        let n = c.len();
        c[n - 1] ^= 1;
        assert_eq!(
            decompress(&c, 15).unwrap_err(),
            ZError::Data("incorrect data check".into())
        );
        assert_eq!(decompress(&c[..4], 15).unwrap_err(), ZError::Buf);
    }

    #[test]
    fn brotli_round_trip() {
        let data = b"brotli brotli brotli brotli, compress me please".repeat(50);
        let c = brotli_compress(&data, 11, 22, 0, data.len());
        assert!(c.len() < data.len());
        assert_eq!(brotli_decompress(&c).unwrap(), data);
    }
}
