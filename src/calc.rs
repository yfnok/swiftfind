//! Tiny expression evaluator for the inline calculator.

struct P<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> P<'a> {
    fn ws(&mut self) {
        while self.i < self.s.len() && (self.s[self.i] as char).is_whitespace() {
            self.i += 1;
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.i).copied()
    }
    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        loop {
            match self.peek() {
                Some(b'+') => {
                    self.i += 1;
                    v += self.term()?;
                }
                Some(b'-') => {
                    self.i += 1;
                    v -= self.term()?;
                }
                _ => return Some(v),
            }
        }
    }
    fn term(&mut self) -> Option<f64> {
        let mut v = self.power()?;
        loop {
            match self.peek() {
                Some(b'*') | Some(b'x') | Some(b'X') => {
                    self.i += 1;
                    v *= self.power()?;
                }
                Some(b'/') => {
                    self.i += 1;
                    v /= self.power()?;
                }
                Some(b'%') => {
                    self.i += 1;
                    v %= self.power()?;
                }
                _ => return Some(v),
            }
        }
    }
    fn power(&mut self) -> Option<f64> {
        let base = self.unary()?;
        if self.peek() == Some(b'^') {
            self.i += 1;
            let exp = self.power()?; // right associative
            return Some(base.powf(exp));
        }
        Some(base)
    }
    fn unary(&mut self) -> Option<f64> {
        match self.peek() {
            Some(b'-') => {
                self.i += 1;
                Some(-self.unary()?)
            }
            Some(b'+') => {
                self.i += 1;
                self.unary()
            }
            _ => self.atom(),
        }
    }
    fn atom(&mut self) -> Option<f64> {
        match self.peek()? {
            b'(' => {
                self.i += 1;
                let v = self.expr()?;
                if self.peek()? != b')' {
                    return None;
                }
                self.i += 1;
                Some(v)
            }
            c if c.is_ascii_digit() || c == b'.' => {
                let start = self.i;
                while self.i < self.s.len()
                    && (self.s[self.i].is_ascii_digit()
                        || self.s[self.i] == b'.'
                        || self.s[self.i] == b'_')
                {
                    self.i += 1;
                }
                std::str::from_utf8(&self.s[start..self.i])
                    .ok()?
                    .replace('_', "")
                    .parse()
                    .ok()
            }
            c if c.is_ascii_alphabetic() => {
                let start = self.i;
                while self.i < self.s.len() && self.s[self.i].is_ascii_alphabetic() {
                    self.i += 1;
                }
                let name = std::str::from_utf8(&self.s[start..self.i])
                    .ok()?
                    .to_ascii_lowercase();
                match name.as_str() {
                    "pi" => return Some(std::f64::consts::PI),
                    "e" => return Some(std::f64::consts::E),
                    _ => {}
                }
                if self.peek()? != b'(' {
                    return None;
                }
                self.i += 1;
                let a = self.expr()?;
                if self.peek()? != b')' {
                    return None;
                }
                self.i += 1;
                Some(match name.as_str() {
                    "sqrt" => a.sqrt(),
                    "sin" => a.sin(),
                    "cos" => a.cos(),
                    "tan" => a.tan(),
                    "ln" => a.ln(),
                    "log" => a.log10(),
                    "abs" => a.abs(),
                    "round" => a.round(),
                    "floor" => a.floor(),
                    "ceil" => a.ceil(),
                    _ => return None,
                })
            }
            _ => None,
        }
    }
}

/// Evaluate `input` if it looks like arithmetic (contains an operator or a function call).
pub fn eval(input: &str) -> Option<String> {
    let t = input.trim().trim_start_matches('=').trim();
    if t.is_empty() {
        return None;
    }
    let looks_math = t
        .bytes()
        .any(|b| matches!(b, b'+' | b'-' | b'*' | b'/' | b'^' | b'%' | b'('))
        && t.bytes().any(|b| b.is_ascii_digit() || b == b'.')
        || t.to_ascii_lowercase()
            .starts_with(|c: char| c.is_ascii_alphabetic())
            && t.contains('(');
    if !looks_math {
        return None;
    }
    let mut p = P {
        s: t.as_bytes(),
        i: 0,
    };
    let v = p.expr()?;
    p.ws();
    if p.i != t.len() || !v.is_finite() {
        return None;
    }
    Some(format_number(v))
}

fn format_number(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let s = format!("{:.10}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::eval;

    #[test]
    fn basic() {
        assert_eq!(eval("1+2*3").as_deref(), Some("7"));
        assert_eq!(eval("(1+2)*3").as_deref(), Some("9"));
        assert_eq!(eval("2^3^2").as_deref(), Some("512"));
        assert_eq!(eval("-4 + 10 / 4").as_deref(), Some("-1.5"));
        assert_eq!(eval("sqrt(16)+1").as_deref(), Some("5"));
        assert_eq!(eval("10 % 4").as_deref(), Some("2"));
    }

    #[test]
    fn not_math() {
        assert_eq!(eval("chrome"), None);
        assert_eq!(eval("123"), None);
        assert_eq!(eval("1+"), None);
        assert_eq!(eval("1/0"), None);
    }
}
