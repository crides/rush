//! Module implementing evaluaton of binary operator AST nodes.

use std::cell::RefCell;
use std::iter;
use std::mem;

use eval::{self, api, Eval, Context, Value};
use eval::model::Invoke;
use eval::model::value::{ArrayRepr, FloatRepr, IntegerRepr, StringRepr};
use eval::util::cmp::{TryEq, TryOrd};
use parse::ast::{Associativity, BinaryOpNode, ScalarNode};


/// State of a short-circuited operation.
#[derive(Debug,PartialEq)]
enum Shortcircuit {
    /// The operation has determined its result
    /// and no further computation is necessary.
    Break,
    /// The result of the operation may change,
    /// so further terms need to be evaluated.
    Continue,
}

/// Evaluation result that involves short-circuiting.
type ScEvalResult = Result<(Value, Shortcircuit), eval::Error>;


/// Lazy wrapper around Value.
///
/// Used as an argument to the short-circuiting operators, allowing them to
/// take both immediate values and Evals.
///
/// WARNING: This type says it implements Eval, but it's actually more like EvalOnce.
/// If Eval::eval is invoked for it the second time, it will panic.
#[derive(Debug)]
struct LazyValue(RefCell<Option<Value>>);

impl From<Value> for LazyValue {
    fn from(value: Value) -> Self { LazyValue(RefCell::new(Some(value))) }
}

impl Eval for LazyValue {
    fn eval(&self, _: &mut Context) -> eval::Result {
        let maybe_value = mem::replace(&mut *self.0.borrow_mut(), None);
        match maybe_value {
            Some(value) => Ok(value),
            None => panic!("<LazyValue as Eval>::eval invoked more than once!")
        }
    }
}


// Top-level impl block for BinaryOpNode code in this module.
impl Eval for BinaryOpNode {
    #[inline]
    fn eval(&self, context: &mut Context) -> eval::Result {
        match self.assoc {
            Associativity::Left => self.eval_left_assoc(context),
            Associativity::Right => self.eval_right_assoc(context),
        }
    }
}

// Public interface for use by other nodes' evaluation code.
impl BinaryOpNode {
    pub fn eval_op(op: &str, left: Value, right: Value, context: &Context) -> eval::Result {
        // These short-circuited operators have to be considered here as well
        // because eval_right_assoc() and CurriedBinaryOpNode::eval() relies on this.
        if BinaryOpNode::is_shortcircuit_op(op) {
            let left = Box::new(LazyValue::from(left)) as Box<Eval>;
            let right = Box::new(LazyValue::from(right)) as Box<Eval>;
            let (value, _) = try!(BinaryOpNode::eval_shortcircuit_op(op, &left, &right, &context));
            return Ok(value);
        }

        match op {
            "<" => BinaryOpNode::eval_lt(left, right),
            "<=" => BinaryOpNode::eval_le(left, right),
            ">" => BinaryOpNode::eval_gt(left, right),
            ">=" => BinaryOpNode::eval_ge(left, right),
            "==" => BinaryOpNode::eval_eq(left, right),
            "!=" => BinaryOpNode::eval_ne(left, right),
            "@" => BinaryOpNode::eval_at(left, right),
            "&" => BinaryOpNode::eval_amp(left, right),
            "$" => BinaryOpNode::eval_dollar(left, right, &context),
            "+" => BinaryOpNode::eval_plus(left, right),
            "-" => BinaryOpNode::eval_minus(left, right),
            "*" => BinaryOpNode::eval_times(left, right),
            "/" => BinaryOpNode::eval_by(left, right),
            "%" => BinaryOpNode::eval_modulo(left, right),
            "**" => BinaryOpNode::eval_power(left, right),
            _ => Err(eval::Error::new(&format!("unknown binary operator: `{}`", op))),
        }
    }
}

// Private implementation methods for evaluating binary operators.
impl BinaryOpNode {
    fn eval_left_assoc(&self, context: &mut Context) -> eval::Result {
        let mut result = try!(self.first.eval(context));
        for &(ref op, ref arg) in &self.rest {
            // allow for terminating evaluation of short-circuiting operators early
            if BinaryOpNode::is_shortcircuit_op(&op[..]) {
                let left = Box::new(LazyValue::from(result)) as Box<Eval>;
                let (res, sc) = try!(
                    BinaryOpNode::eval_shortcircuit_op(&op[..], &left, arg, context));
                result = res;
                if sc == Shortcircuit::Break {
                    break;
                }
            } else {
                let arg = try!(arg.eval(context));
                result = try!(BinaryOpNode::eval_op(&op[..], result, arg, &context));
            }
        }
        Ok(result)
    }

