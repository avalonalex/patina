//! The prompt API on the tree-walker: `call-with-continuation-prompt`,
//! `abort-current-continuation`, and invoking the composable continuation an
//! abort hands its handler. Issue #169.
//!
//! # A prompt is a boundary in the continuation
//!
//! The VM's continuation is a stack, so its prompt is a depth in three stacks
//! and its delimited continuation a copied slice that has to be relocated on
//! invoke (`docs/VM_RUNTIME.md` §5.5). Here the continuation is a value, and
//! the prompt is a value in it: the prompt body runs with
//! [`ContValue::PromptBoundary`] as its continuation, and the caller's
//! continuation waits in a [`PromptFrame`] on the prompt stack. Everything
//! else follows from that one choice:
//!
//! - **Normal return.** The body's value reaches the boundary, which pops the
//!   frame and delivers to the caller. Winds and handlers installed inside
//!   the body have already popped themselves through their own cleanup
//!   continuations, so there is no depth sweep and nothing to get wrong.
//! - **Abort.** The delimited continuation is the aborting call's own
//!   continuation chain — it ends at the boundary — plus the winds, handlers
//!   and prompts *above* the frame. The abort then *jumps* to a landing whose
//!   stacks are cut back to the frame and whose `resume` calls the handler:
//!   the jump's travel runs every after-thunk between here and the prompt
//!   under its own record's handler stack, which is what the VM arrived at in
//!   #165 and what A1 in §5.6 measures.
//! - **Composable invoke.** Re-enter the captured extents outermost first —
//!   each `before` thunk a step under the *invoke site's* handler stack
//!   (`cps_features.rs` pins why not the record's) — then push a frame with
//!   the invoke site's continuation in it, append the captured prompts and
//!   handlers, and deliver the value into the chain. When the chain reaches
//!   its boundary it pops that frame and returns to the invoker. The frame
//!   carries no tag: a delimited continuation does not include its prompt,
//!   so an abort inside the resumed computation finds the invoke site's
//!   enclosing prompt or none (Racket, Guile and the VM agree).
//!
//! The one invariant all of this rests on is that **the prompt stack and the
//! continuation move together**: a full invoke restores its snapshot, an
//! abort cuts back to below its prompt, an invoke pushes, a boundary pops.
//! That is why `CpsContinuation` now carries `prompt_stack` — it always
//! should have (§5.6, row one) — and why the escape arrival in `mod.rs`
//! restores it instead of emptying it.
//!
//! # Where the VM's lessons land here
//!
//! Every one of the VM's prompt defects (#160–#167) was about one of five
//! pieces of dynamic state being carried by some transfers and not others.
//! The table in §5.6 lists them; on this backend the rows read:
//!
//! - a capture saves all three stacks, an arrival restores all three;
//! - an abort's landing carries each stack cut to the frame's recorded depth
//!   — recorded, because a `with-exception-handler` in tail position of the
//!   body and one whose thunk tail-called the prompt are the same depth by
//!   any other measure (§5.6's last paragraph);
//! - a composable continuation's carried prompts store depths relative to
//!   the region's base and the invoke adds the site's lengths back (#164);
//! - `raise` still touches nothing but the handler stack.
//!
//! # What is still shared with `call/cc`
//!
//! A nested trampoline — `apply_from_direct_tagged`, which Rust primitives
//! call back through — starts every stack empty, so an abort from inside such
//! a callback to a prompt outside it reports no matching prompt. Winds and
//! handlers have had that gap since before this module (the "primitive's
//! callback" entry in the triage doc); prompts inherit it rather than add to
//! it.

use super::CpsEvaluator;
use super::continuation::continuation_cont_value;
use super::types::{ContEnv, ContValue, ExceptionHandler, PromptFrame, StepResult};
use crate::eval::error::EvalError;
use patina_core::cps_expr::PromptTag;
use patina_core::tagged_value::TaggedValue;
use patina_core::{CpsContinuation, DynamicWindRecord, ExceptionKind, next_prompt_id};
use std::rc::Rc;

