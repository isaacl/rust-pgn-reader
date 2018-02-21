extern crate pgn_reader;
extern crate arrayvec;
extern crate memmap;
extern crate madvise;
extern crate shakmaty;
extern crate huffman_compress;
extern crate spsa;
extern crate float_cmp;

use pgn_reader::{Visitor, Skip, Reader, San};

use shakmaty::{Chess, Role, Position, Setup, MoveList, Square, Move, Color, Piece};

use memmap::Mmap;
use madvise::{AccessPattern, AdviseMemory};

use std::env;
use std::fs::File;

struct Counter {
    avail: [[u32; 64]; 6],
    hits: [[u32; 64]; 6],
    s_avail: [[u32; 64]; 6],
    s_hits: [[u32; 64]; 6],
    pos: Chess,
    skip: bool
}

impl Counter {
    fn new() -> Counter {
        Counter {
            avail: [[0u32; 64]; 6],
            hits: [[0u32; 64]; 6],
            s_avail: [[0u32; 64]; 6],
            s_hits: [[0u32; 64]; 6],
            pos: Chess::default(),
            skip: false
        }
    }

    fn print_arr(&self, hits: [[u32; 64]; 6], totals: [[u32; 64]; 6]) {
        println!("[");
        for i in 0..6 {
            let mut probs = hits[i].iter().zip(totals[i].iter()).map(|(u, v)|
                if *u == 0 { 0 } else { (1000 * *u) / *v });

            let mut print_elt = || { print!("{:3}", probs.next().unwrap()) };
            print!("[ ");
            for i in 0 .. 8 {
                if i != 0 { print!("  ") }
                for _ in 0 .. 7 { print_elt(); print!(", ") }
                print_elt(); if i != 7 { println!(",") }
            }
            if i != 5 { println!("],\n") }
        }
        println!("]\n]");
    }

    fn print_probs(&self) {
        self.print_arr(self.hits, self.avail);
        println!("source probs");
        self.print_arr(self.s_hits, self.s_avail);
    }

    fn fix_sq(&self, sq: Square) -> Square {
        if self.pos.turn().is_white() { sq.flip_vertical() } else { sq }
    }
}

impl<'pgn> Visitor<'pgn> for Counter {
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


            let ignore = legals.len() == 1;

            let mut move_match: Option<Move> = None;

            for m in legals {
                let dest = self.fix_sq(m.to()) as usize;
                let src = self.fix_sq(m.from().unwrap()) as usize;

                let role = m.role() as usize;
                if !ignore && !m.is_castle() {
                    self.avail[role][dest] += 1;
                    self.s_avail[role][src] += 1;
                }
                if san.matches(&m) {
                    match move_match {
                        Some(_) => {
                            eprintln!("illegal san: {}, dupe move", san);
                            self.skip = true;
                            return;
                        }
                        None => {
                            if !ignore && !m.is_castle() {
                                self.hits[role][dest] += 1;
                                self.s_hits[role][src] += 1;
                            }
                            move_match = Some(m);
                        }
                    }
                }
            }

            match move_match {
                Some(m) => { self.pos.play_unchecked(&m) }
                None => {
                    eprintln!("illegal san: {}, not found", san);
                    self.skip = true;
                    return;
                }
            }
        }
    }

    fn end_game(&mut self, _game: &'pgn [u8]) { }
}

fn main() {
    let arg = env::args().skip(1).next().expect("pgn file as argument");
    eprintln!("reading {} ...", arg);
    let file = File::open(&arg).expect("fopen");
    let mmap = unsafe { Mmap::map(&file).expect("mmap") };
    let pgn = &mmap[..];
    pgn.advise_memory_access(AccessPattern::Sequential).expect("madvise");

    let mut counter = Counter::new();
    {
        let mut reader = Reader::new(&mut counter, pgn);
        for _k in 0..1000000 {
            reader.read_game();
        }
    }
    counter.print_probs();
}
