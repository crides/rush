//! Convenience wrappers around parsing and evaluation.

use std::fs::File;
use std::io::{self, Read, Write, BufRead, BufReader, BufWriter};
use std::u8;

use conv::TryFrom;
use conv::misc::InvalidSentinel;

use super::eval::{Error as EvalError, Eval, Context, Invoke, Result as EvalResult, Value};
use super::eval::value::IntegerRepr;
use super::parse::parse;


/// Name of the variable within expression context that holds the current/input value.
const CURRENT: &'static str = "_";


/// Evaluate the expression within given Context.
/// Returns the resulting Value.
#[inline]
pub fn eval(expr: &str, context: &mut Context) -> io::Result<Value> {
    let ast = try!(parse_exprs(&[expr])).remove(0);
    ast.eval(context).map_err(|e| io::Error::new(io::ErrorKind::Other, e))
}

/// Execute the expression within given Context.
/// The result of the expression is discarded, but any side effects will persist in the Context.
#[inline]
pub fn exec(expr: &str, context: &mut Context) -> io::Result<()> {
    try!(eval(expr, context));
    Ok(())
}


// Single-expression processing.

/// Apply the expresion to a complete input stream, processed as single string,
/// writing to the given output stream.
#[inline]
pub fn apply_string<R: Read, W: Write>(expr: &str, input: R, output: &mut W) -> io::Result<()> {
    apply_string_multi(&[expr], input, output)
}

/// Apply the expression to given input taken as array of lines,
/// writing result to the given output stream.
#[inline]
pub fn apply_lines<R: Read, W: Write>(expr: &str, input: R, output: &mut W) -> io::Result<()> {
    apply_lines_multi(&[expr], input, output)
}

/// Apply the expression to given input stream, line by line,
/// writing to the given output stream.
#[inline]
pub fn map_lines<R: Read, W: Write>(expr: &str, input: R, output: &mut W) -> io::Result<()> {
    map_lines_multi(&[expr], input, output)
}

/// Apply the expression to given input stream, word by word,
/// (each word treated as string in the expression itself),
/// and writing to the given output stream.
#[inline]
pub fn map_words<R: Read, W: Write>(expr: &str, input: R, output: &mut W) -> io::Result<()> {
    map_words_multi(&[expr], input, output)
}

/// Apply the expression to given input stream, character by character
/// (treated as 1-character string in the expression itself),
/// and writing to the given output stream.
#[inline]
pub fn map_chars<R: Read, W: Write>(expr: &str, input: R, output: &mut W) -> io::Result<()> {
    map_chars_multi(&[expr], input, output)
}

/// Apply the expression to bytes of given input stream,
/// writing the transformed bytes into given output stream.
///
/// Note that the expression must always produce a byte (i.e. an integer from the 0-255 range).
#[inline]
pub fn map_bytes<R: Read, W: Write>(expr: &str, input: R, output: &mut W) -> io::Result<()> {
    map_bytes_multi(&[expr], input, output)
}

/// Apply the expression to the content of each file (as string)
/// whose path is given as a line of the input stream.
/// Write the results as lines to the output stream.
#[inline]
pub fn map_files<R: Read, W: Write>(expr: &str, input: R, output: &mut W) -> io::Result<()> {
    map_files_multi(&[expr], input, output)
}


// Multi-expression processing.

/// Apply a sequence of expressions to the input stream taken as single string.
///
/// The stream is provided as a single string to the first expression,
/// whose result is then passed to the second one, etc.
///
/// The final result is written to the given output stream.
#[inline]
pub fn apply_string_multi<R: Read, W: Write>(exprs: &[&str], input: R, output: &mut W) -> io::Result<()> {
    let mut context = Context::new();
    apply_string_multi_ctx(&mut context, exprs, input, output)
}

/// Apply a sequence of expressions to the input stream taken as an array of lines
///
/// The stream is provided as an array of strings to the first expression,
/// whose result is then passed to the second one, etc.
///
/// The final result is written to the given output stream.
#[inline]
pub fn apply_lines_multi<R: Read, W: Write>(exprs: &[&str], input: R, output: &mut W) -> io::Result<()> {
    let mut context = Context::new();
    apply_lines_multi_ctx(&mut context, exprs, input, output)
}

