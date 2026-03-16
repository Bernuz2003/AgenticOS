use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{format_ident, quote, ToTokens};
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::{
    Expr, ExprArray, ExprLit, ExprPath, FnArg, Ident, ItemFn, Lit, LitBool, LitStr, Meta, PatType,
    ReturnType, Token, Type, TypePath, TypeReference,
};

#[proc_macro_attribute]
pub fn agentic_tool(attr: TokenStream, item: TokenStream) -> TokenStream {
    match expand_agentic_tool(attr, item) {
        Ok(tokens) => tokens,
        Err(err) => err.to_compile_error().into(),
    }
}

fn expand_agentic_tool(attr: TokenStream, item: TokenStream) -> Result<TokenStream, syn::Error> {
    let args = Punctuated::<Meta, Token![,]>::parse_terminated
        .parse(attr)
        .map_err(|err| syn::Error::new(Span::call_site(), err))?;
    let config = ToolMacroConfig::parse(args)?;

    let function = syn::parse::<ItemFn>(item)?;
    validate_function_signature(&function)?;

    let function_ident = &function.sig.ident;
    let input_ty = extract_input_type(&function.sig.inputs[0])?;
    let output_ty = extract_output_type(&function.sig.output)?;

    let returns_tool_result = type_last_ident(output_ty)
        .map(|ident| ident == "ToolResult")
        .unwrap_or(false);
    if !returns_tool_result && config.output_schema_type.is_some() {
        return Err(syn::Error::new_spanned(
            function,
            "`output_schema_type` is supported only when the tool returns Result<ToolResult, ToolError>",
        ));
    }

    let tool_struct_ident = format_ident!("{}Tool", pascal_case(function_ident));
    let factory_ident = format_ident!("__{}_host_builtin_factory", function_ident);
    let entry_fn_ident = format_ident!("{}_registry_entry", function_ident);
    let registration_fn_ident = format_ident!("{}_host_builtin_registration", function_ident);

    let tool_name = config.name;
    let description = config.description;
    let aliases = config.aliases;
    let capabilities = config.capabilities;
    let allowed_callers = config.allowed_callers;
    let dangerous = config.dangerous;
    let enabled = config.enabled;
    let output_schema_type = config.output_schema_type;

    let execute_body = if returns_tool_result {
        // ToolResult passthrough: no output serialization, full control to the tool.
        quote! {
            let input: #input_ty = serde_json::from_value(invocation.input.clone()).map_err(|err| {
                crate::tools::error::ToolError::InvalidInput(
                    self.name().to_string(),
                    format!("failed to deserialize input: {err}"),
                )
            })?;
            #function_ident(input, context)
        }
    } else {
        // Typed output: serialize to ToolResult with display_text policy.
        quote! {
            let input: #input_ty = serde_json::from_value(invocation.input.clone()).map_err(|err| {
                crate::tools::error::ToolError::InvalidInput(
                    self.name().to_string(),
                    format!("failed to deserialize input: {err}"),
                )
            })?;
            let output = #function_ident(input, context)?;
            crate::tools::api::typed_output_to_tool_result(self.name(), output)
        }
    };

    let output_schema_expr = if returns_tool_result {
        if let Some(schema_ty) = output_schema_type {
            quote! {
                crate::tools::schema::generated_schema::<#schema_ty>().unwrap_or_else(|err| {
                    tracing::error!(
                        tool = #tool_name,
                        %err,
                        "failed to generate ToolResult output schema, using permissive fallback"
                    );
                    serde_json::json!({ "type": "object" })
                })
            }
        } else {
            // ToolResult passthrough without an explicit schema type: use a permissive output schema.
            quote! {
                serde_json::json!({ "type": "object" })
            }
        }
    } else {
        // Typed output: derive schema from the output type.
        quote! {
            crate::tools::schema::generated_schema::<#output_ty>().unwrap_or_else(|err| {
                // Schema generation from schemars should never fail for a type that
                // derives JsonSchema + Serialize. If it does, fall back to a permissive
                // schema and log the error rather than panicking at boot.
                tracing::error!(
                    tool = #tool_name,
                    %err,
                    "failed to generate output schema, using permissive fallback"
                );
                serde_json::json!({ "type": "object" })
            })
        }
    };

    let expanded = quote! {
        #function

        #[allow(dead_code)]
        #[derive(Debug, Default, Clone, Copy)]
        struct #tool_struct_ident;

        impl crate::tools::api::Tool for #tool_struct_ident {
            fn name(&self) -> &str {
                #tool_name
            }

            fn execute(
                &self,
                invocation: &crate::tools::invocation::ToolInvocation,
                context: &crate::tools::invocation::ToolContext,
            ) -> Result<crate::tools::api::ToolResult, crate::tools::error::ToolError> {
                #execute_body
            }
        }

        #[allow(dead_code)]
        fn #factory_ident() -> Box<dyn crate::tools::api::Tool> {
            Box::new(#tool_struct_ident)
        }

        #[allow(dead_code)]
        pub(crate) fn #entry_fn_ident() -> crate::tool_registry::ToolRegistryEntry {
            let input_schema = crate::tools::schema::generated_schema::<#input_ty>().unwrap_or_else(|err| {
                tracing::error!(
                    tool = #tool_name,
                    %err,
                    "failed to generate input schema, using permissive fallback"
                );
                serde_json::json!({ "type": "object" })
            });
            let output_schema = #output_schema_expr;

            crate::tool_registry::ToolRegistryEntry {
                descriptor: crate::tool_registry::ToolDescriptor {
                    name: #tool_name.to_string(),
                    aliases: vec![#(#aliases.to_string()),*],
                    description: #description.to_string(),
                    input_schema,
                    output_schema,
                    allowed_callers: vec![#(crate::tools::invocation::ToolCaller::#allowed_callers),*],
                    backend_kind: crate::tool_registry::ToolBackendKind::Host,
                    capabilities: vec![#(#capabilities.to_string()),*],
                    dangerous: #dangerous,
                    enabled: #enabled,
                    source: crate::tool_registry::ToolSource::BuiltIn,
                },
                backend: crate::tool_registry::ToolBackendConfig::Host {
                    executor: crate::tool_registry::HostExecutor::Dynamic(#tool_name.to_string()),
                },
            }
        }

        #[allow(dead_code)]
        pub(crate) fn #registration_fn_ident() -> crate::tools::builtins::HostBuiltinRegistration {
            crate::tools::builtins::HostBuiltinRegistration::new(#entry_fn_ident(), #factory_ident)
        }
    };

    Ok(expanded.into())
}

struct ToolMacroConfig {
    name: LitStr,
    description: LitStr,
    aliases: Vec<LitStr>,
    capabilities: Vec<LitStr>,
    allowed_callers: Vec<Ident>,
    output_schema_type: Option<Type>,
    dangerous: LitBool,
    enabled: LitBool,
}

impl ToolMacroConfig {
    fn parse(args: Punctuated<Meta, Token![,]>) -> Result<Self, syn::Error> {
        let mut name = None;
        let mut description = None;
        let mut aliases = None;
        let mut capabilities = None;
        let mut allowed_callers = None;
        let mut output_schema_type = None;
        let mut dangerous = None;
        let mut enabled = None;

        for meta in args {
            let Meta::NameValue(name_value) = meta else {
                return Err(syn::Error::new_spanned(
                    meta,
                    "expected name = value entries in #[agentic_tool(...)]",
                ));
            };
            let Some(ident) = name_value.path.get_ident() else {
                return Err(syn::Error::new_spanned(
                    name_value.path,
                    "unsupported attribute key",
                ));
            };

            match ident.to_string().as_str() {
                "name" => name = Some(parse_lit_str(&name_value.value, "name")?),
                "description" => {
                    description = Some(parse_lit_str(&name_value.value, "description")?)
                }
                "aliases" => aliases = Some(parse_lit_str_array(&name_value.value, "aliases")?),
                "capabilities" => {
                    capabilities = Some(parse_lit_str_array(&name_value.value, "capabilities")?)
                }
                "allowed_callers" => {
                    allowed_callers = Some(parse_ident_array(&name_value.value, "allowed_callers")?)
                }
                "output_schema_type" => {
                    output_schema_type = Some(parse_type(&name_value.value, "output_schema_type")?)
                }
                "dangerous" => dangerous = Some(parse_lit_bool(&name_value.value, "dangerous")?),
                "enabled" => enabled = Some(parse_lit_bool(&name_value.value, "enabled")?),
                _ => {
                    return Err(syn::Error::new_spanned(
                        ident,
                        "unsupported #[agentic_tool] option",
                    ))
                }
            }
        }

        let name = name.ok_or_else(|| missing_option("name"))?;
        validate_tool_name(&name)?;
        let allowed_callers = allowed_callers.ok_or_else(|| missing_option("allowed_callers"))?;
        if allowed_callers.is_empty() {
            return Err(syn::Error::new(
                Span::call_site(),
                "`allowed_callers` must contain at least one caller",
            ));
        }
        for alias in aliases.clone().unwrap_or_default() {
            validate_tool_name(&alias)?;
        }

        Ok(Self {
            name,
            description: description.ok_or_else(|| missing_option("description"))?,
            aliases: aliases.unwrap_or_default(),
            capabilities: capabilities.unwrap_or_default(),
            allowed_callers,
            output_schema_type,
            dangerous: dangerous.unwrap_or_else(|| LitBool::new(false, Span::call_site())),
            enabled: enabled.unwrap_or_else(|| LitBool::new(true, Span::call_site())),
        })
    }
}

fn validate_function_signature(function: &ItemFn) -> Result<(), syn::Error> {
    if let Some(asyncness) = function.sig.asyncness.as_ref() {
        return Err(syn::Error::new_spanned(
            asyncness,
            "#[agentic_tool] does not support async fn in the MVP",
        ));
    }
    if !function.sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &function.sig.generics,
            "#[agentic_tool] does not support generic functions in the MVP",
        ));
    }
    if function.sig.inputs.len() != 2 {
        return Err(syn::Error::new_spanned(
            &function.sig.inputs,
            "#[agentic_tool] expects fn(input, ctx) with exactly two parameters",
        ));
    }

    extract_input_type(&function.sig.inputs[0])?;
    let second = extract_pat_type(&function.sig.inputs[1])?;
    ensure_tool_context_ref(&second.ty)?;
    extract_output_type(&function.sig.output)?;
    Ok(())
}

