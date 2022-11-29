use std::{collections::HashMap, ops::Deref, sync::Arc, vec};

use indexmap::IndexMap;
use itertools::Itertools;
use uplc::{
    ast::{
        builder::{self, constr_index_exposer, CONSTR_FIELDS_EXPOSER, CONSTR_GET_FIELD},
        Constant, Name, Program, Term,
    },
    builtins::DefaultFunction,
    parser::interner::Interner,
    BigInt, PlutusData,
};

use crate::{
    ast::{ArgName, AssignmentKind, BinOp, DataType, Function, Pattern, Span, TypedArg},
    expr::TypedExpr,
    ir::IR,
    tipo::{self, Type, TypeInfo, ValueConstructor, ValueConstructorVariant},
    uplc::{DataTypeKey, FunctionAccessKey},
    IdGenerator,
};

#[derive(Clone)]
pub struct FuncComponents {
    ir: Vec<IR>,
    dependencies: Vec<FunctionAccessKey>,
    args: Vec<String>,
    recursive: bool,
}

pub struct CodeGenerator<'a> {
    defined_functions: HashMap<FunctionAccessKey, ()>,
    functions: &'a HashMap<FunctionAccessKey, &'a Function<Arc<tipo::Type>, TypedExpr>>,
    // type_aliases: &'a HashMap<(String, String), &'a TypeAlias<Arc<tipo::Type>>>,
    data_types: &'a HashMap<DataTypeKey, &'a DataType<Arc<tipo::Type>>>,
    // imports: &'a HashMap<(String, String), &'a Use<String>>,
    // constants: &'a HashMap<(String, String), &'a ModuleConstant<Arc<tipo::Type>, String>>,
    module_types: &'a HashMap<String, TypeInfo>,
    id_gen: IdGenerator,
    needs_field_access: bool,
}

impl<'a> CodeGenerator<'a> {
    pub fn new(
        functions: &'a HashMap<FunctionAccessKey, &'a Function<Arc<tipo::Type>, TypedExpr>>,
        // type_aliases: &'a HashMap<(String, String), &'a TypeAlias<Arc<tipo::Type>>>,
        data_types: &'a HashMap<DataTypeKey, &'a DataType<Arc<tipo::Type>>>,
        // imports: &'a HashMap<(String, String), &'a Use<String>>,
        // constants: &'a HashMap<(String, String), &'a ModuleConstant<Arc<tipo::Type>, String>>,
        module_types: &'a HashMap<String, TypeInfo>,
    ) -> Self {
        CodeGenerator {
            defined_functions: HashMap::new(),
            functions,
            // type_aliases,
            data_types,
            // imports,
            // constants,
            module_types,
            id_gen: IdGenerator::new(),
            needs_field_access: false,
        }
    }

    pub fn generate(&mut self, body: TypedExpr, arguments: Vec<TypedArg>) -> Program<Name> {
        let mut ir_stack = vec![];
        let scope = vec![self.id_gen.next()];

        self.build_ir(&body, &mut ir_stack, scope);

        println!("{ir_stack:#?}");

        self.define_ir(&mut ir_stack);

        println!("{ir_stack:#?}");

        let mut term = self.uplc_code_gen(&mut ir_stack);

        if self.needs_field_access {
            term = builder::constr_get_field(term);

            term = builder::constr_fields_exposer(term);
        }

        // Wrap the validator body if ifThenElse term unit error
        term = builder::final_wrapper(term);

        for arg in arguments.iter().rev() {
            term = Term::Lambda {
                parameter_name: uplc::ast::Name {
                    text: arg.arg_name.get_variable_name().unwrap_or("_").to_string(),
                    unique: 0.into(),
                },
                body: term.into(),
            }
        }

        let mut program = Program {
            version: (1, 0, 0),
            term,
        };

        let mut interner = Interner::new();

        println!("{}", program.to_pretty());

        interner.program(&mut program);

        program
    }