/// Apply a sequence of expressions to the input stream, line by line.
///
/// Every line read from the stream is fed to the first expression
/// (without the \n char) whose result is then passed to the second one, etc.
///
/// The final result is written then to the given output stream.
/// This continues for each line of input.
#[inline]
pub fn map_lines_multi<R: Read, W: Write>(exprs: &[&str], input: R, output: &mut W) -> io::Result<()> {
    let mut context = Context::new();
    map_lines_multi_ctx(&mut context, exprs, input, output)
}

/// Apply a sequence of expressions to the input stream, word by word.
///
/// Every word read from the stream is fed to the first expression,
/// whose result is then passed to the second one, etc.
///
/// The final result is written then to the given output stream.
/// This continues for each word of input.
#[inline]
pub fn map_words_multi<R: Read, W: Write>(exprs: &[&str], input: R, output: &mut W) -> io::Result<()> {
    let mut context = Context::new();
    map_words_multi_ctx(&mut context, exprs, input, output)
}

/// Apply a sequence of expressions to the input stream, character by character.
///
/// Every character read from the stream is fed to the first expression
/// (as 1-character string), whose result is then passed to the second one, etc.
///
/// The final result is written then to the given output stream.
/// This continues for each character of input.
#[inline]
pub fn map_chars_multi<R: Read, W: Write>(exprs: &[&str], input: R, output: &mut W) -> io::Result<()> {
    let mut context = Context::new();
    map_chars_multi_ctx(&mut context, exprs, input, output)
}

/// Apply a sequence of expressions to the input stream, byte by byte.
///
/// Every byte read from the stream is fed to the first expression
/// (as an integer from 0-255 range), whose result is then passed to the second one, etc.
///
/// The final result -- which has to be a 0-255 integer -- is written then
/// to the given output stream. This continues for each byte of input.
#[inline]
pub fn map_bytes_multi<R: Read, W: Write>(exprs: &[&str], input: R, output: &mut W) -> io::Result<()> {
    let mut context = Context::new();
    map_bytes_multi_ctx(&mut context, exprs, input, output)
}

/// Apply the expressions to the content of each file (as string)
/// whose path is given as a line of the input stream.
///
/// Every line read from the stream is interpreted as file path.
/// The corresponding file is read, and its content is fed to the first expression,
/// whose result is then passed to the second one, etc.
//
/// The final result is written then to the given output stream.
/// This continues for each line (file path) of input.
#[inline]
pub fn map_files_multi<R: Read, W: Write>(exprs: &[&str], input: R, output: &mut W) -> io::Result<()> {
    let mut context = Context::new();
    map_files_multi_ctx(&mut context, exprs, input, output)
}

// Multi-expression processing with shared context.

/// Apply a sequence of expressions to the input stream taken as single string.
///
/// The stream is provided as a single string to the first expression,
/// whose result is then passed to the second one, etc.
/// Expression context is shared throughout.
///
/// The final result is written to the given output stream.
pub fn apply_string_multi_ctx<R, W>(context: &mut Context,
                                    exprs: &[&str],
                                    input: R, output: &mut W) -> io::Result<()>
    where R: Read, W: Write
{
    let asts = try!(parse_exprs(exprs));
    let expr_count = asts.len();

    let mut reader = BufReader::new(input);
    let mut input = String::new();
    let byte_count = try!(reader.read_to_string(&mut input));
    let char_count = input.chars().count();

    context.set(CURRENT, Value::String(input));

    let result = try!(process(context, &asts));
    try!(write_result_line(output, result));

    Ok(())
}

/// Apply a sequence of expressions to the input stream taken as an array of lines.
///
/// The stream is provided as an array of strings to the first expression,
/// whose result is then passed to the second one, etc.
/// Expression context is shared throughout.
///
/// The final result is written to the given output stream.
#[allow(or_fun_call)]
pub fn apply_lines_multi_ctx<R, W>(context: &mut Context,
                                   exprs: &[&str],
                                   input: R, output: &mut W) -> io::Result<()>
    where R: Read, W: Write
{
    let asts = try!(parse_exprs(exprs));
    let expr_count = asts.len();

    // parse input lines into a vector of Value objects
    let lines: Vec<_> = BufReader::new(input).lines()
        .map(|r| r.expect("failed to read input line")
            .parse::<Value>().unwrap_or(Value::invalid_sentinel()))
        .filter(|v| *v != Value::invalid_sentinel())
        .collect();
    let line_count = lines.len();

    context.set(CURRENT, Value::Array(lines));

    let result = try!(process(context, &asts));
    try!(write_result_line(output, result));

    Ok(())
}