fn extract_input_type(arg: &FnArg) -> Result<&Type, syn::Error> {
    let typed = extract_pat_type(arg)?;
    if matches!(&*typed.ty, Type::Reference(_)) {
        return Err(syn::Error::new_spanned(
            &typed.ty,
            "the input parameter must be passed by value",
        ));
    }
    Ok(&typed.ty)
}

fn extract_output_type(return_type: &ReturnType) -> Result<&Type, syn::Error> {
    let ReturnType::Type(_, ty) = return_type else {
        return Err(syn::Error::new(
            Span::call_site(),
            "#[agentic_tool] expects a Result<Output, ToolError> return type",
        ));
    };
    let Type::Path(TypePath { path, .. }) = &**ty else {
        return Err(syn::Error::new_spanned(
            ty,
            "#[agentic_tool] expects a Result<Output, ToolError> return type",
        ));
    };
    let Some(segment) = path.segments.last() else {
        return Err(syn::Error::new_spanned(path, "unable to parse return type"));
    };
    if segment.ident != "Result" {
        return Err(syn::Error::new_spanned(
            path,
            "#[agentic_tool] expects a Result<Output, ToolError> return type",
        ));
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return Err(syn::Error::new_spanned(
            &segment.arguments,
            "Result must specify Output and ToolError types",
        ));
    };
    if args.args.len() != 2 {
        return Err(syn::Error::new_spanned(
            &args.args,
            "Result must specify Output and ToolError types",
        ));
    }

    let output_ty = match &args.args[0] {
        syn::GenericArgument::Type(ty) => ty,
        other => {
            return Err(syn::Error::new_spanned(
                other,
                "unsupported Result output type",
            ))
        }
    };
    let error_ty = match &args.args[1] {
        syn::GenericArgument::Type(ty) => ty,
        other => {
            return Err(syn::Error::new_spanned(
                other,
                "unsupported Result error type",
            ))
        }
    };

    if type_last_ident(error_ty)
        .map(|ident| ident != "ToolError")
        .unwrap_or(true)
    {
        return Err(syn::Error::new_spanned(
            error_ty,
            "#[agentic_tool] expects ToolError as the error type",
        ));
    }

    Ok(output_ty)
}

