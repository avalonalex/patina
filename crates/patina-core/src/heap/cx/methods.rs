//! The `Heap` methods the safe crates call, on a context: generated from
//! their signatures once (#703), with `Value<'gc>` for every `TaggedValue`,
//! and kept by hand since. A read borrows the heap for its call, an
//! allocation or a store borrows it mutably; a word the heap answers is
//! branded as it leaves ([`Cx::brand`]).
//!
//! The methods whose `Heap` form handed out a reference into the heap, or an
//! `Rc` payload a store could bypass the funnel through, are in the
//! [parent module](super), reshaped.

use std::rc::Rc;
use std::time::Duration;

use num_bigint::BigInt;
use num_rational::BigRational;

use super::Cx;
use crate::error::SourceLocation;
use crate::heap::account::GcThreshold;
use crate::heap::handles::{Owned, OwnedEnvironment};
use crate::heap::telemetry::HighWater;
use crate::heap::{HeapStats, SpineEnd};
use crate::numeric::NumericError;
use crate::value::Value;

impl<'gc> Cx<'gc> {
    /// [`Heap::alloc_bigint`](crate::Heap::alloc_bigint) on this context's heap.
    pub fn alloc_bigint(&mut self, n: BigInt) -> Value<'gc> {
        let r = self.heap.borrow_mut().alloc_bigint(n);
        self.brand(r)
    }

    /// [`Heap::alloc_bytevector`](crate::Heap::alloc_bytevector) on this context's heap.
    pub fn alloc_bytevector(&mut self, bytes: Vec<u8>) -> Value<'gc> {
        let r = self.heap.borrow_mut().alloc_bytevector(bytes);
        self.brand(r)
    }

    /// [`Heap::alloc_complex`](crate::Heap::alloc_complex) on this context's heap.
    pub fn alloc_complex(&mut self, real: Value<'gc>, imag: Value<'gc>) -> Value<'gc> {
        let r = self
            .heap
            .borrow_mut()
            .alloc_complex(real.tagged(), imag.tagged());
        self.brand(r)
    }

    /// [`Heap::alloc_environment_specifier`](crate::Heap::alloc_environment_specifier) on this context's heap.
    pub fn alloc_environment_specifier(
        &mut self,
        env: Rc<crate::environment::Environment>,
        mutable: bool,
    ) -> Value<'gc> {
        let r = self
            .heap
            .borrow_mut()
            .alloc_environment_specifier(env, mutable);
        self.brand(r)
    }

    /// [`Heap::alloc_ephemeron`](crate::Heap::alloc_ephemeron) on this context's heap.
    pub fn alloc_ephemeron(&mut self, key: Value<'gc>, datum: Value<'gc>) -> Value<'gc> {
        let r = self
            .heap
            .borrow_mut()
            .alloc_ephemeron(key.tagged(), datum.tagged());
        self.brand(r)
    }

    /// [`Heap::alloc_identifier`](crate::Heap::alloc_identifier) on this context's heap.
    pub fn alloc_identifier(
        &mut self,
        name: Rc<str>,
        scopes: crate::scope::ScopeSet,
    ) -> Value<'gc> {
        let r = self.heap.borrow_mut().alloc_identifier(name, scopes);
        self.brand(r)
    }

    /// [`Heap::alloc_label_placeholder`](crate::Heap::alloc_label_placeholder) on this context's heap.
    pub fn alloc_label_placeholder(&mut self, label: usize) -> Value<'gc> {
        let r = self.heap.borrow_mut().alloc_label_placeholder(label);
        self.brand(r)
    }

    /// [`Heap::alloc_macro`](crate::Heap::alloc_macro) on this context's heap.
    pub fn alloc_macro(&mut self, mac: Rc<crate::compiled_macro::CompiledMacro>) -> Value<'gc> {
        let r = self.heap.borrow_mut().alloc_macro(mac);
        self.brand(r)
    }

    /// [`Heap::alloc_pair`](crate::Heap::alloc_pair) on this context's heap.
    pub fn alloc_pair(&mut self, car: Value<'gc>, cdr: Value<'gc>) -> Value<'gc> {
        let r = self
            .heap
            .borrow_mut()
            .alloc_pair(car.tagged(), cdr.tagged());
        self.brand(r)
    }

    /// [`Heap::alloc_port`](crate::Heap::alloc_port) on this context's heap.
    pub fn alloc_port(&mut self, port: Rc<crate::port::Port>) -> Value<'gc> {
        let r = self.heap.borrow_mut().alloc_port(port);
        self.brand(r)
    }

    /// [`Heap::alloc_prompt_tag`](crate::Heap::alloc_prompt_tag) on this context's heap.
    pub fn alloc_prompt_tag(&mut self, tag: Rc<crate::cps_expr::PromptTag>) -> Value<'gc> {
        let r = self.heap.borrow_mut().alloc_prompt_tag(tag);
        self.brand(r)
    }

    /// [`Heap::alloc_rational`](crate::Heap::alloc_rational) on this context's heap.
    pub fn alloc_rational(&mut self, r: BigRational) -> Value<'gc> {
        let r = self.heap.borrow_mut().alloc_rational(r);
        self.brand(r)
    }

    /// [`Heap::alloc_real`](crate::Heap::alloc_real) on this context's heap.
    pub fn alloc_real(&mut self, f: f64) -> Value<'gc> {
        let r = self.heap.borrow_mut().alloc_real(f);
        self.brand(r)
    }

    /// [`Heap::alloc_record_type`](crate::Heap::alloc_record_type) on this context's heap.
    pub fn alloc_record_type(
        &mut self,
        rtd: Rc<crate::record_type::RecordTypeDescriptor>,
    ) -> Value<'gc> {
        let r = self.heap.borrow_mut().alloc_record_type(rtd);
        self.brand(r)
    }

    /// [`Heap::alloc_source_identifier`](crate::Heap::alloc_source_identifier) on this context's heap.
    pub fn alloc_source_identifier(&mut self, name: Rc<str>) -> Value<'gc> {
        let r = self.heap.borrow_mut().alloc_source_identifier(name);
        self.brand(r)
    }

    /// [`Heap::alloc_string`](crate::Heap::alloc_string) on this context's heap.
    pub fn alloc_string(&mut self, s: String) -> Value<'gc> {
        let r = self.heap.borrow_mut().alloc_string(s);
        self.brand(r)
    }

    /// [`Heap::alloc_string_chars`](crate::Heap::alloc_string_chars) on this context's heap.
    pub fn alloc_string_chars(&mut self, chars: Vec<char>) -> Value<'gc> {
        let r = self.heap.borrow_mut().alloc_string_chars(chars);
        self.brand(r)
    }

    /// [`Heap::alloc_values`](crate::Heap::alloc_values) on this context's heap.
    pub fn alloc_values(&mut self, values: Vec<Value<'gc>>) -> Value<'gc> {
        let r = self
            .heap
            .borrow_mut()
            .alloc_values(values.into_iter().map(Value::tagged).collect());
        self.brand(r)
    }

    /// [`Heap::alloc_vector`](crate::Heap::alloc_vector) on this context's heap.
    pub fn alloc_vector(&mut self, elements: Vec<Value<'gc>>) -> Value<'gc> {
        let r = self
            .heap
            .borrow_mut()
            .alloc_vector(elements.into_iter().map(Value::tagged).collect());
        self.brand(r)
    }

    /// [`Heap::alloc_vector_fill`](crate::Heap::alloc_vector_fill) on this context's heap.
    pub fn alloc_vector_fill(&mut self, len: usize, fill: Value<'gc>) -> Value<'gc> {
        let r = self.heap.borrow_mut().alloc_vector_fill(len, fill.tagged());
        self.brand(r)
    }

    /// [`Heap::angle`](crate::Heap::angle) on this context's heap.
    pub fn angle(&mut self, a: Value<'gc>) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().angle(a.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::bytes_since_gc`](crate::Heap::bytes_since_gc) on this context's heap.
    pub fn bytes_since_gc(&self) -> usize {
        self.heap.borrow().bytes_since_gc()
    }

    /// [`Heap::bytevector_copy_into`](crate::Heap::bytevector_copy_into) on this context's heap.
    pub fn bytevector_copy_into(&mut self, tv: Value<'gc>, at: usize, src: &[u8]) -> bool {
        self.heap
            .borrow_mut()
            .bytevector_copy_into(tv.tagged(), at, src)
    }

    /// [`Heap::bytevector_len`](crate::Heap::bytevector_len) on this context's heap.
    pub fn bytevector_len(&self, tv: Value<'gc>) -> Option<usize> {
        self.heap.borrow().bytevector_len(tv.tagged())
    }

    /// [`Heap::bytevector_u8_ref`](crate::Heap::bytevector_u8_ref) on this context's heap.
    pub fn bytevector_u8_ref(&self, tv: Value<'gc>, index: usize) -> Option<u8> {
        self.heap.borrow().bytevector_u8_ref(tv.tagged(), index)
    }

    /// [`Heap::bytevector_u8_set`](crate::Heap::bytevector_u8_set) on this context's heap.
    pub fn bytevector_u8_set(&mut self, tv: Value<'gc>, index: usize, byte: u8) -> bool {
        self.heap
            .borrow_mut()
            .bytevector_u8_set(tv.tagged(), index, byte)
    }

    /// [`Heap::car`](crate::Heap::car) on this context's heap.
    pub fn car(&self, ptr: Value<'gc>) -> Value<'gc> {
        let r = self.heap.borrow().car(ptr.tagged());
        self.brand(r)
    }

    /// [`Heap::cdr`](crate::Heap::cdr) on this context's heap.
    pub fn cdr(&self, ptr: Value<'gc>) -> Value<'gc> {
        let r = self.heap.borrow().cdr(ptr.tagged());
        self.brand(r)
    }

    /// [`Heap::core_syntax`](crate::Heap::core_syntax) on this context's heap.
    pub fn core_syntax(&mut self, form: crate::core_syntax::CoreForm) -> Value<'gc> {
        let r = self.heap.borrow_mut().core_syntax(form);
        self.brand(r)
    }

    /// [`Heap::defer_collection`](crate::Heap::defer_collection) on this context's heap.
    pub fn defer_collection(&mut self) {
        self.heap.borrow_mut().defer_collection()
    }

    /// [`Heap::deferral_high_water`](crate::Heap::deferral_high_water) on this context's heap.
    pub fn deferral_high_water(&self) -> HighWater {
        self.heap.borrow().deferral_high_water()
    }

    /// [`Heap::denominator`](crate::Heap::denominator) on this context's heap.
    pub fn denominator(&mut self, a: Value<'gc>) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().denominator(a.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::ephemeron_state`](crate::Heap::ephemeron_state) on this context's heap.
    pub fn ephemeron_state(&self, tv: Value<'gc>) -> Option<Option<(Value<'gc>, Value<'gc>)>> {
        let r = self.heap.borrow().ephemeron_state(tv.tagged());
        r.map(|o| o.map(|(a, b)| (self.brand(a), self.brand(b))))
    }

    /// [`Heap::gc_threshold`](crate::Heap::gc_threshold) on this context's heap.
    pub fn gc_threshold(&self) -> GcThreshold {
        self.heap.borrow().gc_threshold()
    }

    /// [`Heap::gcd_many`](crate::Heap::gcd_many) on this context's heap.
    pub fn gcd_many(&mut self, args: &[Value<'gc>]) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().gcd_many(Value::tagged_slice(args));
        r.map(|w| self.brand(w))
    }

    /// [`Heap::get_bytevector_bytes`](crate::Heap::get_bytevector_bytes) on this context's heap.
    pub fn get_bytevector_bytes(&self, tv: Value<'gc>) -> Option<Vec<u8>> {
        self.heap.borrow().get_bytevector_bytes(tv.tagged())
    }

    /// [`Heap::get_complex`](crate::Heap::get_complex) on this context's heap.
    pub fn get_complex(&self, tv: Value<'gc>) -> Option<(Value<'gc>, Value<'gc>)> {
        let r = self.heap.borrow().get_complex(tv.tagged());
        r.map(|(a, b)| (self.brand(a), self.brand(b)))
    }

    /// [`Heap::get_core_syntax`](crate::Heap::get_core_syntax) on this context's heap.
    pub fn get_core_syntax(&self, tv: Value<'gc>) -> Option<crate::core_syntax::CoreForm> {
        self.heap.borrow().get_core_syntax(tv.tagged())
    }

    /// [`Heap::get_identifier_data_any`](crate::Heap::get_identifier_data_any) on this context's heap.
    pub fn get_identifier_data_any(
        &self,
        tv: Value<'gc>,
    ) -> Option<(Rc<str>, crate::scope::ScopeSet)> {
        self.heap.borrow().get_identifier_data_any(tv.tagged())
    }

    /// [`Heap::get_pair`](crate::Heap::get_pair) on this context's heap.
    pub fn get_pair(&self, ptr: Value<'gc>) -> (Value<'gc>, Value<'gc>) {
        let r = self.heap.borrow().get_pair(ptr.tagged());
        (self.brand(r.0), self.brand(r.1))
    }

    /// [`Heap::get_procedure`](crate::Heap::get_procedure) on this context's heap.
    pub fn get_procedure(
        &self,
        tv: Value<'gc>,
    ) -> Option<std::rc::Rc<crate::procedure::Procedure>> {
        self.heap.borrow().get_procedure(tv.tagged())
    }

    /// [`Heap::get_real`](crate::Heap::get_real) on this context's heap.
    pub fn get_real(&self, tv: Value<'gc>) -> Option<f64> {
        self.heap.borrow().get_real(tv.tagged())
    }

    /// [`Heap::get_string_contents`](crate::Heap::get_string_contents) on this context's heap.
    pub fn get_string_contents(&self, tv: Value<'gc>) -> Option<String> {
        self.heap.borrow().get_string_contents(tv.tagged())
    }

    /// [`Heap::held`](crate::Heap::held) on this context's heap.
    pub fn held(&self, handle: &Owned) -> Value<'gc> {
        let r = self.heap.borrow().held(handle);
        self.brand(r)
    }

    /// [`Heap::held_environment`](crate::Heap::held_environment) on this context's heap.
    pub fn held_environment(
        &self,
        handle: &OwnedEnvironment,
    ) -> Rc<crate::environment::Environment> {
        self.heap.borrow().held_environment(handle)
    }

    /// [`Heap::hold`](crate::Heap::hold) on this context's heap.
    pub fn hold(&self, value: Value<'gc>) -> Owned {
        self.heap.borrow().hold(value.tagged())
    }

    /// [`Heap::hold_environment`](crate::Heap::hold_environment) on this context's heap.
    pub fn hold_environment(&self, env: Rc<crate::environment::Environment>) -> OwnedEnvironment {
        self.heap.borrow().hold_environment(env)
    }

    /// [`Heap::imag_part`](crate::Heap::imag_part) on this context's heap.
    pub fn imag_part(&mut self, a: Value<'gc>) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().imag_part(a.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::inherit_source`](crate::Heap::inherit_source) on this context's heap.
    pub fn inherit_source(&mut self, original: Value<'gc>, copy: Value<'gc>) {
        self.heap
            .borrow_mut()
            .inherit_source(original.tagged(), copy.tagged())
    }

    /// [`Heap::intern_symbol`](crate::Heap::intern_symbol) on this context's heap.
    pub fn intern_symbol(&mut self, name: &str) -> Value<'gc> {
        let r = self.heap.borrow_mut().intern_symbol(name);
        self.brand(r)
    }

    /// [`Heap::is_any_bytevector`](crate::Heap::is_any_bytevector) on this context's heap.
    pub fn is_any_bytevector(&self, tv: Value<'gc>) -> bool {
        self.heap.borrow().is_any_bytevector(tv.tagged())
    }

    /// [`Heap::is_bigint`](crate::Heap::is_bigint) on this context's heap.
    pub fn is_bigint(&self, tv: Value<'gc>) -> bool {
        self.heap.borrow().is_bigint(tv.tagged())
    }

    /// [`Heap::is_bytevector`](crate::Heap::is_bytevector) on this context's heap.
    pub fn is_bytevector(&self, tv: Value<'gc>) -> bool {
        self.heap.borrow().is_bytevector(tv.tagged())
    }

    /// [`Heap::is_callable`](crate::Heap::is_callable) on this context's heap.
    pub fn is_callable(&self, tv: Value<'gc>) -> bool {
        self.heap.borrow().is_callable(tv.tagged())
    }

    /// [`Heap::is_complex`](crate::Heap::is_complex) on this context's heap.
    pub fn is_complex(&self, tv: Value<'gc>) -> bool {
        self.heap.borrow().is_complex(tv.tagged())
    }

    /// [`Heap::is_continuation`](crate::Heap::is_continuation) on this context's heap.
    pub fn is_continuation(&self, tv: Value<'gc>) -> bool {
        self.heap.borrow().is_continuation(tv.tagged())
    }

    /// [`Heap::is_ephemeron`](crate::Heap::is_ephemeron) on this context's heap.
    pub fn is_ephemeron(&self, tv: Value<'gc>) -> bool {
        self.heap.borrow().is_ephemeron(tv.tagged())
    }

    /// [`Heap::is_exact_number`](crate::Heap::is_exact_number) on this context's heap.
    pub fn is_exact_number(&self, tv: Value<'gc>) -> bool {
        self.heap.borrow().is_exact_number(tv.tagged())
    }

    /// [`Heap::is_exact_zero`](crate::Heap::is_exact_zero) on this context's heap.
    pub fn is_exact_zero(&self, tv: Value<'gc>) -> bool {
        self.heap.borrow().is_exact_zero(tv.tagged())
    }

    /// [`Heap::is_exception`](crate::Heap::is_exception) on this context's heap.
    pub fn is_exception(&self, tv: Value<'gc>) -> bool {
        self.heap.borrow().is_exception(tv.tagged())
    }

    /// [`Heap::is_identifier`](crate::Heap::is_identifier) on this context's heap.
    pub fn is_identifier(&self, tv: Value<'gc>) -> bool {
        self.heap.borrow().is_identifier(tv.tagged())
    }

    /// [`Heap::is_inexact_number`](crate::Heap::is_inexact_number) on this context's heap.
    pub fn is_inexact_number(&self, tv: Value<'gc>) -> bool {
        self.heap.borrow().is_inexact_number(tv.tagged())
    }

    /// [`Heap::is_integer_r7rs`](crate::Heap::is_integer_r7rs) on this context's heap.
    pub fn is_integer_r7rs(&self, tv: Value<'gc>) -> bool {
        self.heap.borrow().is_integer_r7rs(tv.tagged())
    }

    /// [`Heap::is_list`](crate::Heap::is_list) on this context's heap.
    pub fn is_list(&self, tv: Value<'gc>) -> bool {
        self.heap.borrow().is_list(tv.tagged())
    }

    /// [`Heap::is_named`](crate::Heap::is_named) on this context's heap.
    pub fn is_named(&self, tv: Value<'gc>, name: &str) -> bool {
        self.heap.borrow().is_named(tv.tagged(), name)
    }

    /// [`Heap::is_number`](crate::Heap::is_number) on this context's heap.
    pub fn is_number(&self, tv: Value<'gc>) -> bool {
        self.heap.borrow().is_number(tv.tagged())
    }

    /// [`Heap::is_numeric_zero_tv`](crate::Heap::is_numeric_zero_tv) on this context's heap.
    pub fn is_numeric_zero_tv(&self, tv: Value<'gc>) -> bool {
        self.heap.borrow().is_numeric_zero_tv(tv.tagged())
    }

    /// [`Heap::is_procedure`](crate::Heap::is_procedure) on this context's heap.
    pub fn is_procedure(&self, tv: Value<'gc>) -> bool {
        self.heap.borrow().is_procedure(tv.tagged())
    }

    /// [`Heap::is_promise`](crate::Heap::is_promise) on this context's heap.
    pub fn is_promise(&self, tv: Value<'gc>) -> bool {
        self.heap.borrow().is_promise(tv.tagged())
    }

    /// [`Heap::is_prompt_tag`](crate::Heap::is_prompt_tag) on this context's heap.
    pub fn is_prompt_tag(&self, tv: Value<'gc>) -> bool {
        self.heap.borrow().is_prompt_tag(tv.tagged())
    }

    /// [`Heap::is_rational`](crate::Heap::is_rational) on this context's heap.
    pub fn is_rational(&self, tv: Value<'gc>) -> bool {
        self.heap.borrow().is_rational(tv.tagged())
    }

    /// [`Heap::is_rational_r7rs`](crate::Heap::is_rational_r7rs) on this context's heap.
    pub fn is_rational_r7rs(&self, tv: Value<'gc>) -> bool {
        self.heap.borrow().is_rational_r7rs(tv.tagged())
    }

    /// [`Heap::is_real`](crate::Heap::is_real) on this context's heap.
    pub fn is_real(&self, tv: Value<'gc>) -> bool {
        self.heap.borrow().is_real(tv.tagged())
    }

    /// [`Heap::is_real_r7rs`](crate::Heap::is_real_r7rs) on this context's heap.
    pub fn is_real_r7rs(&self, tv: Value<'gc>) -> bool {
        self.heap.borrow().is_real_r7rs(tv.tagged())
    }

    /// [`Heap::is_record`](crate::Heap::is_record) on this context's heap.
    pub fn is_record(&self, tv: Value<'gc>) -> bool {
        self.heap.borrow().is_record(tv.tagged())
    }

    /// [`Heap::is_record_type`](crate::Heap::is_record_type) on this context's heap.
    pub fn is_record_type(&self, tv: Value<'gc>) -> bool {
        self.heap.borrow().is_record_type(tv.tagged())
    }

    /// [`Heap::is_source_identifier`](crate::Heap::is_source_identifier) on this context's heap.
    pub fn is_source_identifier(&self, value: Value<'gc>) -> bool {
        self.heap.borrow().is_source_identifier(value.tagged())
    }

    /// [`Heap::is_symbol`](crate::Heap::is_symbol) on this context's heap.
    pub fn is_symbol(&self, tv: Value<'gc>) -> bool {
        self.heap.borrow().is_symbol(tv.tagged())
    }

    /// [`Heap::last_pause`](crate::Heap::last_pause) on this context's heap.
    pub fn last_pause(&self) -> Duration {
        self.heap.borrow().last_pause()
    }

    /// [`Heap::lcm_many`](crate::Heap::lcm_many) on this context's heap.
    pub fn lcm_many(&mut self, args: &[Value<'gc>]) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().lcm_many(Value::tagged_slice(args));
        r.map(|w| self.brand(w))
    }

    /// [`Heap::list_append`](crate::Heap::list_append) on this context's heap.
    pub fn list_append(&mut self, first: Value<'gc>, second: Value<'gc>) -> Option<Value<'gc>> {
        let r = self
            .heap
            .borrow_mut()
            .list_append(first.tagged(), second.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::list_len`](crate::Heap::list_len) on this context's heap.
    pub fn list_len(&self, tv: Value<'gc>) -> Option<usize> {
        self.heap.borrow().list_len(tv.tagged())
    }

    /// [`Heap::list_ref`](crate::Heap::list_ref) on this context's heap.
    pub fn list_ref(&self, tv: Value<'gc>, index: usize) -> Option<Value<'gc>> {
        let r = self.heap.borrow().list_ref(tv.tagged(), index);
        r.map(|w| self.brand(w))
    }

    /// [`Heap::list_reverse`](crate::Heap::list_reverse) on this context's heap.
    pub fn list_reverse(&mut self, tv: Value<'gc>) -> Option<Value<'gc>> {
        let r = self.heap.borrow_mut().list_reverse(tv.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::list_tail`](crate::Heap::list_tail) on this context's heap.
    pub fn list_tail(&self, tv: Value<'gc>, n: usize) -> Option<Value<'gc>> {
        let r = self.heap.borrow().list_tail(tv.tagged(), n);
        r.map(|w| self.brand(w))
    }

    /// [`Heap::list_to_vec`](crate::Heap::list_to_vec) on this context's heap.
    pub fn list_to_vec(&self, tv: Value<'gc>) -> Option<Vec<Value<'gc>>> {
        let r = self.heap.borrow().list_to_vec(tv.tagged());
        r.map(|v| self.brand_vec(v))
    }

    /// [`Heap::magnitude`](crate::Heap::magnitude) on this context's heap.
    pub fn magnitude(&mut self, a: Value<'gc>) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().magnitude(a.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::make_polar`](crate::Heap::make_polar) on this context's heap.
    pub fn make_polar(
        &mut self,
        magnitude: Value<'gc>,
        angle: Value<'gc>,
    ) -> Result<Value<'gc>, NumericError> {
        let r = self
            .heap
            .borrow_mut()
            .make_polar(magnitude.tagged(), angle.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::make_rectangular`](crate::Heap::make_rectangular) on this context's heap.
    pub fn make_rectangular(
        &mut self,
        real: Value<'gc>,
        imag: Value<'gc>,
    ) -> Result<Value<'gc>, NumericError> {
        let r = self
            .heap
            .borrow_mut()
            .make_rectangular(real.tagged(), imag.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::mmu`](crate::Heap::mmu) on this context's heap.
    pub fn mmu(&self, ms: u64) -> Option<f64> {
        self.heap.borrow().mmu(ms)
    }

    /// [`Heap::numerator`](crate::Heap::numerator) on this context's heap.
    pub fn numerator(&mut self, a: Value<'gc>) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().numerator(a.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_abs`](crate::Heap::numeric_abs) on this context's heap.
    pub fn numeric_abs(&mut self, a: Value<'gc>) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().numeric_abs(a.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_acos`](crate::Heap::numeric_acos) on this context's heap.
    pub fn numeric_acos(&mut self, a: Value<'gc>) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().numeric_acos(a.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_add`](crate::Heap::numeric_add) on this context's heap.
    pub fn numeric_add(
        &mut self,
        a: Value<'gc>,
        b: Value<'gc>,
    ) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().numeric_add(a.tagged(), b.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_asin`](crate::Heap::numeric_asin) on this context's heap.
    pub fn numeric_asin(&mut self, a: Value<'gc>) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().numeric_asin(a.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_atan`](crate::Heap::numeric_atan) on this context's heap.
    pub fn numeric_atan(&mut self, a: Value<'gc>) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().numeric_atan(a.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_atan2`](crate::Heap::numeric_atan2) on this context's heap.
    pub fn numeric_atan2(
        &mut self,
        y: Value<'gc>,
        x: Value<'gc>,
    ) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().numeric_atan2(y.tagged(), x.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_ceiling`](crate::Heap::numeric_ceiling) on this context's heap.
    pub fn numeric_ceiling(&mut self, a: Value<'gc>) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().numeric_ceiling(a.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_cos`](crate::Heap::numeric_cos) on this context's heap.
    pub fn numeric_cos(&mut self, a: Value<'gc>) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().numeric_cos(a.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_div`](crate::Heap::numeric_div) on this context's heap.
    pub fn numeric_div(
        &mut self,
        a: Value<'gc>,
        b: Value<'gc>,
    ) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().numeric_div(a.tagged(), b.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_eq_cmp`](crate::Heap::numeric_eq_cmp) on this context's heap.
    pub fn numeric_eq_cmp(&self, a: Value<'gc>, b: Value<'gc>) -> Result<bool, NumericError> {
        self.heap.borrow().numeric_eq_cmp(a.tagged(), b.tagged())
    }

    /// [`Heap::numeric_exp`](crate::Heap::numeric_exp) on this context's heap.
    pub fn numeric_exp(&mut self, a: Value<'gc>) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().numeric_exp(a.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_expt`](crate::Heap::numeric_expt) on this context's heap.
    pub fn numeric_expt(
        &mut self,
        base: Value<'gc>,
        power: Value<'gc>,
    ) -> Result<Value<'gc>, NumericError> {
        let r = self
            .heap
            .borrow_mut()
            .numeric_expt(base.tagged(), power.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_floor`](crate::Heap::numeric_floor) on this context's heap.
    pub fn numeric_floor(&mut self, a: Value<'gc>) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().numeric_floor(a.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_floor_quotient`](crate::Heap::numeric_floor_quotient) on this context's heap.
    pub fn numeric_floor_quotient(
        &mut self,
        a: Value<'gc>,
        b: Value<'gc>,
    ) -> Result<Value<'gc>, NumericError> {
        let r = self
            .heap
            .borrow_mut()
            .numeric_floor_quotient(a.tagged(), b.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_floor_remainder`](crate::Heap::numeric_floor_remainder) on this context's heap.
    pub fn numeric_floor_remainder(
        &mut self,
        a: Value<'gc>,
        b: Value<'gc>,
    ) -> Result<Value<'gc>, NumericError> {
        let r = self
            .heap
            .borrow_mut()
            .numeric_floor_remainder(a.tagged(), b.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_ge`](crate::Heap::numeric_ge) on this context's heap.
    pub fn numeric_ge(&self, a: Value<'gc>, b: Value<'gc>) -> Result<bool, NumericError> {
        self.heap.borrow().numeric_ge(a.tagged(), b.tagged())
    }

    /// [`Heap::numeric_gt`](crate::Heap::numeric_gt) on this context's heap.
    pub fn numeric_gt(&self, a: Value<'gc>, b: Value<'gc>) -> Result<bool, NumericError> {
        self.heap.borrow().numeric_gt(a.tagged(), b.tagged())
    }

    /// [`Heap::numeric_is_finite`](crate::Heap::numeric_is_finite) on this context's heap.
    pub fn numeric_is_finite(&self, tv: Value<'gc>) -> Option<bool> {
        self.heap.borrow().numeric_is_finite(tv.tagged())
    }

    /// [`Heap::numeric_is_infinite`](crate::Heap::numeric_is_infinite) on this context's heap.
    pub fn numeric_is_infinite(&self, tv: Value<'gc>) -> Option<bool> {
        self.heap.borrow().numeric_is_infinite(tv.tagged())
    }

    /// [`Heap::numeric_is_nan`](crate::Heap::numeric_is_nan) on this context's heap.
    pub fn numeric_is_nan(&self, tv: Value<'gc>) -> Option<bool> {
        self.heap.borrow().numeric_is_nan(tv.tagged())
    }

    /// [`Heap::numeric_le`](crate::Heap::numeric_le) on this context's heap.
    pub fn numeric_le(&self, a: Value<'gc>, b: Value<'gc>) -> Result<bool, NumericError> {
        self.heap.borrow().numeric_le(a.tagged(), b.tagged())
    }

    /// [`Heap::numeric_log`](crate::Heap::numeric_log) on this context's heap.
    pub fn numeric_log(&mut self, a: Value<'gc>) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().numeric_log(a.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_log_base`](crate::Heap::numeric_log_base) on this context's heap.
    pub fn numeric_log_base(
        &mut self,
        x: Value<'gc>,
        base: Value<'gc>,
    ) -> Result<Value<'gc>, NumericError> {
        let r = self
            .heap
            .borrow_mut()
            .numeric_log_base(x.tagged(), base.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_lt`](crate::Heap::numeric_lt) on this context's heap.
    pub fn numeric_lt(&self, a: Value<'gc>, b: Value<'gc>) -> Result<bool, NumericError> {
        self.heap.borrow().numeric_lt(a.tagged(), b.tagged())
    }

    /// [`Heap::numeric_max`](crate::Heap::numeric_max) on this context's heap.
    pub fn numeric_max(
        &mut self,
        a: Value<'gc>,
        b: Value<'gc>,
    ) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().numeric_max(a.tagged(), b.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_min`](crate::Heap::numeric_min) on this context's heap.
    pub fn numeric_min(
        &mut self,
        a: Value<'gc>,
        b: Value<'gc>,
    ) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().numeric_min(a.tagged(), b.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_modulo`](crate::Heap::numeric_modulo) on this context's heap.
    pub fn numeric_modulo(
        &mut self,
        a: Value<'gc>,
        b: Value<'gc>,
    ) -> Result<Value<'gc>, NumericError> {
        let r = self
            .heap
            .borrow_mut()
            .numeric_modulo(a.tagged(), b.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_mul`](crate::Heap::numeric_mul) on this context's heap.
    pub fn numeric_mul(
        &mut self,
        a: Value<'gc>,
        b: Value<'gc>,
    ) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().numeric_mul(a.tagged(), b.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_neg`](crate::Heap::numeric_neg) on this context's heap.
    pub fn numeric_neg(&mut self, a: Value<'gc>) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().numeric_neg(a.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_quotient`](crate::Heap::numeric_quotient) on this context's heap.
    pub fn numeric_quotient(
        &mut self,
        a: Value<'gc>,
        b: Value<'gc>,
    ) -> Result<Value<'gc>, NumericError> {
        let r = self
            .heap
            .borrow_mut()
            .numeric_quotient(a.tagged(), b.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_remainder`](crate::Heap::numeric_remainder) on this context's heap.
    pub fn numeric_remainder(
        &mut self,
        a: Value<'gc>,
        b: Value<'gc>,
    ) -> Result<Value<'gc>, NumericError> {
        let r = self
            .heap
            .borrow_mut()
            .numeric_remainder(a.tagged(), b.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_round`](crate::Heap::numeric_round) on this context's heap.
    pub fn numeric_round(&mut self, a: Value<'gc>) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().numeric_round(a.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_sin`](crate::Heap::numeric_sin) on this context's heap.
    pub fn numeric_sin(&mut self, a: Value<'gc>) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().numeric_sin(a.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_sqrt`](crate::Heap::numeric_sqrt) on this context's heap.
    pub fn numeric_sqrt(&mut self, a: Value<'gc>) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().numeric_sqrt(a.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_square`](crate::Heap::numeric_square) on this context's heap.
    pub fn numeric_square(&mut self, a: Value<'gc>) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().numeric_square(a.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_sub`](crate::Heap::numeric_sub) on this context's heap.
    pub fn numeric_sub(
        &mut self,
        a: Value<'gc>,
        b: Value<'gc>,
    ) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().numeric_sub(a.tagged(), b.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_tan`](crate::Heap::numeric_tan) on this context's heap.
    pub fn numeric_tan(&mut self, a: Value<'gc>) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().numeric_tan(a.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::numeric_to_f64`](crate::Heap::numeric_to_f64) on this context's heap.
    pub fn numeric_to_f64(&self, tv: Value<'gc>) -> Result<f64, NumericError> {
        self.heap.borrow().numeric_to_f64(tv.tagged())
    }

    /// [`Heap::numeric_truncate`](crate::Heap::numeric_truncate) on this context's heap.
    pub fn numeric_truncate(&mut self, a: Value<'gc>) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().numeric_truncate(a.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::open_file_ports`](crate::Heap::open_file_ports) on this context's heap.
    pub fn open_file_ports(&self) -> usize {
        self.heap.borrow().open_file_ports()
    }

    /// [`Heap::pause_max`](crate::Heap::pause_max) on this context's heap.
    pub fn pause_max(&self) -> Duration {
        self.heap.borrow().pause_max()
    }

    /// [`Heap::pause_total`](crate::Heap::pause_total) on this context's heap.
    pub fn pause_total(&self) -> Duration {
        self.heap.borrow().pause_total()
    }

    /// [`Heap::promise_update`](crate::Heap::promise_update) on this context's heap.
    pub fn promise_update(&mut self, outer: Value<'gc>, inner: Value<'gc>) {
        self.heap
            .borrow_mut()
            .promise_update(outer.tagged(), inner.tagged())
    }

    /// [`Heap::real_part`](crate::Heap::real_part) on this context's heap.
    pub fn real_part(&mut self, a: Value<'gc>) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().real_part(a.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::record_source`](crate::Heap::record_source) on this context's heap.
    pub fn record_source(&mut self, value: Value<'gc>, location: SourceLocation) {
        self.heap
            .borrow_mut()
            .record_source(value.tagged(), location)
    }

    /// [`Heap::set_held`](crate::Heap::set_held) on this context's heap.
    pub fn set_held(&self, handle: &Owned, value: Value<'gc>) {
        self.heap.borrow().set_held(handle, value.tagged())
    }

    /// [`Heap::spine`](crate::Heap::spine) on this context's heap.
    pub fn spine(&self, tv: Value<'gc>) -> (Vec<Value<'gc>>, SpineEnd) {
        let r = self.heap.borrow().spine(tv.tagged());
        (self.brand_vec(r.0), r.1)
    }

    /// [`Heap::spine_is_circular`](crate::Heap::spine_is_circular) on this context's heap.
    pub fn spine_is_circular(&self, tv: Value<'gc>) -> bool {
        self.heap.borrow().spine_is_circular(tv.tagged())
    }

    /// [`Heap::stats`](crate::Heap::stats) on this context's heap.
    pub fn stats(&self) -> HeapStats {
        self.heap.borrow().stats()
    }

    /// [`Heap::string_char_len`](crate::Heap::string_char_len) on this context's heap.
    pub fn string_char_len(&self, ptr: Value<'gc>) -> usize {
        self.heap.borrow().string_char_len(ptr.tagged())
    }

    /// [`Heap::string_set_char`](crate::Heap::string_set_char) on this context's heap.
    pub fn string_set_char(&mut self, ptr: Value<'gc>, index: usize, ch: char) {
        self.heap
            .borrow_mut()
            .string_set_char(ptr.tagged(), index, ch)
    }

    /// [`Heap::tagged_value_hash`](crate::Heap::tagged_value_hash) on this context's heap.
    pub fn tagged_value_hash(&self, tv: Value<'gc>) -> u64 {
        self.heap.borrow().tagged_value_hash(tv.tagged())
    }

    /// [`Heap::tagged_value_hash_identity`](crate::Heap::tagged_value_hash_identity) on this context's heap.
    pub fn tagged_value_hash_identity(&self, tv: Value<'gc>) -> u64 {
        self.heap.borrow().tagged_value_hash_identity(tv.tagged())
    }

    /// [`Heap::tagged_values_equal`](crate::Heap::tagged_values_equal) on this context's heap.
    pub fn tagged_values_equal(&self, a: Value<'gc>, b: Value<'gc>) -> bool {
        self.heap
            .borrow()
            .tagged_values_equal(a.tagged(), b.tagged())
    }

    /// [`Heap::to_exact`](crate::Heap::to_exact) on this context's heap.
    pub fn to_exact(&mut self, a: Value<'gc>) -> Result<Value<'gc>, NumericError> {
        let r = self.heap.borrow_mut().to_exact(a.tagged());
        r.map(|w| self.brand(w))
    }

    /// [`Heap::to_inexact`](crate::Heap::to_inexact) on this context's heap.
    pub fn to_inexact(&mut self, a: Value<'gc>) -> Value<'gc> {
        let r = self.heap.borrow_mut().to_inexact(a.tagged());
        self.brand(r)
    }

    /// [`Heap::track_environment`](crate::Heap::track_environment) on this context's heap.
    pub fn track_environment(&self, env: &Rc<crate::environment::Environment>) {
        self.heap.borrow().track_environment(env)
    }

    /// [`Heap::try_pair`](crate::Heap::try_pair) on this context's heap.
    pub fn try_pair(&self, tv: Value<'gc>) -> Option<(Value<'gc>, Value<'gc>)> {
        let r = self.heap.borrow().try_pair(tv.tagged());
        r.map(|(a, b)| (self.brand(a), self.brand(b)))
    }

    /// [`Heap::try_vector_len`](crate::Heap::try_vector_len) on this context's heap.
    pub fn try_vector_len(&self, tv: Value<'gc>) -> Option<usize> {
        self.heap.borrow().try_vector_len(tv.tagged())
    }

    /// [`Heap::type_name`](crate::Heap::type_name) on this context's heap.
    pub fn type_name(&self, tv: Value<'gc>) -> &'static str {
        self.heap.borrow().type_name(tv.tagged())
    }

    /// [`Heap::values_eq`](crate::Heap::values_eq) on this context's heap.
    pub fn values_eq(&self, a: Value<'gc>, b: Value<'gc>) -> bool {
        self.heap.borrow().values_eq(a.tagged(), b.tagged())
    }

    /// [`Heap::values_eqv`](crate::Heap::values_eqv) on this context's heap.
    pub fn values_eqv(&self, a: Value<'gc>, b: Value<'gc>) -> bool {
        self.heap.borrow().values_eqv(a.tagged(), b.tagged())
    }

    /// [`Heap::values_from`](crate::Heap::values_from) on this context's heap.
    pub fn values_from(&mut self, values: Vec<Value<'gc>>) -> Value<'gc> {
        let r = self
            .heap
            .borrow_mut()
            .values_from(values.into_iter().map(Value::tagged).collect());
        self.brand(r)
    }

    /// [`Heap::vector_len`](crate::Heap::vector_len) on this context's heap.
    pub fn vector_len(&self, ptr: Value<'gc>) -> usize {
        self.heap.borrow().vector_len(ptr.tagged())
    }

    /// [`Heap::vector_ref`](crate::Heap::vector_ref) on this context's heap.
    pub fn vector_ref(&self, ptr: Value<'gc>, index: usize) -> Value<'gc> {
        let r = self.heap.borrow().vector_ref(ptr.tagged(), index);
        self.brand(r)
    }

    /// [`Heap::wait_high_water`](crate::Heap::wait_high_water) on this context's heap.
    pub fn wait_high_water(&self) -> HighWater {
        self.heap.borrow().wait_high_water()
    }

    /// [`Heap::wait_time_max`](crate::Heap::wait_time_max) on this context's heap.
    pub fn wait_time_max(&self) -> Duration {
        self.heap.borrow().wait_time_max()
    }
}
