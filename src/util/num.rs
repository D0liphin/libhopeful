/// `ceil(log2(n))`
pub fn log2ceil(n: u64) -> u32 {
    if n <= 1 {
        1
    } else {
        let bitlen = u64::BITS - n.leading_zeros();
        // 1001 = 9 -> 4
        // 1000 = 8 -> 3
        if 1 << (bitlen - 1) == n {
            bitlen - 1
        } else {
            bitlen
        }
    }
}

/// Round up `n` to the nearest `to`
pub fn round_up(n: usize, to: usize) -> usize {
    to * ((n + to - 1) / to)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn log2ceil_computes_correctly() {
        assert_eq!(log2ceil(9), 4);
        assert_eq!(log2ceil(15), 4);
        assert_eq!(log2ceil(8), 3);
        assert_eq!(log2ceil(0), 1);
        assert_eq!(log2ceil(1), 1);
        assert_eq!(log2ceil(2), 1);
    }
}
