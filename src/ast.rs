//! Data structures representing the abstract syntax tree (AST)
//! of parsed expressions.

use std::str::FromStr;

use eval::{self, Eval, Context, Value};


pub struct ValueNode {
    pub value: Value,
}

impl FromStr for ValueNode {
    type Err = <Value as FromStr>::Err;

    fn from_str(s: &str) -> Result<ValueNode, Self::Err> {
        s.parse::<Value>().map(|v| ValueNode{value: v})
    }
}

impl Eval for ValueNode {
    fn eval(&self, context: &Context) -> Result<Value, eval::Error> {
        if let Value::String(ref string) = self.value {
            // treat the literal value as variable name if such variable exists;
            // otherwise, just return the value itself as string
            if let Some(value) = context.get_var(string) {
                return Ok(value.clone());
            }
            return Ok(Value::String(string.clone()));
        }
        Ok(self.value.clone())
    }
}


pub struct BinaryOpNode {
    pub op: String,  // TODO(xion): enum?
    pub left: Box<Eval>,
    pub right: Box<Eval>,
}

impl Eval for BinaryOpNode {
    fn eval(&self, context: &Context) -> Result<Value, eval::Error> {
        match &self.op[..] {
            "+" => {
                let left = try!(self.left.eval(&context));
                let right = try!(self.right.eval(&context));

                if let Value::String(left) = left {
                    if let Value::String(right) = right {
                        return Ok(Value::String(left + &right));
                    }
                }
                // TODO(xion): adding numbers
                eval::Error::err("invalid types for + operator")
            }
            // TODO(xion): other operators
            _ => eval::Error::err(&format!("unknown operator: {}", self.op))
        }
    }
}