/// Apply a sequence of expressions to the input stream, line by line.
///
/// Every line read from the stream is fed to the first expression
/// (without the \n char) whose result is then passed to the second one, etc.
/// Expression context is shared throughout.
///
/// The final result is written then to the given output stream.
/// This continues for each line of input.
pub fn map_lines_multi_ctx<R, W>(context: &mut Context,
                                 exprs: &[&str],
                                 input: R, output: &mut W) -> io::Result<()>
    where R: Read, W: Write
{
    let asts = try!(parse_exprs(exprs));
    let expr_count = asts.len();

    let reader = BufReader::new(input);
    let mut writer = BufWriter::new(output);

    let mut line_count = 0;
    for line in reader.lines() {
        let line = try!(line);
        context.set(CURRENT, to_value(line));

        let result = try!(process(context, &asts));
        try!(write_result_line(&mut writer, result));

        line_count += 1;
    }

    Ok(())
}

/// Apply a sequence of expressions to the input stream, word by word.
///
/// Every word read from the stream is fed to the first expression,
/// whose result is then passed to the second one, etc.
/// Expression context is shared throughout.
///
/// The final result is written then to the given output stream.
/// This continues for each word of input.
pub fn map_words_multi_ctx<R, W>(context: &mut Context,
                                 exprs: &[&str],
                                 input: R, output: &mut W) -> io::Result<()>
    where R: Read, W: Write
{
    let asts = try!(parse_exprs(exprs));
    let expr_count = asts.len();

    let reader = BufReader::new(input);
    let mut writer = BufWriter::new(output);

    let mut word_count = 0;
    {
        // Note that `writer` is taken as a parameter rather than just being
        // captured by the closure, because we need to refer to it mutably
        // in the main loop below.
        let mut maybe_process_word = |word: &mut String,
                                      writer: &mut Write| -> io::Result<()> {
            if word.is_empty() {
                return Ok(());
            }

            context.set(CURRENT, to_value(word.clone()));
            let result = try!(process(context, &asts));

            let retval = try!(String::try_from(result)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)));
            try!(write!(writer, "{}", retval));

            word_count += 1;
            word.clear();
            Ok(())
        };

        let mut word = String::new();
        for line in reader.lines() {
            let line = try!(line);
            for ch in line.chars() {
                // Whitespace characters denote word's end, but they are to be
                // preserved verbatim in the final output.
                if ch.is_whitespace() {
                    try!(maybe_process_word(&mut word, &mut writer));
                    try!(write!(writer, "{}", ch));
                } else {
                    word.push(ch);
                }
            }
            try!(maybe_process_word(&mut word, &mut writer));
        }
    }

    Ok(())
}

/// Apply a sequence of expressions to the input stream, character by character.
///
/// Every character read from the stream is fed to the first expression
/// (as 1-character string), whose result is then passed to the second one, etc.
/// Expression context is shared throughout.
///
/// The final result is written then to the given output stream.
/// This continues for each character of input.
pub fn map_chars_multi_ctx<R, W>(context: &mut Context,
                                 exprs: &[&str],
                                 input: R, output: &mut W) -> io::Result<()>
    where R: Read, W: Write
{
    let asts = try!(parse_exprs(exprs));
    let expr_count = asts.len();

    let reader = BufReader::new(input);
    let mut writer = BufWriter::new(output);

    let mut char_count = 0;
    {
        let mut process_char = |ch: char| -> io::Result<()> {
            context.set(CURRENT, Value::from(ch));

            // TODO(xion): consider enforcing for the final result to also be 1-char string
            // and writing those characters as a contiguous string
            let result = try!(process(context, &asts));
            try!(write_result_line(&mut writer, &result));

            char_count += 1;
            Ok(())
        };

        // TODO(xion): rather than reading the input line by line,
        // use Read::chars() when the feature is stable (same in map_words_multi_ctx)
        let mut first_line = true;
        for line in reader.lines() {
            if !first_line {
                // TODO(xion): cross-platfrorm line ending
                try!(process_char('\n'));
            }
            let line = try!(line);
            for ch in line.chars() {
                try!(process_char(ch));
            }
            first_line = false;
        }
    }

    Ok(())
}

