//! The port's output against bytes recorded from CPython 3.12 (system zlib 1.3)
//! and Node 24.21 (Chromium's zlib) by `generate_vectors.py`: every level,
//! wrapper, window size, memory level and strategy, driven the way each program
//! drives zlib (its input chunking, flushes and output buffer sizes).
use cw_zlib::{deflate_all, Deflater, Flush, HashVariant, OutputSchedule};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const WORDS: &str = "the of and to in is was that for on with as by at from his it an were are \
which this be or has had not but first one their its new after who they have \
her she two been other when there all during into school time may years more \
most only over city some world would where later up such used many can state \
about national out known university united then made";

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

fn inputs() -> BTreeMap<&'static str, Vec<u8>> {
    let words: Vec<&str> = WORDS.split_whitespace().collect();
    let mut out = BTreeMap::new();
    out.insert("empty", vec![]);
    out.insert("hello", b"hello world".to_vec());
    out.insert("repeat", b"abcabcabcabcabcabcabcabc".repeat(200));
    let mut g = Lcg(1);
    out.insert(
        "random",
        (0..3000).map(|_| (g.next() & 0xff) as u8).collect(),
    );
    let mut g = Lcg(2);
    let mut text = String::new();
    let mut n = 0;
    while n < 120000 {
        let w = words[(g.next() % words.len() as u64) as usize];
        let sep = if g.next().is_multiple_of(11) {
            '\n'
        } else {
            ' '
        };
        text.push_str(w);
        text.push(sep);
        n += w.len() + 1;
    }
    out.insert("text", text.into_bytes());
    let mut g = Lcg(3);
    let mut runs = vec![];
    while runs.len() < 70000 {
        let b = (g.next() & 0xff) as u8;
        let k = 1 + g.next() % 300;
        runs.extend(std::iter::repeat_n(b, k as usize));
    }
    out.insert("runs", runs);
    let mut g = Lcg(4);
    let mut big: Vec<u8> = vec![];
    while big.len() < 330000 {
        let k = g.next() % 4;
        if k == 0 {
            for _ in 0..40 {
                big.push((g.next() & 0xff) as u8);
            }
        } else if k == 1 && big.len() > 1000 {
            let start = (g.next() % (big.len() as u64 - 500)) as usize;
            let len = 20 + (g.next() % 200) as usize;
            let end = (start + len).min(big.len());
            let piece = big[start..end].to_vec();
            big.extend(piece);
        } else {
            let w = words[(g.next() % words.len() as u64) as usize];
            big.extend_from_slice(w.as_bytes());
            big.push(b' ');
        }
    }
    out.insert("mixed", big);
    out
}

fn produce(case: &Value, data: &[u8]) -> Vec<u8> {
    let lib = case["lib"].as_str().unwrap();
    let level = case["level"].as_i64().unwrap() as i32;
    let wbits = case["wbits"].as_i64().unwrap() as i32;
    let mem = case["mem"].as_i64().unwrap() as i32;
    let strategy = case["strategy"].as_i64().unwrap() as i32;
    match lib {
        "python" => {
            // compressobj(...).compress(data) + .flush(): two calls, each with a
            // fresh output buffer.
            let mut d = Deflater::new(level, wbits, mem, strategy, HashVariant::Canonical).unwrap();
            let s = OutputSchedule::cpython();
            let mut call = 0;
            let mut out = deflate_all(&mut d, data, Flush::None, &s, &mut call).unwrap();
            let mut call = 0;
            out.extend(deflate_all(&mut d, &[], Flush::Finish, &s, &mut call).unwrap());
            out
        }
        "python-compress" | "python-gzip" => cw_zlib::compress(
            data,
            level,
            wbits,
            mem,
            strategy,
            HashVariant::Canonical,
            &OutputSchedule::cpython(),
        )
        .unwrap(),
        "node" => cw_zlib::compress(
            data,
            level,
            wbits,
            mem,
            strategy,
            HashVariant::Chromium,
            &OutputSchedule::node(16384),
        )
        .unwrap(),
        other => panic!("unknown lib {other}"),
    }
}

#[test]
fn outputs_match_the_real_libraries_byte_for_byte() {
    let text = include_str!("vectors.json");
    let v: Value = serde_json::from_str(text).unwrap();
    let ins = inputs();
    let mut failures = vec![];
    let cases = v["cases"].as_array().unwrap();
    for case in cases {
        let name = case["input"].as_str().unwrap();
        let data = &ins[name];
        let out = produce(case, data);
        let got = hex(&Sha256::digest(&out));
        if got != case["sha256"].as_str().unwrap() {
            failures.push(format!(
                "{} {} level={} wbits={} mem={} strategy={}: len {} vs {}{}",
                case["lib"],
                name,
                case["level"],
                case["wbits"],
                case["mem"],
                case["strategy"],
                out.len(),
                case["len"],
                case.get("hex")
                    .map(|h| format!("\n  want {h}\n  got  {}", hex(&out)))
                    .unwrap_or_default()
            ));
        }
        // Whatever we produce must decode to the input.
        let (back, _) = cw_zlib::decompress(
            &out,
            if case["wbits"].as_i64().unwrap() > 15 {
                31
            } else {
                case["wbits"].as_i64().unwrap() as i32
            },
        )
        .unwrap();
        assert_eq!(&back, data);
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases differ:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