    pub(crate) fn build_ir(&mut self, body: &TypedExpr, ir_stack: &mut Vec<IR>, scope: Vec<u64>) {
        match dbg!(body) {
            TypedExpr::Int { value, .. } => ir_stack.push(IR::Int {
                scope,
                value: value.to_string(),
            }),
            TypedExpr::String { value, .. } => ir_stack.push(IR::String {
                scope,
                value: value.to_string(),
            }),
            TypedExpr::ByteArray { bytes, .. } => ir_stack.push(IR::ByteArray {
                scope,
                bytes: bytes.to_vec(),
            }),
            TypedExpr::Sequence { expressions, .. } => {
                for expr in expressions {
                    let mut scope = scope.clone();
                    scope.push(self.id_gen.next());
                    self.build_ir(expr, ir_stack, scope);
                }
            }
            TypedExpr::Pipeline { expressions, .. } => {
                for expr in expressions {
                    let mut scope = scope.clone();
                    scope.push(self.id_gen.next());
                    self.build_ir(expr, ir_stack, scope);
                }
            }
            TypedExpr::Var {
                constructor, name, ..
            } => {
                ir_stack.push(IR::Var {
                    scope,
                    constructor: constructor.clone(),
                    name: name.clone(),
                });
            }
            TypedExpr::Fn { .. } => todo!(),
            TypedExpr::List {
                elements,
                tail,
                tipo,
                ..
            } => {
                ir_stack.push(IR::List {
                    scope: scope.clone(),
                    count: if tail.is_some() {
                        elements.len() + 1
                    } else {
                        elements.len()
                    },
                    tipo: tipo.clone(),
                    tail: tail.is_some(),
                });

                for element in elements {
                    let mut scope = scope.clone();
                    scope.push(self.id_gen.next());
                    self.build_ir(element, ir_stack, scope.clone())
                }

                if let Some(tail) = tail {
                    let mut scope = scope;
                    scope.push(self.id_gen.next());
                    ir_stack.push(IR::Tail {
                        scope: scope.clone(),
                        count: 1,
                    });
                    self.build_ir(tail, ir_stack, scope);
                }
            }
            TypedExpr::Call { fun, args, .. } => {
                ir_stack.push(IR::Call {
                    scope: scope.clone(),
                    count: args.len() + 1,
                });
                let mut scope_fun = scope.clone();
                scope_fun.push(self.id_gen.next());
                self.build_ir(fun, ir_stack, scope_fun);

                for arg in args {
                    let mut scope = scope.clone();
                    scope.push(self.id_gen.next());
                    self.build_ir(&arg.value, ir_stack, scope);
                }
            }
            TypedExpr::BinOp {
                name, left, right, ..
            } => {
                ir_stack.push(IR::BinOp {
                    scope: scope.clone(),
                    name: *name,
                    count: 2,
                    tipo: left.tipo(),
                });
                let mut scope_left = scope.clone();
                scope_left.push(self.id_gen.next());

                let mut scope_right = scope;
                scope_right.push(self.id_gen.next());

                self.build_ir(left, ir_stack, scope_left);
                self.build_ir(right, ir_stack, scope_right);
            }
            TypedExpr::Assignment {
                value,
                pattern,
                kind,
                tipo,
                ..
            } => {
                let mut define_vec: Vec<IR> = vec![];
                let mut value_vec: Vec<IR> = vec![];
                let mut pattern_vec: Vec<IR> = vec![];

                let mut value_scope = scope.clone();
                value_scope.push(self.id_gen.next());

                self.build_ir(value, &mut value_vec, value_scope);

                self.assignment_ir(
                    pattern,
                    &mut pattern_vec,
                    &mut value_vec,
                    tipo,
                    *kind,
                    scope,
                );

                ir_stack.append(&mut define_vec);
                ir_stack.append(&mut pattern_vec);
            }
            TypedExpr::Trace { .. } => todo!(),
            TypedExpr::When {
                subjects, clauses, ..
            } => {
                let subject_name = format!("__subject_name_{}", self.id_gen.next());
                let constr_var = format!("__constr_name_{}", self.id_gen.next());

                // assuming one subject at the moment
                let subject = subjects[0].clone();
                let mut needs_constr_var = false;

                if let Some((last_clause, clauses)) = clauses.split_last() {
                    let mut clauses_vec = vec![];
                    let mut pattern_vec = vec![];

                    for clause in clauses {
                        let mut scope = scope.clone();
                        scope.push(self.id_gen.next());

                        pattern_vec.push(IR::Clause {
                            scope: scope.clone(),
                            count: 2,
                            tipo: subject.tipo().clone(),
                            subject_name: subject_name.clone(),
                        });

                        self.build_ir(&clause.then, &mut clauses_vec, scope.clone());
                        self.when_ir(
                            &clause.pattern[0],
                            &mut pattern_vec,
                            &mut clauses_vec,
                            &subject.tipo(),
                            constr_var.clone(),
                            &mut needs_constr_var,
                            scope,
                        );
                    }

                    let last_pattern = &last_clause.pattern[0];

                    let mut final_scope = scope.clone();
                    final_scope.push(self.id_gen.next());
                    pattern_vec.push(IR::Finally {
                        scope: final_scope.clone(),
                    });

                    self.build_ir(&last_clause.then, &mut clauses_vec, final_scope);
                    self.when_ir(
                        last_pattern,
                        &mut pattern_vec,
                        &mut clauses_vec,
                        &subject.tipo(),
                        constr_var.clone(),
                        &mut needs_constr_var,
                        scope.clone(),
                    );

                    if needs_constr_var {
                        ir_stack.push(IR::Lam {
                            scope: scope.clone(),
                            name: constr_var.clone(),
                        });

                        self.build_ir(&subject, ir_stack, scope.clone());

                        ir_stack.push(IR::When {
                            scope: scope.clone(),
                            count: clauses.len() + 1,
                            subject_name,
                            tipo: subject.tipo(),
                        });

                        let mut scope = scope;
                        scope.push(self.id_gen.next());

                        ir_stack.push(IR::Var {
                            scope,
                            constructor: ValueConstructor::public(
                                subject.tipo(),
                                ValueConstructorVariant::LocalVariable {
                                    location: Span::empty(),
                                },
                            ),
                            name: constr_var,
                        })
                    } else {
                        ir_stack.push(IR::When {
                            scope: scope.clone(),
                            count: clauses.len() + 1,
                            subject_name,
                            tipo: subject.tipo(),
                        });

                        let mut scope = scope;
                        scope.push(self.id_gen.next());

                        self.build_ir(&subject, ir_stack, scope);
                    }

                    ir_stack.append(&mut pattern_vec);
                };
            }
            TypedExpr::If { .. } => todo!(),
            TypedExpr::RecordAccess {
                record,
                index,
                tipo,
                ..
            } => {
                self.needs_field_access = true;

                ir_stack.push(IR::RecordAccess {
                    scope: scope.clone(),
                    index: *index,
                    tipo: tipo.clone(),
                });

                self.build_ir(record, ir_stack, scope);
            }
            TypedExpr::ModuleSelect {
                constructor,
                module_name,
                ..
            } => match constructor {
                tipo::ModuleValueConstructor::Record { .. } => todo!(),
                tipo::ModuleValueConstructor::Fn { name, .. } => {
                    let func = self.functions.get(&FunctionAccessKey {
                        module_name: module_name.clone(),
                        function_name: name.clone(),
                    });

                    if let Some(_func) = func {
                        todo!()
                    } else {
                        let type_info = self.module_types.get(module_name).unwrap();
                        let value = type_info.values.get(name).unwrap();
                        match &value.variant {
                            ValueConstructorVariant::ModuleFn { builtin, .. } => {
                                let builtin = builtin.unwrap();

                                ir_stack.push(IR::Builtin {
                                    func: builtin,
                                    scope,
                                });
                            }
                            _ => unreachable!(),
                        }
                    }
                }
                tipo::ModuleValueConstructor::Constant { .. } => todo!(),
            },
            TypedExpr::Todo { .. } => todo!(),
            TypedExpr::RecordUpdate { .. } => todo!(),
            TypedExpr::Negate { .. } => todo!(),
            TypedExpr::Tuple { .. } => todo!(),
        }
    }

    fn assignment_ir(
        &mut self,
        pattern: &Pattern<tipo::PatternConstructor, Arc<Type>>,
        pattern_vec: &mut Vec<IR>,
        value_vec: &mut Vec<IR>,
        _tipo: &Type,
        kind: AssignmentKind,
        scope: Vec<u64>,
    ) {
        match pattern {
            Pattern::Int { .. } => todo!(),
            Pattern::String { .. } => todo!(),
            Pattern::Var { name, .. } => {
                pattern_vec.push(IR::Assignment {
                    name: name.clone(),
                    kind,
                    scope,
                });

                pattern_vec.append(value_vec);
            }
            Pattern::VarUsage { .. } => todo!(),
            Pattern::Assign { .. } => todo!(),
            Pattern::Discard { .. } => todo!(),
            list @ Pattern::List { .. } => {
                self.pattern_ir(list, pattern_vec, value_vec, scope);
            }
            Pattern::Constructor { .. } => todo!(),
            Pattern::Tuple { .. } => todo!(),
        }
    }

