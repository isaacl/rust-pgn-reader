extern crate pgn_reader;
extern crate arrayvec;
extern crate memmap;
extern crate madvise;
extern crate shakmaty;
extern crate huffman_compress;
extern crate spsa;
extern crate float_cmp;

use pgn_reader::{Visitor, Skip, Reader, San};

use spsa::{HyperParameters};

use huffman_compress::{Tree, Book, CodeBuilder};

use arrayvec::ArrayVec;

use shakmaty::{Chess, Role, Position, Setup, MoveList, Square, Move, Color, Piece};

use float_cmp::ApproxOrdUlps;

use memmap::Mmap;
use madvise::{AccessPattern, AdviseMemory};

use std::env;
use std::fs::File;

struct Histogram {
    counts: [u64; 256],
    pos: Chess,
    skip: bool,
    theta: [f64; 5]
}

impl Histogram {
    fn new(theta: [f64; 5]) -> Histogram {
        Histogram {
            counts: [0; 256],
            pos: Chess::default(),
            skip: false,
            theta,
        }
    }

    fn huffman(&self) -> (Book<u8>, Tree<u8>) {
        self.counts.iter()
            .enumerate()
            .map(|(k, v)| (k as u8, v + 1))
            .collect::<CodeBuilder<_, _>>()
            .finish()
    }

    fn bits(&self) -> u64 {
        let (book, _) = self.huffman();

        // let div = 1.0 / self.counts.iter().fold(0u64, |u, v| u + *v) as f32;
        // println!("        {}", self.counts[..5].iter().map(|c| format!("{:.3}", *c as f32 * div)).collect::<Vec<_>>().join(", "));

        self.counts.iter()
            .enumerate()
            .map(|(k, v)| book.get(&(k as u8)).map_or(0, |c| c.len() as u64 * v))
            .sum()
    }
}

fn role_value(r: Role) -> f64 {
    match r {
        Role::Pawn => 1.0,
        Role::Knight => 3.0,
        Role::Bishop => 3.0,
        Role::Rook => 3.0,
        Role::Queen => 9.0,
        Role::King => 1000.0,
    }
}

fn promote_value(r: Role) -> f64 {
    match r {
        Role::Pawn => 0.0,
        Role::Knight => 2.0,
        Role::Bishop => 1.0,
        Role::Rook => 1.0,
        Role::Queen => 5.0,
        Role::King => 0.0,
    }
}

impl<'pgn> Visitor<'pgn> for Histogram {
    type Result = ();

    fn begin_game(&mut self) {
        self.pos = Chess::default();
        self.skip = false;
    }

    fn header(&mut self, key: &'pgn [u8], _value: &'pgn [u8]) {
        if key == b"FEN" {
            self.skip = true;
        }
    }

    fn end_headers(&mut self) -> Skip {
        Skip(self.skip)
    }

    fn begin_variation(&mut self) -> Skip {
        Skip(true)
    }

    fn san(&mut self, san: San) {
        if !self.skip {
            let mut legals = MoveList::new();
            self.pos.legal_moves(&mut legals);

            let mut augmented: ArrayVec<[(&Move, (_)); 512]> = legals.iter().map(|m| {
                let dval = dest_value(self.pos.turn(), m) as f64 / 128.0;
                let sval = src_value(self.pos.turn(), m) as f64 / 128.0;

                let score =
                    //self.theta[0] * poor_mans_see(&self.pos, m.from().expect("no drops")) * (role_value(m.role()) - 0.5) +
                    // self.theta[0] * m.capture().map_or(0.0, |r| role_value(r) * piece_value(r.of(!self.pos.turn()), m.to()) as f64 / 1000.0) +
                    self.theta[0] * (m.capture().map_or(0.0, role_value) + m.promotion().map_or(0.0, promote_value)) +
                    self.theta[1] * poor_mans_see(&self.pos, m.to()) * (0.5 - role_value(m.role())) +
                    self.theta[2] * dval +
                    self.theta[3] * dval * sval +
                    self.theta[4] * (if m.is_castle() { 50.0 } else { 0.0 }) +
                    (u32::from(m.to()) as f64 / 1024.0) +
                    (u32::from(m.from().expect("no drops")) as f64 / 1024.0);
                (m, score)
            }).collect();

            augmented.sort_unstable_by(|a, b| b.1.approx_cmp(&a.1, 1));

            let idx = match augmented.iter().position(|a| san.matches(a.0)) {
                Some(idx) => idx,
                None => {
                    eprintln!("illegal san: {}", san);
                    self.skip = true;
                    return;
                }
            };

            self.counts[idx] += 1;

            self.pos.play_unchecked(&augmented[idx].0);
        }
    }

