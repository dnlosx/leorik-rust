//! The NNUE network of Leorik 3.2, embedded into the binary.
//!
//! Architecture: (768x5 -> 640)x2 -> 1x8, horizontally mirrored, 5 king input buckets,
//! 8 material output buckets, SCReLU activation, quantization QA = 255, QB = 64.
//!
//! The file `640HL-S-5io8-6116M-FRCv1.nnue` is the network shipped with Leorik 3.2
//! (MIT licensed, Copyright (c) 2023 Thomas Jahn). Its layout is a raw little-endian
//! dump of `i16` values followed by padding, which maps one-to-one onto [`Network`].

#[cfg(target_endian = "big")]
compile_error!("The embedded NNUE network is stored little-endian; big-endian targets are not supported.");

pub const NETWORK_FILE_NAME: &str = "640HL-S-5io8-6116M-FRCv1.nnue";

pub const SCALE: i32 = 400;
pub const QA: i32 = 255;
pub const QB: i32 = 64;
pub const INPUT_SIZE: usize = 768;
pub const LAYER1_SIZE: usize = 640;
pub const INPUT_BUCKETS: usize = 5;
pub const OUTPUT_BUCKETS: usize = 8;

pub static INPUT_BUCKET_MAP: [u8; 64] = [
    0, 0, 1, 1, 1, 1, 0, 0, //
    2, 2, 3, 3, 3, 3, 2, 2, //
    2, 2, 3, 3, 3, 3, 2, 2, //
    4, 4, 4, 4, 4, 4, 4, 4, //
    4, 4, 4, 4, 4, 4, 4, 4, //
    4, 4, 4, 4, 4, 4, 4, 4, //
    4, 4, 4, 4, 4, 4, 4, 4, //
    4, 4, 4, 4, 4, 4, 4, 4, //
];

const FILE_SIZE: usize = 4_937_024;

#[repr(C, align(64))]
pub struct Network {
    pub feature_weights: [i16; INPUT_BUCKETS * INPUT_SIZE * LAYER1_SIZE],
    pub feature_biases: [i16; LAYER1_SIZE],
    pub output_weights: [i16; LAYER1_SIZE * 2 * OUTPUT_BUCKETS],
    pub output_biases: [i16; OUTPUT_BUCKETS],
    _padding: [u8; 48],
}

const _: () = assert!(core::mem::size_of::<Network>() == FILE_SIZE);

#[repr(C, align(64))]
struct AlignedBytes([u8; FILE_SIZE]);

static NETWORK_BYTES: AlignedBytes = AlignedBytes(*include_bytes!("../net/640HL-S-5io8-6116M-FRCv1.nnue"));

/// The default (and only) network.
#[inline(always)]
pub fn network() -> &'static Network {
    // SAFETY: `Network` is a `repr(C)` struct of plain integers with exactly the size of the
    // embedded file and no internal padding; the bytes are 64-byte aligned like `Network`
    // and every bit pattern is a valid `i16`/`u8`.
    unsafe { &*(NETWORK_BYTES.0.as_ptr() as *const Network) }
}

#[inline(always)]
pub fn material_bucket(piece_count: u32) -> usize {
    let divisor = (32 + OUTPUT_BUCKETS - 1) / OUTPUT_BUCKETS;
    (piece_count as usize - 2) / divisor
}