    fn when_ir(
        &mut self,
        pattern: &Pattern<tipo::PatternConstructor, Arc<tipo::Type>>,
        pattern_vec: &mut Vec<IR>,
        values: &mut Vec<IR>,
        tipo: &Type,
        constr_var: String,
        needs_constr_var: &mut bool,
        scope: Vec<u64>,
    ) {
        match pattern {
            Pattern::Int { value, .. } => {
                pattern_vec.push(IR::Int {
                    scope,
                    value: value.clone(),
                });

                pattern_vec.append(values);
            }
            Pattern::String { .. } => todo!(),
            Pattern::Var { .. } => todo!(),
            Pattern::VarUsage { .. } => todo!(),
            Pattern::Assign { .. } => todo!(),
            Pattern::Discard { .. } => unreachable!(),
            Pattern::List { .. } => todo!(),
            Pattern::Constructor { arguments, .. } => {
                let mut needs_access_to_constr_var = false;
                for arg in arguments {
                    match arg.value {
                        Pattern::Var { .. }
                        | Pattern::List { .. }
                        | Pattern::Constructor { .. } => {
                            needs_access_to_constr_var = true;
                        }
                        _ => {}
                    }
                }

                let mut new_vec = vec![IR::Var {
                    constructor: ValueConstructor::public(
                        tipo.clone().into(),
                        ValueConstructorVariant::LocalVariable {
                            location: Span::empty(),
                        },
                    ),
                    name: constr_var,
                    scope: scope.clone(),
                }];

                if needs_access_to_constr_var {
                    *needs_constr_var = true;
                    new_vec.append(values);

                    self.pattern_ir(pattern, pattern_vec, &mut new_vec, scope);
                } else {
                    self.pattern_ir(pattern, pattern_vec, values, scope);
                }
            }
            Pattern::Tuple { .. } => todo!(),
        }
    }

    fn pattern_ir(
        &mut self,
        pattern: &Pattern<tipo::PatternConstructor, Arc<tipo::Type>>,
        pattern_vec: &mut Vec<IR>,
        values: &mut Vec<IR>,
        scope: Vec<u64>,
    ) {
        match dbg!(pattern) {
            Pattern::Int { .. } => todo!(),
            Pattern::String { .. } => todo!(),
            Pattern::Var { .. } => todo!(),
            Pattern::VarUsage { .. } => todo!(),
            Pattern::Assign { .. } => todo!(),
            Pattern::Discard { .. } => {
                pattern_vec.push(IR::Discard { scope });

                pattern_vec.append(values);
            }
            Pattern::List { elements, tail, .. } => {
                let mut elements_vec = vec![];

                let mut names = vec![];
                for element in elements {
                    match dbg!(element) {
                        Pattern::Var { name, .. } => {
                            names.push(name.clone());
                        }
                        a @ Pattern::List { .. } => {
                            let mut var_vec = vec![];
                            let item_name = format!("list_item_id_{}", self.id_gen.next());
                            names.push(item_name.clone());
                            var_vec.push(IR::Var {
                                constructor: ValueConstructor::public(
                                    Type::App {
                                        public: true,
                                        module: String::new(),
                                        name: String::new(),
                                        args: vec![],
                                    }
                                    .into(),
                                    ValueConstructorVariant::LocalVariable {
                                        location: Span::empty(),
                                    },
                                ),
                                name: item_name,
                                scope: scope.clone(),
                            });
                            self.pattern_ir(a, &mut elements_vec, &mut var_vec, scope.clone());
                        }
                        _ => todo!(),
                    }
                }

                if let Some(tail) = tail {
                    match &**tail {
                        Pattern::Var { name, .. } => names.push(name.clone()),
                        Pattern::Discard { .. } => {}
                        _ => unreachable!(),
                    }
                }

                pattern_vec.push(IR::ListAccessor {
                    names,
                    tail: tail.is_some(),
                    scope,
                });

                pattern_vec.append(values);
                pattern_vec.append(&mut elements_vec);
            }
            Pattern::Constructor {
                is_record,
                name: constr_name,
                arguments,
                constructor,
                tipo,
                ..
            } => {
                let data_type_key = match tipo.as_ref() {
                    Type::Fn { ret, .. } => match &**ret {
                        Type::App { module, name, .. } => DataTypeKey {
                            module_name: module.clone(),
                            defined_type: name.clone(),
                        },
                        _ => unreachable!(),
                    },
                    Type::App { module, name, .. } => DataTypeKey {
                        module_name: module.clone(),
                        defined_type: name.clone(),
                    },
                    _ => unreachable!(),
                };

                let data_type = self.data_types.get(&data_type_key).unwrap();
                let (index, constructor_type) = data_type
                    .constructors
                    .iter()
                    .enumerate()
                    .find(|(_, dt)| &dt.name == constr_name)
                    .unwrap();

                // push constructor Index
                pattern_vec.push(IR::Int {
                    value: index.to_string(),
                    scope: scope.clone(),
                });

                if *is_record {
                    let field_map = match constructor {
                        tipo::PatternConstructor::Record { field_map, .. } => {
                            field_map.clone().unwrap()
                        }
                    };

                    let mut type_map: HashMap<String, Arc<Type>> = HashMap::new();

                    for arg in &constructor_type.arguments {
                        let label = arg.label.clone().unwrap();
                        let field_type = arg.tipo.clone();

                        type_map.insert(label, field_type);
                    }

                    let arguments_index = arguments
                        .iter()
                        .map(|item| {
                            let label = item.label.clone().unwrap_or_default();
                            let field_index = field_map.fields.get(&label).unwrap_or(&0);
                            let (discard, var_name) = match &item.value {
                                Pattern::Var { name, .. } => (false, name.clone()),
                                Pattern::Discard { .. } => (true, "".to_string()),
                                Pattern::List { .. } => todo!(),
                                Pattern::Constructor { .. } => todo!(),
                                _ => todo!(),
                            };

                            (label, var_name, *field_index, discard)
                        })
                        .filter(|(_, _, _, discard)| !discard)
                        .sorted_by(|item1, item2| item1.2.cmp(&item2.2))
                        .collect::<Vec<(String, String, usize, bool)>>();

                    if !arguments_index.is_empty() {
                        pattern_vec.push(IR::FieldsExpose {
                            count: arguments_index.len() + 2,
                            indices: arguments_index
                                .iter()
                                .map(|(label, var_name, index, _)| {
                                    let field_type = type_map.get(label).unwrap();
                                    (*index, var_name.clone(), field_type.clone())
                                })
                                .collect_vec(),
                            scope,
                        });
                    }
                } else {
                    let mut type_map: HashMap<usize, Arc<Type>> = HashMap::new();

                    for (index, arg) in constructor_type.arguments.iter().enumerate() {
                        let field_type = arg.tipo.clone();

                        type_map.insert(index, field_type);
                    }

                    let arguments_index = arguments
                        .iter()
                        .enumerate()
                        .map(|(index, item)| {
                            let (discard, var_name) = match &item.value {
                                Pattern::Var { name, .. } => (false, name.clone()),
                                Pattern::Discard { .. } => (true, "".to_string()),
                                Pattern::List { .. } => todo!(),
                                Pattern::Constructor { .. } => todo!(),
                                _ => todo!(),
                            };

                            (var_name, index, discard)
                        })
                        .filter(|(_, _, discard)| !discard)
                        .collect::<Vec<(String, usize, bool)>>();

                    if !arguments_index.is_empty() {
                        pattern_vec.push(IR::FieldsExpose {
                            count: arguments_index.len() + 2,
                            indices: arguments_index
                                .iter()
                                .map(|(name, index, _)| {
                                    let field_type = type_map.get(index).unwrap();

                                    (*index, name.clone(), field_type.clone())
                                })
                                .collect_vec(),
                            scope,
                        });
                    }
                }
                pattern_vec.append(values);
            }
            Pattern::Tuple { .. } => todo!(),
        }
    }

