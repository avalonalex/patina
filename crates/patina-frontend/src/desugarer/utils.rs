// DesugarError contains Value which is large, but boxing it would add complexity
// for minimal benefit in this interpreter context
#![allow(clippy::result_large_err)]

//! Utility functions for desugaring

use super::error::{DesugarError, Result};
use patina_ir::{Formals, ScopedParam, Symbol};
use patina_runtime::Value;
use std::collections::HashSet;
use std::rc::Rc;

/// Convert a Value list to a Vec
pub fn list_to_vec(value: &Value) -> Result<Vec<Value>> {
    let mut result = Vec::new();
    let mut current = value.clone();

    loop {
        match current {
            Value::Null => return Ok(result),
            Value::Pair(pair) => {
                let borrowed = pair.borrow();
                result.push(borrowed.0.clone());
                current = borrowed.1.clone();
            }
            _ => {
                return Err(DesugarError::ExpectedProperList(format!(
                    "Expected proper list, got improper list ending with {:?}",
                    current
                )));
            }
        }
    }
}

/// Convert a Vec to a Value list
pub fn vec_to_list(values: &[Value]) -> Value {
    values.iter().rev().fold(Value::Null, |acc, v| {
        Value::Pair(Rc::new(std::cell::RefCell::new((v.clone(), acc))))
    })
}

/// Expect exactly one argument
pub fn expect_one_arg(args: &Value, form: &str) -> Result<Value> {
    let args_vec = list_to_vec(args)?;
    if args_vec.len() != 1 {
        return Err(DesugarError::WrongArgCount {
            form: form.to_string(),
            expected: "1".to_string(),
            got: args_vec.len(),
        });
    }
    Ok(args_vec[0].clone())
}

/// Expect exactly two arguments
pub fn expect_two_args(args: &Value, form: &str) -> Result<(Value, Value)> {
    let args_vec = list_to_vec(args)?;
    if args_vec.len() != 2 {
        return Err(DesugarError::WrongArgCount {
            form: form.to_string(),
            expected: "2".to_string(),
            got: args_vec.len(),
        });
    }
    Ok((args_vec[0].clone(), args_vec[1].clone()))
}

/// Parse lambda syntax: (lambda formals body ...)
/// Returns (formals, body_exprs)
pub fn parse_lambda_syntax(args: &Value) -> Result<(Value, Vec<Value>)> {
    let args_vec = list_to_vec(args)?;

    if args_vec.is_empty() {
        return Err(DesugarError::InvalidSyntax(
            "lambda requires formals and body".to_string(),
        ));
    }

    let formals = args_vec[0].clone();
    let body = args_vec[1..].to_vec();

    if body.is_empty() {
        return Err(DesugarError::EmptyBody("lambda".to_string()));
    }

    Ok((formals, body))
}

/// Convert Scheme formals to Formals enum, preserving scope information
/// from identifiers for macro hygiene.
pub fn convert_formals(formals: &Value) -> Result<Formals> {
    match formals {
        // Fixed arity: (x y z)
        Value::Null => Ok(Formals::Fixed(vec![])),

        Value::Pair(_) => {
            // Either proper list (fixed) or improper list (mixed)
            let mut params: Vec<ScopedParam> = Vec::new();
            let mut current = formals.clone();

            loop {
                match current {
                    Value::Null => {
                        // Proper list - fixed arity
                        check_no_duplicates_scoped(&params, "lambda")?;
                        return Ok(Formals::Fixed(params));
                    }
                    Value::Pair(pair) => {
                        let borrowed = pair.borrow();
                        let car = &borrowed.0;
                        let cdr = borrowed.1.clone();

                        // Car must be a symbol or identifier
                        // IMPORTANT: Preserve scopes from identifiers for macro hygiene!
                        match car {
                            Value::Symbol(s) => {
                                params.push(ScopedParam::simple(s.clone()));
                            }
                            Value::Identifier(id) => {
                                // Preserve the identifier's scopes for hygiene
                                params.push(ScopedParam::with_scopes(
                                    id.name.clone(),
                                    id.scopes.clone(),
                                ));
                            }
                            _ => {
                                return Err(DesugarError::InvalidFormals(format!(
                                    "Parameter must be a symbol, got {:?}",
                                    car
                                )));
                            }
                        }

                        current = cdr;
                    }
                    Value::Symbol(rest) => {
                        // Improper list - mixed arity: (x y . rest)
                        check_no_duplicates_scoped(&params, "lambda")?;
                        let rest_param = ScopedParam::simple(rest.clone());
                        if params.iter().any(|p| p.name == rest) {
                            return Err(DesugarError::DuplicateParameter {
                                name: rest.to_string(),
                                context: "lambda".to_string(),
                            });
                        }
                        return Ok(Formals::Mixed {
                            fixed: params,
                            rest: rest_param,
                        });
                    }
                    Value::Identifier(id) => {
                        // Improper list with identifier - mixed arity: (x y . rest)
                        // Preserve scopes for hygiene
                        check_no_duplicates_scoped(&params, "lambda")?;
                        if params.iter().any(|p| p.name == id.name) {
                            return Err(DesugarError::DuplicateParameter {
                                name: id.name.to_string(),
                                context: "lambda".to_string(),
                            });
                        }
                        let rest_param =
                            ScopedParam::with_scopes(id.name.clone(), id.scopes.clone());
                        return Ok(Formals::Mixed {
                            fixed: params,
                            rest: rest_param,
                        });
                    }
                    _ => {
                        return Err(DesugarError::InvalidFormals(format!(
                            "Invalid formal parameters: {:?}",
                            current
                        )));
                    }
                }
            }
        }

        // Variadic: args
        Value::Symbol(s) => Ok(Formals::Variadic(ScopedParam::simple(s.clone()))),

        // Variadic with identifier - preserve scopes
        Value::Identifier(id) => Ok(Formals::Variadic(ScopedParam::with_scopes(
            id.name.clone(),
            id.scopes.clone(),
        ))),

        _ => Err(DesugarError::InvalidFormals(format!(
            "Invalid formal parameters: {:?}",
            formals
        ))),
    }
}

