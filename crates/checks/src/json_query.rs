//! Kuma's `evaluateJsonQuery`: a JSONata expression picks a value from the response, then a
//! second JSONata expression compares it with the expected value as strings.

use std::time::Duration;

use jsonata_core::evaluator::{Context, Evaluator, EvaluatorOptions};
use jsonata_core::parser;
use jsonata_core::value::JValue;
use serde_json::Value;

use crate::definition::{JsonPathOperator, JsonQueryCheck};

pub(crate) struct Comparison {
    pub passed: bool,
    pub value: String,
}

/// Rejects expressions JSONata cannot parse, so the importer can pause such monitors.
pub fn validate(json_path: &str) -> Result<(), String> {
    if json_path.is_empty() {
        return Ok(());
    }
    parser::parse(json_path)
        .map(drop)
        .map_err(|error| error.to_string())
}

/// Parses the body the way axios hands it to Kuma: JSON when it parses, the text otherwise.
pub(crate) fn response_value(body: &[u8]) -> Value {
    match serde_json::from_slice::<Value>(body) {
        // evaluateJsonQuery runs JSON.parse once more on string data.
        Ok(Value::String(text)) => serde_json::from_str(&text).unwrap_or(Value::String(text)),
        Ok(value) => value,
        Err(_) => Value::String(String::from_utf8_lossy(body).into_owned()),
    }
}

/// Runs on a blocking thread: JSONata evaluation is synchronous and user-controlled.
pub(crate) async fn compare(
    check: &JsonQueryCheck,
    data: Value,
    budget: Duration,
) -> Result<Comparison, String> {
    let json_path = check.json_path.clone();
    let operator = check.json_path_operator;
    let expected = check.expected_value.clone();
    tokio::task::spawn_blocking(move || evaluate(&json_path, operator, &expected, data, budget))
        .await
        .map_err(|error| format!("Error evaluating JSON query: {error}"))?
}

fn evaluate(
    json_path: &str,
    operator: JsonPathOperator,
    expected: &str,
    data: Value,
    budget: Duration,
) -> Result<Comparison, String> {
    let options = || EvaluatorOptions {
        timeout_ms: Some(u64::try_from(budget.as_millis()).unwrap_or(u64::MAX).max(1)),
        max_stack_depth: None,
        max_sequence_length: None,
    };
    let input = JValue::from(data);
    let selected = if json_path.is_empty() {
        input.clone()
    } else {
        let ast = parser::parse(json_path).map_err(|error| failure(&error.to_string(), &input))?;
        Evaluator::with_options(Context::new(), options())
            .evaluate(&ast, &input)
            .map_err(|error| failure(&error.to_string(), &input))?
    };
    let value = primitive(&selected).map_err(|reason| failure(&reason, &selected))?;

    let expression = match operator {
        JsonPathOperator::Less
        | JsonPathOperator::LessOrEqual
        | JsonPathOperator::Greater
        | JsonPathOperator::GreaterOrEqual => {
            format!("$number($.value) {operator} $number($.expected)")
        }
        JsonPathOperator::NotEquals => "$.value != $.expected".to_owned(),
        JsonPathOperator::Equals => "$.value = $.expected".to_owned(),
        JsonPathOperator::Contains => "$contains($.value, $.expected)".to_owned(),
    };
    let ast = parser::parse(&expression).map_err(|error| failure(&error.to_string(), &selected))?;
    let bindings = JValue::from(serde_json::json!({ "value": value, "expected": expected }));
    match Evaluator::with_options(Context::new(), options()).evaluate(&ast, &bindings) {
        Ok(JValue::Bool(passed)) => Ok(Comparison { passed, value }),
        Ok(_) => Err(failure(
            "Query evaluation returned undefined. Check query syntax and the structure of the response data",
            &selected,
        )),
        Err(error) => Err(failure(&error.to_string(), &selected)),
    }
}

/// The selected value as JavaScript's `toString` renders it.
fn primitive(value: &JValue) -> Result<String, String> {
    match value {
        JValue::Null | JValue::Undefined => {
            Err("Empty or undefined response. Check query syntax and response structure".to_owned())
        }
        JValue::String(text) => Ok(text.to_string()),
        JValue::Number(number) => Ok(number.to_string()),
        JValue::Bool(flag) => Ok(flag.to_string()),
        JValue::Array(_) => {
            let json = value.to_json_string().unwrap_or_default();
            let shown = match json.char_indices().nth(25) {
                Some((cut, _)) => format!("{}...]", &json[..cut]),
                None => json,
            };
            Err(format!(
                "JSON query returned the array {shown}, but a primitive value is required. Modify your query to return a single value via [0] to get the first element or use an aggregation like $count(), $sum() or $boolean()."
            ))
        }
        _ => Err("The post-JSON query evaluated response from the server is of type object, which cannot be directly compared to the expected value".to_owned()),
    }
}

fn failure(reason: &str, response: &JValue) -> String {
    let json = response.to_json_string().unwrap_or_default();
    // Kuma checks for 50 characters but keeps 100.
    let shown = if json.chars().count() > 50 {
        format!(
            "{}… (truncated)",
            json.chars().take(100).collect::<String>()
        )
    } else {
        json
    };
    format!("Error evaluating JSON query: {reason}. Response from server was: {shown}")
}