    fn uplc_code_gen(&mut self, ir_stack: &mut Vec<IR>) -> Term<Name> {
        let mut arg_stack: Vec<Term<Name>> = vec![];

        while let Some(ir_element) = ir_stack.pop() {
            self.gen_uplc(ir_element, &mut arg_stack);
        }

        arg_stack[0].clone()
    }

    fn gen_uplc(&mut self, ir: IR, arg_stack: &mut Vec<Term<Name>>) {
        match ir {
            IR::Int { value, .. } => {
                let integer = value.parse().unwrap();

                let term = Term::Constant(Constant::Integer(integer));

                arg_stack.push(term);
            }
            IR::String { value, .. } => {
                let term = Term::Constant(Constant::String(value));

                arg_stack.push(term);
            }
            IR::ByteArray { bytes, .. } => {
                let term = Term::Constant(Constant::ByteString(bytes));
                arg_stack.push(term);
            }
            IR::Var {
                name, constructor, ..
            } => match constructor.variant {
                ValueConstructorVariant::LocalVariable { .. } => arg_stack.push(Term::Var(Name {
                    text: name,
                    unique: 0.into(),
                })),
                ValueConstructorVariant::ModuleConstant { .. } => todo!(),
                ValueConstructorVariant::ModuleFn { .. } => todo!(),
                ValueConstructorVariant::Record {
                    name: constr_name, ..
                } => {
                    let data_type_key = match &*constructor.tipo {
                        Type::App { module, name, .. } => DataTypeKey {
                            module_name: module.to_string(),
                            defined_type: name.to_string(),
                        },
                        Type::Fn { ret, .. } => match ret.deref() {
                            Type::App { module, name, .. } => DataTypeKey {
                                module_name: module.to_string(),
                                defined_type: name.to_string(),
                            },
                            _ => unreachable!(),
                        },
                        Type::Var { .. } => todo!(),
                        Type::Tuple { .. } => todo!(),
                    };

                    if data_type_key.defined_type == "Bool" {
                        arg_stack.push(Term::Constant(Constant::Bool(constr_name == "True")));
                    } else {
                        let data_type = self.data_types.get(&data_type_key).unwrap();
                        let (constr_index, _constr) = data_type
                            .constructors
                            .iter()
                            .enumerate()
                            .find(|(_, x)| x.name == *constr_name)
                            .unwrap();

                        let term = Term::Apply {
                            function: Term::Builtin(DefaultFunction::ConstrData).into(),
                            argument: Term::Apply {
                                function: Term::Apply {
                                    function: Term::Builtin(DefaultFunction::MkPairData).into(),
                                    argument: Term::Constant(Constant::Data(PlutusData::BigInt(
                                        BigInt::Int((constr_index as i128).try_into().unwrap()),
                                    )))
                                    .into(),
                                }
                                .into(),
                                argument: Term::Constant(Constant::Data(PlutusData::Array(vec![])))
                                    .into(),
                            }
                            .into(),
                        };

                        arg_stack.push(term);
                    }
                }
            },
            IR::Discard { .. } => {
                arg_stack.push(Term::Constant(Constant::Unit));
            }
            IR::List {
                count, tipo, tail, ..
            } => {
                let mut args = vec![];

                for _ in 0..count {
                    let arg = arg_stack.pop().unwrap();
                    args.push(arg);
                }
                let mut constants = vec![];
                for arg in &args {
                    if let Term::Constant(c) = arg {
                        constants.push(c.clone())
                    }
                }

                let list_type = match tipo.deref() {
                    Type::App { args, .. } => &args[0],
                    _ => unreachable!(),
                };

                if constants.len() == args.len() && !tail {
                    let list =
                        Term::Constant(Constant::ProtoList(list_type.get_uplc_type(), constants));

                    arg_stack.push(list);
                } else {
                    let mut term = if tail {
                        arg_stack.pop().unwrap()
                    } else {
                        Term::Constant(Constant::ProtoList(list_type.get_uplc_type(), vec![]))
                    };

                    for arg in args {
                        term = Term::Apply {
                            function: Term::Apply {
                                function: Term::Force(
                                    Term::Builtin(DefaultFunction::MkCons).into(),
                                )
                                .into(),
                                argument: arg.into(),
                            }
                            .into(),
                            argument: term.into(),
                        };
                    }
                    arg_stack.push(term);
                }
            }

            IR::Tail { .. } => todo!(),
            IR::ListAccessor { names, tail, .. } => {
                let value = arg_stack.pop().unwrap();
                let mut term = arg_stack.pop().unwrap();

                let mut id_list = vec![];

                for _ in 0..names.len() {
                    id_list.push(self.id_gen.next());
                }

                let current_index = 0;
                let (first_name, names) = names.split_first().unwrap();

                term = Term::Apply {
                    function: Term::Lambda {
                        parameter_name: Name {
                            text: first_name.clone(),
                            unique: 0.into(),
                        },
                        body: Term::Apply {
                            function: list_access_to_uplc(
                                names,
                                &id_list,
                                tail,
                                current_index,
                                term,
                            )
                            .into(),
                            argument: Term::Apply {
                                function: Term::Force(
                                    Term::Builtin(DefaultFunction::TailList).into(),
                                )
                                .into(),
                                argument: value.clone().into(),
                            }
                            .into(),
                        }
                        .into(),
                    }
                    .into(),
                    argument: Term::Apply {
                        function: Term::Force(Term::Builtin(DefaultFunction::HeadList).into())
                            .into(),
                        argument: value.into(),
                    }
                    .into(),
                };

                arg_stack.push(term);
            }
            IR::Call { count, .. } => {
                if count >= 2 {
                    let mut term = arg_stack.pop().unwrap();

                    for _ in 0..count - 1 {
                        let arg = arg_stack.pop().unwrap();

                        term = Term::Apply {
                            function: term.into(),
                            argument: arg.into(),
                        };
                    }
                    arg_stack.push(term);
                } else {
                    todo!()
                }
            }
            IR::Builtin { func, .. } => {
                let mut term = Term::Builtin(func);
                for _ in 0..func.force_count() {
                    term = Term::Force(term.into());
                }
                arg_stack.push(term);
            }
            IR::BinOp { name, tipo, .. } => {
                let left = arg_stack.pop().unwrap();
                let right = arg_stack.pop().unwrap();

                let term = match name {
                    BinOp::And => todo!(),
                    BinOp::Or => todo!(),
                    BinOp::Eq => {
                        let default_builtin = match tipo.deref() {
                            Type::App { name, .. } => {
                                if name == "Int" {
                                    Term::Builtin(DefaultFunction::EqualsInteger)
                                } else if name == "String" {
                                    Term::Builtin(DefaultFunction::EqualsString)
                                } else if name == "ByteArray" {
                                    Term::Builtin(DefaultFunction::EqualsByteString)
                                } else if name == "Bool" {
                                    let term = Term::Force(
                                        Term::Apply {
                                            function: Term::Apply {
                                                function: Term::Apply {
                                                    function: Term::Force(
                                                        Term::Builtin(DefaultFunction::IfThenElse)
                                                            .into(),
                                                    )
                                                    .into(),
                                                    argument: left.into(),
                                                }
                                                .into(),
                                                argument: Term::Delay(
                                                    Term::Apply {
                                                        function: Term::Apply {
                                                            function: Term::Apply {
                                                                function: Term::Force(
                                                                    Term::Builtin(
                                                                        DefaultFunction::IfThenElse,
                                                                    )
                                                                    .into(),
                                                                )
                                                                .into(),
                                                                argument: right.clone().into(),
                                                            }
                                                            .into(),
                                                            argument: Term::Constant(
                                                                Constant::Bool(true),
                                                            )
                                                            .into(),
                                                        }
                                                        .into(),
                                                        argument: Term::Constant(Constant::Bool(
                                                            false,
                                                        ))
                                                        .into(),
                                                    }
                                                    .into(),
                                                )
                                                .into(),
                                            }
                                            .into(),
                                            argument: Term::Delay(
                                                Term::Apply {
                                                    function: Term::Apply {
                                                        function: Term::Apply {
                                                            function: Term::Force(
                                                                Term::Builtin(
                                                                    DefaultFunction::IfThenElse,
                                                                )
                                                                .into(),
                                                            )
                                                            .into(),
                                                            argument: right.into(),
                                                        }
                                                        .into(),
                                                        argument: Term::Constant(Constant::Bool(
                                                            false,
                                                        ))
                                                        .into(),
                                                    }
                                                    .into(),
                                                    argument: Term::Constant(Constant::Bool(true))
                                                        .into(),
                                                }
                                                .into(),
                                            )
                                            .into(),
                                        }
                                        .into(),
                                    );

                                    arg_stack.push(term);
                                    return;
                                } else {
                                    Term::Builtin(DefaultFunction::EqualsData)
                                }
                            }
                            _ => unreachable!(),
                        };

                        Term::Apply {
                            function: Term::Apply {
                                function: default_builtin.into(),
                                argument: left.into(),
                            }
                            .into(),
                            argument: right.into(),
                        }
                    }
                    BinOp::NotEq => todo!(),
                    BinOp::LtInt => Term::Apply {
                        function: Term::Apply {
                            function: Term::Builtin(DefaultFunction::LessThanInteger).into(),
                            argument: left.into(),
                        }
                        .into(),
                        argument: right.into(),
                    },
                    BinOp::LtEqInt => todo!(),
                    BinOp::GtEqInt => todo!(),
                    BinOp::GtInt => Term::Apply {
                        function: Term::Apply {
                            function: Term::Builtin(DefaultFunction::LessThanInteger).into(),
                            argument: right.into(),
                        }
                        .into(),
                        argument: left.into(),
                    },
                    BinOp::AddInt => Term::Apply {
                        function: Term::Apply {
                            function: Term::Builtin(DefaultFunction::AddInteger).into(),
                            argument: left.into(),
                        }
                        .into(),
                        argument: right.into(),
                    },
                    BinOp::SubInt => todo!(),
                    BinOp::MultInt => todo!(),
                    BinOp::DivInt => todo!(),
                    BinOp::ModInt => todo!(),
                };
                arg_stack.push(term);
            }
            IR::Assignment { name, .. } => {
                let right_hand = arg_stack.pop().unwrap();
                let lam_body = arg_stack.pop().unwrap();

                let term = Term::Apply {
                    function: Term::Lambda {
                        parameter_name: Name {
                            text: name,
                            unique: 0.into(),
                        },
                        body: lam_body.into(),
                    }
                    .into(),
                    argument: right_hand.into(),
                };

                arg_stack.push(term);
            }
            IR::DefineFunc { .. } => {
                let _body = arg_stack.pop().unwrap();

                todo!()
            }
            IR::DefineConst { .. } => todo!(),
            IR::DefineConstrFields { .. } => todo!(),
            IR::DefineConstrFieldAccess { .. } => todo!(),
            IR::Lam { name, .. } => {
                let arg = arg_stack.pop().unwrap();

                let mut term = arg_stack.pop().unwrap();

                term = Term::Apply {
                    function: Term::Lambda {
                        parameter_name: Name {
                            text: name,
                            unique: 0.into(),
                        },
                        body: term.into(),
                    }
                    .into(),
                    argument: arg.into(),
                };
                arg_stack.push(term);
            }
            IR::When {
                subject_name, tipo, ..
            } => {
                let subject = arg_stack.pop().unwrap();

                let mut term = arg_stack.pop().unwrap();

                term = if tipo.is_int() || tipo.is_bytearray() || tipo.is_string() || tipo.is_list()
                {
                    Term::Apply {
                        function: Term::Lambda {
                            parameter_name: Name {
                                text: subject_name,
                                unique: 0.into(),
                            },
                            body: term.into(),
                        }
                        .into(),
                        argument: subject.into(),
                    }
                } else {
                    Term::Apply {
                        function: Term::Lambda {
                            parameter_name: Name {
                                text: subject_name,
                                unique: 0.into(),
                            },
                            body: term.into(),
                        }
                        .into(),
                        argument: constr_index_exposer(subject).into(),
                    }
                };

                arg_stack.push(term);
            }
            IR::Clause {
                tipo, subject_name, ..
            } => {
                // clause to compare
                let clause = arg_stack.pop().unwrap();

                // the body to be run if the clause matches
                let body = arg_stack.pop().unwrap();

                // the final branch in the when expression
                let mut term = arg_stack.pop().unwrap();

                let checker = if tipo.is_int() {
                    Term::Apply {
                        function: DefaultFunction::EqualsInteger.into(),
                        argument: Term::Var(Name {
                            text: subject_name,
                            unique: 0.into(),
                        })
                        .into(),
                    }
                } else if tipo.is_bytearray() {
                    Term::Apply {
                        function: DefaultFunction::EqualsByteString.into(),
                        argument: Term::Var(Name {
                            text: subject_name,
                            unique: 0.into(),
                        })
                        .into(),
                    }
                } else if tipo.is_bool() {
                    todo!()
                } else if tipo.is_string() {
                    Term::Apply {
                        function: DefaultFunction::EqualsString.into(),
                        argument: Term::Var(Name {
                            text: subject_name,
                            unique: 0.into(),
                        })
                        .into(),
                    }
                } else if tipo.is_list() {
                    todo!()
                } else {
                    Term::Apply {
                        function: DefaultFunction::EqualsInteger.into(),
                        argument: Term::Var(Name {
                            text: subject_name,
                            unique: 0.into(),
                        })
                        .into(),
                    }
                };

                term = Term::Apply {
                    function: Term::Apply {
                        function: Term::Apply {
                            function: Term::Force(DefaultFunction::IfThenElse.into()).into(),
                            argument: Term::Apply {
                                function: checker.into(),
                                argument: clause.into(),
                            }
                            .into(),
                        }
                        .into(),
                        argument: Term::Delay(body.into()).into(),
                    }
                    .into(),
                    argument: Term::Delay(term.into()).into(),
                }
                .force_wrap();

                arg_stack.push(term);
            }
            IR::Finally { .. } => {
                let _clause = arg_stack.pop().unwrap();
            }
            IR::If { .. } => todo!(),
            IR::Constr { .. } => todo!(),
            IR::Fields { .. } => todo!(),
            IR::RecordAccess { index, tipo, .. } => {
                let constr = arg_stack.pop().unwrap();

                let mut term = Term::Apply {
                    function: Term::Apply {
                        function: Term::Var(Name {
                            text: CONSTR_GET_FIELD.to_string(),
                            unique: 0.into(),
                        })
                        .into(),
                        argument: Term::Apply {
                            function: Term::Var(Name {
                                text: CONSTR_FIELDS_EXPOSER.to_string(),
                                unique: 0.into(),
                            })
                            .into(),
                            argument: constr.into(),
                        }
                        .into(),
                    }
                    .into(),
                    argument: Term::Constant(Constant::Integer(index.into())).into(),
                };

                if tipo.is_int() {
                    term = Term::Apply {
                        function: Term::Builtin(DefaultFunction::UnIData).into(),
                        argument: term.into(),
                    };
                } else if tipo.is_bytearray() {
                    term = Term::Apply {
                        function: Term::Builtin(DefaultFunction::UnBData).into(),
                        argument: term.into(),
                    };
                } else if tipo.is_list() {
                    term = Term::Apply {
                        function: Term::Builtin(DefaultFunction::UnListData).into(),
                        argument: term.into(),
                    };
                }

                arg_stack.push(term);
            }
            IR::FieldsExpose {
                count: _count,
                indices,
                ..
            } => {
                self.needs_field_access = true;

                let constr_var = arg_stack.pop().unwrap();
                let mut body = arg_stack.pop().unwrap();

                let mut indices = indices.into_iter().rev();
                let highest = indices.next().unwrap();
                let mut id_list = vec![];

                for _ in 0..highest.0 {
                    id_list.push(self.id_gen.next());
                }

                let constr_name_lam = format!("__constr_fields_{}", self.id_gen.next());
                let highest_loop_index = highest.0 as i32 - 1;
                let last_prev_tail = Term::Var(Name {
                    text: if highest_loop_index == -1 {
                        constr_name_lam.clone()
                    } else {
                        format!(
                            "__tail_{}_{}",
                            highest_loop_index, id_list[highest_loop_index as usize]
                        )
                    },
                    unique: 0.into(),
                });

                let unwrapper = if highest.2.is_int() {
                    Term::Apply {
                        function: DefaultFunction::UnIData.into(),
                        argument: Term::Apply {
                            function: Term::Builtin(DefaultFunction::HeadList).force_wrap().into(),
                            argument: last_prev_tail.into(),
                        }
                        .into(),
                    }
                } else if highest.2.is_bytearray() {
                    Term::Apply {
                        function: DefaultFunction::UnBData.into(),
                        argument: Term::Apply {
                            function: Term::Builtin(DefaultFunction::HeadList).force_wrap().into(),
                            argument: last_prev_tail.into(),
                        }
                        .into(),
                    }
                } else if highest.2.is_list() {
                    Term::Apply {
                        function: DefaultFunction::UnListData.into(),
                        argument: Term::Apply {
                            function: Term::Builtin(DefaultFunction::HeadList).force_wrap().into(),
                            argument: last_prev_tail.into(),
                        }
                        .into(),
                    }
                } else {
                    Term::Apply {
                        function: Term::Builtin(DefaultFunction::HeadList).force_wrap().into(),
                        argument: last_prev_tail.into(),
                    }
                };

                body = Term::Apply {
                    function: Term::Lambda {
                        parameter_name: Name {
                            text: highest.1,
                            unique: 0.into(),
                        },
                        body: body.into(),
                    }
                    .into(),
                    argument: unwrapper.into(),
                };

                let mut current_field = None;
                for index in (0..highest.0).rev() {
                    let current_tail_index = index;
                    let previous_tail_index = if index == 0 { 0 } else { index - 1 };
                    let current_tail_id = id_list[index];
                    let previous_tail_id = if index == 0 { 0 } else { id_list[index - 1] };
                    if current_field.is_none() {
                        current_field = indices.next();
                    }

                    let prev_tail = if index == 0 {
                        Term::Var(Name {
                            text: constr_name_lam.clone(),
                            unique: 0.into(),
                        })
                    } else {
                        Term::Var(Name {
                            text: format!("__tail_{previous_tail_index}_{previous_tail_id}"),
                            unique: 0.into(),
                        })
                    };

                    if let Some(ref field) = current_field {
                        if field.0 == index {
                            let unwrapper = if field.2.is_int() {
                                Term::Apply {
                                    function: DefaultFunction::UnIData.into(),
                                    argument: Term::Apply {
                                        function: Term::Builtin(DefaultFunction::HeadList)
                                            .force_wrap()
                                            .into(),
                                        argument: prev_tail.clone().into(),
                                    }
                                    .into(),
                                }
                            } else if field.2.is_bytearray() {
                                Term::Apply {
                                    function: DefaultFunction::UnBData.into(),
                                    argument: Term::Apply {
                                        function: Term::Builtin(DefaultFunction::HeadList)
                                            .force_wrap()
                                            .into(),
                                        argument: prev_tail.clone().into(),
                                    }
                                    .into(),
                                }
                            } else if field.2.is_list() {
                                Term::Apply {
                                    function: DefaultFunction::UnListData.into(),
                                    argument: Term::Apply {
                                        function: Term::Builtin(DefaultFunction::HeadList)
                                            .force_wrap()
                                            .into(),
                                        argument: prev_tail.clone().into(),
                                    }
                                    .into(),
                                }
                            } else {
                                Term::Apply {
                                    function: Term::Builtin(DefaultFunction::HeadList)
                                        .force_wrap()
                                        .into(),
                                    argument: prev_tail.clone().into(),
                                }
                            };

                            body = Term::Apply {
                                function: Term::Lambda {
                                    parameter_name: Name {
                                        text: field.1.clone(),
                                        unique: 0.into(),
                                    },
                                    body: Term::Apply {
                                        function: Term::Lambda {
                                            parameter_name: Name {
                                                text: format!(
                                                    "__tail_{current_tail_index}_{current_tail_id}"
                                                ),
                                                unique: 0.into(),
                                            },
                                            body: body.into(),
                                        }
                                        .into(),
                                        argument: Term::Apply {
                                            function: Term::Builtin(DefaultFunction::TailList)
                                                .force_wrap()
                                                .into(),
                                            argument: prev_tail.into(),
                                        }
                                        .into(),
                                    }
                                    .into(),
                                }
                                .into(),
                                argument: unwrapper.into(),
                            };

                            current_field = None;
                        } else {
                            body = Term::Apply {
                                function: Term::Lambda {
                                    parameter_name: Name {
                                        text: format!(
                                            "__tail_{current_tail_index}_{current_tail_id}"
                                        ),
                                        unique: 0.into(),
                                    },
                                    body: body.into(),
                                }
                                .into(),
                                argument: Term::Apply {
                                    function: Term::Builtin(DefaultFunction::TailList)
                                        .force_wrap()
                                        .force_wrap()
                                        .into(),
                                    argument: prev_tail.into(),
                                }
                                .into(),
                            }
                        }
                    } else {
                        body = Term::Apply {
                            function: Term::Lambda {
                                parameter_name: Name {
                                    text: format!("__tail_{current_tail_index}_{current_tail_id}"),
                                    unique: 0.into(),
                                },
                                body: body.into(),
                            }
                            .into(),
                            argument: Term::Apply {
                                function: Term::Builtin(DefaultFunction::TailList)
                                    .force_wrap()
                                    .force_wrap()
                                    .into(),
                                argument: prev_tail.into(),
                            }
                            .into(),
                        }
                    }
                }

                body = Term::Apply {
                    function: Term::Lambda {
                        parameter_name: Name {
                            text: constr_name_lam,
                            unique: 0.into(),
                        },
                        body: body.into(),
                    }
                    .into(),
                    argument: Term::Apply {
                        function: Term::Var(Name {
                            text: CONSTR_FIELDS_EXPOSER.to_string(),
                            unique: 0.into(),
                        })
                        .into(),
                        argument: constr_var.into(),
                    }
                    .into(),
                };

                arg_stack.push(body);
            }
            IR::Todo { .. } => todo!(),
            IR::RecordUpdate { .. } => todo!(),
            IR::Negate { .. } => todo!(),
        }
    }

