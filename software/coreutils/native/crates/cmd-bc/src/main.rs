use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive, Zero};
use std::io::{self, Read};

const MAX_INPUT_BYTES: u64 = 1_048_576;
const MAX_SCALE: usize = 10_000;
const MAX_EXPONENT: u32 = 100_000;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Decimal {
    coefficient: BigInt,
    scale: usize,
}

impl Decimal {
    fn integer(value: BigInt) -> Self {
        Self {
            coefficient: value,
            scale: 0,
        }
    }

    fn align(&self, scale: usize) -> BigInt {
        &self.coefficient * power_of_ten(scale - self.scale)
    }

    fn add(&self, other: &Self) -> Self {
        let scale = self.scale.max(other.scale);
        Self {
            coefficient: self.align(scale) + other.align(scale),
            scale,
        }
    }

    fn sub(&self, other: &Self) -> Self {
        let scale = self.scale.max(other.scale);
        Self {
            coefficient: self.align(scale) - other.align(scale),
            scale,
        }
    }

    fn mul(&self, other: &Self, global_scale: usize) -> Self {
        let raw_scale = self.scale + other.scale;
        let scale = raw_scale.min(global_scale.max(self.scale).max(other.scale));
        let mut coefficient = &self.coefficient * &other.coefficient;
        if raw_scale > scale {
            coefficient /= power_of_ten(raw_scale - scale);
        }
        Self { coefficient, scale }
    }

    fn div(&self, other: &Self, scale: usize) -> Result<Self, String> {
        if other.coefficient.is_zero() {
            return Err("divide by zero".into());
        }
        let numerator = &self.coefficient * power_of_ten(scale + other.scale);
        let denominator = &other.coefficient * power_of_ten(self.scale);
        Ok(Self {
            coefficient: numerator / denominator,
            scale,
        })
    }

    fn pow(&self, exponent: u32, global_scale: usize) -> Self {
        let raw_scale = self.scale.saturating_mul(exponent as usize);
        let scale = raw_scale.min(global_scale.max(self.scale));
        let mut coefficient = self.coefficient.pow(exponent);
        if raw_scale > scale {
            coefficient /= power_of_ten(raw_scale - scale);
        }
        Self { coefficient, scale }
    }

    fn compare(&self, other: &Self) -> std::cmp::Ordering {
        let scale = self.scale.max(other.scale);
        self.align(scale).cmp(&other.align(scale))
    }

    fn as_nonnegative_usize(&self, name: &str) -> Result<usize, String> {
        if self.scale != 0 || self.coefficient.is_negative() {
            return Err(format!("{name} must be a non-negative integer"));
        }
        self.coefficient
            .to_usize()
            .ok_or_else(|| format!("{name} is too large"))
    }

    fn format(&self, output_base: u32) -> String {
        if output_base != 10 && self.scale == 0 {
            return self.coefficient.to_str_radix(output_base).to_uppercase();
        }
        if self.scale == 0 {
            return self.coefficient.to_string();
        }

        let negative = self.coefficient.is_negative();
        let mut digits = self.coefficient.abs().to_string();
        if digits.len() <= self.scale {
            digits.insert_str(0, &"0".repeat(self.scale + 1 - digits.len()));
        }
        let split = digits.len() - self.scale;
        let integer = &digits[..split];
        let fraction = &digits[split..];
        let sign = if negative { "-" } else { "" };
        if integer == "0" {
            format!("{sign}.{fraction}")
        } else {
            format!("{sign}{integer}.{fraction}")
        }
    }
}

fn power_of_ten(exponent: usize) -> BigInt {
    BigInt::from(10u8).pow(exponent as u32)
}