    fn end_game(&mut self, _game: &'pgn [u8]) { }
}

fn dest_value(turn: Color, m: &Move) -> i16 {
    let sq = if turn.is_white() { m.to().flip_vertical() } else { m.to() };
    DEST_PROB[m.role() as usize][sq as usize] as i16
}

fn src_value(turn: Color, m: &Move) -> i16 {
    let sq = if turn.is_white() { m.from().unwrap().flip_vertical() } else { m.from().unwrap() };
    SRC_PROB[m.role() as usize][sq as usize] as i16
}
// fn piece_value(piece: Piece, square: Square) -> i16 {
//     let sq = if piece.color.is_white() { square.flip_vertical() } else { square };
//     PSQT[piece.role as usize][usize::from(sq)] as i16
// }
//
// fn move_value(turn: Color, m: &Move) -> i16 {
//     let piece = m.role().of(turn);
//     piece_value(piece, m.to()) - piece_value(piece, m.from().expect("no drops"))
// }

fn poor_mans_see(pos: &Chess, sq: Square) -> f64 {
    if (shakmaty::attacks::pawn_attacks(pos.turn(), sq) & pos.board().pawns() & pos.them()).any() {
        1.0
    } else {
        0.0
    }
}

static DEST_PROB: [[u16; 64]; 6] = [
[ 195, 196, 168, 173, 191, 213, 247, 229,
  379, 356, 284, 227, 254, 343, 365, 370,
   28,  29,  68,  76,  62,  39,  20,  13,
   19,  29,  42,  93,  77,  32,  21,  15,
   14,  24,  53, 132, 131,  42,  20,  15,
   19,  19,  59,  91, 125,  31,  26,  25,
    0,   0,   0,   0,   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   0],

[  95,   1,  23,  18,   7,  28,   1,  45,
   17,  54,  65,  10,  13,  58,  47,  11,
    2,  23,  19,  74,  65,  19,  36,   4,
    8,  30,  53,  41,  52,  59,  38,   8,
   18,   9,  55,  51,  47,  62,  12,  19,
   13,  48, 156,  42,  47, 237,  70,  28,
   10,  28,  35,  56,  50,  30,  37,  25,
   11,   4,  17,  13,  13,  33,   5,   7],

[  23,  10,   4,  16,  12,   8,  15,   6,
   20,  27,  20,   7,   9,  38,  39,  26,
    8,  15,  58,  11,  12,  54,  25,  16,
   16,  67,  11,  40,  33,  11,  79,  23,
   34,   9,  89,  31,  42,  56,  12,  42,
   10,  59,  46,  86,  69,  61,  54,   7,
   29, 139,  36,  59, 102,  21, 199,  17,
   14,  13,  16,  24,  25,  17,   8,   6],

[  13,   3,   5,   9,   7,   6,   3,  17,
   22,  56,  44,  32,  28,  26,  57,  23,
   29,  36,  36,  27,  23,  25,  37,  28,
   31,  32,  33,  29,  29,  29,  35,  31,
   24,  25,  26,  28,  31,  32,  35,  28,
   21,  26,  28,  30,  34,  43,  45,  29,
   17,  28,  33,  37,  38,  45,  36,  18,
   26,  31,  51,  75,  93,  46,  40,  46],

[  42,   5,   5,  12,   6,  15,  28,  56,
   33,  43,   7,   5,   5,  34,  52,  54,
   19,   5,  21,   6,  17,  10,  23,  28,
    5,  22,  15,  25,  24,  21,  10,  49,
   41,  16,  35,  39,  43,  25,  42,   9,
   11,  60,  38,  55,  47,  90,  40,  22,
   12,  26,  86,  70, 103,  34,  28,   8,
   22,  16,  25,  44,  37,  28,  11,  10],

[   4,   3,   4,   1,   2,   0,   1,   0,
   24,  27,  14,   1,   1,   3,   5,   2,
   51,  59,  48,  33,  26,  24,  23,  16,
   83, 115, 106,  94,  89,  85,  86,  60,
  109, 154, 165, 174, 185, 201, 195, 134,
  136, 217, 224, 246, 271, 303, 312, 217,
  168, 250, 261, 127, 148, 281, 406, 283,
  144, 283, 176,  99, 171, 101, 208, 160]
];

static SRC_PROB: [[u16; 64]; 6] = [
[   0,   0,   0,   0,   0,   0,   0,   0,
    2,   2,   4,   5,   4,   1,   1,   1,
   48,  53,  43,  44,  36,  56,  29,  30,
  136,  91,  67,  86,  82,  90, 105, 149,
  106,  82,  97, 104, 133, 101, 112, 130,
   32,  38,  62,  65,  61,  78,  32,  32,
   16,  21,  55, 137, 184,  33,  21,  20,
    0,   0,   0,   0,   0,   0,   0,   0],

[ 104,   0,  73,  60,  35,  33,   0, 111,
  108, 112,  83,   6,  12,  77,  82,  41,
   26,  25,   3,  51,  51,   1,  22,  35,
   39,  86,  36,  35,  41,  39,  73,  45,
   95,  22,  45,  42,  41,  42,  26,  97,
   75,  50,  32,  40,  47,  37,  47,  81,
   72,  62,  48,  67,  71,  46,  68,  90,
   44, 103, 108, 119, 117, 105, 204,  65],

[  51,  24,   0,  23,  26,   0,  53,  46,
   22,   6,  17,   3,   2,  34,   2,  36,
   21,   8,  20,   3,   4,  11,  11,  48,
   33,  84,   4,  33,  30,   6,  85,  34,
   86,  12,  47,  29,  35,  51,  10,  88,
   50,  42,  38,  36,  43,  48,  44,  35,
   34,  45,  35,  58,  60,  36,  38,  36,
   42,  42, 109,  83,  83, 206,  45,  43],

[   1,   6,   4,   2,   2,   1,   6,   1,
   55,  55,  46,  35,  34,  33,  51,  59,
   57,  48,  47,  39,  34,  30,  34,  48,
   50,  50,  46,  39,  40,  38,  45,  48,
   36,  33,  32,  37,  38,  43,  42,  39,
   26,  31,  31,  37,  46,  54,  45,  35,
   18,  18,  26,  36,  40,  43,  29,  22,
   61,  54,  55,  59,  60,  86,  58,  44],

[  52,  31,  21,   0,  18,  25,  58,  92,
   63,  41,   4,   4,   3,  24,  29,  71,
   41,   4,  17,   6,  13,   4,  12,  35,
    8,  38,  15,  19,  20,  22,  16,  53,
   69,  23,  41,  36,  36,  32,  47,  16,
   22,  83,  38,  60,  43,  77,  55,  26,
   13,  19,  73,  82,  93,  35,  32,  14,
   21,  36,  52, 148,  59,  47,  17,   9],

[  22,   3,   1,   4,   0,   2,   0,   1,
   39,  18,  12,   7,   5,   4,   2,   4,
  102,  45,  40,  29,  21,  20,  19,  34,
  153,  91,  86,  77,  72,  70,  70, 115,
  178, 111, 134, 150, 159, 155, 137, 205,
  205, 148, 187, 219, 234, 234, 212, 299,
  216, 176, 221, 245, 252, 245, 235, 277,
  385, 328, 406, 362,  63, 427, 310, 416]
];

fn main() {
    let arg = env::args().skip(1).next().expect("pgn file as argument");
    eprintln!("reading {} ...", arg);
    let file = File::open(&arg).expect("fopen");
    let mmap = unsafe { Mmap::map(&file).expect("mmap") };
    let mut pgn = &mmap[..];
    pgn.advise_memory_access(AccessPattern::Sequential).expect("madvise");

    let batch_size = 200;

    let mut spsa = HyperParameters::default().spsa();

    let mut next_pgn = pgn;

    // let mut histogram = Histogram::new([6.826, 3.310, 3.472, 0.0, 10.959]);
    // let games = 100000;
    // {
    //     let mut reader = Reader::new(&mut histogram, pgn);
    //     for _ in 0..games {
    //         reader.read_game();
    //     }
    // }
    // println!("{} bytes over {} games", histogram.bits() as f64 / games as f64 / 8.0, games);


    for _k in 0..1000 {
        spsa.step(&mut |theta| {
            let mut histogram = Histogram::new(theta);

            {
                let mut reader = Reader::new(&mut histogram, pgn);

                for _ in 0..batch_size {
                    reader.read_game();
                }

                next_pgn = reader.remaining_pgn();

            }

            histogram.bits() as f64 / batch_size as f64 / 8.0
        });

        pgn = next_pgn;
    }
}