    pub(crate) fn define_ir(&mut self, ir_stack: &mut Vec<IR>) {
        let mut to_be_defined_map: IndexMap<FunctionAccessKey, Vec<u64>> = IndexMap::new();
        let mut defined_func_and_calls: IndexMap<FunctionAccessKey, FuncComponents> =
            IndexMap::new();
        let mut func_index_map: IndexMap<FunctionAccessKey, (usize, Vec<u64>)> = IndexMap::new();

        for (index, ir) in ir_stack.iter().enumerate().rev() {
            match ir {
                IR::Var {
                    scope, constructor, ..
                } => {
                    if let ValueConstructorVariant::ModuleFn {
                        name,
                        module,
                        builtin,
                        ..
                    } = &constructor.variant
                    {
                        if builtin.is_none() {
                            let function_key = FunctionAccessKey {
                                module_name: module.clone(),
                                function_name: name.clone(),
                            };

                            if let Some(scope_prev) = to_be_defined_map.get(&function_key) {
                                let new_scope = get_common_ancestor(scope, scope_prev);

                                to_be_defined_map.insert(function_key, new_scope);
                            } else if defined_func_and_calls.get(&function_key).is_some() {
                                to_be_defined_map.insert(function_key.clone(), scope.to_vec());
                            } else {
                                let function = self.functions.get(&function_key).unwrap();

                                let mut func_ir = vec![];

                                self.build_ir(&function.body, &mut func_ir, scope.to_vec());

                                to_be_defined_map.insert(function_key.clone(), scope.to_vec());
                                let mut func_calls = vec![];

                                for ir in func_ir.clone() {
                                    if let IR::Var {
                                        constructor:
                                            ValueConstructor {
                                                variant:
                                                    ValueConstructorVariant::ModuleFn {
                                                        name: func_name,
                                                        module,
                                                        ..
                                                    },
                                                ..
                                            },
                                        ..
                                    } = ir
                                    {
                                        func_calls.push(FunctionAccessKey {
                                            module_name: module.clone(),
                                            function_name: func_name.clone(),
                                        })
                                    }
                                }

                                let mut args = vec![];

                                for arg in function.arguments.iter() {
                                    match &arg.arg_name {
                                        ArgName::Named { name, .. }
                                        | ArgName::NamedLabeled { name, .. } => {
                                            args.push(name.clone());
                                        }
                                        _ => {}
                                    }
                                }
                                if let Ok(index) = func_calls.binary_search(&function_key) {
                                    func_calls.remove(index);
                                    defined_func_and_calls.insert(
                                        function_key,
                                        FuncComponents {
                                            ir: func_ir,
                                            dependencies: func_calls,
                                            recursive: true,
                                            args,
                                        },
                                    );
                                } else {
                                    defined_func_and_calls.insert(
                                        function_key,
                                        FuncComponents {
                                            ir: func_ir,
                                            dependencies: func_calls,
                                            recursive: false,
                                            args,
                                        },
                                    );
                                }
                            }
                        }
                    }
                }
                a => {
                    let scope = a.scope();

                    for func in to_be_defined_map.clone().iter() {
                        println!(
                            "MADE IT HERE 222 and func_scope is {:#?} and scope is {:#?}",
                            func.1.clone(),
                            scope.clone()
                        );

                        if dbg!(get_common_ancestor(&scope, func.1) == scope.to_vec()) {
                            if let Some((_, index_scope)) = func_index_map.get(func.0) {
                                if get_common_ancestor(index_scope, func.1) == scope.to_vec() {
                                    println!("DID insert again");
                                    func_index_map.insert(func.0.clone(), (index, scope.clone()));
                                    to_be_defined_map.shift_remove(func.0);
                                } else {
                                    println!(
                                        "DID update, index_scope is {:#?} and func is {:#?}",
                                        index_scope, func.1
                                    );
                                    to_be_defined_map.insert(
                                        func.0.clone(),
                                        get_common_ancestor(index_scope, func.1),
                                    );
                                    println!("to_be_defined: {:#?}", to_be_defined_map);
                                }
                            } else {
                                println!("DID insert");
                                func_index_map.insert(func.0.clone(), (index, scope.clone()));
                                to_be_defined_map.shift_remove(func.0);
                            }
                        }
                    }
                }
            }
        }

        for func_index in func_index_map.iter() {
            println!("INDEX FUNC IS {func_index:#?}");
            let func = func_index.0;
            let (index, scope) = func_index.1;

            let function_components = defined_func_and_calls.get(func).unwrap();
            let dependencies = function_components.dependencies.clone();

            let mut sorted_functions = vec![];

            for dependency in dependencies {
                let (_, dependency_scope) = func_index_map.get(&dependency).unwrap();
                if get_common_ancestor(scope, dependency_scope) == scope.clone() {
                    let components = defined_func_and_calls.get(&dependency).unwrap();
                    let mut dependency_ir = components.ir.clone();
                    self.define_ir(&mut dependency_ir);
                    sorted_functions.append(&mut dependency_ir);
                }
            }
            if !self.defined_functions.contains_key(func) {
                for item in sorted_functions.into_iter().rev() {
                    ir_stack.insert(*index, item);
                }
                ir_stack.insert(
                    *index,
                    IR::DefineFunc {
                        scope: scope.clone(),
                        func_name: func.function_name.clone(),
                        module_name: func.module_name.clone(),
                        params: function_components.args.clone(),
                        recursive: function_components.recursive,
                    },
                );
                self.defined_functions.insert(func.clone(), ());
            }
        }
    }
}

