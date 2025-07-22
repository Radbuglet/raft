pub struct CtfeFormatter<const CAP: usize> {
    buffer: [u8; CAP],
    len: usize,
    overflowed: bool,
}

impl<const CAP: usize> CtfeFormatter<CAP> {
    #[expect(clippy::new_without_default)]
    pub const fn new() -> Self {
        Self {
            buffer: [0u8; CAP],
            len: 0,
            overflowed: false,
        }
    }

    pub const fn write_str(&mut self, str: &str) {
        if self.overflowed {
            return;
        }

        if str.len() > CAP - self.len {
            self.overflowed = true;
            return;
        }

        sub_slice_mut(&mut self.buffer, self.len, str.len()).copy_from_slice(sub_slice(
            str.as_bytes(),
            0,
            str.len(),
        ));

        self.len += str.len();
    }

    pub const fn write_char(&mut self, ch: char) {
        let mut dst = [0u8; 4];
        self.write_str(ch.encode_utf8(&mut dst));
    }

    pub const fn write_u128(&mut self, mut val: u128) {
        // val = x_0 * 10^0 + x_1 * 10^1 + ... + x_n * 10^n
        //
        // n = floor(log_10(2 ^ 129 - 1)) = 38

        let mut n = 38;
        let mut exp = 10u128.pow(n);
        let mut had_first_digit = false;

        loop {
            let digit = val / exp;
            val -= digit * exp;
            exp /= 10;

            if digit != 0 || had_first_digit || n == 0 {
                self.write_char(['0', '1', '2', '3', '4', '5', '6', '7', '8', '9'][digit as usize]);
            }

            if digit != 0 {
                had_first_digit = true;
            }

            if n == 0 {
                break;
            }

            n -= 1;
        }
    }

    pub const fn write_i128(&mut self, val: i128) {
        if val < 0 {
            self.write_char('-');
        }

        self.write_u128(val.unsigned_abs());
    }

    #[must_use]
    pub const fn finish(&self) -> &str {
        unsafe { std::str::from_utf8_unchecked(sub_slice(&self.buffer, 0, self.len)) }
    }
}

const fn sub_slice<T>(parent: &[T], start: usize, len: usize) -> &[T] {
    assert!(start <= parent.len());
    assert!(len <= parent.len() - start);

    unsafe { std::slice::from_raw_parts(parent.as_ptr().add(start), len) }
}

const fn sub_slice_mut<T>(parent: &mut [T], start: usize, len: usize) -> &mut [T] {
    assert!(start <= parent.len());
    assert!(len <= parent.len() - start);

    unsafe { std::slice::from_raw_parts_mut(parent.as_mut_ptr().add(start), len) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_number() {
        fn check_num(v: u128) {
            let mut f = CtfeFormatter::<48>::new();
            f.write_u128(v);
            assert_eq!(f.finish(), v.to_string());
        }

        check_num(0);
        check_num(1);
        check_num(7);
        check_num(10);
        check_num(11);
        check_num(10003);
        check_num(u128::MAX);
        check_num(u128::MAX - 1);
    }
}
