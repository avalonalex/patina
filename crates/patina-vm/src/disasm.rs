//! Disassembler — pretty-prints `CodeObject` bytecode for debugging.
//!
//! Usage in the VM REPL:
//!   `,dis (expression)`  — compile and print bytecode without executing

use crate::types::code_object::{Arity, CodeObject, CodeObjectId};
use crate::types::instruction::Instruction;
use std::collections::HashMap;

/// Pretty-print a `CodeObject` and all nested lambdas reachable from it.
///
/// `nested` holds the lambdas compiled alongside `top` — the shape the
/// compiler returns.
pub fn disassemble(top: &CodeObject, nested: &[CodeObject]) {
    let by_id: HashMap<CodeObjectId, &CodeObject> = nested.iter().map(|c| (c.id, c)).collect();
    let mut visited = std::collections::HashSet::new();
    disassemble_one(top, 0, &by_id, &mut visited);
}

fn disassemble_one(
    co: &CodeObject,
    depth: usize,
    by_id: &HashMap<CodeObjectId, &CodeObject>,
    visited: &mut std::collections::HashSet<CodeObjectId>,
) {
    if !visited.insert(co.id) {
        return;
    }

    let indent = "  ".repeat(depth);
    let name = co.name.as_deref().unwrap_or("<anonymous>");
    let arity_str = match &co.arity {
        Arity::Fixed(n) => format!("{} args", n),
        Arity::Variadic(n) => format!("≥{} args (variadic)", n),
    };

    println!(
        "{}┌─ CodeObject #{} \"{}\" ({}, {} regs, {} instructions)",
        indent,
        co.id.0,
        name,
        arity_str,
        co.num_regs,
        co.instructions.len(),
    );

    // Collect nested lambda ids referenced by MakeClosure instructions.
    let mut nested_ids: Vec<CodeObjectId> = Vec::new();

    for (pc, instr) in co.instructions.iter().enumerate() {
        let line = format_instruction(instr, &mut nested_ids);
        println!("{}│  {:>4}  {}", indent, pc, line);
    }

    println!("{}└─ end #{}", indent, co.id.0);

    // Recursively disassemble nested lambdas in the order they appear.
    for id in nested_ids {
        if let Some(nested) = by_id.get(&id) {
            println!();
            disassemble_one(nested, depth + 1, by_id, visited);
        }
    }
}

/// Format an inline primitive opcode (Track P P3): `Mnemonic  rD ← name(rA, …)`.
fn fmt_inline_prim(
    mnemonic: &str,
    dst: u16,
    name: &patina_core::core_expr::Symbol,
    args: &[u16],
) -> String {
    let a: Vec<String> = args.iter().map(|r| format!("r{}", r)).collect();
    fmt_prim_operands(mnemonic, dst, name, &a)
}

fn fmt_imm_prim(
    mnemonic: &str,
    dst: u16,
    name: &patina_core::core_expr::Symbol,
    a: u16,
    imm: patina_core::tagged_value::TaggedValue,
) -> String {
    fmt_prim_operands(
        mnemonic,
        dst,
        name,
        &[format!("r{}", a), format!("{:?}", imm)],
    )
}

/// Shared layout for every inline-primitive mnemonic line.
fn fmt_prim_operands(
    mnemonic: &str,
    dst: u16,
    name: &patina_core::core_expr::Symbol,
    operands: &[String],
) -> String {
    format!(
        "{:<13}r{} ← {}({})",
        mnemonic,
        dst,
        name,
        operands.join(", ")
    )
}