    fn eval_right_assoc(&self, context: &mut Context) -> eval::Result {
        if self.rest.is_empty() {
            return self.first.eval(context);
        }

        // evaluate the terms in reverse order; since the AST is tailored
        // towards left-associative operators, it is slightly awkward
        // as it always leaves an operation waiting for the next term
        let mut rest = self.rest.iter().rev();

        // initialize with the "last" term
        let &(ref op, ref arg) = rest.next().unwrap();
        let mut op = op;
        let mut result = try!(arg.eval(context));

        // go through the remaining terms
        // (note how current `result` is always the second arg for an operator)
        for &(ref next_op, ref arg) in rest {
            if BinaryOpNode::is_assignment_op(&op[..]) {
                let arg = try!(BinaryOpNode::resolve_assignment_lhs(arg, context));
                result = try!(BinaryOpNode::eval_assignment_op(&op[..], arg, result, context));
                continue;
            }

            // allow for terminating evaluation of short-circuiting operators early
            if BinaryOpNode::is_shortcircuit_op(&op[..]) {
                let right = Box::new(LazyValue::from(result)) as Box<Eval>;
                let (res, sc) = try!(
                    BinaryOpNode::eval_shortcircuit_op(&op[..], arg, &right, context));
                result = res;
                if sc == Shortcircuit::Break {
                    return Ok(result);
                }
            } else {
                let arg = try!(arg.eval(context));
                result = try!(BinaryOpNode::eval_op(&op[..], arg, result, &context));
            }
            op = next_op;
        }

        // finish by processing the "first" term
        if BinaryOpNode::is_assignment_op(&op[..]) {
            let last = try!(BinaryOpNode::resolve_assignment_lhs(&self.first, context));
            BinaryOpNode::eval_assignment_op(&op[..], last, result, context)
        } else {
            let last = try!(self.first.eval(context));
            BinaryOpNode::eval_op(&op[..], last, result, &context)
        }
    }

    #[inline]
    fn is_assignment_op(op: &str) -> bool {
        ["="].contains(&op)
    }

    /// Resolve the left-hand side of an assignment operator.
    fn resolve_assignment_lhs(arg: &Box<Eval>, context: &mut Context) -> eval::Result {
        // Hack: check if the argument is a scalar AST node.
        // If so, extract the value from it directly, w/o going through
        // Context::resolve so that Value::Symbol will be preserved.
        arg.downcast_ref::<ScalarNode>()
            .map_or_else(|| arg.eval(context), |s| Ok(s.value.clone()))
    }

    fn eval_assignment_op(op: &str, lhs: Value, rhs: Value, context: &mut Context) -> eval::Result {
        match op {
            // TODO(xion): consider supporting compound assignments
            "=" => BinaryOpNode::eval_let(lhs, rhs, context),
            _ => panic!("not an assignment operator: {}", op),
        }
    }

    #[inline]
    fn is_shortcircuit_op(op: &str) -> bool {
        ["&&", "||"].contains(&op)
    }

    /// Evaluate a short-circuiting operator.
    ///
    /// The result contains both the actual Value of the operation
    /// as well as the short-circuiting hint (whether to continue or stop further evaluation).
    fn eval_shortcircuit_op(op: &str,
                            left: &Box<Eval>, right: &Box<Eval>,
                            context: &Context) -> ScEvalResult {
        // XXX: We really don't want BinaryOpNode::eval_op to take &mut Context,
        // so we introduce "mutability" here by adding a local Context.
        //
        // This of course means that any assignments that happen to this context
        // cannot be propagated upwards. This prohibits expressions such as:
        //    foo && (bar = 42)
        // but they wouldn't evaluate anyway (assignment returns nil).
        //
        // But of course, there are some tricks (or custom functions)
        // that allow to perform assignment AND return a non-nil value,
        // so we'll check for any mutations in the temporary context and error-out.
        let mut context = Context::with_parent(context);
        let result = match op {
            "&&" => try!(BinaryOpNode::eval_and(left, right, &mut context)),
            "||" => try!(BinaryOpNode::eval_or(left, right, &mut context)),
            _ => panic!("not a short-circuiting operator: {}", op),
        };
        if !context.is_empty() {
            return Err(eval::Error::other("assignments are not supported in this context"));
        }
        Ok(result)
    }
}

/// Assignment operators.
impl BinaryOpNode {
    /// Evaluate the "=" operator.
    fn eval_let(left: Value, right: Value, context: &mut Context) -> eval::Result {
        if let Value::Symbol(ref name) = left {
            context.set(name, right);
            return Ok(Value::Empty);
        }
        // TODO: allow assignment to array elements and object values;
        // this would require introduction of a "reference value" that's a generalization of Symbol
        // and can point to variables (like Symbol), array/object elements, and array slices
        BinaryOpNode::err("=", left, right)
    }
}