/// Check for duplicate parameters (by name only, for scoped params)
pub fn check_no_duplicates_scoped(params: &[ScopedParam], context: &str) -> Result<()> {
    let mut seen = HashSet::new();
    for param in params {
        if !seen.insert(param.name.as_ref()) {
            return Err(DesugarError::DuplicateParameter {
                name: param.name.to_string(),
                context: context.to_string(),
            });
        }
    }
    Ok(())
}

/// Extract all parameter names from a Formals structure
///
/// Used for tracking which names are shadowed by lambda parameters,
/// so they are not treated as macro calls.
pub fn formals_to_names(formals: &Formals) -> Vec<Rc<str>> {
    match formals {
        Formals::Fixed(params) => params.iter().map(|p| p.name.clone()).collect(),
        Formals::Variadic(p) => vec![p.name.clone()],
        Formals::Mixed { fixed, rest } => {
            let mut names: Vec<Rc<str>> = fixed.iter().map(|p| p.name.clone()).collect();
            names.push(rest.name.clone());
            names
        }
    }
}

/// Check for duplicate parameters (legacy, for symbol-only params)
#[allow(dead_code)]
pub fn check_no_duplicates(params: &[Symbol], context: &str) -> Result<()> {
    let mut seen = HashSet::new();
    for param in params {
        if !seen.insert(param.as_ref()) {
            return Err(DesugarError::DuplicateParameter {
                name: param.to_string(),
                context: context.to_string(),
            });
        }
    }
    Ok(())
}

/// Parse define function syntax: (name param1 param2 ...)
/// Returns (name, formals)
pub fn parse_define_function(pattern: &Value) -> Result<(Symbol, Value)> {
    let pattern_list = list_to_vec(pattern)?;

    if pattern_list.is_empty() {
        return Err(DesugarError::InvalidSyntax(
            "define function requires name and parameters".to_string(),
        ));
    }

    // Accept both Symbol and Identifier (from macro expansion)
    let name = match &pattern_list[0] {
        Value::Symbol(s) => s.clone(),
        Value::Identifier(id) => id.name.clone(),
        _ => {
            return Err(DesugarError::InvalidSyntax(
                "define function name must be a symbol".to_string(),
            ));
        }
    };

    // Convert remaining parameters back to list form for convert_formals
    let params = vec_to_list(&pattern_list[1..]);

    Ok((name, params))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_to_vec_proper_list() {
        let list = vec_to_list(&[Value::Integer(1), Value::Integer(2), Value::Integer(3)]);

        let vec = list_to_vec(&list).unwrap();
        assert_eq!(vec.len(), 3);
    }

    #[test]
    fn test_convert_formals_fixed() {
        let formals = vec_to_list(&[Value::symbol("x"), Value::symbol("y")]);

        let result = convert_formals(&formals).unwrap();
        assert!(matches!(result, Formals::Fixed(_)));
    }

    #[test]
    fn test_convert_formals_variadic() {
        let formals = Value::symbol("args");
        let result = convert_formals(&formals).unwrap();
        assert!(matches!(result, Formals::Variadic(_)));
    }
}
