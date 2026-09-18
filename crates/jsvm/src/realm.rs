//! Creation of the intrinsic objects and the global object.

use crate::value::*;
use crate::vm::*;
use cw_script_host::ScriptHost;
use std::collections::VecDeque;

fn bare(proto: Option<&Obj>) -> Obj {
    Obj::new(ObjData::new(proto.cloned(), Kind::Ordinary))
}

fn noop(_vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    Ok(Value::Undefined)
}

impl<'h> Vm<'h> {
    pub fn new(
        host: &'h mut dyn ScriptHost,
        argv: Vec<String>,
        env: Vec<(String, String)>,
        stdin: Option<String>,
    ) -> Vm<'h> {
        let object_proto = bare(None);
        let function_proto = Obj::new(ObjData::new(
            Some(object_proto.clone()),
            Kind::Function(Box::new(FuncData {
                imp: FuncImpl::Native {
                    f: noop,
                    slots: vec![],
                },
                ctor: CtorKind::None,
                class_ctor: false,
                home: None,
                fields: None,
            })),
        ));
        let op = Some(&object_proto);
        let error_proto = bare(op);
        let mut error_protos = vec![error_proto.clone()];
        for _ in 1..8 {
            error_protos.push(bare(Some(&error_proto)));
        }
        let iterator_proto = bare(op);
        let async_iterator_proto = bare(op);
        let generator_proto = bare(Some(&iterator_proto));
        let async_generator_proto = bare(Some(&async_iterator_proto));
        let placeholder = bare(None);
        let intr = Intrinsics {
            object_proto: object_proto.clone(),
            function_proto: function_proto.clone(),
            array_proto: Obj::new(ObjData::new(
                Some(object_proto.clone()),
                Kind::Array(vec![]),
            )),
            string_proto: Obj::new(ObjData::new(
                Some(object_proto.clone()),
                Kind::String(JsStr::new("")),
            )),
            number_proto: Obj::new(ObjData::new(Some(object_proto.clone()), Kind::Number(0.0))),
            boolean_proto: Obj::new(ObjData::new(
                Some(object_proto.clone()),
                Kind::Boolean(false),
            )),
            symbol_proto: bare(op),
            bigint_proto: bare(op),
            error_protos,
            error_ctors: vec![],
            iterator_proto: iterator_proto.clone(),
            async_iterator_proto,
            array_iter_proto: bare(Some(&iterator_proto)),
            map_iter_proto: bare(Some(&iterator_proto)),
            set_iter_proto: bare(Some(&iterator_proto)),
            string_iter_proto: bare(Some(&iterator_proto)),
            regexp_str_iter_proto: bare(Some(&iterator_proto)),
            generator_proto,
            async_generator_proto,
            generator_function_proto: bare(Some(&function_proto)),
            async_generator_function_proto: bare(Some(&function_proto)),
            async_function_proto: bare(Some(&function_proto)),
            promise_proto: bare(op),
            promise_ctor: placeholder.clone(),
            regexp_proto: bare(op),
            date_proto: bare(op),
            map_proto: bare(op),
            set_proto: bare(op),
            weakmap_proto: bare(op),
            weakset_proto: bare(op),
            weakref_proto: bare(op),
            arraybuffer_proto: bare(op),
            typed_protos: vec![],
            array_iter_next: placeholder.clone(),
            array_values: placeholder.clone(),
            object_ctor: placeholder.clone(),
            array_ctor: placeholder.clone(),
            function_ctor: placeholder.clone(),
            buffer_proto: placeholder,
        };
        let syms = Syms {
            iterator: new_symbol(Some("Symbol.iterator")),
            async_iterator: new_symbol(Some("Symbol.asyncIterator")),
            has_instance: new_symbol(Some("Symbol.hasInstance")),
            to_primitive: new_symbol(Some("Symbol.toPrimitive")),
            to_string_tag: new_symbol(Some("Symbol.toStringTag")),
            species: new_symbol(Some("Symbol.species")),
            is_concat_spreadable: new_symbol(Some("Symbol.isConcatSpreadable")),
            unscopables: new_symbol(Some("Symbol.unscopables")),
            match_: new_symbol(Some("Symbol.match")),
            match_all: new_symbol(Some("Symbol.matchAll")),
            replace: new_symbol(Some("Symbol.replace")),
            search: new_symbol(Some("Symbol.search")),
            split: new_symbol(Some("Symbol.split")),
            inspect_custom: std::rc::Rc::new(Symbol {
                desc: Some(JsStr::new("nodejs.util.inspect.custom")),
                private: false,
                registered: true,
            }),
        };
        let global = bare(Some(&object_proto));
        let start_micros = host.now_micros();
        let seed = host.random_u64() | 1;
        let mut vm = Vm {
            host,
            frames: vec![],
            natives: vec![],
            intr,
            syms,
            global,
            stdout: String::new(),
            stderr: String::new(),
            stdin,
            stdin_consumed: false,
            stdin_pos: 0,
            interactive: false,
            stdin_eof: false,
            awaiting_input: false,
            debug: None,
            steps: 0,
            budget: STEP_BUDGET,
            native_depth: 0,
            throw_site: None,
            exit: Exit::Return,
            microtasks: VecDeque::new(),
            ticks: VecDeque::new(),
            timers: vec![],
            timer_seq: 0,
            timer_id: 1,
            elapsed_ms: 0.0,
            clock_steps: 0,
            loading_parent: None,
            start_micros,
            pending_rejections: vec![],
            modules: vec![],
            argv,
            env,
            exit_code: 0,
            sources: vec![],
            symbol_registry: vec![],
            tail: Tail::Main,
            main_file: String::new(),
            inspect_seen: vec![],
            process: None,
            console_indent: 0,
            console_counts: vec![],
            console_timers: vec![],
            exit_handlers: vec![],
            listeners: vec![],
            stdin_listeners: vec![],
            stdin_flowing: false,
            readline_ifaces: vec![],
            stack_limit: 10,
            rng_state: seed,
            is_esm_main: false,
            import_meta: None,
            trace_funcs: vec![],
            exit_code_set: false,
            esm_promises: vec![],
            timer_frame: 0,
            drain: Drain::default(),
            out_mark: 0,
            open_fds: vec![],
            completion: Value::Undefined,
            handles: vec![],
        };
        // The inspect symbol is registered under its key.
        let ic = vm.syms.inspect_custom.clone();
        vm.symbol_registry
            .push(("nodejs.util.inspect.custom".into(), ic));
        let g = vm.global.clone();
        g.set_hidden("globalThis", Value::Obj(g.clone()));
        crate::builtins::object::install(&mut vm);
        crate::builtins::function::install(&mut vm);
        crate::builtins::array::install(&mut vm);
        crate::builtins::string::install(&mut vm);
        crate::builtins::number::install(&mut vm);
        crate::builtins::symbol::install(&mut vm);
        crate::builtins::error::install(&mut vm);
        crate::builtins::iter::install(&mut vm);
        crate::builtins::mapset::install(&mut vm);
        crate::builtins::json::install(&mut vm);
        crate::builtins::math::install(&mut vm);
        crate::builtins::global::install(&mut vm);
        crate::builtins::reflect::install(&mut vm);
        crate::builtins::date::install(&mut vm);
        crate::builtins::typed::install(&mut vm);
        crate::regexp::install(&mut vm);
        crate::promise::install(&mut vm);
        crate::node::install(&mut vm);
        vm
    }

    /// A native constructor with its prototype object wired up.
    pub fn make_ctor(&mut self, name: &str, len: u32, f: NativeFn, proto: &Obj) -> Obj {
        let c = self.native_fn(name, len, f);
        if let Kind::Function(fd) = &mut c.borrow_mut().kind {
            fd.ctor = CtorKind::Base;
        }
        c.borrow_mut().props.insert(
            Key::str("prototype"),
            Prop::data(Value::Obj(proto.clone()), 0),
        );
        proto.set_hidden("constructor", Value::Obj(c.clone()));
        c
    }

    pub fn set_global(&self, name: &str, v: Value) {
        self.global.set_hidden(name, v);
    }

    pub fn constant(&self, target: &Obj, name: &str, v: Value) {
        target.set_prop(name, v, 0);
    }
}
