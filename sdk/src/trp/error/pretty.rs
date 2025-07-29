use pallas::ledger::addresses::Address;
use tx3_lang::{
    applying::Error as ApplyingError,
    backend::Error as BackendError,
    ir::{AssetExpr, BuiltInOp, Coerce, CompilerOp, Expression, InputQuery, Param, Type},
    UtxoRef,
};
use tx3_resolver::Error as ResolverError;

pub trait PrettyError {
    fn pretty(&self) -> String;
}

impl PrettyError for ResolverError {
    fn pretty(&self) -> String {
        match self {
            ResolverError::InputQueryTooBroad => {
                "Input query is too broad, try adding more constraints.".to_string()
            }
            ResolverError::InputNotResolved(v, q) => {
                format!("Input not resolved: {} with {}", v, q.pretty())
            }
            ResolverError::ExpectedData(data_type, expression) => {
                format!("Expected {}, got {}", data_type, expression.pretty())
            }
            ResolverError::ApplyError(error) => {
                format!("Apply error: {}", error.pretty())
            }
            ResolverError::CantCompileNonConstantTir => {
                "Can't compile non-constant TIR.".to_string()
            }
            ResolverError::BackendError(error) => {
                format!("Backend error: {}", error.pretty())
            }
        }
    }
}

impl PrettyError for Expression {
    fn pretty(&self) -> String {
        match self {
            Expression::None => "none".to_string(),
            Expression::Bytes(bytes) => hex::encode(bytes),
            Expression::Number(n) => n.to_string(),
            Expression::Bool(b) => b.to_string(),
            Expression::String(s) => s.to_owned(),
            Expression::Address(addr) => Address::from_bytes(&addr).unwrap().to_bech32().unwrap(),
            Expression::Hash(hash) => hex::encode(hash),
            Expression::UtxoRefs(refs) => refs
                .iter()
                .map(|r| r.pretty())
                .collect::<Vec<String>>()
                .join(", "),
            Expression::Assets(assets) => {
                let assets_str: Vec<String> = assets.iter().map(|a| a.pretty()).collect();
                format!("assets[{}]", assets_str.join(", "))
            }
            Expression::List(items) => {
                let items_str: Vec<String> = items.iter().map(|item| item.pretty()).collect();
                format!("[{}]", items_str.join(", "))
            }
            Expression::Tuple(tuple) => {
                format!("({}, {})", tuple.0.pretty(), tuple.1.pretty())
            }
            Expression::Struct(s) => {
                let fields_str: Vec<String> = s.fields.iter().map(|f| f.pretty()).collect();
                format!("struct_{} {{ {} }}", s.constructor, fields_str.join(", "))
            }
            Expression::UtxoSet(utxos) => {
                format!("utxo_set({} items)", utxos.len())
            }
            Expression::EvalParam(param) => {
                format!("eval_param({})", param.pretty())
            }
            Expression::EvalBuiltIn(op) => {
                format!("eval_builtin({})", op.pretty())
            }
            Expression::EvalCompiler(op) => {
                format!("eval_compiler({})", op.pretty())
            }
            Expression::EvalCoerce(coerce) => {
                format!("eval_coerce({})", coerce.pretty())
            }
            Expression::AdHocDirective(directive) => {
                format!("directive({})", directive.name)
            }
        }
    }
}

impl PrettyError for UtxoRef {
    fn pretty(&self) -> String {
        format!("{}#{}", hex::encode(&self.txid), self.index)
    }
}

impl PrettyError for AssetExpr {
    fn pretty(&self) -> String {
        if self.policy.is_none() {
            return format!("lovelace = {}", self.amount.pretty());
        }

        format!(
            "{}.{} = {}",
            self.policy.pretty(),
            self.asset_name.pretty(),
            self.amount.pretty()
        )
    }
}

impl PrettyError for Param {
    fn pretty(&self) -> String {
        match self {
            Param::Set(expr) => format!("set({})", expr.pretty()),
            Param::ExpectValue(name, type_) => {
                format!("expect_value({}: {})", name, type_.pretty())
            }
            Param::ExpectInput(name, query) => {
                format!("expect_input({}: {})", name, query.pretty())
            }
            Param::ExpectFees => "expect_fees".to_string(),
        }
    }
}

impl PrettyError for Type {
    fn pretty(&self) -> String {
        match self {
            Type::Undefined => "Undefined".to_string(),
            Type::Unit => "Unit".to_string(),
            Type::Int => "Int".to_string(),
            Type::Bool => "Bool".to_string(),
            Type::Bytes => "Bytes".to_string(),
            Type::Address => "Address".to_string(),
            Type::Utxo => "Utxo".to_string(),
            Type::UtxoRef => "UtxoRef".to_string(),
            Type::AnyAsset => "AnyAsset".to_string(),
            Type::List => "List".to_string(),
            Type::Custom(name) => name.clone(),
        }
    }
}

