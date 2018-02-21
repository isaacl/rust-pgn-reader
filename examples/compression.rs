extern crate pgn_reader;
extern crate arrayvec;
extern crate memmap;
extern crate madvise;
extern crate shakmaty;
extern crate huffman_compress;
extern crate spsa;
extern crate float_cmp;

use pgn_reader::{Visitor, Skip, Reader, San};

use spsa::{HyperParameters, Theta};

use huffman_compress::{Tree, Book, CodeBuilder};

use arrayvec::ArrayVec;

use shakmaty::{Chess, Role, Position, Setup, MoveList, Square, Move, Color};

use float_cmp::ApproxOrdUlps;

use memmap::Mmap;
use madvise::{AccessPattern, AdviseMemory};

use std::env;
use std::fs::File;

struct Histogram {
    counts: [u64; 256],
    pos: Chess,
    skip: bool,
    last_dest: Option<Square>,
    theta: Theta
}

impl Histogram {
    fn new(theta: Theta) -> Histogram {
        Histogram {
            counts: [0; 256],
            pos: Chess::default(),
            skip: false,
            last_dest: None,
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
        self.last_dest = None;
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

            let turn = self.pos.turn();

            let mut augmented: ArrayVec<[(&Move, (_)); 512]> = legals.iter().map(|m| {
                let dval = dest_value(turn, m) as f64 * 0.005;
                let sval = src_value(turn, m) as f64 * 0.005;

                let dist = self.last_dest.map_or(0.0, |d| m.to().distance(d) as f64);

                let score =
                    //self.theta[0] * poor_mans_see(&self.pos, m.from().expect("no drops")) * (role_value(m.role()) - 0.5) +
                    // self.theta[0] * m.capture().map_or(0.0, |r| role_value(r) * piece_value(r.of(!self.pos.turn()), m.to()) as f64 / 1000.0) +
                    self.theta[0] * (m.capture().map_or(0.0, role_value) + m.promotion().map_or(0.0, promote_value)) +
                    self.theta[1] * poor_mans_see(&self.pos, m.to()) * (0.5 - role_value(m.role())) +
                    self.theta[2] * dval +
                    self.theta[3] * dval * sval +
                    self.theta[4] * (if m.is_castle() { 50.0 } else { 0.0 }) +
                    self.theta[5] * dist * dist +
                    (u32::from(m.to()) as f64 * 0.001) +
                    (u32::from(m.from().expect("no drops")) as f64 * 0.001);
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
            self.last_dest = Some(augmented[idx].0.to());
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
[ 111, 109,  98, 100, 106, 117, 127, 122,
  351, 327, 251, 202, 222, 309, 329, 330,
  181, 130, 154, 108,  87, 185, 178, 191,
   79,  81,  66, 123, 111,  87, 110, 100,
   11,  19,  44, 111, 106,  30,  14,  11,
   18,  17,  55,  84, 113,  26,  22,  20,
    0,   0,   0,   0,   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   0],

[ 117,  29,  49,  51,  38,  82,  24,  86,
   33,  53,  63,  51,  69,  44,  42,  22,
   35,  35,  66,  70,  54,  83,  38,  52,
   25,  20,  72,  46,  46,  63,  23,  36,
   12,  35,  44,  46,  54,  58,  47,  11,
    7,  37,  93,  45,  59, 127,  66,   9,
    7,  28,  36,  36,  30,  49,  42,  18,
    9,   2,  12,  10,   8,  22,   3,   5],

[  32,  12,  28,  36,  24,  41,  13,  25,
   17,  44,  24,  63,  60,  27,  52,  18,
    4,  23,  69,  39,  29,  74,  24,   9,
   19,  30,  38,  32,  41,  30,  43,  21,
   29,  36,  43,  39,  35,  30,  34,  47,
   28,  43,  39,  41,  40,  46,  57,  14,
   34,  98,  32,  25,  42,  34, 157,  28,
   11,  11,   7,  15,  16,   8,  10,   3],

[  59,  47,  53,  64,  63,  62,  44,  54,
   72,  69,  61,  50,  48,  48,  57,  61,
   48,  39,  42,  37,  34,  36,  38,  44,
   34,  33,  34,  31,  33,  31,  36,  34,
   23,  24,  25,  26,  27,  30,  34,  25,
   14,  24,  24,  23,  25,  31,  43,  19,
    7,  23,  25,  26,  25,  27,  36,   9,
   13,  11,  20,  31,  32,  17,  12,  22],

[  38,  22,  26,  55,  33,  28,  22,  43,
   33,  52,  41,  28,  39,  39,  42,  30,
   17,  17,  30,  24,  33,  30,  22,  30,
   17,  22,  24,  28,  36,  22,  29,  18,
   14,  16,  23,  35,  30,  24,  17,  27,
   12,  21,  27,  18,  26,  35,  41,  20,
   12,  21,  23,  18,  22,  30,  36,  18,
   24,   4,   7,  14,   9,  16,  15,  14],

[  74,  83,  76,  69,  69,  74,  77,  67,
  104, 137, 115,  98,  99, 110, 121,  92,
  109, 150, 135, 126, 124, 140, 153, 107,
  106, 146, 139, 133, 134, 143, 150, 106,
   87, 123, 129, 129, 130, 129, 120,  80,
   72, 106, 106, 103, 104,  98,  92,  59,
   55,  80,  72,  18,  22,  43,  72,  31,
   22,  39,  36,  18,  38,  15,  32,  11]
];

static SRC_PROB: [[u16; 64]; 6] = [
[   0,   0,   0,   0,   0,   0,   0,   0,
  111, 109,  99, 100, 106, 115, 127, 122,
  358, 320, 259, 207, 235, 294, 321, 336,
  201, 126, 124, 121, 113, 150, 151, 221,
   95,  75,  85,  95, 119,  88, 100, 115,
   28,  34,  55,  56,  53,  67,  28,  27,
   14,  18,  46, 112, 136,  25,  16,  15,
    0,   0,   0,   0,   0,   0,   0,   0],

[  80, 119, 105, 109, 113, 111, 125,  89,
  126, 104,  83,  84,  78,  86, 109, 124,
   93,  61,  54,  47,  58,  57,  72, 100,
   83,  60,  42,  43,  37,  44,  51,  82,
   70,  63,  40,  36,  47,  38,  64,  73,
   54,  38,  20,  44,  42,  21,  33,  55,
   64,  68,  48,  40,  44,  41,  56,  63,
  110,  55,  88,  91,  77,  75,  74, 109],

[  61,  63,  63,  58,  60,  59,  59,  54,
   63,  62,  46,  50,  54,  45,  65,  66,
   49,  38,  37,  32,  33,  36,  35,  46,
   48,  49,  32,  32,  34,  32,  44,  48,
   65,  35,  21,  29,  32,  24,  40,  66,
   33,  22,  27,  18,  21,  29,  25,  37,
   20,  20,  21,  28,  26,  28,  15,  24,
   35,  27,  40,  51,  54,  55,  41,  37],

[  41,  41,  40,  39,  40,  45,  46,  47,
   35,  35,  37,  38,  39,  42,  45,  41,
   41,  41,  40,  38,  40,  43,  47,  46,
   44,  41,  39,  39,  38,  42,  45,  47,
   41,  42,  39,  37,  40,  40,  42,  41,
   41,  36,  38,  39,  39,  39,  34,  36,
   36,  36,  34,  34,  35,  34,  34,  35,
   21,  24,  23,  22,  21,  27,  24,  13],

[  46,  38,  37,  35,  36,  37,  39,  47,
   36,  34,  32,  32,  32,  33,  37,  40,
   33,  31,  30,  28,  28,  30,  33,  33,
   34,  31,  28,  28,  29,  28,  31,  33,
   25,  28,  24,  26,  26,  25,  26,  29,
   29,  20,  23,  19,  22,  21,  23,  27,
   29,  23,  16,  19,  20,  26,  24,  31,
   26,  28,  28,  18,  30,  31,  31,  31],

[ 209, 164, 163, 175, 168, 163, 164, 205,
  142,  96, 102, 102,  99,  91,  82, 131,
  165,  90, 104, 110, 107,  98,  83, 152,
  167, 100, 111, 114, 116, 108,  99, 159,
  164, 101, 107, 107, 106, 108, 100, 154,
  135,  85,  91,  91,  87,  85,  77, 111,
   82,  54,  62,  64,  62,  59,  40,  52,
   88,  37,  40,  84,   9,  86,  21,  50]
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
    //
    // let mut histogram = Histogram::new([6.380, 3.631, 4.146, 3.555, 2.823]);
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