/// Apply a sequence of expressions to the input stream, byte by byte.
///
/// Every byte read from the stream is fed to the first expression
/// (as an integer from 0-255 range), whose result is then passed to the second one, etc.
/// Expression context is shared throughout.
///
/// The final result -- which has to be a 0-255 integer -- is written then
/// to the given output stream. This continues for each byte of input.
pub fn map_bytes_multi_ctx<R, W>(context: &mut Context,
                                 exprs: &[&str],
                                 input: R, output: &mut W) -> io::Result<()>
    where R: Read, W: Write
{
    let asts = try!(parse_exprs(exprs));
    let expr_count = asts.len();

    // we will be handling individual bytes, but buffering can still be helpful
    // if the underlying reader/writer is something slow like a disk or network
    let reader = BufReader::new(input);
    let mut writer = BufWriter::new(output);

    let mut byte_count = 0;
    for byte in reader.bytes() {
        let byte = try!(byte);
        context.set(CURRENT, Value::from(byte));

        let result = try!(process(context, &asts));
        match *result {
            Value::Integer(i) if 0 <= i && i < u8::MAX as IntegerRepr => {
                try!(writer.write_all(&[i as u8]))
            },
            _ => return Err(io::Error::new(io::ErrorKind::InvalidData,
                format!("expected a byte-sized integer, got {}", result))),
        }

        byte_count += 1;
    }

    Ok(())
}

/// Apply the expressions to the content of each file (as string)
/// whose path is given as a line of the input stream.
///
/// Every line read from the stream is interpreted as file path.
/// The corresponding file is read, and its content is fed to the first expression,
/// whose result is then passed to the second one, etc.
/// Expression context is shared throughout.
//
/// The final result is written then to the given output stream.
/// This continues for each line (file path) of input.
pub fn map_files_multi_ctx<R, W>(context: &mut Context,
                                exprs: &[&str],
                                input: R, output: &mut W) -> io::Result<()>
    where R: Read, W: Write
{
    let asts = try!(parse_exprs(exprs));
    let expr_count = asts.len();

    let reader = BufReader::new(input);
    let mut writer = BufWriter::new(output);

    let mut file_count = 0;
    let mut total_byte_count = 0;
    for line in reader.lines() {
        let path = try!(line).trim().to_owned();

        // we try to use the file size to preallocate the string
        // which we'll read the content of the file to
        let mut file = try!(File::open(path));
        let mut content = match file.metadata() {
            Ok(metadata) => String::with_capacity(metadata.len() as usize),
            _ => String::new(),
        };
        let byte_count = try!(file.read_to_string(&mut content));

        context.set(CURRENT, Value::String(content));
        let result = try!(process(context, &asts));
        try!(write_result_line(&mut writer, result));

        file_count += 1;
        total_byte_count += byte_count;
    }

    Ok(())
}

// Utility functions.

fn parse_exprs(exprs: &[&str]) -> io::Result<Vec<Box<Eval>>> {
    let mut result = Vec::new();
    for expr in exprs {
        let ast = try!(parse(expr)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e)));
        result.push(ast);
    }
    Ok(result)
}

fn to_value(input: String) -> Value {
    input.parse::<Value>().unwrap_or_else(|_| Value::String(input))
}

fn process<'c>(context: &'c mut Context, exprs: &[Box<Eval>]) -> io::Result<&'c Value> {
    for ast in exprs {
        let result = try!(evaluate(ast, context));
        context.set(CURRENT, result);
    }
    Ok(context.get(CURRENT).unwrap())
}

fn evaluate<'c>(ast: &Box<Eval>, context: &'c mut Context) -> io::Result<Value> {
    ast.eval(context)
        .and_then(|result| maybe_apply_result(result, context))
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
}

fn maybe_apply_result(result: Value, context: &mut Context) -> EvalResult {
    // result might be a function, in which case we will try to apply to original input
    if let Value::Function(func) = result {
        if func.arity() != 1 {
            return Err(EvalError::new(&format!(
                "output must be an immediate value or a 1-argument function \
                (got {}-argument one)", func.arity())));
        }
        let input = context.unset_here(CURRENT).unwrap();
        return func.invoke1(input, context);
    }
    Ok(result)
}

fn write_result_line<W: Write>(output: &mut W, result: &Value) -> io::Result<()> {
    let result = try!(String::try_from(result)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)));
    write!(output, "{}\n", result)
}
