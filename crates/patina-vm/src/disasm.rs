//! Disassembler — pretty-prints `CodeObject` bytecode for debugging.
//!
//! Usage in the VM REPL:
//!   `,dis (expression)`  — compile and print bytecode without executing

use crate::types::code_object::{Arity, CodeObject, CodeObjectId};
use crate::types::instruction::Instruction;
use std::collections::HashMap;
use std::rc::Rc;

/// Pretty-print a `CodeObject` and all nested lambdas reachable from it.
///
/// `code_store` maps `CodeObjectId → CodeObject` for nested lambdas.
/// Pass the full `VmState::code_store` to get recursive disassembly.
pub fn disassemble(top: &CodeObject, code_store: &HashMap<CodeObjectId, Rc<CodeObject>>) {
    let mut visited = std::collections::HashSet::new();
    disassemble_one(top, 0, code_store, &mut visited);
}

fn disassemble_one(
    co: &CodeObject,
    depth: usize,
    code_store: &HashMap<CodeObjectId, Rc<CodeObject>>,
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
        if let Some(nested) = code_store.get(&id) {
            println!();
            disassemble_one(nested, depth + 1, code_store, visited);
        }
    }
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
        Instruction::CallPrimitive { func_id, args, dst } => {
            let a: Vec<String> = args.iter().map(|r| format!("r{}", r)).collect();
            format!(
                "CallPrim     r{} ← prim#{}({})",
                dst,
                func_id.0,
                a.join(", ")
            )
        }
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
