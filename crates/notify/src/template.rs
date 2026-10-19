//! Kuma's Liquid templates (`notification-provider.js:75-112`), rendered with the `liquid` crate.
//!
//! Kuma uses liquidjs 10.26.0. [`crate::js::Js`] gives values liquidjs semantics: missing keys
//! and keys on `null` print nothing, objects print `[object Object]`, arrays print their items
//! joined without a separator, and only `false` and `null` are falsy. The `json` and `jsonify`
//! filters are added because webhook bodies commonly use them. Other liquidjs-only filters and
//! tags (for example `where_exp`, `group_by`, `slugify`, `base64_encode`, `date_to_xmlschema`,
//! `echo`, `liquid`) are unknown here and fail at parse time, which fails the send permanently.
//! `include` and `render` fail when rendered, as in Kuma, which has no template files.
//! Inside `for` loops and after `assign`, values are liquid's own and lookups of missing keys
//! fail instead of printing nothing.

// liquid's runtime traits are written against kstring's deprecated `KStringCow`.
#![allow(deprecated)]

use std::collections::BTreeSet;
use std::fmt;
use std::sync::LazyLock;

use liquid_core::model::{KStringCow, KStringRef, ScalarCow, Value, ValueCow, ValueView};
use liquid_core::parser::{
    FilterArguments, FilterReflection, Language, ParameterReflection, ParseBlock, ParseFilter,
    ParseTag,
};
use liquid_core::runtime::{
    Expression, GlobalFrame, PartialStore, Registers, Renderable, Runtime, RuntimeBuilder, Template,
};
use liquid_core::{Filter, Result as LiquidResult};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};

use crate::error::SendError;
use crate::event::extract_address;
use crate::js::{Js, NULL};

/// The tags, blocks and filters of `liquid`'s stdlib, plus `json` and `jsonify`.
static LANGUAGE: LazyLock<Language> = LazyLock::new(|| {
    use liquid_lib::stdlib::*;
    let mut language = Language::empty();
    let tags: [Box<dyn ParseTag>; 8] = [
        Box::new(AssignTag),
        Box::new(BreakTag),
        Box::new(ContinueTag),
        Box::new(CycleTag),
        Box::new(IncludeTag),
        Box::new(IncrementTag),
        Box::new(DecrementTag),
        Box::new(RenderTag),
    ];
    for tag in tags {
        language
            .tags
            .register(tag.reflection().tag().to_owned(), tag);
    }
    let blocks: [Box<dyn ParseBlock>; 9] = [
        Box::new(RawBlock),
        Box::new(IfBlock),
        Box::new(UnlessBlock),
        Box::new(IfChangedBlock),
        Box::new(ForBlock),
        Box::new(TableRowBlock),
        Box::new(CommentBlock),
        Box::new(CaptureBlock),
        Box::new(CaseBlock),
    ];
    for block in blocks {
        let name = block.reflection().start_tag().to_owned();
        language.blocks.register(name, block);
    }
    let filters: [Box<dyn ParseFilter>; 50] = [
        Box::new(Abs),
        Box::new(Append),
        Box::new(AtLeast),
        Box::new(AtMost),
        Box::new(Capitalize),
        Box::new(Ceil),
        Box::new(Compact),
        Box::new(Concat),
        Box::new(Date),
        Box::new(Default),
        Box::new(DividedBy),
        Box::new(Downcase),
        Box::new(Escape),
        Box::new(EscapeOnce),
        Box::new(First),
        Box::new(Floor),
        Box::new(Join),
        Box::new(Last),
        Box::new(Lstrip),
        Box::new(Map),
        Box::new(Minus),
        Box::new(Modulo),
        Box::new(NewlineToBr),
        Box::new(Plus),
        Box::new(Prepend),
        Box::new(Remove),
        Box::new(RemoveFirst),
        Box::new(Replace),
        Box::new(ReplaceFirst),
        Box::new(Reverse),
        Box::new(Round),
        Box::new(Rstrip),
        Box::new(Size),
        Box::new(Slice),
        Box::new(Sort),
        Box::new(SortNatural),
        Box::new(Split),
        Box::new(Strip),
        Box::new(StripHtml),
        Box::new(StripNewlines),
        Box::new(Times),
        Box::new(Truncate),
        Box::new(TruncateWords),
        Box::new(Uniq),
        Box::new(Upcase),
        Box::new(UrlDecode),
        Box::new(UrlEncode),
        Box::new(Where),
        Box::new(JsonFilterParser("json")),
        Box::new(JsonFilterParser("jsonify")),
    ];
    for filter in filters {
        let name = filter.reflection().name().to_owned();
        language.filters.register(name, filter);
    }
    language
});

/// A template that does not parse, with liquid's explanation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct TemplateError(String);

