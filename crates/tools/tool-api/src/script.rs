//! Pre-request and post-response scripts in JavaScript (Boa), with the
//! part of Postman's `pm` API that scripts use day to day:
//!
//! - `pm.environment` / `pm.globals` / `pm.collectionVariables` /
//!   `pm.variables`: `get`, `set`, `unset`, `has`
//! - `pm.request`: `method`, `url`, `headers` (object), `params` (object),
//!   `body` (`{kind, text, form}`) — changes made in a pre-request script
//!   are sent
//! - `pm.response`: `code`, `status`, `headers` (object), `responseTime`,
//!   `text()`, `json()`, `to.have.status(n)`, `to.be.ok`
//! - `pm.test(name, fn)`, `pm.expect(value)` with a small chai subset
//!   (`equal`, `eql`, `include`, `have.property`, `have.length`, `be.true`,
//!   `be.false`, `be.null`, `be.undefined`, `be.above`, `be.below`,
//!   `be.oneOf`, `match`, `exist`, and `.not`)
//! - `console.log/info/warn/error`
//! - `require`-free libraries: a collection's JavaScript libraries are
//!   evaluated first, so anything they define is in scope.
//!
//! The engine runs with a loop and recursion limit and has no I/O: no
//! `fetch`, no timers, no file access. Scripts cannot reach the vault; a
//! `secret:` reference resolves only when the request is sent.

use std::cell::RefCell;
use std::collections::HashMap;

use boa_engine::{Context, JsNativeError, JsResult, JsValue, NativeFunction, Source, js_string};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    Local,
    Environment,
    Collection,
    Globals,
}