pub fn format_instruction(instr: &Instruction, nested: &mut Vec<CodeObjectId>) -> String {
    match instr {
        Instruction::LoadImmediate { dst, val } => {
            format!("LoadImm      r{} ← {:?}", dst, val)
        }
        Instruction::LoadConst { dst, idx } => {
            format!("LoadConst    r{} ← const[{}]", dst, idx)
        }
        Instruction::Move { dst, src } => {
            format!("Move         r{} ← r{}", dst, src)
        }
        Instruction::LoadClosure { dst, slot } => {
            format!("LoadClosure  r{} ← closure[{}]", dst, slot)
        }
        Instruction::StoreClosure { slot, src } => {
            format!("StoreClosure closure[{}] ← r{}", slot, src)
        }
        Instruction::LoadGlobal { dst, name } => {
            format!("LoadGlobal   r{} ← globals[{}]", dst, name)
        }
        Instruction::StoreGlobal { name, src } => {
            format!("StoreGlobal  globals[{}] ← r{}", name, src)
        }
        Instruction::AllocCell { dst, src } => {
            format!("AllocCell    r{} ← box(r{})", dst, src)
        }
        Instruction::ReadCell { dst, cell } => {
            format!("ReadCell     r{} ← *r{}", dst, cell)
        }
        Instruction::WriteCell { cell, src } => {
            format!("WriteCell    *r{} ← r{}", cell, src)
        }
        Instruction::MakeClosure {
            dst,
            code_id,
            free_vars,
        } => {
            nested.push(*code_id);
            let fv: Vec<String> = free_vars.iter().map(|r| format!("r{}", r)).collect();
            format!(
                "MakeClosure  r{} ← closure(#{}, [{}])",
                dst,
                code_id.0,
                fv.join(", ")
            )
        }
        Instruction::Jump { target } => {
            format!("Jump         → {}", target)
        }
        Instruction::JumpIf { cond, target } => {
            format!("JumpIf       r{} → {}", cond, target)
        }
        Instruction::JumpUnless { cond, target } => {
            format!("JumpUnless   r{} → {}", cond, target)
        }
        Instruction::Call { func, args, dst } => {
            let a: Vec<String> = args.iter().map(|r| format!("r{}", r)).collect();
            format!("Call         r{} ← r{}({})", dst, func, a.join(", "))
        }
        Instruction::TailCall { func, args } => {
            let a: Vec<String> = args.iter().map(|r| format!("r{}", r)).collect();
            format!("TailCall     r{}({})", func, a.join(", "))
        }
        Instruction::Apply { func, args, dst } => {
            let a: Vec<String> = args.iter().map(|r| format!("r{}", r)).collect();
            format!("Apply        r{} ← r{}({}...)", dst, func, a.join(", "))
        }
        Instruction::TailApply { func, args } => {
            let a: Vec<String> = args.iter().map(|r| format!("r{}", r)).collect();
            format!("TailApply    r{}({}...)", func, a.join(", "))
        }
        Instruction::Return { val } => {
            format!("Return       r{}", val)
        }
        Instruction::CallPrimitive {
            func_id,
            name,
            args,
            dst,
        } => {
            let a: Vec<String> = args.iter().map(|r| format!("r{}", r)).collect();
            format!(
                "CallPrim     r{} ← {}#{}({})",
                dst,
                name,
                func_id.0,
                a.join(", ")
            )
        }
        Instruction::Add {
            a, b, dst, name, ..
        } => fmt_inline_prim("Add", *dst, name, &[*a, *b]),
        Instruction::Sub {
            a, b, dst, name, ..
        } => fmt_inline_prim("Sub", *dst, name, &[*a, *b]),
        Instruction::Mul {
            a, b, dst, name, ..
        } => fmt_inline_prim("Mul", *dst, name, &[*a, *b]),
        Instruction::Lt {
            a, b, dst, name, ..
        } => fmt_inline_prim("Lt", *dst, name, &[*a, *b]),
        Instruction::NumEq {
            a, b, dst, name, ..
        } => fmt_inline_prim("NumEq", *dst, name, &[*a, *b]),
        Instruction::Eq {
            a, b, dst, name, ..
        } => fmt_inline_prim("Eq", *dst, name, &[*a, *b]),
        Instruction::Cons {
            a, b, dst, name, ..
        } => fmt_inline_prim("Cons", *dst, name, &[*a, *b]),
        Instruction::Car { src, dst, name, .. } => fmt_inline_prim("Car", *dst, name, &[*src]),
        Instruction::Cdr { src, dst, name, .. } => fmt_inline_prim("Cdr", *dst, name, &[*src]),
        Instruction::Not { src, dst, name, .. } => fmt_inline_prim("Not", *dst, name, &[*src]),
        Instruction::NotJumpUnless {
            src, dst, target, ..
        } => {
            format!("NotJumpUnless r{} ← not(r{}) → {}", dst, src, target)
        }
        Instruction::AddImm {
            a, imm, dst, name, ..
        } => fmt_imm_prim("AddImm", *dst, name, *a, *imm),
        Instruction::SubImm {
            a, imm, dst, name, ..
        } => fmt_imm_prim("SubImm", *dst, name, *a, *imm),
        Instruction::LtImm {
            a, imm, dst, name, ..
        } => fmt_imm_prim("LtImm", *dst, name, *a, *imm),
        Instruction::NumEqImm {
            a, imm, dst, name, ..
        } => fmt_imm_prim("NumEqImm", *dst, name, *a, *imm),
        Instruction::NullP { src, dst, name, .. } => fmt_inline_prim("NullP", *dst, name, &[*src]),
        Instruction::PairP { src, dst, name, .. } => fmt_inline_prim("PairP", *dst, name, &[*src]),
        Instruction::VectorP { src, dst, name, .. } => {
            fmt_inline_prim("VectorP", *dst, name, &[*src])
        }
        Instruction::VectorRef {
            v, i, dst, name, ..
        } => fmt_inline_prim("VectorRef", *dst, name, &[*v, *i]),
        Instruction::VectorSet {
            v,
            i,
            val,
            dst,
            name,
            ..
        } => fmt_inline_prim("VectorSet", *dst, name, &[*v, *i, *val]),
        Instruction::ReturnMulti { vals } => {
            let v: Vec<String> = vals.iter().map(|r| format!("r{}", r)).collect();
            format!("ReturnMulti  ({})", v.join(", "))
        }
        Instruction::ReceiveValues { dsts } => {
            let d: Vec<String> = dsts.iter().map(|r| format!("r{}", r)).collect();
            format!("RecvValues   ({})", d.join(", "))
        }
        Instruction::Define { name, src } => {
            format!("Define       globals[{}] ← r{}", name, src)
        }
        Instruction::CallWithPrompt {
            body,
            tag,
            handler,
            dst,
        } => {
            format!(
                "CallWithPrompt r{} ← prompt(tag=r{}, body=r{}, handler=r{})",
                dst, tag, body, handler
            )
        }
        Instruction::AbortToPrompt { tag, val } => {
            format!("AbortToPrompt tag=r{}, val=r{}", tag, val)
        }
        Instruction::CaptureComposable { dst, tag } => {
            format!("CaptureComposable r{} ← r{}", dst, tag)
        }
        Instruction::InvokeContinuation {
            cont,
            val,
            composable,
        } => {
            format!("InvokeCont   r{}(r{}) composable={}", cont, val, composable)
        }
        Instruction::CallWithValues {
            dst,
            consumer,
            producer_result,
        } => {
            format!(
                "CallWithValues r{} ← r{}(values-or r{})",
                dst, consumer, producer_result
            )
        }
        Instruction::TailCallWithValues {
            consumer,
            producer_result,
        } => {
            format!(
                "TailCallWithValues r{}(values-or r{})",
                consumer, producer_result
            )
        }
        Instruction::PushWind { before, after } => {
            format!("PushWind      before=r{} after=r{}", before, after)
        }
        Instruction::PopWind => "PopWind".to_string(),
        Instruction::Nop => "Nop".to_string(),
    }
}
