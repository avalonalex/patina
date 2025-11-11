use rustyline::validate::{ValidationContext, ValidationResult, Validator};

pub struct SchemeValidator;

impl SchemeValidator {
    pub fn new() -> Self {
        SchemeValidator
    }

    fn count_parens(input: &str) -> (i32, i32) {
        let mut paren_balance = 0;
        let mut bracket_balance = 0;
        let mut in_string = false;
        let mut escape_next = false;
        let mut in_comment = false;

        for ch in input.chars() {
            if ch == '\n' {
                in_comment = false;
            }

            if in_comment {
                continue;
            }

            if escape_next {
                escape_next = false;
                continue;
            }

            match ch {
                '\\' if in_string => escape_next = true,
                '"' => in_string = !in_string,
                ';' if !in_string => in_comment = true,
                '(' if !in_string => paren_balance += 1,
                ')' if !in_string => paren_balance -= 1,
                '[' if !in_string => bracket_balance += 1,
                ']' if !in_string => bracket_balance -= 1,
                _ => {}
            }
        }

        (paren_balance, bracket_balance)
    }
}

impl Validator for SchemeValidator {
    fn validate(&self, ctx: &mut ValidationContext) -> rustyline::Result<ValidationResult> {
        let input = ctx.input();
        let (paren_balance, bracket_balance) = Self::count_parens(input);

        if paren_balance > 0 || bracket_balance > 0 {
            // More opening than closing - continue to next line
            Ok(ValidationResult::Incomplete)
        } else if paren_balance < 0 || bracket_balance < 0 {
            // More closing than opening - syntax error but accept it so user sees error
            Ok(ValidationResult::Valid(None))
        } else {
            // Balanced - good to evaluate
            Ok(ValidationResult::Valid(None))
        }
    }
}