impl PrettyError for InputQuery {
    fn pretty(&self) -> String {
        let flags = match (self.many, self.collateral) {
            (true, true) => " [many, collateral]",
            (true, false) => " [many]",
            (false, true) => " [collateral]",
            (false, false) => "",
        };
        format!(
            "query(addr: {}, min: {}, ref: {}{})",
            self.address.pretty(),
            self.min_amount.pretty(),
            self.r#ref.pretty(),
            flags
        )
    }
}

impl PrettyError for BuiltInOp {
    fn pretty(&self) -> String {
        match self {
            BuiltInOp::NoOp(expr) => format!("noop({})", expr.pretty()),
            BuiltInOp::Add(a, b) => format!("{} + {}", a.pretty(), b.pretty()),
            BuiltInOp::Sub(a, b) => format!("{} - {}", a.pretty(), b.pretty()),
            BuiltInOp::Concat(a, b) => format!("{} ++ {}", a.pretty(), b.pretty()),
            BuiltInOp::Negate(expr) => format!("-{}", expr.pretty()),
            BuiltInOp::Property(expr, idx) => format!("{}.{}", expr.pretty(), idx),
        }
    }
}

impl PrettyError for CompilerOp {
    fn pretty(&self) -> String {
        match self {
            CompilerOp::BuildScriptAddress(expr) => {
                format!("build_script_address({})", expr.pretty())
            }
        }
    }
}

impl PrettyError for Coerce {
    fn pretty(&self) -> String {
        match self {
            Coerce::NoOp(expr) => format!("coerce_noop({})", expr.pretty()),
            Coerce::IntoAssets(expr) => format!("into_assets({})", expr.pretty()),
            Coerce::IntoDatum(expr) => format!("into_datum({})", expr.pretty()),
            Coerce::IntoScript(expr) => format!("into_script({})", expr.pretty()),
        }
    }
}

impl PrettyError for BackendError {
    fn pretty(&self) -> String {
        match self {
            BackendError::TransientError(s) => {
                format!("a transient error occurred: {}", s)
            }
            BackendError::StoreError(s) => format!("a storage error occurred: {}", s),
            BackendError::InvalidPattern(s) => format!("invalid pattern: {}", s),
            BackendError::UtxoNotFound(utxo_ref) => {
                format!("UTXO not found: {}", utxo_ref.pretty())
            }
            BackendError::CoerceError(from, to) => {
                format!("error coercing {} into {}", from, to)
            }
            BackendError::ConsistencyError(s) => {
                format!("a consistency error occurred: {}", s)
            }
            BackendError::ArgNotAssigned(s) => format!("argument not assigned: {}", s),
            BackendError::FormatError(s) => format!("a format error occurred: {}", s),
            BackendError::MissingExpression(s) => format!("missing expression: {}", s),
            BackendError::ValueOverflow(s) => format!("a value overflow occurred: {}", s),
            BackendError::NoAstAnalysis => "no AST analysis was performed".to_string(),
            BackendError::CantResolveSymbol(s) => format!("can't resolve symbol: {}", s),
            BackendError::CantReduce(op) => format!("can't reduce: {}", op.pretty()),
        }
    }
}

impl PrettyError for ApplyingError {
    fn pretty(&self) -> String {
        match self {
            ApplyingError::InvalidBuiltInOp(op) => {
                format!("invalid built-in operation: {}", op.pretty())
            }
            ApplyingError::BackendError(be) => {
                format!("a backend error occurred: {}", be)
            }
            ApplyingError::InvalidArgument(val, name) => {
                format!("invalid argument {:?} for {}", val, name)
            }
            ApplyingError::PropertyNotFound(prop, obj) => {
                format!("property '{}' not found in {}", prop, obj)
            }
            ApplyingError::PropertyIndexNotFound(idx, obj) => {
                format!("property index {} not found in {}", idx, obj)
            }
            ApplyingError::InvalidBinaryOp(op, a, b) => {
                format!("invalid binary operation '{}' over {} and {}", op, a, b)
            }
            ApplyingError::InvalidUnaryOp(op, a) => {
                format!("invalid unary operation '{}' over {}", op, a)
            }
            ApplyingError::CannotCoerceIntoAssets(expr) => {
                format!("cannot coerce {} into assets", expr.pretty())
            }
            ApplyingError::CannotCoerceIntoDatum(expr) => {
                format!("cannot coerce {} into datum", expr.pretty())
            }
        }
    }
}
