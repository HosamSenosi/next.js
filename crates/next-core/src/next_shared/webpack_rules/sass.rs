use std::mem::take;

use anyhow::{Result, bail};
use serde_json::Value as JsonValue;
use turbo_rcstr::rcstr;
use turbo_tasks::{ResolvedVc, Vc};
use turbopack::module_options::{LoaderRuleItem, WebpackRules};
use turbopack_node::transforms::webpack::WebpackLoaderItem;

#[turbo_tasks::function]
pub async fn add_sass_loader(
    sass_options: Vc<JsonValue>,
    webpack_rules: Vc<WebpackRules>,
) -> Result<Vc<WebpackRules>> {
    let sass_options = sass_options.await?;
    let Some(mut sass_options) = sass_options.as_object().cloned() else {
        bail!("sass_options must be an object");
    };

    // TODO: Remove this once we upgrade to sass-loader 16
    let silence_deprecations = if let Some(v) = sass_options.get("silenceDeprecations") {
        v.clone()
    } else {
        serde_json::json!(["legacy-js-api"])
    };

    sass_options.insert("silenceDeprecations".into(), silence_deprecations);

    // additionalData is a loader option but Next.js has it under `sassOptions` in
    // `next.config.js`
    let additional_data = sass_options
        .get("prependData")
        .or(sass_options.get("additionalData"));
    let sass_loader = WebpackLoaderItem {
        loader: rcstr!("next/dist/compiled/sass-loader"),
        options: take(
            serde_json::json!({
                "implementation": sass_options.get("implementation"),
                "sourceMap": true,
                "sassOptions": sass_options,
                "additionalData": additional_data
            })
            .as_object_mut()
            .unwrap(),
        ),
    };
    let resolve_url_loader = WebpackLoaderItem {
        loader: rcstr!("next/dist/build/webpack/loaders/resolve-url-loader/index"),
        options: take(
            serde_json::json!({
                // https://github.com/vercel/turbo/blob/d527eb54be384a4658243304cecd547d09c05c6b/crates/turbopack-node/src/transforms/webpack.rs#L191
                "sourceMap": true
            })
            .as_object_mut()
            .unwrap(),
        ),
    };

    let loaders = ResolvedVc::cell(vec![resolve_url_loader, sass_loader]);

    let mut rules = webpack_rules.owned().await?;

    for (pattern, rename) in [
        (rcstr!("*.module.s[ac]ss"), rcstr!("*.module.css")),
        (rcstr!("*.s[ac]ss"), rcstr!("*.css")),
    ] {
        rules.push((
            pattern,
            LoaderRuleItem {
                loaders,
                rename_as: Some(rename),
                condition: None,
            },
        ));
    }

    Ok(Vc::cell(rules))
}
