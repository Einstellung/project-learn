use halo2_proofs::{pasta::Fp, arithmetic::Field};

pub const BASE: u64 = 29;
pub const WORD_COUNT: usize = 6;
pub const WORD_LEN : usize = 5;

pub fn word_to_chars(word: &str) -> Vec<u64> {
    let mut res = vec![];
    for c in word.chars() {
        res.push((c as u64) - ('a' as u64) + 1);
    }
    res
}

pub fn word_to_polyhash(word: &str) -> u64 {
    let chars = word_to_chars(word);
    let mut hash = 0;
    for c in chars {
        hash = hash * BASE;
        hash += c;
    }

    hash
}

pub fn compute_diff(word: &str, final_word: &str) -> Vec<Vec<Fp>> {
    let mut res = vec![];
    let mut green = vec![];
    for i in 0..WORD_LEN {
        if word.chars().nth(i) == final_word.chars().nth(i) {
            green.push(Fp::one());
        } else {
            green.push(Fp::zero());
        }
    }
    res.push(green);

    let mut yellow = vec![Fp::zero(); WORD_LEN];
    for i in 0..WORD_LEN {
        for j in 0..WORD_LEN {
            if word.chars().nth(i) == final_word.chars().nth(j) {
                yellow[i] = Fp::one();
            }
        }
    }
    res.push(yellow);
    // println!("word {:?} final {:?} res {:?}", word, final_word, res);
    
    res
}

pub fn compute_diff_u64(word: &str, final_word: &str) -> Vec<Vec<u64>> {
    let mut res = vec![];
    let mut green = vec![];
    for i in 0..WORD_LEN {
        if word.chars().nth(i) == final_word.chars().nth(i) {
            green.push(1 as u64);
        } else {
            green.push(0 as u64);
        }
    }
    res.push(green);

    let mut yellow = vec![0 as u64; WORD_LEN];
    for i in 0..WORD_LEN {
        for j in 0..WORD_LEN {
            if word.chars().nth(i) == final_word.chars().nth(j) {
                yellow[i] = 1 as u64;
            }
        }
    }
    res.push(yellow);
    // println!("word {:?} final {:?} res {:?}", word, final_word, res);
    
    res
}


#[test]
fn test() {
    use halo2_proofs::pasta::Fp;

    // let hash = word_to_polyhash("fault");
    // println!("hash {}", hash);
    let final_char = word_to_chars("fault");
    println!("{:?}", final_char);
    // let diff = compute_diff("fault", "fluff");
    // println!("diff {:?}", diff);

    // let fluff = word_to_chars("fluff");
    // let yellow_diff = {
    //     (0..WORD_LEN).fold(Fp::from(1), |expr, j| {
    //         expr * (Fp::from(20) - Fp::from(fluff[j]))
    //     })
    // };
    // println!("yellow_diff {:?}", yellow_diff);

    // let inv = Fp::from(14).invert();
    // println!("inv {:?}", inv);

}