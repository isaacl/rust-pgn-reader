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
    theta: [f64; 4]
}

impl Histogram {
    fn new(theta: [f64; 4]) -> Histogram {
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
                let mval = move_value(self.pos.turn(), m) as f64 / 128.0;
                let score =
                    //self.theta[0] * poor_mans_see(&self.pos, m.from().expect("no drops")) * (role_value(m.role()) - 0.5) +
                    // self.theta[0] * m.capture().map_or(0.0, |r| role_value(r) * piece_value(r.of(!self.pos.turn()), m.to()) as f64 / 1000.0) +
                    self.theta[0] * (m.capture().map_or(0.0, role_value) + m.promotion().map_or(0.0, promote_value)) +
                    self.theta[1] * poor_mans_see(&self.pos, m.to()) * (0.5 - role_value(m.role())) +
                    self.theta[2] * mval +
                    self.theta[3] * mval * mval +
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

fn piece_value(piece: Piece, square: Square) -> i16 {
    let sq = if piece.color.is_white() { square } else { square.flip_vertical() };
    PSQT[piece.role as usize][usize::from(sq)] as i16
}

fn move_value(turn: Color, m: &Move) -> i16 {
    let piece = m.role().of(turn);
    piece_value(piece, m.to()) // - piece_value(piece, m.from().expect("no drops"))
}

fn poor_mans_see(pos: &Chess, sq: Square) -> f64 {
    if (shakmaty::attacks::pawn_attacks(pos.turn(), sq) & pos.board().pawns() & pos.them()).any() {
        1.0
    } else {
        0.0
    }
}

static PSQT: [[u16; 64]; 6] = [
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

[   2,   3,   4,   1,   2,   0,   1,   0,
   24,  27,  14,   1,   1,   3,   5,   2,
   51,  59,  48,  33,  26,  24,  23,  16,
   83, 115, 106,  94,  89,  85,  86,  60,
  109, 154, 165, 174, 185, 201, 195, 134,
  136, 217, 224, 246, 271, 303, 312, 217,
  168, 250, 261, 127, 148, 281, 406, 283,
  525, 283, 176,  99, 171, 101, 208, 494]
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

    // let mut histogram = Histogram::new([6.544, 3.085, 3.703, 1.801]);
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
