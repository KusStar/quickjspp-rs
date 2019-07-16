mod bindings;
mod value;

use std::{convert::TryFrom, error, fmt};

pub use bindings::Callback;
pub use value::*;

#[derive(PartialEq, Debug)]
pub enum ExecutionError {
    InputWithZeroBytes,
    Conversion(ValueError),
    Internal(String),
    Exception(JsValue),
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use ExecutionError::*;
        match self {
            InputWithZeroBytes => write!(f, "Invalid script input: code contains zero byte (\\0)"),
            Conversion(e) => e.fmt(f),
            Internal(e) => write!(f, "Internal error: {}", e),
            Exception(e) => write!(f, "Execution failed with exception: {:?}", e),
        }
    }
}

impl error::Error for ExecutionError {}

impl From<ValueError> for ExecutionError {
    fn from(v: ValueError) -> Self {
        ExecutionError::Conversion(v)
    }
}

#[derive(Debug)]
pub enum ContextError {
    RuntimeCreationFailed,
    ContextCreationFailed,
}

impl fmt::Display for ContextError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use ContextError::*;
        match self {
            RuntimeCreationFailed => write!(f, "Could not create runtime"),
            ContextCreationFailed => write!(f, "Could not create context"),
        }
    }
}

impl error::Error for ContextError {}

pub struct Context {
    wrapper: bindings::ContextWrapper,
}

impl Context {
    pub fn new() -> Result<Self, ContextError> {
        Ok(Self {
            wrapper: bindings::ContextWrapper::new()?,
        })
    }

    // Evaluates Javascript code and returns the value of the final expression.
    pub fn eval(&self, code: &str) -> Result<JsValue, ExecutionError> {
        let value_raw = self.wrapper.eval(code)?;
        let value = value_raw.to_value()?;
        Ok(value)
    }

    /// Evaluates Javascript code and returns the value of the final expression
    /// as a Rust type.
    pub fn eval_as<R>(&self, code: &str) -> Result<R, ExecutionError>
    where
        R: TryFrom<JsValue>,
        R::Error: Into<ValueError>,
    {
        let value_raw = self.wrapper.eval(code)?;
        let value = value_raw.to_value()?;
        let ret = R::try_from(value).map_err(|e| e.into())?;
        Ok(ret)
    }

    /// Call a global function in the Javascript namespace.
    pub fn call_function(
        &self,
        function_name: &str,
        args: impl IntoIterator<Item = impl Into<JsValue>>,
    ) -> Result<JsValue, ExecutionError> {
        let qargs = args
            .into_iter()
            .map(|arg| self.wrapper.serialize_value(arg.into()))
            .collect::<Result<Vec<_>, _>>()?;

        let global = self.wrapper.global()?;
        let func_obj = global.property(function_name)?;

        if !func_obj.is_object() {
            return Err(ExecutionError::Internal(format!(
                "Could not find function '{}' in global scope: does not exist, or not an object",
                function_name
            )));
        }

        let value = self.wrapper.call_function(func_obj, qargs)?.to_value()?;
        Ok(value)
    }

