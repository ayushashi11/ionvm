use std::time::Instant;

fn is_munchausen(number: i64, cache: &[i64; 10]) -> bool {
    let mut n = number;
    let mut total = 0;

    if n == 0 {
        return false;
    }

    while n > 0 {
        let digit = (n % 10) as usize;
        total += cache[digit];
        if total > number {
            return false;
        }
        n /= 10;
    }

    total == number
}

fn main() {
    let n_limit = 10000000;
    let cache = [0, 1, 4, 27, 256, 3125, 46656, 823543, 16777216, 387420489];

    let start = Instant::now();
    for n in 0..n_limit {
        if is_munchausen(n, &cache) {
            println!("{}", n);
        }
    }
    let duration = start.elapsed();
    println!("Time elapsed: {:?}", duration.as_secs_f64());
}