impl Scope {
    fn parse(s: &str) -> Option<Scope> {
        match s {
            "local" => Some(Scope::Local),
            "environment" => Some(Scope::Environment),
            "collection" => Some(Scope::Collection),
            "globals" => Some(Scope::Globals),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scopes {
    pub local: HashMap<String, String>,
    pub environment: HashMap<String, String>,
    pub collection: HashMap<String, String>,
    pub globals: HashMap<String, String>,
}

impl Scopes {
    fn map(&self, scope: Scope) -> &HashMap<String, String> {
        match scope {
            Scope::Local => &self.local,
            Scope::Environment => &self.environment,
            Scope::Collection => &self.collection,
            Scope::Globals => &self.globals,
        }
    }
    fn map_mut(&mut self, scope: Scope) -> &mut HashMap<String, String> {
        match scope {
            Scope::Local => &mut self.local,
            Scope::Environment => &mut self.environment,
            Scope::Collection => &mut self.collection,
            Scope::Globals => &mut self.globals,
        }
    }
    /// `pm.variables.get`: the narrowest scope wins.
    pub fn lookup(&self, key: &str) -> Option<&str> {
        [
            Scope::Local,
            Scope::Environment,
            Scope::Collection,
            Scope::Globals,
        ]
        .into_iter()
        .find_map(|s| self.map(s).get(key).map(String::as_str))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub scope: Scope,
    pub key: String,
    /// `None` = unset.
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct Input {
    /// `{method, url, headers: {}, params: {}, body: {kind, text, form: {}}}`
    pub request: Value,
    /// `{code, status, headers: {}, body: "", responseTime}` after a send.
    pub response: Option<Value>,
    pub scopes: Scopes,
    pub libraries: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Output {
    /// `pm.request` after the script (pre-request scripts may change it).
    pub request: Option<Value>,
    pub scopes: Scopes,
    pub changes: Vec<Change>,
    pub logs: Vec<String>,
    pub tests: Vec<TestResult>,
    pub error: Option<String>,
}

#[derive(Default)]
struct Host {
    scopes: Scopes,
    changes: Vec<Change>,
    logs: Vec<String>,
    tests: Vec<TestResult>,
}

thread_local! {
    static HOST: RefCell<Host> = RefCell::new(Host::default());
}

fn arg_string(args: &[JsValue], i: usize, ctx: &mut Context) -> JsResult<String> {
    match args.get(i) {
        None => Ok(String::new()),
        Some(v) if v.is_undefined() || v.is_null() => Ok(String::new()),
        Some(v) => Ok(v.to_string(ctx)?.to_std_string_escaped()),
    }
}

fn scope_arg(args: &[JsValue], ctx: &mut Context) -> JsResult<Scope> {
    let s = arg_string(args, 0, ctx)?;
    Scope::parse(&s).ok_or_else(|| {
        JsNativeError::typ()
            .with_message(format!("unknown scope {s}"))
            .into()
    })
}

fn hs_log(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let mut parts = Vec::new();
    for v in args {
        let text = if v.is_object() {
            match v.to_json(ctx)? {
                Some(j) => serde_json::to_string(&j).unwrap_or_default(),
                None => "undefined".into(),
            }
        } else {
            v.to_string(ctx)?.to_std_string_escaped()
        };
        parts.push(text);
    }
    HOST.with(|h| h.borrow_mut().logs.push(parts.join(" ")));
    Ok(JsValue::undefined())
}

fn hs_get(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let scope = arg_string(args, 0, ctx)?;
    let key = arg_string(args, 1, ctx)?;
    let found = HOST.with(|h| {
        let h = h.borrow();
        if scope == "any" {
            h.scopes.lookup(&key).map(str::to_string)
        } else {
            Scope::parse(&scope).and_then(|s| h.scopes.map(s).get(&key).cloned())
        }
    });
    Ok(match found {
        Some(v) => JsValue::from(js_string!(v)),
        None => JsValue::undefined(),
    })
}

fn hs_set(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let scope = scope_arg(args, ctx)?;
    let key = arg_string(args, 1, ctx)?;
    let value = match args.get(2) {
        Some(v) if v.is_object() => match v.to_json(ctx)? {
            Some(j) => serde_json::to_string(&j).unwrap_or_default(),
            None => String::new(),
        },
        _ => arg_string(args, 2, ctx)?,
    };
    HOST.with(|h| {
        let mut h = h.borrow_mut();
        h.scopes.map_mut(scope).insert(key.clone(), value.clone());
        h.changes.push(Change {
            scope,
            key,
            value: Some(value),
        });
    });
    Ok(JsValue::undefined())
}

fn hs_unset(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let scope = scope_arg(args, ctx)?;
    let key = arg_string(args, 1, ctx)?;
    HOST.with(|h| {
        let mut h = h.borrow_mut();
        h.scopes.map_mut(scope).remove(&key);
        h.changes.push(Change {
            scope,
            key,
            value: None,
        });
    });
    Ok(JsValue::undefined())
}

fn hs_test(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let name = arg_string(args, 0, ctx)?;
    let passed = args.get(1).is_some_and(|v| v.to_boolean());
    let message = arg_string(args, 2, ctx)?;
    HOST.with(|h| {
        h.borrow_mut().tests.push(TestResult {
            name,
            passed,
            message,
        })
    });
    Ok(JsValue::undefined())
}

/// The `pm` object, written in JavaScript over the four host functions.
const PRELUDE: &str = r#"
(function () {
  const scopeApi = (scope) => ({
    get: (k) => __hs_get(scope, String(k)),
    set: (k, v) => __hs_set(scope, String(k), v),
    unset: (k) => __hs_unset(scope, String(k)),
    has: (k) => __hs_get(scope, String(k)) !== undefined,
    replaceIn: (t) => String(t).replace(/\{\{([^}]+)\}\}/g, (m, k) => { const v = __hs_get("any", k.trim()); return v === undefined ? m : v; }),
    toObject: () => ({}),
  });
  const console = {
    log: (...a) => __hs_log(...a), info: (...a) => __hs_log(...a),
    warn: (...a) => __hs_log("[warn]", ...a), error: (...a) => __hs_log("[error]", ...a),
    debug: (...a) => __hs_log(...a),
  };
  globalThis.console = console;
  const req = globalThis.__hs_request || { method: "GET", url: "", headers: {}, params: {}, body: { kind: "none", text: "", form: {} } };
  req.headers = req.headers || {};
  req.headers.add = function (h) { if (h && h.key !== undefined) { this[h.key] = h.value; } };
  req.headers.upsert = req.headers.add;
  req.headers.remove = function (k) { delete this[k]; };
  req.headers.get = function (k) { const kk = Object.keys(this).find((x) => x.toLowerCase() === String(k).toLowerCase()); return kk === undefined ? undefined : this[kk]; };
  req.headers.has = function (k) { return this.get(k) !== undefined; };
  Object.defineProperty(req.headers, "add", { enumerable: false });
  Object.defineProperty(req.headers, "upsert", { enumerable: false });
  Object.defineProperty(req.headers, "remove", { enumerable: false });
  Object.defineProperty(req.headers, "get", { enumerable: false });
  Object.defineProperty(req.headers, "has", { enumerable: false });
  const res = globalThis.__hs_response;
  const response = res ? {
    code: res.code, status: res.status, headers: res.headers || {}, responseTime: res.responseTime,
    text: () => res.body, json: () => JSON.parse(res.body),
    to: {
      have: {
        status: (n) => { if (typeof n === "number" ? res.code !== n : res.status !== n) throw new Error("expected status " + n + " but got " + res.code); },
        header: (k) => { if (!Object.keys(res.headers || {}).some((x) => x.toLowerCase() === String(k).toLowerCase())) throw new Error("expected header " + k); },
        jsonBody: () => { JSON.parse(res.body); },
      },
      be: { get ok() { if (res.code < 200 || res.code >= 300) throw new Error("expected 2xx but got " + res.code); return true; } },
    },
  } : undefined;
  if (response) { response.headers.get = function (k) { const kk = Object.keys(this).find((x) => x.toLowerCase() === String(k).toLowerCase()); return kk === undefined ? undefined : this[kk]; }; Object.defineProperty(response.headers, "get", { enumerable: false }); }
  function expect(actual) {
    const mk = (negate) => {
      const check = (cond, msg) => { if (negate ? cond : !cond) throw new Error((negate ? "expected not " : "expected ") + msg); };
      const api = {
        equal: (e) => check(actual === e, JSON.stringify(actual) + " to equal " + JSON.stringify(e)),
        equals: (e) => api.equal(e),
        eql: (e) => check(JSON.stringify(actual) === JSON.stringify(e), JSON.stringify(actual) + " to deep equal " + JSON.stringify(e)),
        deep: null,
        include: (e) => check(typeof actual === "string" ? actual.includes(e) : Array.isArray(actual) ? actual.includes(e) : (actual && Object.keys(e || {}).every((k) => actual[k] === e[k])), JSON.stringify(actual) + " to include " + JSON.stringify(e)),
        contain: (e) => api.include(e),
        match: (re) => check(new RegExp(re).test(String(actual)), JSON.stringify(actual) + " to match " + re),
        exist: () => check(actual !== null && actual !== undefined, "value to exist"),
        get ok() { check(!!actual, "value to be truthy"); return true; },
        get true() { check(actual === true, JSON.stringify(actual) + " to be true"); return true; },
        get false() { check(actual === false, JSON.stringify(actual) + " to be false"); return true; },
        get null() { check(actual === null, JSON.stringify(actual) + " to be null"); return true; },
        get undefined() { check(actual === undefined, "value to be undefined"); return true; },
        get empty() { check(actual == null || actual.length === 0 || (typeof actual === "object" && Object.keys(actual).length === 0), "value to be empty"); return true; },
        above: (n) => check(actual > n, actual + " to be above " + n),
        below: (n) => check(actual < n, actual + " to be below " + n),
        least: (n) => check(actual >= n, actual + " to be at least " + n),
        most: (n) => check(actual <= n, actual + " to be at most " + n),
        within: (a, b) => check(actual >= a && actual <= b, actual + " to be within " + a + ".." + b),
        oneOf: (list) => check(list.includes(actual), JSON.stringify(actual) + " to be one of " + JSON.stringify(list)),
        a: (t) => check((t === "array" ? Array.isArray(actual) : typeof actual === t), "value to be a " + t),
        an: (t) => api.a(t),
        property: (p, v) => check(actual != null && p in Object(actual) && (v === undefined || actual[p] === v), "property " + p),
        length: (n) => check(actual != null && actual.length === n, "length " + n),
        lengthOf: (n) => api.length(n),
        keys: (...ks) => check(actual != null && ks.flat().every((k) => k in actual), "keys " + ks.flat().join(",")),
        string: (s) => check(String(actual).includes(s), JSON.stringify(actual) + " to contain " + s),
      };
      api.to = api; api.be = api; api.been = api; api.is = api; api.that = api; api.have = api; api.has = api; api.with = api; api.at = api; api.of = api; api.same = api; api.deep = api; api.and = api;
      Object.defineProperty(api, "not", { get: () => mk(!negate) });
      return api;
    };
    return mk(false);
  }
  const pm = {
    environment: scopeApi("environment"),
    globals: scopeApi("globals"),
    collectionVariables: scopeApi("collection"),
    variables: Object.assign(scopeApi("local"), { get: (k) => __hs_get("any", String(k)) }),
    request: req,
    response: response,
    expect: expect,
    test: (name, fn) => { try { fn(); __hs_test(String(name), true, ""); } catch (e) { __hs_test(String(name), false, String(e && e.message ? e.message : e)); } },
    info: { requestName: req.name || "", iteration: 0, iterationCount: 1 },
    iterationData: { get: () => undefined },
    sendRequest: () => { throw new Error("pm.sendRequest is not available in HyperStack scripts"); },
  };
  pm.environmentVariables = pm.environment;
  pm.globalVariables = pm.globals;
  pm.collectionVars = pm.collectionVariables;
  globalThis.pm = pm;
  globalThis.postman = { setEnvironmentVariable: (k, v) => pm.environment.set(k, v), setGlobalVariable: (k, v) => pm.globals.set(k, v), setCollectionVariable: (k, v) => pm.collectionVariables.set(k, v), getEnvironmentVariable: (k) => pm.environment.get(k), getGlobalVariable: (k) => pm.globals.get(k), getCollectionVariable: (k) => pm.collectionVariables.get(k), clearEnvironmentVariable: (k) => pm.environment.unset(k), clearGlobalVariable: (k) => pm.globals.unset(k) };
  globalThis.tests = {};
  globalThis.responseCode = response ? { code: response.code, name: response.status } : undefined;
  globalThis.responseBody = response ? response.text() : undefined;
  globalThis.responseTime = response ? response.responseTime : undefined;
})();
"#;

/// Run `code` with the `pm` API bound to `input`. Never panics: engine
/// errors land in `Output::error`, and whatever the script changed before
/// failing is still reported.
pub fn run(code: &str, input: Input) -> Output {
    HOST.with(|h| {
        *h.borrow_mut() = Host {
            scopes: input.scopes.clone(),
            ..Default::default()
        }
    });
    let mut ctx = Context::default();
    ctx.runtime_limits_mut().set_loop_iteration_limit(2_000_000);
    ctx.runtime_limits_mut().set_recursion_limit(512);
    let mut error = None;
    let register = |ctx: &mut Context| -> JsResult<()> {
        ctx.register_global_callable(
            js_string!("__hs_log"),
            1,
            NativeFunction::from_fn_ptr(hs_log),
        )?;
        ctx.register_global_callable(
            js_string!("__hs_get"),
            2,
            NativeFunction::from_fn_ptr(hs_get),
        )?;
        ctx.register_global_callable(
            js_string!("__hs_set"),
            3,
            NativeFunction::from_fn_ptr(hs_set),
        )?;
        ctx.register_global_callable(
            js_string!("__hs_unset"),
            2,
            NativeFunction::from_fn_ptr(hs_unset),
        )?;
        ctx.register_global_callable(
            js_string!("__hs_test"),
            3,
            NativeFunction::from_fn_ptr(hs_test),
        )?;
        Ok(())
    };
    if let Err(e) = register(&mut ctx) {
        error = Some(format!("engine: {e}"));
    }
    let setup = format!(
        "globalThis.__hs_request = {}; globalThis.__hs_response = {};",
        serde_json::to_string(&input.request).unwrap_or_else(|_| "{}".into()),
        input
            .response
            .as_ref()
            .map(|r| serde_json::to_string(r).unwrap_or_else(|_| "null".into()))
            .unwrap_or_else(|| "null".into())
    );
    if error.is_none()
        && let Err(e) = ctx.eval(Source::from_bytes(setup.as_bytes()))
    {
        error = Some(format!("setup: {e}"));
    }
    if error.is_none()
        && let Err(e) = ctx.eval(Source::from_bytes(PRELUDE.as_bytes()))
    {
        error = Some(format!("prelude: {e}"));
    }
    for lib in &input.libraries {
        if error.is_some() {
            break;
        }
        if let Err(e) = ctx.eval(Source::from_bytes(lib.as_bytes())) {
            error = Some(format!("library: {}", friendly(&e, &mut ctx)));
        }
    }
    if error.is_none()
        && let Err(e) = ctx.eval(Source::from_bytes(code.as_bytes()))
    {
        error = Some(friendly(&e, &mut ctx));
    }
    let request = if error.is_none() {
        ctx.eval(Source::from_bytes(b"JSON.stringify(pm.request)"))
            .ok()
            .and_then(|v| v.as_string().map(|s| s.to_std_string_escaped()))
            .and_then(|t| serde_json::from_str(&t).ok())
    } else {
        None
    };
    let host = HOST.with(|h| std::mem::take(&mut *h.borrow_mut()));
    Output {
        request,
        scopes: host.scopes,
        changes: host.changes,
        logs: host.logs,
        tests: host.tests,
        error,
    }
}

fn friendly(e: &boa_engine::JsError, ctx: &mut Context) -> String {
    match e.try_native(ctx) {
        Ok(native) => native.to_string(),
        Err(_) => e.to_string(),
    }
}

/// Request JSON for the script from the parts the sender has.
// Eight named parts read better than a struct built once per call.
#[allow(clippy::too_many_arguments)]
pub fn request_value(
    name: &str,
    method: &str,
    url: &str,
    headers: &[(String, String)],
    params: &[(String, String)],
    body_kind: &str,
    body_text: &str,
    form: &[(String, String)],
) -> Value {
    let obj = |list: &[(String, String)]| -> Value {
        Value::Object(
            list.iter()
                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                .collect(),
        )
    };
    serde_json::json!({
        "name": name,
        "method": method,
        "url": url,
        "headers": obj(headers),
        "params": obj(params),
        "body": { "kind": body_kind, "text": body_text, "form": obj(form) },
    })
}

/// Response JSON for the script.
pub fn response_value(
    code: u16,
    status: &str,
    headers: &[(String, String)],
    body: &str,
    millis: u64,
) -> Value {
    serde_json::json!({
        "code": code,
        "status": status,
        "headers": Value::Object(headers.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect()),
        "body": body,
        "responseTime": millis,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input_with_response(body: &str) -> Input {
        let mut scopes = Scopes::default();
        scopes.environment.insert("base".into(), "https://e".into());
        scopes.globals.insert("g".into(), "1".into());
        Input {
            request: request_value(
                "r",
                "POST",
                "{{base}}/x",
                &[("X-A".into(), "1".into())],
                &[],
                "json",
                "{}",
                &[],
            ),
            response: Some(response_value(
                201,
                "Created",
                &[("Content-Type".into(), "application/json".into())],
                body,
                42,
            )),
            scopes,
            libraries: vec!["function helper(x) { return x * 2; }".into()],
        }
    }

    #[test]
    fn variables_tests_logs_and_request_edits() {
        let out = run(
            r#"
            const data = pm.response.json();
            pm.environment.set("token", data.token);
            pm.globals.unset("g");
            pm.collectionVariables.set("n", helper(21));
            pm.environmentVariables.set("alias_env", "e");
            pm.globalVariables.set("alias_glob", "g");
            postman.setCollectionVariable("alias_coll", "c");
            console.log("hello", data);
            pm.test("status 201", () => pm.response.to.have.status(201));
            pm.test("has token", () => pm.expect(data.token).to.equal("abc"));
            pm.test("fails", () => pm.expect(1).to.equal(2));
            pm.test("not", () => pm.expect([1,2]).to.not.include(3));
            pm.request.headers.add({ key: "X-B", value: pm.variables.get("base") });
            pm.request.url = pm.request.url + "?y=1";
            "#,
            input_with_response(r#"{"token":"abc"}"#),
        );
        assert_eq!(out.error, None, "{:?}", out.logs);
        assert_eq!(
            out.scopes.environment.get("token").map(String::as_str),
            Some("abc")
        );
        assert!(!out.scopes.globals.contains_key("g"));
        assert_eq!(
            out.scopes.collection.get("n").map(String::as_str),
            Some("42")
        );
        assert_eq!(out.logs[0], "hello {\"token\":\"abc\"}");
        let results: Vec<(&str, bool)> = out
            .tests
            .iter()
            .map(|t| (t.name.as_str(), t.passed))
            .collect();
        assert_eq!(
            results,
            vec![
                ("status 201", true),
                ("has token", true),
                ("fails", false),
                ("not", true)
            ]
        );
        assert!(out.tests[2].message.contains("expected"));
        let req = out.request.unwrap();
        assert_eq!(req["headers"]["X-B"], "https://e");
        assert_eq!(req["url"], "{{base}}/x?y=1");
        // The three Postman calls and the three aliases: six writes.
        assert_eq!(out.changes.len(), 6);
        assert_eq!(
            out.scopes.environment.get("alias_env").map(String::as_str),
            Some("e")
        );
        assert_eq!(
            out.scopes.globals.get("alias_glob").map(String::as_str),
            Some("g")
        );
        assert_eq!(
            out.scopes.collection.get("alias_coll").map(String::as_str),
            Some("c")
        );
    }

    #[test]
    fn errors_are_reported_not_panicked() {
        let out = run("this is not js", Input::default());
        assert!(out.error.is_some());
        let out = run(
            "pm.environment.set('a', 1); throw new Error('boom');",
            Input::default(),
        );
        assert!(out.error.as_deref().is_some_and(|e| e.contains("boom")));
        assert_eq!(
            out.scopes.environment.get("a").map(String::as_str),
            Some("1")
        );
        let out = run("while (true) {}", Input::default());
        assert!(out.error.is_some());
    }
}