impl<'a> CpsEvaluator<'a> {
    /// `(call-with-continuation-prompt body [tag [handler]])`
    ///
    /// Push a frame and run `body` with the boundary as its continuation.
    /// The same defaults as the VM: a fresh tag when none is given — nothing
    /// can abort to it, which is the point of a fresh one — and `#f` for a
    /// missing handler, which an abort then reports as not a procedure.
    pub(super) fn apply_call_with_prompt(
        &self,
        args: Vec<TaggedValue>,
        cont: ContValue,
        cont_env: ContEnv,
        mut prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        if args.is_empty() || args.len() > 3 {
            return self.maybe_route_error_through_cps(
                EvalError::WrongArity {
                    expected: "1 to 3".to_string(),
                    actual: args.len(),
                },
                cont,
                cont_env,
                prompt_stack,
                dynamic_winds,
                exception_handlers,
            );
        }
        let body = args[0];
        let handler = args.get(2).copied().unwrap_or(TaggedValue::FALSE);

        // Decide inside the borrow, route outside it:
        // `maybe_route_error_through_cps` allocates the exception object, so
        // it takes `borrow_mut()` and would panic against a live `borrow()`.
        let heap = self.evaluator.global_env.heap();
        let checked: Result<Rc<PromptTag>, &'static str> = {
            let heap_ref = heap.borrow();
            if !heap_ref.is_callable(body) {
                Err("call-with-continuation-prompt: first argument must be a procedure")
            } else {
                match args.get(1) {
                    Some(&tag) => heap_ref.get_prompt_tag(tag).cloned().ok_or(
                        "call-with-continuation-prompt: second argument must be a prompt tag",
                    ),
                    None => Ok(Rc::new(PromptTag::new("default"))),
                }
            }
        };
        let tag = match checked {
            Ok(tag) => tag,
            Err(message) => {
                return self.maybe_route_error_through_cps(
                    EvalError::TypeError(message.to_string()),
                    cont,
                    cont_env,
                    prompt_stack,
                    dynamic_winds,
                    exception_handlers,
                );
            }
        };