// Logical operators.
// Note that these operators can short-circuit, which is why they don't take Values.
impl BinaryOpNode {
    /// Evaluate the "&&" operator for two values.
    #[inline]
    fn eval_and(left: &Box<Eval>, right: &Box<Eval>,
                context: &mut Context) -> ScEvalResult {
        let left = try!(left.eval(context));
        let is_true = try!(api::conv::bool(left.clone())).unwrap_bool();
        if is_true {
            let right = try!(right.eval(context));
            Ok((right, Shortcircuit::Continue))
        } else {
            Ok((left, Shortcircuit::Break))
        }
    }

    /// Evaluate the "||" operator for two values.
    #[inline]
    fn eval_or(left: &Box<Eval>, right: &Box<Eval>,
               context: &mut Context) -> ScEvalResult {
        let left = try!(left.eval(context));
        let is_true = try!(api::conv::bool(left.clone())).unwrap_bool();
        if is_true {
            Ok((left, Shortcircuit::Break))
        } else {
            let right = try!(right.eval(context));
            Ok((right, Shortcircuit::Continue))
        }
    }
}

// Comparison operators.
impl BinaryOpNode {
    /// Evaluate the "<" operator for two values.
    fn eval_lt(left: Value, right: Value) -> eval::Result {
        left.try_lt(&right).map(Value::Boolean)
    }

    /// Evaluate the "<=" operator for two values.
    fn eval_le(left: Value, right: Value) -> eval::Result {
        left.try_le(&right).map(Value::Boolean)
    }

    /// Evaluate the ">" operator for two values.
    fn eval_gt(left: Value, right: Value) -> eval::Result {
        left.try_gt(&right).map(Value::Boolean)
    }

    /// Evaluate the ">=" operator for two values.
    fn eval_ge(left: Value, right: Value) -> eval::Result {
        left.try_ge(&right).map(Value::Boolean)
    }

    /// Evaluate the "==" operator for two values.
    fn eval_eq(left: Value, right: Value) -> eval::Result {
        left.try_eq(&right).map(Value::Boolean)
    }

    /// Evaluate the "!=" operator for two values.
    fn eval_ne(left: Value, right: Value) -> eval::Result {
        left.try_ne(&right).map(Value::Boolean)
    }

    /// Evaluate the "@" operator for two values.
    fn eval_at(left: Value, right: Value) -> eval::Result {
        // value @ array is a membership test
        if let Value::Array(ref a) = right {
            return Ok(Value::Boolean(a.contains(&left)));
        }

        // string @ regex is a match attempt
        eval2!((left: &String, right: &Regex) -> Boolean { right.is_match(left) });

        BinaryOpNode::err("@", left, right)
    }
}

// Functional operators.
impl BinaryOpNode {
    /// Evaluate the "&" operator for two values.
    fn eval_amp(left: Value, right: Value) -> eval::Result {
        if left.is_function() && right.is_function() {
            let left = left.unwrap_function();
            let right = right.unwrap_function();
            return right.compose_with(left)  // reverse order!
                .map(Value::Function)
                .ok_or_else(|| eval::Error::new(
                    "second argument of `&` must be a unary function"
                ));
        }
        BinaryOpNode::err("&", left, right)
    }

    /// Evaluate the "$" operator for two values.
    fn eval_dollar(left: Value, right: Value, context: &Context) -> eval::Result {
        if left.is_function() {
            let left = left.unwrap_function();
            return if left.arity() == 1 {
                left.invoke1(right, &context)
            } else {
                left.curry(right)
                    .map(Value::Function)
                    .ok_or_else(|| eval::Error::new(
                        "left side of `$` must be a function taking at least one argument"
                    ))
            };
        }
        BinaryOpNode::err("$", left, right)
    }
}

/// Arithmetic operators.
impl BinaryOpNode {
    /// Evaluate the "+" operator for two values.
    fn eval_plus(left: Value, right: Value) -> eval::Result {
        eval2!(left, right : &String { left.clone() + &*right });
        eval2!(left, right : Integer { left + right });
        eval2!(left, right : Float { left + right });
        eval2!((left: Integer, right: Float) -> Float { left as FloatRepr + right });
        eval2!((left: Float, right: Integer) -> Float { left + right as FloatRepr });

        eval2!((left: &Array, right: &Array) -> Array {{
            let mut left = left.clone();
            let mut right = right.clone();
            left.append(&mut right);
            left
        }});
        eval2!((left: &Object, right: &Object) -> Object {{
            let mut left = left.clone();
            for (k, v) in right {
                left.insert(k.to_owned(), v.clone());
            }
            left
        }});

        BinaryOpNode::err("+", left, right)
    }