struct Parser<'a> {
    input: &'a [u8],
    position: usize,
    input_base: u32,
    scale: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str, input_base: u32, scale: usize) -> Self {
        Self {
            input: input.as_bytes(),
            position: 0,
            input_base,
            scale,
        }
    }

    fn parse(mut self) -> Result<Decimal, String> {
        let value = self.comparison()?;
        self.whitespace();
        if self.position != self.input.len() {
            return Err(format!("unexpected input at byte {}", self.position));
        }
        Ok(value)
    }

    fn comparison(&mut self) -> Result<Decimal, String> {
        let left = self.additive()?;
        self.whitespace();
        let operator = ["<=", ">=", "==", "!=", "<", ">"]
            .into_iter()
            .find(|operator| self.remaining().starts_with(operator.as_bytes()));
        let Some(operator) = operator else {
            return Ok(left);
        };
        self.position += operator.len();
        let right = self.additive()?;
        let ordering = left.compare(&right);
        let matches = match operator {
            "<" => ordering.is_lt(),
            ">" => ordering.is_gt(),
            "<=" => !ordering.is_gt(),
            ">=" => !ordering.is_lt(),
            "==" => ordering.is_eq(),
            "!=" => !ordering.is_eq(),
            _ => unreachable!(),
        };
        Ok(Decimal::integer(BigInt::from(matches as u8)))
    }

    fn additive(&mut self) -> Result<Decimal, String> {
        let mut value = self.multiplicative()?;
        loop {
            self.whitespace();
            let operator = self.peek();
            if !matches!(operator, Some(b'+') | Some(b'-')) {
                return Ok(value);
            }
            self.position += 1;
            let right = self.multiplicative()?;
            value = if operator == Some(b'+') {
                value.add(&right)
            } else {
                value.sub(&right)
            };
        }
    }

    fn multiplicative(&mut self) -> Result<Decimal, String> {
        let mut value = self.exponential()?;
        loop {
            self.whitespace();
            let operator = self.peek();
            if !matches!(operator, Some(b'*') | Some(b'/')) {
                return Ok(value);
            }
            self.position += 1;
            let right = self.exponential()?;
            value = if operator == Some(b'*') {
                value.mul(&right, self.scale)
            } else {
                value.div(&right, self.scale)?
            };
        }
    }

    fn exponential(&mut self) -> Result<Decimal, String> {
        let value = self.unary()?;
        self.whitespace();
        if self.peek() != Some(b'^') {
            return Ok(value);
        }
        self.position += 1;
        let exponent = self.exponential()?;
        let exponent = exponent.as_nonnegative_usize("exponent")?;
        let exponent = u32::try_from(exponent)
            .ok()
            .filter(|value| *value <= MAX_EXPONENT)
            .ok_or_else(|| format!("exponent exceeds limit {MAX_EXPONENT}"))?;
        Ok(value.pow(exponent, self.scale))
    }

    fn unary(&mut self) -> Result<Decimal, String> {
        self.whitespace();
        if self.peek() == Some(b'+') {
            self.position += 1;
            return self.unary();
        }
        if self.peek() == Some(b'-') {
            self.position += 1;
            let mut value = self.unary()?;
            value.coefficient = -value.coefficient;
            return Ok(value);
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Decimal, String> {
        self.whitespace();
        if self.peek() == Some(b'(') {
            self.position += 1;
            let value = self.comparison()?;
            self.whitespace();
            if self.peek() != Some(b')') {
                return Err("missing closing parenthesis".into());
            }
            self.position += 1;
            return Ok(value);
        }

        let start = self.position;
        while self
            .peek()
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'.')
        {
            self.position += 1;
        }
        if start == self.position {
            return Err(format!("expected number at byte {}", self.position));
        }
        let text = std::str::from_utf8(&self.input[start..self.position])
            .map_err(|_| "number is not UTF-8")?;
        parse_number(text, self.input_base)
    }

    fn whitespace(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.position += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.position).copied()
    }

    fn remaining(&self) -> &[u8] {
        &self.input[self.position..]
    }
}