        let id = next_prompt_id();
        prompt_stack.push(PromptFrame {
            id,
            tag: Some(tag),
            handler,
            cont,
            wind_depth: dynamic_winds.len(),
            handler_depth: exception_handlers.len(),
        });
        Ok(StepResult::ApplyProc {
            proc: body,
            args: vec![],
            cont: ContValue::PromptBoundary { id },
            env: self.evaluator.global_env.clone(),
            cont_env,
            prompt_stack,
            dynamic_winds,
            exception_handlers,
        })
    }

    /// `(abort-current-continuation tag [value])`
    ///
    /// Capture the delimited continuation, then jump to the prompt: the
    /// travel leaves every extent between here and it, and the arrival calls
    /// the handler with `(value k)` under the prompt's own dynamic
    /// environment. Never returns normally to this step's caller — the
    /// result is the travel's first thunk step, or the parked escape.
    pub(super) fn apply_abort_current_continuation(
        &self,
        args: Vec<TaggedValue>,
        cont: ContValue,
        cont_env: ContEnv,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        if args.is_empty() {
            return self.maybe_route_error_through_cps(
                EvalError::WrongArity {
                    expected: "at least 1".to_string(),
                    actual: 0,
                },
                cont,
                cont_env,
                prompt_stack,
                dynamic_winds,
                exception_handlers,
            );
        }
        let heap = self.evaluator.global_env.heap();
        let tag = heap.borrow().get_prompt_tag(args[0]).cloned();
        let Some(tag) = tag else {
            return self.maybe_route_error_through_cps(
                EvalError::TypeError(
                    "abort-current-continuation: first argument must be a prompt tag".to_string(),
                ),
                cont,
                cont_env,
                prompt_stack,
                dynamic_winds,
                exception_handlers,
            );
        };
        let value = args.get(1).copied().unwrap_or(TaggedValue::UNSPECIFIED);

        // The innermost prompt carrying `tag`. An invoke's frame carries none
        // and so is passed over, as it should be.
        let matching = |frame: &PromptFrame| frame.tag.as_ref().is_some_and(|t| t.id == tag.id);
        let Some(idx) = prompt_stack.iter().rposition(matching) else {
            return self.maybe_route_error_through_cps(
                EvalError::SchemeException {
                    kind: ExceptionKind::Error,
                    message: "abort-current-continuation: no matching prompt tag for abort"
                        .to_string(),
                    irritants_display: String::new(),
                },
                cont,
                cont_env,
                prompt_stack,
                dynamic_winds,
                exception_handlers,
            );
        };
        let frame = prompt_stack[idx].clone();

        // Clamp rather than slice, as the VM does: a `raise` pops the handler
        // entry it is running *before* calling it, so a handler that aborts to
        // a prompt established under itself arrives with a shorter stack than
        // the prompt recorded.
        let wind_depth = frame.wind_depth.min(dynamic_winds.len());
        let handler_depth = frame.handler_depth.min(exception_handlers.len());

        // The delimited continuation: this call's own continuation, which
        // ends at the frame's boundary, with the dynamic state above the
        // frame. The carried prompts' depths become relative to that base, so
        // that an invoke can put them on stacks of any length.
        let inner_prompts: Vec<PromptFrame> = prompt_stack[idx + 1..]
            .iter()
            .map(|inner| PromptFrame {
                wind_depth: inner.wind_depth.saturating_sub(wind_depth),
                handler_depth: inner.handler_depth.saturating_sub(handler_depth),
                ..inner.clone()
            })
            .collect();
        let delimited = self.capture(
            &cont,
            &cont_env,
            Some(frame.id),
            &dynamic_winds[wind_depth..],
            &exception_handlers[handler_depth..],
            &inner_prompts,
        );
        let delimited = heap.borrow_mut().alloc_continuation(delimited);

        // The landing: the machine as it will be once the abort has arrived —
        // every stack cut back to the prompt — with the handler call as what
        // it resumes into. Travelling there is what runs the after-thunks of
        // every extent being left, each in its own `dynamic-wind` call's
        // environment (`wind.rs`); `prompt_stack` goes along uncut so that a
        // thunk on the way can itself abort.
        let landing = self.capture(
            &ContValue::AbortLanding {
                handler: frame.handler,
                delimited,
                cont: Box::new(frame.cont),
            },
            &cont_env,
            None,
            &dynamic_winds[..wind_depth],
            &exception_handlers[..handler_depth],
            &prompt_stack[..idx],
        );
        self.jump_to_continuation(value, landing, cont_env, prompt_stack, dynamic_winds)
    }

    /// `(k value)` for a composable continuation `k`.
    ///
    /// Not a jump. The captured computation is appended to the invoke site's
    /// — its extents re-entered, its prompts and handlers stacked on top of
    /// the site's — and returns to `cont` when it reaches its boundary. Every
    /// re-entered extent has a `before` thunk to run first, one step each,
    /// through [`ContValue::ComposableInvokeStep`].
    #[allow(clippy::too_many_arguments)]
    pub(super) fn invoke_composable(
        &self,
        target: Rc<CpsContinuation>,
        value: TaggedValue,
        cont: ContValue,
        cont_env: ContEnv,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        self.resume_composable(
            target,
            value,
            0,
            cont,
            cont_env,
            prompt_stack,
            dynamic_winds,
            exception_handlers,
        )
    }

    /// Continue a composable invoke from the captured extent at `index`: run
    /// its `before` thunk if there is one, else every extent is entered and
    /// the chain itself is resumed with `value`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn resume_composable(
        &self,
        target: Rc<CpsContinuation>,
        value: TaggedValue,
        index: usize,
        cont: ContValue,
        cont_env: ContEnv,
        mut prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        mut exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        if let Some(record) = target.dynamic_winds.get(index) {
            // Under the invoke site's handler stack — `exception_handlers` as
            // it stands — and not the record's own, which is the difference
            // between this and a jump's `push_wind_step`: a jump's target
            // replaces the machine, so the record's stack is the only
            // candidate; this one extends it, so the site's stack is a prefix
            // of the right answer, and installing the record's would lose the
            // site's handlers and resurrect capture-site ones whose extent is
            // over.
            let before = record.before;
            return Ok(StepResult::ApplyProc {
                proc: before,
                args: vec![],
                cont: ContValue::ComposableInvokeStep {
                    target,
                    value,
                    index,
                    cont: Box::new(cont),
                },
                env: self.evaluator.global_env.clone(),
                cont_env,
                prompt_stack,
                dynamic_winds,
                exception_handlers,
            });
        }

        // Every extent is entered: the captured records now sit on top of the
        // invoke site's, and that base is what the carried prompts' relative
        // depths are added to.
        let boundary = target
            .boundary
            .expect("resume_composable is only reached for a composable continuation");
        let wind_base = dynamic_winds
            .len()
            .saturating_sub(target.dynamic_winds.len());
        let handler_base = exception_handlers.len();
        prompt_stack.push(PromptFrame {
            id: boundary,
            tag: None,
            handler: TaggedValue::FALSE,
            cont,
            wind_depth: wind_base,
            handler_depth: handler_base,
        });
        prompt_stack.extend(target.prompt_stack.iter().map(|inner| PromptFrame {
            wind_depth: inner.wind_depth + wind_base,
            handler_depth: inner.handler_depth + handler_base,
            ..inner.clone()
        }));
        exception_handlers.extend(target.exception_handlers.iter().cloned());

        Ok(StepResult::InvokeContinuation {
            cont: continuation_cont_value(&target),
            value,
            env: target.env.clone(),
            cont_env: target.captured_cont_env.clone(),
            prompt_stack,
            dynamic_winds,
            exception_handlers,
        })
    }
}