    /// Evaluate the "-" operator for two values.
    fn eval_minus(left: Value, right: Value) -> eval::Result {
        eval2!(left, right : Integer { left - right });
        eval2!(left, right : Float { left - right });
        eval2!((left: Integer, right: Float) -> Float { left as FloatRepr - right });
        eval2!((left: Float, right: Integer) -> Float { left - right as FloatRepr });
        BinaryOpNode::err("-", left, right)
    }

    /// Evaluate the "*" operator for two values.
    fn eval_times(left: Value, right: Value) -> eval::Result {
        eval2!(left, right : Integer { left * right });
        eval2!(left, right : Float { left * right });

        // multiplying string/array by a number is repeating (like in Python)
        eval2!((left: &String, right: Integer) -> String where (right > 0) {
            iter::repeat(left).map(StringRepr::clone).take(right as usize).collect()
        });
        eval2!((left: &Array, right: Integer) -> Array where (right > 0) {{
            iter::repeat(left).map(ArrayRepr::clone).take((right - 1) as usize)
                .fold(left.clone(), |mut res, mut next| { res.append(&mut next); res })
        }});

        // "multiplying" array by string means a join, with string as separator
        if left.is_array() && right.is_string() {
            return api::strings::join(left, right);
        }

        // "multiplying" functions is composition
        if left.is_function() && right.is_function() {
            let left = left.unwrap_function();
            let right = right.unwrap_function();
            return left.compose_with(right)
                .map(Value::Function)
                .ok_or_else(|| eval::Error::new(
                    "left side of function composition must be unary"
                ));
        }

        BinaryOpNode::err("*", left, right)
    }

    /// Evaluate the "/" operator for two values.
    fn eval_by(left: Value, right: Value) -> eval::Result {
        eval2!(left, right : Integer { left / right });
        eval2!(left, right : Float { left / right });
        eval2!((left: Integer, right: Float) -> Float { left as FloatRepr / right });
        eval2!((left: Float, right: Integer) -> Float { left / right as FloatRepr });

        // "dividing" string by string or regex is a shorthand for split()
        if left.is_string() && (right.is_string() || right.is_regex()) {
            return api::strings::split(right, left);  // split(delim, string)
        }

        BinaryOpNode::err("/", left, right)
    }

    /// Evaluate the "%" operator for two values.
    fn eval_modulo(left: Value, right: Value) -> eval::Result {
        // modulo/remainder
        eval2!(left, right : Integer { left % right });
        eval2!(left, right : Float { left % right });
        eval2!((left: Integer, right: Float) -> Float {
            (left as FloatRepr) % right
        });
        eval2!((left: Float, right: Integer) -> Float {
            left % (right as FloatRepr)
        });

        // string formatting (for just one argument (but it can be an array))
        if left.is_string() {
            return api::strings::format_(left, right);
        }

        BinaryOpNode::err("%", left, right)
    }

    /// Evaluate the "**" operator for two values.
    fn eval_power(left: Value, right: Value) -> eval::Result {
        if let (&Value::Integer(l), &Value::Integer(r)) = (&left, &right) {
            if r < 0 {
                // x**(-y) is equivalent to 1/x**y
                let base = l as FloatRepr;
                let exp = -r as FloatRepr;
                return Ok(Value::Float(1.0 / base.powf(exp)));
            }
            if r < (u32::max_value() as IntegerRepr) {
                return Ok(Value::Integer(l.pow(r as u32)));
            }
            return Err(eval::Error::new(&format!("exponent out of range: {}", r)));
        }

        eval2!(left, right : Float { left.powf(right) });

        eval2!((left: Integer, right: Float) -> Float {
            (left as FloatRepr).powf(right)
        });
        eval2!((left: Float, right: Integer) -> Float {{
            if right > (i32::max_value() as IntegerRepr) {
                return Err(eval::Error::new(&format!(
                    "exponent out of range: {}", right
                )));
            }
            left.powi(right as i32)
        }});

        BinaryOpNode::err("**", left, right)
    }
}

// Utility functions.
impl BinaryOpNode {
    /// Produce an error about invalid arguments for an operator.
    #[inline]
    fn err(op: &str, left: Value, right: Value) -> eval::Result {
        Err(eval::Error::invalid(op, vec![&left, &right]))
    }
}