/// Parses a template without rendering it, to validate input or flag imported templates.
pub fn check_template(source: &str) -> Result<(), TemplateError> {
    liquid_core::parser::parse(source, &LANGUAGE)
        .map(drop)
        .map_err(|error| TemplateError(error.to_string()))
}

/// Kuma's `renderTemplate(template, msg, monitorJSON, heartbeatJSON)`.
pub(crate) fn render(
    template: &str,
    msg: &str,
    monitor: &Js,
    heartbeat: &Js,
) -> Result<String, SendError> {
    let template = liquid_core::parser::parse(template, &LANGUAGE)
        .map(Template::new)
        .map_err(|error| SendError::Template(error.to_string()))?;

    let (name, address) = if monitor.is_null() {
        (
            Js::from("Monitor Name not available"),
            "testing.hostname".into(),
        )
    } else {
        (monitor.get("name").clone(), extract_address(monitor))
    };
    let status = match heartbeat {
        Js::Null => "⚠️ Test",
        beat if beat.get("status") == &Js::Int(0) => "🔴 Down",
        _ => "✅ Up",
    };
    let context = Js::object([
        ("STATUS", status.into()),
        ("NAME", name.clone()),
        ("HOSTNAME_OR_URL", address.as_str().into()),
        ("status", status.into()),
        ("name", name),
        ("hostnameOrURL", address.into()),
        ("monitorJSON", monitor.clone()),
        ("heartbeatJSON", heartbeat.clone()),
        ("msg", msg.into()),
    ]);
    // Assigned variables live in the outer frame and shadow the context.
    let runtime = GlobalFrame::new(JsFrame {
        parent: RuntimeBuilder::new().build(),
        root: &context,
    });
    template
        .render(&runtime)
        .map_err(|error| SendError::Template(error.to_string()))
}

/// Serves variables straight from [`Js`] values. liquid's own frames convert looked-up values
/// into its `Value`, which loses key order and JavaScript's lenient lookups.
struct JsFrame<'a, P> {
    parent: P,
    root: &'a Js,
}

impl<P: Runtime> JsFrame<'_, P> {
    fn lookup(&self, path: &[ScalarCow<'_>]) -> ValueCow<'_> {
        liquid_core::model::try_find(self.root, path).unwrap_or(ValueCow::Borrowed(&NULL))
    }
}

impl<P: Runtime> Runtime for JsFrame<'_, P> {
    fn partials(&self) -> &dyn PartialStore {
        self.parent.partials()
    }

    fn name(&self) -> Option<KStringRef<'_>> {
        self.parent.name()
    }

    fn roots(&self) -> BTreeSet<KStringCow<'_>> {
        let mut roots = self.parent.roots();
        roots.extend(liquid_core::model::ObjectView::keys(self.root));
        roots
    }

    fn try_get(&self, path: &[ScalarCow<'_>]) -> Option<ValueCow<'_>> {
        Some(self.lookup(path))
    }

    fn get(&self, path: &[ScalarCow<'_>]) -> LiquidResult<ValueCow<'_>> {
        Ok(self.lookup(path))
    }

    fn set_global(&self, name: liquid_core::model::KString, val: Value) -> Option<Value> {
        self.parent.set_global(name, val)
    }

    fn set_index(&self, name: liquid_core::model::KString, val: Value) -> Option<Value> {
        self.parent.set_index(name, val)
    }

    fn get_index<'a>(&'a self, name: &str) -> Option<ValueCow<'a>> {
        self.parent.get_index(name)
    }

    fn registers(&self) -> &Registers {
        self.parent.registers()
    }
}

/// liquidjs `json` / `jsonify`: `JSON.stringify(value, null, space)`.
#[derive(Clone)]
struct JsonFilterParser(&'static str);

impl FilterReflection for JsonFilterParser {
    fn name(&self) -> &str {
        self.0
    }

    fn description(&self) -> &str {
        "Serializes the input as JSON"
    }

    fn positional_parameters(&self) -> &'static [ParameterReflection] {
        &[ParameterReflection {
            name: "space",
            description: "Indent width",
            is_optional: true,
        }]
    }

    fn keyword_parameters(&self) -> &'static [ParameterReflection] {
        &[]
    }
}

impl ParseFilter for JsonFilterParser {
    fn parse(&self, mut arguments: FilterArguments<'_>) -> LiquidResult<Box<dyn Filter>> {
        let space = arguments.positional.next();
        if arguments.positional.next().is_some() || arguments.keyword.next().is_some() {
            return Err(liquid_core::Error::with_msg(format!(
                "`{}` takes at most one argument",
                self.0
            )));
        }
        Ok(Box::new(JsonFilter { space }))
    }

    fn reflection(&self) -> &dyn FilterReflection {
        self
    }
}

#[derive(Debug)]
struct JsonFilter {
    space: Option<Expression>,
}

impl fmt::Display for JsonFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("json")
    }
}

