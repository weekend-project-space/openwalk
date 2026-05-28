mod bindings;
mod errors;
mod host_functions;

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use scheme4r::{
    runtime::{procedure::Procedure, BuiltinFn, EnvRef},
    Environment, Scheme, SchemeError, Value,
};

use crate::{
    browser::BrowserClient, builtin_tools, extlib::LIB, tool_metadata::parse_tool_metadata,
};

use bindings::install_openwalk_bindings;
use errors::scheme_error_to_anyhow;
use host_functions::install_host_context;

pub const SCHEME_BUILTINS: &[&str] = builtin_tools::SCHEME_BUILTINS;

pub async fn execute_script(
    script_path: &Path,
    args: &[String],
    browser: BrowserClient,
    session_name: Option<String>,
) -> Result<Value> {
    let source = tokio::fs::read_to_string(script_path)
        .await
        .with_context(|| format!("failed to read script {}", script_path.display()))?;
    let script_path = script_path.to_path_buf();
    let args = args.to_vec();
    let session_name = session_name;

    tokio::task::block_in_place(|| {
        execute_script_sync(script_path, source, args, browser, session_name)
    })
}

pub async fn execute_builtin(
    name: &str,
    args: &[String],
    browser: BrowserClient,
    session_name: Option<String>,
) -> Result<Value> {
    let name = name.to_string();
    let args = args.to_vec();
    let session_name = session_name;

    tokio::task::block_in_place(|| execute_builtin_sync(name, args, browser, session_name))
}

fn execute_builtin_sync(
    name: String,
    args: Vec<String>,
    browser: BrowserClient,
    session_name: Option<String>,
) -> Result<Value> {
    if !SCHEME_BUILTINS.contains(&name.as_str()) {
        bail!("unknown builtin host function `{name}`");
    }

    let env = Environment::standard();
    let pseudo_path = PathBuf::from(format!("<builtin:{name}>"));
    install_openwalk_bindings(
        env.clone(),
        &pseudo_path,
        &args,
        session_name.as_deref(),
        None,
    );

    let builtin = lookup_builtin_function(env.clone(), &name)?;
    let cli_args = builtin_tools::cli_args_to_scheme_values(&name, &args)?;
    let engine = scheme4r::eval::Engine::new(env);

    let _guard = install_host_context(browser);
    let value = builtin(&engine, &cli_args)
        .map_err(|err| scheme_error_to_anyhow(format!("builtin `{name}` execution failed"), err))?;

    Ok(value)
}

fn execute_script_sync(
    script_path: PathBuf,
    source: String,
    args: Vec<String>,
    browser: BrowserClient,
    session_name: Option<String>,
) -> Result<Value> {
    let env = Environment::standard();
    let script_meta = parse_tool_metadata(&source)?;
    install_openwalk_bindings(
        env.clone(),
        &script_path,
        &args,
        session_name.as_deref(),
        script_meta.as_ref(),
    );
    let scheme = Scheme::with_env(env);

    let _guard = install_host_context(browser);
    let loaded_value = scheme.eval(&format!("{} {}", LIB, source)).map_err(|err| {
        scheme_error_to_anyhow("scheme execution failed while loading script", err)
    })?;

    let value: Value = match scheme.eval("(main openwalk-args)") {
        Ok(value) => value,
        Err(err) if is_missing_main(&err) => loaded_value,
        Err(err) => {
            return Err(scheme_error_to_anyhow(
                "scheme execution failed while calling `main`",
                err,
            ));
        }
    };

    Ok(value)
}

fn lookup_builtin_function(env: EnvRef, name: &str) -> Result<BuiltinFn> {
    let value = env
        .borrow()
        .lookup(name)
        .map_err(|err| anyhow::anyhow!("builtin `{name}` lookup failed: {err}"))?;

    match value {
        Value::Procedure(proc_ref) => match proc_ref.as_ref() {
            Procedure::Builtin { func, .. } => Ok(*func),
            _ => bail!("builtin `{name}` is not registered as a host builtin"),
        },
        _ => bail!("builtin `{name}` is not bound to a callable procedure"),
    }
}

fn is_missing_main(err: &SchemeError) -> bool {
    err.to_string().contains("undefined variable: main")
}

#[cfg(test)]
use host_functions::{browser_console, browser_list, browser_upload};

#[cfg(test)]
mod tests;