fn parse_number(text: &str, input_base: u32) -> Result<Decimal, String> {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    let digits = format!("{whole}{fraction}");
    if digits.is_empty() {
        return Err("empty number".into());
    }
    let coefficient = BigInt::parse_bytes(digits.as_bytes(), input_base)
        .ok_or_else(|| format!("invalid base-{input_base} number: {text}"))?;
    Ok(Decimal {
        coefficient,
        scale: fraction.len(),
    })
}

#[derive(Debug)]
struct Calculator {
    input_base: u32,
    output_base: u32,
    scale: usize,
}

impl Default for Calculator {
    fn default() -> Self {
        Self {
            input_base: 10,
            output_base: 10,
            scale: 0,
        }
    }
}

impl Calculator {
    fn execute(&mut self, input: &str) -> Result<Vec<String>, String> {
        let mut output = Vec::new();
        for statement in input.split([';', '\n']) {
            let statement = statement.trim();
            if statement.is_empty() {
                continue;
            }
            if let Some((name, expression)) = statement.split_once('=') {
                if matches!(name.trim(), "scale" | "ibase" | "obase") {
                    let value = Parser::new(expression, self.input_base, self.scale).parse()?;
                    let value = value.as_nonnegative_usize(name.trim())?;
                    match name.trim() {
                        "scale" if value <= MAX_SCALE => self.scale = value,
                        "scale" => return Err(format!("scale exceeds limit {MAX_SCALE}")),
                        "ibase" if (2..=16).contains(&value) => self.input_base = value as u32,
                        "obase" if (2..=16).contains(&value) => self.output_base = value as u32,
                        name => return Err(format!("{name} must be in 2..=16")),
                    }
                    continue;
                }
            }

            let expression = if let Some(rest) = statement.strip_prefix("if") {
                let rest = rest.trim_start();
                if !rest.starts_with('(') {
                    return Err("if requires a parenthesized condition".into());
                }
                let closing = matching_parenthesis(rest)?;
                let condition =
                    Parser::new(&rest[1..closing], self.input_base, self.scale).parse()?;
                if condition.coefficient.is_zero() {
                    continue;
                }
                rest[closing + 1..].trim()
            } else {
                statement
            };
            let value = Parser::new(expression, self.input_base, self.scale).parse()?;
            output.push(value.format(self.output_base));
        }
        Ok(output)
    }
}

fn matching_parenthesis(input: &str) -> Result<usize, String> {
    let mut depth = 0usize;
    for (index, byte) in input.bytes().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Ok(index);
                }
            }
            _ => {}
        }
    }
    Err("if condition is missing a closing parenthesis".into())
}

fn run() -> Result<(), String> {
    let mut input = String::new();
    io::stdin()
        .take(MAX_INPUT_BYTES + 1)
        .read_to_string(&mut input)
        .map_err(|error| format!("read stdin: {error}"))?;
    if input.len() as u64 > MAX_INPUT_BYTES {
        return Err(format!("input exceeds {MAX_INPUT_BYTES} bytes"));
    }
    let output = Calculator::default().execute(&input)?;
    for line in output {
        println!("{line}");
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("bc: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calculate(input: &str) -> Vec<String> {
        Calculator::default().execute(input).unwrap()
    }

    #[test]
    fn supports_xfstests_integer_and_base_expressions() {
        assert_eq!(calculate("obase=8; 3840"), ["7400"]);
        assert_eq!(
            calculate("2^63 - 2\n2^63 - 1"),
            ["9223372036854775806", "9223372036854775807"]
        );
        assert_eq!(calculate("ibase=16; FF"), ["255"]);
    }

    #[test]
    fn supports_scaled_arithmetic_and_conditionals() {
        assert_eq!(calculate("scale=5; 100-5*0.01*100"), ["95.00"]);
        assert_eq!(calculate("scale=5; 1/3"), [".33333"]);
        assert_eq!(calculate("if (4 <= 5) 1; if (4 > 5) 0"), ["1"]);
    }
}