impl Filter for JsonFilter {
    fn evaluate(&self, input: &dyn ValueView, runtime: &dyn Runtime) -> LiquidResult<Value> {
        let space = match &self.space {
            Some(space) => space
                .evaluate(runtime)?
                .as_scalar()
                .and_then(|s| s.to_integer())
                .unwrap_or(0),
            None => 0,
        };
        // JSON.stringify caps the indent at 10.
        let indent = " ".repeat(space.clamp(0, 10) as usize);
        let mut out = Vec::new();
        let result = if indent.is_empty() {
            ViewJson(input).serialize(&mut serde_json::Serializer::new(&mut out))
        } else {
            let formatter = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
            ViewJson(input).serialize(&mut serde_json::Serializer::with_formatter(
                &mut out, formatter,
            ))
        };
        result.map_err(|error| liquid_core::Error::with_msg(error.to_string()))?;
        Ok(Value::scalar(String::from_utf8_lossy(&out).into_owned()))
    }
}

/// Serializes any Liquid value, keeping object key order.
struct ViewJson<'a>(&'a dyn ValueView);

impl Serialize for ViewJson<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let value = self.0;
        if value.is_nil() {
            return serializer.serialize_unit();
        }
        if let Some(array) = value.as_array() {
            let mut seq = serializer.serialize_seq(Some(array.size() as usize))?;
            for item in array.values() {
                seq.serialize_element(&ViewJson(item))?;
            }
            return seq.end();
        }
        if let Some(object) = value.as_object() {
            let mut map = serializer.serialize_map(Some(object.size() as usize))?;
            for (key, item) in object.iter() {
                map.serialize_entry(key.as_str(), &ViewJson(item))?;
            }
            return map.end();
        }
        match value.as_scalar() {
            Some(scalar) => match scalar.type_name() {
                "boolean" => serializer.serialize_bool(scalar.to_bool().unwrap_or_default()),
                "whole number" => serializer.serialize_i64(scalar.to_integer().unwrap_or_default()),
                "fractional number" => {
                    serializer.serialize_f64(scalar.to_float().unwrap_or_default())
                }
                _ => serializer.serialize_str(scalar.to_kstr().as_str()),
            },
            None => serializer.serialize_unit(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::render;
    use crate::js::Js;

    fn monitor() -> Js {
        Js::object([
            ("name", Js::from("API")),
            ("type", "port".into()),
            ("hostname", "db.example.com".into()),
            ("port", Js::Int(5432)),
            ("path", vec!["Prod", "API"].into()),
            (
                "tags",
                Js::Array(vec![Js::object([
                    ("name", Js::from("prod")),
                    ("value", "".into()),
                ])]),
            ),
        ])
    }

    fn beat() -> Js {
        Js::object([("status", Js::Int(0)), ("ping", Js::Null)])
    }

    fn up_down(template: &str) -> String {
        render(template, "msg", &monitor(), &beat()).unwrap()
    }

    #[test]
    fn missing_values_print_nothing_like_liquidjs() {
        assert_eq!(
            up_down("[{{ nope }}|{{ monitorJSON.nope.deeper }}|{{ heartbeatJSON.ping }}]"),
            "[||]"
        );
        assert_eq!(
            render(
                "[{{ monitorJSON.name }}|{{ heartbeatJSON.msg }}]",
                "m",
                &Js::Null,
                &Js::Null
            )
            .unwrap(),
            "[|]"
        );
    }

    #[test]
    fn values_print_like_javascript_string() {
        assert_eq!(
            up_down(
                "{{ monitorJSON }} {{ monitorJSON.path }} {{ monitorJSON.port }} {{ status }} {{ hostnameOrURL }}"
            ),
            "[object Object] ProdAPI 5432 🔴 Down db.example.com:5432"
        );
    }

    #[test]
    fn conditions_follow_liquid_truthiness() {
        let template = "{% if heartbeatJSON.status == 0 %}down{% endif %}\
            {% if monitorJSON.tags[0].value %}set{% endif %}\
            {% if heartbeatJSON.ping %}ping{% else %}none{% endif %}\
            {{ heartbeatJSON.ping | default: 'n/a' }}";
        // Empty strings are truthy in Liquid, `null` is not.
        assert_eq!(up_down(template), "downsetnonen/a");
        assert_eq!(
            up_down("{% for tag in monitorJSON.tags %}{{ tag.name }}{% endfor %}"),
            "prod"
        );
    }

    #[test]
    fn json_filter_keeps_key_order_and_indent() {
        assert_eq!(
            up_down("{{ heartbeatJSON | json }}"),
            "{\"status\":0,\"ping\":null}"
        );
        assert_eq!(
            up_down("{{ heartbeatJSON | json: 2 }}"),
            "{\n  \"status\": 0,\n  \"ping\": null\n}"
        );
        assert_eq!(up_down("{{ msg | jsonify }}"), "\"msg\"");
    }
}