    /// Add a global JS function that is backed by a Rust function or closure.
    pub fn add_callback<F>(
        &self,
        name: &str,
        callback: impl bindings::Callback<F> + 'static,
    ) -> Result<(), ExecutionError> {
        self.wrapper.add_callback(name, callback)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // #[test]
    // fn test_global_properties() {
    //     let c = Context::new().unwrap();

    //     assert_eq!(
    //         c.global_property("lala"),
    //         Err(ExecutionError::Exception(
    //             "Global object does not have property 'lala'".into()
    //         ))
    //     );

    //     c.set_global_property("testprop", true).unwrap();
    //     assert_eq!(
    //         c.global_property("testprop").unwrap(),
    //         JsValue::Bool(true),
    //     );
    // }

    #[test]
    fn test_eval_pass() {
        let c = Context::new().unwrap();

        let cases = vec![
            ("null", Ok(JsValue::Null)),
            ("true", Ok(JsValue::Bool(true))),
            ("2 > 10", Ok(JsValue::Bool(false))),
            ("1", Ok(JsValue::Int(1))),
            ("1 + 1", Ok(JsValue::Int(2))),
            ("1.1", Ok(JsValue::Float(1.1))),
            ("2.2 * 2 + 5", Ok(JsValue::Float(9.4))),
            ("\"abc\"", Ok(JsValue::String("abc".into()))),
            (
                "[1,2]",
                Ok(JsValue::Array(vec![JsValue::Int(1), JsValue::Int(2)])),
            ),
        ];

        for (code, res) in cases.into_iter() {
            assert_eq!(c.eval(code), res,);
        }

        assert_eq!(c.eval_as::<bool>("true").unwrap(), true,);
        assert_eq!(c.eval_as::<i32>("1 + 2").unwrap(), 3,);

        let value: String = c.eval_as("var x = 44; x.toString()").unwrap();
        assert_eq!(&value, "44");
    }

    // TODO: test for a better error once quickjs reports parse errors.
    // quickjs swallows the error in this case, sadly.
    #[test]
    fn test_eval_syntax_error() {
        let c = Context::new().unwrap();
        assert_eq!(
            c.eval(
                r#"
                !!!!
            "#
            ),
            Err(ExecutionError::Internal("Unknown Exception".into(),))
        );
    }

    // TODO: make this pass with the correct error.
    // quickjs swallows the error in this case, sadly.
    #[test]
    fn test_eval_exception() {
        let c = Context::new().unwrap();
        assert_eq!(
            c.eval(
                r#"
                function f() {
                    throw new Error("My Error");
                }
                f();
            "#
            ),
            // Err(ExecutionError::Exception(
            //     "My Error".into(),
            // ))
            Err(ExecutionError::Internal("Unknown Exception".into(),))
        );
    }

    #[test]
    fn test_call() {
        let c = Context::new().unwrap();

        assert_eq!(
            c.call_function("parseInt", vec!["22"]).unwrap(),
            JsValue::Int(22),
        );

        c.eval(
            r#"
            function add(a, b) {
                return a + b;
            }
        "#,
        )
        .unwrap();
        assert_eq!(
            c.call_function("add", vec![5, 7]).unwrap(),
            JsValue::Int(12),
        );

        c.eval(
            r#"
            function sumArray(arr) {
                let sum = 0;
                for (const value of arr) {
                    sum += value;
                }
                return sum;
            }
        "#,
        )
        .unwrap();
        assert_eq!(
            c.call_function("sumArray", vec![vec![1, 2, 3]]).unwrap(),
            JsValue::Int(6),
        );

        c.eval(
            r#"
            function addObject(obj) {
                let sum = 0;
                for (const key of Object.keys(obj)) {
                    sum += obj[key];
                }
                return sum;
            }
        "#,
        )
        .unwrap();
        let mut obj = std::collections::HashMap::<String, i32>::new();
        obj.insert("a".into(), 10);
        obj.insert("b".into(), 20);
        obj.insert("c".into(), 30);
        assert_eq!(
            c.call_function("addObject", vec![obj]).unwrap(),
            JsValue::Int(60),
        );
    }

    #[test]
    fn test_callback() {
        let c = Context::new().unwrap();

        c.add_callback("cb1", |flag: bool| !flag).unwrap();
        assert_eq!(c.eval("cb1(true)").unwrap(), JsValue::Bool(false),);

        c.add_callback("concat2", |a: String, b: String| format!("{}{}", a, b))
            .unwrap();
        assert_eq!(
            c.eval(r#"concat2("abc", "def")"#).unwrap(),
            JsValue::String("abcdef".into()),
        );

        c.add_callback("add2", |a: i32, b: i32| -> i32 { a + b })
            .unwrap();
        assert_eq!(c.eval("add2(5, 11)").unwrap(), JsValue::Int(16),);
    }

    #[test]
    fn test_callback_invalid_argcount() {
        let c = Context::new().unwrap();

        c.add_callback("cb", |a: i32, b: i32| a + b).unwrap();

        assert_eq!(
            c.eval(" cb(5) "),
            Err(ExecutionError::Exception(
                "Internal error: Invalid argument count: Expected 2, got 1".into()
            )),
        );
    }

}