fn ensure_tool_context_ref(ty: &Type) -> Result<(), syn::Error> {
    let Type::Reference(TypeReference { elem, .. }) = ty else {
        return Err(syn::Error::new_spanned(
            ty,
            "the context parameter must be &ToolContext",
        ));
    };
    if type_last_ident(elem)
        .map(|ident| ident == "ToolContext")
        .unwrap_or(false)
    {
        Ok(())
    } else {
        Err(syn::Error::new_spanned(
            elem,
            "the context parameter must be &ToolContext",
        ))
    }
}

fn extract_pat_type(arg: &FnArg) -> Result<&PatType, syn::Error> {
    match arg {
        FnArg::Typed(typed) => Ok(typed),
        FnArg::Receiver(receiver) => Err(syn::Error::new_spanned(
            receiver,
            "#[agentic_tool] does not support methods",
        )),
    }
}

fn type_last_ident(ty: &Type) -> Option<&Ident> {
    let Type::Path(TypePath { path, .. }) = ty else {
        return None;
    };
    path.segments.last().map(|segment| &segment.ident)
}

fn parse_lit_str(expr: &Expr, label: &str) -> Result<LitStr, syn::Error> {
    let Expr::Lit(ExprLit {
        lit: Lit::Str(value),
        ..
    }) = expr
    else {
        return Err(syn::Error::new_spanned(
            expr,
            format!("{label} must be a string literal"),
        ));
    };
    Ok(value.clone())
}