fn get_common_ancestor(scope: &[u64], scope_prev: &[u64]) -> Vec<u64> {
    let longest_length = if scope.len() >= scope_prev.len() {
        scope.len()
    } else {
        scope_prev.len()
    };

    if *scope == *scope_prev {
        return scope.to_vec();
    }

    for index in 0..longest_length {
        if scope.get(index).is_none() {
            return scope.to_vec();
        } else if scope_prev.get(index).is_none() {
            return scope_prev.to_vec();
        } else if scope[index] != scope_prev[index] {
            return scope[0..index].to_vec();
        }
    }
    vec![]
}

fn list_access_to_uplc(
    names: &[String],
    id_list: &[u64],
    tail: bool,
    current_index: usize,
    term: Term<Name>,
) -> Term<Name> {
    let (first, names) = names.split_first().unwrap();

    if names.len() == 1 && tail {
        Term::Lambda {
            parameter_name: Name {
                text: format!("tail_index_{}_{}", current_index, id_list[current_index]),
                unique: 0.into(),
            },
            body: Term::Apply {
                function: Term::Lambda {
                    parameter_name: Name {
                        text: first.clone(),
                        unique: 0.into(),
                    },
                    body: Term::Apply {
                        function: Term::Lambda {
                            parameter_name: Name {
                                text: names[0].clone(),
                                unique: 0.into(),
                            },
                            body: term.into(),
                        }
                        .into(),
                        argument: Term::Apply {
                            function: Term::Force(Term::Builtin(DefaultFunction::TailList).into())
                                .into(),
                            argument: Term::Var(Name {
                                text: format!(
                                    "tail_index_{}_{}",
                                    current_index, id_list[current_index]
                                ),
                                unique: 0.into(),
                            })
                            .into(),
                        }
                        .into(),
                    }
                    .into(),
                }
                .into(),
                argument: Term::Apply {
                    function: Term::Force(Term::Builtin(DefaultFunction::HeadList).into()).into(),
                    argument: Term::Var(Name {
                        text: format!("tail_index_{}_{}", current_index, id_list[current_index]),
                        unique: 0.into(),
                    })
                    .into(),
                }
                .into(),
            }
            .into(),
        }
    } else if names.is_empty() {
        Term::Lambda {
            parameter_name: Name {
                text: format!("tail_index_{}_{}", current_index, id_list[current_index]),
                unique: 0.into(),
            },
            body: Term::Apply {
                function: Term::Lambda {
                    parameter_name: Name {
                        text: first.clone(),
                        unique: 0.into(),
                    },
                    body: term.into(),
                }
                .into(),
                argument: Term::Apply {
                    function: Term::Force(Term::Builtin(DefaultFunction::HeadList).into()).into(),
                    argument: Term::Var(Name {
                        text: format!("tail_index_{}_{}", current_index, id_list[current_index]),
                        unique: 0.into(),
                    })
                    .into(),
                }
                .into(),
            }
            .into(),
        }
    } else {
        Term::Lambda {
            parameter_name: Name {
                text: format!("tail_index_{}_{}", current_index, id_list[current_index]),
                unique: 0.into(),
            },
            body: Term::Apply {
                function: Term::Lambda {
                    parameter_name: Name {
                        text: first.clone(),
                        unique: 0.into(),
                    },
                    body: Term::Apply {
                        function: list_access_to_uplc(
                            names,
                            id_list,
                            tail,
                            current_index + 1,
                            term,
                        )
                        .into(),
                        argument: Term::Apply {
                            function: Term::Force(Term::Builtin(DefaultFunction::TailList).into())
                                .into(),
                            argument: Term::Var(Name {
                                text: format!(
                                    "tail_index_{}_{}",
                                    current_index, id_list[current_index]
                                ),
                                unique: 0.into(),
                            })
                            .into(),
                        }
                        .into(),
                    }
                    .into(),
                }
                .into(),
                argument: Term::Apply {
                    function: Term::Force(Term::Builtin(DefaultFunction::HeadList).into()).into(),
                    argument: Term::Var(Name {
                        text: format!("tail_index_{}_{}", current_index, id_list[current_index]),
                        unique: 0.into(),
                    })
                    .into(),
                }
                .into(),
            }
            .into(),
        }
    }
}