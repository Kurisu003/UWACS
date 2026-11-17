//! Tiny calculator for number fields: "778/2", "W-768", "(1080-762)/2".
//! + - * / % and parentheses, decimals, and the variables W and H (screen width/height).

pub fn eval(src: &str, w: f64, h: f64) -> Result<f64, String> {
    let toks = lex(src)?;
    if toks.is_empty() {
        return Err("empty".into());
    }
    let mut p = Parser { t: &toks, i: 0, w, h };
    let v = p.expr()?;
    if p.i != toks.len() {
        return Err(format!("unexpected '{}'", p.t[p.i].show()));
    }
    if !v.is_finite() {
        return Err("division by zero".into());
    }
    Ok(v)
}

/// Rounded to whole pixels.
pub fn eval_i(src: &str, w: f64, h: f64) -> Result<i32, String> {
    eval(src, w, h).map(|v| v.round() as i32)
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Var(char),
    Op(char),
}

impl Tok {
    fn show(&self) -> String {
        match self {
            Tok::Num(n) => n.to_string(),
            Tok::Var(c) | Tok::Op(c) => c.to_string(),
        }
    }
}

fn lex(s: &str) -> Result<Vec<Tok>, String> {
    let c: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
        } else if ch.is_ascii_digit() || ch == '.' || ch == ',' {
            let st = i;
            while i < c.len() && (c[i].is_ascii_digit() || c[i] == '.' || c[i] == ',') {
                i += 1;
            }
            let t: String = c[st..i].iter().map(|&x| if x == ',' { '.' } else { x }).collect();
            out.push(Tok::Num(t.parse().map_err(|_| format!("bad number '{t}'"))?));
        } else if "+-*/%()x×÷".contains(ch) {
            let op = match ch {
                'x' | '×' => '*',
                '÷' => '/',
                o => o,
            };
            out.push(Tok::Op(op));
            i += 1;
        } else if ch == 'W' || ch == 'H' || ch == 'w' || ch == 'h' {
            out.push(Tok::Var(ch.to_ascii_uppercase()));
            i += 1;
        } else {
            return Err(format!("unknown '{ch}'"));
        }
    }
    Ok(out)
}

struct Parser<'a> {
    t: &'a [Tok],
    i: usize,
    w: f64,
    h: f64,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }

    fn expr(&mut self) -> Result<f64, String> {
        let mut v = self.term()?;
        while let Some(Tok::Op(o @ ('+' | '-'))) = self.peek().cloned() {
            self.i += 1;
            let r = self.term()?;
            v = if o == '+' { v + r } else { v - r };
        }
        Ok(v)
    }

    fn term(&mut self) -> Result<f64, String> {
        let mut v = self.unary()?;
        loop {
            match self.peek().cloned() {
                Some(Tok::Op(o @ ('*' | '/' | '%'))) => {
                    self.i += 1;
                    let r = self.unary()?;
                    v = match o {
                        '*' => v * r,
                        '/' => v / r,
                        _ => v % r,
                    };
                }
                // implicit multiplication: 2(3) or 2W
                Some(Tok::Op('(')) | Some(Tok::Var(_)) => {
                    let r = self.unary()?;
                    v *= r;
                }
                _ => return Ok(v),
            }
        }
    }

    fn unary(&mut self) -> Result<f64, String> {
        match self.peek().cloned() {
            Some(Tok::Op('-')) => {
                self.i += 1;
                Ok(-self.unary()?)
            }
            Some(Tok::Op('+')) => {
                self.i += 1;
                self.unary()
            }
            _ => self.atom(),
        }
    }

    fn atom(&mut self) -> Result<f64, String> {
        match self.peek().cloned() {
            Some(Tok::Num(n)) => {
                self.i += 1;
                Ok(n)
            }
            Some(Tok::Var(c)) => {
                self.i += 1;
                Ok(if c == 'W' { self.w } else { self.h })
            }
            Some(Tok::Op('(')) => {
                self.i += 1;
                let v = self.expr()?;
                if self.peek() != Some(&Tok::Op(')')) {
                    return Err("missing ')'".into());
                }
                self.i += 1;
                Ok(v)
            }
            Some(t) => Err(format!("unexpected '{}'", t.show())),
            None => Err("ends too early".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn calc() {
        assert_eq!(eval_i("778/2", 0.0, 0.0), Ok(389));
        assert_eq!(eval_i("(1080-762)/2", 0.0, 0.0), Ok(159));
        assert_eq!(eval_i("W-768", 1024.0, 768.0), Ok(256));
        assert_eq!(eval_i("-5+2*3", 0.0, 0.0), Ok(1));
        assert_eq!(eval_i("2(3+1)", 0.0, 0.0), Ok(8));
        assert_eq!(eval_i("9", 0.0, 0.0), Ok(9));
        assert!(eval("1/0", 0.0, 0.0).is_err());
        assert!(eval("3+", 0.0, 0.0).is_err());
        assert!(eval("abc", 0.0, 0.0).is_err());
    }
}