fn parse_lit_bool(expr: &Expr, label: &str) -> Result<LitBool, syn::Error> {
    let Expr::Lit(ExprLit {
        lit: Lit::Bool(value),
        ..
    }) = expr
    else {
        return Err(syn::Error::new_spanned(
            expr,
            format!("{label} must be a boolean literal"),
        ));
    };
    Ok(value.clone())
}

fn parse_lit_str_array(expr: &Expr, label: &str) -> Result<Vec<LitStr>, syn::Error> {
    let Expr::Array(ExprArray { elems, .. }) = expr else {
        return Err(syn::Error::new_spanned(
            expr,
            format!("{label} must be an array literal"),
        ));
    };

    elems
        .iter()
        .map(|elem| parse_lit_str(elem, label))
        .collect()
}

fn parse_ident_array(expr: &Expr, label: &str) -> Result<Vec<Ident>, syn::Error> {
    let Expr::Array(ExprArray { elems, .. }) = expr else {
        return Err(syn::Error::new_spanned(
            expr,
            format!("{label} must be an array literal"),
        ));
    };

    elems
        .iter()
        .map(|elem| {
            let Expr::Path(ExprPath { path, .. }) = elem else {
                return Err(syn::Error::new_spanned(
                    elem,
                    format!("{label} entries must be bare identifiers"),
                ));
            };
            path.get_ident().cloned().ok_or_else(|| {
                syn::Error::new_spanned(path, format!("{label} entries must be bare identifiers"))
            })
        })
        .collect()
}

fn parse_type(expr: &Expr, label: &str) -> Result<Type, syn::Error> {
    syn::parse2::<Type>(expr.to_token_stream())
        .map_err(|_| syn::Error::new_spanned(expr, format!("{label} must be a valid Rust type")))
}

fn missing_option(name: &str) -> syn::Error {
    syn::Error::new(
        Span::call_site(),
        format!("missing required #[agentic_tool] option `{name}`"),
    )
}

fn validate_tool_name(name: &LitStr) -> Result<(), syn::Error> {
    let value = name.value();
    if value.is_empty() {
        return Err(syn::Error::new_spanned(name, "tool name cannot be empty"));
    }
    if value
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '_' | '-' | '.'))
    {
        Ok(())
    } else {
        Err(syn::Error::new_spanned(
            name,
            "tool name must use only a-z, 0-9, '_', '-', '.'",
        ))
    }
}

fn pascal_case(ident: &Ident) -> String {
    ident
        .to_string()
        .split('_')
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            let mut chars = segment.chars();
            match chars.next() {
                Some(first) => {
                    let mut out = String::new();
                    out.extend(first.to_uppercase());
                    out.push_str(chars.as_str());
                    out
                }
                None => String::new(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{pascal_case, validate_tool_name};
    use proc_macro2::Span;
    use syn::{Ident, LitStr};

    #[test]
    fn pascal_case_converts_snake_case_function_names() {
        assert_eq!(
            pascal_case(&Ident::new("read_file", Span::call_site())),
            "ReadFile"
        );
        assert_eq!(pascal_case(&Ident::new("calc", Span::call_site())), "Calc");
    }

    #[test]
    fn validate_tool_name_accepts_canonical_names() {
        validate_tool_name(&LitStr::new("read_file", Span::call_site())).expect("valid");
        validate_tool_name(&LitStr::new("tool.v2", Span::call_site())).expect("valid");
    }

    #[test]
    fn validate_tool_name_rejects_invalid_names() {
        assert!(validate_tool_name(&LitStr::new("ReadFile", Span::call_site())).is_err());
        assert!(validate_tool_name(&LitStr::new("read file", Span::call_site())).is_err());
    }
}
